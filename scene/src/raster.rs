//! Drawing on the CPU: triangles with a depth buffer and flat shading by a
//! light from just above and left of the eye, and lines over them. Pixels are premultiplied BGRA,
//! opaque, as a dma-buf in `AR24` wants them.

use crate::camera::Camera;
use crate::math::Vec3;
use crate::{Lines, Mesh};

/// What a frame is drawn into.
#[derive(Debug, Clone)]
pub struct Frame {
    pub width: u32,
    pub height: u32,
    /// BGRA, row by row, no padding.
    pub pixels: Vec<u8>,
    /// 1/depth per pixel; 0 is nothing drawn.
    depth: Vec<f32>,
}

/// How a frame is drawn.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Style {
    pub background: [u8; 3],
    /// Fill the triangles (off: lines only, for wireframe).
    pub fill: bool,
}

impl Default for Style {
    fn default() -> Style {
        Style { background: [22, 22, 30], fill: true }
    }
}

/// The light every face gets, whichever way it faces.
const AMBIENT: f32 = 0.22;
/// Where the light is, off the view direction: above and to the left.
const LIGHT_ABOVE: f32 = 0.5;
const LIGHT_LEFT: f32 = 0.3;
/// How far in front of a surface lines on it are drawn, as a fraction of
/// depth, so edges aren't hidden by the faces they bound.
const LINE_BIAS: f32 = 2e-3;

impl Frame {
    pub fn new(width: u32, height: u32) -> Frame {
        let n = width as usize * height as usize;
        Frame { width, height, pixels: vec![0; n * 4], depth: vec![0.0; n] }
    }

    /// The pixel at `(x, y)` as RGB.
    pub fn rgb(&self, x: u32, y: u32) -> [u8; 3] {
        let o = (y * self.width + x) as usize * 4;
        [self.pixels[o + 2], self.pixels[o + 1], self.pixels[o]]
    }

    /// Draw `meshes` and `lines` as `camera` sees them.
    pub fn render(&mut self, camera: &Camera, meshes: &[Mesh], lines: &[Lines], style: &Style) {
        let [r, g, b] = style.background;
        for p in self.pixels.as_chunks_mut::<4>().0 {
            *p = [b, g, r, 255];
        }
        self.depth.fill(0.0);
        if style.fill {
            for m in meshes {
                for (t, n) in m.triangles.iter().zip(&m.normals) {
                    self.triangle(camera, t, *n, m.color);
                }
            }
        }
        for l in lines {
            for s in &l.segments {
                self.line(camera, *s, l.color);
            }
        }
    }

    fn triangle(&mut self, camera: &Camera, t: &[Vec3; 3], normal: Vec3, color: [u8; 3]) {
        let c = t.map(|p| camera.to_camera(p));
        // Lit from just above and left of the eye, from either side: a flat
        // face is one shade, and the three faces of a box seen isometrically
        // are three.
        let (right, up, forward) = camera.axes();
        let towards = (forward + up * LIGHT_ABOVE - right * LIGHT_LEFT).normalized();
        let light = AMBIENT + (1.0 - AMBIENT) * normal.dot(towards).abs();
        let rgb = color.map(|v| (v as f32 * light).round().min(255.0) as u8);
        let poly = clip_near(&c, camera.near());
        let (w, h) = (self.width as f32, self.height as f32);
        let s: Vec<(f32, f32, f32)> = poly.iter().map(|&v| camera.project(v, w, h)).collect();
        for i in 1..s.len().saturating_sub(1) {
            self.fill([s[0], s[i], s[i + 1]], rgb);
        }
    }

    /// A screen-space triangle: pixel x, y and depth at each corner.
    fn fill(&mut self, v: [(f32, f32, f32); 3], [r, g, b]: [u8; 3]) {
        let edge =
            |a: (f32, f32, f32), b: (f32, f32, f32), x: f32, y: f32| (b.0 - a.0) * (y - a.1) - (b.1 - a.1) * (x - a.0);
        let area = edge(v[0], v[1], v[2].0, v[2].1);
        if area.abs() < 1e-9 {
            return;
        }
        let x0 = v.iter().map(|p| p.0).fold(f32::INFINITY, f32::min).floor().max(0.0) as u32;
        let x1 =
            (v.iter().map(|p| p.0).fold(f32::NEG_INFINITY, f32::max).ceil() as i64).clamp(0, self.width as i64) as u32;
        let y0 = v.iter().map(|p| p.1).fold(f32::INFINITY, f32::min).floor().max(0.0) as u32;
        let y1 =
            (v.iter().map(|p| p.1).fold(f32::NEG_INFINITY, f32::max).ceil() as i64).clamp(0, self.height as i64) as u32;
        let inv = v.map(|p| 1.0 / p.2);
        for y in y0..y1 {
            let py = y as f32 + 0.5;
            for x in x0..x1 {
                let px = x as f32 + 0.5;
                let b0 = edge(v[1], v[2], px, py) / area;
                let b1 = edge(v[2], v[0], px, py) / area;
                let b2 = edge(v[0], v[1], px, py) / area;
                if b0 < 0.0 || b1 < 0.0 || b2 < 0.0 {
                    continue;
                }
                let z = b0 * inv[0] + b1 * inv[1] + b2 * inv[2];
                let i = (y * self.width + x) as usize;
                if z > self.depth[i] {
                    self.depth[i] = z;
                    self.pixels[i * 4..i * 4 + 4].copy_from_slice(&[b, g, r, 255]);
                }
            }
        }
    }

    fn line(&mut self, camera: &Camera, [a, b]: [Vec3; 2], [r, g, bl]: [u8; 3]) {
        let (mut a, mut b) = (camera.to_camera(a), camera.to_camera(b));
        let near = camera.near();
        if a.z < near && b.z < near {
            return;
        }
        // Clipped at the near plane.
        if a.z < near || b.z < near {
            let t = (near - a.z) / (b.z - a.z);
            let m = a + (b - a) * t;
            if a.z < near { a = m } else { b = m }
        }
        let (w, h) = (self.width as f32, self.height as f32);
        let (pa, pb) = (camera.project(a, w, h), camera.project(b, w, h));
        let steps = (pb.0 - pa.0).abs().max((pb.1 - pa.1).abs()).ceil().clamp(1.0, 16384.0) as u32;
        let (ia, ib) = (1.0 / pa.2, 1.0 / pb.2);
        for k in 0..=steps {
            let t = k as f32 / steps as f32;
            let (x, y) = (pa.0 + (pb.0 - pa.0) * t, pa.1 + (pb.1 - pa.1) * t);
            if x < 0.0 || y < 0.0 || x >= w || y >= h {
                continue;
            }
            let i = (y as u32 * self.width + x as u32) as usize;
            let z = (ia + (ib - ia) * t) * (1.0 + LINE_BIAS);
            if z >= self.depth[i] {
                self.depth[i] = z;
                self.pixels[i * 4..i * 4 + 4].copy_from_slice(&[bl, g, r, 255]);
            }
        }
    }
}

/// Triangle `c` (camera space) clipped to `z >= near`: 0, 3 or 4 corners.
fn clip_near(c: &[Vec3; 3], near: f32) -> Vec<Vec3> {
    let mut out = Vec::with_capacity(4);
    for i in 0..3 {
        let (a, b) = (c[i], c[(i + 1) % 3]);
        if a.z >= near {
            out.push(a);
        }
        if (a.z >= near) != (b.z >= near) {
            out.push(a + (b - a) * ((near - a.z) / (b.z - a.z)));
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::camera::View;

    fn front(distance: f32) -> Camera {
        let mut c = Camera { distance, ..Camera::default() };
        c.view(View::Front);
        c
    }

    /// A square in the plane `y`, facing −Y (the front camera), `half` wide.
    fn square(y: f32, half: f32, color: [u8; 3]) -> Mesh {
        let p = |x: f32, z: f32| Vec3::new(x, y, z);
        let mut m = Mesh::from_triangles(vec![
            [p(-half, -half), p(half, -half), p(half, half)],
            [p(-half, -half), p(half, half), p(-half, half)],
        ]);
        m.color = color;
        m
    }

    const BG: [u8; 3] = [22, 22, 30];

    #[test]
    fn a_face_seen_square_on_is_well_lit() {
        let mut f = Frame::new(40, 30);
        f.render(&front(10.0), &[square(0.0, 1.0, [200, 100, 50])], &[], &Style::default());
        let [r, g, b] = f.rgb(20, 15);
        assert!(r > 170 && r <= 200 && g > 85 && b > 40, "nearly full light head on: {r} {g} {b}");
        assert_eq!(f.rgb(0, 0), BG);
        assert_eq!(&f.pixels[3..4], &[255], "opaque");
    }

    #[test]
    fn the_nearer_face_wins_in_any_order() {
        let near = square(-2.0, 1.0, [0, 200, 0]);
        let far = square(2.0, 3.0, [200, 0, 0]);
        for order in [[near.clone(), far.clone()], [far.clone(), near.clone()]] {
            let mut f = Frame::new(40, 30);
            f.render(&front(10.0), &order, &[], &Style::default());
            let [r, g, _] = f.rgb(20, 15);
            assert!(g > 150 && r == 0, "green, the nearer, in the middle");
            let [r, g, _] = f.rgb(20, 3);
            assert!(r > 150 && g == 0, "red, the farther and bigger, around it");
        }
    }

    #[test]
    fn a_box_seen_isometrically_has_three_shades() {
        let p = |x: f32, y: f32, z: f32| Vec3::new(x, y, z);
        // Its front (−Y), right (+X) and top (+Z) faces, each a triangle.
        let faces = Mesh::from_triangles(vec![
            [p(-1.0, -1.0, -1.0), p(1.0, -1.0, -1.0), p(1.0, -1.0, 1.0)],
            [p(1.0, -1.0, -1.0), p(1.0, 1.0, -1.0), p(1.0, 1.0, 1.0)],
            [p(-1.0, -1.0, 1.0), p(1.0, -1.0, 1.0), p(1.0, 1.0, 1.0)],
        ]);
        let mut c = Camera::default();
        c.fit(faces.bounds());
        let shade = |i: usize| {
            let mut f = Frame::new(80, 60);
            let one = Mesh { triangles: vec![faces.triangles[i]], normals: vec![faces.normals[i]], ..faces.clone() };
            f.render(&c, &[one], &[], &Style::default());
            f.pixels.chunks(4).map(|p| p[2]).max().unwrap()
        };
        let (front, right, top) = (shade(0), shade(1), shade(2));
        assert!(front != right && right != top && top != front, "{front} {right} {top}");
    }

    #[test]
    fn faces_at_an_angle_are_darker_and_backs_are_lit() {
        let mut f = Frame::new(40, 30);
        let mut m = square(0.0, 1.0, [200, 200, 200]);
        m.normals.iter_mut().for_each(|n| *n = Vec3::new(0.0, 1.0, 1.0).normalized());
        f.render(&front(10.0), &[m], &[], &Style::default());
        let v = f.rgb(20, 15)[0];
        assert!(v > 100 && v < 200, "{v}");
    }

    #[test]
    fn triangles_through_the_eye_are_clipped() {
        let mut c = front(10.0);
        c.distance = 0.5; // the eye is inside the big square's plane
        let big = Mesh::from_triangles(vec![[
            Vec3::new(-50.0, -30.0, -50.0),
            Vec3::new(50.0, 30.0, -50.0),
            Vec3::new(0.0, 0.0, 50.0),
        ]]);
        let mut f = Frame::new(40, 30);
        f.render(&c, &[big], &[], &Style::default());
        assert!(f.pixels.chunks(4).any(|p| p[..3] != [30, 22, 22]), "some of it in front is drawn");
    }

    #[test]
    fn lines_show_over_the_faces_they_lie_on() {
        let mut f = Frame::new(40, 30);
        let edge =
            Lines { segments: vec![[Vec3::new(-1.0, 0.0, 0.0), Vec3::new(1.0, 0.0, 0.0)]], color: [255, 255, 0] };
        f.render(&front(10.0), &[square(0.0, 1.0, [50, 50, 50])], std::slice::from_ref(&edge), &Style::default());
        assert_eq!(f.rgb(20, 15), [255, 255, 0]);
        // Behind the face, hidden.
        let behind = Lines { segments: vec![[Vec3::new(-1.0, 1.0, 0.0), Vec3::new(1.0, 1.0, 0.0)]], ..edge };
        f.render(&front(10.0), &[square(0.0, 1.0, [50, 50, 50])], &[behind], &Style::default());
        assert_ne!(f.rgb(20, 15), [255, 255, 0]);
    }

    #[test]
    fn wireframe_draws_only_lines() {
        let mut f = Frame::new(40, 30);
        f.render(&front(10.0), &[square(0.0, 1.0, [200, 0, 0])], &[], &Style { fill: false, ..Style::default() });
        assert_eq!(f.rgb(20, 15), BG);
    }
}
