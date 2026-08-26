// SPDX-License-Identifier: Apache-2.0
//! Validate the machine-readable record-layout tables under `docs/layouts/`.
//!
//! The tables carry the numbers (offsets, widths, sizes, endianness) that the
//! format specifications state in prose. Prose byte-offset paragraphs drift
//! against each other and against the codecs; a table that a test arithmetically
//! closes does not. This test is the enforcement half of that contract:
//!
//! * every record and every field cites a specification section and an anchor
//!   phrase, and the phrase must actually occur inside that section — a field
//!   whose numbers are invented cannot pass, because it has nothing to cite;
//! * byte records must tile their declared size exactly, with every unstated
//!   region named as an explicit `[[record.gap]]`;
//! * the set of arithmetic problems the validator computes must equal the set
//!   of `[[record.discrepancy]]` blocks the table declares, so a spec-versus-spec
//!   contradiction has to be written down and an invented one is rejected;
//! * every multi-byte field resolves an endianness, or says in a note that the
//!   specification does not state one;
//! * `[[record.code]]` cross-checks assert a literal substring is present in a
//!   named source file, which is how a table claims agreement with a parser.
//!
//! After validation the test emits one `src/layout.rs` per mapped table: a
//! module of `usize` offset constants per byte-layout record, a `*_VALUE`
//! constant for each field that declares `value`, and a `token` module of tag
//! constants. Records that declare a `[[record.discrepancy]]` are listed in a
//! comment and omitted. `UPDATE_LAYOUT_CODE=1` rewrites the checked-in files;
//! a table edit without regeneration fails the byte-for-byte comparison.
//! Parsing functions are not generated.
//!
//! `layout_validator_rejects_broken_tables` runs the same validator over
//! broken fixtures in `tests/fixtures/layout-invalid/`.

#![allow(clippy::unwrap_used)] // Test code: a failed unwrap is a test failure.

use std::collections::{BTreeMap, BTreeSet};
use std::fmt::Write as _;
use std::io::Write as IoWrite;
use std::path::{Component, Path, PathBuf};
use std::process::{Command, Stdio};

use serde::Deserialize;

/// Every format directory the workspace ships a codec or spec for.
const EXPECTED_FORMATS: &[&str] = &[
    "asm", "catia", "creo", "f3d", "freecad", "iges", "inventor", "nx", "protein", "rhino",
    "sldprt", "step",
];

// ---------------------------------------------------------------------------
// Schema
// ---------------------------------------------------------------------------

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct LayoutFile {
    schema: u32,
    format: String,
    /// Specification document the anchors resolve against, repo-relative.
    spec: String,
    #[serde(default)]
    note: String,
    /// File-wide endianness default for multi-byte fields.
    #[serde(default)]
    endianness: Option<String>,
    #[serde(default, rename = "type")]
    types: Vec<TypeDecl>,
    #[serde(default, rename = "token")]
    tokens: Vec<TokenDecl>,
    #[serde(default, rename = "record")]
    records: Vec<Record>,
    #[serde(default, rename = "not_applicable")]
    not_applicable: Vec<NotApplicable>,
}

/// A composite fixed-width unit the specification treats as one field, such as
/// a tagged SAB chunk. Declaring it once keeps its width and endianness in a
/// single place instead of repeating them on every field that uses it.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct TypeDecl {
    name: String,
    bytes: u64,
    /// Required: a composite type states the endianness of its numeric part.
    endianness: String,
    note: String,
    #[serde(default)]
    section: Option<String>,
    #[serde(default)]
    anchor: Option<String>,
}

/// A tag-byte inventory row (f3d SAB tags, CATIA markers, 3dm typecodes).
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct TokenDecl {
    tag: String,
    name: String,
    /// Payload width in bytes; absent means variable-length.
    #[serde(default)]
    payload_bytes: Option<u64>,
    note: String,
    section: String,
    anchor: String,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct Record {
    name: String,
    /// `byte` = absolute byte offsets; `slot` = ordered typed slots with no
    /// stated offsets; `column` = 1-based inclusive character columns.
    kind: RecordKind,
    /// Section number in `spec`, e.g. `6.2`.
    section: String,
    /// Phrase that must occur inside that section.
    anchor: String,
    #[serde(default)]
    size: Option<u64>,
    #[serde(default)]
    endianness: Option<String>,
    #[serde(default)]
    note: String,
    /// Parser source paths. A locator, not a substring check.
    #[serde(default, deserialize_with = "deserialize_one_or_many")]
    parsed_by: Vec<String>,
    #[serde(default, rename = "field")]
    fields: Vec<Field>,
    #[serde(default, rename = "gap")]
    gaps: Vec<Gap>,
    #[serde(default, rename = "discrepancy")]
    discrepancies: Vec<Discrepancy>,
    #[serde(default, rename = "code")]
    code: Vec<CodeCheck>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "lowercase")]
enum RecordKind {
    Byte,
    Slot,
    Column,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct Field {
    name: String,
    /// Byte offset from the record start. Required for `kind = "byte"`.
    #[serde(default)]
    offset: Option<u64>,
    /// 1-based inclusive column range `"a-b"`. Required for `kind = "column"`.
    #[serde(default)]
    columns: Option<String>,
    #[serde(rename = "type")]
    ty: String,
    #[serde(default)]
    endianness: Option<String>,
    /// `spec` = the section states this outright; `derived` = arithmetic over
    /// values the section states; `code` = the anchor is a parser fact.
    source: Source,
    /// Phrase that must occur inside the record's section.
    anchor: String,
    #[serde(default)]
    note: String,
    /// Constant contents the specification states for this field.
    #[serde(default)]
    value: Option<toml::Value>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "lowercase")]
enum Source {
    Spec,
    Derived,
    Code,
}

/// A byte range inside a `byte` record for which the specification states no
/// field. Gaps are named rather than skipped so the unknowns stay visible.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct Gap {
    offset: u64,
    size: u64,
    note: String,
}

/// A contradiction the table records instead of resolving. Every entry must
/// correspond to a problem the validator independently computes, and every
/// computed problem must have an entry.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct Discrepancy {
    kind: DiscrepancyKind,
    /// The value the table's own fields add up to.
    #[serde(default)]
    computed: Option<u64>,
    /// The value the specification declares.
    #[serde(default)]
    declared: Option<u64>,
    note: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Deserialize)]
#[serde(rename_all = "snake_case")]
enum DiscrepancyKind {
    /// Fields plus gaps do not end at the declared record size.
    SizeMismatch,
    /// Two stated field extents cover the same byte.
    Overlap,
}

/// A literal substring that must be present in a source file. This is how a
/// table claims a parser agrees with it on a fact the emitter cannot express.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct CodeCheck {
    path: String,
    contains: String,
    note: String,
}

fn deserialize_one_or_many<'de, D>(deserializer: D) -> Result<Vec<String>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    struct OneOrMany;

    impl<'de> serde::de::Visitor<'de> for OneOrMany {
        type Value = Vec<String>;

        fn expecting(&self, formatter: &mut std::fmt::Formatter) -> std::fmt::Result {
            formatter.write_str("a path string or an array of paths")
        }

        fn visit_str<E: serde::de::Error>(self, value: &str) -> Result<Self::Value, E> {
            Ok(vec![value.to_string()])
        }

        fn visit_string<E: serde::de::Error>(self, value: String) -> Result<Self::Value, E> {
            Ok(vec![value])
        }

        fn visit_seq<A>(self, mut seq: A) -> Result<Self::Value, A::Error>
        where
            A: serde::de::SeqAccess<'de>,
        {
            let mut out = Vec::new();
            while let Some(item) = seq.next_element()? {
                out.push(item);
            }
            Ok(out)
        }
    }

    deserializer.deserialize_any(OneOrMany)
}

/// True when `path` names a test module rather than a parser.
fn is_test_source_path(path: &str) -> bool {
    Path::new(path)
        .components()
        .any(|component| match component {
            Component::Normal(name) => name == "tests",
            _ => false,
        })
}

/// A token tag that can be emitted as a Rust constant.
#[derive(Debug)]
enum TokenConst {
    U8(u8),
    U16(u16),
    U32(u32),
    U64(u64),
    Bytes(Vec<u8>),
}

/// Parse a token `tag` into a typed constant. Ranges such as `00..7f` are omitted.
fn parse_token_tag(tag: &str) -> Option<TokenConst> {
    if tag.contains("..") {
        return None;
    }
    let compact = tag.trim();
    if let Some(hex) = compact
        .strip_prefix("0x")
        .or_else(|| compact.strip_prefix("0X"))
    {
        let hex = hex.replace('_', "");
        let value = u64::from_str_radix(&hex, 16).ok()?;
        return Some(match hex.len() {
            1 | 2 => TokenConst::U8(u8::try_from(value).ok()?),
            3 | 4 => TokenConst::U16(u16::try_from(value).ok()?),
            5..=8 => TokenConst::U32(u32::try_from(value).ok()?),
            9..=16 => TokenConst::U64(value),
            _ => return None,
        });
    }
    let parts: Vec<&str> = compact.split_whitespace().collect();
    if parts.len() > 1
        && parts
            .iter()
            .all(|part| part.len() == 2 && part.chars().all(|c| c.is_ascii_hexdigit()))
    {
        let bytes = parts
            .iter()
            .map(|part| u8::from_str_radix(part, 16).ok())
            .collect::<Option<Vec<u8>>>()?;
        return Some(TokenConst::Bytes(bytes));
    }
    if let Ok(value) = compact.parse::<u64>() {
        return Some(if let Ok(value) = u8::try_from(value) {
            TokenConst::U8(value)
        } else if let Ok(value) = u16::try_from(value) {
            TokenConst::U16(value)
        } else if let Ok(value) = u32::try_from(value) {
            TokenConst::U32(value)
        } else {
            TokenConst::U64(value)
        });
    }
    if !tag.is_empty() && tag.bytes().all(|b| (0x20..=0x7e).contains(&b)) {
        return Some(TokenConst::Bytes(tag.as_bytes().to_vec()));
    }
    None
}

fn token_const_name(name: &str) -> Option<String> {
    let ident: String = name
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() { c } else { '_' })
        .collect::<String>()
        .split('_')
        .filter(|part| !part.is_empty())
        .collect::<Vec<_>>()
        .join("_")
        .to_ascii_uppercase();
    if ident.is_empty() || ident.starts_with(|c: char| c.is_ascii_digit()) {
        return None;
    }
    Some(ident)
}

fn rust_hex(value: u64, digits: usize) -> String {
    let raw = format!("{value:0digits$x}");
    let mut parts = Vec::new();
    let mut rest = raw.as_str();
    while rest.len() > 4 {
        let split = rest.len() - 4;
        parts.push(&rest[split..]);
        rest = &rest[..split];
    }
    parts.push(rest);
    parts.reverse();
    format!("0x{}", parts.join("_"))
}

fn rust_byte_array(bytes: &[u8]) -> String {
    let ascii_ok = bytes.iter().all(|&b| b == 0 || (0x20..=0x7e).contains(&b));
    let has_letter = bytes.iter().any(|&b| b.is_ascii_alphabetic());
    if ascii_ok && has_letter {
        let mut out = String::from("*b\"");
        for &b in bytes {
            match b {
                0 => out.push_str("\\0"),
                b'\\' => out.push_str("\\\\"),
                b'"' => out.push_str("\\\""),
                _ => out.push(b as char),
            }
        }
        out.push('"');
        return out;
    }
    let parts: Vec<String> = bytes.iter().map(|b| format!("0x{b:02x}")).collect();
    format!("[{}]", parts.join(", "))
}

fn rust_token_ty(value: &TokenConst) -> String {
    match value {
        TokenConst::U8(_) => "u8".to_string(),
        TokenConst::U16(_) => "u16".to_string(),
        TokenConst::U32(_) => "u32".to_string(),
        TokenConst::U64(_) => "u64".to_string(),
        TokenConst::Bytes(bytes) => format!("[u8; {}]", bytes.len()),
    }
}

/// Decode a field `value` against its declared type. `width` is the byte width.
fn decode_field_value(raw: &toml::Value, ty: &str, width: Option<u64>) -> Result<String, String> {
    let element = ty.split_once('[').map_or(ty, |(head, _)| head);
    if matches!(
        element,
        "cstring" | "lp_ascii" | "lp_utf16" | "token_stream" | "subrecord" | "array" | "text"
    ) {
        return Err("value is not valid on a variable-length type".to_string());
    }
    if element == "bytes" {
        let Some(len) = width else {
            return Err("value on `bytes` needs a fixed width".to_string());
        };
        let bytes = match raw {
            toml::Value::String(text) => text.as_bytes().to_vec(),
            toml::Value::Array(items) => items
                .iter()
                .map(|item| {
                    let n = item
                        .as_integer()
                        .ok_or_else(|| format!("byte value entry is not an integer: {item}"))?;
                    u8::try_from(n).map_err(|_| format!("byte value {n} is out of 0..=255"))
                })
                .collect::<Result<Vec<u8>, _>>()?,
            _ => return Err("bytes value must be a string or an array of integers".to_string()),
        };
        if u64::try_from(bytes.len()).unwrap_or(u64::MAX) != len {
            return Err(format!(
                "value is {} bytes, type `{ty}` is {len} bytes",
                bytes.len()
            ));
        }
        return Ok(format!("[u8; {len}] = {}", rust_byte_array(&bytes)));
    }
    match element {
        "u8" | "enum8" | "bool8" | "char" => {
            let n = raw
                .as_integer()
                .ok_or_else(|| "integer value required".to_string())?;
            let v = u8::try_from(n).map_err(|_| format!("value {n} does not fit `{ty}`"))?;
            Ok(format!("u8 = {v}"))
        }
        "u16" => {
            let n = raw
                .as_integer()
                .ok_or_else(|| "integer value required".to_string())?;
            let v = u16::try_from(n).map_err(|_| format!("value {n} does not fit `{ty}`"))?;
            Ok(format!("u16 = {}", rust_hex(u64::from(v), 4)))
        }
        "u32" => {
            let n = raw
                .as_integer()
                .ok_or_else(|| "integer value required".to_string())?;
            let v = u32::try_from(n).map_err(|_| format!("value {n} does not fit `{ty}`"))?;
            Ok(format!("u32 = {}", rust_hex(u64::from(v), 8)))
        }
        "u64" => {
            let n = raw
                .as_integer()
                .ok_or_else(|| "integer value required".to_string())?;
            let v = u64::try_from(n).map_err(|_| format!("value {n} does not fit `{ty}`"))?;
            Ok(format!("u64 = {}", rust_hex(v, 16)))
        }
        "i8" | "i16" | "i32" | "i64" => {
            let n = raw
                .as_integer()
                .ok_or_else(|| "integer value required".to_string())?;
            Ok(format!("{element} = {n}"))
        }
        "f32" | "f64" => {
            let n = raw
                .as_float()
                .or_else(|| raw.as_integer().map(|i| i as f64))
                .ok_or_else(|| "float value required".to_string())?;
            Ok(format!("{element} = {n:?}"))
        }
        _ => Err(format!("value is not supported on type `{ty}`")),
    }
}

/// A part of a format that has no tabulatable layout, with the reason.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct NotApplicable {
    area: String,
    reason: String,
    section: String,
    anchor: String,
}

// ---------------------------------------------------------------------------
// Type widths
// ---------------------------------------------------------------------------

/// A builtin type's extent.
#[derive(Debug, Clone, Copy)]
enum Builtin {
    Fixed(u64),
    Variable,
}

/// Look up a builtin type name, or `None` when the name is not a builtin.
fn builtin_width(ty: &str) -> Option<Builtin> {
    let fixed = |n: u64| Some(Builtin::Fixed(n));
    match ty {
        "u8" | "i8" | "bool8" | "enum8" | "char" => fixed(1),
        "u16" | "i16" => fixed(2),
        "u24" => fixed(3),
        "u32" | "i32" | "f32" => fixed(4),
        "u64" | "i64" | "f64" => fixed(8),
        // Variable-length: legal in `slot` records, rejected in `byte` records.
        "cstring" | "lp_ascii" | "lp_utf16" | "token_stream" | "subrecord" | "array" | "text" => {
            Some(Builtin::Variable)
        }
        _ => None,
    }
}

/// Resolve `type`, including `name[N]` array syntax and file-local composites.
fn type_width(ty: &str, custom: &BTreeMap<String, u64>) -> Result<Option<u64>, String> {
    if let Some(open) = ty.find('[') {
        let Some(close) = ty.strip_suffix(']') else {
            return Err(format!("malformed array type `{ty}`"));
        };
        let count: u64 = close[open + 1..]
            .parse()
            .map_err(|_| format!("malformed array length in `{ty}`"))?;
        let element = &ty[..open];
        if element == "bytes" {
            return Ok(Some(count));
        }
        let inner = type_width(element, custom)?
            .ok_or_else(|| format!("variable-length element in array type `{ty}`"))?;
        return Ok(Some(inner * count));
    }
    if let Some(width) = custom.get(ty) {
        return Ok(Some(*width));
    }
    match builtin_width(ty) {
        Some(Builtin::Fixed(width)) => Ok(Some(width)),
        Some(Builtin::Variable) => Ok(None),
        None => Err(format!("unknown type `{ty}`")),
    }
}

/// Types whose bytes are a number, or an array of them, and therefore need an
/// endianness. `bytes[N]` is opaque and does not.
fn is_multibyte_scalar(ty: &str) -> bool {
    let element = ty.split_once('[').map_or(ty, |(head, _)| head);
    matches!(
        element,
        "u16" | "i16" | "u24" | "u32" | "i32" | "f32" | "u64" | "i64" | "f64"
    )
}

// ---------------------------------------------------------------------------
// Specification anchors
// ---------------------------------------------------------------------------

/// Collapse whitespace so an anchor matches across markdown table padding and
/// hard-wrapped prose.
fn normalize(text: &str) -> String {
    text.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// Map each section of a markdown document to its normalized body.
///
/// A numbered heading (`## 7.` / `### 7.6 Title`, the convention
/// `scripts/check-doc-anchors.py` already depends on) is keyed by its number.
/// Every heading is additionally keyed by its full title text, because some
/// specifications — `iges.md` — use unnumbered headings.
fn section_bodies(markdown: &str) -> BTreeMap<String, String> {
    let lines: Vec<&str> = markdown.lines().collect();
    let mut starts: Vec<(usize, Vec<String>)> = Vec::new();
    for (index, line) in lines.iter().enumerate() {
        let hashes = line.len() - line.trim_start_matches('#').len();
        if !(2..=6).contains(&hashes) {
            continue;
        }
        let rest = line[hashes..].trim_start();
        if rest.len() == line[hashes..].len() {
            continue; // No space after the hashes.
        }
        let mut keys = vec![normalize(rest)];
        let number: String = rest
            .chars()
            .take_while(|c| c.is_ascii_digit() || *c == '.')
            .collect();
        if !number.is_empty() && rest[number.len()..].starts_with(char::is_whitespace) {
            keys.push(number.trim_end_matches('.').to_string());
        }
        starts.push((index, keys));
    }
    let mut bodies = BTreeMap::new();
    for (position, (index, keys)) in starts.iter().enumerate() {
        let end = starts
            .get(position + 1)
            .map_or(lines.len(), |(next, _)| *next);
        let body = normalize(&lines[*index..end].join(" "));
        for key in keys {
            bodies.insert(key.clone(), body.clone());
        }
    }
    bodies
}

// ---------------------------------------------------------------------------
// Validation
// ---------------------------------------------------------------------------

struct Context {
    root: PathBuf,
    /// Spec path -> section number -> normalized body.
    specs: BTreeMap<String, BTreeMap<String, String>>,
}

impl Context {
    fn new(root: PathBuf) -> Self {
        Self {
            root,
            specs: BTreeMap::new(),
        }
    }

    fn sections(&mut self, spec: &str) -> Result<&BTreeMap<String, String>, String> {
        if !self.specs.contains_key(spec) {
            let path = self.root.join(spec);
            let text = read_text(&path).map_err(|e| format!("cannot read spec `{spec}`: {e}"))?;
            self.specs.insert(spec.to_string(), section_bodies(&text));
        }
        Ok(&self.specs[spec])
    }
}

/// Read a text file with `\r\n` folded to `\n`.
fn read_text(path: &Path) -> std::io::Result<String> {
    Ok(std::fs::read_to_string(path)?.replace("\r\n", "\n"))
}

/// Check that `anchor` occurs inside `section` of `spec`.
fn check_anchor(
    ctx: &mut Context,
    spec: &str,
    section: &str,
    anchor: &str,
    what: &str,
    errors: &mut Vec<String>,
) {
    if anchor.trim().is_empty() {
        errors.push(format!("{what}: empty anchor"));
        return;
    }
    let sections = match ctx.sections(spec) {
        Ok(sections) => sections,
        Err(message) => {
            errors.push(format!("{what}: {message}"));
            return;
        }
    };
    let Some(body) = sections.get(section) else {
        errors.push(format!("{what}: `{spec}` has no section §{section}"));
        return;
    };
    if !body.contains(&normalize(anchor)) {
        errors.push(format!(
            "{what}: anchor not found in `{spec}` §{section}: {anchor:?}"
        ));
    }
}

#[allow(clippy::too_many_lines)]
fn validate(ctx: &mut Context, path: &Path, file: &LayoutFile) -> Vec<String> {
    let mut errors = Vec::new();
    let where_file = path.file_name().unwrap().to_string_lossy().to_string();
    let push = |errors: &mut Vec<String>, message: String| errors.push(message);

    if file.schema != 1 {
        push(
            &mut errors,
            format!("{where_file}: unsupported schema version {}", file.schema),
        );
    }
    if file.format.trim().is_empty() {
        push(&mut errors, format!("{where_file}: empty `format`"));
    }
    if !ctx.root.join(&file.spec).is_file() {
        push(
            &mut errors,
            format!("{where_file}: `spec` path does not exist: {}", file.spec),
        );
    }

    // Composite type declarations.
    let mut custom: BTreeMap<String, u64> = BTreeMap::new();
    for decl in &file.types {
        if builtin_width(&decl.name).is_some() {
            push(
                &mut errors,
                format!("{where_file}: type `{}` shadows a builtin", decl.name),
            );
        }
        if custom.insert(decl.name.clone(), decl.bytes).is_some() {
            push(
                &mut errors,
                format!("{where_file}: duplicate type `{}`", decl.name),
            );
        }
        if decl.bytes == 0 {
            push(
                &mut errors,
                format!("{where_file}: type `{}` has zero width", decl.name),
            );
        }
        if !matches!(decl.endianness.as_str(), "little" | "big" | "n/a") {
            push(
                &mut errors,
                format!(
                    "{where_file}: type `{}` endianness must be little|big|n/a, got {:?}",
                    decl.name, decl.endianness
                ),
            );
        }
        if decl.note.trim().is_empty() {
            push(
                &mut errors,
                format!("{where_file}: type `{}` has an empty note", decl.name),
            );
        }
        if let (Some(section), Some(anchor)) = (&decl.section, &decl.anchor) {
            check_anchor(
                ctx,
                &file.spec,
                section,
                anchor,
                &format!("{where_file}: type `{}`", decl.name),
                &mut errors,
            );
        }
    }

    // Token inventories.
    let mut seen_tags = BTreeSet::new();
    let mut seen_token_consts = BTreeSet::new();
    for token in &file.tokens {
        if !seen_tags.insert(token.tag.clone()) {
            push(
                &mut errors,
                format!("{where_file}: duplicate token tag `{}`", token.tag),
            );
        }
        if token.name.trim().is_empty() {
            push(
                &mut errors,
                format!("{where_file}: token `{}` has an empty name", token.tag),
            );
        }
        if parse_token_tag(&token.tag).is_some() {
            match token_const_name(&token.name) {
                None => push(
                    &mut errors,
                    format!(
                        "{where_file}: token `{}` name `{}` is not a Rust constant",
                        token.tag, token.name
                    ),
                ),
                Some(ident) => {
                    if !seen_token_consts.insert(ident.clone()) {
                        push(
                            &mut errors,
                            format!("{where_file}: duplicate token constant `{ident}`"),
                        );
                    }
                }
            }
        }
        check_anchor(
            ctx,
            &file.spec,
            &token.section,
            &token.anchor,
            &format!("{where_file}: token `{}`", token.tag),
            &mut errors,
        );
    }

    for entry in &file.not_applicable {
        if entry.reason.trim().is_empty() {
            push(
                &mut errors,
                format!(
                    "{where_file}: not_applicable `{}` has an empty reason",
                    entry.area
                ),
            );
        }
        check_anchor(
            ctx,
            &file.spec,
            &entry.section,
            &entry.anchor,
            &format!("{where_file}: not_applicable `{}`", entry.area),
            &mut errors,
        );
    }

    let mut record_names = BTreeSet::new();
    for record in &file.records {
        let at = format!("{where_file}: record `{}`", record.name);
        if !record_names.insert(record.name.clone()) {
            push(&mut errors, format!("{at}: duplicate record name"));
        }
        if record.fields.is_empty() {
            push(&mut errors, format!("{at}: no fields"));
        }
        check_anchor(
            ctx,
            &file.spec,
            &record.section,
            &record.anchor,
            &at,
            &mut errors,
        );

        for path in &record.parsed_by {
            if path.trim().is_empty() {
                push(&mut errors, format!("{at}: empty `parsed_by` path"));
                continue;
            }
            if is_test_source_path(path) {
                push(
                    &mut errors,
                    format!("{at}: `parsed_by` names a test path `{path}`"),
                );
            }
            if !ctx.root.join(path).is_file() {
                push(
                    &mut errors,
                    format!("{at}: `parsed_by` path does not exist: {path}"),
                );
            }
        }

        let mut field_names = BTreeSet::new();
        for field in &record.fields {
            let at = format!("{at}, field `{}`", field.name);
            if !field_names.insert(field.name.clone()) {
                push(&mut errors, format!("{at}: duplicate field name"));
            }
            check_anchor(
                ctx,
                &file.spec,
                &record.section,
                &field.anchor,
                &at,
                &mut errors,
            );
            if field.source == Source::Derived && field.note.trim().is_empty() {
                push(
                    &mut errors,
                    format!("{at}: `source = \"derived\"` requires a note stating the derivation"),
                );
            }

            let width = match type_width(&field.ty, &custom) {
                Ok(width) => width,
                Err(message) => {
                    push(&mut errors, format!("{at}: {message}"));
                    continue;
                }
            };
            if let Some(raw) = &field.value {
                if let Err(message) = decode_field_value(raw, &field.ty, width) {
                    push(&mut errors, format!("{at}: {message}"));
                }
            }

            // Endianness: field override, then record, then file default.
            if is_multibyte_scalar(&field.ty) {
                let resolved = field
                    .endianness
                    .as_deref()
                    .or(record.endianness.as_deref())
                    .or(file.endianness.as_deref());
                match resolved {
                    None => push(
                        &mut errors,
                        format!("{at}: multi-byte field resolves no endianness"),
                    ),
                    Some("unstated") if field.note.trim().is_empty() => push(
                        &mut errors,
                        format!(
                            "{at}: endianness `unstated` requires a note saying the spec omits it"
                        ),
                    ),
                    Some("little" | "big" | "unstated") => {}
                    Some(other) => push(
                        &mut errors,
                        format!("{at}: endianness must be little|big|unstated, got {other:?}"),
                    ),
                }
            }

            match record.kind {
                RecordKind::Byte => {
                    if field.offset.is_none() {
                        push(
                            &mut errors,
                            format!("{at}: byte record field needs `offset`"),
                        );
                    }
                    if field.columns.is_some() {
                        push(&mut errors, format!("{at}: `columns` in a byte record"));
                    }
                    if width.is_none() {
                        push(
                            &mut errors,
                            format!("{at}: variable-length type `{}` in a byte record", field.ty),
                        );
                    }
                }
                RecordKind::Slot => {
                    if field.offset.is_some() {
                        push(
                            &mut errors,
                            format!("{at}: slot record field must not state `offset`"),
                        );
                    }
                    if field.columns.is_some() {
                        push(&mut errors, format!("{at}: `columns` in a slot record"));
                    }
                }
                RecordKind::Column => {
                    if field.columns.is_none() {
                        push(
                            &mut errors,
                            format!("{at}: column record field needs `columns`"),
                        );
                    }
                    if field.offset.is_some() {
                        push(&mut errors, format!("{at}: `offset` in a column record"));
                    }
                }
            }
        }

        match record.kind {
            RecordKind::Byte => validate_byte_extent(record, &custom, &at, &mut errors),
            RecordKind::Column => validate_columns(record, &at, &mut errors),
            RecordKind::Slot => {
                if !record.gaps.is_empty() {
                    push(
                        &mut errors,
                        format!("{at}: `gap` is only valid for byte records"),
                    );
                }
                if !record.discrepancies.is_empty() {
                    push(
                        &mut errors,
                        format!("{at}: `discrepancy` is only valid for byte or column records"),
                    );
                }
            }
        }

        for check in &record.code {
            if is_test_source_path(&check.path) {
                push(
                    &mut errors,
                    format!("{at}: code check path `{}` names a test file", check.path),
                );
            }
            let path = ctx.root.join(&check.path);
            match read_text(&path) {
                Err(e) => push(
                    &mut errors,
                    format!("{at}: code check cannot read `{}`: {e}", check.path),
                ),
                Ok(text) => {
                    if !normalize(&text).contains(&normalize(&check.contains)) {
                        push(
                            &mut errors,
                            format!(
                                "{at}: code check failed, `{}` does not contain {:?}",
                                check.path, check.contains
                            ),
                        );
                    }
                }
            }
        }
    }

    errors
}

/// Byte records must tile `[0, size)` with fields and explicitly named gaps.
fn validate_byte_extent(
    record: &Record,
    custom: &BTreeMap<String, u64>,
    at: &str,
    errors: &mut Vec<String>,
) {
    let mut spans: Vec<(u64, u64, String)> = Vec::new();
    for field in &record.fields {
        let (Some(offset), Ok(Some(width))) = (field.offset, type_width(&field.ty, custom)) else {
            return; // Already reported above; extent arithmetic is meaningless.
        };
        spans.push((offset, width, format!("field `{}`", field.name)));
    }
    for (index, gap) in record.gaps.iter().enumerate() {
        if gap.size == 0 {
            errors.push(format!("{at}: gap {index} has zero size"));
        }
        if gap.note.trim().is_empty() {
            errors.push(format!("{at}: gap at {} has an empty note", gap.offset));
        }
        spans.push((gap.offset, gap.size, format!("gap@{}", gap.offset)));
    }
    spans.sort_by_key(|(offset, _, _)| *offset);

    let mut cursor = 0u64;
    let mut holes = Vec::new();
    let mut overlaps = Vec::new();
    for (offset, width, label) in &spans {
        if *offset < cursor {
            overlaps.push(format!(
                "{label} at {offset} overlaps the preceding span ending at {cursor}"
            ));
        } else if *offset > cursor {
            holes.push(format!("{cursor}..{offset}"));
        }
        cursor = cursor.max(offset + width);
    }
    if !holes.is_empty() {
        errors.push(format!(
            "{at}: uncovered byte ranges {}; declare each as a [[record.gap]]",
            holes.join(", ")
        ));
    }

    reconcile(record, cursor, &overlaps, at, errors);
}

/// Column records must tile columns `1..=size` with no overlap.
fn validate_columns(record: &Record, at: &str, errors: &mut Vec<String>) {
    let mut spans: Vec<(u64, u64, &str)> = Vec::new();
    for field in &record.fields {
        let Some(text) = field.columns.as_deref() else {
            return;
        };
        let Some((start, end)) = text.split_once('-') else {
            errors.push(format!(
                "{at}, field `{}`: columns must read `start-end`",
                field.name
            ));
            return;
        };
        let (Ok(start), Ok(end)) = (start.trim().parse::<u64>(), end.trim().parse::<u64>()) else {
            errors.push(format!(
                "{at}, field `{}`: unparsable column range {text:?}",
                field.name
            ));
            return;
        };
        if start == 0 || end < start {
            errors.push(format!(
                "{at}, field `{}`: columns are 1-based and ascending, got {text:?}",
                field.name
            ));
            return;
        }
        spans.push((start, end, &field.name));
    }
    spans.sort_by_key(|(start, _, _)| *start);
    let mut cursor = 1u64;
    let mut holes = Vec::new();
    let mut overlaps = Vec::new();
    for (start, end, name) in &spans {
        if *start < cursor {
            overlaps.push(format!(
                "field `{name}`: column {start} overlaps the preceding field"
            ));
        } else if *start > cursor {
            holes.push(format!("{cursor}-{}", start - 1));
        }
        cursor = cursor.max(end + 1);
    }
    if !holes.is_empty() {
        errors.push(format!("{at}: uncovered columns {}", holes.join(", ")));
    }
    reconcile(record, cursor - 1, &overlaps, at, errors);
}

/// Reconcile the arithmetic problems the validator computed against the
/// `[[record.discrepancy]]` blocks the table declares. The two sets must be
/// equal: an undeclared problem fails, and a declared problem that does not
/// exist fails just as hard, so the block cannot be used as an escape hatch.
fn reconcile(
    record: &Record,
    computed: u64,
    overlaps: &[String],
    at: &str,
    errors: &mut Vec<String>,
) {
    let mut found: BTreeSet<DiscrepancyKind> = BTreeSet::new();
    if !overlaps.is_empty() {
        found.insert(DiscrepancyKind::Overlap);
    }
    if record.size.is_some_and(|size| size != computed) {
        found.insert(DiscrepancyKind::SizeMismatch);
    }

    let mut declared: BTreeSet<DiscrepancyKind> = BTreeSet::new();
    for block in &record.discrepancies {
        if !declared.insert(block.kind) {
            errors.push(format!("{at}: duplicate `{:?}` discrepancy", block.kind));
        }
        if block.note.trim().is_empty() {
            errors.push(format!("{at}: `{:?}` has an empty note", block.kind));
        }
    }

    for kind in declared.difference(&found) {
        match kind {
            DiscrepancyKind::SizeMismatch => errors.push(format!(
                "{at}: declares a `size_mismatch` but fields close exactly at {computed}"
            )),
            DiscrepancyKind::Overlap => errors.push(format!(
                "{at}: declares an `overlap` but no two field extents overlap"
            )),
        }
    }
    for kind in found.difference(&declared) {
        match kind {
            DiscrepancyKind::SizeMismatch => errors.push(format!(
                "{at}: fields and gaps end at {computed} but `size` is {}; \
                 fix the table or record a [[record.discrepancy]]",
                record.size.unwrap_or_default()
            )),
            DiscrepancyKind::Overlap => errors.push(format!(
                "{at}: {}; fix the table or record a [[record.discrepancy]]",
                overlaps.join("; ")
            )),
        }
    }

    if let Some(block) = record
        .discrepancies
        .iter()
        .find(|d| d.kind == DiscrepancyKind::SizeMismatch)
    {
        if record.size.is_none() {
            errors.push(format!(
                "{at}: `size_mismatch` needs a declared `size` to compare against"
            ));
        }
        if found.contains(&DiscrepancyKind::SizeMismatch) {
            if block.computed != Some(computed) {
                errors.push(format!(
                    "{at}: `size_mismatch.computed` is {:?} but the fields end at {computed}",
                    block.computed
                ));
            }
            if block.declared != record.size {
                errors.push(format!(
                    "{at}: `size_mismatch.declared` is {:?} but `size` is {:?}",
                    block.declared, record.size
                ));
            }
        }
    }
}

// ---------------------------------------------------------------------------
// Rendering
// ---------------------------------------------------------------------------

/// Render a layout file as the markdown page the specification links to.
fn render(file: &LayoutFile) -> String {
    let mut out = String::new();
    let _ = writeln!(
        out,
        "<!-- Generated from docs/layouts/{}.toml by",
        file.format
    );
    let _ = writeln!(
        out,
        "     crates/cadmpeg/tests/layout_tables.rs. Do not edit by hand;"
    );
    let _ = writeln!(
        out,
        "     run `UPDATE_LAYOUT_DOCS=1 cargo test -p cadmpeg --test layout_tables`. -->"
    );
    let _ = writeln!(out);
    let _ = writeln!(out, "# `{}` record layouts", file.format);
    let _ = writeln!(out);
    let _ = writeln!(out, "Source of truth: [`{0}`](../../{0}).", file.spec);
    let _ = writeln!(out, "Table source: `docs/layouts/{}.toml`.", file.format);
    if !file.note.trim().is_empty() {
        let _ = writeln!(out);
        let _ = writeln!(out, "{}", file.note.trim());
    }

    if !file.types.is_empty() {
        let _ = writeln!(out, "\n## Composite types\n");
        let _ = writeln!(out, "| Type | Bytes | Endianness | Meaning |");
        let _ = writeln!(out, "| ---- | ----: | ---------- | ------- |");
        for decl in &file.types {
            let _ = writeln!(
                out,
                "| `{}` | {} | {} | {} |",
                decl.name,
                decl.bytes,
                decl.endianness,
                cell(&decl.note)
            );
        }
    }

    if !file.tokens.is_empty() {
        let _ = writeln!(out, "\n## Tag inventory\n");
        let _ = writeln!(out, "| Tag | Name | Payload | Meaning | Spec |");
        let _ = writeln!(out, "| --- | ---- | ------: | ------- | ---- |");
        for token in &file.tokens {
            let payload = token
                .payload_bytes
                .map_or_else(|| "variable".to_string(), |n| format!("{n} B"));
            let _ = writeln!(
                out,
                "| `{}` | {} | {} | {} | §{} |",
                token.tag,
                cell(&token.name),
                payload,
                cell(&token.note),
                token.section
            );
        }
    }

    for record in &file.records {
        let _ = writeln!(out, "\n## `{}`\n", record.name);
        let kind = match record.kind {
            RecordKind::Byte => "byte offsets",
            RecordKind::Slot => "ordered slots (no stated byte offsets)",
            RecordKind::Column => "1-based character columns",
        };
        let size = record
            .size
            .map_or_else(|| "not stated".to_string(), |s| format!("{s} B"));
        let _ = writeln!(
            out,
            "Spec §{} · layout: {kind} · size: {size}",
            record.section
        );
        if !record.note.trim().is_empty() {
            let _ = writeln!(out, "\n{}", record.note.trim());
        }
        if !record.parsed_by.is_empty() {
            let _ = writeln!(out, "\nParsed by:");
            for path in &record.parsed_by {
                let _ = writeln!(out, "- `{path}`");
            }
        }
        let _ = writeln!(out);
        match record.kind {
            RecordKind::Byte => {
                let _ = writeln!(
                    out,
                    "| Offset | Size | Field | Type | Endian | Src | Meaning |"
                );
                let _ = writeln!(
                    out,
                    "| -----: | ---: | ----- | ---- | ------ | --- | ------- |"
                );
            }
            RecordKind::Slot => {
                let _ = writeln!(out, "| # | Slot | Type | Endian | Src | Meaning |");
                let _ = writeln!(out, "| -: | ---- | ---- | ------ | --- | ------- |");
            }
            RecordKind::Column => {
                let _ = writeln!(out, "| Columns | Field | Type | Src | Meaning |");
                let _ = writeln!(out, "| ------- | ----- | ---- | --- | ------- |");
            }
        }
        let custom: BTreeMap<String, u64> = file
            .types
            .iter()
            .map(|t| (t.name.clone(), t.bytes))
            .collect();
        let mut rows: Vec<(u64, String)> = Vec::new();
        for (index, field) in record.fields.iter().enumerate() {
            let endian = field
                .endianness
                .as_deref()
                .or(record.endianness.as_deref())
                .or(file.endianness.as_deref())
                .unwrap_or("—");
            let src = match field.source {
                Source::Spec => "spec",
                Source::Derived => "derived",
                Source::Code => "code",
            };
            let mut meaning = if field.note.trim().is_empty() {
                cell(&field.anchor)
            } else {
                cell(&field.note)
            };
            if let Some(raw) = &field.value {
                let _ = write!(meaning, " · value `{raw}`");
            }
            match record.kind {
                RecordKind::Byte => {
                    let offset = field.offset.unwrap_or(0);
                    let width = type_width(&field.ty, &custom)
                        .ok()
                        .flatten()
                        .map_or_else(|| "?".to_string(), |w| w.to_string());
                    rows.push((
                        offset,
                        format!(
                            "| {offset} | {width} | `{}` | `{}` | {endian} | {src} | {meaning} |",
                            field.name, field.ty
                        ),
                    ));
                }
                RecordKind::Slot => {
                    rows.push((
                        index as u64,
                        format!(
                            "| {index} | `{}` | `{}` | {endian} | {src} | {meaning} |",
                            field.name, field.ty
                        ),
                    ));
                }
                RecordKind::Column => {
                    let columns = field.columns.clone().unwrap_or_default();
                    let start = columns
                        .split_once('-')
                        .and_then(|(a, _)| a.trim().parse().ok())
                        .unwrap_or(0);
                    rows.push((
                        start,
                        format!(
                            "| {columns} | `{}` | `{}` | {src} | {meaning} |",
                            field.name, field.ty
                        ),
                    ));
                }
            }
        }
        if record.kind != RecordKind::Slot {
            rows.sort_by_key(|(key, _)| *key);
        }
        for (_, row) in &rows {
            let _ = writeln!(out, "{row}");
        }
        if !record.gaps.is_empty() {
            let _ = writeln!(out, "\nUnstated regions:\n");
            for gap in &record.gaps {
                let _ = writeln!(
                    out,
                    "- `{}..{}` ({} B): {}",
                    gap.offset,
                    gap.offset + gap.size,
                    gap.size,
                    gap.note.trim()
                );
            }
        }
        if !record.discrepancies.is_empty() {
            let _ = writeln!(out, "\n**Discrepancies:**\n");
            for item in &record.discrepancies {
                let _ = writeln!(out, "- {}", item.note.trim());
            }
        }
        if !record.code.is_empty() {
            let _ = writeln!(out, "\nCross-checked against code:\n");
            for check in &record.code {
                let _ = writeln!(out, "- `{}` — {}", check.path, check.note.trim());
            }
        }
    }

    if !file.not_applicable.is_empty() {
        let _ = writeln!(out, "\n## Not tabulated\n");
        let _ = writeln!(out, "| Area | Spec | Reason |");
        let _ = writeln!(out, "| ---- | ---- | ------ |");
        for entry in &file.not_applicable {
            let _ = writeln!(
                out,
                "| {} | §{} | {} |",
                cell(&entry.area),
                entry.section,
                cell(&entry.reason)
            );
        }
    }

    out
}

/// Escape a value for a markdown table cell.
fn cell(text: &str) -> String {
    normalize(text).replace('|', "\\|")
}

// ---------------------------------------------------------------------------
// Generated layout constants
// ---------------------------------------------------------------------------

/// Table stem → generated Rust path, repo-relative. `step` is absent because
/// it has no byte-layout records.
const GENERATED_LAYOUT_RS: &[(&str, &str)] = &[
    ("asm", "crates/cadmpeg-asm/src/layout.rs"),
    ("catia", "crates/cadmpeg-codec-catia/src/layout.rs"),
    ("creo", "crates/cadmpeg-codec-creo/src/layout.rs"),
    ("f3d", "crates/cadmpeg-codec-f3d/src/layout.rs"),
    ("freecad", "crates/cadmpeg-codec-freecad/src/layout.rs"),
    ("iges", "crates/cadmpeg-codec-iges/src/layout.rs"),
    ("inventor", "crates/cadmpeg-codec-inventor/src/layout.rs"),
    ("nx", "crates/cadmpeg-codec-nx/src/layout.rs"),
    ("protein", "crates/cadmpeg-protein/src/layout.rs"),
    ("rhino", "crates/cadmpeg-codec-rhino/src/layout.rs"),
    ("sldprt", "crates/cadmpeg-codec-sldprt/src/layout.rs"),
];

const RUST_KEYWORDS: &[&str] = &[
    "Self", "abstract", "as", "async", "await", "become", "box", "break", "const", "continue",
    "crate", "do", "dyn", "else", "enum", "extern", "false", "final", "fn", "for", "if", "impl",
    "in", "let", "loop", "macro", "match", "mod", "move", "mut", "override", "priv", "pub", "ref",
    "return", "self", "static", "struct", "super", "trait", "true", "try", "type", "typeof",
    "union", "unsafe", "unsized", "use", "virtual", "where", "while", "yield",
];

fn is_snake_case(name: &str) -> bool {
    let mut chars = name.chars();
    let Some(first) = chars.next() else {
        return false;
    };
    if !first.is_ascii_lowercase() {
        return false;
    }
    let mut prev_underscore = false;
    for c in chars {
        if c == '_' {
            if prev_underscore {
                return false;
            }
            prev_underscore = true;
            continue;
        }
        if !c.is_ascii_lowercase() && !c.is_ascii_digit() {
            return false;
        }
        prev_underscore = false;
    }
    !prev_underscore
}

fn check_ident(kind: &str, name: &str, at: &str, errors: &mut Vec<String>) {
    if !is_snake_case(name) {
        errors.push(format!("{at}: {kind} `{name}` is not snake_case"));
    }
    if RUST_KEYWORDS.contains(&name) {
        errors.push(format!("{at}: {kind} `{name}` is a Rust keyword"));
    }
}

fn resolved_endian<'a>(
    file: &'a LayoutFile,
    record: &'a Record,
    field: &'a Field,
) -> Option<&'a str> {
    field
        .endianness
        .as_deref()
        .or(record.endianness.as_deref())
        .or(file.endianness.as_deref())
}

fn endian_phrase(endian: Option<&str>) -> Option<&'static str> {
    match endian {
        Some("little") => Some("little-endian"),
        Some("big") => Some("big-endian"),
        Some("unstated") => Some("endianness unstated"),
        _ => None,
    }
}

fn fence_text(text: &str) -> String {
    normalize(text).replace("```", "'''")
}

/// Turn one validated table into the checked-in `layout.rs` source.
fn emit_layout_rs(file: &LayoutFile) -> Result<String, Vec<String>> {
    let mut errors = Vec::new();
    let mut omitted = Vec::new();
    let mut modules = String::new();

    for record in &file.records {
        let at = format!("{}: record `{}`", file.format, record.name);
        if record.kind != RecordKind::Byte {
            continue;
        }
        if !record.discrepancies.is_empty() {
            let kinds: Vec<&str> = record
                .discrepancies
                .iter()
                .map(|d| match d.kind {
                    DiscrepancyKind::SizeMismatch => "size_mismatch",
                    DiscrepancyKind::Overlap => "overlap",
                })
                .collect();
            let note = record
                .discrepancies
                .first()
                .map(|d| fence_text(&d.note))
                .unwrap_or_default();
            omitted.push(format!(
                "// - `{}` ({}): {note}",
                record.name,
                kinds.join(", ")
            ));
            continue;
        }

        check_ident("record", &record.name, &at, &mut errors);
        let mut seen = BTreeSet::new();
        let mut fields_out = String::new();
        for field in &record.fields {
            let at = format!("{at}, field `{}`", field.name);
            if field.name == "len" {
                errors.push(format!(
                    "{at}: field name `len` collides with the record length constant `LEN`"
                ));
            }
            if !is_snake_case(&field.name) {
                errors.push(format!("{at}: field `{}` is not snake_case", field.name));
            }
            let const_name = field.name.to_ascii_uppercase();
            if !seen.insert(const_name.clone()) {
                errors.push(format!("{at}: constant `{const_name}` already emitted"));
            }
            let Some(offset) = field.offset else {
                errors.push(format!("{at}: byte field has no offset"));
                continue;
            };
            let ty_part = if is_multibyte_scalar(&field.ty) {
                match endian_phrase(resolved_endian(file, record, field)) {
                    Some(endian) => format!("`{}`, {endian}", field.ty),
                    None => format!("`{}`", field.ty),
                }
            } else {
                format!("`{}`", field.ty)
            };
            let _ = writeln!(
                fields_out,
                "    /// Offset of `{0}` ({ty_part}). Spec §{1}.",
                field.name, record.section
            );
            let _ = writeln!(
                fields_out,
                "    pub(crate) const {const_name}: usize = {offset};"
            );
            if let Some(raw) = &field.value {
                let value_name = format!("{const_name}_VALUE");
                if !seen.insert(value_name.clone()) {
                    errors.push(format!("{at}: constant `{value_name}` already emitted"));
                }
                let custom: BTreeMap<String, u64> = file
                    .types
                    .iter()
                    .map(|t| (t.name.clone(), t.bytes))
                    .collect();
                let width = type_width(&field.ty, &custom).ok().flatten();
                match decode_field_value(raw, &field.ty, width) {
                    Ok(binding) => {
                        let _ = writeln!(
                            fields_out,
                            "    /// Stated value of `{0}` (`{1}`). Spec §{2}.",
                            field.name, field.ty, record.section
                        );
                        let _ =
                            writeln!(fields_out, "    pub(crate) const {value_name}: {binding};");
                    }
                    Err(message) => errors.push(format!("{at}: {message}")),
                }
            }
        }

        let _ = writeln!(
            modules,
            "/// Byte offsets for the `{}` record.",
            record.name
        );
        let _ = writeln!(modules, "///");
        match record.size {
            Some(size) => {
                let _ = writeln!(
                    modules,
                    "/// Spec §{}. Record length {size} B.",
                    record.section
                );
            }
            None => {
                let _ = writeln!(modules, "/// Spec §{}.", record.section);
            }
        }
        if !record.note.trim().is_empty() {
            let _ = writeln!(modules, "///");
            let _ = writeln!(modules, "/// ```text");
            let _ = writeln!(modules, "/// {}", fence_text(&record.note));
            let _ = writeln!(modules, "/// ```");
        }
        let _ = writeln!(modules, "pub(crate) mod {} {{", record.name);
        if let Some(size) = record.size {
            let _ = writeln!(
                modules,
                "    /// Record length in bytes. Spec §{}.",
                record.section
            );
            let _ = writeln!(modules, "    pub(crate) const LEN: usize = {size};");
        }
        modules.push_str(&fields_out);
        let _ = writeln!(modules, "}}");
        let _ = writeln!(modules);
    }

    if !errors.is_empty() {
        return Err(errors);
    }

    let mut token_mod = String::new();
    let mut token_consts: Vec<(String, TokenConst, &TokenDecl)> = Vec::new();
    for token in &file.tokens {
        let Some(value) = parse_token_tag(&token.tag) else {
            continue;
        };
        let Some(name) = token_const_name(&token.name) else {
            continue;
        };
        token_consts.push((name, value, token));
    }
    if !token_consts.is_empty() {
        let _ = writeln!(token_mod, "/// Tag constants from the table inventory.");
        let _ = writeln!(token_mod, "pub(crate) mod token {{");
        for (name, value, token) in &token_consts {
            let _ = writeln!(
                token_mod,
                "    /// `{}` (`{}`). Spec §{}.",
                token.name, token.tag, token.section
            );
            match value {
                TokenConst::Bytes(bytes) => {
                    let _ = writeln!(
                        token_mod,
                        "    pub(crate) const {name}: [u8; {}] = {};",
                        bytes.len(),
                        rust_byte_array(bytes)
                    );
                }
                other => {
                    let _ = writeln!(
                        token_mod,
                        "    pub(crate) const {name}: {} = {};",
                        rust_token_ty(other),
                        match other {
                            TokenConst::U8(v) => format!("{v}"),
                            TokenConst::U16(v) => rust_hex(u64::from(*v), 4),
                            TokenConst::U32(v) => rust_hex(u64::from(*v), 8),
                            TokenConst::U64(v) => rust_hex(*v, 16),
                            TokenConst::Bytes(_) => unreachable!(),
                        }
                    );
                }
            }
        }
        let _ = writeln!(token_mod, "}}");
        let _ = writeln!(token_mod);
    }

    let mut out = String::new();
    let _ = writeln!(out, "// SPDX-License-Identifier: Apache-2.0");
    let _ = writeln!(
        out,
        "//! Byte-offset and value constants generated from `docs/layouts/{}.toml`.",
        file.format
    );
    let _ = writeln!(out, "//!");
    let _ = writeln!(out, "//! Do not edit by hand. Regenerate with:");
    let _ = writeln!(
        out,
        "//! `UPDATE_LAYOUT_CODE=1 cargo test -p cadmpeg --test layout_tables`."
    );
    let _ = writeln!(out);
    let _ = writeln!(
        out,
        "#![allow(dead_code)] // Not every generated constant is referenced yet."
    );
    let _ = writeln!(out);
    if !omitted.is_empty() {
        let _ = writeln!(
            out,
            "// Records omitted because the table declares a contradiction."
        );
        let _ = writeln!(out, "//");
        for line in &omitted {
            let _ = writeln!(out, "{line}");
        }
        let _ = writeln!(out);
    }
    out.push_str(&token_mod);
    out.push_str(modules.trim_end());
    if !out.ends_with('\n') {
        out.push('\n');
    }
    Ok(rustfmt_source(&out))
}

/// rustfmt the emitter output so checked-in `layout.rs` files stay equal to
/// `cargo fmt` (long byte arrays wrap) and to `UPDATE_LAYOUT_CODE=1`.
fn rustfmt_source(src: &str) -> String {
    let mut child = Command::new("rustfmt")
        .args(["--emit", "stdout", "--quiet", "--edition", "2021"])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("spawn rustfmt");
    child
        .stdin
        .as_mut()
        .expect("rustfmt stdin")
        .write_all(src.as_bytes())
        .expect("write rustfmt stdin");
    let output = child.wait_with_output().expect("wait rustfmt");
    assert!(
        output.status.success(),
        "rustfmt rejected generated layout.rs:\n{}",
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8(output.stdout).expect("rustfmt stdout utf-8")
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

fn repo_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .ancestors()
        .nth(2)
        .unwrap()
        .to_path_buf()
}

fn layout_files(dir: &Path) -> Vec<PathBuf> {
    let mut paths: Vec<PathBuf> = std::fs::read_dir(dir)
        .unwrap()
        .map(|entry| entry.unwrap().path())
        .filter(|path| path.extension().is_some_and(|e| e == "toml"))
        .collect();
    paths.sort();
    paths
}

fn parse(path: &Path) -> LayoutFile {
    let text = read_text(path).unwrap();
    toml::from_str(&text).unwrap_or_else(|e| panic!("{}: {e}", path.display()))
}

#[test]
fn ascii_token_tags_keep_trailing_spaces() {
    match parse_token_tag("FINJPL  ") {
        Some(TokenConst::Bytes(bytes)) => assert_eq!(bytes, b"FINJPL  "),
        other => panic!("expected eight-byte ASCII tag, got {other:?}"),
    }
}

#[test]
fn every_format_has_a_layout_table() {
    let root = repo_root();
    let found: BTreeSet<String> = layout_files(&root.join("docs/layouts"))
        .iter()
        .map(|path| path.file_stem().unwrap().to_string_lossy().to_string())
        .collect();
    let expected: BTreeSet<String> = EXPECTED_FORMATS.iter().map(|s| (*s).to_string()).collect();
    assert_eq!(
        found, expected,
        "docs/layouts/*.toml must cover exactly the workspace formats"
    );
}

#[test]
fn layout_tables_are_internally_consistent() {
    let root = repo_root();
    let mut ctx = Context::new(root.clone());
    let mut failures = Vec::new();
    for path in layout_files(&root.join("docs/layouts")) {
        let file = parse(&path);
        assert_eq!(
            file.format,
            path.file_stem().unwrap().to_string_lossy(),
            "{}: `format` must match the file stem",
            path.display()
        );
        failures.extend(validate(&mut ctx, &path, &file));
    }
    assert!(
        failures.is_empty(),
        "layout table validation failed:\n  {}",
        failures.join("\n  ")
    );
}

#[test]
fn layout_validator_rejects_broken_tables() {
    let cases: &[(&str, &str)] = &[
        ("overlapping-fields.toml", "overlaps the preceding span"),
        ("uncovered-gap.toml", "uncovered byte ranges"),
        ("size-mismatch-undeclared.toml", "fix the table or record a"),
        ("size-mismatch-wrong-numbers.toml", "but the fields end at"),
        ("phantom-discrepancy.toml", "declares a `size_mismatch` but"),
        ("unknown-type.toml", "unknown type"),
        ("missing-endianness.toml", "resolves no endianness"),
        ("fabricated-anchor.toml", "anchor not found"),
        ("missing-offset.toml", "byte record field needs `offset`"),
        ("bad-section.toml", "has no section"),
        ("variable-in-byte-record.toml", "variable-length type"),
        ("column-hole.toml", "uncovered columns"),
        ("failed-code-check.toml", "code check failed"),
        ("slot-with-offset.toml", "must not state `offset`"),
        ("value-width-mismatch.toml", "value is 2 bytes"),
        ("parsed-by-missing.toml", "`parsed_by` path does not exist"),
        ("code-check-in-tests.toml", "names a test file"),
    ];
    let root = repo_root();
    let dir = root.join("crates/cadmpeg/tests/fixtures/layout-invalid");
    let mut ctx = Context::new(root.clone());

    let present: BTreeSet<String> = layout_files(&dir)
        .iter()
        .map(|p| p.file_name().unwrap().to_string_lossy().to_string())
        .collect();
    let expected: BTreeSet<String> = cases.iter().map(|(name, _)| (*name).to_string()).collect();
    assert_eq!(present, expected, "invalid-fixture directory drifted");

    for (name, expected_error) in cases {
        let path = dir.join(name);
        let file = parse(&path);
        let errors = validate(&mut ctx, &path, &file);
        assert!(
            errors.iter().any(|e| e.contains(expected_error)),
            "{name}: expected an error containing {expected_error:?}, got:\n  {}",
            if errors.is_empty() {
                "<no errors at all — the validator is vacuous for this rule>".to_string()
            } else {
                errors.join("\n  ")
            }
        );
    }
}

#[test]
fn rendered_layout_pages_match_the_tables() {
    let root = repo_root();
    let update = std::env::var_os("UPDATE_LAYOUT_DOCS").is_some();
    let mut stale = Vec::new();
    for path in layout_files(&root.join("docs/layouts")) {
        let file = parse(&path);
        let rendered = render(&file);
        let target = path.with_extension("md");
        if update {
            std::fs::write(&target, &rendered).unwrap();
            continue;
        }
        let current = read_text(&target).unwrap_or_default();
        if current != rendered {
            stale.push(
                target
                    .file_name()
                    .unwrap_or(target.as_os_str())
                    .to_string_lossy()
                    .to_string(),
            );
        }
    }
    assert!(
        stale.is_empty(),
        "rendered layout pages are stale: {}\n\
         regenerate with `UPDATE_LAYOUT_DOCS=1 cargo test -p cadmpeg --test layout_tables`",
        stale.join(", ")
    );
}

#[test]
fn every_byte_layout_table_has_a_generated_file() {
    let root = repo_root();
    let mapped: BTreeSet<&str> = GENERATED_LAYOUT_RS
        .iter()
        .map(|(format, _)| *format)
        .collect();
    for path in layout_files(&root.join("docs/layouts")) {
        let file = parse(&path);
        let has_byte = file
            .records
            .iter()
            .any(|record| record.kind == RecordKind::Byte);
        if has_byte {
            assert!(
                mapped.contains(file.format.as_str()),
                "{} has a byte-layout record but is missing from GENERATED_LAYOUT_RS",
                file.format
            );
        } else {
            assert!(
                !mapped.contains(file.format.as_str()),
                "{} has no byte-layout record and must not appear in GENERATED_LAYOUT_RS",
                file.format
            );
        }
    }
}

#[test]
fn generated_layout_code_matches_the_tables() {
    let root = repo_root();
    let update = std::env::var_os("UPDATE_LAYOUT_CODE").is_some();
    let mut stale = Vec::new();
    let mut emit_errors = Vec::new();
    for (format, relative) in GENERATED_LAYOUT_RS {
        let table = root.join("docs/layouts").join(format!("{format}.toml"));
        let file = parse(&table);
        assert_eq!(
            file.format,
            *format,
            "{}: format must match the mapping key",
            table.display()
        );
        let rendered = match emit_layout_rs(&file) {
            Ok(rendered) => rendered,
            Err(errors) => {
                emit_errors.extend(errors);
                continue;
            }
        };
        if *format == "sldprt" {
            assert!(
                rendered.contains("`chart_00_28`"),
                "sldprt layout.rs must list the discrepant chart record as omitted"
            );
            assert!(
                !rendered.contains("mod chart_00_28"),
                "sldprt layout.rs must not emit constants for a discrepant record"
            );
        }
        let target = root.join(relative);
        if update {
            std::fs::write(&target, &rendered).unwrap();
            continue;
        }
        let current = read_text(&target).unwrap_or_default();
        if current != rendered {
            stale.push((*relative).to_string());
        }
    }
    assert!(
        emit_errors.is_empty(),
        "layout constant emitter rejected a table:\n  {}",
        emit_errors.join("\n  ")
    );
    assert!(
        stale.is_empty(),
        "generated layout.rs files are stale: {}\n\
         regenerate with `UPDATE_LAYOUT_CODE=1 cargo test -p cadmpeg --test layout_tables`",
        stale.join(", ")
    );
}
