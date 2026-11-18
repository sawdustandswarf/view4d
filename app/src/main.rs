//! `view4d`: 3D scenes that change over time, in a pane of termmd.
//!
//! `view4d FILE.stl|.obj|.3mf|.prim [--place KEYS] [--focus=no] [--view front|top|right|iso]`
//! shows it in termmd; `view4d --orrery` the planets now; `--png OUT.png
//! [--size WxH]` draws a picture instead.

mod gpu;
mod model;
mod show;
mod viewer;
mod watch;

use std::path::PathBuf;
use std::process::ExitCode;

use view4d_orrery::Orrery;
use view4d_scene::{Frame, Scene, View};
use viewer::Viewer;

type Result<T> = std::result::Result<T, Box<dyn std::error::Error>>;

/// Printed when view4d starts, and alone by `--version`.
const BANNER: &str =
    concat!("view4d v", env!("CARGO_PKG_VERSION"), "\nCopyright © 2026\nRichard Binet\nsawdust and swarf");

const USAGE: &str = "\
usage: view4d FILE.stl|.obj|.3mf|.prim [--place KEYS] [--focus=no] [--view front|top|right|iso]
              [--only PART]... [--hide PART]... [--bed WxD] [--set NAME=VALUE]...
              [--animate NAME=FROM:TO[:SECONDS]]
       view4d --orrery [--place KEYS] [--focus=no] [--view ...] [--idle] [--speed N]
       (either: [--orbit SECONDS], the eye going round once in SECONDS until a drag)
       view4d FILE|--orrery --png OUT.png [--size WxH] [--view ...]
       view4d --version

In termmd: drag to orbit, right/middle drag or Ctrl+drag to pan, the wheel
zooms; f fits, r resets, 1-4 front/top/right/iso, o the overlay, q quits.
A model follows its file (rewrite it and it is read again); w wireframe.
Click a part, or ] and [, to select it; h hides it, H shows it alone, a all;
b the bed's outline (--bed WxD in mm: centred under the model).
A .prim file (primlib's) is built at its values, or as --set gives them;
--animate sweeps one from FROM to TO and back every SECONDS (default 4),
such as a joint's angle: hinged_lid.prim --animate open_angle=0:110.
The orrery shows the planets now, distances on a log scale, from above;
Space pauses, +/- faster and slower, ./, a day, >/< a year, n now,
d the distance scale, l the names. --speed N: start N steps faster, as
pressing + N times (1 an hour a second ... 7 ten years a second). --idle: in the top right corner, small,
leaving the keyboard to the shell (--place and --focus still apply).
--place: termmd placement keys (default \"place=right w=50% h=50%\"),
e.g. \"fixed=top-right w=40 h=15\".";

#[derive(Debug)]
struct Options {
    /// The model's file; none for the orrery.
    file: Option<PathBuf>,
    png: Option<PathBuf>,
    size: (u32, u32),
    /// None: the scene's own (isometric for a model, top for the orrery).
    view: Option<View>,
    place: String,
    focus: bool,
    /// Which parts, and the bed.
    show: model::Show,
    /// The orrery's speed: steps up from real time, as `+` takes them.
    speed: usize,
    /// Seconds for the eye to go round once by itself.
    orbit: Option<f32>,
}

fn options(args: impl Iterator<Item = String>) -> Result<Options> {
    let (mut file, mut png, mut size, mut view, mut orrery) = (None, None, (800, 600), None, false);
    let (mut place, mut focus, mut idle, mut speed, mut orbit) = (None, None, false, 0, None);
    let mut show = model::Show::default();
    // `--name=value` as well as `--name value`.
    let mut args = args.flat_map(|a| match a.split_once('=') {
        Some((k, v)) if a.starts_with("--") => vec![k.to_string(), v.to_string()],
        _ => vec![a],
    });
    while let Some(a) = args.next() {
        let mut value = |name: &str| args.next().ok_or(format!("{name} needs a value"));
        match a.as_str() {
            "--png" => png = Some(PathBuf::from(value("--png")?)),
            "--size" => {
                let v = value("--size")?;
                let (w, h) = v.split_once('x').ok_or("--size is WxH")?;
                size = (w.parse()?, h.parse()?);
                if size.0 == 0 || size.1 == 0 || size.0 > 8192 || size.1 > 8192 {
                    return Err("--size: 1 to 8192 a side".into());
                }
            }
            "--view" => {
                view = Some(match value("--view")?.as_str() {
                    "front" => View::Front,
                    "top" => View::Top,
                    "right" => View::Right,
                    "iso" | "isometric" => View::Isometric,
                    v => return Err(format!("--view {v}: front, top, right or iso").into()),
                })
            }
            "--place" => place = Some(value("--place")?),
            "--only" => show.only.push(value("--only")?),
            "--hide" => show.hide.push(value("--hide")?),
            "--set" => {
                let v = value("--set")?;
                match v.split_once('=') {
                    Some((n, e)) if !n.trim().is_empty() && !e.trim().is_empty() => {
                        show.values.push((n.trim().to_string(), e.trim().to_string()))
                    }
                    _ => return Err(format!("--set {v}: NAME=VALUE, such as length=60").into()),
                }
            }
            "--bed" => {
                let v = value("--bed")?;
                let size = v.split_once('x').and_then(|(w, d)| Some((w.parse::<f32>().ok()?, d.parse::<f32>().ok()?)));
                match size {
                    Some((w, d)) if w > 0.0 && d > 0.0 && w.is_finite() && d.is_finite() => show.bed = Some((w, d)),
                    _ => return Err(format!("--bed {v}: WxD in mm, such as 256x256").into()),
                }
            }
            "--animate" => {
                let v = value("--animate")?;
                show.sweep = Some(model::Sweep::parse(&v).ok_or(format!("--animate {v}: NAME=FROM:TO[:SECONDS]"))?);
            }
            "--orbit" => {
                let v = value("--orbit")?;
                let s = v.parse::<f32>().ok().filter(|s| *s > 0.0 && s.is_finite());
                orbit = Some(s.ok_or(format!("--orbit {v}: seconds, over 0"))?);
            }
            "--idle" => idle = true,
            "--speed" => {
                let v = value("--speed")?;
                speed = v.parse().ok().filter(|n| *n <= 7).ok_or(format!("--speed {v}: 0 to 7"))?;
            }
            "--orrery" => orrery = true,
            "--focus" => {
                focus = Some(match value("--focus")?.as_str() {
                    "yes" => true,
                    "no" => false,
                    v => return Err(format!("--focus={v}: yes or no").into()),
                })
            }
            "-h" | "--help" => return Err(USAGE.into()),
            s if s.starts_with('-') => return Err(format!("unknown option {s}\n{USAGE}").into()),
            _ if file.is_some() => return Err(format!("one file at a time\n{USAGE}").into()),
            _ => file = Some(PathBuf::from(a)),
        }
    }
    match (&file, orrery) {
        (None, false) => return Err(USAGE.into()),
        (Some(_), true) => return Err(format!("a file or --orrery, not both\n{USAGE}").into()),
        (Some(_), false) if speed > 0 => return Err("--speed is for the orrery".into()),
        (None, true) if show != model::Show::default() => {
            return Err("--only, --hide, --bed, --set and --animate are for a model".into());
        }
        _ => {}
    }
    let place = place.unwrap_or_else(|| if idle { show::IDLE_PLACE } else { show::DEFAULT_PLACE }.to_string());
    let focus = focus.unwrap_or(!idle);
    Ok(Options { file, png, size, view, place, focus, show, speed, orbit })
}

fn run(o: Options) -> Result<()> {
    let (scene, view): (Box<dyn Scene>, _) = match &o.file {
        Some(f) => (Box::new(model::Model::open(f, &o.show)?), View::Isometric),
        None => {
            let mut orrery = Orrery::now();
            let now = std::time::Instant::now();
            for _ in 0..o.speed {
                orrery.key_at('+', now);
            }
            (Box::new(orrery), View::Top)
        }
    };
    let mut viewer = Viewer::new(scene, o.view.unwrap_or(view));
    if let Some(seconds) = o.orbit {
        viewer.orbit(seconds);
    }
    let Some(out) = o.png else {
        return show::show(viewer, &o.place, o.focus);
    };
    viewer.overlay = false;
    let mut frame = Frame::new(o.size.0, o.size.1);
    viewer.draw(&mut frame);
    std::fs::write(&out, view4d_scene::png::encode(&frame)).map_err(|e| format!("{}: {e}", out.display()))?;
    eprintln!("{}", viewer.scene.status());
    Ok(())
}

fn main() -> ExitCode {
    println!("{BANNER}");
    let args: Vec<String> = std::env::args().skip(1).collect();
    if args.iter().any(|a| a == "-v" || a == "--version") {
        return ExitCode::SUCCESS;
    }
    match options(args.into_iter()).and_then(run) {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("view4d: {e}");
            ExitCode::FAILURE
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn opts(a: &[&str]) -> Result<Options> {
        options(a.iter().map(|s| s.to_string()))
    }

    #[test]
    fn arguments() {
        let o = opts(&["part.stl", "--png", "out.png", "--size", "320x200", "--view", "top"]).unwrap();
        let file = o.file.as_deref().and_then(|p| p.to_str());
        assert_eq!((file, o.png.as_deref().and_then(|p| p.to_str())), (Some("part.stl"), Some("out.png")));
        assert_eq!((o.size, o.view), ((320, 200), Some(View::Top)));
        let o = opts(&["a.stl"]).unwrap();
        assert_eq!((o.view, o.place.as_str(), o.focus), (None, show::DEFAULT_PLACE, true));
        let o = opts(&["a.stl", "--focus=no", "--place=fixed=top-right w=40", "--view=front"]).unwrap();
        assert_eq!((o.focus, o.place.as_str(), o.view), (false, "fixed=top-right w=40", Some(View::Front)));
        assert_eq!(opts(&["--orrery"]).unwrap().file, None);
        assert!(opts(&["a.stl", "--orrery"]).is_err());
        assert_eq!(opts(&["--orrery", "--speed", "4"]).unwrap().speed, 4);
        assert_eq!(opts(&["a.stl", "--orbit", "8"]).unwrap().orbit, Some(8.0));
        assert!(opts(&["a.stl", "--orbit", "0"]).is_err());
        assert!(opts(&["--orrery", "--speed", "8"]).is_err());
        assert!(opts(&["a.stl", "--speed", "2"]).is_err());
        let o = opts(&["--orrery", "--idle"]).unwrap();
        assert_eq!((o.place.as_str(), o.focus), (show::IDLE_PLACE, false));
        let o = opts(&["--orrery", "--idle", "--focus=yes", "--place", "fixed=top-left w=20"]).unwrap();
        assert_eq!((o.place.as_str(), o.focus), ("fixed=top-left w=20", true), "--idle only sets defaults");
        assert!(opts(&[]).is_err());
        assert!(opts(&["a.stl", "b.stl"]).is_err());
        assert!(opts(&["a.stl", "--size", "0x10"]).is_err());
        assert!(opts(&["a.stl", "--view", "back"]).is_err());
        assert!(opts(&["a.stl", "--png"]).is_err());
        assert!(opts(&["a.stl", "--focus=maybe"]).is_err());
        let o = opts(&["a.3mf", "--only", "lid", "--only=base", "--hide", "pin", "--bed", "350x320"]).unwrap();
        let show = model::Show {
            only: vec!["lid".into(), "base".into()],
            hide: vec!["pin".into()],
            bed: Some((350.0, 320.0)),
            ..model::Show::default()
        };
        assert_eq!(o.show, show);
        assert!(opts(&["a.stl", "--bed", "350"]).is_err());
        let o = opts(&["h.prim", "--set", "length=60", "--set=rings = 5"]).unwrap();
        assert_eq!(o.show.values, [("length".to_string(), "60".to_string()), ("rings".to_string(), "5".to_string())]);
        assert!(opts(&["h.prim", "--set", "length"]).is_err());
        assert!(opts(&["a.stl", "--bed", "0x10"]).is_err());
        assert!(opts(&["--orrery", "--hide", "Mars"]).is_err());
    }
}
