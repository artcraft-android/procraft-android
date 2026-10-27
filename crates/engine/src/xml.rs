//! A small, forgiving XML reader for the settings other painting apps keep as XML: elements with
//! their attributes, text and children. What it can't make sense of ends the document there.

/// An element: its name (with any prefix), attributes, text (CDATA included) and children.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Element {
    pub name: String,
    pub attributes: Vec<(String, String)>,
    pub text: String,
    pub children: Vec<Element>,
}

/// How deep elements may nest.
const MOST_DEPTH: usize = 256;

impl Element {
    /// The attribute `name` (a prefixed one matches by the part after its prefix).
    pub fn attribute(&self, name: &str) -> Option<&str> {
        self.attributes.iter().find(|(k, _)| local(k) == name).map(|(_, v)| v.as_str())
    }

    /// The first child named `name` (by the part after any prefix).
    pub fn child(&self, name: &str) -> Option<&Element> {
        self.children.iter().find(|c| local(&c.name) == name)
    }

    /// The first element named `name` here or anywhere below, depth first.
    pub fn find(&self, name: &str) -> Option<&Element> {
        let mut stack = vec![self];
        while let Some(e) = stack.pop() {
            if local(&e.name) == name {
                return Some(e);
            }
            stack.extend(e.children.iter().rev());
        }
        None
    }

    /// Every element named `name` here and below, in document order.
    pub fn all(&self, name: &str) -> Vec<&Element> {
        let mut out = Vec::new();
        let mut stack = vec![self];
        while let Some(e) = stack.pop() {
            if local(&e.name) == name {
                out.push(e);
            }
            stack.extend(e.children.iter().rev());
        }
        out
    }
}

fn local(name: &str) -> &str {
    name.rsplit_once(':').map_or(name, |(_, l)| l)
}

/// The root element of `text`; none if there is none.
pub fn parse(text: &str) -> Option<Element> {
    let mut stack: Vec<Element> = Vec::new();
    let mut root = None;
    let mut rest = text;
    // closes the innermost open element into its parent (or as the root)
    let close = |stack: &mut Vec<Element>, root: &mut Option<Element>| {
        if let Some(done) = stack.pop() {
            match stack.last_mut() {
                Some(parent) => parent.children.push(done),
                None => *root = root.take().or(Some(done)),
            }
        }
    };
    while !rest.is_empty() && root.is_none() {
        let Some(lt) = rest.find('<') else {
            if let Some(top) = stack.last_mut() {
                top.text.push_str(&unescape(rest));
            }
            break;
        };
        if lt > 0
            && let Some(top) = stack.last_mut()
        {
            top.text.push_str(&unescape(rest.get(..lt)?));
        }
        rest = rest.get(lt..)?;
        if let Some(after) = rest.strip_prefix("<![CDATA[") {
            let (data, more) = after.split_once("]]>").unwrap_or((after, ""));
            if let Some(top) = stack.last_mut() {
                top.text.push_str(data);
            }
            rest = more;
        } else if let Some(after) = rest.strip_prefix("<!--") {
            rest = after.split_once("-->").map_or("", |(_, m)| m);
        } else if rest.starts_with("<?") || rest.starts_with("<!") {
            rest = rest.split_once('>').map_or("", |(_, m)| m);
        } else if let Some(after) = rest.strip_prefix("</") {
            rest = after.split_once('>').map_or("", |(_, m)| m);
            close(&mut stack, &mut root);
        } else {
            let (tag, more) = split_tag(rest.get(1..)?)?;
            rest = more;
            let closed = tag.trim_end().ends_with('/');
            let tag = tag.trim_end().trim_end_matches('/');
            let name_end = tag.find(|c: char| c.is_whitespace()).unwrap_or(tag.len());
            let element = Element {
                name: tag.get(..name_end)?.to_owned(),
                attributes: attributes(tag.get(name_end..)?),
                ..Element::default()
            };
            if stack.len() >= MOST_DEPTH {
                return None;
            }
            stack.push(element);
            if closed {
                close(&mut stack, &mut root);
            }
        }
    }
    // an unfinished document: what it has, closed
    while root.is_none() && !stack.is_empty() {
        close(&mut stack, &mut root);
    }
    root
}

/// A tag's inside up to its `>` (not one in a quoted value), and what follows.
fn split_tag(s: &str) -> Option<(&str, &str)> {
    let mut quote = None;
    for (i, c) in s.char_indices() {
        match (quote, c) {
            (None, '"' | '\'') => quote = Some(c),
            (Some(q), _) if c == q => quote = None,
            (None, '>') => return Some((s.get(..i)?, s.get(i + 1..)?)),
            _ => {}
        }
    }
    None
}

/// `name="value"` pairs (or with single quotes).
fn attributes(mut s: &str) -> Vec<(String, String)> {
    let mut out = Vec::new();
    loop {
        s = s.trim_start();
        let Some(eq) = s.find('=') else { break };
        let name = s.get(..eq).unwrap_or_default().trim().to_owned();
        let after = s.get(eq + 1..).unwrap_or_default().trim_start();
        let Some(q) = after.chars().next().filter(|c| *c == '"' || *c == '\'') else { break };
        let body = after.get(1..).unwrap_or_default();
        let Some(end) = body.find(q) else { break };
        out.push((name, unescape(body.get(..end).unwrap_or_default())));
        s = body.get(end + 1..).unwrap_or_default();
    }
    out
}

/// Text with its entities (named and numbered) put back.
pub fn unescape(s: &str) -> String {
    if !s.contains('&') {
        return s.to_owned();
    }
    let mut out = String::with_capacity(s.len());
    let mut rest = s;
    while let Some(amp) = rest.find('&') {
        out.push_str(rest.get(..amp).unwrap_or_default());
        let after = rest.get(amp + 1..).unwrap_or_default();
        let Some(semi) = after.find(';').filter(|&n| n <= 10) else {
            out.push('&');
            rest = after;
            continue;
        };
        let entity = after.get(..semi).unwrap_or_default();
        let ch = match entity {
            "lt" => Some('<'),
            "gt" => Some('>'),
            "amp" => Some('&'),
            "quot" => Some('"'),
            "apos" => Some('\''),
            _ => entity
                .strip_prefix("#x")
                .or_else(|| entity.strip_prefix("#X"))
                .and_then(|h| u32::from_str_radix(h, 16).ok())
                .or_else(|| entity.strip_prefix('#').and_then(|d| d.parse().ok()))
                .and_then(char::from_u32),
        };
        match ch {
            Some(c) => out.push(c),
            None => {
                out.push('&');
                out.push_str(entity);
                out.push(';');
            }
        }
        rest = after.get(semi + 1..).unwrap_or_default();
    }
    out.push_str(rest);
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn elements_attributes_text_and_cdata_read() {
        let doc = r#"<?xml version="1.0"?>
<!-- a preset -->
<Preset paintopid="paintbrush" name="Ink &amp; Wash">
  <param type="string" name="OpacityValue"><![CDATA[0.5]]></param>
  <param name='brush_definition'><![CDATA[<Brush type="auto_brush"><MaskGenerator ratio="1"/></Brush>]]></param>
  <empty/>
  <dc:title>Caf&#233; &lt;set&gt;</dc:title>
</Preset>"#;
        let root = parse(doc).unwrap();
        assert_eq!((root.name.as_str(), root.attribute("name")), ("Preset", Some("Ink & Wash")));
        let params = root.all("param");
        assert_eq!(params.len(), 2);
        assert_eq!(params[0].text, "0.5");
        let inner = parse(&params[1].text).unwrap();
        assert_eq!(inner.find("MaskGenerator").and_then(|m| m.attribute("ratio")), Some("1"));
        assert_eq!(root.child("title").map(|t| t.text.as_str()), Some("Café <set>"));
        assert!(root.child("empty").is_some());
    }

    #[test]
    fn broken_documents_give_what_they_have() {
        let root = parse("<a x=\"1\"><b>text").unwrap();
        assert_eq!(root.child("b").map(|b| b.text.as_str()), Some("text"));
        assert_eq!(parse("no markup"), None);
        assert_eq!(unescape("a &bogus; b & c"), "a &bogus; b & c");
        let deep = "<a>".repeat(1000);
        assert_eq!(parse(&deep), None);
        crate::damaged::survives("XML", br#"<a k="v"><b><![CDATA[x]]></b>&amp;<!-- c --></a>"#, |b| {
            let _ = parse(&String::from_utf8_lossy(b));
        });
    }
}
