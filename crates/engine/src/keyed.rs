//! Keyed archives: an object graph kept in a binary property list (`$objects`, `$top`). Reading
//! follows references and unwraps the common containers; writing builds the object list.

use crate::plist::{self, Value};

/// A keyed archive read from a property list.
pub struct Archive {
    objects: Vec<Value>,
    root: u64,
}

/// An object of an archive: its fields, and its class's name.
#[derive(Clone, Copy)]
pub struct Object<'a> {
    archive: &'a Archive,
    fields: &'a Value,
}

impl Archive {
    /// The archive in `bytes`, if it is one.
    pub fn read(bytes: &[u8]) -> Option<Archive> {
        let top = plist::read(bytes)?;
        let objects = match top.get("$objects")? {
            Value::Array(a) => a.clone(),
            _ => return None,
        };
        let root = top.get("$top")?.get("root")?.as_uid()?;
        Some(Archive { objects, root })
    }

    /// The archive's root object.
    pub fn root(&self) -> Option<Object<'_>> {
        self.object(self.root)
    }

    fn object(&self, uid: u64) -> Option<Object<'_>> {
        let fields = self.objects.get(usize::try_from(uid).ok()?)?;
        match fields {
            Value::Dict(_) => Some(Object { archive: self, fields }),
            _ => None,
        }
    }

    /// The value at `uid`, references followed (`$null` reads as none).
    fn value(&self, uid: u64) -> Option<&Value> {
        match self.objects.get(usize::try_from(uid).ok()?)? {
            Value::String(s) if s == "$null" => None,
            v => Some(v),
        }
    }
}

impl<'a> Object<'a> {
    /// The class the object was archived as.
    pub fn class(&self) -> Option<&'a str> {
        let uid = self.fields.get("$class")?.as_uid()?;
        self.archive.object(uid)?.fields.get("$classname")?.as_str()
    }

    /// The raw value of `key`, a reference followed once.
    pub fn raw(&self, key: &str) -> Option<&'a Value> {
        match self.fields.get(key)? {
            Value::Uid(u) => self.archive.value(*u),
            v => Some(v),
        }
    }

    /// The object `key` refers to.
    pub fn object(&self, key: &str) -> Option<Object<'a>> {
        self.archive.object(self.fields.get(key)?.as_uid()?)
    }

    pub fn int(&self, key: &str) -> Option<i64> {
        self.raw(key)?.as_int()
    }

    pub fn real(&self, key: &str) -> Option<f64> {
        self.raw(key)?.as_real()
    }

    pub fn bool(&self, key: &str) -> Option<bool> {
        self.raw(key)?.as_bool()
    }

    pub fn data(&self, key: &str) -> Option<&'a [u8]> {
        match self.raw(key)? {
            Value::Data(d) => Some(d),
            // data kept as an object (NSMutableData)
            Value::Dict(_) => self.object(key)?.fields.get("NS.data")?.as_data(),
            _ => None,
        }
    }

    /// A string: inline, or an `NSString` object.
    pub fn string(&self, key: &str) -> Option<String> {
        match self.raw(key)? {
            Value::String(s) => Some(s.clone()),
            Value::Dict(_) => self.object(key)?.fields.get("NS.string")?.as_str().map(str::to_owned),
            _ => None,
        }
    }

    /// The values of an array (`NS.objects`) that `key` refers to, references followed once:
    /// strings and numbers kept inline.
    pub fn values(&self, key: &str) -> Vec<&'a Value> {
        let Some(list) = self.object(key).and_then(|o| o.fields.get("NS.objects")?.as_array()) else {
            return Vec::new();
        };
        list.iter()
            .filter_map(|v| match v {
                Value::Uid(u) => self.archive.value(*u),
                v => Some(v),
            })
            .collect()
    }

    /// The objects of an array (`NS.objects`) that `key` refers to.
    pub fn array(&self, key: &str) -> Vec<Object<'a>> {
        let Some(list) = self.object(key).and_then(|o| o.fields.get("NS.objects")?.as_array()) else {
            return Vec::new();
        };
        list.iter().filter_map(|v| self.archive.object(v.as_uid()?)).collect()
    }
}

/// Builds a keyed archive: objects are added and referred to by the index [`Builder::add`] gives.
pub struct Builder {
    objects: Vec<Value>,
    classes: Vec<(String, u64)>,
}

impl Default for Builder {
    fn default() -> Builder {
        Builder::new()
    }
}

impl Builder {
    pub fn new() -> Builder {
        Builder { objects: vec![Value::String("$null".to_owned())], classes: Vec::new() }
    }

    /// The reference to nothing.
    pub fn null() -> Value {
        Value::Uid(0)
    }

    /// Adds a value (a string, data...) and gives its reference.
    pub fn add(&mut self, v: Value) -> Value {
        self.objects.push(v);
        Value::Uid(self.objects.len() as u64 - 1)
    }

    /// Reserves a place for an object filled in later with [`Builder::set`] (for objects that refer
    /// back to themselves through others).
    pub fn reserve(&mut self) -> Value {
        self.add(Value::Null)
    }

    /// Fills the place `at` reserved.
    pub fn set(&mut self, at: &Value, v: Value) {
        if let Some(slot) = at.as_uid().and_then(|u| self.objects.get_mut(usize::try_from(u).ok()?)) {
            *slot = v;
        }
    }

    /// The reference to a class record `name`, whose class chain is `chain` (itself first).
    pub fn class(&mut self, chain: &[&str]) -> Value {
        let name = chain.first().copied().unwrap_or("NSObject");
        if let Some((_, uid)) = self.classes.iter().find(|(n, _)| n == name) {
            return Value::Uid(*uid);
        }
        let record = Value::Dict(vec![
            ("$classes".to_owned(), Value::Array(chain.iter().map(|c| Value::String((*c).to_owned())).collect())),
            ("$classname".to_owned(), Value::String(name.to_owned())),
        ]);
        let r = self.add(record);
        if let Some(uid) = r.as_uid() {
            self.classes.push((name.to_owned(), uid));
        }
        r
    }

    /// An object of class `chain` with `fields`.
    pub fn object(&mut self, chain: &[&str], fields: Vec<(&str, Value)>) -> Value {
        let at = self.reserve();
        self.fill(&at, chain, fields);
        at
    }

    /// Fills a reserved place with an object of class `chain`.
    pub fn fill(&mut self, at: &Value, chain: &[&str], fields: Vec<(&str, Value)>) {
        let class = self.class(chain);
        let mut entries: Vec<(String, Value)> = fields.into_iter().map(|(k, v)| (k.to_owned(), v)).collect();
        entries.push(("$class".to_owned(), class));
        self.set(at, Value::Dict(entries));
    }

    /// A string object's reference.
    pub fn string(&mut self, s: &str) -> Value {
        self.add(Value::String(s.to_owned()))
    }

    /// An array (`NSArray`, or `NSMutableArray` if `mutable`) of references.
    pub fn array(&mut self, items: Vec<Value>, mutable: bool) -> Value {
        let chain: &[&str] =
            if mutable { &["NSMutableArray", "NSArray", "NSObject"] } else { &["NSArray", "NSObject"] };
        self.object(chain, vec![("NS.objects", Value::Array(items))])
    }

    /// The archive, rooted at `root`.
    pub fn finish(self, root: Value) -> Vec<u8> {
        plist::write(&Value::Dict(vec![
            ("$version".to_owned(), Value::Int(100_000)),
            ("$archiver".to_owned(), Value::String("NSKeyedArchiver".to_owned())),
            ("$top".to_owned(), Value::Dict(vec![("root".to_owned(), root)])),
            ("$objects".to_owned(), Value::Array(self.objects)),
        ]))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_object_graph_round_trips() {
        let mut b = Builder::new();
        let doc = b.reserve();
        let name = b.string("Layer 1");
        let layer = b.object(
            &["SilicaLayer", "ValkyrieLayer", "NSObject"],
            vec![
                ("name", name),
                ("opacity", Value::Real(0.5)),
                ("hidden", Value::Bool(true)),
                ("document", doc.clone()),
            ],
        );
        let layers = b.array(vec![layer], true);
        let title = b.string("Untitled");
        b.fill(
            &doc,
            &["SilicaDocument", "ValkyrieDocument", "NSObject"],
            vec![("layers", layers), ("name", title), ("tileSize", Value::Int(256)), ("mask", Builder::null())],
        );
        let bytes = b.finish(doc);

        let archive = Archive::read(&bytes).unwrap();
        let root = archive.root().unwrap();
        assert_eq!(root.class(), Some("SilicaDocument"));
        assert_eq!(root.string("name").as_deref(), Some("Untitled"));
        assert_eq!(root.int("tileSize"), Some(256));
        assert!(root.object("mask").is_none());
        let layers = root.array("layers");
        assert_eq!(layers.len(), 1);
        let l = layers.first().unwrap();
        assert_eq!(l.class(), Some("SilicaLayer"));
        assert_eq!(l.string("name").as_deref(), Some("Layer 1"));
        assert_eq!(l.real("opacity"), Some(0.5));
        assert_eq!(l.bool("hidden"), Some(true));
        assert_eq!(l.object("document").and_then(|d| d.class()), Some("SilicaDocument"));
    }
}
