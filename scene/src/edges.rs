//! The edges worth drawing: where faces meet at an angle, and where a
//! surface ends. Found once per mesh; a mesh's own triangles share corners
//! by exact position, as STL writes them.
//!
//! CAD meshers leave T-junctions: a long edge on one side of a seam, two
//! shorter ones on the other, so no edge has a face on each side and every
//! one looks like the end of the surface. Such lone edges are matched by
//! the line they lie on: one overlapped by edges of faces it is nearly
//! level with is inside the surface, and not drawn. And a triangle a file
//! holds twice (a body exported twice) counts once.

use std::collections::{HashMap, HashSet};

use crate::math::Vec3;
use crate::{Lines, Mesh};

/// Faces meeting at more than this, in degrees, have an edge drawn between them.
pub const FEATURE_ANGLE: f32 = 30.0;

/// `mesh`'s feature edges: between faces at more than `angle` degrees,
/// at the boundary of an open surface, and where more than two faces meet.
pub fn feature_edges(mesh: &Mesh, angle: f32, color: [u8; 3]) -> Lines {
    let cos = angle.to_radians().cos();
    let mut segments = Vec::new();
    let mut lone = Vec::new();
    for (key, faces) in edge_faces(mesh) {
        match faces[..] {
            [a, b] if mesh.normals[a].dot(mesh.normals[b]) >= cos => {}
            [face] => lone.push((key, face)),
            _ => segments.push(key),
        }
    }
    segments.extend(unmatched(mesh, &lone, cos));
    Lines { segments, color }
}

/// A line, by its rounded direction and rounded point nearest the origin.
type LineKey = ([i64; 3], [i64; 3]);
/// Where an edge lies along its line, its face, and its index among the lone edges.
type Span = (f32, f32, usize, usize);

/// Of `lone` edges (each with its one face), those not inside the surface:
/// not overlapped, on the same line, by a lone edge of another face within
/// the angle whose cosine is `cos`.
fn unmatched(mesh: &Mesh, lone: &[([Vec3; 2], usize)], cos: f32) -> Vec<[Vec3; 2]> {
    let size = mesh.bounds().size().length().max(1e-6);
    let tol = size * 1e-5;
    // A line's key: its direction (one way round) and its point nearest
    // the origin, both rounded.
    let key = |[a, b]: [Vec3; 2]| {
        let mut d = (b - a).normalized();
        if d.x < -1e-6 || (d.x.abs() <= 1e-6 && (d.y < -1e-6 || (d.y.abs() <= 1e-6 && d.z < 0.0))) {
            d = -d;
        }
        let p = a - d * a.dot(d);
        let r = |v: f32, step: f32| (v / step).round() as i64;
        ([r(d.x, 1e-4), r(d.y, 1e-4), r(d.z, 1e-4)], [r(p.x, tol), r(p.y, tol), r(p.z, tol)], d)
    };
    let mut lines: HashMap<LineKey, Vec<Span>> = HashMap::new();
    for (i, &(e, face)) in lone.iter().enumerate() {
        let (dk, pk, d) = key(e);
        let (t0, t1) = (e[0].dot(d), e[1].dot(d));
        lines.entry((dk, pk)).or_default().push((t0.min(t1), t0.max(t1), face, i));
    }
    let mut inside = vec![false; lone.len()];
    for spans in lines.values() {
        for &(a0, a1, fa, ia) in spans {
            let mut overlapped = false;
            let mut level = true;
            for &(b0, b1, fb, _) in spans {
                if fb == fa || b1.min(a1) - b0.max(a0) <= tol {
                    continue;
                }
                overlapped = true;
                level &= mesh.normals[fa].dot(mesh.normals[fb]) >= cos;
            }
            inside[ia] = overlapped && level;
        }
    }
    lone.iter().zip(inside).filter(|(_, i)| !i).map(|(e, _)| e.0).collect()
}

/// Every edge of every triangle, once: a wireframe.
pub fn all_edges(mesh: &Mesh, color: [u8; 3]) -> Lines {
    Lines { segments: edge_faces(mesh).into_iter().map(|(e, _)| e).collect(), color }
}

/// Each edge, by its two ends, with the triangles that have it.
fn edge_faces(mesh: &Mesh) -> Vec<([Vec3; 2], Vec<usize>)> {
    let bits = |p: Vec3| [p.x.to_bits(), p.y.to_bits(), p.z.to_bits()];
    let mut ids: HashMap<[u32; 3], u32> = HashMap::new();
    let mut points = Vec::new();
    let mut id = |p: Vec3| {
        *ids.entry(bits(p)).or_insert_with(|| {
            points.push(p);
            points.len() as u32 - 1
        })
    };
    let mut edges: HashMap<(u32, u32), Vec<usize>> = HashMap::new();
    let mut order = Vec::new();
    let mut seen = HashSet::new();
    for (i, t) in mesh.triangles.iter().enumerate() {
        let v = t.map(&mut id);
        let mut sorted = v;
        sorted.sort_unstable();
        if !seen.insert(sorted) {
            continue;
        }
        for k in 0..3 {
            let (a, b) = (v[k], v[(k + 1) % 3]);
            if a == b {
                continue;
            }
            let key = (a.min(b), a.max(b));
            let faces = edges.entry(key).or_default();
            if faces.is_empty() {
                order.push(key);
            }
            faces.push(i);
        }
    }
    order
        .into_iter()
        .map(|k| ([points[k.0 as usize], points[k.1 as usize]], edges.remove(&k).unwrap_or_default()))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A unit cube, two triangles a side, wound outwards.
    pub(crate) fn cube() -> Mesh {
        let p = |i: u32| Vec3::new((i & 1) as f32, (i >> 1 & 1) as f32, (i >> 2 & 1) as f32);
        let quads = [[0, 2, 3, 1], [4, 5, 7, 6], [0, 1, 5, 4], [2, 6, 7, 3], [0, 4, 6, 2], [1, 3, 7, 5]];
        let mut t = Vec::new();
        for q in quads {
            t.push([p(q[0]), p(q[1]), p(q[2])]);
            t.push([p(q[0]), p(q[2]), p(q[3])]);
        }
        Mesh::from_triangles(t)
    }

    #[test]
    fn a_cube_has_twelve_edges_not_eighteen() {
        let c = cube();
        assert_eq!(all_edges(&c, [0; 3]).segments.len(), 18, "12 edges and 6 face diagonals");
        assert_eq!(feature_edges(&c, FEATURE_ANGLE, [0; 3]).segments.len(), 12);
        for n in &c.normals {
            assert_eq!(n.length(), 1.0, "wound outwards, none degenerate");
        }
    }

    #[test]
    fn t_junctions_are_inside_the_surface() {
        // A flat 2×1 rectangle: one triangle on the left of the seam x = 1,
        // two on the right, meeting it at (1, 0.5): a T-junction.
        let v = |x: f32, y: f32| Vec3::new(x, y, 0.0);
        let flat = Mesh::from_triangles(vec![
            [v(0.0, 0.0), v(1.0, 0.0), v(1.0, 1.0)],
            [v(0.0, 0.0), v(1.0, 1.0), v(0.0, 1.0)],
            [v(1.0, 0.0), v(2.0, 0.0), v(1.0, 0.5)],
            [v(1.0, 0.5), v(2.0, 0.0), v(2.0, 1.0)],
            [v(1.0, 0.5), v(2.0, 1.0), v(1.0, 1.0)],
        ]);
        let e = feature_edges(&flat, FEATURE_ANGLE, [0; 3]).segments;
        assert!(e.iter().all(|s| s[0].x != 1.0 || s[1].x != 1.0), "nothing drawn along the seam: {e:?}");
        assert_eq!(e.len(), 6, "the outline: two sides along x of two edges each, and the two ends");

        // Folded along the seam, it is an edge again.
        let mut folded = flat.clone();
        for t in &mut folded.triangles[2..] {
            for p in t.iter_mut() {
                p.z = p.x - 1.0;
            }
        }
        let folded = Mesh::from_triangles(folded.triangles);
        let e = feature_edges(&folded, FEATURE_ANGLE, [0; 3]).segments;
        assert!(e.iter().any(|s| s[0].x == 1.0 && s[1].x == 1.0), "the fold is drawn");
    }

    #[test]
    fn a_body_written_twice_counts_once() {
        let mut c = cube();
        c.triangles.extend(c.triangles.clone());
        c.normals.extend(c.normals.clone());
        assert_eq!(feature_edges(&c, FEATURE_ANGLE, [0; 3]).segments.len(), 12);
    }

    #[test]
    fn an_open_surface_has_its_boundary() {
        let mut c = cube();
        c.triangles.truncate(2);
        c.normals.truncate(2);
        assert_eq!(feature_edges(&c, FEATURE_ANGLE, [0; 3]).segments.len(), 4, "a lone square's four sides");
    }
}
