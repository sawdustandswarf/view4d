//! Solids as closed meshes of convex polygons in f64, and the booleans on
//! them by BSP trees (the method of Evan Wallace's csg.js): each solid's
//! polygons are clipped by the other's tree, and what is left joined.
//!
//! A polygon keeps the plane it was made on through every split, so faces
//! that lie on one plane stay on it exactly; points within `EPSILON` (mm) of
//! a plane count as on it. Faces of two solids that meet on one plane
//! (a cut flush with a face, two boxes fused side by side) are told apart by
//! which way they face, as csg.js does.
//!
//! The trees are walked with stacks, not recursion: a tree can be as deep as
//! a solid has faces.

use std::ops::{Add, Mul, Neg, Sub};

/// Points within this distance of a plane (mm) are on it.
pub const EPSILON: f64 = 1e-5;

#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct V3 {
    pub x: f64,
    pub y: f64,
    pub z: f64,
}

pub const fn v3(x: f64, y: f64, z: f64) -> V3 {
    V3 { x, y, z }
}

impl V3 {
    pub fn dot(self, o: V3) -> f64 {
        self.x * o.x + self.y * o.y + self.z * o.z
    }

    pub fn cross(self, o: V3) -> V3 {
        v3(self.y * o.z - self.z * o.y, self.z * o.x - self.x * o.z, self.x * o.y - self.y * o.x)
    }

    pub fn len(self) -> f64 {
        self.dot(self).sqrt()
    }

    pub fn unit(self) -> V3 {
        self * (1.0 / self.len())
    }

    pub fn lerp(self, o: V3, t: f64) -> V3 {
        self + (o - self) * t
    }

    pub fn min(self, o: V3) -> V3 {
        v3(self.x.min(o.x), self.y.min(o.y), self.z.min(o.z))
    }

    pub fn max(self, o: V3) -> V3 {
        v3(self.x.max(o.x), self.y.max(o.y), self.z.max(o.z))
    }

    /// Component `i` (0 x, 1 y, 2 z).
    pub fn at(self, i: usize) -> f64 {
        [self.x, self.y, self.z][i]
    }

    pub fn with(mut self, i: usize, v: f64) -> V3 {
        match i {
            0 => self.x = v,
            1 => self.y = v,
            _ => self.z = v,
        }
        self
    }
}

impl Add for V3 {
    type Output = V3;
    fn add(self, o: V3) -> V3 {
        v3(self.x + o.x, self.y + o.y, self.z + o.z)
    }
}

impl Sub for V3 {
    type Output = V3;
    fn sub(self, o: V3) -> V3 {
        v3(self.x - o.x, self.y - o.y, self.z - o.z)
    }
}

impl Mul<f64> for V3 {
    type Output = V3;
    fn mul(self, k: f64) -> V3 {
        v3(self.x * k, self.y * k, self.z * k)
    }
}

impl Neg for V3 {
    type Output = V3;
    fn neg(self) -> V3 {
        v3(-self.x, -self.y, -self.z)
    }
}

/// The plane n · p = w, n a unit vector.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Plane {
    pub n: V3,
    pub w: f64,
}

impl Plane {
    /// The plane through three points, facing the way they turn
    /// anticlockwise; None if they are (nearly) on a line.
    pub fn through(a: V3, b: V3, c: V3) -> Option<Plane> {
        let n = (b - a).cross(c - a);
        let l = n.len();
        if l < 1e-12 {
            return None;
        }
        let n = n * (1.0 / l);
        Some(Plane { n, w: n.dot(a) })
    }

    fn flip(&mut self) {
        self.n = -self.n;
        self.w = -self.w;
    }
}

/// A convex polygon, its vertices anticlockwise seen from outside.
#[derive(Debug, Clone, PartialEq)]
pub struct Polygon {
    pub verts: Vec<V3>,
    pub plane: Plane,
}

impl Polygon {
    /// A polygon of `verts` (convex, planar, at least 3), on the plane its
    /// largest corner gives; None if it has no area.
    pub fn new(verts: Vec<V3>) -> Option<Polygon> {
        if verts.len() < 3 {
            return None;
        }
        // Newell's normal: right for any planar polygon, whatever its first corners.
        let mut n = V3::default();
        for (i, a) in verts.iter().enumerate() {
            let b = verts[(i + 1) % verts.len()];
            n = n + v3((a.y - b.y) * (a.z + b.z), (a.z - b.z) * (a.x + b.x), (a.x - b.x) * (a.y + b.y));
        }
        let l = n.len();
        if l < 1e-12 {
            return None;
        }
        let n = n * (1.0 / l);
        let centre = verts.iter().fold(V3::default(), |s, &v| s + v) * (1.0 / verts.len() as f64);
        Some(Polygon { plane: Plane { n, w: n.dot(centre) }, verts })
    }

    fn flip(&mut self) {
        self.verts.reverse();
        self.plane.flip();
    }
}

const COPLANAR: u8 = 0;
const FRONT: u8 = 1;
const BACK: u8 = 2;
const SPANNING: u8 = 3;

/// Where `poly` lies against `plane`: into the four lists (coplanar ones
/// facing the plane's way go in `co_front`).
fn split(
    plane: &Plane,
    poly: Polygon,
    co_front: &mut Vec<Polygon>,
    co_back: &mut Vec<Polygon>,
    front: &mut Vec<Polygon>,
    back: &mut Vec<Polygon>,
) {
    let mut kind = 0;
    let mut types = Vec::with_capacity(poly.verts.len());
    for v in &poly.verts {
        let t = plane.n.dot(*v) - plane.w;
        let ty = if t < -EPSILON {
            BACK
        } else if t > EPSILON {
            FRONT
        } else {
            COPLANAR
        };
        kind |= ty;
        types.push((ty, t));
    }
    match kind {
        COPLANAR => {
            if plane.n.dot(poly.plane.n) > 0.0 {
                co_front.push(poly)
            } else {
                co_back.push(poly)
            }
        }
        FRONT => front.push(poly),
        BACK => back.push(poly),
        _ => {
            let n = poly.verts.len();
            let (mut f, mut b) = (Vec::with_capacity(n + 1), Vec::with_capacity(n + 1));
            for i in 0..n {
                let j = (i + 1) % n;
                let ((ti, di), (tj, dj)) = (types[i], types[j]);
                let (vi, vj) = (poly.verts[i], poly.verts[j]);
                if ti != BACK {
                    f.push(vi);
                }
                if ti != FRONT {
                    b.push(vi);
                }
                if (ti | tj) == SPANNING {
                    let v = vi.lerp(vj, di / (di - dj));
                    f.push(v);
                    b.push(v);
                }
            }
            if f.len() >= 3 {
                front.push(Polygon { verts: f, plane: poly.plane });
            }
            if b.len() >= 3 {
                back.push(Polygon { verts: b, plane: poly.plane });
            }
        }
    }
}

#[derive(Debug, Clone)]
struct Node {
    plane: Plane,
    front: Option<usize>,
    back: Option<usize>,
    polys: Vec<Polygon>,
}

/// Polygons still to add: the node they go to (None: a new one), and the
/// parent the new one hangs from, on its front (true) or back.
type Pending = (Option<usize>, Vec<Polygon>, Option<(usize, bool)>);

/// A BSP tree of a solid's polygons, its nodes in an arena.
#[derive(Debug, Clone, Default)]
struct Tree {
    nodes: Vec<Node>,
}

impl Tree {
    fn new(polys: Vec<Polygon>) -> Tree {
        let mut t = Tree::default();
        t.add(polys);
        t
    }

    /// Adds `polys` to the tree, splitting them by the planes already there.
    fn add(&mut self, polys: Vec<Polygon>) {
        if polys.is_empty() {
            return;
        }
        let mut stack: Vec<Pending> = vec![(if self.nodes.is_empty() { None } else { Some(0) }, polys, None)];
        while let Some((at, polys, parent)) = stack.pop() {
            if polys.is_empty() {
                continue;
            }
            let i = match at {
                Some(i) => i,
                None => {
                    self.nodes.push(Node { plane: polys[0].plane, front: None, back: None, polys: vec![] });
                    let i = self.nodes.len() - 1;
                    if let Some((p, is_front)) = parent {
                        if is_front {
                            self.nodes[p].front = Some(i);
                        } else {
                            self.nodes[p].back = Some(i);
                        }
                    }
                    i
                }
            };
            let plane = self.nodes[i].plane;
            let (mut co_f, mut co_b, mut f, mut b) = (vec![], vec![], vec![], vec![]);
            for p in polys {
                split(&plane, p, &mut co_f, &mut co_b, &mut f, &mut b);
            }
            let node = &mut self.nodes[i];
            node.polys.extend(co_f);
            node.polys.extend(co_b);
            let (nf, nb) = (node.front, node.back);
            stack.push((nf, f, Some((i, true))));
            stack.push((nb, b, Some((i, false))));
        }
    }

    /// The parts of `polys` outside this tree's solid.
    fn clip(&self, polys: Vec<Polygon>) -> Vec<Polygon> {
        if self.nodes.is_empty() {
            return polys;
        }
        let mut out = Vec::new();
        let mut stack = vec![(0usize, polys)];
        while let Some((i, polys)) = stack.pop() {
            let node = &self.nodes[i];
            let (mut f, mut b) = (Vec::new(), Vec::new());
            for p in polys {
                // Coplanar ones go with the side they face (csg.js).
                let (mut cf, mut cb) = (Vec::new(), Vec::new());
                split(&node.plane, p, &mut cf, &mut cb, &mut f, &mut b);
                f.append(&mut cf);
                b.append(&mut cb);
            }
            match node.front {
                Some(n) if !f.is_empty() => stack.push((n, f)),
                Some(_) => {}
                None => out.append(&mut f),
            }
            if let Some(n) = node.back
                && !b.is_empty()
            {
                stack.push((n, b));
            }
            // With no back node, what is behind is inside: dropped.
        }
        out
    }

    /// Removes the parts of this tree's polygons inside `other`'s solid.
    fn clip_to(&mut self, other: &Tree) {
        for node in &mut self.nodes {
            let polys = std::mem::take(&mut node.polys);
            node.polys = other.clip(polys);
        }
    }

    /// Inside out.
    fn invert(&mut self) {
        for node in &mut self.nodes {
            node.polys.iter_mut().for_each(Polygon::flip);
            node.plane.flip();
            std::mem::swap(&mut node.front, &mut node.back);
        }
    }

    fn polygons(self) -> Vec<Polygon> {
        self.nodes.into_iter().flat_map(|n| n.polys).collect()
    }
}

/// Bounds: (min, max).
pub type Bounds = (V3, V3);

fn overlap((a0, a1): Bounds, (b0, b1): Bounds) -> bool {
    (0..3).all(|i| a0.at(i) <= b1.at(i) + EPSILON && b0.at(i) <= a1.at(i) + EPSILON)
}

/// A solid: a closed surface of convex polygons facing out. Empty is
/// nothing.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Solid {
    pub polys: Vec<Polygon>,
}

impl Solid {
    pub fn new(polys: Vec<Polygon>) -> Solid {
        Solid { polys }
    }

    pub fn is_empty(&self) -> bool {
        self.polys.is_empty()
    }

    /// Its bounds; None if empty.
    pub fn bounds(&self) -> Option<Bounds> {
        let mut vs = self.polys.iter().flat_map(|p| p.verts.iter().copied());
        let first = vs.next()?;
        Some(vs.fold((first, first), |(lo, hi), v| (lo.min(v), hi.max(v))))
    }

    /// The volume it encloses (by the divergence theorem, about `origin`:
    /// the same for any origin when the surface is closed).
    pub fn volume_about(&self, origin: V3) -> f64 {
        let mut v = 0.0;
        for p in &self.polys {
            let a = p.verts[0] - origin;
            for w in p.verts[1..].windows(2) {
                v += a.dot((w[0] - origin).cross(w[1] - origin));
            }
        }
        v / 6.0
    }

    /// Its faces' areas, and the length of the sum of their area vectors
    /// (each face's area along its normal): 0 for a closed surface, so
    /// the second over the first is how open it is.
    pub fn area_balance(&self) -> (f64, f64) {
        let (mut total, mut sum) = (0.0, V3::default());
        for p in &self.polys {
            let a = p.verts[0];
            for w in p.verts[1..].windows(2) {
                let n = (w[0] - a).cross(w[1] - a) * 0.5;
                total += n.len();
                sum = sum + n;
            }
        }
        (total, sum.len())
    }

    pub fn volume(&self) -> f64 {
        self.volume_about(V3::default())
    }

    /// Its triangles (each polygon as a fan), anticlockwise from outside.
    pub fn triangles(&self) -> impl Iterator<Item = [V3; 3]> + '_ {
        self.polys.iter().flat_map(|p| p.verts[1..].windows(2).map(move |w| [p.verts[0], w[0], w[1]]))
    }

    /// Moved by `d`.
    pub fn translated(mut self, d: V3) -> Solid {
        for p in &mut self.polys {
            p.verts.iter_mut().for_each(|v| *v = *v + d);
            p.plane.w += p.plane.n.dot(d);
        }
        self
    }

    /// Turned by the rotation `m` (rows) about the origin.
    pub fn turned(mut self, m: &[[f64; 3]; 3]) -> Solid {
        let rot = |v: V3| {
            v3(
                m[0][0] * v.x + m[0][1] * v.y + m[0][2] * v.z,
                m[1][0] * v.x + m[1][1] * v.y + m[1][2] * v.z,
                m[2][0] * v.x + m[2][1] * v.y + m[2][2] * v.z,
            )
        };
        for p in &mut self.polys {
            p.verts.iter_mut().for_each(|v| *v = rot(*v));
            p.plane.n = rot(p.plane.n);
        }
        self
    }

    /// Turned by `angle` degrees about the z axis.
    pub fn rotated_z(mut self, angle: f64) -> Solid {
        let (s, c) = angle.to_radians().sin_cos();
        let rot = |v: V3| v3(c * v.x - s * v.y, s * v.x + c * v.y, v.z);
        for p in &mut self.polys {
            p.verts.iter_mut().for_each(|v| *v = rot(*v));
            p.plane.n = rot(p.plane.n);
        }
        self
    }

    /// Everything in either. Solids whose bounds don't meet are only put
    /// together.
    pub fn union(self, other: Solid) -> Solid {
        match (self.bounds(), other.bounds()) {
            (None, _) => return other,
            (_, None) => return self,
            (Some(a), Some(b)) if !overlap(a, b) => {
                let mut polys = self.polys;
                polys.extend(other.polys);
                return Solid { polys };
            }
            _ => {}
        }
        let (mut a, mut b) = (Tree::new(self.polys), Tree::new(other.polys));
        a.clip_to(&b);
        b.clip_to(&a);
        b.invert();
        b.clip_to(&a);
        b.invert();
        let mut polys = a.polygons();
        polys.extend(b.polygons());
        Solid { polys }
    }

    /// This, less `other`.
    pub fn subtract(self, other: Solid) -> Solid {
        match (self.bounds(), other.bounds()) {
            (None, _) => return Solid::default(),
            (Some(a), Some(b)) if overlap(a, b) => {}
            _ => return self,
        }
        let (mut a, mut b) = (Tree::new(self.polys), Tree::new(other.polys));
        a.invert();
        a.clip_to(&b);
        b.clip_to(&a);
        b.invert();
        b.clip_to(&a);
        b.invert();
        let mut polys = a.polygons();
        polys.extend(b.polygons());
        let mut out = Solid { polys };
        out.polys.iter_mut().for_each(Polygon::flip);
        out
    }

    /// What is in both.
    pub fn intersect(self, other: Solid) -> Solid {
        match (self.bounds(), other.bounds()) {
            (Some(a), Some(b)) if overlap(a, b) => {}
            _ => return Solid::default(),
        }
        let (mut a, mut b) = (Tree::new(self.polys), Tree::new(other.polys));
        a.invert();
        b.clip_to(&a);
        b.invert();
        a.clip_to(&b);
        b.clip_to(&a);
        let mut polys = a.polygons();
        polys.extend(b.polygons());
        let mut out = Solid { polys };
        out.polys.iter_mut().for_each(Polygon::flip);
        out
    }

    /// Everything in any of `solids`.
    pub fn union_all(solids: Vec<Solid>) -> Solid {
        // Pairwise, so each boolean is between solids of similar size.
        let mut layer = solids;
        while layer.len() > 1 {
            let mut next = Vec::with_capacity(layer.len().div_ceil(2));
            let mut it = layer.into_iter();
            while let Some(a) = it.next() {
                next.push(match it.next() {
                    Some(b) => a.union(b),
                    None => a,
                });
            }
            layer = next;
        }
        layer.pop().unwrap_or_default()
    }
}

/// The box between `lo` and `hi`.
pub fn cuboid(lo: V3, hi: V3) -> Solid {
    let c = |i: usize| {
        v3(
            if i & 1 != 0 { hi.x } else { lo.x },
            if i & 2 != 0 { hi.y } else { lo.y },
            if i & 4 != 0 { hi.z } else { lo.z },
        )
    };
    // Each face's corners, anticlockwise from outside.
    let faces = [[0, 4, 6, 2], [1, 3, 7, 5], [0, 1, 5, 4], [2, 6, 7, 3], [0, 2, 3, 1], [4, 5, 7, 6]];
    Solid { polys: faces.iter().filter_map(|f| Polygon::new(f.iter().map(|&i| c(i)).collect())).collect() }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn close(a: f64, b: f64) -> bool {
        (a - b).abs() < 1e-9 * a.abs().max(b.abs()).max(1.0)
    }

    /// Closed: the volume is the same about any origin.
    fn closed(s: &Solid) -> bool {
        let v = s.volume();
        [v3(100.0, -50.0, 30.0), v3(-7.0, 300.0, -90.0)].iter().all(|&o| close(s.volume_about(o), v))
    }

    fn cube(x: f64, y: f64, z: f64, size: f64) -> Solid {
        cuboid(v3(x, y, z), v3(x + size, y + size, z + size))
    }

    #[test]
    fn a_box() {
        let b = cuboid(v3(0.0, 0.0, 0.0), v3(1.0, 2.0, 3.0));
        assert_eq!(b.polys.len(), 6);
        assert!(close(b.volume(), 6.0) && closed(&b));
        for p in &b.polys {
            let c = p.verts.iter().fold(V3::default(), |s, &v| s + v) * 0.25;
            assert!(p.plane.n.dot(c - v3(0.5, 1.0, 1.5)) > 0.0, "a face looks in");
        }
    }

    #[test]
    fn overlapping_boxes() {
        let (a, b) = (cube(0.0, 0.0, 0.0, 2.0), cube(1.0, 1.0, 1.0, 2.0));
        for (s, want) in [
            (a.clone().union(b.clone()), 15.0),
            (a.clone().subtract(b.clone()), 7.0),
            (a.clone().intersect(b.clone()), 1.0),
            (b.clone().subtract(a.clone()), 7.0),
        ] {
            assert!(close(s.volume(), want), "{} not {want}", s.volume());
            assert!(closed(&s));
        }
    }

    #[test]
    fn faces_on_one_plane() {
        // Side by side, sharing a face.
        let (a, b) = (cube(0.0, 0.0, 0.0, 1.0), cube(1.0, 0.0, 0.0, 1.0));
        let u = a.clone().union(b.clone());
        assert!(close(u.volume(), 2.0) && closed(&u));
        assert!(a.clone().intersect(b.clone()).volume().abs() < 1e-9);
        // A cut flush with the top, and through and through.
        let slab = cuboid(v3(0.0, 0.0, 0.0), v3(4.0, 4.0, 1.0));
        let pocket = slab.clone().subtract(cuboid(v3(1.0, 1.0, 0.5), v3(3.0, 3.0, 1.0)));
        assert!(close(pocket.volume(), 16.0 - 2.0) && closed(&pocket));
        let hole = slab.clone().subtract(cuboid(v3(1.0, 1.0, 0.0), v3(3.0, 3.0, 1.0)));
        assert!(close(hole.volume(), 12.0) && closed(&hole));
        // The same solid twice.
        assert!(close(slab.clone().union(slab.clone()).volume(), 16.0));
        assert!(slab.clone().subtract(slab.clone()).volume().abs() < 1e-9);
        assert!(close(slab.clone().intersect(slab.clone()).volume(), 16.0));
    }

    #[test]
    fn apart() {
        let (a, b) = (cube(0.0, 0.0, 0.0, 1.0), cube(5.0, 0.0, 0.0, 1.0));
        assert!(close(a.clone().union(b.clone()).volume(), 2.0));
        assert_eq!(a.clone().subtract(b.clone()), a);
        assert!(a.intersect(b).is_empty());
    }

    #[test]
    fn many_at_once() {
        let cubes: Vec<Solid> = (0..20).map(|i| cube(i as f64 * 0.5, 0.0, 0.0, 1.0)).collect();
        let u = Solid::union_all(cubes);
        assert!(close(u.volume(), 10.5) && closed(&u));
    }

    #[test]
    fn moved_and_turned() {
        let b = cuboid(v3(0.0, 0.0, 0.0), v3(2.0, 1.0, 1.0)).translated(v3(1.0, 0.0, 0.0)).rotated_z(90.0);
        let (lo, hi) = b.bounds().unwrap();
        assert!(close(lo.x, -1.0) && close(hi.y, 3.0) && close(lo.y, 1.0));
        // Planes moved with it: a boolean still works.
        let cut = b.subtract(cuboid(v3(-2.0, 0.0, 0.0), v3(2.0, 2.0, 2.0)));
        assert!(close(cut.volume(), 1.0) && closed(&cut));
    }
}

/// A solid as a mesh for drawing and writing: shared vertices, every edge
/// met by exactly one other going the other way when the surface is
/// closed (no T-junctions), each face's triangles with its plane's normal.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Mesh {
    pub verts: Vec<V3>,
    /// Triangles: indices into `verts`, anticlockwise from outside.
    pub triangles: Vec<[u32; 3]>,
    /// One per triangle.
    pub normals: Vec<V3>,
}

/// Vertices this close (mm) are one: twice as close as the booleans take a
/// point to be on a plane, since two splits near one point can leave three
/// a little more than EPSILON apart in a row (stencil's bridges across the
/// curve of an 8), and the first found wins.
pub const WELD: f64 = 2.0 * EPSILON;

impl Solid {
    /// As a `Mesh`: vertices within `WELD` welded, each vertex lying on
    /// another face's edge put into that edge, faces triangulated without
    /// slivers.
    pub fn mesh(&self) -> Mesh {
        let mut verts: Vec<V3> = vec![];
        let mut grid: std::collections::HashMap<[i64; 3], u32> = std::collections::HashMap::new();
        let cell = |v: V3| [(v.x / WELD).round() as i64, (v.y / WELD).round() as i64, (v.z / WELD).round() as i64];
        let mut weld = |v: V3| -> u32 {
            let c = cell(v);
            for dx in -1..=1 {
                for dy in -1..=1 {
                    for dz in -1..=1 {
                        if let Some(&i) = grid.get(&[c[0] + dx, c[1] + dy, c[2] + dz])
                            && (verts[i as usize] - v).len() <= WELD
                        {
                            return i;
                        }
                    }
                }
            }
            verts.push(v);
            let i = (verts.len() - 1) as u32;
            grid.insert(c, i);
            i
        };
        let mut faces: Vec<(Vec<u32>, V3)> = vec![];
        for p in &self.polys {
            let mut idx: Vec<u32> = vec![];
            for &v in &p.verts {
                let i = weld(v);
                if idx.last() != Some(&i) {
                    idx.push(i);
                }
            }
            while idx.len() > 1 && idx[0] == idx[idx.len() - 1] {
                idx.pop();
            }
            if idx.len() >= 3 {
                faces.push((idx, p.plane.n));
            }
        }
        split_t_junctions(&verts, &mut faces);
        let mut m = Mesh { verts, ..Mesh::default() };
        for (idx, n) in faces {
            for t in triangulate(&m.verts, &idx, n) {
                m.triangles.push(t);
                m.normals.push(n);
            }
        }
        m
    }
}

impl Mesh {
    /// The directed edges with no twin going the other way: none for a
    /// closed surface.
    pub fn open_edges(&self) -> usize {
        let mut count: std::collections::HashMap<(u32, u32), i32> = std::collections::HashMap::new();
        for t in &self.triangles {
            for k in 0..3 {
                let (a, b) = (t[k], t[(k + 1) % 3]);
                if a < b {
                    *count.entry((a, b)).or_default() += 1;
                } else {
                    *count.entry((b, a)).or_default() -= 1;
                }
            }
        }
        count.values().map(|c| c.unsigned_abs() as usize).sum()
    }
}

/// Each face edge with no twin gets the vertices that lie along it (ends
/// of other edges on its line), so that it meets its neighbours' edges end
/// to end. Again, while that finds more (a vertex put in can be one that
/// lies along another edge).
fn split_t_junctions(verts: &[V3], faces: &mut [(Vec<u32>, V3)]) {
    for _ in 0..4 {
        if !split_once(verts, faces) {
            break;
        }
    }
}

/// Vertices to put into a face's edge `k`: (distance along it, vertex).
type EdgeInserts = (usize, Vec<(f64, u32)>);

/// One pass of `split_t_junctions`: whether it put any vertex in.
fn split_once(verts: &[V3], faces: &mut [(Vec<u32>, V3)]) -> bool {
    use std::collections::{HashMap, HashSet};
    let mut directed: HashSet<(u32, u32)> = HashSet::new();
    for (idx, _) in faces.iter() {
        for k in 0..idx.len() {
            directed.insert((idx[k], idx[(k + 1) % idx.len()]));
        }
    }
    // Every edge's ends, bucketed by its line: direction (one way round)
    // and the point nearest the origin, rounded; looked up in neighbouring
    // buckets too, so rounding never parts two edges on one line.
    const D: f64 = 1e-3; // direction
    const P: f64 = 1e-3; // mm
    let line = |a: V3, b: V3| {
        let mut d = (b - a).unit();
        if d.x < -1e-9 || (d.x.abs() <= 1e-9 && (d.y < -1e-9 || (d.y.abs() <= 1e-9 && d.z < 0.0))) {
            d = -d;
        }
        let p = a - d * a.dot(d);
        let r = |v: f64, s: f64| (v / s).floor() as i64;
        [r(d.x, D), r(d.y, D), r(d.z, D), r(p.x, P), r(p.y, P), r(p.z, P)]
    };
    let mut buckets: HashMap<[i64; 6], Vec<u32>> = HashMap::new();
    let mut lone = vec![];
    for (f, (idx, _)) in faces.iter().enumerate() {
        for k in 0..idx.len() {
            let (a, b) = (idx[k], idx[(k + 1) % idx.len()]);
            if !directed.contains(&(b, a)) {
                lone.push((f, k));
            }
            if a < b || !directed.contains(&(b, a)) {
                let e = buckets.entry(line(verts[a as usize], verts[b as usize])).or_default();
                e.push(a);
                e.push(b);
            }
        }
    }
    // What goes into each lone edge, by face: (edge k, [(t, vertex)]).
    let mut inserts: HashMap<usize, Vec<EdgeInserts>> = HashMap::new();
    for &(f, k) in &lone {
        let idx = &faces[f].0;
        let (a, b) = (idx[k], idx[(k + 1) % idx.len()]);
        let (va, vb) = (verts[a as usize], verts[b as usize]);
        let len = (vb - va).len();
        let d = (vb - va) * (1.0 / len);
        let key = line(va, vb);
        let mut found: Vec<(f64, u32)> = vec![];
        for n in 0..729 {
            let mut k2 = key;
            let mut m = n;
            for c in &mut k2 {
                *c += (m % 3) as i64 - 1;
                m /= 3;
            }
            for &w in buckets.get(&k2).map(Vec::as_slice).unwrap_or_default() {
                if w == a || w == b {
                    continue;
                }
                let vw = verts[w as usize] - va;
                let t = vw.dot(d);
                if t <= WELD || t >= len - WELD {
                    continue;
                }
                if (vw - d * t).len() <= 2.0 * WELD && !found.iter().any(|&(_, x)| x == w) {
                    found.push((t, w));
                }
            }
        }
        if !found.is_empty() {
            found.sort_by(|x, y| x.0.total_cmp(&y.0));
            inserts.entry(f).or_default().push((k, found));
        }
    }
    let any = !inserts.is_empty();
    for (f, mut edges) in inserts {
        edges.sort_by_key(|e| std::cmp::Reverse(e.0));
        let idx = &mut faces[f].0;
        for (k, found) in edges {
            for (j, (_, w)) in found.into_iter().enumerate() {
                idx.insert(k + 1 + j, w);
            }
        }
    }
    any
}

/// A convex face (with vertices perhaps along its edges) as triangles: a
/// fan, which has every edge of the face as an edge of one triangle, so
/// neighbouring faces meet edge to edge. Its apex is the vertex that
/// leaves the fewest flat triangles (an apex on a line of vertices makes
/// one per vertex past the next; they have no area, but keep the edges).
fn triangulate(verts: &[V3], idx: &[u32], n: V3) -> Vec<[u32; 3]> {
    let m = idx.len();
    let p = |k: usize| verts[idx[k % m] as usize];
    // b on the line from a to c (within WELD).
    let flat = |a: V3, b: V3, c: V3| {
        let l = (c - a).len();
        l < 1e-15 || ((b - a).cross(c - b).dot(n) / l).abs() <= WELD
    };
    let apex = if (0..m).any(|k| flat(p(k), p(k + 1), p(k + 2))) {
        (0..m).min_by_key(|&a| (1..m - 1).filter(|&k| flat(p(a), p(a + k), p(a + k + 1))).count()).unwrap_or(0)
    } else {
        0
    };
    (1..m - 1).map(|k| [idx[apex], idx[(apex + k) % m], idx[(apex + k + 1) % m]]).collect()
}

#[cfg(test)]
mod mesh_tests {
    use super::*;

    #[test]
    fn closed_without_t_junctions() {
        // A box with a smaller one on top: its top face meets the small one's edges.
        let s = cuboid(v3(0.0, 0.0, 0.0), v3(4.0, 4.0, 1.0)).union(cuboid(v3(1.0, 1.0, 1.0), v3(3.0, 3.0, 2.0)));
        let m = s.mesh();
        assert_eq!(m.open_edges(), 0);
        let v: f64 = m
            .triangles
            .iter()
            .map(|t| {
                let [a, b, c] = t.map(|i| m.verts[i as usize]);
                a.dot(b.cross(c)) / 6.0
            })
            .sum();
        assert!((v - 20.0).abs() < 1e-9, "{v}");
        // An L of two boxes side by side: a T-junction where they meet.
        let l = cuboid(v3(0.0, 0.0, 0.0), v3(2.0, 1.0, 1.0)).union(cuboid(v3(0.0, 1.0, 0.0), v3(1.0, 2.0, 1.0)));
        assert_eq!(l.mesh().open_edges(), 0);
    }
}
