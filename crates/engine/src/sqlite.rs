//! Reading SQLite 3 databases, as the file format is documented: the schema table on the first
//! page, each table's b-tree of pages (with payloads spilling onto overflow pages), and records.
//! Enough to list a table's rows; damage ends a table where it is.

/// A value in a row.
#[derive(Clone, Debug, PartialEq)]
pub enum Value {
    Null,
    Int(i64),
    Real(f64),
    Text(String),
    Blob(Vec<u8>),
}

impl Value {
    pub fn as_int(&self) -> Option<i64> {
        match self {
            Value::Int(v) => Some(*v),
            Value::Real(v) if v.is_finite() => Some(*v as i64),
            _ => None,
        }
    }

    pub fn as_real(&self) -> Option<f64> {
        match self {
            Value::Int(v) => Some(*v as f64),
            Value::Real(v) if v.is_finite() => Some(*v),
            _ => None,
        }
    }

    pub fn as_text(&self) -> Option<&str> {
        match self {
            Value::Text(t) => Some(t),
            _ => None,
        }
    }

    pub fn as_blob(&self) -> Option<&[u8]> {
        match self {
            Value::Blob(b) => Some(b),
            _ => None,
        }
    }
}

/// A table's columns (as its schema names them) and rows.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Table {
    pub columns: Vec<String>,
    pub rows: Vec<Vec<Value>>,
}

impl Table {
    /// The value of column `name` (any case) in `row`.
    pub fn get<'a>(&self, row: &'a [Value], name: &str) -> Option<&'a Value> {
        let at = self.columns.iter().position(|c| c.eq_ignore_ascii_case(name))?;
        row.get(at)
    }
}

/// An open database.
pub struct Database<'a> {
    bytes: &'a [u8],
    page_size: usize,
    usable: usize,
    utf16: Option<bool>,
}

/// The most rows read from a table, and the most bytes a payload may have.
const MOST_ROWS: usize = 100_000;
const MOST_PAYLOAD: usize = 64 << 20;

/// The database in `bytes`; none if it isn't one.
pub fn open(bytes: &[u8]) -> Option<Database<'_>> {
    if bytes.get(..16)? != b"SQLite format 3\0" {
        return None;
    }
    let size = u16::from_be_bytes([*bytes.get(16)?, *bytes.get(17)?]);
    let page_size = if size == 1 { 65536 } else { usize::from(size) };
    if page_size < 512 || !page_size.is_power_of_two() {
        return None;
    }
    let reserved = usize::from(*bytes.get(20)?);
    let usable = page_size.checked_sub(reserved).filter(|u| *u >= 480)?;
    // text: UTF-8, or UTF-16 little- or big-endian
    let utf16 = match u32::from_be_bytes(bytes.get(56..60)?.try_into().ok()?) {
        2 => Some(false),
        3 => Some(true),
        _ => None,
    };
    Some(Database { bytes, page_size, usable, utf16 })
}

impl Database<'_> {
    /// The table named `name` (any case); none if there is none.
    pub fn table(&self, name: &str) -> Option<Table> {
        let schema = self.rows(1);
        let entry = schema.iter().find(|(_, r)| {
            r.first().and_then(Value::as_text) == Some("table")
                && r.get(1).and_then(Value::as_text).is_some_and(|n| n.eq_ignore_ascii_case(name))
        })?;
        let root = u32::try_from(entry.1.get(3)?.as_int()?).ok()?;
        let sql = entry.1.get(4).and_then(Value::as_text).unwrap_or_default();
        let (columns, rowid) = columns(sql);
        let rows = self
            .rows(root)
            .into_iter()
            .map(|(id, mut row)| {
                // a column that is the row's id is kept as nothing in the record
                if let Some(slot) = rowid.and_then(|at| row.get_mut(at)).filter(|v| **v == Value::Null) {
                    *slot = Value::Int(id);
                }
                row.resize(columns.len().max(row.len()), Value::Null);
                row
            })
            .collect();
        Some(Table { columns, rows })
    }

    fn page(&self, n: u32) -> Option<&[u8]> {
        let start = (n as usize).checked_sub(1)?.checked_mul(self.page_size)?;
        self.bytes.get(start..start.checked_add(self.page_size)?)
    }

    /// The rows of the table whose b-tree starts at page `root`: each its id and values.
    fn rows(&self, root: u32) -> Vec<(i64, Vec<Value>)> {
        let mut out = Vec::new();
        let mut seen = vec![false; self.bytes.len() / self.page_size + 1];
        let mut pending = vec![root];
        // each byte of a table's payloads is in the file once: more than that is cells sharing
        // their bytes (a damaged or crafted file asking for far more memory than it has)
        let mut budget = self.bytes.len();
        while let Some(n) = pending.pop() {
            if out.len() >= MOST_ROWS {
                break;
            }
            match seen.get_mut(n as usize) {
                Some(s) if !*s => *s = true,
                _ => continue,
            }
            let Some(page) = self.page(n) else { continue };
            let h = if n == 1 { 100 } else { 0 };
            let (Some(&kind), Some(count)) = (page.get(h), be16(page, h + 3)) else { continue };
            match kind {
                // interior: the children left to right (pushed so the leftmost comes off first)
                0x05 => {
                    let mut children: Vec<u32> = (0..count)
                        .filter_map(|i| {
                            be16(page, h + 12 + 2 * usize::from(i)).and_then(|at| be32(page, usize::from(at)))
                        })
                        .collect();
                    children.extend(be32(page, h + 8));
                    pending.extend(children.into_iter().rev());
                }
                0x0D => {
                    for i in 0..count {
                        let Some(at) = be16(page, h + 8 + 2 * usize::from(i)) else { break };
                        if out.len() >= MOST_ROWS {
                            break;
                        }
                        if let Some(row) = self.cell(page, usize::from(at), &mut budget) {
                            out.push(row);
                        }
                    }
                }
                _ => {}
            }
        }
        out
    }

    /// A leaf cell: its row id and record, the payload gathered from overflow pages (taken from
    /// `budget`, none if more than is left).
    fn cell(&self, page: &[u8], at: usize, budget: &mut usize) -> Option<(i64, Vec<Value>)> {
        let (len, a) = varint(page, at)?;
        let (id, b) = varint(page, at + a)?;
        let len = usize::try_from(len).ok().filter(|l| *l <= MOST_PAYLOAD)?;
        *budget = budget.checked_sub(len)?;
        let start = at + a + b;
        // how much of the payload is on this page (the rest on overflow pages)
        let u = self.usable;
        let most_here = u - 35;
        let local = if len <= most_here {
            len
        } else {
            let least = (u - 12) * 32 / 255 - 23;
            let k = least + (len - least) % (u - 4);
            if k <= most_here { k } else { least }
        };
        let mut payload = page.get(start..start + local)?.to_vec();
        let mut next = if local < len { be32(page, start + local)? } else { 0 };
        let mut hops = 0;
        while payload.len() < len && next != 0 && hops < self.bytes.len() / self.page_size {
            let over = self.page(next)?;
            let take = (len - payload.len()).min(u - 4);
            payload.extend_from_slice(over.get(4..4 + take)?);
            next = be32(over, 0)?;
            hops += 1;
        }
        if payload.len() < len {
            return None;
        }
        Some((id, self.record(&payload)?))
    }

    /// A record's values.
    fn record(&self, payload: &[u8]) -> Option<Vec<Value>> {
        let (header, mut at) = varint(payload, 0)?;
        let header = usize::try_from(header).ok()?;
        let mut body = header;
        let mut values = Vec::new();
        while at < header {
            let (kind, n) = varint(payload, at)?;
            at += n;
            let (value, used) = match kind {
                0 => (Value::Null, 0),
                1..=6 => {
                    let size = match kind {
                        5 => 6,
                        6 => 8,
                        k => k as usize,
                    };
                    let bytes = payload.get(body..body + size)?;
                    // big-endian, signed
                    let mut v: i64 = if bytes.first().is_some_and(|b| b & 0x80 != 0) { -1 } else { 0 };
                    for &b in bytes {
                        v = (v << 8) | i64::from(b);
                    }
                    (Value::Int(v), size)
                }
                7 => (Value::Real(f64::from_be_bytes(payload.get(body..body + 8)?.try_into().ok()?)), 8),
                8 => (Value::Int(0), 0),
                9 => (Value::Int(1), 0),
                k if k >= 12 && k % 2 == 0 => {
                    let size = usize::try_from((k - 12) / 2).ok()?;
                    (Value::Blob(payload.get(body..body.checked_add(size)?)?.to_vec()), size)
                }
                k if k >= 13 => {
                    let size = usize::try_from((k - 13) / 2).ok()?;
                    (Value::Text(self.text(payload.get(body..body.checked_add(size)?)?)), size)
                }
                _ => return None,
            };
            body += used;
            values.push(value);
        }
        Some(values)
    }

    fn text(&self, bytes: &[u8]) -> String {
        match self.utf16 {
            None => String::from_utf8_lossy(bytes).into_owned(),
            Some(big) => {
                let units: Vec<u16> = bytes
                    .as_chunks::<2>()
                    .0
                    .iter()
                    .map(|c| if big { u16::from_be_bytes(*c) } else { u16::from_le_bytes(*c) })
                    .collect();
                String::from_utf16_lossy(&units)
            }
        }
    }
}

/// A table's column names from its `CREATE TABLE` statement, and which one (if any) is the row's
/// id (an `INTEGER PRIMARY KEY`).
fn columns(sql: &str) -> (Vec<String>, Option<usize>) {
    let (Some(open), Some(close)) = (sql.find('('), sql.rfind(')')) else { return (Vec::new(), None) };
    let inside = sql.get(open + 1..close).unwrap_or_default();
    let mut parts = Vec::new();
    let (mut depth, mut from) = (0i32, 0usize);
    for (i, c) in inside.char_indices() {
        match c {
            '(' => depth += 1,
            ')' => depth -= 1,
            ',' if depth == 0 => {
                parts.push(inside.get(from..i).unwrap_or_default());
                from = i + 1;
            }
            _ => {}
        }
    }
    parts.push(inside.get(from..).unwrap_or_default());
    let unquote =
        |s: &str| s.trim().trim_matches(|c| c == '"' || c == '`' || c == '[' || c == ']' || c == '\'').to_owned();
    let mut names = Vec::new();
    let mut integer = Vec::new();
    // the primary key: a column's own (and whether it is the row's id), or the table's columns
    let mut rowid = None;
    let mut table_key = Vec::new();
    for part in parts {
        // the words in capitals, one space apart (as `PRIMARY KEY` may not be)
        let flat = part.split_whitespace().collect::<Vec<_>>().join(" ").to_ascii_uppercase();
        let key = flat.find("PRIMARY KEY").and_then(|at| flat.get(at + "PRIMARY KEY".len()..));
        // table constraints, not columns (their keyword may run into a parenthesis)
        let word: String = flat.chars().take_while(|c| c.is_ascii_alphanumeric() || *c == '_').collect();
        if ["PRIMARY", "UNIQUE", "CHECK", "FOREIGN", "CONSTRAINT"].contains(&word.as_str()) {
            if let Some(list) = key.and_then(|k| k.split_once('(')).and_then(|(_, l)| l.split_once(')')).map(|(l, _)| l)
            {
                table_key = list.split(',').map(|c| unquote(c.split_whitespace().next().unwrap_or_default())).collect();
            }
            continue;
        }
        let mut words = part.split_whitespace();
        let name = unquote(words.next().unwrap_or_default());
        let is_integer = words.next().is_some_and(|t| t.eq_ignore_ascii_case("INTEGER"));
        // the row's id: an INTEGER PRIMARY KEY, though not one declared DESC there
        if is_integer && key.is_some_and(|k| !k.trim_start().starts_with("DESC")) {
            rowid = Some(names.len());
        }
        names.push(name);
        integer.push(is_integer);
    }
    if rowid.is_none()
        && let [only] = table_key.as_slice()
    {
        rowid = names.iter().position(|n| n.eq_ignore_ascii_case(only)).filter(|&at| integer.get(at) == Some(&true));
    }
    (names, rowid)
}

fn be16(b: &[u8], at: usize) -> Option<u16> {
    Some(u16::from_be_bytes([*b.get(at)?, *b.get(at + 1)?]))
}

fn be32(b: &[u8], at: usize) -> Option<u32> {
    Some(u32::from_be_bytes(b.get(at..at + 4)?.try_into().ok()?))
}

/// A variable-length number (up to 9 bytes, seven bits each but the ninth's eight) and its length.
fn varint(b: &[u8], at: usize) -> Option<(i64, usize)> {
    let mut v: u64 = 0;
    for i in 0..9 {
        let byte = *b.get(at + i)?;
        if i == 8 {
            return Some((((v << 8) | u64::from(byte)) as i64, 9));
        }
        v = (v << 7) | u64::from(byte & 0x7F);
        if byte & 0x80 == 0 {
            return Some((v as i64, i + 1));
        }
    }
    None
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;

    /// A one-page-per-table database of 4096-byte pages: `tables` each a name, its `CREATE TABLE`
    /// statement and its rows (values as serial type and bytes), all on one leaf page; a value
    /// too big for its page spills onto overflow pages.
    pub(crate) fn database(tables: &[(&str, &str, Vec<Vec<Value>>)]) -> Vec<u8> {
        const PAGE: usize = 4096;
        fn put_varint(out: &mut Vec<u8>, mut v: u64) {
            let mut bytes = vec![(v & 0x7F) as u8];
            v >>= 7;
            while v > 0 {
                bytes.push((v & 0x7F) as u8 | 0x80);
                v >>= 7;
            }
            out.extend(bytes.iter().rev());
        }
        fn record(values: &[Value]) -> Vec<u8> {
            let (mut types, mut body) = (Vec::new(), Vec::new());
            for v in values {
                match v {
                    Value::Null => put_varint(&mut types, 0),
                    Value::Int(i) => {
                        put_varint(&mut types, 6);
                        body.extend_from_slice(&i.to_be_bytes());
                    }
                    Value::Real(r) => {
                        put_varint(&mut types, 7);
                        body.extend_from_slice(&r.to_be_bytes());
                    }
                    Value::Text(t) => {
                        put_varint(&mut types, 13 + 2 * t.len() as u64);
                        body.extend_from_slice(t.as_bytes());
                    }
                    Value::Blob(b) => {
                        put_varint(&mut types, 12 + 2 * b.len() as u64);
                        body.extend_from_slice(b);
                    }
                }
            }
            let mut out = Vec::new();
            put_varint(&mut out, types.len() as u64 + 1);
            out.extend(types);
            out.extend(body);
            out
        }
        // page 1 the schema; then each table's leaf; overflow pages after
        let mut pages: Vec<Vec<u8>> = vec![Vec::new(); 1 + tables.len()];
        let leaf = |pages: &mut Vec<Vec<u8>>, n: usize, rows: Vec<Vec<Value>>| {
            let h = if n == 0 { 100 } else { 0 };
            let mut page = vec![0u8; PAGE];
            let mut cells: Vec<Vec<u8>> = Vec::new();
            for (id, row) in rows.iter().enumerate() {
                let payload = record(row);
                let mut cell = Vec::new();
                put_varint(&mut cell, payload.len() as u64);
                put_varint(&mut cell, id as u64 + 1);
                let u = PAGE;
                let (most, least) = (u - 35, (u - 12) * 32 / 255 - 23);
                if payload.len() <= most {
                    cell.extend(&payload);
                } else {
                    let k = least + (payload.len() - least) % (u - 4);
                    let local = if k <= most { k } else { least };
                    cell.extend(&payload[..local]);
                    let mut rest = &payload[local..];
                    let first = pages.len() + 1;
                    cell.extend_from_slice(&(first as u32).to_be_bytes());
                    while !rest.is_empty() {
                        let take = rest.len().min(u - 4);
                        let mut over = vec![0u8; PAGE];
                        let next = if rest.len() > take { pages.len() + 2 } else { 0 };
                        over[..4].copy_from_slice(&(next as u32).to_be_bytes());
                        over[4..4 + take].copy_from_slice(&rest[..take]);
                        pages.push(over);
                        rest = &rest[take..];
                    }
                }
                cells.push(cell);
            }
            page[h] = 0x0D;
            page[h + 3..h + 5].copy_from_slice(&(cells.len() as u16).to_be_bytes());
            let mut end = PAGE;
            for (i, cell) in cells.iter().enumerate() {
                end -= cell.len();
                page[end..end + cell.len()].copy_from_slice(cell);
                page[h + 8 + 2 * i..h + 10 + 2 * i].copy_from_slice(&(end as u16).to_be_bytes());
            }
            page[h + 5..h + 7].copy_from_slice(&(end as u16).to_be_bytes());
            pages[n] = page;
        };
        let schema: Vec<Vec<Value>> = tables
            .iter()
            .enumerate()
            .map(|(i, (name, sql, _))| {
                vec![
                    Value::Text("table".into()),
                    Value::Text((*name).into()),
                    Value::Text((*name).into()),
                    Value::Int(i as i64 + 2),
                    Value::Text((*sql).into()),
                ]
            })
            .collect();
        for (i, (_, _, rows)) in tables.iter().enumerate() {
            leaf(&mut pages, i + 1, rows.clone());
        }
        leaf(&mut pages, 0, schema);
        let mut out: Vec<u8> = pages.concat();
        out[..16].copy_from_slice(b"SQLite format 3\0");
        out[16..18].copy_from_slice(&(PAGE as u16).to_be_bytes());
        out[56..60].copy_from_slice(&1u32.to_be_bytes());
        out
    }

    #[test]
    fn tables_read_with_their_columns_ids_and_long_values() {
        let long: Vec<u8> = (0..20_000).map(|i| (i % 251) as u8).collect();
        let db = database(&[
            (
                "Node",
                "CREATE TABLE Node (_PW_ID INTEGER PRIMARY KEY AUTOINCREMENT, NodeName TEXT, NodeVariantID INTEGER)",
                vec![
                    vec![Value::Null, Value::Text("Pencil".into()), Value::Int(3)],
                    vec![Value::Null, Value::Text("Ink".into()), Value::Int(4)],
                ],
            ),
            (
                "Material",
                "CREATE TABLE \"Material\" (id INTEGER, \"FileData\" BLOB, Size REAL, UNIQUE(id))",
                vec![vec![Value::Int(1), Value::Blob(long.clone()), Value::Real(2.5)]],
            ),
        ]);
        let d = open(&db).unwrap();
        let node = d.table("node").unwrap();
        assert_eq!(node.columns, ["_PW_ID", "NodeName", "NodeVariantID"]);
        assert_eq!(node.rows.len(), 2);
        assert_eq!(node.get(&node.rows[1], "NodeName").and_then(Value::as_text), Some("Ink"));
        // the id column holds the row's id
        assert_eq!(node.get(&node.rows[1], "_PW_ID").and_then(Value::as_int), Some(2));
        let m = d.table("Material").unwrap();
        assert_eq!(m.columns, ["id", "FileData", "Size"]);
        assert_eq!(m.get(&m.rows[0], "FileData").and_then(Value::as_blob), Some(&long[..]));
        assert_eq!(m.get(&m.rows[0], "Size").and_then(Value::as_real), Some(2.5));
        assert!(d.table("Nothing").is_none());
    }

    #[test]
    fn the_row_id_column_is_found_however_the_key_is_written() {
        let rowid = |sql: &str| columns(sql).1;
        assert_eq!(rowid("CREATE TABLE t(id INTEGER PRIMARY KEY, x)"), Some(0));
        assert_eq!(rowid("CREATE TABLE t(x, id integer not null primary\n  key autoincrement)"), Some(1));
        assert_eq!(rowid("CREATE TABLE t(x TEXT, id INTEGER, PRIMARY KEY(id))"), Some(1));
        assert_eq!(rowid("CREATE TABLE t(\"id\" INTEGER, CONSTRAINT k PRIMARY KEY (\"ID\" ASC))"), Some(0));
        // not the row's id: another type, a key of two columns, the DESC quirk
        assert_eq!(rowid("CREATE TABLE t(IntegerName TEXT PRIMARY KEY)"), None);
        assert_eq!(rowid("CREATE TABLE t(a INTEGER, b INTEGER, PRIMARY KEY(a, b))"), None);
        assert_eq!(rowid("CREATE TABLE t(id INTEGER PRIMARY KEY DESC)"), None);
        assert_eq!(rowid("CREATE TABLE t(id INT PRIMARY KEY)"), None);
    }

    #[test]
    fn cells_sharing_their_bytes_take_no_more_than_the_file_has() {
        let db = database(&[("T", "CREATE TABLE T (b BLOB)", vec![vec![Value::Blob(vec![7; 3000])]])]);
        // the table's leaf (page 2) made to list its one cell 500 times
        let mut bad = db.clone();
        let leaf = 4096;
        let at = [bad[leaf + 8], bad[leaf + 9]];
        bad[leaf + 3..leaf + 5].copy_from_slice(&500u16.to_be_bytes());
        for i in 0..500 {
            bad[leaf + 8 + 2 * i..leaf + 10 + 2 * i].copy_from_slice(&at);
        }
        let rows = open(&bad).unwrap().table("T").unwrap().rows;
        assert!(!rows.is_empty() && rows.len() * 3000 <= bad.len(), "{} rows", rows.len());
    }

    #[test]
    fn other_files_and_damaged_ones_are_refused_not_a_crash() {
        assert!(open(b"not a database").is_none());
        let db = database(&[(
            "T",
            "CREATE TABLE T (a TEXT, b BLOB)",
            vec![vec![Value::Text("x".into()), Value::Blob(vec![1; 5000])]],
        )]);
        crate::damaged::survives("SQLite", &db, |b| {
            if let Some(d) = open(b) {
                let _ = d.table("T");
            }
        });
    }
}
