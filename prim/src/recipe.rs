//! primlib's recipes and assemblies (FORMAT.md's "Recipes" and
//! "Assemblies"): parts, each a primitive (or, in an assembly, a recipe or
//! assembly) at values bound to the recipe's names, placed by key, row,
//! series, choice and joint. Laid out here as `primlib/recipe.py`'s
//! `layout` lays them out.

use std::collections::HashMap;

use crate::expr::{self, Env, Value};
use crate::json::Value as Json;
use crate::{Error, Prim};

/// A part: a primitive (or recipe, or assembly) at values, mirrored or
/// not, in a colour or not, some of a compound's pieces or all, to print
/// or not.
#[derive(Debug, Clone, PartialEq)]
pub struct Part {
    pub name: String,
    pub primitive: String,
    /// Param → expression over the recipe's names.
    pub bind: Vec<(String, String)>,
    /// 0–2: mirrored across the plane through its origin normal to that axis.
    pub mirror: Option<usize>,
    pub colour: Option<[f64; 3]>,
    /// The names of the compound's pieces it takes, or all of it.
    pub pieces: Option<Vec<String>>,
    /// On the print list (a recipe or assembly: all its primitives).
    pub print: bool,
}

/// What a part uses: a primitive, or (in an assembly) a recipe or assembly.
#[derive(Debug, Clone, PartialEq)]
pub enum Used {
    Primitive(Prim),
    Recipe(Box<Recipe>),
}

/// A recipe or assembly file.
#[derive(Debug, Clone, PartialEq)]
pub struct Recipe {
    /// Its names and checks, as a primitive's: the `ops` are the placements,
    /// `result` the one that is the recipe.
    pub head: Prim,
    /// `kind: assembly`: its parts may be recipes or assemblies.
    pub assembly: bool,
    pub slots: usize,
    pub parts: Vec<Part>,
    /// The documents the parts use.
    pub uses: HashMap<String, Used>,
}

/// A primitive part, nested ones by path (`edges/S`), as `layout`'s
/// `Placed::part` names them, with its values.
#[derive(Debug, Clone, PartialEq)]
pub struct Leaf<'a> {
    pub path: String,
    pub prim: &'a Prim,
    pub values: Vec<(String, String)>,
    pub mirror: Option<usize>,
    pub colour: Option<[f64; 3]>,
    pub pieces: Option<&'a [String]>,
    pub print: bool,
}

/// A rotation (rows) and a place.
pub type Rotation = [[f64; 3]; 3];

pub const IDENTITY: Rotation = [[1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0]];

/// A part placed: turned by `rotation`, then at `at`. `name` says where
/// it came from (a row's name and slot: `edge_2F`).
#[derive(Debug, Clone, PartialEq)]
pub struct Placed {
    pub name: String,
    pub part: String,
    pub rotation: Rotation,
    pub at: [f64; 3],
}

fn mul(a: &Rotation, b: &Rotation) -> Rotation {
    let mut m = [[0.0; 3]; 3];
    for (i, row) in m.iter_mut().enumerate() {
        for (j, v) in row.iter_mut().enumerate() {
            *v = (0..3).map(|k| a[i][k] * b[k][j]).sum();
        }
    }
    m
}

fn add(a: [f64; 3], b: [f64; 3]) -> [f64; 3] {
    [a[0] + b[0], a[1] + b[1], a[2] + b[2]]
}

pub fn apply(m: &Rotation, v: [f64; 3]) -> [f64; 3] {
    [0, 1, 2].map(|i| (0..3).map(|k| m[i][k] * v[k]).sum())
}

/// About `axis` (0–2) by `degrees`, anticlockwise seen from its + end;
/// quarter turns exact.
pub fn turn(axis: usize, degrees: f64) -> Rotation {
    let q = degrees / 90.0;
    let (c, s) = if q == q.round() {
        [(1.0, 0.0), (0.0, 1.0), (-1.0, 0.0), (0.0, -1.0)][(q.round() as i64).rem_euclid(4) as usize]
    } else {
        let (s, c) = degrees.to_radians().sin_cos();
        (c, s)
    };
    match axis {
        0 => [[1.0, 0.0, 0.0], [0.0, c, -s], [0.0, s, c]],
        1 => [[c, 0.0, s], [0.0, 1.0, 0.0], [-s, 0.0, c]],
        _ => [[c, -s, 0.0], [s, c, 0.0], [0.0, 0.0, 1.0]],
    }
}

/// The mirror across the plane normal to `axis` through the origin.
pub fn mirror(axis: usize) -> Rotation {
    let mut m = IDENTITY;
    m[axis][axis] = -1.0;
    m
}

fn axis(name: &str) -> Option<usize> {
    ["x", "y", "z"].iter().position(|a| *a == name)
}

fn text<'a>(v: &'a Json, key: &str, what: &str) -> Result<&'a str, Error> {
    v.get(key).and_then(Json::as_str).ok_or_else(|| Error(format!("{what}: no {key:?}")))
}

impl Recipe {
    pub(crate) fn from_json(doc: &Json, assembly: bool) -> Result<Recipe, Error> {
        for key in ["parts", "uses", "slots"] {
            if doc.get(key).is_none() {
                return Err(Error(format!("no {key:?}")));
            }
        }
        let head = Prim::from_json(doc, "place")?;
        let mut uses = HashMap::new();
        for (name, sub) in
            doc.get("uses").and_then(Json::as_object).ok_or_else(|| Error("\"uses\" is not an object".into()))?
        {
            let used = match crate::kind(sub) {
                Ok(crate::Kind::Primitive) => Prim::from_json(sub, "ops").map(Used::Primitive),
                Ok(_) if !assembly => Err(Error("a recipe uses primitives only".into())),
                Ok(k) => Recipe::from_json(sub, k == crate::Kind::Assembly).map(|r| Used::Recipe(Box::new(r))),
                Err(e) => Err(e),
            };
            uses.insert(name.clone(), used.map_err(|e| Error(format!("uses {name}: {e}")))?);
        }
        let parts = doc
            .get("parts")
            .and_then(Json::as_array)
            .ok_or_else(|| Error("\"parts\" is not a list".into()))?
            .iter()
            .map(|p| {
                let name = text(p, "name", "a part")?.to_string();
                let what = format!("part {name}");
                let primitive = text(p, "primitive", &what)?.to_string();
                if !uses.contains_key(&primitive) {
                    return Err(Error(format!("{what}: its primitive {primitive:?} isn't in uses")));
                }
                let bind = p
                    .get("bind")
                    .and_then(Json::as_object)
                    .ok_or_else(|| Error(format!("{what}: no \"bind\"")))?
                    .iter()
                    .map(|(k, v)| {
                        v.as_str()
                            .map(|v| (k.clone(), v.to_string()))
                            .ok_or_else(|| Error(format!("{what}: {k} is not an expression")))
                    })
                    .collect::<Result<_, _>>()?;
                let mirror = match p.get("mirror") {
                    None | Some(Json::Null) => None,
                    Some(m) => Some(m.as_str().and_then(axis).ok_or_else(|| Error(format!("{what}: mirror {m:?}")))?),
                };
                let colour = match p.get("colour").and_then(Json::as_array) {
                    Some([r, g, b]) => Some([r, g, b].map(|c| c.as_f64().unwrap_or(0.0).clamp(0.0, 1.0))),
                    _ => None,
                };
                let pieces = match p.get("pieces") {
                    None | Some(Json::Null) => None,
                    Some(Json::Array(a)) => Some(
                        a.iter()
                            .map(|n| n.as_str().map(String::from).ok_or_else(|| Error(format!("{what}: pieces {n:?}"))))
                            .collect::<Result<_, _>>()?,
                    ),
                    Some(v) => return Err(Error(format!("{what}: pieces {v:?}"))),
                };
                let print = matches!(p.get("print"), Some(Json::Bool(true)));
                Ok(Part { name, primitive, bind, mirror, colour, pieces, print })
            })
            .collect::<Result<_, Error>>()?;
        let slots =
            doc.get("slots").and_then(Json::as_index).ok_or_else(|| Error("\"slots\" is not a count".into()))?;
        Ok(Recipe { head, assembly, slots, parts, uses })
    }

    /// The recipe's names at `values` over its own (see `Prim::names`).
    pub fn names(&self, values: &[(String, String)]) -> Result<Env, Error> {
        self.head.names(values)
    }

    /// Each part's primitive values (param → expression) at `env`.
    pub fn part_values(&self, env: &Env) -> Result<Vec<Vec<(String, String)>>, Error> {
        self.parts
            .iter()
            .map(|p| {
                p.bind
                    .iter()
                    .map(|(k, e)| {
                        let v = expr::parse(e)
                            .and_then(|x| x.eval(env, None))
                            .map_err(|err| Error(format!("part {}: {k}: {err}", p.name)))?;
                        Ok((
                            k.clone(),
                            match v {
                                Value::Number(n) => format!("{n:?}"),
                                Value::Text(t) => format!("'{t}"),
                            },
                        ))
                    })
                    .collect()
            })
            .collect()
    }

    /// The checks that fail at `env`: the recipe's own, then (if those
    /// pass) each part's primitive's at the part's values, named by part.
    pub fn failed_checks(&self, env: &Env) -> Result<Vec<String>, Error> {
        let own: Vec<String> = self.head.failed_checks(env)?.into_iter().map(String::from).collect();
        if !own.is_empty() {
            return Ok(own);
        }
        let mut out = vec![];
        for (part, values) in self.parts.iter().zip(self.part_values(env)?) {
            let named = |e: Error| Error(format!("part {}: {e}", part.name));
            let failed = match &self.uses[&part.primitive] {
                Used::Primitive(prim) => {
                    prim.failed_checks(&prim.names(&values).map_err(named)?)?.into_iter().map(String::from).collect()
                }
                Used::Recipe(r) => r.failed_checks(&r.names(&values).map_err(named)?)?,
            };
            out.extend(failed.into_iter().map(|m| format!("part {}: {m}", part.name)));
        }
        Ok(out)
    }

    /// The parts placed at `env`: a part that is a recipe or assembly laid
    /// out where it is placed, its own named `{placed}/{its name}`, part
    /// `{part}/{its part}`.
    pub fn layout(&self, env: &Env) -> Result<Vec<Placed>, Error> {
        let placed = self.place(self.head.result, env)?;
        if !self.parts.iter().any(|p| matches!(self.uses[&p.primitive], Used::Recipe(_))) {
            return Ok(placed);
        }
        let values = self.part_values(env)?;
        let mut inner: HashMap<&str, Vec<Placed>> = HashMap::new();
        for (part, vals) in self.parts.iter().zip(&values) {
            if let Used::Recipe(r) = &self.uses[&part.primitive] {
                let named = |e: Error| Error(format!("part {}: {e}", part.name));
                inner.insert(&part.name, r.names(vals).and_then(|e| r.layout(&e)).map_err(named)?);
            }
        }
        let mut out = vec![];
        for p in placed {
            let Some(qs) = inner.get(p.part.as_str()) else {
                out.push(p);
                continue;
            };
            for q in qs {
                out.push(Placed {
                    name: format!("{}/{}", p.name, q.name),
                    part: format!("{}/{}", p.part, q.part),
                    rotation: mul(&p.rotation, &q.rotation),
                    at: add(apply(&p.rotation, q.at), p.at),
                });
            }
        }
        Ok(out)
    }

    /// Every primitive part at `env`, nested ones by path, in order.
    pub fn leaves(&self, env: &Env) -> Result<Vec<Leaf<'_>>, Error> {
        self.leaves_under(env, "", false)
    }

    fn leaves_under(&self, env: &Env, prefix: &str, printed: bool) -> Result<Vec<Leaf<'_>>, Error> {
        let mut out = vec![];
        for (part, values) in self.parts.iter().zip(self.part_values(env)?) {
            let path = format!("{prefix}{}", part.name);
            let print = printed || part.print;
            match &self.uses[&part.primitive] {
                Used::Primitive(prim) => out.push(Leaf {
                    path,
                    prim,
                    values,
                    mirror: part.mirror,
                    colour: part.colour,
                    pieces: part.pieces.as_deref(),
                    print,
                }),
                Used::Recipe(r) => {
                    let env = r.names(&values).map_err(|e| Error(format!("part {}: {e}", part.name)))?;
                    out.extend(r.leaves_under(&env, &format!("{path}/"), print)?);
                }
            }
        }
        Ok(out)
    }

    fn place(&self, i: usize, env: &Env) -> Result<Vec<Placed>, Error> {
        let node = &self.head.ops[i];
        let kind = node.get("op").and_then(Json::as_str).unwrap_or("");
        let name = node.get("name").and_then(Json::as_str);
        let what = format!("placement {i} ({kind}{})", name.map(|n| format!(" {n}")).unwrap_or_default());
        let at = |e: Error| Error(format!("{what}: {e}"));
        let num = |key: &str, env: &Env| -> Result<f64, Error> {
            let t = text(node, key, &what)?;
            expr::parse(t).and_then(|x| x.number(env, None)).map_err(|e| Error(format!("{what}: {key}: {e}")))
        };
        let reference = |key: &str| -> Result<usize, Error> {
            let r = node
                .get(key)
                .and_then(Json::as_index)
                .ok_or_else(|| Error(format!("{what}: {key:?} is not a placement")))?;
            if r >= i {
                return Err(Error(format!("{what}: refers to placement {r}, not one before it")));
            }
            Ok(r)
        };
        Ok(match kind {
            "instance" => {
                let part = text(node, "part", &what)?.to_string();
                vec![Placed {
                    name: name.unwrap_or(&part).to_string(),
                    rotation: self.turns(node, env).map_err(at)?,
                    at: self.point(node, "at", env).map_err(at)?,
                    part,
                }]
            }
            "row" => self.row(node, name.unwrap_or("row"), env).map_err(at)?,
            "series" => {
                let n = num("count", env)?.trunc().max(0.0) as usize;
                let index = text(node, "index", &what)?;
                let item = reference("item")?;
                let mut out = vec![];
                for k in 0..n {
                    let mut env = env.clone();
                    env.set(index, k as f64);
                    out.extend(self.place(item, &env)?);
                }
                out
            }
            "choose" => {
                let key = if num("guard", env)? > 0.0 { "then" } else { "otherwise" };
                match node.get(key) {
                    None | Some(Json::Null) => vec![],
                    Some(_) => self.place(reference(key)?, env)?,
                }
            }
            "group" => {
                let of = node.get("of").and_then(Json::as_array).ok_or_else(|| Error(format!("{what}: no \"of\"")))?;
                let mut out = vec![];
                for r in of {
                    let r = r
                        .as_index()
                        .filter(|&r| r < i)
                        .ok_or_else(|| Error(format!("{what}: {r:?} is not a placement before it")))?;
                    out.extend(self.place(r, env)?);
                }
                out
            }
            "move" => {
                let m = self.turns(node, env).map_err(at)?;
                let by = self.point(node, "by", env).map_err(at)?;
                self.place(reference("base")?, env)?
                    .into_iter()
                    .map(|p| {
                        let a = apply(&m, p.at);
                        Placed { rotation: mul(&m, &p.rotation), at: [a[0] + by[0], a[1] + by[1], a[2] + by[2]], ..p }
                    })
                    .collect()
            }
            "joint" => {
                let ax = text(node, "axis", &what).ok().and_then(axis);
                let ax = ax.ok_or_else(|| Error(format!("{what}: axis {:?}", node.get("axis"))))?;
                let m = turn(ax, num("angle", env)?);
                let a = self.point(node, "at", env).map_err(at)?;
                // About the line through a: p' = m (p - a) + a.
                let ma = apply(&m, a);
                let by = [a[0] - ma[0], a[1] - ma[1], a[2] - ma[2]];
                self.place(reference("base")?, env)?
                    .into_iter()
                    .map(|p| Placed { rotation: mul(&m, &p.rotation), at: add(apply(&m, p.at), by), ..p })
                    .collect()
            }
            _ => return Err(Error(format!("{what}: unknown placement"))),
        })
    }

    fn point(&self, node: &Json, key: &str, env: &Env) -> Result<[f64; 3], Error> {
        let a = node.get(key).and_then(Json::as_array).ok_or_else(|| Error(format!("no {key:?}")))?;
        let [x, y, z] = a else { return Err(Error(format!("{key:?} is not (x, y, z)"))) };
        let n = |v: &Json| -> Result<f64, Error> {
            let t = v.as_str().ok_or_else(|| Error(format!("{key:?} holds something not an expression")))?;
            Ok(expr::parse(t)?.number(env, None)?)
        };
        Ok([n(x)?, n(y)?, n(z)?])
    }

    fn turns(&self, node: &Json, env: &Env) -> Result<Rotation, Error> {
        let mut m = IDENTITY;
        for t in node.get("turn").and_then(Json::as_array).unwrap_or_default() {
            let a = t
                .get("axis")
                .and_then(Json::as_str)
                .and_then(axis)
                .ok_or_else(|| Error(format!("a turn about {:?}", t.get("axis"))))?;
            let angle = expr::parse(text(t, "angle", "a turn")?)?.number(env, None)?;
            m = mul(&turn(a, angle), &m);
        }
        Ok(m)
    }

    fn row(&self, node: &Json, name: &str, env: &Env) -> Result<Vec<Placed>, Error> {
        let key = match expr::parse(text(node, "key", "a row")?)?.eval(env, None)? {
            Value::Text(t) => t,
            Value::Number(n) => return Err(Error(format!("its key is {n}, a number, not text"))),
        };
        let along = text(node, "along", "a row")?;
        let (sign, ax) = match along.strip_prefix('-') {
            Some(a) => (-1.0, axis(a)),
            None => (1.0, axis(along)),
        };
        let ax = ax.ok_or_else(|| Error(format!("along {along:?}")))?;
        let pieces = node.get("pieces").and_then(Json::as_object).ok_or_else(|| Error("no \"pieces\"".into()))?;
        let num = |t: &str| -> Result<f64, Error> { Ok(expr::parse(t)?.number(env, None)?) };
        let start = self.point(node, "at", env)?;
        let gap = num(text(node, "gap", "a row")?)?;
        let m = self.turns(node, env)?;
        let mut s = num(text(node, "lead", "a row")?)?;
        let mut out = vec![];
        for (k, c) in key.chars().enumerate() {
            let Some((_, piece)) = pieces.iter().find(|(l, _)| l.chars().eq([c])) else {
                let letters: Vec<&str> = pieces.iter().map(|(l, _)| l.as_str()).collect();
                return Err(Error(format!("its key {key:?} has {c:?}, not one of {}", letters.join(" "))));
            };
            let mut at = start;
            at[ax] += sign * s;
            out.push(Placed {
                name: format!("{name}_{}{c}", k + 1),
                part: text(piece, "part", "a piece")?.to_string(),
                rotation: m,
                at,
            });
            s += num(text(piece, "length", "a piece")?)? + gap;
        }
        Ok(out)
    }
}
