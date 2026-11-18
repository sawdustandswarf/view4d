//! The camera: it orbits a target point, pans and zooms. Z is up, as in
//! FreeCAD; the front view looks along +Y.

use std::f32::consts::FRAC_PI_2;

use crate::math::{Aabb, Vec3};

/// The standard views, as FreeCAD names them.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum View {
    /// From −Y, looking along +Y.
    Front,
    /// From +Z, looking down.
    Top,
    /// From +X.
    Right,
    /// From (+1, −1, +1), FreeCAD's isometric.
    Isometric,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Camera {
    /// What it orbits and looks at.
    pub target: Vec3,
    /// About Z, radians; 0 sees the scene from +X.
    pub yaw: f32,
    /// Above the XY plane, radians, −π/2 to π/2.
    pub pitch: f32,
    /// From the target.
    pub distance: f32,
    /// Vertical field of view, radians.
    pub fov: f32,
}

/// Radians the camera turns per pixel dragged.
const ORBIT_PER_PIXEL: f32 = 0.01;
/// How much one wheel step brings the camera closer.
const ZOOM_STEP: f32 = 0.9;
/// Room around a fitted scene.
const FIT_MARGIN: f32 = 1.1;

impl Default for Camera {
    fn default() -> Camera {
        let mut c = Camera { target: Vec3::ZERO, yaw: 0.0, pitch: 0.0, distance: 10.0, fov: 30f32.to_radians() };
        c.view(View::Isometric);
        c
    }
}

impl Camera {
    /// Look from the standard view `v`, keeping target and distance.
    pub fn view(&mut self, v: View) {
        (self.yaw, self.pitch) = match v {
            View::Front => (-FRAC_PI_2, 0.0),
            View::Top => (-FRAC_PI_2, FRAC_PI_2),
            View::Right => (0.0, 0.0),
            View::Isometric => (-std::f32::consts::FRAC_PI_4, (1.0 / 2f32.sqrt()).atan()),
        };
    }

    /// Look at `b`'s centre from far enough away to see all of it.
    pub fn fit(&mut self, b: Aabb) {
        if b.is_empty() {
            return;
        }
        self.target = b.center();
        let radius = (b.size().length() / 2.0).max(1e-3);
        self.distance = radius / (self.fov / 2.0).sin() * FIT_MARGIN;
    }

    /// Turn about the target by a drag of `(dx, dy)` pixels: right turns
    /// the scene right, down tips it towards you.
    pub fn orbit(&mut self, dx: f32, dy: f32) {
        self.yaw -= dx * ORBIT_PER_PIXEL;
        self.pitch = (self.pitch + dy * ORBIT_PER_PIXEL).clamp(-FRAC_PI_2, FRAC_PI_2);
    }

    /// Move the target so the scene follows a drag of `(dx, dy)` pixels in
    /// a view `height` pixels tall.
    pub fn pan(&mut self, dx: f32, dy: f32, height: f32) {
        let per_pixel = 2.0 * self.distance * (self.fov / 2.0).tan() / height.max(1.0);
        let (right, up, _) = self.axes();
        self.target = self.target - right * (dx * per_pixel) + up * (dy * per_pixel);
    }

    /// `steps` wheel steps: positive away, negative closer.
    pub fn zoom(&mut self, steps: f32) {
        self.distance = (self.distance * ZOOM_STEP.powf(-steps)).max(1e-4);
    }

    /// Where it is.
    pub fn eye(&self) -> Vec3 {
        let (_, _, forward) = self.axes();
        self.target - forward * self.distance
    }

    /// Right, up and forward, unit length; defined straight above or below
    /// too, where up follows the yaw.
    pub fn axes(&self) -> (Vec3, Vec3, Vec3) {
        let (sy, cy) = self.yaw.sin_cos();
        let (sp, cp) = self.pitch.sin_cos();
        let forward = Vec3::new(-cp * cy, -cp * sy, -sp);
        let right = Vec3::new(-sy, cy, 0.0);
        (right, right.cross(forward), forward)
    }

    /// `p` in camera space: x right, y up, z the depth in front.
    pub fn to_camera(&self, p: Vec3) -> Vec3 {
        let (right, up, forward) = self.axes();
        let d = p - self.eye();
        Vec3::new(d.dot(right), d.dot(up), d.dot(forward))
    }

    /// Camera-space `c` (in front: `c.z > 0`) on a `w`×`h` screen: pixel x
    /// and y (y down), and the depth.
    pub fn project(&self, c: Vec3, w: f32, h: f32) -> (f32, f32, f32) {
        let focal = (h / 2.0) / (self.fov / 2.0).tan();
        (w / 2.0 + c.x / c.z * focal, h / 2.0 - c.y / c.z * focal, c.z)
    }

    /// The ray from the eye through pixel `(x, y)` of a `w`×`h` screen: where
    /// it starts and its unit direction. The inverse of [`Camera::project`].
    pub fn ray(&self, x: f32, y: f32, w: f32, h: f32) -> (Vec3, Vec3) {
        let focal = (h / 2.0) / (self.fov / 2.0).tan();
        let (right, up, forward) = self.axes();
        let dir = right * ((x - w / 2.0) / focal) + up * ((h / 2.0 - y) / focal) + forward;
        (self.eye(), dir.normalized())
    }

    /// Near enough the eye to clip at, for a scene of this size.
    pub fn near(&self) -> f32 {
        self.distance * 1e-3
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn close(a: Vec3, b: Vec3) -> bool {
        (a - b).length() < 1e-4
    }

    fn on_screen(c: &Camera, p: Vec3) -> (f32, f32) {
        let (x, y, _) = c.project(c.to_camera(p), 200.0, 100.0);
        (x, y)
    }

    #[test]
    fn standard_views() {
        let mut c = Camera { distance: 10.0, ..Camera::default() };
        c.view(View::Front);
        assert!(close(c.eye(), Vec3::new(0.0, -10.0, 0.0)));
        let (x, y) = on_screen(&c, Vec3::new(1.0, 0.0, 1.0));
        assert!(x > 100.0 && y < 50.0, "+X right, +Z up from the front: {x} {y}");
        assert_eq!(on_screen(&c, Vec3::ZERO), (100.0, 50.0));

        c.view(View::Top);
        assert!(close(c.eye(), Vec3::new(0.0, 0.0, 10.0)));
        let (x, y) = on_screen(&c, Vec3::new(1.0, 1.0, 0.0));
        assert!(x > 100.0 && y < 50.0, "+X right, +Y up from above: {x} {y}");

        c.view(View::Right);
        assert!(close(c.eye(), Vec3::new(10.0, 0.0, 0.0)));
        let (x, _) = on_screen(&c, Vec3::new(0.0, 1.0, 0.0));
        assert!(x > 100.0, "+Y to the right from the right");

        c.view(View::Isometric);
        let e = c.eye().normalized();
        let k = 1.0 / 3f32.sqrt();
        assert!(close(e, Vec3::new(k, -k, k)), "{e:?}");
    }

    #[test]
    fn axes_are_orthonormal_everywhere() {
        for (yaw, pitch) in [(0.0, 0.0), (1.0, 0.5), (-2.0, FRAC_PI_2), (3.0, -FRAC_PI_2)] {
            let c = Camera { yaw, pitch, ..Camera::default() };
            let (r, u, f) = c.axes();
            for v in [r, u, f] {
                assert!((v.length() - 1.0).abs() < 1e-5);
            }
            assert!(r.dot(u).abs() < 1e-5 && r.dot(f).abs() < 1e-5 && u.dot(f).abs() < 1e-5);
            assert!(close(r.cross(u), -f), "right-handed: x right, y up, looking along −z of r×u");
        }
    }

    #[test]
    fn fit_sees_the_whole_box() {
        let b = Aabb { min: Vec3::new(10.0, 20.0, 0.0), max: Vec3::new(110.0, 60.0, 30.0) };
        let mut c = Camera::default();
        c.fit(b);
        assert_eq!(c.target, b.center());
        for i in 0..8 {
            let corner = |bit: u32, lo: f32, hi: f32| if i & bit != 0 { hi } else { lo };
            let p = Vec3::new(corner(1, b.min.x, b.max.x), corner(2, b.min.y, b.max.y), corner(4, b.min.z, b.max.z));
            let (x, y) = on_screen(&c, p);
            assert!((0.0..=200.0).contains(&x) && (0.0..=100.0).contains(&y), "corner {p:?} at {x}, {y}");
        }
    }

    #[test]
    fn orbit_pan_zoom() {
        let mut c = Camera::default();
        c.view(View::Front);
        c.orbit(0.0, 1e6);
        assert_eq!(c.pitch, FRAC_PI_2, "pitch stops straight above");

        let mut c = Camera::default();
        c.view(View::Front);
        let before = on_screen(&c, Vec3::ZERO);
        c.pan(20.0, 10.0, 100.0);
        let after = on_screen(&c, Vec3::ZERO);
        assert!((after.0 - before.0 - 20.0).abs() < 1e-3 && (after.1 - before.1 - 10.0).abs() < 1e-3, "{after:?}");

        let d = c.distance;
        c.zoom(-1.0);
        assert!((c.distance - d * 0.9).abs() < 1e-4);
        c.zoom(1.0);
        assert!((c.distance - d).abs() < 1e-4);
    }

    #[test]
    fn a_ray_through_a_projected_point_meets_it() {
        let mut c = Camera::default();
        c.fit(crate::Aabb { min: Vec3::new(-5.0, -5.0, -5.0), max: Vec3::new(5.0, 5.0, 5.0) });
        let p = Vec3::new(3.0, -2.0, 1.0);
        let (x, y, depth) = c.project(c.to_camera(p), 640.0, 480.0);
        let (from, dir) = c.ray(x, y, 640.0, 480.0);
        let along = (p - from).dot(dir);
        assert!(close(from + dir * along, p), "{:?}", from + dir * along);
        assert!(along >= depth, "along the ray, at least as far as straight ahead");
    }
}
