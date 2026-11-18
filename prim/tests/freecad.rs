//! Each primitive built at its defaults against FreeCAD's build of it
//! (tests/data/freecad.json, from primlib's tools/baseline.py through
//! tools/view4d_fixtures.py): the volume within 0.1%, the bounds within
//! 0.05 mm, closed to a millionth of its area, and its mesh with no open
//! edge. A primitive that needs an operation not built yet is listed,
//! not failed.

use std::path::{Path, PathBuf};
use std::time::Instant;

use view4d_prim::Prim;
use view4d_prim::csg::Solid;
use view4d_prim::json::{self, Value as Json};

fn data() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/data")
}

/// Primitives whose meshes are known to have open edges, and why. Each
/// must still have them (when one closes, take it off).
const OPEN: [(&str, &str); 1] = [(
    "twist_plug.prim",
    "where the lead (a loft about z) meets the rest of the lug (a revolve): their shared end face \
     is cut by different steps, leaving slivers (7e-4 mm² unbalanced in 4,600). For M8",
)];

#[test]
fn as_freecad_builds_them() {
    let fc = json::parse(&std::fs::read_to_string(data().join("freecad.json")).unwrap()).unwrap();
    let (mut ok, mut waiting, mut bad) = (vec![], vec![], vec![]);
    for (file, want) in fc.as_object().unwrap() {
        let prim = Prim::read(&data().join(file)).unwrap();
        let env = prim.names(&[]).unwrap();
        let t = Instant::now();
        let pieces = match prim.build(&env) {
            Ok(p) => p,
            Err(e) if e.0.contains("is not built yet") => {
                waiting.push(format!("{file}: {e}"));
                continue;
            }
            Err(e) => {
                bad.push(format!("{file}: {e}"));
                continue;
            }
        };
        let ms = t.elapsed().as_millis();
        // A compound's solids are measured each and summed, as FreeCAD does
        // (stencil's two templates are the same plate, cut two ways).
        let volume: f64 = pieces.iter().map(|p| p.solid.volume()).sum();
        let solids: Vec<Solid> = pieces.into_iter().map(|p| p.solid).collect();
        let solid = Solid::new(solids.iter().flat_map(|s| s.polys.clone()).collect());
        let want_volume = want.get("volume").and_then(Json::as_f64).unwrap();
        let (lo, hi) = solid.bounds().unwrap();
        let want_box: Vec<f64> =
            want.get("box").unwrap().as_array().unwrap().iter().map(|v| v.as_f64().unwrap()).collect();
        let got_box = [lo.x, lo.y, lo.z, hi.x, hi.y, hi.z];
        let off = (volume - want_volume) / want_volume;
        // Closed: the faces' area vectors sum to nothing (a sliver of a face
        // kept or dropped whole leaves a little).
        let (area, unbalanced) = solid.area_balance();
        let open = unbalanced / area;
        // And as a mesh: every edge met by one going the other way.
        let t = Instant::now();
        let meshes: Vec<_> = solids.iter().map(Solid::mesh).collect();
        let open_edges: usize = meshes.iter().map(|m| m.open_edges()).sum();
        let triangles: usize = meshes.iter().map(|m| m.triangles.len()).sum();
        let mesh_ms = t.elapsed().as_millis();
        let box_off = got_box.iter().zip(&want_box).map(|(a, b)| (a - b).abs()).fold(0.0, f64::max);
        let line = format!(
            "{file}: volume {volume:.3}, FreeCAD {want_volume:.3} ({:+.4}%); bounds within {box_off:.4} mm; {unbalanced:.1e} mm² of {area:.0} unbalanced; {} polygons, {ms} ms; \
             mesh {} triangles, {open_edges} open edges, {mesh_ms} ms",
            off * 100.0,
            solid.polys.len(),
            triangles
        );
        let known_open = OPEN.iter().any(|(f, _)| f == file);
        if off.abs() > 0.001 || box_off > 0.05 || open > 1e-6 || (open_edges > 0) != known_open {
            bad.push(line);
        } else {
            ok.push(line);
        }
    }
    eprintln!("as FreeCAD builds them:\n  {}", ok.join("\n  "));
    eprintln!("waiting for an operation:\n  {}", waiting.join("\n  "));
    assert!(bad.is_empty(), "differ from FreeCAD:\n  {}", bad.join("\n  "));
}
