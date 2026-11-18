//! Against primlib's own evaluator: every primitive primlib exports, and the
//! values `primlib/expr.py` gives its names and expressions, at the defaults
//! and other values (tests/data, written by primlib's
//! `tools/view4d_fixtures.py`; run it again when primlib changes).

use std::path::{Path, PathBuf};

use view4d_prim::expr::{self, Env, Value};
use view4d_prim::json::{self, Value as Json};
use view4d_prim::{Document, Prim};

fn data() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/data")
}

fn cases() -> Json {
    json::parse(&std::fs::read_to_string(data().join("cases.json")).unwrap()).unwrap()
}

fn close(a: f64, b: f64) -> bool {
    (a - b).abs() <= 1e-9 * a.abs().max(b.abs()).max(1.0)
}

fn same(got: &Value, want: &Json) -> bool {
    match (got, want) {
        (Value::Number(a), Json::Number(b)) => close(*a, *b),
        (Value::Text(a), Json::String(b)) => a == b,
        _ => false,
    }
}

#[test]
fn every_primitive_as_primlib_evaluates_it() {
    let cases = cases();
    let files = cases.get("files").unwrap().as_object().unwrap();
    assert!(files.len() >= 19, "{} files", files.len());
    let (mut compared, mut bad) = (0, vec![]);
    for (file, file_cases) in files {
        let prim = Prim::read(&data().join(file)).unwrap();
        for case in file_cases.as_array().unwrap() {
            let values: Vec<(String, String)> = case
                .get("values")
                .unwrap()
                .as_object()
                .unwrap()
                .iter()
                .map(|(k, v)| (k.clone(), v.as_str().unwrap().to_string()))
                .collect();
            let env = prim.names(&values).unwrap_or_else(|e| panic!("{file} {values:?}: {e}"));
            let names = case.get("names").unwrap().as_object().unwrap();
            assert_eq!(env.len(), names.len(), "{file}: names");
            for (n, want) in names {
                compared += 1;
                let got = env.get(n).unwrap_or_else(|| panic!("{file}: no name {n}"));
                if !same(got, want) {
                    bad.push(format!("{file} {values:?}: name {n}: {got:?}, primlib {want:?}"));
                }
            }
            let failed: Vec<&str> =
                case.get("failed").unwrap().as_array().unwrap().iter().map(|m| m.as_str().unwrap()).collect();
            assert_eq!(prim.failed_checks(&env).unwrap(), failed, "{file} {values:?}: checks");
            for e in case.get("exprs").unwrap().as_array().unwrap() {
                let e = e.as_array().unwrap();
                let (text, i, want) = (e[0].as_str().unwrap(), &e[1], &e[2]);
                let mut env: Env = env.clone();
                if let Some(i) = i.as_f64() {
                    env.set("i", i);
                }
                compared += 1;
                match expr::parse(text).and_then(|x| x.eval(&env, None)) {
                    Ok(got) if same(&got, want) => {}
                    got => bad.push(format!("{file} {values:?}: {text} (i={i:?}): {got:?}, primlib {want:?}")),
                }
            }
        }
    }
    eprintln!("{compared} values compared with primlib's");
    assert!(bad.is_empty(), "{} of {compared} differ:\n{}", bad.len(), bad.join("\n"));
    assert!(compared > 2000, "only {compared} compared");
}

#[test]
fn what_primlib_refuses_is_refused() {
    let cases = cases();
    for bad in cases.get("errors").unwrap().as_array().unwrap() {
        let bad = bad.as_str().unwrap();
        assert!(expr::evaluate(bad, &Env::new()).is_err(), "{bad:?} evaluated");
    }
}

#[test]
fn text_as_primlib_has_it() {
    let cases = cases();
    let text = cases.get("text").unwrap();
    let mut env = Env::new();
    for (k, v) in text.get("env").unwrap().as_object().unwrap() {
        match v {
            Json::String(s) => env.set_value(k, Value::Text(s.clone())),
            v => env.set(k, v.as_f64().unwrap()),
        }
    }
    let mut bad = vec![];
    for case in text.get("cases").unwrap().as_array().unwrap() {
        let case = case.as_array().unwrap();
        let (t, want) = (case[0].as_str().unwrap(), &case[1]);
        match expr::parse(t).and_then(|x| x.eval(&env, None)) {
            Ok(got) if same(&got, want) => {}
            got => bad.push(format!("{t}: {got:?}, primlib {want:?}")),
        }
    }
    for t in text.get("errors").unwrap().as_array().unwrap() {
        let t = t.as_str().unwrap();
        if let Ok(v) = expr::parse(t).and_then(|x| x.eval(&env, None)) {
            bad.push(format!("{t}: {v:?}, primlib refuses it"));
        }
    }
    assert!(bad.is_empty(), "{}", bad.join("\n"));
}

#[test]
fn every_expression_in_every_file_parses() {
    // The bounds (xmin(@3)) aren't in the cases: they need geometry. They parse.
    fn walk(v: &Json, key: &str, out: &mut Vec<String>) {
        const NOT_EXPRS: [&str; 12] =
            ["op", "name", "axis", "plane", "join", "index", "id", "ruled", "sides", "normal", "part", "along"];
        match v {
            Json::String(s) if !NOT_EXPRS.contains(&key) && !s.starts_with('\'') => out.push(s.clone()),
            Json::Array(a) => a.iter().for_each(|x| walk(x, key, out)),
            Json::Object(o) => o.iter().for_each(|(k, x)| walk(x, k, out)),
            _ => {}
        }
    }
    let mut n = 0;
    for entry in std::fs::read_dir(data()).unwrap() {
        let path = entry.unwrap().path();
        if path.extension().is_some_and(|e| e == "prim") {
            let mut exprs = vec![];
            match Document::parse(&std::fs::read_to_string(&path).unwrap()).unwrap() {
                Document::Primitive(prim) => prim.ops.iter().for_each(|op| walk(op, "", &mut exprs)),
                Document::Recipe(r) => {
                    r.head.ops.iter().for_each(|op| walk(op, "", &mut exprs));
                    exprs.extend(r.parts.iter().flat_map(|p| p.bind.iter().map(|(_, e)| e.clone())));
                }
            }
            for e in exprs {
                n += 1;
                expr::parse(&e).unwrap_or_else(|err| panic!("{}: {e}: {err}", path.display()));
            }
        }
    }
    assert!(n > 1000, "only {n}");
}
