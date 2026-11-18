//! primlib's `.prim` files (`view4d-prim`): the primitive built at the
//! file's values, or others given, into parts: one, named by the file, or
//! a compound's, each in a colour of its own. A recipe's or assembly's
//! primitive parts (nested ones by path) are each built once, at their
//! values and of the pieces they take, and drawn wherever it places them.
//! A part built the same as in the last recipe read is not built again, so
//! a file rewritten with only its placing changed (a joint's angle) reads
//! again at once.

use std::sync::Mutex;

use view4d_prim::recipe::{Leaf, Recipe, Rotation, apply, mirror};
use view4d_prim::{Document, Prim};
use view4d_scene::edges::{FEATURE_ANGLE, feature_edges};
use view4d_scene::{Mesh, Vec3};

use crate::{Error, Part};

/// The colours a compound's parts take, in order.
pub const PALETTE: [[u8; 3]; 6] =
    [[180, 190, 205], [225, 160, 95], [125, 185, 130], [205, 135, 175], [125, 165, 225], [215, 200, 115]];

/// The parts of the primitive in `text`, built at `values` (name →
/// expression) over the file's own; `name` names a part the file doesn't.
/// A check that fails is an error, giving each one's message.
pub fn parse(text: &str, values: &[(String, String)], name: &str) -> Result<Vec<Part>, Error> {
    let e = |e: view4d_prim::Error| Error(e.0);
    match Document::parse(text).map_err(e)? {
        Document::Primitive(prim) => primitive(&prim, values, name),
        Document::Recipe(r) => recipe(&r, values),
    }
}

fn mesh_of(solid: &view4d_prim::csg::Solid, color: [u8; 3]) -> Mesh {
    let f = |v: view4d_prim::csg::V3| Vec3::new(v.x as f32, v.y as f32, v.z as f32);
    // Welded, without T-junctions, so faces meet edge to edge; normals from the planes the
    // faces were made on, where a sliver's winding would give a poor one.
    let m = solid.mesh();
    let triangles = m.triangles.iter().map(|t| t.map(|i| f(m.verts[i as usize]))).collect();
    let normals = m.normals.iter().map(|&n| f(n)).collect();
    Mesh { triangles, normals, color }
}

/// `mesh` turned by `m` (which may mirror: then each triangle turns over,
/// so it still winds anticlockwise from outside), then moved by `at`.
fn placed(mesh: &Mesh, m: &Rotation, at: [f64; 3]) -> Mesh {
    let det = m[0][0] * (m[1][1] * m[2][2] - m[1][2] * m[2][1]) - m[0][1] * (m[1][0] * m[2][2] - m[1][2] * m[2][0])
        + m[0][2] * (m[1][0] * m[2][1] - m[1][1] * m[2][0]);
    let turn = |v: Vec3| {
        let r = apply(m, [v.x as f64, v.y as f64, v.z as f64]);
        Vec3::new(r[0] as f32, r[1] as f32, r[2] as f32)
    };
    let mv = |v: Vec3| turn(v) + Vec3::new(at[0] as f32, at[1] as f32, at[2] as f32);
    let triangles =
        mesh.triangles.iter().map(|t| if det < 0.0 { [mv(t[0]), mv(t[2]), mv(t[1])] } else { t.map(mv) }).collect();
    Mesh { triangles, normals: mesh.normals.iter().map(|&n| turn(n)).collect(), color: mesh.color }
}

/// `segments` turned by `m`, then moved by `at`.
fn placed_segments(segments: &[[Vec3; 2]], m: &Rotation, at: [f64; 3]) -> Vec<[Vec3; 2]> {
    let mv = |v: Vec3| {
        let r = apply(m, [v.x as f64, v.y as f64, v.z as f64]);
        Vec3::new((r[0] + at[0]) as f32, (r[1] + at[1]) as f32, (r[2] + at[2]) as f32)
    };
    segments.iter().map(|s| s.map(mv)).collect()
}

/// A part as the last recipe read built it: what it was built from, and the mesh.
struct Built {
    prim: Prim,
    values: Vec<(String, String)>,
    pieces: Option<Vec<String>>,
    mirror: Option<usize>,
    color: [u8; 3],
    mesh: Mesh,
    /// Its feature edges, to be placed with it.
    edges: Vec<[Vec3; 2]>,
}

/// The parts the last recipe read built.
static LAST: Mutex<Vec<Built>> = Mutex::new(Vec::new());

fn recipe(r: &Recipe, values: &[(String, String)]) -> Result<Vec<Part>, Error> {
    let e = |e: view4d_prim::Error| Error(e.0);
    let env = r.names(values).map_err(e)?;
    let failed = r.failed_checks(&env).map_err(e)?;
    if !failed.is_empty() {
        return Err(Error(format!("{} fails its checks: {}", r.head.primitive, failed.join("; "))));
    }
    let layout = r.layout(&env).map_err(e)?;
    let leaves = r.leaves(&env).map_err(e)?;
    // Each part placed is built once (a part not placed isn't built).
    let mut meshes: Vec<Option<(Mesh, Vec<[Vec3; 2]>)>> = vec![None; leaves.len()];
    let mut last = LAST.lock().unwrap_or_else(|e| e.into_inner());
    let mut built = vec![];
    let mut out = vec![];
    for p in layout {
        let i = leaves.iter().position(|l| l.path == p.part).ok_or_else(|| Error(format!("no part {:?}", p.part)))?;
        if meshes[i].is_none() {
            let leaf = &leaves[i];
            let color = leaf.colour.map_or(PALETTE[i % PALETTE.len()], |c| c.map(|v| (v * 255.0).round() as u8));
            let same = |b: &&Built| {
                b.prim == *leaf.prim
                    && b.values == leaf.values
                    && b.pieces.as_deref() == leaf.pieces
                    && b.mirror == leaf.mirror
                    && b.color == color
            };
            let (mesh, edges) = match last.iter().find(same) {
                Some(b) => (b.mesh.clone(), b.edges.clone()),
                None => {
                    let mesh = build_leaf(leaf, color)?;
                    let edges = feature_edges(&mesh, FEATURE_ANGLE, color).segments;
                    (mesh, edges)
                }
            };
            built.push(Built {
                prim: leaf.prim.clone(),
                values: leaf.values.clone(),
                pieces: leaf.pieces.map(<[String]>::to_vec),
                mirror: leaf.mirror,
                color,
                mesh: mesh.clone(),
                edges: edges.clone(),
            });
            meshes[i] = Some((mesh, edges));
        }
        let (mesh, edges) = meshes[i].as_ref().unwrap();
        let (mesh, edges) = (placed(mesh, &p.rotation, p.at), placed_segments(edges, &p.rotation, p.at));
        out.push(Part { name: p.name, mesh, edges: Some(edges) });
    }
    *last = built;
    Ok(out)
}

/// A recipe's primitive part built at its values, of the pieces it takes,
/// mirrored if it is, in `color`; not yet placed.
fn build_leaf(leaf: &Leaf, color: [u8; 3]) -> Result<Mesh, Error> {
    let at = |err: view4d_prim::Error| Error(format!("part {}: {}", leaf.path, err.0));
    let mut pieces = leaf.prim.names(&leaf.values).and_then(|env| leaf.prim.build(&env)).map_err(at)?;
    if let Some(take) = leaf.pieces {
        pieces.retain(|p| p.name.as_ref().is_some_and(|n| take.contains(n)));
        if pieces.len() != take.len() {
            return Err(Error(format!("part {}: pieces {take:?}, not all in its compound", leaf.path)));
        }
    }
    let solid = view4d_prim::csg::Solid::union_all(pieces.into_iter().map(|p| p.solid).collect());
    let mesh = mesh_of(&solid, color);
    Ok(match leaf.mirror {
        Some(axis) => placed(&mesh, &mirror(axis), [0.0; 3]),
        None => mesh,
    })
}

fn primitive(prim: &Prim, values: &[(String, String)], name: &str) -> Result<Vec<Part>, Error> {
    let e = |e: view4d_prim::Error| Error(e.0);
    let env = prim.names(values).map_err(e)?;
    let failed = prim.failed_checks(&env).map_err(e)?;
    if !failed.is_empty() {
        return Err(Error(format!("{} fails its checks: {}", prim.primitive, failed.join("; "))));
    }
    let pieces = prim.build(&env).map_err(e)?;
    let many = pieces.len() > 1;
    Ok(pieces
        .into_iter()
        .enumerate()
        .map(|(i, p)| {
            let color = if many { PALETTE[i % PALETTE.len()] } else { Mesh::DEFAULT_COLOR };
            let mesh = mesh_of(&p.solid, color);
            let name = match (p.name, many) {
                (Some(n), true) => n,
                (None, true) => format!("part {}", i + 1),
                _ => name.to_string(),
            };
            Part { name, mesh, edges: None }
        })
        .collect())
}

#[cfg(test)]
mod tests {
    use super::*;

    const TWO: &str = r#"{"format": "primlib.prim", "version": 1, "primitive": "two", "title": "Two",
        "params": [{"name": "w", "default": "10"}], "fixed": [], "derived": [],
        "checks": [{"expr": "w - 1", "message": "w must be over 1"}],
        "ops": [{"id": 0, "op": "box", "name": "a", "x": ["0", "w"], "y": ["0", "1"], "z": ["0", "1"]},
                {"id": 1, "op": "box", "name": "b", "x": ["0", "1"], "y": ["2", "3"], "z": ["0", "1"]},
                {"id": 2, "op": "compound", "name": null, "of": [0, 1]}],
        "result": 2, "motion": []}"#;

    #[test]
    fn parts_in_colours() {
        let parts = parse(TWO, &[], "two").unwrap();
        let names: Vec<_> = parts.iter().map(|p| p.name.as_str()).collect();
        assert_eq!(names, ["a", "b"]);
        assert_ne!(parts[0].mesh.color, parts[1].mesh.color);
        assert_eq!(parts[0].mesh.bounds().max.x, 10.0);
        let wide = parse(TWO, &[("w".into(), "20".into())], "two").unwrap();
        assert_eq!(wide[0].mesh.bounds().max.x, 20.0);
        // One piece is named by the file.
        let one = parse(&TWO.replace("\"result\": 2", "\"result\": 0"), &[], "two").unwrap();
        assert_eq!((one.len(), one[0].name.as_str(), one[0].mesh.color), (1, "two", Mesh::DEFAULT_COLOR));
    }

    const PAIR: &str = r#"{"format": "primlib.prim", "version": 2, "kind": "recipe", "primitive": "pair", "title": "",
        "params": [{"name": "n", "default": "2"}], "fixed": [], "derived": [], "checks": [], "slots": 8,
        "parts": [{"name": "a", "primitive": "wedge", "bind": {"w": "3"}, "mirror": null, "colour": [1, 0, 0]},
                  {"name": "b", "primitive": "wedge", "bind": {}, "mirror": "x", "colour": null}],
        "place": [{"id": 0, "op": "instance", "name": "one", "part": "a", "at": ["10 * i", "0", "0"],
                   "turn": [{"axis": "z", "angle": "90"}]},
                  {"id": 1, "op": "series", "name": "row", "count": "n", "index": "i", "item": 0},
                  {"id": 2, "op": "instance", "name": "other", "part": "b", "at": ["0", "0", "5"], "turn": []},
                  {"id": 3, "op": "group", "name": null, "of": [1, 2]}],
        "result": 3, "motion": [],
        "uses": {"wedge": {"format": "primlib.prim", "version": 1, "primitive": "wedge", "title": "",
            "params": [{"name": "w", "default": "1"}], "fixed": [], "derived": [], "checks": [],
            "ops": [{"id": 0, "op": "prism", "name": null, "plane": "xy", "points": [["0", "0"], ["w", "0"], ["0", "1"]],
                     "span": ["0", "1"]}],
            "result": 0, "motion": []}}}"#;

    /// The volume a mesh encloses: positive when it winds anticlockwise from outside.
    fn volume(m: &Mesh) -> f32 {
        m.triangles.iter().map(|[a, b, c]| a.dot(b.cross(*c))).sum::<f32>() / 6.0
    }

    #[test]
    fn recipes_placed_mirrored_coloured() {
        let parts = parse(PAIR, &[], "pair").unwrap();
        let names: Vec<_> = parts.iter().map(|p| p.name.as_str()).collect();
        assert_eq!(names, ["one", "one", "other"]);
        // a: w = 3, turned a quarter about z (x -> y), the second 10 along x.
        let b = parts[1].mesh.bounds();
        assert!((b.min.x - 9.0).abs() < 1e-5 && (b.max.x - 10.0).abs() < 1e-5 && (b.max.y - 3.0).abs() < 1e-5, "{b:?}");
        assert_eq!(parts[0].mesh.color, [255, 0, 0]);
        // b: mirrored in x, still closed and outward.
        let m = parts[2].mesh.bounds();
        assert!((m.min.x + 1.0).abs() < 1e-5 && m.max.x.abs() < 1e-5 && (m.min.z - 5.0).abs() < 1e-5, "{m:?}");
        for p in &parts {
            assert!(volume(&p.mesh) > 0.0, "{}: {}", p.name, volume(&p.mesh));
        }
        assert_eq!(parse(PAIR, &[("n".into(), "0".into())], "pair").unwrap().len(), 1);
    }

    #[test]
    fn a_changed_part_is_built_again() {
        let before = parse(PAIR, &[], "pair").unwrap();
        // Only the placing changes: the same meshes, placed anew.
        let more = parse(PAIR, &[("n".into(), "3".into())], "pair").unwrap();
        assert_eq!(more.len(), 4);
        assert_eq!(more[0].mesh.triangles, before[0].mesh.triangles);
        // The wedge's definition changes, its values don't: built again.
        let taller = PAIR.replace(r#"["0", "1"]],"#, r#"["0", "4"]],"#);
        assert_ne!(taller, PAIR);
        let after = parse(&taller, &[], "pair").unwrap();
        assert!((after[2].mesh.bounds().max.y - 4.0).abs() < 1e-5, "{:?}", after[2].mesh.bounds());
    }

    #[test]
    fn checks_and_names() {
        assert_eq!(
            parse(TWO, &[("w".into(), "1".into())], "two").unwrap_err().0,
            "two fails its checks: w must be over 1"
        );
        assert_eq!(parse(TWO, &[("h".into(), "1".into())], "two").unwrap_err().0, "two has no parameter h");
    }

    #[test]
    fn an_assembly_with_a_joint_and_pieces() {
        let path = concat!(env!("CARGO_MANIFEST_DIR"), "/../prim/tests/data/hinged_lid.prim");
        let text = std::fs::read_to_string(path).unwrap();
        let shut = parse(&text, &[], "hinged_lid").unwrap();
        let open = parse(&text, &[("open_angle".into(), "90".into())], "hinged_lid").unwrap();
        let names: Vec<_> = shut.iter().map(|p| p.name.as_str()).collect();
        assert!(names.contains(&"base_run/edge_1S") && names.contains(&"hinge_lid"), "{names:?}");
        let find = |parts: &[Part], n: &str| parts.iter().find(|p| p.name == n).unwrap().mesh.bounds();
        // The lid run lies flat shut, and stands up open: its z extent grows to its width in y.
        let (a, b) = (find(&shut, "lid_run/edge_1S"), find(&open, "lid_run/edge_1S"));
        assert!((a.max.y - a.min.y - (b.max.z - b.min.z)).abs() < 1e-3, "{a:?} {b:?}");
        assert!(b.min.z > a.min.z + 10.0, "{a:?} {b:?}");
        // The base doesn't move.
        assert_eq!(find(&shut, "hinge_base"), find(&open, "hinge_base"));
        // A hinge's two leaves are different pieces: neither is the whole hinge.
        let (lid, base) = (find(&shut, "hinge_lid"), find(&shut, "hinge_base"));
        assert_ne!(lid, base);
        for p in &shut {
            assert!(volume(&p.mesh) > 0.0, "{}: {}", p.name, volume(&p.mesh));
        }
    }
}
