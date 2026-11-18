//! The scene viewer: a scene, the camera looking at it, and what keys and
//! the mouse do to them. Drawing goes into a [`Frame`]; where the frame goes
//! (a PNG, a termmd surface) is someone else's business.

use std::f32::consts::TAU;
use std::time::Instant;

use termmd_surface::Input;
use termmd_surface::input::{Key, KeyEventType, MouseAction, MouseButton};
use view4d_scene::{Camera, Frame, Scene, Style, View, text};

/// The overlay's text, and the scene's error.
const OVERLAY_COLOR: [u8; 3] = [200, 205, 215];
const ERROR_COLOR: [u8; 3] = [240, 110, 100];
/// Space between the overlay and the frame's edge, in font pixels.
const OVERLAY_MARGIN: u32 = 2;

/// Font pixels to a screen pixel for text over `frame`: bigger in bigger
/// frames.
fn overlay_scale(frame: &Frame) -> u32 {
    (frame.height / 400).clamp(1, 3)
}

/// What an input asks of the loop.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Step {
    /// Nothing changed.
    Same,
    /// Draw again.
    Redraw,
    /// Stop.
    Quit,
}

/// A drag under way: how it moves the camera, and where the pointer was.
#[derive(Debug, Clone, Copy, PartialEq)]
struct Drag {
    pan: bool,
    at: (u32, u32),
    /// Where it started, and whether it has gone further than a click.
    from: (u32, u32),
    moved: bool,
}

/// How far the eye rises and falls while orbiting, radians either way.
const ORBIT_BOB: f32 = 0.25;

/// The eye going round the scene by itself (`--orbit`), until a drag.
#[derive(Debug, Clone, Copy, PartialEq)]
struct Orbit {
    /// Once round every this many seconds.
    seconds: f32,
    start: Instant,
    /// Where it was when it began.
    yaw: f32,
    pitch: f32,
}

/// How far, in pixels, a press may move and still be a click.
const CLICK_SLOP: u32 = 3;

pub struct Viewer {
    pub scene: Box<dyn Scene>,
    pub camera: Camera,
    view: View,
    /// The overlay: the scene's status, at the bottom left.
    pub overlay: bool,
    drag: Option<Drag>,
    orbit: Option<Orbit>,
}

impl Viewer {
    /// A viewer of `scene` from the standard view `view`, fitted.
    pub fn new(scene: Box<dyn Scene>, view: View) -> Viewer {
        let mut v = Viewer { scene, camera: Camera::default(), view, overlay: true, drag: None, orbit: None };
        v.reset();
        v
    }

    /// Back to the view it started with, fitted.
    pub fn reset(&mut self) {
        self.camera.view(self.view);
        self.camera.fit(self.scene.bounds());
    }

    /// Take the eye round the scene once every `seconds`, rising and
    /// falling a little, until the scene is dragged.
    pub fn orbit(&mut self, seconds: f32) {
        let (yaw, pitch) = (self.camera.yaw, self.camera.pitch);
        self.orbit = Some(Orbit { seconds, start: Instant::now(), yaw, pitch });
    }

    /// Whether anything moves by itself, so frames should keep coming.
    pub fn animates(&self) -> bool {
        self.orbit.is_some() || self.scene.animates()
    }

    /// Move on to now: the eye if it orbits, the scene if it animates.
    pub fn tick(&mut self) {
        if let Some(o) = self.orbit {
            let turn = (o.start.elapsed().as_secs_f32() / o.seconds).fract();
            self.camera.yaw = o.yaw + turn * TAU;
            self.camera.pitch = (o.pitch + ORBIT_BOB * (2.0 * turn * TAU).sin()).clamp(-1.5, 1.5);
        }
        if self.scene.animates() {
            self.scene.poll();
        }
    }

    /// Draw it into `frame`.
    pub fn draw(&self, frame: &mut Frame) {
        frame.render(&self.camera, self.scene.meshes(), self.scene.lines(), &Style::default());
        self.draw_labels(frame);
        if self.overlay {
            self.draw_overlay(frame);
        }
    }

    /// The scene's labels, each just right of and above its point, if
    /// that is in front of the camera.
    fn draw_labels(&self, frame: &mut Frame) {
        let scale = overlay_scale(frame);
        let (w, h) = (frame.width as f32, frame.height as f32);
        for l in self.scene.labels() {
            let c = self.camera.to_camera(l.at);
            if c.z <= self.camera.near() {
                continue;
            }
            let (x, y, _) = self.camera.project(c, w, h);
            let gap = 2 * scale as i32;
            let up = text::size(&l.text, scale).1 as i32;
            frame.text(x as i32 + gap, y as i32 - up - gap, scale, &l.text, l.color);
        }
    }

    /// The status, and any error under it, at the bottom left on a darker
    /// band; bigger in bigger frames.
    fn draw_overlay(&self, frame: &mut Frame) {
        let scale = overlay_scale(frame);
        let m = OVERLAY_MARGIN * scale;
        let cols = text::columns(frame.width.saturating_sub(4 * m), scale);
        let status = self.scene.status();
        let lines: Vec<(String, [u8; 3])> = [(Some(status.as_str()), OVERLAY_COLOR), (self.scene.error(), ERROR_COLOR)]
            .into_iter()
            .filter_map(|(t, rgb)| t.map(|t| text::wrap(t, cols).into_iter().map(move |l| (l, rgb))))
            .flatten()
            .collect();
        let all = lines.iter().map(|l| l.0.as_str()).collect::<Vec<_>>().join("\n");
        let (w, h) = text::size(&all, scale);
        let (x, y) = (2 * m as i32, frame.height as i32 - h as i32 - 2 * m as i32);
        frame.rect(x - m as i32, y - m as i32, w + 2 * m, h + 2 * m, Style::default().background, 170);
        for (i, (l, rgb)) in lines.iter().enumerate() {
            frame.text(x, y + (i as u32 * text::line_height(scale)) as i32, scale, l, *rgb);
        }
    }

    /// Apply `input`, in a view `size` pixels: a drag orbits (a right or
    /// middle drag, or Ctrl and a drag, pans), the wheel zooms, a click is
    /// the scene's to pick with; `f` fits, `r` resets, `1`–`4` are the
    /// front, top, right and isometric views, `o` the overlay, `q` or Escape
    /// quits; other keys are the scene's, as typed (Shift+`.` is `>`).
    pub fn input(&mut self, input: &Input, (width, height): (u32, u32)) -> Step {
        match input {
            Input::Key(k) if k.event != KeyEventType::Release => {
                let view = |v| Some(v);
                let typed = match (&k.key, k.text.as_deref().map(|t| (t.chars().next(), t.chars().count()))) {
                    (Key::Char(_), Some((Some(c), 1))) if !k.mods.ctrl && !k.mods.alt => Key::Char(c),
                    (key, _) => *key,
                };
                let to = match typed {
                    Key::Char('q') | Key::Escape => return Step::Quit,
                    Key::Char('f') => {
                        self.camera.fit(self.scene.bounds());
                        return Step::Redraw;
                    }
                    Key::Char('r') => {
                        self.reset();
                        return Step::Redraw;
                    }
                    Key::Char('o') => {
                        self.overlay = !self.overlay;
                        return Step::Redraw;
                    }
                    Key::Char('1') => view(View::Front),
                    Key::Char('2') => view(View::Top),
                    Key::Char('3') => view(View::Right),
                    Key::Char('4') => view(View::Isometric),
                    Key::Char(c) => {
                        // A scene whose extent changes (the orrery's scale) is fitted again.
                        let before = self.scene.bounds();
                        if !self.scene.key(c) {
                            return Step::Same;
                        }
                        if self.scene.bounds().size() != before.size() {
                            self.camera.fit(self.scene.bounds());
                        }
                        return Step::Redraw;
                    }
                    _ => None,
                };
                match to {
                    Some(v) => {
                        self.camera.view(v);
                        Step::Redraw
                    }
                    None => Step::Same,
                }
            }
            Input::Mouse(m) => match m.action {
                MouseAction::Press(b @ (MouseButton::Left | MouseButton::Middle | MouseButton::Right)) => {
                    let pan = b != MouseButton::Left || m.mods.ctrl;
                    self.drag = Some(Drag { pan, at: (m.x, m.y), from: (m.x, m.y), moved: false });
                    Step::Same
                }
                MouseAction::Release(_) => match self.drag.take() {
                    Some(Drag { pan: false, moved: false, .. }) => {
                        let (from, dir) = self.camera.ray(m.x as f32, m.y as f32, width as f32, height as f32);
                        if self.scene.pick(from, dir) { Step::Redraw } else { Step::Same }
                    }
                    _ => Step::Same,
                },
                MouseAction::Motion(Some(_)) => {
                    let Some(d) = self.drag.as_mut() else { return Step::Same };
                    let (dx, dy) = (m.x as f32 - d.at.0 as f32, m.y as f32 - d.at.1 as f32);
                    d.at = (m.x, m.y);
                    d.moved |= m.x.abs_diff(d.from.0).max(m.y.abs_diff(d.from.1)) > CLICK_SLOP;
                    if !d.moved {
                        return Step::Same;
                    }
                    self.orbit = None;
                    if d.pan {
                        self.camera.pan(dx, dy, height as f32);
                    } else {
                        self.camera.orbit(dx, dy);
                    }
                    if (dx, dy) == (0.0, 0.0) { Step::Same } else { Step::Redraw }
                }
                MouseAction::Wheel(_, dy) if dy != 0 => {
                    self.camera.zoom(dy as f32);
                    Step::Redraw
                }
                _ => Step::Same,
            },
            // Losing the focus mid-drag loses the release too.
            Input::Focus(false) => {
                self.drag = None;
                Step::Same
            }
            _ => Step::Same,
        }
    }
}

#[cfg(test)]
mod tests {
    use termmd_surface::input::{KeyEvent, Mods, MouseEvent};
    use view4d_scene::{Mesh, Vec3};

    use super::*;
    use crate::model::Model;

    fn viewer() -> Viewer {
        let v = |x, y, z| Vec3::new(x, y, z);
        let mesh = Mesh::from_triangles(vec![[v(0., 0., 0.), v(1., 0., 0.), v(0., 1., 1.)]]);
        let parts = vec![view4d_model::Part { name: "tri".into(), mesh, edges: None }];
        Viewer::new(Box::new(Model::new("tri.stl".into(), parts)), View::Isometric)
    }

    fn key(c: char) -> Input {
        Input::Key(KeyEvent { key: Key::Char(c), mods: Mods::NONE, event: KeyEventType::Press, text: None })
    }

    fn mouse(action: MouseAction, x: u32, y: u32, ctrl: bool) -> Input {
        Input::Mouse(MouseEvent { action, x, y, mods: Mods { ctrl, ..Mods::NONE } })
    }

    const SIZE: (u32, u32) = (200, 100);

    #[test]
    fn the_eye_orbits_until_a_drag() {
        let mut v = viewer();
        assert!(!v.animates());
        v.orbit(1.0);
        assert!(v.animates());
        let (yaw, pitch) = (v.camera.yaw, v.camera.pitch);
        v.orbit = v.orbit.map(|o| Orbit { start: Instant::now() - std::time::Duration::from_millis(250), ..o });
        v.tick();
        assert!((v.camera.yaw - (yaw + TAU / 4.0)).abs() < 0.05, "a quarter turn in a quarter of the time");
        assert!(v.camera.pitch.abs() < 1.5 && (v.camera.pitch - pitch).abs() < ORBIT_BOB + 0.01);
        v.input(&mouse(MouseAction::Press(MouseButton::Left), 10, 10, false), SIZE);
        v.input(&mouse(MouseAction::Motion(Some(MouseButton::Left)), 30, 10, false), SIZE);
        assert!(!v.animates(), "a drag stops it");
    }

    #[test]
    fn a_drag_orbits_and_a_right_drag_pans() {
        let mut v = viewer();
        let (yaw, target) = (v.camera.yaw, v.camera.target);
        assert_eq!(v.input(&mouse(MouseAction::Motion(Some(MouseButton::Left)), 5, 5, false), SIZE), Step::Same);
        v.input(&mouse(MouseAction::Press(MouseButton::Left), 10, 10, false), SIZE);
        let step = v.input(&mouse(MouseAction::Motion(Some(MouseButton::Left)), 30, 10, false), SIZE);
        assert_eq!(step, Step::Redraw);
        assert!(v.camera.yaw < yaw, "dragged right, the scene turns right");
        assert_eq!(v.camera.target, target, "orbiting keeps the target");
        v.input(&mouse(MouseAction::Release(MouseButton::Left), 30, 10, false), SIZE);
        assert_eq!(v.input(&mouse(MouseAction::Motion(None), 50, 10, false), SIZE), Step::Same);

        let yaw = v.camera.yaw;
        v.input(&mouse(MouseAction::Press(MouseButton::Right), 10, 10, false), SIZE);
        v.input(&mouse(MouseAction::Motion(Some(MouseButton::Right)), 30, 10, false), SIZE);
        assert_eq!(v.camera.yaw, yaw, "panning keeps the direction");
        assert_ne!(v.camera.target, target);

        let target = v.camera.target;
        v.input(&mouse(MouseAction::Press(MouseButton::Left), 10, 10, true), SIZE);
        v.input(&mouse(MouseAction::Motion(Some(MouseButton::Left)), 10, 40, true), SIZE);
        assert_ne!(v.camera.target, target, "Ctrl and a drag pans too");
    }

    #[test]
    fn keys_and_the_wheel() {
        let mut v = viewer();
        let d = v.camera.distance;
        v.input(&mouse(MouseAction::Wheel(0, -1), 0, 0, false), SIZE);
        assert!(v.camera.distance < d, "the wheel up comes closer");
        assert_eq!(v.input(&key('f'), SIZE), Step::Redraw);
        assert_eq!(v.camera.distance, d, "f fits again");

        v.input(&key('1'), SIZE);
        assert_eq!(v.camera.pitch, 0.0, "front");
        v.input(&key('r'), SIZE);
        let mut iso = Camera::default();
        iso.view(View::Isometric);
        assert_eq!((v.camera.yaw, v.camera.pitch), (iso.yaw, iso.pitch), "r: back to the start");

        assert_eq!(v.input(&key('w'), SIZE), Step::Redraw, "the scene's key");
        assert!(v.scene.meshes().is_empty(), "the model's wireframe");
        assert_eq!(v.input(&key('x'), SIZE), Step::Same);
        assert_eq!(v.input(&key('q'), SIZE), Step::Quit);
    }

    #[test]
    fn a_click_picks_and_keys_are_as_typed() {
        let mut v = viewer();
        // A click on the triangle's centroid, moving a little; a corner is empty.
        let c = v.camera.project(v.camera.to_camera(Vec3::new(1.0, 1.0, 1.0) / 3.0), 200.0, 100.0);
        let (x, y) = (c.0 as u32, c.1 as u32);
        v.input(&mouse(MouseAction::Press(MouseButton::Left), x, y, false), SIZE);
        v.input(&mouse(MouseAction::Motion(Some(MouseButton::Left)), x + 1, y + 1, false), SIZE);
        assert_eq!(v.input(&mouse(MouseAction::Release(MouseButton::Left), x + 1, y + 1, false), SIZE), Step::Redraw);
        assert!(v.scene.status().ends_with("> tri"), "{}", v.scene.status());
        let yaw = v.camera.yaw;
        v.input(&mouse(MouseAction::Press(MouseButton::Left), 2, 2, false), SIZE);
        v.input(&mouse(MouseAction::Motion(Some(MouseButton::Left)), 30, 2, false), SIZE);
        assert_eq!(v.input(&mouse(MouseAction::Release(MouseButton::Left), 30, 2, false), SIZE), Step::Same);
        assert!(v.camera.yaw != yaw && v.scene.status().ends_with("> tri"), "a drag is not a click");
        v.input(&mouse(MouseAction::Press(MouseButton::Left), 2, 2, false), SIZE);
        assert_eq!(v.input(&mouse(MouseAction::Release(MouseButton::Left), 2, 2, false), SIZE), Step::Redraw);
        assert!(!v.scene.status().contains('>'), "a click on nothing selects nothing");

        // Shift+h is H (the part alone), not h, by the text it typed.
        v.input(&key(']'), SIZE);
        let shift_h = KeyEvent {
            key: Key::Char('h'),
            mods: Mods { shift: true, ..Mods::NONE },
            event: KeyEventType::Press,
            text: Some("H".into()),
        };
        v.input(&Input::Key(shift_h), SIZE);
        assert_eq!(v.scene.meshes().len(), 1, "H shows tri alone; h would have hidden it");
        assert_eq!(v.input(&key('h'), SIZE), Step::Redraw);
        assert!(v.scene.meshes().is_empty());
    }

    #[test]
    fn the_overlay() {
        let mut v = viewer();
        let mut f = Frame::new(200, 60);
        v.draw(&mut f);
        let bright = |f: &Frame| {
            (0..60).flat_map(|y| (0..60).map(move |x| (x, y))).filter(|&(x, y)| f.rgb(x, y)[0] > 190).count()
        };
        let with = bright(&f);
        assert!(with > 0, "the overlay's text, bottom left");
        v.input(&key('o'), SIZE);
        v.draw(&mut f);
        assert!(bright(&f) < with, "o hides it");
    }
}
