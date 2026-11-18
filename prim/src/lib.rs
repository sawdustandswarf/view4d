//! `view4d-prim`: primlib's `.prim` files (primlib's docs/FORMAT.md): a
//! primitive's parameters, checks and tree of operations, its sizes
//! expressions over its own names, read so view4d can build it at any values.
//!
//! ```no_run
//! let prim = view4d_prim::Prim::read(std::path::Path::new("hinge.prim")).unwrap();
//! let env = prim.names(&[("length".into(), "60".into())]).unwrap();
//! assert!(prim.failed_checks(&env).unwrap().is_empty());
//! ```

pub mod build;
pub mod csg;
pub mod expr;
pub mod json;
pub mod profile;
pub mod recipe;
pub mod text;

use std::path::Path;

use expr::{Env, Value, evaluate};
use json::Value as Json;

/// Why a file can't be read or built.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Error(pub String);

impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

impl std::error::Error for Error {}

impl From<expr::ExprError> for Error {
    fn from(e: expr::ExprError) -> Error {
        Error(e.0)
    }
}

/// The format and the newest version read (2: recipes and assemblies).
pub const FORMAT: &str = "primlib.prim";
pub const VERSION: u32 = 2;

/// A parameter: its default is an expression (a formula when it starts
/// with `=`, over the parameters before it), or a text starting with `'`.
#[derive(Debug, Clone, PartialEq)]
pub struct Param {
    pub name: String,
    pub default: String,
    pub note: String,
    /// The heading it sits under.
    pub group: Option<String>,
}

/// A fixed or derived value.
#[derive(Debug, Clone, PartialEq)]
pub struct Named {
    pub name: String,
    pub expr: String,
    pub note: String,
}

/// An expression that must be over 0, and what to change if it isn't.
#[derive(Debug, Clone, PartialEq)]
pub struct Check {
    pub expr: String,
    pub message: String,
}

/// A `.prim` file.
#[derive(Debug, Clone, PartialEq)]
pub struct Prim {
    /// The library's key for it (`hinge`).
    pub primitive: String,
    pub title: String,
    pub params: Vec<Param>,
    pub fixed: Vec<Named>,
    pub derived: Vec<Named>,
    pub checks: Vec<Check>,
    /// The values to build at, in place of the defaults: name → expression.
    pub values: Vec<(String, String)>,
    /// The operations, each `{id, op, name, ...}`, id its place here.
    pub ops: Vec<Json>,
    /// The operation that is the part.
    pub result: usize,
}

fn text<'a>(v: &'a Json, key: &str, what: &str) -> Result<&'a str, Error> {
    v.get(key).and_then(Json::as_str).ok_or_else(|| Error(format!("{what}: no {key:?} text")))
}

/// An optional text: missing or null is empty.
fn opt_text(v: &Json, key: &str) -> String {
    v.get(key).and_then(Json::as_str).unwrap_or_default().to_string()
}

fn list<'a>(doc: &'a Json, key: &str) -> Result<&'a [Json], Error> {
    doc.get(key)
        .ok_or_else(|| Error(format!("no {key:?}")))?
        .as_array()
        .ok_or_else(|| Error(format!("{key:?} is not a list")))
}

/// A name's value: a number, or a text when it is another name's text.
fn value(text: &str, env: &Env) -> Result<Value, expr::ExprError> {
    expr::parse(text)?.eval(env, None)
}

/// What a document is.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Kind {
    Primitive,
    Recipe,
    Assembly,
}

/// The format, version and kind, checked.
fn kind(doc: &Json) -> Result<Kind, Error> {
    if doc.as_object().is_none() || doc.get("format").and_then(Json::as_str) != Some(FORMAT) {
        return Err(Error(format!("not a {FORMAT} file")));
    }
    match doc.get("version").and_then(Json::as_index) {
        Some(v) if v <= VERSION as usize => {}
        Some(v) => return Err(Error(format!("version {v}: this reads up to {VERSION}"))),
        None => return Err(Error("no version".into())),
    }
    match doc.get("kind") {
        None | Some(Json::Null) => Ok(Kind::Primitive),
        Some(k) => match k.as_str() {
            Some("primitive") => Ok(Kind::Primitive),
            Some("recipe") => Ok(Kind::Recipe),
            Some("assembly") => Ok(Kind::Assembly),
            _ => Err(Error(format!("kind {k:?}: a primitive, recipe or assembly"))),
        },
    }
}

/// A `.prim` file of any kind: an assembly is a `Recipe` whose parts may
/// be recipes or assemblies (`Recipe::assembly`).
#[derive(Debug, Clone, PartialEq)]
pub enum Document {
    Primitive(Prim),
    Recipe(recipe::Recipe),
}

impl Document {
    /// A file's text, of any kind.
    pub fn parse(text_: &str) -> Result<Document, Error> {
        let doc = json::parse(text_)?;
        Ok(match kind(&doc)? {
            Kind::Primitive => Document::Primitive(Prim::from_json(&doc, "ops")?),
            Kind::Recipe => Document::Recipe(recipe::Recipe::from_json(&doc, false)?),
            Kind::Assembly => Document::Recipe(recipe::Recipe::from_json(&doc, true)?),
        })
    }
}

impl Prim {
    /// The file at `path`, or why not, saying which file.
    pub fn read(path: &Path) -> Result<Prim, Error> {
        let at = |e: &dyn std::fmt::Display| Error(format!("{}: {e}", path.display()));
        let text = std::fs::read_to_string(path).map_err(|e| at(&e))?;
        Prim::parse(&text).map_err(|e| at(&e))
    }

    /// A primitive file's text, checked as primlib's `loads` checks it (a
    /// recipe or assembly is refused: `Document::parse` reads any).
    pub fn parse(text_: &str) -> Result<Prim, Error> {
        let doc = json::parse(text_)?;
        match kind(&doc)? {
            Kind::Primitive => Prim::from_json(&doc, "ops"),
            Kind::Recipe => Err(Error("a recipe, not a primitive".into())),
            Kind::Assembly => Err(Error("an assembly, not a primitive".into())),
        }
    }

    /// A primitive document; or, with `nodes` "place", a recipe's names,
    /// checks and placements.
    pub(crate) fn from_json(doc: &Json, nodes: &str) -> Result<Prim, Error> {
        for key in ["params", "fixed", "derived", "checks", nodes, "result"] {
            if doc.get(key).is_none() {
                return Err(Error(format!("no {key:?}")));
            }
        }
        let params = list(doc, "params")?
            .iter()
            .map(|p| {
                let name = text(p, "name", "a parameter")?.to_string();
                let default = text(p, "default", &name)?.to_string();
                Ok(Param {
                    default,
                    note: opt_text(p, "note"),
                    group: p.get("group").and_then(Json::as_str).map(String::from),
                    name,
                })
            })
            .collect::<Result<_, Error>>()?;
        let named = |key: &str| {
            list(doc, key)?
                .iter()
                .map(|d| {
                    let name = text(d, "name", key)?.to_string();
                    Ok(Named { expr: text(d, "expr", &name)?.to_string(), note: opt_text(d, "note"), name })
                })
                .collect::<Result<Vec<_>, Error>>()
        };
        let checks = list(doc, "checks")?
            .iter()
            .map(|c| Ok(Check { expr: text(c, "expr", "a check")?.to_string(), message: opt_text(c, "message") }))
            .collect::<Result<_, Error>>()?;
        let values = match doc.get("values") {
            None | Some(Json::Null) => vec![],
            Some(v) => v
                .as_object()
                .ok_or_else(|| Error("\"values\" is not an object".into()))?
                .iter()
                .map(|(k, v)| match v {
                    Json::String(s) => Ok((k.clone(), s.clone())),
                    Json::Number(n) => Ok((k.clone(), format!("{n}"))),
                    _ => Err(Error(format!("value of {k} is not an expression"))),
                })
                .collect::<Result<_, Error>>()?,
        };
        let what = if nodes == "ops" { "operation" } else { "placement" };
        let ops = list(doc, nodes)?.to_vec();
        for (i, op) in ops.iter().enumerate() {
            if op.get("id").and_then(Json::as_index) != Some(i) {
                return Err(Error(format!("{what} {i} has id {:?}", op.get("id"))));
            }
        }
        let result = doc.get("result").and_then(Json::as_index).filter(|&r| r < ops.len());
        let result = result.ok_or_else(|| Error(format!("\"result\" is not an {what}")))?;
        Ok(Prim {
            primitive: text(doc, "primitive", "the file")?.to_string(),
            title: opt_text(doc, "title"),
            params,
            fixed: named("fixed")?,
            derived: named("derived")?,
            checks,
            values,
            ops,
            result,
        })
    }

    /// Every name (params, fixed, derived) → its value: each param from
    /// `values`, else the file's `values`, else its default. Names that
    /// aren't parameters are an error, listing them.
    pub fn names(&self, values: &[(String, String)]) -> Result<Env, Error> {
        let unknown: Vec<&str> =
            values.iter().map(|(k, _)| k.as_str()).filter(|k| !self.params.iter().any(|p| p.name == *k)).collect();
        if !unknown.is_empty() {
            return Err(Error(format!("{} has no parameter {}", self.primitive, unknown.join(", "))));
        }
        let given = |name: &str| values.iter().chain(&self.values).find(|(k, _)| k == name).map(|(_, v)| v.as_str());
        let mut env = Env::new();
        for p in &self.params {
            let v = given(&p.name).unwrap_or(&p.default);
            let v = match v.strip_prefix('\'') {
                Some(t) => Value::Text(t.to_string()),
                None => value(v.strip_prefix('=').unwrap_or(v), &env).map_err(|e| Error(format!("{}: {e}", p.name)))?,
            };
            env.set_value(&p.name, v);
        }
        for d in self.fixed.iter().chain(&self.derived) {
            let v = match d.expr.strip_prefix('\'') {
                Some(t) => Value::Text(t.to_string()),
                None => value(&d.expr, &env).map_err(|e| Error(format!("{}: {e}", d.name)))?,
            };
            env.set_value(&d.name, v);
        }
        Ok(env)
    }

    /// The part's pieces at `env` (from `names`): one solid, or a compound's
    /// named parts.
    pub fn build(&self, env: &Env) -> Result<Vec<build::Piece>, Error> {
        build::build(&self.ops, self.result, env)
    }

    /// The messages of the checks that fail (not over 0) at `env`.
    pub fn failed_checks(&self, env: &Env) -> Result<Vec<&str>, Error> {
        let mut out = vec![];
        for c in &self.checks {
            if evaluate(&c.expr, env).map_err(|e| Error(format!("check {:?}: {e}", c.expr)))?.partial_cmp(&0.0)
                != Some(std::cmp::Ordering::Greater)
            {
                out.push(c.message.as_str());
            }
        }
        Ok(out)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const TREE: &str = r#"{"format": "primlib.prim", "version": 1, "primitive": "t", "title": "T",
        "params": [{"name": "w", "default": "10"}, {"name": "h", "default": "=w / 2"},
                   {"name": "label", "default": "'OAK"}],
        "fixed": [{"name": "t", "expr": "2"}],
        "derived": [{"name": "area", "expr": "w * h"}, {"name": "inner", "expr": "w - 2 * t"}],
        "checks": [{"expr": "inner", "message": "w is too small"}],
        "ops": [{"id": 0, "op": "box", "name": null, "x": ["0", "w"], "y": ["0", "h"], "z": ["0", "t"]}],
        "result": 0, "motion": []}"#;

    fn num(env: &Env, n: &str) -> f64 {
        env.get(n).and_then(Value::number).unwrap()
    }

    // As primlib's tests/test_expr.py, Names.
    #[test]
    fn defaults_formulas_texts() {
        let p = Prim::parse(TREE).unwrap();
        let env = p.names(&[]).unwrap();
        assert_eq!([num(&env, "w"), num(&env, "h"), num(&env, "area"), num(&env, "inner")], [10.0, 5.0, 50.0, 6.0]);
        assert_eq!(env.get("label"), Some(&Value::Text("OAK".into())));
        assert!(p.failed_checks(&env).unwrap().is_empty());
    }

    #[test]
    fn values() {
        let p = Prim::parse(TREE).unwrap();
        let env = p.names(&[("w".into(), "4".into()), ("h".into(), "1".into())]).unwrap();
        assert_eq!((num(&env, "h"), num(&env, "area")), (1.0, 4.0));
        assert_eq!(p.failed_checks(&env).unwrap(), ["w is too small"]);
        assert_eq!(p.names(&[("x".into(), "1".into())]).unwrap_err().0, "t has no parameter x");
    }

    #[test]
    fn file_values_then_given() {
        let p = Prim::parse(&TREE.replace("\"result\": 0", "\"values\": {\"w\": \"20\"}, \"result\": 0")).unwrap();
        assert_eq!(num(&p.names(&[]).unwrap(), "area"), 200.0);
        assert_eq!(num(&p.names(&[("w".into(), "8".into())]).unwrap(), "area"), 32.0);
    }

    #[test]
    fn refused() {
        for (from, to, why) in [
            ("primlib.prim", "other", "not a primlib.prim file"),
            ("\"version\": 1", "\"version\": 3", "version 3: this reads up to 2"),
            ("\"version\": 1", "\"version\": 1, \"kind\": \"widget\"", "kind"),
            ("\"version\": 1", "\"version\": 2, \"kind\": \"assembly\"", "an assembly, not a primitive"),
            ("\"version\": 1", "\"version\": 2, \"kind\": \"recipe\"", "a recipe, not a primitive"),
            ("\"id\": 0", "\"id\": 1", "operation 0 has id"),
            ("\"result\": 0", "\"result\": 3", "\"result\" is not an operation"),
            ("\"checks\"", "\"cheques\"", "no \"checks\""),
        ] {
            let e = Prim::parse(&TREE.replace(from, to)).unwrap_err();
            assert!(e.0.starts_with(why), "{from} -> {to}: {e}");
        }
    }
}
