//! A line of text as FreeCAD's Draft ShapeString makes it (primlib's
//! `text` operation): glyph outlines from the font file (termmd-font),
//! each advanced by the glyph's advance and the font's kerning, scaled so
//! that the capital M is `size` tall, its left margin taken away and the
//! anchor put where `justify` says. Contours inside an odd number of others
//! are holes. Curves are flattened to `TEXT_TOLERANCE`: a tenth of other
//! curves', since a letter is mostly curve and chords lose area by about
//! two thirds of their sag along it. Glyphs are hinted as FreeCAD loads
//! them (see `PPEM`) in their heights, advances and kerning, but not in
//! the font's own instructions: those move points by up to half a pixel,
//! about 0.06% of a letter's area.

use termmd_font::sfnt::{Font, OutlineSink};

use crate::Error;
use crate::build::TOLERANCE;

/// How far a flattened curve of a glyph may stand off the true one (mm).
pub const TEXT_TOLERANCE: f64 = TOLERANCE / 10.0;

/// The pixels per em at which FreeCAD's FT2FC loads glyphs.
const PPEM: f64 = 480.0;
use crate::csg::Solid;
use crate::profile::{self, Vertex};

/// A flat shape in the xy plane: closed contours (mm), at height `z`.
#[derive(Debug, Clone, PartialEq)]
pub struct Flat {
    pub contours: Vec<Vec<[f64; 2]>>,
    pub z: f64,
}

impl Flat {
    /// (xmin, ymin, xmax, ymax); None if empty.
    pub fn bounds(&self) -> Option<[f64; 4]> {
        let mut pts = self.contours.iter().flatten();
        let p = pts.next()?;
        Some(
            pts.fold([p[0], p[1], p[0], p[1]], |b, p| [b[0].min(p[0]), b[1].min(p[1]), b[2].max(p[0]), b[3].max(p[1])]),
        )
    }
}

/// Collects a glyph's contours in font units, curves flattened for a
/// scale of `scale` mm per unit.
struct Contours {
    scale: f64,
    pen: [f64; 2],
    out: Vec<Vec<[f64; 2]>>,
}

impl Contours {
    fn at(&self, x: f32, y: f32) -> [f64; 2] {
        [self.pen[0] + x as f64, self.pen[1] + y as f64]
    }

    fn last(&self) -> [f64; 2] {
        self.out.last().and_then(|c| c.last()).copied().unwrap_or(self.pen)
    }

    fn push(&mut self, p: [f64; 2]) {
        if let Some(c) = self.out.last_mut() {
            c.push(p);
        }
    }

    /// Steps for a curve that bows `bow` font units off its chord.
    fn steps(&self, bow: f64) -> usize {
        ((bow * self.scale / TEXT_TOLERANCE).sqrt().ceil() as usize).clamp(1, 128)
    }
}

fn len(a: [f64; 2]) -> f64 {
    a[0].hypot(a[1])
}

impl OutlineSink for Contours {
    fn move_to(&mut self, x: f32, y: f32) {
        let p = self.at(x, y);
        self.out.push(vec![p]);
    }

    fn line_to(&mut self, x: f32, y: f32) {
        let p = self.at(x, y);
        self.push(p);
    }

    fn quad_to(&mut self, cx: f32, cy: f32, x: f32, y: f32) {
        let (p0, c, p1) = (self.last(), self.at(cx, cy), self.at(x, y));
        let n = self.steps(len([p0[0] - 2.0 * c[0] + p1[0], p0[1] - 2.0 * c[1] + p1[1]]) / 4.0);
        for i in 1..=n {
            let t = i as f64 / n as f64;
            let u = 1.0 - t;
            self.push([
                u * u * p0[0] + 2.0 * u * t * c[0] + t * t * p1[0],
                u * u * p0[1] + 2.0 * u * t * c[1] + t * t * p1[1],
            ]);
        }
    }

    fn cubic_to(&mut self, c1x: f32, c1y: f32, c2x: f32, c2y: f32, x: f32, y: f32) {
        let (p0, c1, c2, p1) = (self.last(), self.at(c1x, c1y), self.at(c2x, c2y), self.at(x, y));
        let d1 = len([p0[0] - 2.0 * c1[0] + c2[0], p0[1] - 2.0 * c1[1] + c2[1]]);
        let d2 = len([c1[0] - 2.0 * c2[0] + p1[0], c1[1] - 2.0 * c2[1] + p1[1]]);
        let n = self.steps(d1.max(d2) * 0.75);
        for i in 1..=n {
            let t = i as f64 / n as f64;
            let u = 1.0 - t;
            let (a, b, c, d) = (u * u * u, 3.0 * u * u * t, 3.0 * u * t * t, t * t * t);
            self.push([a * p0[0] + b * c1[0] + c * c2[0] + d * p1[0], a * p0[1] + b * c1[1] + c * c2[1] + d * p1[1]]);
        }
    }

    fn close(&mut self) {}
}

/// The highest point of `c`'s outline in font units: the capital M's
/// height, which ShapeString scales to the size.
fn top_of(font: &Font<'_>, c: char) -> Option<f64> {
    let mut s = Contours { scale: 1.0, pen: [0.0, 0.0], out: vec![] };
    font.outline(font.glyph_index(c)?, &mut s);
    s.out.iter().flatten().map(|p| p[1]).reduce(f64::max)
}

/// `string` set in the font `data`, `size` the capital height (mm), the
/// anchor at `at` (x, y, z) placed by `justify`: an index into Top /
/// Middle / Bottom (3 apart) × Left / Center / Right.
pub fn line(data: &[u8], string: &str, size: f64, at: [f64; 3], justify: usize) -> Result<Flat, Error> {
    let font = Font::parse(data, 0).map_err(|e| Error(format!("the font: {e}")))?;
    if size <= 0.0 {
        return Err(Error(format!("text of size {size}")));
    }
    // FreeCAD loads each glyph hinted at 480 ppem (48 pt, ten times over):
    // x stays as drawn, but heights, advances and kerning come to whole
    // pixels. The cap height, which sets the scale, is rounded the same way.
    let px = PPEM / f64::from(font.metrics().units_per_em);
    let snap = |u: f64| (u * px).round() / px;
    let cap =
        top_of(&font, 'M').map(snap).filter(|&h| h > 0.0).ok_or_else(|| Error("the font has no capital M".into()))?;
    let scale = size / cap;
    let mut s = Contours { scale, pen: [0.0, 0.0], out: vec![] };
    let mut prev = None;
    for c in string.chars() {
        let Some(g) = font.glyph_index(c) else { continue };
        if let Some(p) = prev {
            s.pen[0] += snap(f64::from(font.kerning(p, g)));
        }
        font.outline(g, &mut s);
        s.pen[0] += snap(f64::from(font.advance(g)));
        prev = Some(g);
    }
    let mut contours: Vec<Vec<[f64; 2]>> = s
        .out
        .into_iter()
        .map(|c| dedup(c.into_iter().map(|p| [p[0] * scale, p[1] * scale]).collect()))
        .filter(|c| c.len() >= 3)
        .collect();
    let mut flat = Flat { contours: vec![], z: at[2] };
    let Some([x0, _, x1, _]) = (Flat { contours: contours.clone(), z: 0.0 }).bounds() else {
        return Ok(flat); // only spaces
    };
    // ShapeString's justification_vector: the left margin taken away, then
    // the anchor's place on the cap height and the width.
    let (v, h) = (justify / 3, justify % 3);
    if justify > 8 {
        return Err(Error(format!("justify {justify}: 0 to 8")));
    }
    let dx = -x0 - [0.0, (x1 - x0) / 2.0, x1 - x0][h];
    let dy = -[size, size / 2.0, 0.0][v];
    for c in &mut contours {
        for p in c.iter_mut() {
            *p = [p[0] + dx + at[0], p[1] + dy + at[1]];
        }
    }
    flat.contours = contours;
    Ok(flat)
}

fn dedup(mut c: Vec<[f64; 2]>) -> Vec<[f64; 2]> {
    c.dedup_by(|a, b| (a[0] - b[0]).hypot(a[1] - b[1]) < 1e-6);
    while c.len() > 1 && (c[0][0] - c[c.len() - 1][0]).hypot(c[0][1] - c[c.len() - 1][1]) < 1e-6 {
        c.pop();
    }
    c
}

/// Whether `p` is inside the closed polygon `c` (even-odd).
fn inside(p: [f64; 2], c: &[[f64; 2]]) -> bool {
    let mut is = false;
    for i in 0..c.len() {
        let (a, b) = (c[i], c[(i + 1) % c.len()]);
        if (a[1] > p[1]) != (b[1] > p[1]) && p[0] < a[0] + (p[1] - a[1]) / (b[1] - a[1]) * (b[0] - a[0]) {
            is = !is;
        }
    }
    is
}

/// `flat` extruded over z from `lo` to `hi`: each outer contour a prism,
/// less the prisms of the holes directly inside it.
pub fn extrude(flat: &Flat, (lo, hi): (f64, f64)) -> Result<Solid, Error> {
    let cs = &flat.contours;
    // Depth: how many others each lies inside (by a point of its own).
    let depth: Vec<usize> =
        (0..cs.len()).map(|i| (0..cs.len()).filter(|&j| j != i && inside(cs[i][0], &cs[j])).count()).collect();
    let prism = |c: &[[f64; 2]]| {
        let pts: Vec<Vertex> = c.iter().map(|p| Vertex { a: p[0], b: p[1], r: 0.0 }).collect();
        profile::prism(&pts, "xy", (lo, hi))
    };
    let mut solids = vec![];
    for (i, c) in cs.iter().enumerate() {
        if depth[i] % 2 == 1 {
            continue;
        }
        let holes: Vec<Solid> = (0..cs.len())
            .filter(|&j| depth[j] == depth[i] + 1 && inside(cs[j][0], c))
            .map(|j| prism(&cs[j]))
            .collect::<Result<_, _>>()?;
        solids.push(prism(c)?.subtract(Solid::union_all(holes)));
    }
    Ok(Solid::union_all(solids))
}

#[cfg(test)]
mod tests {
    use super::*;

    const BOLD: &str = "/usr/share/fonts/liberation/LiberationSans-Bold.ttf";

    fn area(c: &[[f64; 2]]) -> f64 {
        (0..c.len()).map(|i| c[i][0] * c[(i + 1) % c.len()][1] - c[(i + 1) % c.len()][0] * c[i][1]).sum::<f64>() / 2.0
    }

    /// As FreeCAD 1.x makes them (Draft ShapeString, Liberation Sans Bold,
    /// size 40, cap height reference, scaled to size): bounds and area.
    #[test]
    fn as_shapestring_makes_them() {
        let Ok(data) = std::fs::read(BOLD) else { return };
        let mut bad = 0;
        for (s, j, want, want_area) in [
            ("OAK", 4, [-63.1818, -20.6061, 63.1818, 20.6061], 2255.5189),
            ("OAK", 6, [0.0, -0.6061, 126.3636, 40.6061], 2255.5189),
            ("48", 4, [-31.0256, -20.6061, 31.0256, 20.6061], 1324.6419),
            ("AV", 6, [0.0, 0.0, 74.6572, 40.0], 1286.7107),
            ("To", 6, [0.0, -0.6061, 63.7652, 40.0], 1063.796),
            ("HH", 4, [-38.1326, -20.0, 38.1326, 20.0], 1582.735),
            ("LINE 3", 4, [-87.4848, -20.6061, 87.4848, 20.6061], 3060.2061),
        ] {
            let f = line(&data, s, 40.0, [0.0, 0.0, 0.0], j).unwrap();
            let b = f.bounds().unwrap();
            let off = b.iter().zip(want).map(|(a, b)| (a - b).abs()).fold(0.0, f64::max);
            // Holes wind against their outlines, so the signed sum is the area.
            let a = f.contours.iter().map(|c| area(c)).sum::<f64>().abs();
            let v = extrude(&f, (0.0, 2.0)).unwrap().volume() / 2.0;
            eprintln!(
                "{s} {j}: bounds off {off:.4}, area {:+.4}%, extruded {:+.4}%",
                (a / want_area - 1.0) * 100.0,
                (v / want_area - 1.0) * 100.0
            );
            // The rest is the font's own y hinting of points between the
            // pixel rows (H's crossbar), which needs a bytecode interpreter.
            bad += usize::from(off > 0.05 || (a / want_area - 1.0).abs() > 1e-3 || (v / want_area - 1.0).abs() > 1e-3);
        }
        assert_eq!(bad, 0);
    }
}
