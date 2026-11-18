//! Meshes made rather than read: a sphere, for now.

use crate::Mesh;
use crate::math::Vec3;

/// Rings of latitude, pole to pole; twice as many meridians.
const RINGS: usize = 12;

/// A sphere at `center` of `radius`, wound outwards, in `color`.
pub fn sphere(center: Vec3, radius: f32, color: [u8; 3]) -> Mesh {
    let point = |i: usize, j: usize| {
        let lat = (i as f32 / RINGS as f32 - 0.5) * std::f32::consts::PI;
        let lon = j as f32 / (2 * RINGS) as f32 * std::f32::consts::TAU;
        center + Vec3::new(lat.cos() * lon.cos(), lat.cos() * lon.sin(), lat.sin()) * radius
    };
    let mut triangles = Vec::with_capacity(4 * RINGS * RINGS);
    for i in 0..RINGS {
        for j in 0..2 * RINGS {
            let (a, b, c, d) = (point(i, j), point(i, j + 1), point(i + 1, j + 1), point(i + 1, j));
            // A quad, counter-clockwise from outside; at a pole one of its
            // triangles has no area.
            if i != 0 {
                triangles.push([a, b, c]);
            }
            if i + 1 != RINGS {
                triangles.push([a, c, d]);
            }
        }
    }
    Mesh { color, ..Mesh::from_triangles(triangles) }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_sphere_faces_out() {
        let c = Vec3::new(1.0, 2.0, 3.0);
        let s = sphere(c, 2.0, [1, 2, 3]);
        assert_eq!(s.color, [1, 2, 3]);
        assert_eq!(s.triangles.len(), 2 * RINGS * (2 * RINGS - 2));
        for (t, n) in s.triangles.iter().zip(&s.normals) {
            let mid = (t[0] + t[1] + t[2]) * (1.0 / 3.0);
            assert!(n.dot(mid - c) > 0.0, "outwards");
            assert!(t.iter().all(|&p| ((p - c).length() - 2.0).abs() < 1e-5));
        }
        let b = s.bounds();
        assert!((b.size().x - 4.0).abs() < 1e-5 && (b.size().z - 4.0).abs() < 1e-5);
    }
}
