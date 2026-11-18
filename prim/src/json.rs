//! A small JSON reader (RFC 8259): what a `.prim` file is written in.
//! Objects keep their keys in the order the file gives them; numbers are
//! f64.

use crate::Error;

#[derive(Debug, Clone, PartialEq)]
pub enum Value {
    Null,
    Bool(bool),
    Number(f64),
    String(String),
    Array(Vec<Value>),
    Object(Vec<(String, Value)>),
}

impl Value {
    /// The value at `key`, if this is an object that has it.
    pub fn get(&self, key: &str) -> Option<&Value> {
        match self {
            Value::Object(fields) => fields.iter().find(|(k, _)| k == key).map(|(_, v)| v),
            _ => None,
        }
    }

    pub fn as_str(&self) -> Option<&str> {
        match self {
            Value::String(s) => Some(s),
            _ => None,
        }
    }

    pub fn as_f64(&self) -> Option<f64> {
        match self {
            Value::Number(n) => Some(*n),
            _ => None,
        }
    }

    /// A number that is a whole number, not negative.
    pub fn as_index(&self) -> Option<usize> {
        match self {
            Value::Number(n) if *n >= 0.0 && n.fract() == 0.0 && *n < u32::MAX as f64 => Some(*n as usize),
            _ => None,
        }
    }

    pub fn as_array(&self) -> Option<&[Value]> {
        match self {
            Value::Array(a) => Some(a),
            _ => None,
        }
    }

    pub fn as_object(&self) -> Option<&[(String, Value)]> {
        match self {
            Value::Object(o) => Some(o),
            _ => None,
        }
    }

    pub fn is_null(&self) -> bool {
        matches!(self, Value::Null)
    }
}

/// The value `text` holds, or where and why it isn't JSON.
pub fn parse(text: &str) -> Result<Value, Error> {
    let mut p = Parser { s: text.as_bytes(), pos: 0, text };
    p.ws();
    let v = p.value(0)?;
    p.ws();
    if p.pos != p.s.len() {
        return Err(p.error("more after the value"));
    }
    Ok(v)
}

/// Arrays and objects nested deeper than this are refused, not followed
/// until the stack runs out.
const MAX_DEPTH: usize = 256;

struct Parser<'a> {
    s: &'a [u8],
    pos: usize,
    text: &'a str,
}

impl Parser<'_> {
    fn error(&self, what: &str) -> Error {
        let before = &self.text.as_bytes()[..self.pos.min(self.s.len())];
        let line = before.iter().filter(|&&b| b == b'\n').count() + 1;
        let col = before.iter().rev().take_while(|&&b| b != b'\n').count() + 1;
        Error(format!("JSON line {line} column {col}: {what}"))
    }

    fn peek(&self) -> Option<u8> {
        self.s.get(self.pos).copied()
    }

    fn ws(&mut self) {
        while matches!(self.peek(), Some(b' ' | b'\t' | b'\n' | b'\r')) {
            self.pos += 1;
        }
    }

    fn expect(&mut self, b: u8) -> Result<(), Error> {
        if self.peek() == Some(b) {
            self.pos += 1;
            Ok(())
        } else {
            Err(self.error(&format!("expected '{}'", b as char)))
        }
    }

    fn word(&mut self, w: &str, v: Value) -> Result<Value, Error> {
        if self.s[self.pos..].starts_with(w.as_bytes()) {
            self.pos += w.len();
            Ok(v)
        } else {
            Err(self.error("not a value"))
        }
    }

    fn value(&mut self, depth: usize) -> Result<Value, Error> {
        if depth > MAX_DEPTH {
            return Err(self.error("nested too deeply"));
        }
        match self.peek() {
            None => Err(self.error("ends early")),
            Some(b'{') => self.object(depth),
            Some(b'[') => self.array(depth),
            Some(b'"') => Ok(Value::String(self.string()?)),
            Some(b't') => self.word("true", Value::Bool(true)),
            Some(b'f') => self.word("false", Value::Bool(false)),
            Some(b'n') => self.word("null", Value::Null),
            Some(b'-' | b'0'..=b'9') => self.number(),
            Some(_) => Err(self.error("not a value")),
        }
    }

    fn array(&mut self, depth: usize) -> Result<Value, Error> {
        self.pos += 1;
        let mut out = Vec::new();
        self.ws();
        if self.peek() == Some(b']') {
            self.pos += 1;
            return Ok(Value::Array(out));
        }
        loop {
            self.ws();
            out.push(self.value(depth + 1)?);
            self.ws();
            match self.peek() {
                Some(b',') => self.pos += 1,
                Some(b']') => {
                    self.pos += 1;
                    return Ok(Value::Array(out));
                }
                _ => return Err(self.error("expected ',' or ']'")),
            }
        }
    }

    fn object(&mut self, depth: usize) -> Result<Value, Error> {
        self.pos += 1;
        let mut out = Vec::new();
        self.ws();
        if self.peek() == Some(b'}') {
            self.pos += 1;
            return Ok(Value::Object(out));
        }
        loop {
            self.ws();
            if self.peek() != Some(b'"') {
                return Err(self.error("expected a key"));
            }
            let k = self.string()?;
            self.ws();
            self.expect(b':')?;
            self.ws();
            let v = self.value(depth + 1)?;
            out.push((k, v));
            self.ws();
            match self.peek() {
                Some(b',') => self.pos += 1,
                Some(b'}') => {
                    self.pos += 1;
                    return Ok(Value::Object(out));
                }
                _ => return Err(self.error("expected ',' or '}'")),
            }
        }
    }

    fn hex4(&mut self) -> Result<u32, Error> {
        let h = self.s.get(self.pos..self.pos + 4).ok_or_else(|| self.error("\\u ends early"))?;
        let h = std::str::from_utf8(h).ok().and_then(|h| u32::from_str_radix(h, 16).ok());
        let h = h.ok_or_else(|| self.error("bad \\u escape"))?;
        self.pos += 4;
        Ok(h)
    }

    fn string(&mut self) -> Result<String, Error> {
        self.pos += 1; // the opening quote
        let mut out = String::new();
        loop {
            let start = self.pos;
            while !matches!(self.peek(), None | Some(b'"' | b'\\' | 0..=0x1f)) {
                self.pos += 1;
            }
            // The text is valid UTF-8 and runs stop only at ASCII bytes.
            out.push_str(&self.text[start..self.pos]);
            match self.peek() {
                None => return Err(self.error("string never ends")),
                Some(b'"') => {
                    self.pos += 1;
                    return Ok(out);
                }
                Some(b'\\') => {
                    self.pos += 1;
                    let e = self.peek().ok_or_else(|| self.error("string never ends"))?;
                    self.pos += 1;
                    match e {
                        b'"' => out.push('"'),
                        b'\\' => out.push('\\'),
                        b'/' => out.push('/'),
                        b'b' => out.push('\u{8}'),
                        b'f' => out.push('\u{c}'),
                        b'n' => out.push('\n'),
                        b'r' => out.push('\r'),
                        b't' => out.push('\t'),
                        b'u' => {
                            let mut c = self.hex4()?;
                            if (0xd800..0xdc00).contains(&c) && self.s[self.pos..].starts_with(b"\\u") {
                                let at = self.pos;
                                self.pos += 2;
                                let lo = self.hex4()?;
                                if (0xdc00..0xe000).contains(&lo) {
                                    c = 0x10000 + ((c - 0xd800) << 10) + (lo - 0xdc00);
                                } else {
                                    self.pos = at;
                                }
                            }
                            out.push(char::from_u32(c).unwrap_or('\u{fffd}'));
                        }
                        _ => return Err(self.error("bad escape")),
                    }
                }
                Some(_) => return Err(self.error("control character in a string")),
            }
        }
    }

    fn number(&mut self) -> Result<Value, Error> {
        let start = self.pos;
        let digits = |p: &mut Self| {
            let s = p.pos;
            while matches!(p.peek(), Some(b'0'..=b'9')) {
                p.pos += 1;
            }
            p.pos > s
        };
        if self.peek() == Some(b'-') {
            self.pos += 1;
        }
        if self.peek() == Some(b'0') {
            self.pos += 1;
        } else if !digits(self) {
            return Err(self.error("bad number"));
        }
        if self.peek() == Some(b'.') {
            self.pos += 1;
            if !digits(self) {
                return Err(self.error("bad number"));
            }
        }
        if matches!(self.peek(), Some(b'e' | b'E')) {
            self.pos += 1;
            if matches!(self.peek(), Some(b'+' | b'-')) {
                self.pos += 1;
            }
            if !digits(self) {
                return Err(self.error("bad number"));
            }
        }
        self.text[start..self.pos].parse().map(Value::Number).map_err(|_| self.error("bad number"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn obj(fields: &[(&str, Value)]) -> Value {
        Value::Object(fields.iter().map(|(k, v)| (k.to_string(), v.clone())).collect())
    }

    #[test]
    fn values() {
        assert_eq!(parse(" null ").unwrap(), Value::Null);
        assert_eq!(parse("[true, false]").unwrap(), Value::Array(vec![Value::Bool(true), Value::Bool(false)]));
        assert_eq!(parse("-0.5e2").unwrap(), Value::Number(-50.0));
        assert_eq!(parse("[]").unwrap(), Value::Array(vec![]));
        assert_eq!(
            parse(r#"{"b": 1, "a": {"c": []}}"#).unwrap(),
            obj(&[("b", Value::Number(1.0)), ("a", obj(&[("c", Value::Array(vec![]))]))])
        );
    }

    #[test]
    fn keys_keep_their_order() {
        let v = parse(r#"{"z": 1, "a": 2, "m": 3}"#).unwrap();
        let keys: Vec<_> = v.as_object().unwrap().iter().map(|(k, _)| k.as_str()).collect();
        assert_eq!(keys, ["z", "a", "m"]);
        assert_eq!(v.get("a"), Some(&Value::Number(2.0)));
    }

    #[test]
    fn strings() {
        assert_eq!(parse(r#""a\"b\\c\/\n\t""#).unwrap(), Value::String("a\"b\\c/\n\t".into()));
        assert_eq!(parse(r#""é é ±""#).unwrap(), Value::String("é é ±".into()));
        assert_eq!(parse(r#""😀""#).unwrap(), Value::String("😀".into()));
        assert_eq!(parse(r#""\ud83d""#).unwrap(), Value::String("\u{fffd}".into()));
    }

    #[test]
    fn errors() {
        for bad in ["", "[1,]", "{\"a\" 1}", "01", "1.", "-", "\"a", "[1 2]", "nul", "{1: 2}", "1 1", "\"\t\""] {
            assert!(parse(bad).is_err(), "{bad:?} parsed");
        }
        let e = parse("{\n  \"a\": [1,\n  x]}").unwrap_err();
        assert_eq!(e.0, "JSON line 3 column 3: not a value");
        let deep = "[".repeat(1000);
        assert!(parse(&deep).unwrap_err().0.contains("nested too deeply"));
    }
}
