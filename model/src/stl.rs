//! STL, binary and ASCII. Normals are worked out from each triangle's
//! winding: files often leave them zero or get them wrong. A file's own
//! normal is used only to orient a triangle too thin to have one.

use view4d_scene::{Mesh, Vec3};

use crate::Error;

/// The mesh in `bytes`, binary or ASCII.
pub fn parse(bytes: &[u8]) -> Result<Mesh, Error> {
    // Binary files may start with "solid" too: their size says which.
    if let Some(count) = binary_count(bytes) {
        return Ok(binary(bytes, count));
    }
    if bytes.trim_ascii_start().starts_with(b"solid") {
        return ascii(bytes);
    }
    Err(Error(match bytes.len() {
        0..84 => "too short for STL".into(),
        _ => "not STL: neither ASCII (`solid`) nor binary of the size its header gives".into(),
    }))
}

/// The triangle count, if `bytes` is exactly a binary STL of that many.
fn binary_count(bytes: &[u8]) -> Option<usize> {
    let count = u32::from_le_bytes(bytes.get(80..84)?.try_into().ok()?) as usize;
    (bytes.len() == 84 + count.checked_mul(50)?).then_some(count)
}

fn binary(bytes: &[u8], count: usize) -> Mesh {
    let f = |o: usize| f32::from_le_bytes(bytes[o..o + 4].try_into().expect("4 bytes"));
    let v = |o: usize| Vec3::new(f(o), f(o + 4), f(o + 8));
    let mut triangles = Vec::with_capacity(count);
    let mut given = Vec::with_capacity(count);
    for i in 0..count {
        let o = 84 + i * 50;
        given.push(v(o));
        triangles.push([v(o + 12), v(o + 24), v(o + 36)]);
    }
    mesh(triangles, given)
}

fn ascii(bytes: &[u8]) -> Result<Mesh, Error> {
    let text = String::from_utf8_lossy(bytes);
    let mut words = text.split_ascii_whitespace();
    let mut triangles = Vec::new();
    let mut given = Vec::new();
    let mut normal = Vec3::ZERO;
    let mut corners: Vec<Vec3> = Vec::with_capacity(3);
    let three = |words: &mut std::str::SplitAsciiWhitespace<'_>, what: &str| -> Result<Vec3, Error> {
        let mut n = [0f32; 3];
        for v in &mut n {
            let w = words.next().ok_or_else(|| Error(format!("a {what} cut short")))?;
            *v = w.parse().map_err(|_| Error(format!("a {what} with `{w}` for a number")))?;
        }
        Ok(Vec3::new(n[0], n[1], n[2]))
    };
    while let Some(w) = words.next() {
        match w {
            "normal" => normal = three(&mut words, "normal")?,
            "vertex" => corners.push(three(&mut words, "vertex")?),
            "endfacet" => {
                let [a, b, c] = corners[..] else {
                    return Err(Error(format!("a facet with {} vertices", corners.len())));
                };
                triangles.push([a, b, c]);
                given.push(normal);
                corners.clear();
                normal = Vec3::ZERO;
            }
            _ => {}
        }
    }
    if !corners.is_empty() {
        return Err(Error("the last facet has no `endfacet`".into()));
    }
    Ok(mesh(triangles, given))
}

/// A mesh of `triangles`, normals from their winding, or the file's
/// (`given`) for one too thin to have its own.
fn mesh(triangles: Vec<[Vec3; 3]>, given: Vec<Vec3>) -> Mesh {
    let mut m = Mesh::from_triangles(triangles);
    for (n, g) in m.normals.iter_mut().zip(given) {
        if *n == Vec3::ZERO {
            *n = g.normalized();
        }
    }
    m
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A tetrahedron with its corner at the origin, wound outwards.
    fn tetrahedron() -> Vec<[Vec3; 3]> {
        let (o, x, y, z) =
            (Vec3::ZERO, Vec3::new(10.0, 0.0, 0.0), Vec3::new(0.0, 10.0, 0.0), Vec3::new(0.0, 0.0, 10.0));
        vec![[o, y, x], [o, x, z], [o, z, y], [x, y, z]]
    }

    fn binary_file(tris: &[[Vec3; 3]], header: &[u8]) -> Vec<u8> {
        let mut b = header.to_vec();
        b.resize(80, 0);
        b.extend((tris.len() as u32).to_le_bytes());
        for t in tris {
            b.extend([0u8; 12]); // normals left zero, as many exporters do
            for p in t {
                for c in [p.x, p.y, p.z] {
                    b.extend(c.to_le_bytes());
                }
            }
            b.extend([0u8; 2]);
        }
        b
    }

    fn ascii_file(tris: &[[Vec3; 3]]) -> String {
        let mut s = String::from("solid part\n");
        for t in tris {
            s += "  facet normal 0 0 0\n    outer loop\n";
            for p in t {
                s += &format!("      vertex {} {} {:e}\n", p.x, p.y, p.z);
            }
            s += "    endloop\n  endfacet\n";
        }
        s + "endsolid part\n"
    }

    fn check(m: &Mesh) {
        assert_eq!(m.triangles, tetrahedron());
        assert_eq!(m.normals[0], Vec3::new(0.0, 0.0, -1.0), "the base faces down, out of the solid");
        assert_eq!(m.normals[1], Vec3::new(0.0, -1.0, 0.0));
        let slant = 1.0 / 3f32.sqrt();
        assert!((m.normals[3] - Vec3::new(slant, slant, slant)).length() < 1e-6);
        assert_eq!(m.bounds().size(), Vec3::new(10.0, 10.0, 10.0));
    }

    #[test]
    fn binary_and_ascii() {
        check(&parse(&binary_file(&tetrahedron(), b"made by FreeCAD")).unwrap());
        check(&parse(ascii_file(&tetrahedron()).as_bytes()).unwrap());
    }

    #[test]
    fn binary_whose_header_says_solid() {
        check(&parse(&binary_file(&tetrahedron(), b"solid part, binary all the same")).unwrap());
    }

    #[test]
    fn a_thin_triangle_keeps_the_files_normal() {
        let p = Vec3::new(1.0, 1.0, 1.0);
        let text = "solid t\nfacet normal 0 0 2\nouter loop\nvertex 1 1 1\nvertex 1 1 1\nvertex 1 1 1\nendloop\nendfacet\nendsolid t\n";
        let m = parse(text.as_bytes()).unwrap();
        assert_eq!(m.triangles, [[p, p, p]]);
        assert_eq!(m.normals, [Vec3::new(0.0, 0.0, 1.0)]);
    }

    #[test]
    fn errors() {
        assert!(parse(b"").is_err());
        assert!(parse(b"hello, not a model").is_err());
        let mut cut = binary_file(&tetrahedron(), b"");
        cut.pop();
        assert!(parse(&cut).is_err(), "a binary file one byte short");
        assert!(parse(b"solid a\nfacet normal 0 0 1\nouter loop\nvertex 1 2\n").is_err());
        assert!(parse(b"solid a\nfacet normal 0 0 1\nouter loop\nvertex 1 2 x\n").is_err());
        assert!(parse(b"solid a\nfacet\nvertex 0 0 0\nvertex 1 0 0\nendfacet\n").is_err());
        assert!(parse(b"solid a\nfacet\nvertex 0 0 0\nvertex 1 0 0\nvertex 0 1 0\n").is_err());
        assert_eq!(parse(b"solid empty\nendsolid empty\n").unwrap().triangles.len(), 0);
    }
}
