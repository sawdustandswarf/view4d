//! Solids from flat profiles (primlib's docs/FORMAT.md, "From profiles"):
//! polygons whose corners may be rounded, extruded (`prism`), lofted
//! through (`loft`), turned about z (`revolve`, `loft_about`); and the
//! other round solids (`sphere`, `sector`, `polygon`).

use std::f64::consts::TAU;

use crate::Error;
use crate::build::{area_true, segments};
use crate::csg::{Polygon, Solid, V3, v3};

/// A profile's vertex: (a, b), rounded by a tangent arc of `r` when r > 0.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Vertex {
    pub a: f64,
    pub b: f64,
    pub r: f64,
}

/// Rounded corners are left sharp at or below this radius, as primlib's
/// FreeCAD target does.
const SHARP: f64 = 1e-6;

/// The outline of `pts` (closed), vertex `k` as `count[k]` points: a
/// rounded one an arc of that many points, a sharp one its point repeated
/// (so the outlines of a loft's sections match point for point). Errors as
/// primlib's sketches fail: two vertices on one point, or a round that
/// doesn't fit.
pub fn outline(pts: &[Vertex], count: &[usize]) -> Result<Vec<[f64; 2]>, Error> {
    let n = pts.len();
    if n < 3 {
        return Err(Error(format!("a profile of {n} points")));
    }
    let p = |i: usize| [pts[i % n].a, pts[i % n].b];
    let len = |a: [f64; 2], b: [f64; 2]| (a[0] - b[0]).hypot(a[1] - b[1]);
    for i in 0..n {
        if len(p(i), p(i + 1)) < 1e-6 {
            let a = p(i);
            return Err(Error(format!(
                "profile vertices {i} and {} coincide at ({:.4}, {:.4})",
                (i + 1) % n,
                a[0],
                a[1]
            )));
        }
    }
    let mut out = Vec::new();
    let mut taken = vec![0.0; n]; // each edge's length used by the rounds at its ends
    for k in 0..n {
        let points = count.get(k).copied().unwrap_or(1).max(1);
        let (prev, here, next) = (p(k + n - 1), p(k), p(k + 1));
        let r = pts[k].r;
        if r <= SHARP || points < 2 {
            out.extend(std::iter::repeat_n(here, points));
            continue;
        }
        let unit = |d: [f64; 2]| {
            let l = d[0].hypot(d[1]);
            [d[0] / l, d[1] / l]
        };
        let u = unit([here[0] - prev[0], here[1] - prev[1]]);
        let w = unit([next[0] - here[0], next[1] - here[1]]);
        let turn = u[0] * w[1] - u[1] * w[0];
        if turn.abs() < 1e-9 {
            return Err(Error(format!("vertex {k} can't be rounded: it is on a line")));
        }
        // Tangent length r / tan(half the corner angle), as primlib's _round_corners.
        let half = (-(u[0] * w[0] + u[1] * w[1])).clamp(-1.0, 1.0).acos() / 2.0;
        let t = r / half.tan();
        taken[(k + n - 1) % n] += t;
        taken[k] += t;
        let p1 = [here[0] - u[0] * t, here[1] - u[1] * t];
        // The centre is r from p1, to the inside of the turn.
        let side = turn.signum();
        let c = [p1[0] - u[1] * r * side, p1[1] + u[0] * r * side];
        let a0 = (p1[1] - c[1]).atan2(p1[0] - c[0]);
        let sweep = (std::f64::consts::PI - 2.0 * half) * side;
        for s in 0..points {
            let a = a0 + sweep * s as f64 / (points - 1) as f64;
            out.push([c[0] + r * a.cos(), c[1] + r * a.sin()]);
        }
    }
    for (i, &used) in taken.iter().enumerate() {
        if used > len(p(i), p(i + 1)) + 1e-9 {
            return Err(Error(format!("the rounds at vertices {i} and {} don't fit on the edge between", (i + 1) % n)));
        }
    }
    Ok(out)
}

/// How many points each vertex becomes: 1 if it is sharp in every one of
/// `sections` (a loft's must match), else an arc's, by its largest radius
/// and the angle it turns.
pub fn arc_points(sections: &[&[Vertex]]) -> Vec<usize> {
    let n = sections[0].len();
    (0..n)
        .map(|k| {
            sections
                .iter()
                .filter(|s| s[k].r > SHARP)
                .map(|s| {
                    let m = s.len();
                    let (prev, here, next) = (s[(k + m - 1) % m], s[k], s[(k + 1) % m]);
                    let a1 = (here.b - prev.b).atan2(here.a - prev.a);
                    let a2 = (next.b - here.b).atan2(next.a - here.a);
                    let mut turn = (a2 - a1).abs();
                    if turn > std::f64::consts::PI {
                        turn = TAU - turn;
                    }
                    ((segments(s[k].r) as f64 * turn / TAU).ceil() as usize).max(1) + 1
                })
                .max()
                .unwrap_or(1)
        })
        .collect()
}

/// Twice the signed area: positive when anticlockwise.
fn area2(pts: &[[f64; 2]]) -> f64 {
    (0..pts.len())
        .map(|i| {
            let (a, b) = (pts[i], pts[(i + 1) % pts.len()]);
            a[0] * b[1] - b[0] * a[1]
        })
        .sum()
}

/// `pts` without a point equal to the one before it.
fn dedup(pts: &[[f64; 2]]) -> Vec<[f64; 2]> {
    let mut out: Vec<[f64; 2]> = Vec::with_capacity(pts.len());
    for &p in pts {
        if out.last().is_none_or(|q| (q[0] - p[0]).hypot(q[1] - p[1]) > 1e-9) {
            out.push(p);
        }
    }
    while out.len() > 1 && (out[0][0] - out[out.len() - 1][0]).hypot(out[0][1] - out[out.len() - 1][1]) <= 1e-9 {
        out.pop();
    }
    out
}

fn is_convex(pts: &[[f64; 2]]) -> bool {
    let n = pts.len();
    (0..n).all(|i| {
        let (a, b, c) = (pts[i], pts[(i + 1) % n], pts[(i + 2) % n]);
        (b[0] - a[0]) * (c[1] - b[1]) - (b[1] - a[1]) * (c[0] - b[0]) >= -1e-12
    })
}

/// An anticlockwise simple polygon as convex pieces: itself if convex,
/// else triangles by ear clipping.
pub fn convex_pieces(pts: &[[f64; 2]]) -> Vec<Vec<[f64; 2]>> {
    let pts = dedup(pts);
    if pts.len() < 3 {
        return vec![];
    }
    if is_convex(&pts) {
        return vec![pts];
    }
    let cross = |a: [f64; 2], b: [f64; 2], c: [f64; 2]| (b[0] - a[0]) * (c[1] - a[1]) - (b[1] - a[1]) * (c[0] - a[0]);
    let mut idx: Vec<usize> = (0..pts.len()).collect();
    let mut out = vec![];
    let mut guard = 0;
    while idx.len() > 3 && guard < 10 * pts.len() * pts.len() {
        guard += 1;
        let m = idx.len();
        let mut clipped = false;
        for i in 0..m {
            let (ia, ib, ic) = (idx[(i + m - 1) % m], idx[i], idx[(i + 1) % m]);
            let (a, b, c) = (pts[ia], pts[ib], pts[ic]);
            let k = cross(a, b, c);
            if k <= 1e-12 {
                if k.abs() <= 1e-12 && (b[0] - a[0]) * (c[0] - b[0]) + (b[1] - a[1]) * (c[1] - b[1]) > 0.0 {
                    // b is on the line from a to c: drop it.
                    idx.remove(i);
                    clipped = true;
                    break;
                }
                continue;
            }
            let inside = idx.iter().any(|&j| {
                if j == ia || j == ib || j == ic {
                    return false;
                }
                let p = pts[j];
                cross(a, b, p) >= -1e-12 && cross(b, c, p) >= -1e-12 && cross(c, a, p) >= -1e-12
            });
            if !inside {
                out.push(vec![a, b, c]);
                idx.remove(i);
                clipped = true;
                break;
            }
        }
        if !clipped {
            break;
        }
    }
    if idx.len() == 3 {
        out.push(idx.iter().map(|&i| pts[i]).collect());
    }
    out
}

/// A solid of `polys`, turned to face out if it was made inside out.
fn outward(polys: Vec<Polygon>) -> Solid {
    let s = Solid::new(polys);
    if s.volume() < 0.0 {
        let mut s = s;
        for p in &mut s.polys {
            p.verts.reverse();
            p.plane.n = -p.plane.n;
            p.plane.w = -p.plane.w;
        }
        s
    } else {
        s
    }
}

/// The axes of a profile's plane: (a, b, normal).
pub fn plane_axes(plane: &str) -> Result<(usize, usize, usize), Error> {
    match plane {
        "xy" => Ok((0, 1, 2)),
        "yz" => Ok((1, 2, 0)),
        "zx" => Ok((2, 0, 1)),
        _ => Err(Error(format!("plane {plane:?}"))),
    }
}

fn place((a, b, n): (usize, usize, usize), p: [f64; 2], h: f64) -> V3 {
    V3::default().with(a, p[0]).with(b, p[1]).with(n, h)
}

/// Faces of a cap: `pts` at height `h`, facing +normal when `up`.
fn cap(axes: (usize, usize, usize), pts: &[[f64; 2]], h: f64, up: bool, out: &mut Vec<Polygon>) {
    let mut pts = pts.to_vec();
    if area2(&pts) < 0.0 {
        pts.reverse();
    }
    for piece in convex_pieces(&pts) {
        let mut vs: Vec<V3> = piece.iter().map(|&p| place(axes, p, h)).collect();
        if !up {
            vs.reverse();
        }
        out.extend(Polygon::new(vs));
    }
}

/// Sides joining two outlines of equal length, ruled: each pair of edges a
/// quad if flat, else two triangles.
fn sides(lo: &[V3], hi: &[V3], out: &mut Vec<Polygon>) {
    let n = lo.len();
    for i in 0..n {
        let j = (i + 1) % n;
        quad(lo[i], lo[j], hi[j], hi[i], out);
    }
}

fn dedup3(pts: &[V3]) -> Vec<V3> {
    let mut out: Vec<V3> = vec![];
    for &p in pts {
        if out.last().is_none_or(|q| (*q - p).len() > 1e-9) {
            out.push(p);
        }
    }
    while out.len() > 1 && (out[0] - out[out.len() - 1]).len() <= 1e-9 {
        out.pop();
    }
    out
}

/// The polygon `pts` in `plane`, extruded along its normal over `span`.
pub fn prism(pts: &[Vertex], plane: &str, (lo, hi): (f64, f64)) -> Result<Solid, Error> {
    loft(&[(pts, lo), (pts, hi)], plane)
}

/// A ruled loft through `sections`, each a polygon in `plane` at a height
/// along its normal; their vertices correspond, rounds included.
pub fn loft(sections: &[(&[Vertex], f64)], plane: &str) -> Result<Solid, Error> {
    let axes = plane_axes(plane)?;
    if sections.len() < 2 {
        return Err(Error("a loft of fewer than two sections".into()));
    }
    let n = sections[0].0.len();
    if sections.iter().any(|(s, _)| s.len() != n) {
        return Err(Error("a loft's sections have different numbers of vertices".into()));
    }
    let arcs = arc_points(&sections.iter().map(|(s, _)| *s).collect::<Vec<_>>());
    let outlines = sections.iter().map(|(s, _)| outline(s, &arcs)).collect::<Result<Vec<_>, _>>()?;
    let mut polys = vec![];
    let first = sections[0].1;
    let last = sections[sections.len() - 1].1;
    cap(axes, &outlines[0], first, first > last, &mut polys);
    cap(axes, &outlines[outlines.len() - 1], last, last > first, &mut polys);
    // Sides wind with the outline anticlockwise and heights rising; outward() fixes the rest.
    for k in 0..sections.len() - 1 {
        let lo: Vec<V3> = outlines[k].iter().map(|&p| place(axes, p, sections[k].1)).collect();
        let hi: Vec<V3> = outlines[k + 1].iter().map(|&p| place(axes, p, sections[k + 1].1)).collect();
        let (lo, hi) = if area2(&outlines[0]) < 0.0 {
            (lo.into_iter().rev().collect::<Vec<_>>(), hi.into_iter().rev().collect::<Vec<_>>())
        } else {
            (lo, hi)
        };
        if sections[k + 1].1 > sections[k].1 {
            sides(&lo, &hi, &mut polys);
        } else {
            sides(&hi, &lo, &mut polys);
        }
    }
    let s = outward(polys);
    if s.volume().abs() < 1e-12 {
        return Err(Error("the loft has no volume".into()));
    }
    Ok(s)
}

/// The (r, z) polygon `pts` turned about the z axis from `span.0` to
/// `span.1` degrees (anticlockwise seen from +z), its radii grown so each
/// step's wedge has the true wedge's volume.
pub fn revolve(pts: &[Vertex], (from, to): (f64, f64)) -> Result<Solid, Error> {
    let sweep = to - from;
    if sweep.abs() < 1e-9 {
        return Err(Error("a revolve through no angle".into()));
    }
    let profile = outline(pts, &arc_points(&[pts]))?;
    let r_max = profile.iter().fold(0.0f64, |m, p| m.max(p[0]));
    let full = (sweep.abs() - 360.0).abs() < 1e-9;
    let n_full = segments(r_max);
    let steps = if full { n_full } else { ((n_full as f64 * sweep.abs() / 360.0).ceil() as usize).max(1) };
    let rings = (0..=steps).map(|i| (profile.clone(), from + sweep * i as f64 / steps as f64)).collect();
    about_z(rings, full).map_err(|e| Error(format!("the revolve {e}")))
}

/// A smooth loft through `sections`, each an (r, z) polygon in the
/// half-plane at its angle (degrees) about z; their vertices correspond,
/// rounds included. Between sections each point follows a Catmull-Rom
/// curve in (r, z, angle), in steps as fine as a revolve's.
pub fn loft_about(sections: &[(&[Vertex], f64)]) -> Result<Solid, Error> {
    if sections.len() < 2 {
        return Err(Error("a loft of fewer than two sections".into()));
    }
    let n = sections[0].0.len();
    if sections.iter().any(|(s, _)| s.len() != n) {
        return Err(Error("a loft's sections have different numbers of vertices".into()));
    }
    let arcs = arc_points(&sections.iter().map(|(s, _)| *s).collect::<Vec<_>>());
    let outlines = sections.iter().map(|(s, _)| outline(s, &arcs)).collect::<Result<Vec<_>, _>>()?;
    let r_max = outlines.iter().flatten().fold(0.0f64, |m, p| m.max(p[0]));
    let per_degree = segments(r_max) as f64 / 360.0;
    let last = sections.len() - 1;
    // Catmull-Rom between k and k + 1, its ends' neighbours mirrored.
    let at = |k: usize, t: f64, f: &dyn Fn(usize) -> f64| {
        let p1 = f(k);
        let p2 = f(k + 1);
        let p0 = if k == 0 { 2.0 * p1 - p2 } else { f(k - 1) };
        let p3 = if k + 1 == last { 2.0 * p2 - p1 } else { f(k + 2) };
        let (t2, t3) = (t * t, t * t * t);
        0.5 * (2.0 * p1
            + (p3 - p0) * 0.0
            + (-p0 + p2) * t
            + (2.0 * p0 - 5.0 * p1 + 4.0 * p2 - p3) * t2
            + (-p0 + 3.0 * p1 - 3.0 * p2 + p3) * t3)
    };
    let mut rings = vec![(outlines[0].clone(), sections[0].1)];
    for k in 0..last {
        let span = (sections[k + 1].1 - sections[k].1).abs();
        let steps = ((span * per_degree).ceil() as usize).max(1);
        for s in 1..=steps {
            let t = s as f64 / steps as f64;
            let angle = at(k, t, &|i| sections[i].1);
            let pts = (0..outlines[0].len())
                .map(|j| [at(k, t, &|i| outlines[i][j][0]), at(k, t, &|i| outlines[i][j][1])])
                .collect();
            rings.push((pts, angle));
        }
    }
    about_z(rings, false).map_err(|e| Error(format!("the loft {e}")))
}

/// The solid through `rings`, each an (r, z) outline in the half-plane at
/// its angle (degrees) about z, joined in order: closed round (`full`, the
/// last ring the first again) or with the first and last as ends. Radii
/// are grown by the mean step's area-true factor.
fn about_z(rings: Vec<(Vec<[f64; 2]>, f64)>, full: bool) -> Result<Solid, Error> {
    let first = &rings[0].0;
    // Anticlockwise in (r, z), so the ends and sides below wind as one.
    let flip = area2(first) < 0.0;
    if rings.iter().flat_map(|(o, _)| o).any(|p| p[0] < -1e-9) {
        return Err(Error("crosses the axis".into()));
    }
    let steps = rings.len() - 1;
    let mean = ((rings[steps].1 - rings[0].1) / steps as f64).to_radians().abs();
    let k = if mean > 1e-12 { (mean / mean.sin()).sqrt() } else { 1.0 };
    let place = |p: [f64; 2], angle: f64| {
        let a = angle.to_radians();
        v3(p[0] * k * a.cos(), p[0] * k * a.sin(), p[1])
    };
    let ring3: Vec<Vec<V3>> = rings
        .iter()
        .map(|(o, a)| {
            let mut v: Vec<V3> = o.iter().map(|&p| place(p, *a)).collect();
            if flip {
                v.reverse();
            }
            v
        })
        .collect();
    let mut polys = vec![];
    let m = first.len();
    for i in 0..steps {
        let (lo, hi) = (&ring3[i], &ring3[i + 1]);
        for e in 0..m {
            let f = (e + 1) % m;
            quad(lo[f], lo[e], hi[e], hi[f], &mut polys);
        }
    }
    if !full {
        // The ends: the outline in its half-planes.
        for (end, (outline, angle)) in [(false, &rings[0]), (true, &rings[steps])] {
            let mut o = dedup(outline);
            if flip {
                o.reverse();
            }
            for piece in convex_pieces(&o) {
                let mut vs: Vec<V3> = piece.iter().map(|&p| place(p, *angle)).collect();
                if end {
                    vs.reverse();
                }
                polys.extend(Polygon::new(dedup3(&vs)));
            }
        }
    }
    let s = outward(polys);
    if s.volume().abs() < 1e-12 {
        return Err(Error("has no volume".into()));
    }
    Ok(s)
}

/// The face a, b, c, d: one polygon if flat, else four triangles about its
/// centre, which lies on the ruled (bilinear) surface through the corners;
/// fewer corners where some are one point.
fn quad(a: V3, b: V3, c: V3, d: V3, out: &mut Vec<Polygon>) {
    let face = dedup3(&[a, b, c, d]);
    if face.len() == 4 {
        // Flat when c is on the plane of a, b and d.
        let nrm = (b - a).cross(d - a);
        if nrm.len() > 1e-12 && nrm.unit().dot(c - a).abs() <= 1e-9 {
            out.extend(Polygon::new(face));
        } else {
            let m = (a + b + c + d) * 0.25;
            for (p, q) in [(a, b), (b, c), (c, d), (d, a)] {
                out.extend(Polygon::new(vec![p, q, m]));
            }
        }
    } else if face.len() == 3 {
        out.extend(Polygon::new(face));
    }
}

/// A sphere: a polyhedron of latitude bands, scaled to the sphere's volume.
pub fn sphere(c: V3, r: f64) -> Result<Solid, Error> {
    if r <= 0.0 {
        return Err(Error(format!("a sphere of radius {r}")));
    }
    let n = segments(r);
    let bands = n / 2;
    let pt = |i: usize, j: usize| {
        let lat = std::f64::consts::PI * (i as f64 / bands as f64 - 0.5);
        let lon = TAU * j as f64 / n as f64;
        v3(lat.cos() * lon.cos(), lat.cos() * lon.sin(), lat.sin())
    };
    let mut polys = vec![];
    for i in 0..bands {
        for j in 0..n {
            let face = dedup3(&[pt(i, j), pt(i, j + 1), pt(i + 1, j + 1), pt(i + 1, j)]);
            polys.extend(Polygon::new(face));
        }
    }
    let unit = outward(polys);
    let k = (TAU * 2.0 / 3.0 / unit.volume()).cbrt() * r;
    let mut s = Solid::new(vec![]);
    for p in unit.polys {
        s.polys.extend(Polygon::new(p.verts.iter().map(|&v| c + v * k).collect()));
    }
    Ok(s)
}

/// A cylinder sector about z over the z `span`: radius `r`, from `start`
/// degrees through `angle`.
pub fn sector(r: f64, span: (f64, f64), start: f64, angle: f64) -> Result<Solid, Error> {
    if angle.abs() >= 360.0 - 1e-9 {
        let pts: Vec<Vertex> = circle(r);
        return prism(&pts, "xy", span);
    }
    let n = segments(r);
    let steps = ((n as f64 * angle.abs() / 360.0).ceil() as usize).max(1);
    let step = angle.to_radians() / steps as f64;
    let k = (step.abs() / step.abs().sin()).sqrt();
    let mut pts = vec![Vertex { a: 0.0, b: 0.0, r: 0.0 }];
    for i in 0..=steps {
        let a = start.to_radians() + step * i as f64;
        pts.push(Vertex { a: r * k * a.cos(), b: r * k * a.sin(), r: 0.0 });
    }
    prism(&pts, "xy", span)
}

/// A regular prism about z over the z `span`, `sides` corners on a circle
/// of radius `r`, one on +x.
pub fn polygon(sides: usize, r: f64, span: (f64, f64)) -> Result<Solid, Error> {
    if sides < 3 {
        return Err(Error(format!("a polygon of {sides} sides")));
    }
    let pts: Vec<Vertex> = (0..sides)
        .map(|i| {
            let a = TAU * i as f64 / sides as f64;
            Vertex { a: r * a.cos(), b: r * a.sin(), r: 0.0 }
        })
        .collect();
    prism(&pts, "xy", span)
}

fn circle(r: f64) -> Vec<Vertex> {
    let n = segments(r);
    let k = area_true(n);
    (0..n)
        .map(|i| {
            let a = TAU * i as f64 / n as f64;
            Vertex { a: r * k * a.cos(), b: r * k * a.sin(), r: 0.0 }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn vx(pts: &[(f64, f64)]) -> Vec<Vertex> {
        pts.iter().map(|&(a, b)| Vertex { a, b, r: 0.0 }).collect()
    }

    fn close(a: f64, b: f64, rel: f64) -> bool {
        (a - b).abs() <= rel * a.abs().max(b.abs())
    }

    fn closed(s: &Solid) -> bool {
        let v = s.volume();
        close(s.volume_about(v3(50.0, -20.0, 7.0)), v, 1e-9)
    }

    #[test]
    fn an_l_extruded_each_way() {
        let l = vx(&[(0.0, 0.0), (3.0, 0.0), (3.0, 1.0), (1.0, 1.0), (1.0, 3.0), (0.0, 3.0)]);
        for plane in ["xy", "yz", "zx"] {
            for (pts, span) in [(l.clone(), (0.0, 2.0)), (l.iter().rev().copied().collect(), (2.0, -1.0))] {
                let s = prism(&pts, plane, span).unwrap();
                let h = (span.1 - span.0).abs();
                assert!(close(s.volume(), 5.0 * h, 1e-9) && closed(&s), "{plane} {span:?}: {}", s.volume());
            }
        }
        let (lo, hi) = prism(&l, "yz", (0.0, 2.0)).unwrap().bounds().unwrap();
        assert_eq!((lo, hi), (v3(0.0, 0.0, 0.0), v3(2.0, 3.0, 3.0)));
    }

    #[test]
    fn rounded_corners() {
        // A 10 × 10 square, one corner rounded by 2: less the corner's 4 - π.
        let mut sq = vx(&[(0.0, 0.0), (10.0, 0.0), (10.0, 10.0), (0.0, 10.0)]);
        sq[2].r = 2.0;
        let s = prism(&sq, "xy", (0.0, 1.0)).unwrap();
        // Arcs are chords inside the circle: a little under (0.02% here).
        let want = 100.0 - (4.0 - std::f64::consts::PI);
        assert!(close(s.volume(), want, 1e-3) && s.volume() < want && closed(&s), "{}", s.volume());
        // A concave corner rounded adds.
        let mut l = vx(&[(0.0, 0.0), (4.0, 0.0), (4.0, 2.0), (2.0, 2.0), (2.0, 4.0), (0.0, 4.0)]);
        l[3].r = 1.0;
        let s = prism(&l, "xy", (0.0, 1.0)).unwrap();
        assert!(close(s.volume(), 12.0 + (1.0 - std::f64::consts::PI / 4.0), 1e-3) && closed(&s));
        // Too big to fit.
        sq[2].r = 20.0;
        assert!(prism(&sq, "xy", (0.0, 1.0)).is_err());
    }

    #[test]
    fn a_loft_of_squares() {
        let big = vx(&[(0.0, 0.0), (2.0, 0.0), (2.0, 2.0), (0.0, 2.0)]);
        let small = vx(&[(0.5, 0.5), (1.5, 0.5), (1.5, 1.5), (0.5, 1.5)]);
        let s = loft(&[(&big, 0.0), (&small, 3.0)], "xy").unwrap();
        // A frustum of a pyramid: h/3 (A1 + A2 + √(A1 A2)).
        assert!(close(s.volume(), 1.0 * (4.0 + 1.0 + 2.0), 1e-9) && closed(&s));
    }

    #[test]
    fn revolved() {
        // A rectangle r 1..2, z 0..1 turned once: a tube.
        let p = vx(&[(1.0, 0.0), (2.0, 0.0), (2.0, 1.0), (1.0, 1.0)]);
        let full = revolve(&p, (0.0, 360.0)).unwrap();
        assert!(close(full.volume(), std::f64::consts::PI * 3.0, 1e-9) && closed(&full));
        let quarter = revolve(&p, (90.0, 180.0)).unwrap();
        assert!(close(quarter.volume(), std::f64::consts::PI * 3.0 / 4.0, 1e-9) && closed(&quarter));
        let (lo, hi) = quarter.bounds().unwrap();
        assert!(hi.x.abs() < 1e-9 && lo.y.abs() < 1e-9 && lo.x < -1.9 && hi.y > 1.9);
        // Touching the axis: a cone.
        let cone = revolve(&vx(&[(0.0, 0.0), (1.0, 0.0), (0.0, 1.0)]), (0.0, 360.0)).unwrap();
        assert!(close(cone.volume(), std::f64::consts::PI / 3.0, 1e-9) && closed(&cone));
    }

    #[test]
    fn lofted_about() {
        // Like sections are a revolve.
        let p = vx(&[(1.0, 0.0), (2.0, 0.0), (2.0, 1.0), (1.0, 1.0)]);
        let s = loft_about(&[(&p, 0.0), (&p, 30.0), (&p, 60.0), (&p, 90.0)]).unwrap();
        assert!(close(s.volume(), std::f64::consts::PI * 3.0 / 4.0, 1e-6) && closed(&s), "{}", s.volume());
        // Rising evenly: the same volume, moved up as it turns.
        let up = |z: f64| vx(&[(1.0, z), (2.0, z), (2.0, z + 1.0), (1.0, z + 1.0)]);
        let (a, b, c) = (up(0.0), up(0.5), up(1.0));
        let s = loft_about(&[(&a, 0.0), (&b, 45.0), (&c, 90.0)]).unwrap();
        assert!(close(s.volume(), std::f64::consts::PI * 3.0 / 4.0, 1e-3) && closed(&s), "{}", s.volume());
        let (lo, hi) = s.bounds().unwrap();
        assert!(lo.z.abs() < 1e-9 && (hi.z - 2.0).abs() < 1e-9);
    }

    #[test]
    fn round_things() {
        let s = sphere(v3(1.0, 2.0, 3.0), 2.0).unwrap();
        assert!(close(s.volume(), 4.0 / 3.0 * std::f64::consts::PI * 8.0, 1e-9) && closed(&s));
        let q = sector(2.0, (0.0, 1.0), 0.0, 90.0).unwrap();
        assert!(close(q.volume(), std::f64::consts::PI, 1e-9) && closed(&q));
        let hex = polygon(6, 1.0, (0.0, 1.0)).unwrap();
        assert!(close(hex.volume(), 3.0 * 3f64.sqrt() / 2.0, 1e-9));
        assert!(close(hex.bounds().unwrap().1.x, 1.0, 1e-12));
    }
}
