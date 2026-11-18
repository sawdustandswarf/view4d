//! A model: parts read from a file, followed as the file is rewritten,
//! with their feature edges, or every edge as a wireframe (`w`). A part can
//! be selected (a click, `[` and `]`), hidden (`h`), shown alone (`H`), all
//! shown again (`a`); the print bed is outlined under it (`b`).
//!
//! Hiding and the selection go by a part's name, so they last across a
//! reload of the file.
//!
//! A `.prim` file's value can sweep from one number to another and back
//! (`--animate`): the parts are placed again each frame, and built again
//! only when the value changes their shape.

use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::time::Instant;

use view4d_model::Part;
use view4d_scene::edges::{FEATURE_ANGLE, all_edges, feature_edges};
use view4d_scene::{Aabb, Lines, Mesh, Scene, Vec3, ray_hits};

use crate::watch::Watch;

/// The colour edges are drawn in over faces.
pub const EDGE_COLOR: [u8; 3] = [40, 44, 56];
/// The colour of a wireframe's lines, on the dark background.
pub const WIRE_COLOR: [u8; 3] = [170, 180, 200];
/// The selected part's edges.
pub const SELECTED_COLOR: [u8; 3] = [255, 196, 64];
/// The bed's outline.
pub const BED_COLOR: [u8; 3] = [90, 110, 140];

/// Which parts to show, and the bed, as asked on the command line.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Show {
    /// Show only these parts, by name (empty: all).
    pub only: Vec<String>,
    /// Hide these.
    pub hide: Vec<String>,
    /// A bed of this width and depth in mm, centred under the model,
    /// instead of the one the file gives.
    pub bed: Option<(f32, f32)>,
    /// A `.prim` file's values, name → expression, over its own.
    pub values: Vec<(String, String)>,
    /// A value swept over time.
    pub sweep: Option<Sweep>,
}

/// A `.prim` file's value going from `from` to `to` and back, eased, once
/// every `seconds`.
#[derive(Debug, Clone, PartialEq)]
pub struct Sweep {
    pub name: String,
    pub from: f64,
    pub to: f64,
    pub seconds: f64,
}

impl Sweep {
    /// `NAME=FROM:TO[:SECONDS]`, four seconds when not given.
    pub fn parse(s: &str) -> Option<Sweep> {
        let (name, range) = s.split_once('=')?;
        let n: Vec<f64> = range.split(':').map(|v| v.trim().parse().ok()).collect::<Option<_>>()?;
        let (from, to, seconds) = match n[..] {
            [from, to] => (from, to, 4.0),
            [from, to, seconds] => (from, to, seconds),
            _ => return None,
        };
        let ok = !name.trim().is_empty() && from.is_finite() && to.is_finite() && seconds > 0.0 && seconds.is_finite();
        ok.then(|| Sweep { name: name.trim().to_string(), from, to, seconds })
    }

    /// The value `t` seconds in: `from` at 0, `to` half way.
    pub fn at(&self, t: f64) -> f64 {
        let turn = (t / self.seconds).fract();
        self.from + (self.to - self.from) * (1.0 - (turn * std::f64::consts::TAU).cos()) / 2.0
    }
}

pub struct Model {
    /// What it is called in the overlay: the file's name.
    name: String,
    /// The file, and its watch; none for a model made in memory.
    file: Option<(PathBuf, Watch)>,
    /// The values a `.prim` file is built at, each time it is read.
    values: Vec<(String, String)>,
    /// A value swept, and when the sweep began.
    sweep: Option<(Sweep, Instant)>,
    /// What the parts cover over the sweep, so the camera fits all of it.
    swept_bounds: Aabb,
    parts: Vec<Part>,
    /// The part names, each once, in the order they first come.
    names: Vec<String>,
    bounds: Aabb,
    /// Each part's feature edges, drawn over its faces.
    edges: Vec<Vec<[Vec3; 2]>>,
    /// Each part's every edge; made the first time they are asked for.
    wire: Option<Vec<Vec<[Vec3; 2]>>>,
    wireframe: bool,
    hidden: HashSet<String>,
    selected: Option<String>,
    /// The bed's width and depth, if one is asked for.
    bed_size: Option<(f32, f32)>,
    show_bed: bool,
    /// What is drawn, made again when any of the above changes.
    meshes: Vec<Mesh>,
    lines: Vec<Lines>,
    /// Why the file could not be read again, until it can.
    error: Option<String>,
}

impl Model {
    /// The model in `path`, to be followed, showing what `show` asks.
    pub fn open(path: &Path, show: &Show) -> Result<Model, String> {
        let watch = Watch::new(path.to_path_buf());
        let mut values: Vec<_> =
            show.values.iter().filter(|(n, _)| show.sweep.as_ref().is_none_or(|s| *n != s.name)).cloned().collect();
        let parts = match &show.sweep {
            Some(s) => view4d_model::read_at(path, &swept(&values, s, s.from))?,
            None => view4d_model::read_at(path, &values)?,
        };
        let name = path.file_name().map_or_else(|| path.display().to_string(), |n| n.to_string_lossy().into_owned());
        let mut m = Model::new(name, parts);
        m.show(show)?;
        m.file = Some((path.to_path_buf(), watch));
        if let Some(s) = &show.sweep {
            // Its ends and some way between: parts read again are not built again.
            for i in 1..=8 {
                let parts = view4d_model::read_at(path, &swept(&values, s, s.at(s.seconds * i as f64 / 16.0)))?;
                m.swept_bounds = parts.iter().fold(m.swept_bounds, |b, p| b.union(p.mesh.bounds()));
            }
        }
        m.sweep = show.sweep.clone().map(|s| (s, Instant::now()));
        m.values = std::mem::take(&mut values);
        Ok(m)
    }

    /// A model of `parts`, called `name`.
    pub fn new(name: String, parts: Vec<Part>) -> Model {
        let mut m = Model {
            name,
            file: None,
            values: Vec::new(),
            sweep: None,
            swept_bounds: Aabb::EMPTY,
            parts: Vec::new(),
            names: Vec::new(),
            bounds: Aabb::EMPTY,
            edges: Vec::new(),
            wire: None,
            wireframe: false,
            hidden: HashSet::new(),
            selected: None,
            bed_size: None,
            show_bed: true,
            meshes: Vec::new(),
            lines: Vec::new(),
            error: None,
        };
        m.replace(parts);
        m
    }

    /// Show and hide parts, and size the bed, as `show` asks; an error
    /// for a part there is none of.
    pub fn show(&mut self, show: &Show) -> Result<(), String> {
        for n in show.only.iter().chain(&show.hide) {
            if !self.names.contains(n) {
                return Err(format!("{}: no part called `{n}`; the parts: {}", self.name, self.names.join(", ")));
            }
        }
        if !show.only.is_empty() {
            self.hidden = self.names.iter().filter(|n| !show.only.contains(n)).cloned().collect();
        }
        self.hidden.extend(show.hide.iter().cloned());
        self.bed_size = show.bed;
        self.refresh();
        Ok(())
    }

    /// Show `parts` instead (the file changed).
    pub fn replace(&mut self, parts: Vec<Part>) {
        self.edges = parts
            .iter()
            .map(|p| p.edges.clone().unwrap_or_else(|| feature_edges(&p.mesh, FEATURE_ANGLE, EDGE_COLOR).segments))
            .collect();
        self.wire = None;
        self.bounds = parts.iter().fold(Aabb::EMPTY, |b, p| b.union(p.mesh.bounds()));
        self.names.clear();
        for p in &parts {
            if !self.names.contains(&p.name) {
                self.names.push(p.name.clone());
            }
        }
        self.parts = parts;
        self.error = None;
        self.refresh();
    }

    fn visible(&self, p: &Part) -> bool {
        !self.hidden.contains(&p.name)
    }

    /// The bed's outline, if one is asked for: centred under the model.
    fn bed(&self) -> Option<Vec<Vec3>> {
        let (w, d) = self.bed_size?;
        let (c, z) = if self.bounds.is_empty() { (Vec3::ZERO, 0.0) } else { (self.bounds.center(), self.bounds.min.z) };
        let (x0, y0) = (c.x - w / 2.0, c.y - d / 2.0);
        let corners = [[0.0, 0.0], [w, 0.0], [w, d], [0.0, d]];
        Some(corners.iter().map(|[x, y]| Vec3::new(x0 + x, y0 + y, z)).collect())
    }

    /// Make what is drawn again.
    fn refresh(&mut self) {
        if self.wireframe && self.wire.is_none() {
            self.wire = Some(self.parts.iter().map(|p| all_edges(&p.mesh, WIRE_COLOR).segments).collect());
        }
        let (each, color) = match &self.wire {
            Some(w) if self.wireframe => (w, WIRE_COLOR),
            _ => (&self.edges, EDGE_COLOR),
        };
        let (mut plain, mut selected) = (Vec::new(), Vec::new());
        for (p, segments) in self.parts.iter().zip(each) {
            if self.visible(p) {
                let to = if self.selected.as_ref() == Some(&p.name) { &mut selected } else { &mut plain };
                to.extend_from_slice(segments);
            }
        }
        self.lines = vec![Lines { segments: plain, color }, Lines { segments: selected, color: SELECTED_COLOR }];
        if let Some(bed) = self.bed().filter(|_| self.show_bed) {
            let segments = (0..bed.len()).map(|i| [bed[i], bed[(i + 1) % bed.len()]]).collect();
            self.lines.push(Lines { segments, color: BED_COLOR });
        }
        self.meshes = match self.wireframe {
            true => Vec::new(),
            false => self.parts.iter().filter(|p| self.visible(p)).map(|p| p.mesh.clone()).collect(),
        };
    }

    /// Select the part `step` names along from the selected one, round.
    fn step(&mut self, step: isize) {
        let n = self.names.len() as isize;
        if n == 0 {
            return;
        }
        let at = self.selected.as_ref().and_then(|s| self.names.iter().position(|n| n == s));
        let i = match at {
            Some(i) => (i as isize + step).rem_euclid(n),
            None if step > 0 => 0,
            None => n - 1,
        };
        self.selected = Some(self.names[i as usize].clone());
    }
}

/// `values` with the swept one at `v`.
fn swept(values: &[(String, String)], s: &Sweep, v: f64) -> Vec<(String, String)> {
    let mut out = values.to_vec();
    out.push((s.name.clone(), format!("{v:?}")));
    out
}

impl Scene for Model {
    fn meshes(&self) -> &[Mesh] {
        &self.meshes
    }

    fn lines(&self) -> &[Lines] {
        &self.lines
    }

    fn bounds(&self) -> Aabb {
        self.bounds.union(self.swept_bounds)
    }

    /// Name, size in mm, triangles, parts if more than one, the selected
    /// part, and how many are hidden.
    fn status(&self) -> String {
        let s = self.bounds.size();
        let n: usize = self.parts.iter().map(|p| p.mesh.triangles.len()).sum();
        if n == 0 {
            return format!("{}  empty", self.name);
        }
        let plural = if n == 1 { "" } else { "s" };
        let mut status = format!("{}  {:.1} x {:.1} x {:.1} mm  {n} triangle{plural}", self.name, s.x, s.y, s.z);
        if self.names.len() > 1 {
            status += &format!("  {} parts", self.names.len());
        }
        if let Some(sel) = &self.selected {
            status += &format!("  > {sel}{}", if self.hidden.contains(sel) { " (hidden)" } else { "" });
        }
        if !self.hidden.is_empty() {
            status += &format!("  {} hidden", self.hidden.len());
        }
        status
    }

    fn error(&self) -> Option<&str> {
        self.error.as_deref()
    }

    /// `w` the wireframe; `]` and `[` select the next and previous part,
    /// `h` hides or shows it, `H` shows it alone (again: all), `a` shows
    /// all; `b` the bed.
    fn key(&mut self, key: char) -> bool {
        match key {
            'w' => self.wireframe = !self.wireframe,
            ']' => self.step(1),
            '[' => self.step(-1),
            'h' | 'H' if self.selected.is_none() => return false,
            'h' => {
                let s = self.selected.clone().expect("selected");
                if !self.hidden.remove(&s) {
                    self.hidden.insert(s);
                }
            }
            'H' => {
                let s = self.selected.clone().expect("selected");
                let others: HashSet<String> = self.names.iter().filter(|n| **n != s).cloned().collect();
                self.hidden = if self.hidden == others { HashSet::new() } else { others };
            }
            'a' if !self.hidden.is_empty() => self.hidden.clear(),
            'b' if self.bed().is_some() => self.show_bed = !self.show_bed,
            _ => return false,
        }
        self.refresh();
        true
    }

    /// Select the shown part the ray meets first; nothing if none.
    fn pick(&mut self, from: Vec3, dir: Vec3) -> bool {
        let hit = self
            .parts
            .iter()
            .filter(|p| self.visible(p))
            .flat_map(|p| p.mesh.triangles.iter().filter_map(move |&t| Some((ray_hits(t, from, dir)?, p))))
            .min_by(|a, b| a.0.total_cmp(&b.0))
            .map(|(_, p)| p.name.clone());
        if hit == self.selected {
            return false;
        }
        self.selected = hit;
        self.refresh();
        true
    }

    fn animates(&self) -> bool {
        self.sweep.is_some()
    }

    /// Read the file again once it has changed and settled, or each frame
    /// with the swept value where it is now; if it can't be, keep the last
    /// parts and say why.
    fn poll(&mut self) -> bool {
        let Some((path, watch)) = &mut self.file else { return false };
        let values = match &self.sweep {
            // Read each frame: a file rewritten is read with it.
            Some((s, start)) => swept(&self.values, s, s.at(start.elapsed().as_secs_f64())),
            None if watch.changed() => self.values.clone(),
            None => return false,
        };
        match view4d_model::read_at(path, &values) {
            Ok(parts) => self.replace(parts),
            Err(e) => self.error = Some(e),
        }
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn v(x: f32, y: f32, z: f32) -> Vec3 {
        Vec3::new(x, y, z)
    }

    fn part(name: &str, z: f32) -> Part {
        Part {
            name: name.into(),
            mesh: Mesh::from_triangles(vec![[v(0., 0., z), v(1., 0., z), v(0., 1., z + 1.)]]),
            edges: None,
        }
    }

    fn tri() -> Vec<Part> {
        vec![part("tri", 0.0)]
    }

    fn colors(m: &Model) -> Vec<([u8; 3], usize)> {
        m.lines().iter().map(|l| (l.color, l.segments.len())).collect()
    }

    #[test]
    fn status_and_wireframe() {
        let mut m = Model::new("tri.stl".into(), tri());
        assert_eq!(m.status(), "tri.stl  1.0 x 1.0 x 1.0 mm  1 triangle");
        assert_eq!(m.meshes().len(), 1);
        assert_eq!(colors(&m)[0].0, EDGE_COLOR);
        assert!(m.key('w'));
        assert_eq!((m.meshes().len(), colors(&m)[0]), (0, (WIRE_COLOR, 3)));
        m.replace(Vec::new());
        assert_eq!((m.meshes().len(), colors(&m)[0]), (0, (WIRE_COLOR, 0)), "still a wireframe");
        assert_eq!(m.status(), "tri.stl  empty");
        assert!(m.key('w'));
        assert_eq!(m.meshes().len(), 0, "no parts");
        m.replace(tri());
        assert_eq!(m.meshes().len(), 1);
        assert!(!m.key('x'));
    }

    #[test]
    fn selecting_and_hiding() {
        let three = || vec![part("base", 0.0), part("lid", 2.0), part("base", 4.0), part("pin", 6.0)];
        let mut m = Model::new("case.3mf".into(), three());
        assert!(!m.key('h') && !m.key('H') && !m.key('a'), "nothing selected, nothing hidden");
        assert!(m.key(']'));
        assert_eq!(m.status(), "case.3mf  1.0 x 1.0 x 7.0 mm  4 triangles  3 parts  > base");
        assert_eq!(colors(&m), [(EDGE_COLOR, 6), (SELECTED_COLOR, 6)], "both of base's triangles");
        assert!(m.key('h'));
        assert_eq!(m.meshes().len(), 2);
        assert!(m.status().ends_with("> base (hidden)  1 hidden"));
        assert!(m.key('[') && m.key('['));
        assert!(m.status().contains("> lid"), "round from the first to the last, and back");
        assert!(m.key('H'));
        assert_eq!(m.meshes().len(), 1);
        assert!(m.status().ends_with("> lid  2 hidden"));
        m.replace(three());
        assert_eq!(m.meshes().len(), 1, "hidden by name, across a reload");
        assert!(m.key('H'));
        assert_eq!(m.meshes().len(), 4, "H again shows all");
        assert!(m.key('h') && m.key('a'));
        assert_eq!(m.meshes().len(), 4);
    }

    #[test]
    fn picking() {
        // Two flat squares, one over the other, seen from above.
        let square = |name: &str, z: f32| Part {
            name: name.into(),
            mesh: Mesh::from_triangles(vec![
                [v(0., 0., z), v(2., 0., z), v(2., 2., z)],
                [v(0., 0., z), v(2., 2., z), v(0., 2., z)],
            ]),
            edges: None,
        };
        let mut m = Model::new("two".into(), vec![square("low", 0.0), square("high", 1.0)]);
        let down = v(0., 0., -1.);
        assert!(m.pick(v(0.5, 1.5, 5.0), down));
        assert_eq!(m.selected.as_deref(), Some("high"), "the nearer");
        assert!(!m.pick(v(0.5, 1.5, 5.0), down), "no change");
        m.hidden.insert("high".into());
        assert!(m.pick(v(0.5, 1.5, 5.0), down));
        assert_eq!(m.selected.as_deref(), Some("low"), "not a hidden one");
        assert!(m.pick(v(5.0, 5.0, 5.0), down));
        assert_eq!(m.selected, None, "a miss selects nothing");
    }

    #[test]
    fn sweeps() {
        let s = Sweep::parse("open_angle=0:120").unwrap();
        assert_eq!((s.name.as_str(), s.from, s.to, s.seconds), ("open_angle", 0.0, 120.0, 4.0));
        assert_eq!(Sweep::parse("a=10:20:2").unwrap().seconds, 2.0);
        for bad in ["a", "=1:2", "a=1", "a=1:x", "a=1:2:0", "a=1:2:3:4"] {
            assert!(Sweep::parse(bad).is_none(), "{bad}");
        }
        assert!(s.at(0.0).abs() < 1e-9 && (s.at(2.0) - 120.0).abs() < 1e-9 && s.at(4.0).abs() < 1e-9);
        assert!((s.at(1.0) - 60.0).abs() < 1e-9 && (s.at(3.0) - 60.0).abs() < 1e-9);
    }

    #[test]
    fn only_hide_and_the_bed() {
        let mut m = Model::new("m".into(), vec![part("a", 0.0), part("b", 2.0), part("c", 4.0)]);
        assert_eq!(m.lines().len(), 2, "no bed");
        assert!(!m.key('b'));
        let only = Show {
            only: vec!["a".into(), "b".into()],
            hide: vec!["b".into()],
            bed: Some((10.0, 20.0)),
            ..Show::default()
        };
        m.show(&only).unwrap();
        assert_eq!(m.meshes().len(), 1);
        let bed = &m.lines()[2];
        assert_eq!((bed.color, bed.segments.len()), (BED_COLOR, 4));
        assert_eq!(bed.segments[0], [v(-4.5, -9.5, 0.0), v(5.5, -9.5, 0.0)], "centred under, at the bottom");
        assert!(m.key('b'));
        assert_eq!(m.lines().len(), 2);
        let e = m.show(&Show { hide: vec!["d".into()], ..Show::default() }).unwrap_err();
        assert_eq!(e, "m: no part called `d`; the parts: a, b, c");
    }

    #[test]
    fn a_file_followed() {
        let path = std::env::temp_dir().join(format!("view4d-model-{}.stl", std::process::id()));
        let ascii = |z: f32| {
            format!(
                "solid t\nfacet normal 0 0 1\nouter loop\nvertex 0 0 0\nvertex 1 0 0\nvertex 0 1 {z}\nendloop\nendfacet\nendsolid t\n"
            )
        };
        std::fs::write(&path, ascii(1.0)).unwrap();
        let mut m = Model::open(&path, &Show::default()).unwrap();
        assert!(!m.poll());
        std::fs::write(&path, ascii(2.5)).unwrap();
        assert!(!m.poll() && m.poll(), "read again once settled");
        assert_eq!(m.bounds().size().z, 2.5);
        std::fs::write(&path, "solid t\nfacet normal").unwrap();
        assert!(!m.poll() && m.poll());
        assert!(m.error().is_some_and(|e| e.contains("view4d-model")), "{:?}", m.error());
        assert_eq!(m.bounds().size().z, 2.5, "the last good mesh stays");
        std::fs::remove_file(&path).unwrap();
        assert!(Model::open(&path, &Show::default()).is_err());
    }

    #[test]
    fn an_obj_with_its_materials() {
        let dir = std::env::temp_dir().join(format!("view4d-obj-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("c.mtl"), "newmtl lid\nKd 0.2 0.4 0.6\n").unwrap();
        let obj = "mtllib c.mtl\nv 0 0 0\nv 4 0 0\nv 4 3 0\nv 0 3 2\n\
                   o base\nf 1 2 3\no lid\nusemtl lid\nf 1 3 4\n";
        std::fs::write(dir.join("case.OBJ"), obj).unwrap();
        let m = Model::open(&dir.join("case.OBJ"), &Show::default()).unwrap();
        assert_eq!(m.status(), "case.OBJ  4.0 x 3.0 x 2.0 mm  2 triangles  2 parts");
        let colors: Vec<_> = m.meshes().iter().map(|m| m.color).collect();
        assert_eq!(colors, [Mesh::DEFAULT_COLOR, [51, 102, 153]]);
        std::fs::remove_dir_all(&dir).unwrap();
    }
}
