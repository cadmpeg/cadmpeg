// SPDX-License-Identifier: Apache-2.0
//! Single-record lookup by arena and ID for `cadmpeg query item`.
//!
//! Default output is pretty-printed JSON records (blank-line separated), not
//! TSV: nested records do not fit the other views' table convention.
//! `--fields` projects a closed list of dotted paths as TSV; cell escaping is
//! projection-specific (see [`project_fields`]).

use std::fmt;

use anyhow::{bail, Context, Result};
use cadmpeg_core::decode::alloc_filled;
use clap::Args;
use serde::de::{DeserializeSeed, IgnoredAny, MapAccess, SeqAccess, Visitor};
use serde::{Deserialize, Deserializer};
use serde_json::value::RawValue;

use super::{detect, print_json, read_input, Artifact};

/// Input selection for `query item`.
#[derive(Debug, Args)]
pub struct ItemArgs {
    /// JSON file, or `-` for standard input.
    pub file: std::path::PathBuf,
    /// Arena address: `model.<arena>`, `native.<codec>.<arena>`, or bare
    /// `<arena>` as shorthand for `model.<arena>`. Same dotted names as
    /// `query counts --json`.
    pub arena: String,
    /// Record IDs (exact or unique suffix). Omit for the first record;
    /// conflicts with `--head`.
    pub ids: Vec<String>,
    /// Print the first N records in arena order. Conflicts with explicit IDs.
    #[arg(long, value_name = "N", conflicts_with = "ids")]
    pub head: Option<usize>,
    /// Comma-separated dotted field paths; project as TSV (no expressions).
    /// Conflicts with `--json`.
    #[arg(long, value_delimiter = ',', conflicts_with = "json")]
    pub fields: Option<Vec<String>>,
    /// Wrap matched records in the versioned JSON envelope.
    #[arg(long)]
    pub json: bool,
}

/// Where the requested arena lives in the document.
#[derive(Debug, Clone)]
pub(crate) enum ArenaTarget {
    Model { arena: String },
    Native { codec: String, arena: String },
}

impl ArenaTarget {
    pub(crate) fn parse(spec: &str) -> Result<Self> {
        if !spec.contains('.') {
            return Ok(Self::Model {
                arena: spec.to_owned(),
            });
        }
        let parts: Vec<&str> = spec.split('.').collect();
        match parts.as_slice() {
            ["model", arena] if !arena.is_empty() => Ok(Self::Model {
                arena: (*arena).to_owned(),
            }),
            ["native", codec, arena] if !codec.is_empty() && !arena.is_empty() => {
                Ok(Self::Native {
                    codec: (*codec).to_owned(),
                    arena: (*arena).to_owned(),
                })
            }
            _ => bail!(
                "arena name {spec:?} is not addressable; use `model.<arena>`, \
                 `native.<codec>.<arena>`, or a bare `<arena>` shorthand for \
                 `model.<arena>` (same names as `cadmpeg query counts --json`)"
            ),
        }
    }

    pub(crate) fn dotted(&self) -> String {
        match self {
            Self::Model { arena } => format!("model.{arena}"),
            Self::Native { codec, arena } => format!("native.{codec}.{arena}"),
        }
    }

    fn matches_model(&self, arena: &str) -> bool {
        matches!(self, Self::Model { arena: a } if a == arena)
    }

    fn matches_native(&self, codec: &str, arena: &str) -> bool {
        matches!(self, Self::Native { codec: c, arena: a } if c == codec && a == arena)
    }
}

/// How many / which records to retain while streaming the arena.
#[derive(Debug)]
enum KeepMode<'a> {
    /// Keep records that exact- or suffix-match any requested ID.
    Ids(&'a [String]),
    /// Keep the first N records in arena order.
    Head(usize),
}

/// Result of one targeted deserialize of a CADIR document.
struct Capture {
    /// True when the target key was present and its value was a JSON array.
    found_array: bool,
    entry_count: u64,
    /// Kept raw records (ID hits or first-N).
    kept: Vec<Box<RawValue>>,
    /// Every JSON-string `id` observed in the target arena (ID mode).
    all_ids: Vec<String>,
    /// Dotted names of every addressable (JSON-array) arena, with entry counts.
    addressable: Vec<(String, u64)>,
}

/// Tolerant id probe: a non-string `id` becomes `None` instead of failing the
/// element.
#[derive(Deserialize)]
struct IdProbe {
    #[serde(default)]
    id: Option<serde_json::Value>,
}

fn string_id(raw: &RawValue) -> Option<String> {
    let probe: IdProbe = serde_json::from_str(raw.get()).ok()?;
    match probe.id {
        Some(serde_json::Value::String(s)) => Some(s),
        _ => None,
    }
}

/// Runs `query item` against one artifact.
pub fn run(args: &ItemArgs) -> Result<()> {
    let bytes = read_input(&args.file)?;
    let artifact = detect(&bytes, &args.file)?;
    match artifact {
        Artifact::Cadir(_) => {}
        Artifact::Report(_) => bail!(
            "{} is a command report; reports have no arenas. Use \
             `cadmpeg query findings` / `cadmpeg query losses` on the report, or \
             `cadmpeg dump SOURCE -o doc.json && cadmpeg query item doc.json ARENA ID`",
            args.file.display()
        ),
        Artifact::Sidecar(_) => bail!(
            "{} is a decode sidecar (`<stem>.fidelity.json`); sidecars have no \
             arenas. Run `cadmpeg dump SOURCE -o doc.json && cadmpeg query item \
             doc.json ARENA ID`",
            args.file.display()
        ),
    }

    let target = ArenaTarget::parse(&args.arena)?;
    let text = std::str::from_utf8(&bytes)
        .with_context(|| format!("{} is not valid UTF-8", args.file.display()))?;

    let mode = if args.ids.is_empty() {
        KeepMode::Head(args.head.unwrap_or(1))
    } else {
        KeepMode::Ids(&args.ids)
    };

    let capture = CaptureSeed {
        target: &target,
        mode: &mode,
    }
    .deserialize(&mut serde_json::Deserializer::from_str(text))
    .with_context(|| format!("parsing the CADIR document {}", args.file.display()))?;

    if !capture.found_array {
        bail!("{}", unknown_arena_message(&target, &capture.addressable));
    }

    match mode {
        KeepMode::Head(_) => {
            let values = parse_kept(&capture.kept)?;
            emit(args, &values)
        }
        KeepMode::Ids(ids) => {
            let dotted = target.dotted();
            let (values, errors) = resolve_ids(ids, &capture, &dotted)?;
            match (emit(args, &values), errors.is_empty()) {
                (Ok(()), true) => Ok(()),
                (Ok(()), false) => bail!("{}", errors.join("\n")),
                (Err(err), true) => Err(err),
                (Err(err), false) => bail!("{err}\n{}", errors.join("\n")),
            }
        }
    }
}

fn parse_kept(kept: &[Box<RawValue>]) -> Result<Vec<serde_json::Value>> {
    kept.iter()
        .map(|raw| {
            serde_json::from_str(raw.get())
                .with_context(|| "parsing a retained arena record as JSON")
        })
        .collect()
}

fn resolve_ids(
    ids: &[String],
    capture: &Capture,
    dotted: &str,
) -> Result<(Vec<serde_json::Value>, Vec<String>)> {
    let mut indexed: Vec<(Option<String>, &RawValue)> = Vec::with_capacity(capture.kept.len());
    for raw in &capture.kept {
        indexed.push((string_id(raw), raw.as_ref()));
    }

    let mut values = Vec::new();
    let mut errors = Vec::new();

    for request in ids {
        match resolve_one(request, &indexed) {
            Ok(raw) => {
                let value: serde_json::Value = serde_json::from_str(raw.get())
                    .with_context(|| format!("parsing record for id {request:?}"))?;
                values.push(value);
            }
            Err(ResolveError::Ambiguous(matches)) => {
                errors.push(ambiguous_message(request, dotted, &matches));
            }
            Err(ResolveError::Missing) => {
                errors.push(miss_id_message(
                    dotted,
                    request,
                    capture.entry_count,
                    &capture.all_ids,
                ));
            }
        }
    }
    Ok((values, errors))
}

enum ResolveError {
    Missing,
    Ambiguous(Vec<String>),
}

fn resolve_one<'a>(
    request: &str,
    indexed: &[(Option<String>, &'a RawValue)],
) -> Result<&'a RawValue, ResolveError> {
    let mut exact: Option<&RawValue> = None;
    for (id, raw) in indexed {
        if id.as_deref() == Some(request) {
            exact = Some(*raw);
            break;
        }
    }
    if let Some(raw) = exact {
        return Ok(raw);
    }

    let mut suffix: Vec<(String, &RawValue)> = Vec::new();
    for (id, raw) in indexed {
        if let Some(id) = id {
            if id.ends_with(request) {
                suffix.push((id.clone(), *raw));
            }
        }
    }
    match suffix.len() {
        0 => Err(ResolveError::Missing),
        1 => Ok(suffix[0].1),
        _ => Err(ResolveError::Ambiguous(
            suffix.into_iter().map(|(id, _)| id).collect(),
        )),
    }
}

fn emit(args: &ItemArgs, values: &[serde_json::Value]) -> Result<()> {
    emit_values("item", args.json, args.fields.as_deref(), values)
}

/// Pretty-print records, TSV `--fields`, or the versioned `--json` envelope.
pub(crate) fn emit_values(
    view: &str,
    json: bool,
    fields: Option<&[String]>,
    values: &[serde_json::Value],
) -> Result<()> {
    if json {
        print_json(view, &serde_json::Value::Array(values.to_vec()));
        return Ok(());
    }
    // Empty arena / no matches: empty stdout (no TSV header), exit 0 unless a
    // caller layers ID or --fields teaching errors on top.
    if values.is_empty() {
        return Ok(());
    }
    if let Some(paths) = fields {
        let (tsv, empty_paths) = project_fields(values, paths)?;
        print!("{tsv}");
        if !empty_paths.is_empty() {
            bail!("{}", empty_fields_message(values, &empty_paths));
        }
        return Ok(());
    }
    for (i, value) in values.iter().enumerate() {
        if i > 0 {
            println!();
        }
        println!(
            "{}",
            serde_json::to_string_pretty(value).context("serializing a record")?
        );
    }
    Ok(())
}

pub(crate) fn unknown_arena_message(target: &ArenaTarget, addressable: &[(String, u64)]) -> String {
    let list = if addressable.is_empty() {
        "(none — this document has no array arenas)".to_owned()
    } else {
        addressable
            .iter()
            .map(|(name, _)| name.as_str())
            .collect::<Vec<_>>()
            .join(", ")
    };
    format!(
        "unknown arena {}; addressable arenas in this document: {}; run \
         `cadmpeg query counts FILE` for entries per arena",
        target.dotted(),
        list
    )
}

pub(crate) fn miss_id_message(
    arena: &str,
    request: &str,
    entry_count: u64,
    all_ids: &[String],
) -> String {
    const SHOWN: usize = 10;
    let lower = request.to_lowercase();
    let mut label = "close ids";
    let mut names: Vec<&str> = all_ids
        .iter()
        .filter(|id| id.to_lowercase().contains(&lower))
        .take(SHOWN)
        .map(String::as_str)
        .collect();
    if names.is_empty() {
        label = "ids include";
        names = all_ids.iter().take(SHOWN).map(String::as_str).collect();
    }
    if names.is_empty() {
        format!(
            "no record in {arena} ({entry_count} entries) has string id {request:?}; \
             none of the entries carry a JSON-string `id` field"
        )
    } else {
        format!(
            "no record in {arena} ({entry_count} entries) has id {request:?}; {label}: {}; \
             pass a longer suffix or the full id",
            names.join(", ")
        )
    }
}

pub(crate) fn ambiguous_message(request: &str, arena: &str, matches: &[String]) -> String {
    const SHOWN: usize = 10;
    let shown: Vec<&str> = matches.iter().take(SHOWN).map(String::as_str).collect();
    format!(
        "ambiguous id suffix {request:?} in {arena}; matches: {}; pass a longer \
         suffix or the full id",
        shown.join(", ")
    )
}

pub(crate) fn empty_fields_message(values: &[serde_json::Value], empty_paths: &[String]) -> String {
    let mut parts = Vec::new();
    for path in empty_paths {
        let keys = first_parent_keys(values, path);
        let key_note = match keys {
            Some(keys) if !keys.is_empty() => format!("; observed fields: {}", keys.join(", ")),
            Some(_) => "; parent object has no fields".to_owned(),
            None => "; parent path is absent or not an object in every row".to_owned(),
        };
        parts.push(format!(
            "field path {path:?} was empty in every projected row{key_note}"
        ));
    }
    parts.push(
        "list fields with `cadmpeg query schema FILE ARENA` (native records) or \
         `cadmpeg query schema model.<arena>` (IR types)"
            .to_owned(),
    );
    parts.join("\n")
}

fn first_parent_keys(values: &[serde_json::Value], path: &str) -> Option<Vec<String>> {
    let parent = match path.rsplit_once('.') {
        Some((parent, _)) => parent,
        None => "",
    };
    for value in values {
        let node = if parent.is_empty() {
            Some(value)
        } else {
            navigate(value, parent)
        };
        if let Some(serde_json::Value::Object(map)) = node {
            return Some(map.keys().cloned().collect());
        }
    }
    None
}

/// Projects records to TSV. Returns the full TSV text (including header) and
/// every path that was empty in all rows.
///
/// Cell rules (projection-specific; not the other views' [`super::cell`]):
/// JSON strings/numbers/bools are bare; tab/newline in a string become `\t`/
/// `\n`; null or absent is an empty cell; arrays/objects are compact JSON.
pub(crate) fn project_fields(
    values: &[serde_json::Value],
    paths: &[String],
) -> Result<(String, Vec<String>)> {
    if paths.is_empty() {
        bail!("--fields requires at least one dotted path");
    }
    let mut out = String::new();
    out.push_str(&paths.join("\t"));
    out.push('\n');

    let mut empty_counts = alloc_filled(paths.len(), 0usize, "cli query field counts")?;
    let row_count = values.len();
    for value in values {
        for (i, path) in paths.iter().enumerate() {
            if i > 0 {
                out.push('\t');
            }
            let cell = match navigate(value, path) {
                None | Some(serde_json::Value::Null) => {
                    empty_counts[i] += 1;
                    String::new()
                }
                Some(v) => field_cell(v),
            };
            out.push_str(&cell);
        }
        out.push('\n');
    }

    let empty_paths = if row_count == 0 {
        Vec::new()
    } else {
        paths
            .iter()
            .zip(empty_counts.iter())
            .filter(|&(_, &count)| count == row_count)
            .map(|(path, _)| path.clone())
            .collect()
    };
    Ok((out, empty_paths))
}

pub(crate) fn navigate<'a>(
    value: &'a serde_json::Value,
    path: &str,
) -> Option<&'a serde_json::Value> {
    let mut cur = value;
    for part in path.split('.') {
        cur = cur.as_object()?.get(part)?;
    }
    Some(cur)
}

pub(crate) fn field_cell(value: &serde_json::Value) -> String {
    match value {
        serde_json::Value::Null => String::new(),
        serde_json::Value::Bool(b) => b.to_string(),
        serde_json::Value::Number(n) => n.to_string(),
        serde_json::Value::String(s) => s.replace('\t', "\\t").replace('\n', "\\n"),
        serde_json::Value::Array(_) | serde_json::Value::Object(_) => {
            serde_json::to_string(value).unwrap_or_default()
        }
    }
}

// --- DeserializeSeed capture ------------------------------------------------

struct CaptureSeed<'a> {
    target: &'a ArenaTarget,
    mode: &'a KeepMode<'a>,
}

impl<'de> DeserializeSeed<'de> for CaptureSeed<'_> {
    type Value = Capture;

    fn deserialize<D: Deserializer<'de>>(self, deserializer: D) -> Result<Capture, D::Error> {
        deserializer.deserialize_map(DocumentVisitor {
            target: self.target,
            mode: self.mode,
        })
    }
}

struct DocumentVisitor<'a> {
    target: &'a ArenaTarget,
    mode: &'a KeepMode<'a>,
}

impl<'de> Visitor<'de> for DocumentVisitor<'_> {
    type Value = Capture;

    fn expecting(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("a CADIR document object")
    }

    fn visit_map<A: MapAccess<'de>>(self, mut map: A) -> Result<Capture, A::Error> {
        let mut capture = Capture {
            found_array: false,
            entry_count: 0,
            kept: Vec::new(),
            all_ids: Vec::new(),
            addressable: Vec::new(),
        };
        while let Some(key) = map.next_key::<String>()? {
            match key.as_str() {
                "model" => {
                    map.next_value_seed(ModelSeed {
                        target: self.target,
                        mode: self.mode,
                        capture: &mut capture,
                    })?;
                }
                "native" => {
                    map.next_value_seed(NativeRootSeed {
                        target: self.target,
                        mode: self.mode,
                        capture: &mut capture,
                    })?;
                }
                _ => {
                    let _ = map.next_value::<IgnoredAny>()?;
                }
            }
        }
        Ok(capture)
    }
}

struct ModelSeed<'a> {
    target: &'a ArenaTarget,
    mode: &'a KeepMode<'a>,
    capture: &'a mut Capture,
}

impl<'de> DeserializeSeed<'de> for ModelSeed<'_> {
    type Value = ();

    fn deserialize<D: Deserializer<'de>>(self, deserializer: D) -> Result<(), D::Error> {
        deserializer.deserialize_map(ArenasVisitor {
            namespace: ArenaNamespace::Model,
            codec: None,
            target: self.target,
            mode: self.mode,
            capture: self.capture,
        })
    }
}

struct NativeRootSeed<'a> {
    target: &'a ArenaTarget,
    mode: &'a KeepMode<'a>,
    capture: &'a mut Capture,
}

impl<'de> DeserializeSeed<'de> for NativeRootSeed<'_> {
    type Value = ();

    fn deserialize<D: Deserializer<'de>>(self, deserializer: D) -> Result<(), D::Error> {
        deserializer.deserialize_map(NativeRootVisitor {
            target: self.target,
            mode: self.mode,
            capture: self.capture,
        })
    }
}

struct NativeRootVisitor<'a> {
    target: &'a ArenaTarget,
    mode: &'a KeepMode<'a>,
    capture: &'a mut Capture,
}

impl<'de> Visitor<'de> for NativeRootVisitor<'_> {
    type Value = ();

    fn expecting(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("a native codec map")
    }

    fn visit_map<A: MapAccess<'de>>(self, mut map: A) -> Result<(), A::Error> {
        while let Some(codec) = map.next_key::<String>()? {
            map.next_value_seed(NativeCodecSeed {
                codec: &codec,
                target: self.target,
                mode: self.mode,
                capture: self.capture,
            })?;
        }
        Ok(())
    }
}

struct NativeCodecSeed<'a> {
    codec: &'a str,
    target: &'a ArenaTarget,
    mode: &'a KeepMode<'a>,
    capture: &'a mut Capture,
}

impl<'de> DeserializeSeed<'de> for NativeCodecSeed<'_> {
    type Value = ();

    fn deserialize<D: Deserializer<'de>>(self, deserializer: D) -> Result<(), D::Error> {
        deserializer.deserialize_map(NativeCodecVisitor {
            codec: self.codec,
            target: self.target,
            mode: self.mode,
            capture: self.capture,
        })
    }
}

struct NativeCodecVisitor<'a> {
    codec: &'a str,
    target: &'a ArenaTarget,
    mode: &'a KeepMode<'a>,
    capture: &'a mut Capture,
}

impl<'de> Visitor<'de> for NativeCodecVisitor<'_> {
    type Value = ();

    fn expecting(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("a native codec object with an arenas map")
    }

    fn visit_map<A: MapAccess<'de>>(self, mut map: A) -> Result<(), A::Error> {
        while let Some(key) = map.next_key::<String>()? {
            if key == "arenas" {
                map.next_value_seed(ArenasSeed {
                    namespace: ArenaNamespace::Native,
                    codec: Some(self.codec),
                    target: self.target,
                    mode: self.mode,
                    capture: self.capture,
                })?;
            } else {
                let _ = map.next_value::<IgnoredAny>()?;
            }
        }
        Ok(())
    }
}

#[derive(Clone, Copy)]
enum ArenaNamespace {
    Model,
    Native,
}

struct ArenasSeed<'a> {
    namespace: ArenaNamespace,
    codec: Option<&'a str>,
    target: &'a ArenaTarget,
    mode: &'a KeepMode<'a>,
    capture: &'a mut Capture,
}

impl<'de> DeserializeSeed<'de> for ArenasSeed<'_> {
    type Value = ();

    fn deserialize<D: Deserializer<'de>>(self, deserializer: D) -> Result<(), D::Error> {
        deserializer.deserialize_map(ArenasVisitor {
            namespace: self.namespace,
            codec: self.codec,
            target: self.target,
            mode: self.mode,
            capture: self.capture,
        })
    }
}

struct ArenasVisitor<'a> {
    namespace: ArenaNamespace,
    codec: Option<&'a str>,
    target: &'a ArenaTarget,
    mode: &'a KeepMode<'a>,
    capture: &'a mut Capture,
}

impl<'de> Visitor<'de> for ArenasVisitor<'_> {
    type Value = ();

    fn expecting(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("an arenas object")
    }

    fn visit_map<A: MapAccess<'de>>(self, mut map: A) -> Result<(), A::Error> {
        while let Some(arena) = map.next_key::<String>()? {
            let is_target = match self.namespace {
                ArenaNamespace::Model => self.target.matches_model(&arena),
                ArenaNamespace::Native => self
                    .codec
                    .is_some_and(|codec| self.target.matches_native(codec, &arena)),
            };
            let dotted = match self.namespace {
                ArenaNamespace::Model => format!("model.{arena}"),
                ArenaNamespace::Native => {
                    format!("native.{}.{arena}", self.codec.unwrap_or(""))
                }
            };
            map.next_value_seed(ArenaValueSeed {
                dotted,
                is_target,
                mode: self.mode,
                capture: self.capture,
            })?;
        }
        Ok(())
    }
}

struct ArenaValueSeed<'a> {
    dotted: String,
    is_target: bool,
    mode: &'a KeepMode<'a>,
    capture: &'a mut Capture,
}

impl<'de> DeserializeSeed<'de> for ArenaValueSeed<'_> {
    type Value = ();

    fn deserialize<D: Deserializer<'de>>(self, deserializer: D) -> Result<(), D::Error> {
        deserializer.deserialize_any(ArenaValueVisitor {
            dotted: self.dotted,
            is_target: self.is_target,
            mode: self.mode,
            capture: self.capture,
        })
    }
}

struct ArenaValueVisitor<'a> {
    dotted: String,
    is_target: bool,
    mode: &'a KeepMode<'a>,
    capture: &'a mut Capture,
}

impl<'de> Visitor<'de> for ArenaValueVisitor<'_> {
    type Value = ();

    fn expecting(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("an arena array")
    }

    fn visit_seq<A: SeqAccess<'de>>(self, mut seq: A) -> Result<(), A::Error> {
        if !self.is_target {
            let mut n = 0u64;
            while seq.next_element::<IgnoredAny>()?.is_some() {
                n += 1;
            }
            self.capture.addressable.push((self.dotted, n));
            return Ok(());
        }
        self.capture.found_array = true;
        match self.mode {
            KeepMode::Head(n) => {
                while let Some(raw) = seq.next_element::<Box<RawValue>>()? {
                    self.capture.entry_count += 1;
                    if self.capture.kept.len() < *n {
                        self.capture.kept.push(raw);
                    }
                }
            }
            KeepMode::Ids(ids) => {
                while let Some(raw) = seq.next_element::<Box<RawValue>>()? {
                    self.capture.entry_count += 1;
                    let id = string_id(&raw);
                    if let Some(ref id) = id {
                        self.capture.all_ids.push(id.clone());
                        if ids.iter().any(|req| id == req || id.ends_with(req)) {
                            self.capture.kept.push(raw);
                        }
                    }
                }
            }
        }
        self.capture
            .addressable
            .push((self.dotted, self.capture.entry_count));
        Ok(())
    }

    fn visit_map<A: MapAccess<'de>>(self, mut map: A) -> Result<(), A::Error> {
        while map.next_entry::<IgnoredAny, IgnoredAny>()?.is_some() {}
        Ok(())
    }

    fn visit_unit<E: serde::de::Error>(self) -> Result<(), E> {
        Ok(())
    }

    fn visit_none<E: serde::de::Error>(self) -> Result<(), E> {
        Ok(())
    }

    fn visit_bool<E: serde::de::Error>(self, _: bool) -> Result<(), E> {
        Ok(())
    }

    fn visit_i64<E: serde::de::Error>(self, _: i64) -> Result<(), E> {
        Ok(())
    }

    fn visit_u64<E: serde::de::Error>(self, _: u64) -> Result<(), E> {
        Ok(())
    }

    fn visit_f64<E: serde::de::Error>(self, _: f64) -> Result<(), E> {
        Ok(())
    }

    fn visit_str<E: serde::de::Error>(self, _: &str) -> Result<(), E> {
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn arena_target_parses_shorthand_and_dotted() {
        match ArenaTarget::parse("faces").unwrap() {
            ArenaTarget::Model { arena } => assert_eq!(arena, "faces"),
            ArenaTarget::Native { .. } => panic!("expected model"),
        }
        match ArenaTarget::parse("native.creo.curve_parameters").unwrap() {
            ArenaTarget::Native { codec, arena } => {
                assert_eq!(codec, "creo");
                assert_eq!(arena, "curve_parameters");
            }
            ArenaTarget::Model { .. } => panic!("expected native"),
        }
    }

    #[test]
    fn field_cell_escapes_tab_and_compacts_arrays() {
        assert_eq!(field_cell(&serde_json::json!("a\tb")), "a\\tb".to_owned());
        assert_eq!(field_cell(&serde_json::json!([1, 2])), "[1,2]".to_owned());
    }
}
