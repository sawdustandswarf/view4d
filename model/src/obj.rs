//! Wavefront OBJ, with its MTL materials for colour. A part is what an `o`
//! or `g` names; a part with faces in several materials becomes one mesh
//! per material. Faces with more than three corners are split into a fan
//! from the first. Normals are worked out from the winding, as for STL; the
//! file's `vn` and texture coordinates are not used.

use std::collections::HashMap;

use view4d_scene::{Mesh, Vec3};

use crate::{Error, Part};

/// The parts in `text`. `mtl` is asked for each `mtllib` file's text by
/// the name the OBJ gives (relative to the OBJ's directory); one it can't
/// give leaves its materials in the default colour.
pub fn parse(text: &str, mut mtl: impl FnMut(&str) -> Option<String>) -> Result<Vec<Part>, Error> {
    let mut vertices: Vec<Vec3> = Vec::new();
    let mut colors: HashMap<String, [u8; 3]> = HashMap::new();
    // Meshes by part and material, in the order they first appear.
    let mut parts: Vec<(String, Option<String>, Vec<[Vec3; 3]>)> = Vec::new();
    let mut index: HashMap<(String, Option<String>), usize> = HashMap::new();
    let (mut part, mut material) = (String::new(), None::<String>);

    for (n, line) in logical_lines(text) {
        let at = |what: String| Error(format!("line {n}: {what}"));
        let line = line.split('#').next().unwrap_or("");
        let mut words = line.split_ascii_whitespace();
        let Some(keyword) = words.next() else { continue };
        let rest = || line.trim_start()[keyword.len()..].trim();
        match keyword {
            "v" => {
                let mut c = [0f32; 3];
                for v in &mut c {
                    let w = words.next().ok_or_else(|| at("a vertex with fewer than three numbers".into()))?;
                    *v = w.parse().map_err(|_| at(format!("a vertex with `{w}` for a number")))?;
                }
                vertices.push(Vec3::new(c[0], c[1], c[2]));
            }
            "f" => {
                let corners = words
                    .map(|w| corner(w, vertices.len()).ok_or_else(|| at(format!("a face corner `{w}`"))))
                    .collect::<Result<Vec<usize>, Error>>()?;
                if corners.len() < 3 {
                    return Err(at(format!("a face with {} corners", corners.len())));
                }
                let key = (part.clone(), material.clone());
                let i = *index.entry(key).or_insert_with(|| {
                    parts.push((part.clone(), material.clone(), Vec::new()));
                    parts.len() - 1
                });
                let p = |i: usize| vertices[corners[i]];
                parts[i].2.extend((1..corners.len() - 1).map(|k| [p(0), p(k), p(k + 1)]));
            }
            "o" | "g" => part = rest().to_string(),
            "usemtl" => material = Some(rest().to_string()).filter(|m| !m.is_empty()),
            "mtllib" => {
                for file in words {
                    if let Some(text) = mtl(file) {
                        colors.extend(materials(&text));
                    }
                }
            }
            _ => {}
        }
    }
    Ok(parts
        .into_iter()
        .filter(|(_, _, t)| !t.is_empty())
        .map(|(name, material, triangles)| {
            let mut mesh = Mesh::from_triangles(triangles);
            if let Some(c) = material.and_then(|m| colors.get(&m).copied()) {
                mesh.color = c;
            }
            Part { name, mesh, edges: None }
        })
        .collect())
}

/// Lines with their numbers, a line ending in `\` joined to the next.
fn logical_lines(text: &str) -> impl Iterator<Item = (usize, String)> + '_ {
    let mut lines = text.lines().enumerate();
    std::iter::from_fn(move || {
        let (n, first) = lines.next()?;
        let mut line = first.to_string();
        while line.ends_with('\\') {
            line.pop();
            line.push(' ');
            match lines.next() {
                Some((_, more)) => line += more,
                None => break,
            }
        }
        Some((n + 1, line))
    })
}

/// The vertex a face corner (`v`, `v/vt`, `v//vn`, `v/vt/vn`) names, as an
/// index into the `count` vertices so far: from 1, or back from the latest
/// if negative.
fn corner(word: &str, count: usize) -> Option<usize> {
    let v: i64 = word.split('/').next()?.parse().ok()?;
    let i = if v < 0 { count as i64 + v } else { v - 1 };
    (0..count as i64).contains(&i).then_some(i as usize)
}

/// Each material's diffuse colour (`Kd`, 0–1 each) in `text`, an MTL file.
fn materials(text: &str) -> HashMap<String, [u8; 3]> {
    let mut colors = HashMap::new();
    let mut name = None;
    for line in text.lines() {
        let line = line.split('#').next().unwrap_or("").trim();
        let mut words = line.split_ascii_whitespace();
        match words.next() {
            Some("newmtl") => name = Some(line["newmtl".len()..].trim().to_string()),
            Some("Kd") => {
                let c: Vec<f32> = words.take(3).filter_map(|w| w.parse().ok()).collect();
                if let (Some(name), &[r, g, b]) = (&name, &c[..]) {
                    let byte = |v: f32| (v.clamp(0.0, 1.0) * 255.0).round() as u8;
                    colors.insert(name.clone(), [byte(r), byte(g), byte(b)]);
                }
            }
            _ => {}
        }
    }
    colors
}

#[cfg(test)]
mod tests {
    use super::*;

    const CUBE_AND_TRIANGLE: &str = "\
# two parts, the cube in two materials
mtllib parts.mtl missing.mtl
o cube
v 0 0 0
v 10 0 0
v 10 10 0
v 0 10 0
v 0 0 10
v 10 0 10
v 10 10 10
v 0 10 10
usemtl red
f 1 4 3 2
f 5 6 7 8
usemtl blue
f 1/1 2/2 6/3 5/4
f 2//1 3//1 7//1 6//1
f -5/1/1 -1/2/2 -2/3/3 -6/4/4
f 1 5 \\
  8 4
o triangle
usemtl gold   # in no MTL file
v 0 0 20
v 1 0 20
v 0 1 20
f -3 -2 -1
";

    const PARTS_MTL: &str = "newmtl red\nKd 1 0 0\nnewmtl blue\nKs 1 1 1\nKd 0 0.5 1.2\n";

    fn parts() -> Vec<Part> {
        parse(CUBE_AND_TRIANGLE, |f| (f == "parts.mtl").then(|| PARTS_MTL.to_string())).unwrap()
    }

    #[test]
    fn parts_and_colours() {
        let p = parts();
        let summary: Vec<_> = p.iter().map(|p| (p.name.as_str(), p.mesh.triangles.len(), p.mesh.color)).collect();
        assert_eq!(
            summary,
            [("cube", 4, [255, 0, 0]), ("cube", 8, [0, 128, 255]), ("triangle", 1, Mesh::DEFAULT_COLOR)]
        );
    }

    #[test]
    fn quads_split_and_wound_outwards() {
        let p = parts();
        // The cube's faces are wound outwards: every normal points away from its centre.
        let centre = Vec3::new(5.0, 5.0, 5.0);
        for m in [&p[0].mesh, &p[1].mesh] {
            for (t, n) in m.triangles.iter().zip(&m.normals) {
                assert!((t[0] - centre).dot(*n) > 0.0, "{t:?} {n:?}");
            }
        }
        assert_eq!(p[0].mesh.normals[0], Vec3::new(0.0, 0.0, -1.0));
        assert_eq!(p[2].mesh.triangles[0][2], Vec3::new(0.0, 1.0, 20.0), "negative indices count back");
    }

    #[test]
    fn errors() {
        let e = |text: &str| parse(text, |_| None).unwrap_err().0;
        assert_eq!(e("v 0 0 0\nv 1 0 0\nf 1 2"), "line 3: a face with 2 corners");
        assert_eq!(e("v 0 0 0\nv 1 0 0\nv 0 1 0\nf 1 2 4"), "line 4: a face corner `4`");
        assert_eq!(e("v 0 0 0\nv 1 0 0\nv 0 1 0\nf 1 2 -4"), "line 4: a face corner `-4`");
        assert_eq!(e("f 0 1 2"), "line 1: a face corner `0`");
        assert_eq!(e("v 0 0"), "line 1: a vertex with fewer than three numbers");
        assert_eq!(e("\nv 0 x 0"), "line 2: a vertex with `x` for a number");
        assert!(parse("# nothing\nv 1 2 3\n", |_| None).unwrap().is_empty());
    }
}
