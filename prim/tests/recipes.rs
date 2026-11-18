//! Recipes and assemblies laid out as primlib lays them out (`primlib/recipe.py`'s
//! `layout`, written into tests/data/recipes.json by primlib's
//! tools/view4d_fixtures.py), and their parts built.

use std::path::PathBuf;

use view4d_prim::json::{self, Value as Json};
use view4d_prim::recipe::Recipe;
use view4d_prim::{Document, Error};

fn data() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/data")
}

fn recipe(file: &str) -> Recipe {
    match Document::parse(&std::fs::read_to_string(data().join(file)).unwrap()).unwrap() {
        Document::Recipe(r) => r,
        Document::Primitive(_) => panic!("{file} is a primitive"),
    }
}

fn values(case: &Json) -> Vec<(String, String)> {
    case.get("values")
        .unwrap()
        .as_object()
        .unwrap()
        .iter()
        .map(|(k, v)| (k.clone(), v.as_str().unwrap().to_string()))
        .collect()
}

fn close(a: f64, b: f64) -> bool {
    (a - b).abs() <= 1e-9 * a.abs().max(b.abs()).max(1.0)
}

#[test]
fn as_primlib_lays_them_out() {
    let cases = json::parse(&std::fs::read_to_string(data().join("recipes.json")).unwrap()).unwrap();
    let (mut compared, mut bad) = (0, vec![]);
    for (file, file_cases) in cases.as_object().unwrap() {
        let r = recipe(file);
        for case in file_cases.as_array().unwrap() {
            let values = values(case);
            let env = r.names(&values).unwrap_or_else(|e| panic!("{file} {values:?}: {e}"));
            let failed = r.failed_checks(&env).unwrap();
            let want: Vec<&str> =
                case.get("failed").unwrap().as_array().unwrap().iter().map(|m| m.as_str().unwrap()).collect();
            assert_eq!(failed, want, "{file} {values:?}: checks");
            if !failed.is_empty() {
                continue;
            }
            let leaves = r.leaves(&env).unwrap();
            let want_parts = case.get("parts").unwrap().as_object().unwrap();
            let paths: Vec<&str> = leaves.iter().map(|l| l.path.as_str()).collect();
            let want_paths: Vec<&str> = want_parts.iter().map(|(k, _)| k.as_str()).collect();
            assert_eq!(paths, want_paths, "{file} {values:?}: leaf parts");
            let printed: Vec<&str> = leaves.iter().filter(|l| l.print).map(|l| l.path.as_str()).collect();
            let mut want_print: Vec<&str> =
                case.get("print").unwrap().as_array().unwrap().iter().map(|p| p.as_str().unwrap()).collect();
            want_print.sort();
            let mut printed = printed;
            printed.sort();
            assert_eq!(printed, want_print, "{file} {values:?}: the print list");
            for (leaf, (_, want)) in leaves.iter().zip(want_parts) {
                let got = &leaf.values;
                for (k, w) in want.as_object().unwrap() {
                    let g = &got.iter().find(|(n, _)| n == k).unwrap().1;
                    let w = w.as_str().unwrap();
                    let same = match (g.parse::<f64>(), w.parse::<f64>()) {
                        (Ok(a), Ok(b)) => close(a, b),
                        _ => g == w,
                    };
                    if !same {
                        bad.push(format!("{file} {values:?}: part {} {k}: {g}, primlib {w}", leaf.path));
                    }
                }
            }
            let placed = r.layout(&env).unwrap();
            let want = case.get("placed").unwrap().as_array().unwrap();
            assert_eq!(placed.len(), want.len(), "{file} {values:?}: how many placed");
            for (p, w) in placed.iter().zip(want) {
                compared += 1;
                let w = w.as_array().unwrap();
                let rot: Vec<f64> = w[2]
                    .as_array()
                    .unwrap()
                    .iter()
                    .flat_map(|r| r.as_array().unwrap().iter().map(|v| v.as_f64().unwrap()))
                    .collect();
                let at: Vec<f64> = w[3].as_array().unwrap().iter().map(|v| v.as_f64().unwrap()).collect();
                let same = p.name == w[0].as_str().unwrap()
                    && p.part == w[1].as_str().unwrap()
                    && p.rotation.iter().flatten().zip(&rot).all(|(a, b)| close(*a, *b))
                    && p.at.iter().zip(&at).all(|(a, b)| close(*a, *b));
                if !same {
                    bad.push(format!("{file} {values:?}: {p:?}, primlib {w:?}"));
                }
            }
        }
    }
    assert!(bad.is_empty(), "{}", bad.join("\n"));
    assert!(compared >= 20, "only {compared} placed parts compared");
}

#[test]
fn every_part_builds() {
    for file in ["edge_fill.prim", "edge_run.prim", "support_pair.prim", "hinged_lid.prim"] {
        let r = recipe(file);
        let env = r.names(&[]).unwrap();
        for leaf in r.leaves(&env).unwrap() {
            let built = leaf.prim.names(&leaf.values).and_then(|e| leaf.prim.build(&e));
            assert!(built.as_ref().is_ok_and(|p| !p.is_empty()), "{file} part {}: {:?}", leaf.path, built.err());
            // The pieces a part takes are pieces the compound has.
            let names: Vec<_> = built.unwrap().into_iter().filter_map(|p| p.name).collect();
            for piece in leaf.pieces.unwrap_or_default() {
                assert!(names.contains(piece), "{file} part {}: no piece {piece} in {names:?}", leaf.path);
            }
        }
    }
}

#[test]
fn refused() {
    let text = std::fs::read_to_string(data().join("support_pair.prim")).unwrap();
    for (from, to, why) in [
        ("\"slots\"", "\"slats\"", "no \"slots\""),
        (
            "\"primitive\": \"support_half\", \"bind\"",
            "\"primitive\": \"nothing\", \"bind\"",
            "part half: its primitive",
        ),
        ("\"mirror\": \"y\"", "\"mirror\": \"w\"", "part mirrored: mirror"),
        ("\"kind\": \"recipe\"", "\"kind\": \"widget\"", "kind"),
    ] {
        assert!(text.contains(from), "{from}");
        let e: Error = Document::parse(&text.replacen(from, to, 1)).unwrap_err();
        assert!(e.0.starts_with(why), "{from} -> {to}: {e}");
    }
}

#[test]
fn a_recipe_uses_primitives_only() {
    // hinged_lid as a recipe: its edge_run part is a recipe, which only an assembly may use.
    let text = std::fs::read_to_string(data().join("hinged_lid.prim")).unwrap();
    let e = Document::parse(&text.replacen("\"kind\": \"assembly\"", "\"kind\": \"recipe\"", 1)).unwrap_err();
    assert!(e.0.contains("a recipe uses primitives only"), "{e}");
}

#[test]
fn the_lid_opens() {
    let r = recipe("hinged_lid.prim");
    assert!(r.assembly);
    let at = |angle: &str, name: &str| {
        let env = r.names(&[("open_angle".into(), angle.into())]).unwrap();
        r.layout(&env).unwrap().into_iter().find(|p| p.name == name).unwrap().at
    };
    // Shut, the lid run's first piece is width / 2 along y; a quarter open about x, it stands up in z.
    let [_, y, z] = at("0", "lid_run/edge_1S");
    assert!(close(y, 30.0) && close(z, 0.0), "{y} {z}");
    let [_, y, z] = at("90", "lid_run/edge_1S");
    assert!(close(y, 0.0) && close(z, 30.0), "{y} {z}");
    assert_eq!(at("0", "hinge_base"), at("90", "hinge_base"));
}
