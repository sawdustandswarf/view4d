//! A `.prim` file's operations built into solids at given values: what
//! primlib's FreeCAD target makes, as closed meshes (`csg`). Each size is
//! evaluated from the names; `choose` and `series` are settled at every
//! build. An operation not built yet is named, and nothing is returned
//! half built.
//!
//! Curved faces are polygons of `segments(r)` sides, their radii grown so
//! each section has the circle's area: volumes come out as the true solid's.

use std::collections::HashMap;

use crate::Error;
use crate::csg::{Polygon, Solid, V3, cuboid, v3};
use crate::expr::{self, Env, Expr, Value};
use crate::json::Value as Json;
use crate::profile::{self, Vertex};
use crate::text::{self, Flat};

/// A built piece: a solid, and the name of the operation that made it
/// (passed on through `choose` and `move`), which a compound's parts keep.
#[derive(Debug, Clone, PartialEq)]
pub struct Piece {
    pub name: Option<String>,
    pub solid: Solid,
}

/// The largest distance (mm) a curved face's polygons may stand from the
/// true surface.
pub const TOLERANCE: f64 = 0.01;

/// How many sides a circle of radius `r` gets: enough for `TOLERANCE`, a
/// multiple of 4 (so there are sides at the axes' ends), 16 to 256.
pub fn segments(r: f64) -> usize {
    let r = r.abs();
    if r <= TOLERANCE {
        return 16;
    }
    let n = (std::f64::consts::PI / (1.0 - TOLERANCE / r).acos()).ceil() as usize;
    n.div_ceil(4).clamp(4, 64) * 4
}

/// The circumradius of an `n`-gon with the area of a circle of radius 1.
pub fn area_true(n: usize) -> f64 {
    let a = std::f64::consts::TAU / n as f64;
    (std::f64::consts::TAU / (n as f64 * a.sin())).sqrt()
}

/// Builds `ops` at `env`.
#[derive(Debug)]
pub struct Builder<'a> {
    ops: &'a [Json],
    exprs: HashMap<&'a str, Expr>,
    uses_index: Vec<bool>,
    made: HashMap<(usize, Option<u64>), Vec<Piece>>,
    flats: HashMap<(usize, Option<u64>), Flat>,
    fonts: HashMap<String, Vec<u8>>,
}

/// The fields that hold operations.
const REFS: [&str; 4] = ["base", "item", "then", "otherwise"];
const REF_LISTS: [&str; 2] = ["of", "tools"];
/// The fields that hold words, not expressions.
const WORDS: [&str; 10] = ["id", "op", "name", "axis", "plane", "join", "index", "along_axis", "ruled", "sides"];

fn strings<'a>(v: &'a Json, key: &str, out: &mut Vec<&'a str>) {
    match v {
        Json::String(s) if !WORDS.contains(&key) => out.push(s),
        Json::Array(a) => a.iter().for_each(|x| strings(x, key, out)),
        Json::Object(o) => o.iter().for_each(|(k, x)| strings(x, k, out)),
        _ => {}
    }
}

/// The operations `op` is made from: its fields, and the bounds it uses.
fn refs(op: &Json, exprs: &HashMap<&str, Expr>) -> Vec<usize> {
    let mut out: Vec<usize> = REFS.iter().filter_map(|k| op.get(k).and_then(Json::as_index)).collect();
    for k in REF_LISTS {
        out.extend(op.get(k).and_then(Json::as_array).unwrap_or_default().iter().filter_map(Json::as_index));
    }
    let mut ss = vec![];
    strings(op, "", &mut ss);
    for s in ss {
        if let Some(e) = exprs.get(s) {
            e.refs(&mut out);
        }
    }
    out
}

fn op_name(op: &Json) -> String {
    let id = op.get("id").and_then(Json::as_index).unwrap_or(0);
    let kind = op.get("op").and_then(Json::as_str).unwrap_or("?");
    match op.get("name").and_then(Json::as_str) {
        Some(n) => format!("operation {id} ({kind} {n})"),
        None => format!("operation {id} ({kind})"),
    }
}

fn index_key(env: &Env) -> Option<u64> {
    env.get("i").and_then(Value::number).map(f64::to_bits)
}

impl<'a> Builder<'a> {
    /// Ready to build `ops`: every expression parsed, and which operations
    /// depend on a series' index. An expression that doesn't parse, or an
    /// operation made from one after it, is an error.
    pub fn new(ops: &'a [Json]) -> Result<Builder<'a>, Error> {
        let mut exprs = HashMap::new();
        for op in ops {
            let mut ss = vec![];
            strings(op, "", &mut ss);
            for s in ss {
                if !exprs.contains_key(s) && !s.starts_with('\'') {
                    let e = expr::parse(s).map_err(|e| Error(format!("{}: {e}", op_name(op))))?;
                    exprs.insert(s, e);
                }
            }
        }
        let mut uses_index = Vec::with_capacity(ops.len());
        for (i, op) in ops.iter().enumerate() {
            let mut ss = vec![];
            strings(op, "", &mut ss);
            let own = ss.iter().any(|s| exprs.get(s).is_some_and(|e| e.uses("i")));
            let rs = refs(op, &exprs);
            if let Some(bad) = rs.iter().find(|&&r| r >= i) {
                return Err(Error(format!("{}: refers to operation {bad}, not one before it", op_name(op))));
            }
            uses_index.push(own || rs.iter().any(|&r| uses_index[r]));
        }
        Ok(Builder { ops, exprs, uses_index, made: HashMap::new(), flats: HashMap::new(), fonts: HashMap::new() })
    }

    /// Operation `id` at `env`: its pieces (one, or a compound's).
    pub fn build(&mut self, id: usize, env: &Env) -> Result<Vec<Piece>, Error> {
        let key = (id, if self.uses_index[id] { index_key(env) } else { None });
        if let Some(p) = self.made.get(&key) {
            return Ok(p.clone());
        }
        let op = &self.ops[id];
        let built = self
            .op(op, env)
            .map_err(|e| if e.0.starts_with("operation ") { e } else { Error(format!("{}: {e}", op_name(op))) })?;
        self.made.insert(key, built.clone());
        Ok(built)
    }

    /// Text operation `id` at `env`: a flat shape, which only `extrude`
    /// makes a solid of.
    fn flat(&mut self, id: usize, env: &Env) -> Result<Flat, Error> {
        let key = (id, if self.uses_index[id] { index_key(env) } else { None });
        if let Some(f) = self.flats.get(&key) {
            return Ok(f.clone());
        }
        let op = &self.ops[id];
        let made = self.text(op, env).map_err(|e| Error(format!("{}: {e}", op_name(op))))?;
        self.flats.insert(key, made.clone());
        Ok(made)
    }

    fn text(&mut self, op: &Json, env: &Env) -> Result<Flat, Error> {
        if op.get("op").and_then(Json::as_str) != Some("text") {
            return Err(Error("not a text: only a text is flat".into()));
        }
        let string = self.value(Self::word(op, "string")?, env)?;
        let font = self.value(Self::word(op, "font")?, env)?;
        let size = self.num(op, "size", env)?;
        let at = self.nums(Self::field(op, "at")?, env)?;
        let [x, y, z] = at[..] else { return Err(Error("\"at\" is not (x, y, z)".into())) };
        let justify = op.get("justify").map_or(Ok(4.0), |_| self.num(op, "justify", env))?;
        if !(0.0..=8.0).contains(&justify) || justify.fract() != 0.0 {
            return Err(Error(format!("justify {justify}: a whole number, 0 to 8")));
        }
        if !self.fonts.contains_key(&font) {
            let data = std::fs::read(&font).map_err(|e| Error(format!("the font {font}: {e}")))?;
            self.fonts.insert(font.clone(), data);
        }
        text::line(&self.fonts[&font], &string, size, [x, y, z], justify as usize)
    }

    /// An expression's value as text: a name holding text, a quoted
    /// literal ('OAK), or a number written out.
    fn value(&mut self, text: &str, env: &Env) -> Result<String, Error> {
        if let Some(literal) = text.strip_prefix('\'') {
            return Ok(literal.to_string());
        }
        let e = self.exprs.get(text).ok_or_else(|| Error(format!("{text:?} is not an expression")))?;
        match e.eval(env, None).map_err(|e| Error(format!("{text}: {e}")))? {
            Value::Text(t) => Ok(t),
            Value::Number(n) => Ok(format!("{n}")),
        }
    }

    fn solid(&mut self, id: usize, env: &Env) -> Result<Solid, Error> {
        Ok(Solid::union_all(self.build(id, env)?.into_iter().map(|p| p.solid).collect()))
    }

    // -- fields --------------------------------------------------------------

    fn field<'j>(op: &'j Json, key: &str) -> Result<&'j Json, Error> {
        op.get(key).ok_or_else(|| Error(format!("no {key:?}")))
    }

    fn eval(&mut self, text: &str, env: &Env) -> Result<f64, Error> {
        let e = self.exprs.get(text).ok_or_else(|| Error(format!("{text:?} is not an expression")))?.clone();
        let mut refs = vec![];
        e.refs(&mut refs);
        if refs.is_empty() {
            return e.number(env, None).map_err(|e| Error(format!("{text}: {e}")));
        }
        // xmin(@id) and the like: operation id built, its bounds.
        let cell = std::cell::RefCell::new(self);
        let extent = |id: usize, axis: usize, max: bool| -> Result<f64, expr::ExprError> {
            let mut b = cell.borrow_mut();
            let empty = || expr::ExprError(format!("@{id} is empty: it has no bounds"));
            if b.ops[id].get("op").and_then(Json::as_str) == Some("text") {
                let flat = b.flat(id, env).map_err(|e| expr::ExprError(e.0))?;
                let [x0, y0, x1, y1] = flat.bounds().ok_or_else(empty)?;
                return Ok([[x0, x1], [y0, y1], [flat.z, flat.z]][axis][usize::from(max)]);
            }
            let solid = b.solid(id, env).map_err(|e| expr::ExprError(e.0))?;
            let (lo, hi) = solid.bounds().ok_or_else(empty)?;
            Ok(if max { hi.at(axis) } else { lo.at(axis) })
        };
        e.number(env, Some(&extent)).map_err(|e| Error(format!("{text}: {e}")))
    }

    fn num(&mut self, op: &Json, key: &str, env: &Env) -> Result<f64, Error> {
        let v = Self::field(op, key)?;
        let s = v.as_str().ok_or_else(|| Error(format!("{key:?} is not an expression")))?;
        self.eval(s, env)
    }

    fn nums(&mut self, v: &Json, env: &Env) -> Result<Vec<f64>, Error> {
        let a = v.as_array().ok_or_else(|| Error("expected a list of expressions".into()))?;
        a.iter()
            .map(|x| match x {
                Json::String(s) => self.eval(s, env),
                Json::Null => Ok(0.0),
                Json::Number(n) => Ok(*n),
                _ => Err(Error("expected an expression".into())),
            })
            .collect()
    }

    fn span(&mut self, op: &Json, key: &str, env: &Env) -> Result<(f64, f64), Error> {
        match self.nums(Self::field(op, key)?, env)?.as_slice() {
            [a, b] => Ok((*a, *b)),
            _ => Err(Error(format!("{key:?} is not a span"))),
        }
    }

    /// A polygon: [a, b] or [a, b, radius] expressions.
    fn points(&mut self, v: &Json, env: &Env) -> Result<Vec<Vertex>, Error> {
        let a = v.as_array().ok_or_else(|| Error("points are not a list".into()))?;
        a.iter()
            .map(|p| match self.nums(p, env)?.as_slice() {
                [a, b] => Ok(Vertex { a: *a, b: *b, r: 0.0 }),
                [a, b, r] => Ok(Vertex { a: *a, b: *b, r: *r }),
                _ => Err(Error("a point is not (a, b) or (a, b, radius)".into())),
            })
            .collect()
    }

    fn word<'j>(op: &'j Json, key: &str) -> Result<&'j str, Error> {
        Self::field(op, key)?.as_str().ok_or_else(|| Error(format!("{key:?} is not a word")))
    }

    fn axis(op: &Json) -> Result<usize, Error> {
        match op.get("axis").and_then(Json::as_str) {
            Some("x") => Ok(0),
            Some("y") => Ok(1),
            Some("z") => Ok(2),
            a => Err(Error(format!("axis {a:?}"))),
        }
    }

    fn reference(op: &Json, key: &str) -> Result<usize, Error> {
        Self::field(op, key)?.as_index().ok_or_else(|| Error(format!("{key:?} is not an operation")))
    }

    fn references(op: &Json, key: &str) -> Result<Vec<usize>, Error> {
        let a = Self::field(op, key)?.as_array().ok_or_else(|| Error(format!("{key:?} is not a list")))?;
        a.iter()
            .map(|v| v.as_index().ok_or_else(|| Error(format!("{key:?} holds something not an operation"))))
            .collect()
    }

    fn count(&mut self, op: &Json, key: &str, env: &Env) -> Result<usize, Error> {
        let n = self.num(op, key, env)?;
        if (n - n.round()).abs() > 1e-6 || n < 0.0 {
            return Err(Error(format!("{key} {n} is not a whole number")));
        }
        Ok(n.round() as usize)
    }

    // -- operations ------------------------------------------------------------

    fn op(&mut self, op: &Json, env: &Env) -> Result<Vec<Piece>, Error> {
        let kind = op.get("op").and_then(Json::as_str).unwrap_or("");
        let solid = match kind {
            "box" => {
                let (x, y, z) = (self.span(op, "x", env)?, self.span(op, "y", env)?, self.span(op, "z", env)?);
                cuboid(v3(x.0, y.0, z.0), v3(x.1, y.1, z.1))
            }
            "beyond" => {
                let n = self.nums(Self::field(op, "normal")?, env)?;
                let n = v3(n[0], n[1], n[2]).unit();
                let (d, size) = (self.num(op, "d", env)?, self.num(op, "size", env)?);
                beyond(n, d, size)
            }
            "cylinder" | "cone" => {
                let axis = Self::axis(op)?;
                let c = self.nums(Self::field(op, "centre")?, env)?;
                let (lo, hi) = self.span(op, "span", env)?;
                let (r1, r2) = if kind == "cylinder" {
                    let r = self.num(op, "r", env)?;
                    (r, r)
                } else {
                    (self.num(op, "r1", env)?, self.num(op, "r2", env)?)
                };
                frustum(axis, (c[0], c[1]), (lo, hi), r1, r2)?
            }
            "slot" => {
                // Two cylinders and the box between, as FreeCAD's target makes it.
                let axis = Self::axis(op)?;
                let c = self.nums(Self::field(op, "centre")?, env)?;
                let (lo, hi) = self.span(op, "span", env)?;
                let (r, travel) = (self.num(op, "r", env)?, self.num(op, "travel", env)?);
                let others: Vec<usize> = (0..3).filter(|&a| a != axis).collect();
                let along = match Self::word(op, "along")? {
                    "x" => 0,
                    "y" => 1,
                    "z" => 2,
                    a => return Err(Error(format!("along {a:?}"))),
                };
                let k = others.iter().position(|&a| a == along).ok_or_else(|| Error("along its own axis".into()))?;
                let mut ends = vec![];
                for sign in [-1.0, 1.0] {
                    let mut at = [c[0], c[1]];
                    at[k] += sign * travel / 2.0;
                    ends.push(frustum(axis, (at[0], at[1]), (lo, hi), r, r)?);
                }
                let (mut b0, mut b1) = (V3::default().with(axis, lo), V3::default().with(axis, hi));
                for (i, &a) in others.iter().enumerate() {
                    let half = if i == k { travel / 2.0 } else { r };
                    b0 = b0.with(a, c[i] - half);
                    b1 = b1.with(a, c[i] + half);
                }
                ends.push(cuboid(b0, b1));
                Solid::union_all(ends)
            }
            "sector" => {
                let (r, span) = (self.num(op, "r", env)?, self.span(op, "span", env)?);
                let (start, angle) = (self.num(op, "start", env)?, self.num(op, "angle", env)?);
                profile::sector(r, span, start, angle)?
            }
            "polygon" => {
                let sides =
                    Self::field(op, "sides")?.as_index().ok_or_else(|| Error("\"sides\" is not a count".into()))?;
                let (r, span) = (self.num(op, "r", env)?, self.span(op, "span", env)?);
                profile::polygon(sides, r, span)?
            }
            "sphere" => {
                let c = self.nums(Self::field(op, "centre")?, env)?;
                profile::sphere(v3(c[0], c[1], c[2]), self.num(op, "r", env)?)?
            }
            "prism" => {
                let pts = self.points(Self::field(op, "points")?, env)?;
                profile::prism(&pts, Self::word(op, "plane")?, self.span(op, "span", env)?)?
            }
            "loft" => {
                let mut sections = vec![];
                for sec in Self::field(op, "sections")?.as_array().unwrap_or_default() {
                    sections.push((self.points(Self::field(sec, "points")?, env)?, self.num(sec, "at", env)?));
                }
                let refs: Vec<(&[Vertex], f64)> = sections.iter().map(|(p, at)| (p.as_slice(), *at)).collect();
                profile::loft(&refs, Self::word(op, "plane")?)?
            }
            "loft_about" => {
                let mut sections = vec![];
                for sec in Self::field(op, "sections")?.as_array().unwrap_or_default() {
                    sections.push((self.points(Self::field(sec, "points")?, env)?, self.num(sec, "angle", env)?));
                }
                let refs: Vec<(&[Vertex], f64)> = sections.iter().map(|(p, a)| (p.as_slice(), *a)).collect();
                profile::loft_about(&refs)?
            }
            "revolve" => {
                let pts = self.points(Self::field(op, "points")?, env)?;
                profile::revolve(&pts, self.span(op, "span", env)?)?
            }
            "move" => {
                let d = self.nums(Self::field(op, "by")?, env)?;
                let base = Self::reference(op, "base")?;
                let pieces = self.build(base, env)?;
                return Ok(pieces
                    .into_iter()
                    .map(|p| Piece { name: p.name, solid: p.solid.translated(v3(d[0], d[1], d[2])) })
                    .collect());
            }
            "turn" => {
                let axis = match Self::field(op, "axis")?.as_str() {
                    Some("x") => 0,
                    Some("y") => 1,
                    Some("z") => 2,
                    a => return Err(Error(format!("turn about {a:?}"))),
                };
                let m = crate::recipe::turn(axis, self.num(op, "angle", env)?);
                let base = Self::reference(op, "base")?;
                let pieces = self.build(base, env)?;
                return Ok(pieces.into_iter().map(|p| Piece { name: p.name, solid: p.solid.turned(&m) }).collect());
            }
            "array" => {
                let base = self.solid(Self::reference(op, "base")?, env)?;
                let along =
                    Self::field(op, "along")?.as_object().ok_or_else(|| Error("\"along\" is not an object".into()))?;
                let mut copies = vec![base];
                for (axis, a) in along {
                    let i = match axis.as_str() {
                        "x" => 0,
                        "y" => 1,
                        "z" => 2,
                        _ => return Err(Error(format!("array along {axis:?}"))),
                    };
                    let n = self.count(a, "count", env)?;
                    let (start, step) = (self.num(a, "start", env)?, self.num(a, "step", env)?);
                    let mut next = Vec::with_capacity(copies.len() * n);
                    for c in &copies {
                        for k in 0..n {
                            next.push(c.clone().translated(V3::default().with(i, start + k as f64 * step)));
                        }
                    }
                    copies = next;
                }
                Solid::union_all(copies)
            }
            "polar" => {
                let base = self.solid(Self::reference(op, "base")?, env)?;
                let n = self.count(op, "count", env)?;
                let angle = self.num(op, "angle", env)?;
                let step = if (angle - 360.0).abs() < 1e-9 { angle / n as f64 } else { angle / (n.max(2) - 1) as f64 };
                Solid::union_all((0..n).map(|k| base.clone().rotated_z(k as f64 * step)).collect())
            }
            "fuse" | "common" | "compound" => {
                let of = Self::references(op, "of")?;
                if kind == "compound" {
                    let mut out = vec![];
                    for id in of {
                        out.extend(self.build(id, env)?);
                    }
                    return Ok(out);
                }
                let mut solids = vec![];
                for id in of {
                    solids.push(self.solid(id, env)?);
                }
                if kind == "fuse" {
                    Solid::union_all(solids)
                } else {
                    let mut it = solids.into_iter();
                    let first = it.next().unwrap_or_default();
                    it.fold(first, Solid::intersect)
                }
            }
            "cut" => {
                let base = self.solid(Self::reference(op, "base")?, env)?;
                let mut tools = vec![];
                for id in Self::references(op, "tools")? {
                    tools.push(self.solid(id, env)?);
                }
                base.subtract(Solid::union_all(tools))
            }
            "choose" => {
                let guard = self.num(op, "guard", env)?;
                let next = Self::reference(op, if guard > 0.0 { "then" } else { "otherwise" })?;
                return self.build(next, env);
            }
            "series" => {
                let n = self.num(op, "count", env)?.trunc();
                if n < 1.0 {
                    return Err(Error(format!("a count of {n}: nothing to build")));
                }
                let index = op.get("index").and_then(Json::as_str).unwrap_or("i");
                if index != "i" {
                    return Err(Error(format!("index {index:?}: only \"i\" is read")));
                }
                let item = Self::reference(op, "item")?;
                let mut pieces = vec![];
                for k in 0..n as usize {
                    let mut env = env.clone();
                    env.set("i", k as f64);
                    let built = self.build(item, &env)?;
                    pieces.push(built);
                }
                if op.get("join").and_then(Json::as_str) == Some("compound") {
                    return Ok(pieces.into_iter().flatten().collect());
                }
                Solid::union_all(pieces.into_iter().flatten().map(|p| p.solid).collect())
            }
            "text" => return Err(Error("a text is flat: extrude it to make a solid".into())),
            "extrude" => {
                let flat = self.flat(Self::reference(op, "base")?, env)?;
                let length = self.num(op, "length", env)?;
                let up = match op.get("z") {
                    None | Some(Json::Null) => 0.0,
                    Some(_) => self.num(op, "z", env)?,
                };
                let (lo, hi) = (flat.z + up, flat.z + up + length);
                text::extrude(&flat, if length < 0.0 { (hi, lo) } else { (lo, hi) })?
            }
            _ => return Err(Error(format!("{}: {kind:?} is not built yet", op_name(op)))),
        };
        let name = op.get("name").and_then(Json::as_str).map(String::from);
        Ok(vec![Piece { name, solid }])
    }
}

/// Everything beyond the plane n · p = `d`, as FreeCAD's target makes it: a
/// cube `size` across, one face on the plane.
fn beyond(n: V3, d: f64, size: f64) -> Solid {
    // Any two axes square to n and each other.
    let t = if n.x.abs() < 0.9 { v3(1.0, 0.0, 0.0) } else { v3(0.0, 1.0, 0.0) };
    let u = n.cross(t).unit();
    let v = n.cross(u);
    let o = n * d - (u + v) * (size / 2.0);
    let p = |a: f64, b: f64, c: f64| o + u * (a * size) + v * (b * size) + n * (c * size);
    let c: Vec<V3> = (0..8).map(|i| p((i & 1) as f64, ((i >> 1) & 1) as f64, ((i >> 2) & 1) as f64)).collect();
    // u, v, n is right handed, so the faces wind as cuboid's.
    let faces = [[0, 4, 6, 2], [1, 3, 7, 5], [0, 1, 5, 4], [2, 6, 7, 3], [0, 2, 3, 1], [4, 5, 7, 6]];
    Solid::new(faces.iter().filter_map(|f| Polygon::new(f.iter().map(|&i| c[i]).collect())).collect())
}

/// A truncated cone (a cylinder when `r1` = `r2`) along `axis`, its axis at
/// `centre` (the other two coordinates in xyz order), radius `r1` at
/// `span.0` and `r2` at `span.1`.
fn frustum(axis: usize, centre: (f64, f64), (lo, hi): (f64, f64), r1: f64, r2: f64) -> Result<Solid, Error> {
    if hi <= lo || r1 < 0.0 || r2 < 0.0 || (r1 == 0.0 && r2 == 0.0) {
        return Err(Error(format!("no solid: span {lo}..{hi}, radii {r1} and {r2}")));
    }
    let n = segments(r1.max(r2));
    let k = area_true(n);
    // The axes (a, b) round it, so that (a, b, axis) is right handed.
    let (a, b) = ((axis + 1) % 3, (axis + 2) % 3);
    let (ca, cb) = if a < b { (centre.0, centre.1) } else { (centre.1, centre.0) };
    let ring = |r: f64, h: f64| -> Vec<V3> {
        (0..n)
            .map(|i| {
                let t = std::f64::consts::TAU * i as f64 / n as f64;
                V3::default().with(a, ca + r * k * t.cos()).with(b, cb + r * k * t.sin()).with(axis, h)
            })
            .collect()
    };
    let (bottom, top) = (ring(r1, lo), ring(r2, hi));
    let mut polys = vec![];
    if r1 > 0.0 {
        polys.extend(Polygon::new(bottom.iter().rev().copied().collect()));
    }
    if r2 > 0.0 {
        polys.extend(Polygon::new(top.clone()));
    }
    for i in 0..n {
        let j = (i + 1) % n;
        let apex = |h: f64| V3::default().with(a, ca).with(b, cb).with(axis, h);
        // A cone's sides meet at its apex.
        let side = if r2 == 0.0 {
            vec![bottom[i], bottom[j], apex(hi)]
        } else if r1 == 0.0 {
            vec![apex(lo), top[j], top[i]]
        } else {
            vec![bottom[i], bottom[j], top[j], top[i]]
        };
        polys.extend(Polygon::new(side));
    }
    Ok(Solid::new(polys))
}

/// The pieces of `ops` from `result`, at `env`.
pub fn build(ops: &[Json], result: usize, env: &Env) -> Result<Vec<Piece>, Error> {
    Builder::new(ops)?.build(result, env)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::json;

    fn close(a: f64, b: f64, rel: f64) -> bool {
        (a - b).abs() <= rel * a.abs().max(b.abs())
    }

    fn ops(text: &str) -> Vec<Json> {
        json::parse(text).unwrap().as_array().unwrap().to_vec()
    }

    #[test]
    fn sides() {
        assert_eq!(segments(0.001), 16);
        assert!(segments(3.0).is_multiple_of(4) && segments(3.0) >= 36);
        assert_eq!(segments(1000.0), 256);
    }

    #[test]
    fn round_solids_have_their_volumes() {
        for axis in 0..3 {
            let c = frustum(axis, (1.0, 2.0), (0.0, 10.0), 3.0, 3.0).unwrap();
            assert!(close(c.volume(), std::f64::consts::PI * 90.0, 1e-9), "{}", c.volume());
            let k = frustum(axis, (0.0, 0.0), (-1.0, 2.0), 2.0, 1.0).unwrap();
            assert!(close(k.volume(), std::f64::consts::PI * 3.0 / 3.0 * (4.0 + 1.0 + 2.0), 1e-9));
            let apex = frustum(axis, (0.0, 0.0), (0.0, 3.0), 2.0, 0.0).unwrap();
            assert!(close(apex.volume(), std::f64::consts::PI * 4.0, 1e-9));
            // The axis where it should be.
            let (lo, hi) = c.bounds().unwrap();
            assert!(close(lo.at(axis), 0.0, 1e-12) && close(hi.at(axis), 10.0, 1e-12));
        }
    }

    #[test]
    fn beyond_is_beyond() {
        let n = v3(1.0, 1.0, 0.0).unit();
        let b = beyond(n, 2.0, 10.0);
        assert!(close(b.volume(), 1000.0, 1e-9));
        for p in &b.polys {
            for v in &p.verts {
                assert!(n.dot(*v) >= 2.0 - 1e-9);
            }
        }
    }

    #[test]
    fn a_tree() {
        // A plate with a row of three holes, moved; the count from a name.
        let ops = ops(r#"[
            {"id": 0, "op": "box", "name": "plate", "x": ["0", "w"], "y": ["0", "10"], "z": ["0", "2"]},
            {"id": 1, "op": "cylinder", "name": "hole", "axis": "z", "centre": ["0", "5"], "r": "1", "span": ["-1", "3"]},
            {"id": 2, "op": "array", "name": "holes", "base": 1, "along": {"x": {"count": "n", "start": "5", "step": "10"}}},
            {"id": 3, "op": "cut", "name": null, "base": 0, "tools": [2]},
            {"id": 4, "op": "move", "name": null, "base": 3, "by": [null, null, "1"]}
        ]"#);
        let mut env = Env::new();
        env.set("w", 30.0);
        env.set("n", 3.0);
        let p = build(&ops, 4, &env).unwrap();
        assert_eq!(p.len(), 1);
        let want = 30.0 * 10.0 * 2.0 - 3.0 * std::f64::consts::PI * 2.0;
        assert!(close(p[0].solid.volume(), want, 1e-9), "{}", p[0].solid.volume());
        assert!(close(p[0].solid.bounds().unwrap().0.z, 1.0, 1e-12));
        env.set("n", 2.5);
        assert!(build(&ops, 4, &env).unwrap_err().0.contains("not a whole number"));
    }

    #[test]
    fn turned_about_an_axis() {
        // A box 1 x 2 x 3, a quarter about z (x -> y), then a half about x (y -> -y, z -> -z).
        let ops = ops(r#"[
            {"id": 0, "op": "box", "name": null, "x": ["0", "1"], "y": ["0", "2"], "z": ["0", "w"]},
            {"id": 1, "op": "turn", "name": null, "base": 0, "axis": "z", "angle": "90"},
            {"id": 2, "op": "turn", "name": null, "base": 1, "axis": "x", "angle": "180"}
        ]"#);
        let mut env = Env::new();
        env.set("w", 3.0);
        let p = build(&ops, 2, &env).unwrap();
        let (lo, hi) = p[0].solid.bounds().unwrap();
        assert_eq!((lo.x, lo.y, lo.z, hi.x, hi.y, hi.z), (-2.0, -1.0, -3.0, 0.0, 0.0, 0.0));
        assert!(close(p[0].solid.volume(), 6.0, 1e-12), "{}", p[0].solid.volume());
    }

    #[test]
    fn choices_series_and_parts() {
        let ops = ops(r#"[
            {"id": 0, "op": "box", "name": "a", "x": ["0", "1"], "y": ["0", "1"], "z": ["0", "1"]},
            {"id": 1, "op": "box", "name": "b", "x": ["0", "2"], "y": ["0", "1"], "z": ["0", "1"]},
            {"id": 2, "op": "choose", "name": null, "guard": "big", "then": 1, "otherwise": 0},
            {"id": 3, "op": "box", "name": "ring", "x": ["3 * i", "3 * i + 1"], "y": ["0", "1"], "z": ["0", "1"]},
            {"id": 4, "op": "series", "name": "rings", "count": "n", "index": "i", "item": 3, "join": "fuse"},
            {"id": 5, "op": "compound", "name": null, "of": [2, 4]},
            {"id": 6, "op": "text", "name": "label"}
        ]"#);
        let mut env = Env::new();
        env.set("big", 1.0);
        env.set("n", 4.7);
        let p = build(&ops, 5, &env).unwrap();
        assert_eq!(p.iter().map(|p| p.name.as_deref()).collect::<Vec<_>>(), [Some("b"), Some("rings")]);
        assert!(close(p[0].solid.volume(), 2.0, 1e-12) && close(p[1].solid.volume(), 4.0, 1e-12));
        env.set("big", 0.0);
        assert!(close(build(&ops, 5, &env).unwrap()[0].solid.volume(), 1.0, 1e-12));
        assert_eq!(
            build(&ops, 6, &env).unwrap_err().0,
            "operation 6 (text label): a text is flat: extrude it to make a solid"
        );
        let back = ops_with(r#"{"id": 0, "op": "move", "name": null, "base": 0, "by": [null, null, null]}"#);
        assert!(Builder::new(&back).is_err());
    }

    fn ops_with(op: &str) -> Vec<Json> {
        ops(&format!("[{op}]"))
    }
}
