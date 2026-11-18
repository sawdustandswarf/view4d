//! `view4d-scene`: the scene viewer's parts (docs/DESIGN.md).

pub mod camera;
pub mod edges;
pub mod math;
pub mod png;
pub mod raster;
pub mod shapes;
pub mod text;

pub use camera::{Camera, View};
pub use math::{Aabb, Vec3};
pub use raster::{Frame, Style};

/// Triangles with a face normal each, in the scene's units, of one colour.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Mesh {
    pub triangles: Vec<[Vec3; 3]>,
    /// One per triangle, unit length (zero for a degenerate one).
    pub normals: Vec<Vec3>,
    /// sRGB, 0–255.
    pub color: [u8; 3],
}

impl Mesh {
    /// The colour meshes are drawn in unless they say otherwise.
    pub const DEFAULT_COLOR: [u8; 3] = [180, 190, 205];

    /// A mesh of `triangles`, normals from their winding (counter-clockwise
    /// seen from outside).
    pub fn from_triangles(triangles: Vec<[Vec3; 3]>) -> Mesh {
        let normals = triangles.iter().map(|&t| face_normal(t)).collect();
        Mesh { triangles, normals, color: Mesh::DEFAULT_COLOR }
    }

    pub fn bounds(&self) -> Aabb {
        let mut b = Aabb::EMPTY;
        for t in &self.triangles {
            t.iter().for_each(|&p| b.add(p));
        }
        b
    }
}

/// The unit normal of triangle `t`, by its winding; zero if degenerate.
pub fn face_normal([a, b, c]: [Vec3; 3]) -> Vec3 {
    (b - a).cross(c - a).normalized()
}

/// How far along the ray from `from` in direction `dir` it meets triangle
/// `t`, from either side; none if it misses or the triangle is behind.
/// (Möller and Trumbore.)
pub fn ray_hits([a, b, c]: [Vec3; 3], from: Vec3, dir: Vec3) -> Option<f32> {
    let (e1, e2) = (b - a, c - a);
    let p = dir.cross(e2);
    let det = e1.dot(p);
    if det.abs() < 1e-12 {
        return None;
    }
    let s = from - a;
    let u = s.dot(p) / det;
    let q = s.cross(e1);
    let v = dir.dot(q) / det;
    let t = e2.dot(q) / det;
    (u >= 0.0 && v >= 0.0 && u + v <= 1.0 && t > 0.0).then_some(t)
}

/// Line segments of one colour: edges, a wireframe, orbits.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Lines {
    pub segments: Vec<[Vec3; 2]>,
    pub color: [u8; 3],
}

/// A name written beside a point in the scene, in a colour.
#[derive(Debug, Clone, PartialEq)]
pub struct Label {
    pub at: Vec3,
    pub text: String,
    pub color: [u8; 3],
}

/// What the viewer shows: a model, the orrery. The viewer has the camera,
/// input and the overlay; a scene says what there is to see, and handles
/// keys the viewer leaves alone.
pub trait Scene {
    /// Solid things, drawn shaded.
    fn meshes(&self) -> &[Mesh];
    /// Lines over and among them: edges, orbits.
    fn lines(&self) -> &[Lines];
    /// Its extent, for fitting the camera.
    fn bounds(&self) -> Aabb;
    /// The overlay's line.
    fn status(&self) -> String;
    /// Something wrong, shown under the status.
    fn error(&self) -> Option<&str> {
        None
    }
    /// A key the viewer doesn't use; true if what is shown changed.
    fn key(&mut self, _key: char) -> bool {
        false
    }
    /// Asked twice a second, and before each frame while it animates: true
    /// if what is shown changed by itself (its file was rewritten, the
    /// clock moved on).
    fn poll(&mut self) -> bool {
        false
    }
    /// Whether it moves by itself, so frames should keep coming.
    fn animates(&self) -> bool {
        false
    }
    /// Names to write beside points: the planets'.
    fn labels(&self) -> &[Label] {
        &[]
    }
    /// A click on what the ray from `from` along `dir` (unit length) meets
    /// first; true if what is shown changed.
    fn pick(&mut self, _from: Vec3, _dir: Vec3) -> bool {
        false
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rays_and_triangles() {
        let t = [Vec3::new(0.0, 0.0, 0.0), Vec3::new(2.0, 0.0, 0.0), Vec3::new(0.0, 2.0, 0.0)];
        let down = Vec3::new(0.0, 0.0, -1.0);
        assert_eq!(ray_hits(t, Vec3::new(0.5, 0.5, 3.0), down), Some(3.0));
        assert_eq!(ray_hits(t, Vec3::new(0.5, 0.5, -3.0), -down), Some(3.0), "from behind too");
        assert_eq!(ray_hits(t, Vec3::new(1.5, 1.5, 3.0), down), None, "beside it");
        assert_eq!(ray_hits(t, Vec3::new(0.5, 0.5, -3.0), down), None, "behind the ray");
        assert_eq!(ray_hits(t, Vec3::new(0.5, 0.5, 3.0), Vec3::new(1.0, 0.0, 0.0)), None, "parallel");
    }
}
