//! A small XML reader: elements and their attributes, which is all 3MF
//! needs. Text, comments, processing instructions, CDATA and the doctype
//! are skipped. Names are local: a prefix (`m:color`) is dropped, so
//! namespaces are not told apart. Entities in attribute values are decoded.

use std::borrow::Cow;

use crate::Error;

/// An element's start: its local name and attributes.
#[derive(Debug, Clone, PartialEq)]
pub struct Start<'a> {
    pub name: &'a str,
    attrs: Vec<(&'a str, Cow<'a, str>)>,
    /// `<name ... />`: no end follows.
    pub empty: bool,
}

impl Start<'_> {
    /// The attribute of local name `name`.
    pub fn attr(&self, name: &str) -> Option<&str> {
        self.attrs.iter().find(|(n, _)| *n == name).map(|(_, v)| v.as_ref())
    }
}

#[derive(Debug, Clone, PartialEq)]
pub enum Event<'a> {
    Start(Start<'a>),
    /// An element's end, by local name; not given for an empty element.
    End(&'a str),
}

/// The events of a document, in order.
#[derive(Debug)]
pub struct Reader<'a> {
    text: &'a str,
    pos: usize,
}

impl<'a> Reader<'a> {
    pub fn new(text: &'a str) -> Reader<'a> {
        Reader { text, pos: 0 }
    }

    fn error(&self, what: &str) -> Error {
        let line = self.text[..self.pos.min(self.text.len())].matches('\n').count() + 1;
        Error(format!("XML line {line}: {what}"))
    }

    /// Past the next `end`, or an error naming what was cut short.
    fn skip_past(&mut self, end: &str, what: &str) -> Result<(), Error> {
        match self.text[self.pos..].find(end) {
            Some(i) => {
                self.pos += i + end.len();
                Ok(())
            }
            None => Err(self.error(&format!("{what} never ends"))),
        }
    }

    fn next_event(&mut self) -> Result<Option<Event<'a>>, Error> {
        loop {
            let Some(i) = self.text[self.pos..].find('<') else { return Ok(None) };
            self.pos += i;
            let rest = &self.text[self.pos..];
            if rest.starts_with("<?") {
                self.skip_past("?>", "a processing instruction")?;
            } else if rest.starts_with("<!--") {
                self.skip_past("-->", "a comment")?;
            } else if rest.starts_with("<![CDATA[") {
                self.skip_past("]]>", "CDATA")?;
            } else if rest.starts_with("<!") {
                self.skip_past(">", "a declaration")?;
            } else if let Some(rest) = rest.strip_prefix("</") {
                let end = rest.find('>').ok_or_else(|| self.error("an end tag never ends"))?;
                let name = local(rest[..end].trim());
                self.pos += 2 + end + 1;
                return Ok(Some(Event::End(name)));
            } else {
                return self.start().map(Some);
            }
        }
    }

    /// The element starting at `<`.
    fn start(&mut self) -> Result<Event<'a>, Error> {
        let text = self.text;
        let mut p = self.pos + 1;
        let name_end = text[p..].find(|c: char| c.is_ascii_whitespace() || c == '/' || c == '>').map(|i| p + i);
        let Some(name_end) = name_end.filter(|&e| e > p) else { return Err(self.error("a tag without a name")) };
        let name = local(&text[p..name_end]);
        p = name_end;
        let mut attrs = Vec::new();
        loop {
            p += text[p..].len() - text[p..].trim_start().len();
            let rest = &text[p..];
            if rest.starts_with("/>") || rest.starts_with('>') {
                let empty = rest.starts_with('/');
                self.pos = p + if empty { 2 } else { 1 };
                return Ok(Event::Start(Start { name, attrs, empty }));
            }
            let eq = rest.find('=').ok_or_else(|| self.error(&format!("`<{name}` never ends")))?;
            let attr = local(rest[..eq].trim());
            let value = rest[eq + 1..].trim_start();
            let quote = value.chars().next().filter(|&q| q == '"' || q == '\'');
            let Some(quote) = quote else { return Err(self.error(&format!("`{attr}` without a quoted value"))) };
            let close = value[1..].find(quote).ok_or_else(|| self.error(&format!("`{attr}`'s value never ends")))?;
            attrs.push((attr, unescape(&value[1..1 + close])));
            p = text.len() - value.len() + 1 + close + 1;
        }
    }
}

impl<'a> Iterator for Reader<'a> {
    type Item = Result<Event<'a>, Error>;

    fn next(&mut self) -> Option<Self::Item> {
        let event = self.next_event();
        if event.is_err() {
            self.pos = self.text.len(); // no more after an error
        }
        event.transpose()
    }
}

/// `name` without its prefix.
fn local(name: &str) -> &str {
    name.rsplit(':').next().unwrap_or(name)
}

/// `value` with its entities and character references decoded; one not
/// known is left as it is.
fn unescape(value: &str) -> Cow<'_, str> {
    if !value.contains('&') {
        return Cow::Borrowed(value);
    }
    let mut out = String::with_capacity(value.len());
    let mut rest = value;
    while let Some(i) = rest.find('&') {
        out += &rest[..i];
        rest = &rest[i..];
        let decoded = rest.find(';').and_then(|end| {
            let c = match &rest[1..end] {
                "lt" => '<',
                "gt" => '>',
                "amp" => '&',
                "quot" => '"',
                "apos" => '\'',
                r => {
                    let n = match r.strip_prefix("#x").or_else(|| r.strip_prefix("#X")) {
                        Some(hex) => u32::from_str_radix(hex, 16).ok(),
                        None => r.strip_prefix('#').and_then(|d| d.parse().ok()),
                    };
                    char::from_u32(n?)?
                }
            };
            Some((c, end))
        });
        match decoded {
            Some((c, end)) => {
                out.push(c);
                rest = &rest[end + 1..];
            }
            None => {
                out.push('&');
                rest = &rest[1..];
            }
        }
    }
    out += rest;
    Cow::Owned(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn events(text: &str) -> Result<Vec<String>, Error> {
        Reader::new(text)
            .map(|e| {
                e.map(|e| match e {
                    Event::Start(s) => {
                        let attrs: Vec<String> = s.attrs.iter().map(|(n, v)| format!(" {n}={v}")).collect();
                        format!("<{}{}{}>", s.name, attrs.concat(), if s.empty { "/" } else { "" })
                    }
                    Event::End(n) => format!("</{n}>"),
                })
            })
            .collect()
    }

    #[test]
    fn elements_and_attributes() {
        let doc = "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<!DOCTYPE x>\n<!-- a <comment> -->\n\
                   <model unit = 'millimeter' xml:lang=\"en-US\">text<![CDATA[<not/>]]>\n\
                   <m:color color=\"#FF0000\"/><vertex\n x=\"1.5\" y='a &amp; b &lt;&#65;&#x42;&bogus;'/></model>";
        assert_eq!(
            events(doc).unwrap(),
            [
                "<model unit=millimeter lang=en-US>",
                "<color color=#FF0000/>",
                "<vertex x=1.5 y=a & b <AB&bogus;/>",
                "</model>"
            ]
        );
        let mut r = Reader::new("<a b='1' c=\"2\">");
        let Some(Ok(Event::Start(s))) = r.next() else { panic!() };
        assert_eq!((s.attr("b"), s.attr("c"), s.attr("d")), (Some("1"), Some("2"), None));
    }

    #[test]
    fn errors() {
        let e = |text: &str| events(text).unwrap_err().0;
        assert_eq!(e("<a>\n<!-- never"), "XML line 2: a comment never ends");
        assert_eq!(e("<a b=1/>"), "XML line 1: `b` without a quoted value");
        assert_eq!(e("<a b='1/>"), "XML line 1: `b`'s value never ends");
        assert_eq!(e("<a b='1'"), "XML line 1: `<a` never ends");
        assert_eq!(e("< a/>"), "XML line 1: a tag without a name");
        assert_eq!(e("<a></a"), "XML line 1: an end tag never ends");
        assert_eq!(events("just text").unwrap(), Vec::<String>::new());
    }
}
