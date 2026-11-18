//! The expression language of primlib's sizes (primlib's docs/EXPRESSIONS.md),
//! as its reference evaluator `primlib/expr.py` has it: the same tokens,
//! precedence, functions (trigonometry in degrees) and errors. Tested against
//! it on every expression of every primitive (tests/primlib.rs).
//!
//! ```
//! use view4d_prim::expr::{evaluate, Env};
//! let mut env = Env::new();
//! env.set("a", 3.0);
//! assert_eq!(evaluate("max(a; 5) / 2", &env).unwrap(), 2.5);
//! ```

use std::collections::HashMap;
use std::fmt;

/// An expression that doesn't parse or can't be evaluated.
#[derive(Debug, Clone, PartialEq)]
pub struct ExprError(pub String);

impl fmt::Display for ExprError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl std::error::Error for ExprError {}

fn err<T>(msg: impl Into<String>) -> Result<T, ExprError> {
    Err(ExprError(msg.into()))
}

/// A value: a number, or text (a font path, a line of text, a key).
#[derive(Debug, Clone, PartialEq)]
pub enum Value {
    Number(f64),
    Text(String),
}

impl Value {
    pub fn number(&self) -> Option<f64> {
        match self {
            Value::Number(n) => Some(*n),
            Value::Text(_) => None,
        }
    }
}

/// Names and their values.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Env(HashMap<String, Value>);

impl Env {
    pub fn new() -> Env {
        Env::default()
    }

    pub fn set(&mut self, name: &str, v: f64) {
        self.0.insert(name.to_string(), Value::Number(v));
    }

    pub fn set_value(&mut self, name: &str, v: Value) {
        self.0.insert(name.to_string(), v);
    }

    pub fn get(&self, name: &str) -> Option<&Value> {
        self.0.get(name)
    }

    pub fn len(&self) -> usize {
        self.0.len()
    }

    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }

    pub fn iter(&self) -> impl Iterator<Item = (&str, &Value)> {
        self.0.iter().map(|(k, v)| (k.as_str(), v))
    }
}

// -- tokens -------------------------------------------------------------------

#[derive(Debug, Clone, PartialEq)]
enum Tok {
    Num(f64),
    Text(String),
    Ref(usize),
    Name(String),
    Op(&'static str),
}

const OPS: [&str; 19] =
    [">=", "<=", "==", "!=", "-", "+", "*", "/", "(", ")", "?", ":", ";", ",", "<", ">", "[", "]", "@"];

fn is_word(b: u8) -> bool {
    b.is_ascii_alphanumeric() || b == b'_'
}

fn tokens(text: &str) -> Result<Vec<Tok>, ExprError> {
    let s = text.as_bytes();
    let mut out = Vec::new();
    let mut pos = 0;
    let skip_ws = |p: &mut usize| {
        while *p < s.len() && s[*p].is_ascii_whitespace() {
            *p += 1;
        }
    };
    loop {
        skip_ws(&mut pos);
        if pos >= s.len() {
            return Ok(out);
        }
        let start = pos;
        let c = s[pos];
        if c.is_ascii_digit() || (c == b'.' && s.get(pos + 1).is_some_and(u8::is_ascii_digit)) {
            // (\d+\.?\d*|\.\d+)([eE][-+]?\d+)?
            while pos < s.len() && s[pos].is_ascii_digit() {
                pos += 1;
            }
            if pos < s.len() && s[pos] == b'.' {
                pos += 1;
                while pos < s.len() && s[pos].is_ascii_digit() {
                    pos += 1;
                }
            }
            if pos < s.len() && matches!(s[pos], b'e' | b'E') {
                let mut p = pos + 1;
                if p < s.len() && matches!(s[p], b'+' | b'-') {
                    p += 1;
                }
                if p < s.len() && s[p].is_ascii_digit() {
                    while p < s.len() && s[p].is_ascii_digit() {
                        p += 1;
                    }
                    pos = p;
                }
            }
            let n: f64 = text[start..pos].parse().map_err(|_| ExprError(format!("bad number in {text:?}")))?;
            // An optional unit, `deg` or `mm`, each × 1: (?:\s*(deg|mm)\b)?
            let mut p = pos;
            skip_ws(&mut p);
            for unit in ["deg", "mm"] {
                let end = p + unit.len();
                if s[p..].starts_with(unit.as_bytes()) && !s.get(end).is_some_and(|&b| is_word(b)) {
                    pos = end;
                    break;
                }
            }
            out.push(Tok::Num(n));
        } else if s[pos..].starts_with(b"<<") {
            // <<text>>: up to the first >>
            let Some(end) = text[pos + 2..].find(">>") else {
                return err(format!("unexpected {:?} in {text:?}", text[start..].chars().take(10).collect::<String>()));
            };
            out.push(Tok::Text(text[pos + 2..pos + 2 + end].to_string()));
            pos += 2 + end + 2;
        } else if c == b'@' && s.get(pos + 1).is_some_and(u8::is_ascii_digit) {
            pos += 1;
            while pos < s.len() && s[pos].is_ascii_digit() {
                pos += 1;
            }
            let id = text[start + 1..pos].parse().map_err(|_| ExprError(format!("bad reference in {text:?}")))?;
            out.push(Tok::Ref(id));
        } else if c.is_ascii_alphabetic() || c == b'_' {
            while pos < s.len() && is_word(s[pos]) {
                pos += 1;
            }
            out.push(Tok::Name(text[start..pos].to_string()));
        } else if let Some(op) = OPS.iter().find(|op| *op != &"@" && s[pos..].starts_with(op.as_bytes())) {
            pos += op.len();
            out.push(Tok::Op(op));
        } else {
            let rest: String = text[start..].trim().chars().take(10).collect();
            return err(format!("unexpected {rest:?} in {text:?}"));
        }
    }
}

// -- parser ---------------------------------------------------------------------

/// A parsed expression.
#[derive(Debug, Clone, PartialEq)]
pub enum Expr {
    Num(f64),
    /// `<<text>>`
    Text(String),
    Name(String),
    /// `@id`: another operation, only as the argument of `xmin()` and the like.
    Ref(usize),
    Neg(Box<Expr>),
    /// `+ - * /`
    Bin(char, Box<Expr>, Box<Expr>),
    /// `< > <= >= == !=`
    Cmp(&'static str, Box<Expr>, Box<Expr>),
    If(Box<Expr>, Box<Expr>, Box<Expr>),
    Call(Func, Vec<Expr>),
    /// `name[k]`: one letter (only a name is indexed, as in FreeCAD).
    Index(String, Box<Expr>),
    /// `name[a:b]`
    Slice(String, Box<Expr>, Box<Expr>),
}

/// The functions, and the bounds of another operation.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Func {
    Sin,
    Cos,
    Tan,
    Asin,
    Acos,
    Atan,
    Atan2,
    Sqrt,
    Abs,
    Floor,
    Ceil,
    Min,
    Max,
    /// C's fmod, as FreeCAD's `mod`.
    Mod,
    /// Text functions FreeCAD hasn't (primlib writes them out for it).
    Len,
    Count,
    Repeat,
    /// `xmin(@id)` … `zmax(@id)`: axis 0–2, and whether the max.
    Extent(usize, bool),
}

impl Func {
    fn by_name(name: &str) -> Option<Func> {
        use Func::*;
        Some(match name {
            "sin" => Sin,
            "cos" => Cos,
            "tan" => Tan,
            "asin" => Asin,
            "acos" => Acos,
            "atan" => Atan,
            "atan2" => Atan2,
            "sqrt" => Sqrt,
            "abs" => Abs,
            "floor" => Floor,
            "ceil" => Ceil,
            "min" => Min,
            "max" => Max,
            "mod" => Mod,
            "len" => Len,
            "count" => Count,
            "repeat" => Repeat,
            _ => {
                let b = name.as_bytes();
                let axis = match b.first() {
                    Some(b'x') => 0,
                    Some(b'y') => 1,
                    Some(b'z') => 2,
                    _ => return None,
                };
                return match &name[1..] {
                    "min" => Some(Extent(axis, false)),
                    "max" => Some(Extent(axis, true)),
                    _ => None,
                };
            }
        })
    }

    fn name(self) -> String {
        use Func::*;
        match self {
            Sin => "sin",
            Cos => "cos",
            Tan => "tan",
            Asin => "asin",
            Acos => "acos",
            Atan => "atan",
            Atan2 => "atan2",
            Sqrt => "sqrt",
            Abs => "abs",
            Floor => "floor",
            Ceil => "ceil",
            Min => "min",
            Max => "max",
            Mod => "mod",
            Len => "len",
            Count => "count",
            Repeat => "repeat",
            Extent(a, max) => return format!("{}{}", ["x", "y", "z"][a], if max { "max" } else { "min" }),
        }
        .to_string()
    }

    /// How many arguments it takes; None for one or more.
    fn arity(self) -> Option<usize> {
        match self {
            Func::Min | Func::Max => None,
            Func::Atan2 | Func::Mod | Func::Count | Func::Repeat => Some(2),
            _ => Some(1),
        }
    }
}

struct Parser<'a> {
    text: &'a str,
    toks: Vec<Tok>,
    i: usize,
}

impl Parser<'_> {
    fn peek(&self) -> Option<&Tok> {
        self.toks.get(self.i)
    }

    fn peek_op(&self, op: &str) -> bool {
        matches!(self.peek(), Some(Tok::Op(o)) if *o == op)
    }

    fn take_op(&mut self, op: &str) -> Result<(), ExprError> {
        if !self.peek_op(op) {
            return err(format!("expected {op:?} in {:?}", self.text));
        }
        self.i += 1;
        Ok(())
    }

    fn conditional(&mut self) -> Result<Expr, ExprError> {
        let cond = self.comparison()?;
        if self.peek_op("?") {
            self.i += 1;
            let a = self.conditional()?;
            self.take_op(":")?;
            let b = self.conditional()?;
            return Ok(Expr::If(Box::new(cond), Box::new(a), Box::new(b)));
        }
        Ok(cond)
    }

    fn comparison(&mut self) -> Result<Expr, ExprError> {
        let a = self.additive()?;
        if let Some(Tok::Op(op @ ("<" | ">" | "<=" | ">=" | "==" | "!="))) = self.peek() {
            let op = *op;
            self.i += 1;
            let b = self.additive()?;
            return Ok(Expr::Cmp(op, Box::new(a), Box::new(b)));
        }
        Ok(a)
    }

    fn additive(&mut self) -> Result<Expr, ExprError> {
        let mut a = self.term()?;
        while let Some(Tok::Op(op @ ("+" | "-"))) = self.peek() {
            let op = op.as_bytes()[0] as char;
            self.i += 1;
            a = Expr::Bin(op, Box::new(a), Box::new(self.term()?));
        }
        Ok(a)
    }

    fn term(&mut self) -> Result<Expr, ExprError> {
        let mut a = self.unary()?;
        while let Some(Tok::Op(op @ ("*" | "/"))) = self.peek() {
            let op = op.as_bytes()[0] as char;
            self.i += 1;
            a = Expr::Bin(op, Box::new(a), Box::new(self.unary()?));
        }
        Ok(a)
    }

    fn unary(&mut self) -> Result<Expr, ExprError> {
        if self.peek_op("-") {
            self.i += 1;
            return Ok(Expr::Neg(Box::new(self.unary()?)));
        }
        if self.peek_op("+") {
            self.i += 1;
            return self.unary();
        }
        let start = self.i;
        let a = self.atom()?;
        if !self.peek_op("[") {
            return Ok(a);
        }
        let Expr::Name(name) = a else {
            return err(format!("only a name is indexed, as FreeCAD has it, in {:?}", self.text));
        };
        if self.i != start + 1 {
            return err(format!("only a name is indexed, as FreeCAD has it, in {:?}", self.text));
        }
        self.i += 1;
        let k = self.additive()?;
        let e = if self.peek_op(":") {
            self.i += 1;
            Expr::Slice(name, Box::new(k), Box::new(self.additive()?))
        } else {
            Expr::Index(name, Box::new(k))
        };
        self.take_op("]")?;
        Ok(e)
    }

    fn atom(&mut self) -> Result<Expr, ExprError> {
        let Some(tok) = self.toks.get(self.i).cloned() else {
            return err(format!("{:?} ends early", self.text));
        };
        self.i += 1;
        match tok {
            Tok::Num(n) => Ok(Expr::Num(n)),
            Tok::Text(t) => Ok(Expr::Text(t)),
            Tok::Ref(id) => Ok(Expr::Ref(id)),
            Tok::Op("(") => {
                let e = self.conditional()?;
                self.take_op(")")?;
                Ok(e)
            }
            Tok::Name(name) => {
                if !self.peek_op("(") {
                    return Ok(Expr::Name(name));
                }
                self.i += 1;
                let mut args = Vec::new();
                if !self.peek_op(")") {
                    args.push(self.conditional()?);
                    while self.peek_op(";") || self.peek_op(",") {
                        self.i += 1;
                        args.push(self.conditional()?);
                    }
                }
                self.take_op(")")?;
                match Func::by_name(&name) {
                    Some(f) => Ok(Expr::Call(f, args)),
                    None => err(format!("unknown function {name}() in {:?}", self.text)),
                }
            }
            Tok::Op(op) => err(format!("unexpected {op:?} in {:?}", self.text)),
        }
    }
}

/// The expression `text`, parsed.
pub fn parse(text: &str) -> Result<Expr, ExprError> {
    let mut p = Parser { text, toks: tokens(text)?, i: 0 };
    let e = p.conditional()?;
    if let Some(t) = p.peek() {
        let t = match t {
            Tok::Num(n) => format!("{n}"),
            Tok::Text(t) => format!("<<{t}>>"),
            Tok::Ref(id) => format!("@{id}"),
            Tok::Name(n) => n.clone(),
            Tok::Op(o) => o.to_string(),
        };
        return err(format!("unexpected {t:?} in {text:?}"));
    }
    Ok(e)
}

// -- evaluation -------------------------------------------------------------------

/// Gives `xmin(@id)` and the like: (operation id, axis 0–2, whether the max).
pub type Extents<'a> = &'a dyn Fn(usize, usize, bool) -> Result<f64, ExprError>;

impl Expr {
    /// The value, names from `env`, bounds from `extent` (none: an error).
    pub fn eval(&self, env: &Env, extent: Option<Extents>) -> Result<Value, ExprError> {
        let num = |e: &Expr, what: &str| e.eval(env, extent).and_then(|v| as_number(v, what));
        let text = |e: &Expr, what: &str| e.eval(env, extent).and_then(|v| as_text(v, what));
        let whole = |e: &Expr, what: &str| {
            let v = num(e, what)?;
            if v != v.floor() {
                return err(format!("{what}: {v} is not a whole number"));
            }
            Ok(v as i64)
        };
        let n = Value::Number;
        Ok(match self {
            Expr::Num(v) => n(*v),
            Expr::Text(t) => Value::Text(t.clone()),
            Expr::Name(name) => match env.get(name) {
                Some(v) => v.clone(),
                None if name == "pi" => n(std::f64::consts::PI),
                None => return err(format!("unknown name {name}")),
            },
            Expr::Ref(id) => return err(format!("@{id} is an operation, not a number")),
            Expr::Neg(a) => n(-num(a, "-")?),
            Expr::Bin(op, a, b) => {
                let (a, b) = (a.eval(env, extent)?, b.eval(env, extent)?);
                let what = op.to_string();
                if let ('+', Value::Text(a), Value::Text(b)) = (op, &a, &b) {
                    return Ok(Value::Text(format!("{a}{b}")));
                }
                let (a, b) = (as_number(a, &what)?, as_number(b, &what)?);
                n(match op {
                    '+' => a + b,
                    '-' => a - b,
                    '*' => a * b,
                    _ if b == 0.0 => return err("division by zero"),
                    _ => a / b,
                })
            }
            Expr::Cmp(op, a, b) => {
                let t = match (a.eval(env, extent)?, b.eval(env, extent)?) {
                    (Value::Number(a), Value::Number(b)) => match *op {
                        "<" => a < b,
                        ">" => a > b,
                        "<=" => a <= b,
                        ">=" => a >= b,
                        "==" => a == b,
                        _ => a != b,
                    },
                    (Value::Text(a), Value::Text(b)) if matches!(*op, "==" | "!=") => (a == b) == (*op == "=="),
                    _ => return err(format!("{op}: text is compared only with text, by == and !=")),
                };
                n(if t { 1.0 } else { 0.0 })
            }
            Expr::If(c, a, b) => {
                if num(c, "?")? != 0.0 {
                    a.eval(env, extent)?
                } else {
                    b.eval(env, extent)?
                }
            }
            Expr::Index(name, k) => {
                let s: Vec<char> = text(&Expr::Name(name.clone()), "[]")?.chars().collect();
                let k = whole(k, "[]")?;
                let len = s.len() as i64;
                if !(-len..len).contains(&k) {
                    return err(format!("[{k}]: past the end of {:?}", s.iter().collect::<String>()));
                }
                Value::Text(s[k.rem_euclid(len.max(1)) as usize].to_string())
            }
            Expr::Slice(name, a, b) => {
                let s: Vec<char> = text(&Expr::Name(name.clone()), "[:]")?.chars().collect();
                let len = s.len() as i64;
                // As Python: negative from the end, clipped.
                let at = |i: i64| (if i < 0 { i + len } else { i }).clamp(0, len) as usize;
                let (a, b) = (at(whole(a, "[:]")?), at(whole(b, "[:]")?));
                Value::Text(if a < b { s[a..b].iter().collect() } else { String::new() })
            }
            Expr::Call(Func::Extent(axis, max), args) => {
                let f = Func::Extent(*axis, *max).name();
                let [Expr::Ref(id)] = args.as_slice() else {
                    return err(format!("{f}() takes an operation, @id"));
                };
                let Some(extent) = extent else {
                    return err(format!("{f}(@{id}) needs the operation's shape"));
                };
                n(extent(*id, *axis, *max)?)
            }
            Expr::Call(f @ (Func::Len | Func::Count | Func::Repeat), args) => {
                let want = if *f == Func::Len { 1 } else { 2 };
                if args.len() != want {
                    let a = if want == 1 { "one argument" } else { "2 arguments" };
                    return err(format!("{}() takes {a}, not {}", f.name(), args.len()));
                }
                let what = format!("{}()", f.name());
                let s = text(&args[0], &what)?;
                match f {
                    Func::Len => n(s.chars().count() as f64),
                    Func::Count => {
                        let t = text(&args[1], &what)?;
                        n(if t.is_empty() { 0.0 } else { s.matches(t.as_str()).count() as f64 })
                    }
                    _ => {
                        let k = whole(&args[1], &what)?;
                        if k < 0 {
                            return err(format!("repeat(): {k} times"));
                        }
                        Value::Text(s.repeat(k as usize))
                    }
                }
            }
            Expr::Call(f, args) => {
                if let Some(k) = f.arity()
                    && args.len() != k
                {
                    let s = if k > 1 { "s" } else { "" };
                    return err(format!("{}() takes {k} argument{s}, not {}", f.name(), args.len()));
                }
                let what = format!("{}()", f.name());
                let v = args.iter().map(|a| num(a, &what)).collect::<Result<Vec<_>, _>>()?;
                let r = match f {
                    Func::Sin => v[0].to_radians().sin(),
                    Func::Cos => v[0].to_radians().cos(),
                    Func::Tan => v[0].to_radians().tan(),
                    Func::Asin => v[0].asin().to_degrees(),
                    Func::Acos => v[0].acos().to_degrees(),
                    Func::Atan => v[0].atan().to_degrees(),
                    Func::Atan2 => v[0].atan2(v[1]).to_degrees(),
                    Func::Sqrt => v[0].sqrt(),
                    Func::Abs => v[0].abs(),
                    Func::Floor => v[0].floor(),
                    Func::Ceil => v[0].ceil(),
                    Func::Mod => v[0] % v[1],
                    Func::Min | Func::Max if v.is_empty() => {
                        return err(format!("{}() takes one argument or more", f.name()));
                    }
                    Func::Min => v.iter().copied().fold(f64::INFINITY, f64::min),
                    Func::Max => v.iter().copied().fold(f64::NEG_INFINITY, f64::max),
                    Func::Extent(..) | Func::Len | Func::Count | Func::Repeat => unreachable!(),
                };
                if r.is_nan() && !v.iter().any(|x| x.is_nan()) {
                    let args: Vec<String> = v.iter().map(|x| format!("{x}")).collect();
                    return err(format!("{}({}): math domain error", f.name(), args.join(", ")));
                }
                n(r)
            }
        })
    }

    /// The value, which must be a number.
    pub fn number(&self, env: &Env, extent: Option<Extents>) -> Result<f64, ExprError> {
        let what = match self {
            Expr::Name(name) => name.clone(),
            _ => "the value".into(),
        };
        self.eval(env, extent).and_then(|v| match v {
            Value::Number(v) => Ok(v),
            Value::Text(_) => err(format!("{what} is text, not a number")),
        })
    }

    /// Whether it uses `name`.
    pub fn uses(&self, name: &str) -> bool {
        match self {
            Expr::Name(n) => n == name,
            Expr::Num(_) | Expr::Text(_) | Expr::Ref(_) => false,
            Expr::Index(n, k) => n == name || k.uses(name),
            Expr::Slice(n, a, b) => n == name || a.uses(name) || b.uses(name),
            Expr::Neg(a) => a.uses(name),
            Expr::Bin(_, a, b) | Expr::Cmp(_, a, b) => a.uses(name) || b.uses(name),
            Expr::If(c, a, b) => c.uses(name) || a.uses(name) || b.uses(name),
            Expr::Call(_, args) => args.iter().any(|a| a.uses(name)),
        }
    }

    /// The operations whose bounds it uses (`xmin(@id)`), each id once.
    pub fn refs(&self, out: &mut Vec<usize>) {
        match self {
            Expr::Ref(id) => {
                if !out.contains(id) {
                    out.push(*id);
                }
            }
            Expr::Num(_) | Expr::Text(_) | Expr::Name(_) => {}
            Expr::Index(_, k) => k.refs(out),
            Expr::Slice(_, a, b) => {
                a.refs(out);
                b.refs(out);
            }
            Expr::Neg(a) => a.refs(out),
            Expr::Bin(_, a, b) | Expr::Cmp(_, a, b) => {
                a.refs(out);
                b.refs(out);
            }
            Expr::If(c, a, b) => {
                c.refs(out);
                a.refs(out);
                b.refs(out);
            }
            Expr::Call(_, args) => args.iter().for_each(|a| a.refs(out)),
        }
    }
}

fn as_number(v: Value, what: &str) -> Result<f64, ExprError> {
    match v {
        Value::Number(v) => Ok(v),
        Value::Text(t) => err(format!("{what}: {t:?} is text, not a number")),
    }
}

fn as_text(v: Value, what: &str) -> Result<String, ExprError> {
    match v {
        Value::Text(t) => Ok(t),
        Value::Number(v) => err(format!("{what}: {v} is a number, not text")),
    }
}

/// The value of `text`, which must be a number, names from `env`.
pub fn evaluate(text: &str, env: &Env) -> Result<f64, ExprError> {
    parse(text)?.number(env, None)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ev(text: &str) -> f64 {
        evaluate(text, &Env::new()).unwrap_or_else(|e| panic!("{text}: {e}"))
    }

    fn close(a: f64, b: f64) -> bool {
        (a - b).abs() < 1e-12
    }

    // As primlib's tests/test_expr.py.
    #[test]
    fn precedence() {
        assert_eq!(ev("1 + 2 * 3"), 7.0);
        assert_eq!(ev("(1 + 2) * 3"), 9.0);
        assert_eq!(ev("8 / 4 / 2"), 1.0);
        assert_eq!(ev("10 - 4 - 3"), 3.0);
        assert_eq!(ev("-2 * -3"), 6.0);
        assert_eq!(ev("+(1 + 1) * 2"), 4.0);
    }

    #[test]
    fn comparisons_are_one_or_zero() {
        let mut env = Env::new();
        env.set("a", 1.0);
        assert_eq!(ev("2 > 1"), 1.0);
        assert_eq!(ev("2 >= 3"), 0.0);
        assert_eq!(ev("1 + 1 == 2"), 1.0);
        assert_eq!(evaluate("a != a", &env).unwrap(), 0.0);
        assert!(evaluate("1 < 2 < 3", &env).is_err());
        assert_eq!(ev("(1 < 2) < 3"), 1.0);
    }

    #[test]
    fn conditional() {
        let mut env = Env::new();
        env.set("a", 1.0);
        assert_eq!(evaluate("a > 0 ? 1 : 2", &env).unwrap(), 1.0);
        env.set("a", 0.0);
        assert_eq!(evaluate("a > 0 ? 1 : 2", &env).unwrap(), 2.0);
        assert_eq!(ev("0 ? 1 : 0 ? 2 : 3"), 3.0);
        assert_eq!(ev("1 ? 0 ? 5 : 6 : 7"), 6.0);
        assert_eq!(ev("1 ? 2 : 1 / 0"), 2.0);
    }

    #[test]
    fn degrees() {
        assert!(close(ev("sin(30)"), 0.5));
        assert!(close(ev("cos(60 deg)"), 0.5));
        assert!(close(ev("tan(45deg)"), 1.0));
        assert!(close(ev("asin(0.5) / 1deg"), 30.0));
        assert!(close(ev("atan2(1; 1)"), 45.0));
        assert!(close(ev("180 deg / 4"), 45.0));
        assert_eq!(ev("pi"), std::f64::consts::PI);
    }

    #[test]
    fn functions() {
        assert_eq!(ev("max(1; 3; 2)"), 3.0);
        assert_eq!(ev("min(4, 2)"), 2.0);
        assert_eq!(ev("floor(-1.5) + ceil(1.2)"), 0.0);
        assert_eq!(ev("abs(-2) * sqrt(9)"), 6.0);
    }

    #[test]
    fn numbers() {
        assert_eq!(ev("1e-3 * 1000"), 1.0);
        assert_eq!(ev(".5 + 1."), 1.5);
        assert_eq!(ev("2 mm"), 2.0);
        // A unit only as a whole word: `2 mmx` is 2 then a name.
        assert!(evaluate("2 mmx", &Env::new()).is_err());
        // `1e` is 1 then the name e.
        assert!(evaluate("1e", &Env::new()).is_err());
    }

    #[test]
    fn extents() {
        let e = parse("xmax(@3) - xmin(@3)").unwrap();
        let extent = |id: usize, axis: usize, max: bool| {
            assert_eq!((id, axis), (3, 0));
            Ok(if max { 4.0 } else { -1.0 })
        };
        assert_eq!(e.number(&Env::new(), Some(&extent)).unwrap(), 5.0);
        let mut refs = vec![];
        e.refs(&mut refs);
        assert_eq!(refs, [3]);
        assert!(evaluate("xmin(@3)", &Env::new()).is_err());
        assert!(evaluate("xmin(3)", &Env::new()).is_err());
    }

    #[test]
    fn text_names() {
        let mut env = Env::new();
        env.set_value("font", Value::Text("/f.ttf".into()));
        assert_eq!(parse("font").unwrap().eval(&env, None).unwrap(), Value::Text("/f.ttf".into()));
        assert!(evaluate("font + 1", &env).is_err());
    }

    #[test]
    fn uses() {
        let e = parse("i * pitch + (a ? sin(i) : 0)").unwrap();
        assert!(e.uses("i") && e.uses("pitch") && e.uses("a") && !e.uses("b"));
    }
}
