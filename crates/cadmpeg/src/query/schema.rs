// SPDX-License-Identifier: Apache-2.0
//! Schema projection for `cadmpeg query schema`.
//!
//! With no FILE this describes the IR types this binary was built with,
//! generated from the `cadmpeg-ir` derives: every field, which are
//! optional, and every variant of a tagged union. With a CADIR FILE it
//! infers native (and other) arena fields from the records themselves —
//! presence, JSON type, an example, and a `relation` column per dotted path.

use std::path::Path;

use anyhow::{bail, Context, Result};
use clap::Args;
use serde_json::{Map, Value};

use super::item::ArenaTarget;
use super::{cell, print_json};

/// Target selection for `query schema`.
#[derive(Debug, Args)]
pub struct SchemaArgs {
    /// CADIR document, or an IR arena / `sidecar` when describing
    /// compile-time types. Omit both arguments to list every model arena.
    #[arg(value_name = "FILE|ARENA")]
    pub file_or_target: Option<String>,
    /// Arena (`model.<arena>` or `native.<codec>.<arena>`) when the first
    /// argument is a CADIR document.
    #[arg(value_name = "ARENA")]
    pub arena: Option<String>,
    /// Print the projected schema subtree as JSON (with its `$defs`
    /// closure) instead of the field table.
    #[arg(long)]
    pub json: bool,
}

const SHAPE: &str = "the generated schema does not have the expected shape";

/// Runs `query schema`.
pub fn run(args: &SchemaArgs) -> Result<()> {
    match (
        args.file_or_target.as_deref(),
        args.arena.as_deref(),
        args.json,
    ) {
        (Some(file), arena, json) if looks_like_file(file) => {
            super::schema_infer::run(file, arena, json)
        }
        (None, None, json) => compile_run(None, json),
        (Some("sidecar"), None, json) => sidecar(json),
        (Some(spec), None, json) => compile_run(Some(spec), json),
        (Some(spec), Some(_), _) => bail!(
            "`query schema {spec} …` is not a CADIR file plus an arena. Infer \
             native fields with `cadmpeg query schema FILE native.<codec>.<arena>`; \
             IR types take one argument: `cadmpeg query schema model.<arena>`"
        ),
        (None, Some(_), _) => unreachable!("clap fills the first positional first"),
    }
}

/// True when `spec` is a path (`-`, slash, `.json`, or an existing file)
/// rather than an arena address.
fn looks_like_file(spec: &str) -> bool {
    spec == "-"
        || spec.contains('/')
        || spec.contains('\\')
        || spec.to_ascii_lowercase().ends_with(".json")
        || Path::new(spec).is_file()
}

fn compile_run(spec: Option<&str>, json: bool) -> Result<()> {
    let root =
        serde_json::to_value(cadmpeg_ir::cadir_json_schema()).context("serializing the schema")?;
    let defs = root
        .get("$defs")
        .and_then(Value::as_object)
        .context(SHAPE)?;
    let model = root
        .get("properties")
        .and_then(|p| p.get("model"))
        .map(|node| resolve(defs, node))
        .and_then(Value::as_object)
        .context(SHAPE)?;

    match spec {
        None => listing(model, defs, json),
        Some(spec) => {
            let target = ArenaTarget::parse(spec)?;
            match &target {
                ArenaTarget::Native { codec, arena } => bail!(
                    "native arena records are per-document. Infer their fields \
                     from a decoded CADIR file: `cadmpeg query schema FILE \
                     native.{codec}.{arena}`"
                ),
                ArenaTarget::Model { arena } => arena_table(arena, model, defs, json),
            }
        }
    }
}

fn listing(model: &Map<String, Value>, defs: &Map<String, Value>, json: bool) -> Result<()> {
    let properties = model
        .get("properties")
        .and_then(Value::as_object)
        .context(SHAPE)?;
    let required = required_set(model.get("required"));
    if json {
        let mut payload = Map::new();
        for (arena, node) in properties {
            payload.insert(
                format!("model.{arena}"),
                Value::String(element_label(node, defs)),
            );
        }
        print_json("schema", &Value::Object(payload));
        return Ok(());
    }
    println!("arena\telement\trequired");
    for (arena, node) in properties {
        let flag = if required.contains(arena.as_str()) {
            "yes"
        } else {
            "no"
        };
        println!("model.{arena}\t{}\t{flag}", element_label(node, defs));
    }
    eprintln!(
        "note: native arena fields are per-document — \
         `cadmpeg query schema FILE native.<codec>.<arena>` infers them; \
         the decode sidecar shape is `cadmpeg query schema sidecar`"
    );
    Ok(())
}

fn arena_table(
    arena: &str,
    model: &Map<String, Value>,
    defs: &Map<String, Value>,
    json: bool,
) -> Result<()> {
    let properties = model
        .get("properties")
        .and_then(Value::as_object)
        .context(SHAPE)?;
    let Some(node) = properties.get(arena) else {
        let known: Vec<&str> = properties.keys().map(String::as_str).collect();
        bail!(
            "unknown model arena {arena:?}; the IR defines: {}; a document's \
             actual arenas come from `cadmpeg query counts FILE`",
            known.join(", ")
        );
    };
    let items = node.get("items").context(SHAPE)?;
    let element = resolve(defs, items);
    if json {
        let payload = serde_json::json!({
            "arena": format!("model.{arena}"),
            "element": ref_name(items).unwrap_or("(inline)"),
            "schema": element,
            "defs": defs_closure(element, defs),
        });
        print_json("schema", &payload);
        return Ok(());
    }
    print_type_table(element, defs);
    Ok(())
}

fn sidecar(json: bool) -> Result<()> {
    let root = serde_json::to_value(cadmpeg_ir::decode_sidecar_json_schema())
        .context("serializing the sidecar schema")?;
    let empty = Map::new();
    let defs = root
        .get("$defs")
        .and_then(Value::as_object)
        .unwrap_or(&empty);
    if json {
        let payload = serde_json::json!({
            "element": "DecodeSidecar",
            "schema": root,
        });
        print_json("schema", &payload);
        return Ok(());
    }
    print_type_table(&root, defs);
    Ok(())
}

/// Prints one struct schema as a `field  type  required  description` table.
fn print_type_table(element: &Value, defs: &Map<String, Value>) {
    let Some(properties) = element.get("properties").and_then(Value::as_object) else {
        // Arena elements that are not plain structs (e.g. an enum): fall back
        // to the compact one-line description of the whole type.
        println!("type\t{}", cell(&type_label(element, defs)));
        return;
    };
    let required = required_set(element.get("required"));
    println!("field\ttype\trequired\tdescription");
    for (field, node) in properties {
        let flag = if required.contains(field.as_str()) {
            "yes"
        } else {
            "no"
        };
        let description = node
            .get("description")
            .and_then(Value::as_str)
            .unwrap_or_default();
        println!(
            "{field}\t{}\t{flag}\t{}",
            cell(&type_label(node, defs)),
            cell(description)
        );
    }
}

fn required_set(required: Option<&Value>) -> std::collections::BTreeSet<&str> {
    required
        .and_then(Value::as_array)
        .map(|list| list.iter().filter_map(Value::as_str).collect())
        .unwrap_or_default()
}

fn resolve<'a>(defs: &'a Map<String, Value>, node: &'a Value) -> &'a Value {
    match ref_name(node) {
        Some(name) => defs.get(name).unwrap_or(node),
        None => node,
    }
}

fn ref_name(node: &Value) -> Option<&str> {
    node.get("$ref")?.as_str()?.strip_prefix("#/$defs/")
}

fn element_label(node: &Value, defs: &Map<String, Value>) -> String {
    match node.get("items") {
        Some(items) => type_label(items, defs),
        None => type_label(node, defs),
    }
}

/// Compact one-line type label for a schema node. Follows a `$ref` one level
/// to describe enums (tag key + every variant), never deeper.
fn type_label(node: &Value, defs: &Map<String, Value>) -> String {
    if let Some(name) = ref_name(node) {
        let suffix = defs
            .get(name)
            .and_then(|def| enum_suffix(def, defs))
            .unwrap_or_default();
        return format!("{name}{suffix}");
    }
    if let Some(any_of) = node.get("anyOf").and_then(Value::as_array) {
        // schemars encodes Option<T> as anyOf [T, null].
        let labels: Vec<String> = any_of
            .iter()
            .filter(|branch| branch.get("type").and_then(Value::as_str) != Some("null"))
            .map(|branch| type_label(branch, defs))
            .collect();
        let nullable = any_of.len() != labels.len();
        let joined = labels.join(" | ");
        return if nullable {
            format!("{joined}?")
        } else {
            joined
        };
    }
    if node.get("oneOf").is_some() {
        let suffix = enum_suffix(node, defs).unwrap_or_default();
        return format!("(union){suffix}");
    }
    if let Some(values) = node.get("enum").and_then(Value::as_array) {
        let names: Vec<&str> = values.iter().filter_map(Value::as_str).collect();
        return format!("enum({})", names.join(", "));
    }
    match node.get("type").and_then(Value::as_str) {
        Some("array") => {
            let items = node.get("items").map_or("any".to_owned(), |items| {
                // Item labels stay shallow: name only, no enum expansion.
                ref_name(items).map_or_else(|| type_label(items, defs), str::to_owned)
            });
            format!("array<{items}>")
        }
        Some(name) => name.to_owned(),
        None => match node.get("type").and_then(Value::as_array) {
            // ["string", "null"] style nullable primitives.
            Some(list) => {
                let names: Vec<&str> = list
                    .iter()
                    .filter_map(Value::as_str)
                    .filter(|name| *name != "null")
                    .collect();
                let nullable = list.len() != names.len();
                format!("{}{}", names.join(" | "), if nullable { "?" } else { "" })
            }
            None => "any".to_owned(),
        },
    }
}

/// For a `oneOf` union: `(tagged by <key>, N variants: a, b, …)` when every
/// variant carries the same const-valued property, else the variant shapes.
/// For a plain string enum: `(enum: a, b, …)`.
fn enum_suffix(def: &Value, _defs: &Map<String, Value>) -> Option<String> {
    if let Some(values) = def.get("enum").and_then(Value::as_array) {
        let names: Vec<&str> = values.iter().filter_map(Value::as_str).collect();
        return Some(format!(" (enum: {})", names.join(", ")));
    }
    let variants = def.get("oneOf").and_then(Value::as_array)?;
    if variants.is_empty() {
        return None;
    }
    if let Some((tag, values)) = common_tag(variants) {
        return Some(format!(
            " (tagged by {tag}, {} variants: {})",
            values.len(),
            values.join(", ")
        ));
    }
    let shapes: Vec<String> = variants
        .iter()
        .map(|variant| {
            variant.get("type").and_then(Value::as_str).map_or_else(
                || ref_name(variant).map_or_else(|| "object".to_owned(), str::to_owned),
                str::to_owned,
            )
        })
        .collect();
    Some(format!(
        " (untagged, {} shapes: {})",
        shapes.len(),
        shapes.join(" | ")
    ))
}

/// The property key that has a string `const` in every variant, with the
/// const values in variant order.
fn common_tag(variants: &[Value]) -> Option<(String, Vec<String>)> {
    let first = variants
        .first()?
        .get("properties")
        .and_then(Value::as_object)?;
    'candidate: for key in first.keys() {
        let mut values = Vec::with_capacity(variants.len());
        for variant in variants {
            let Some(constant) = variant
                .get("properties")
                .and_then(|p| p.get(key))
                .and_then(|node| node.get("const"))
                .and_then(Value::as_str)
            else {
                continue 'candidate;
            };
            values.push(constant.to_owned());
        }
        return Some((key.clone(), values));
    }
    None
}

/// Transitive `$defs` referenced from `element`, for `--json` output.
fn defs_closure(element: &Value, defs: &Map<String, Value>) -> Value {
    let mut wanted = std::collections::BTreeSet::new();
    let mut queue = vec![element];
    while let Some(node) = queue.pop() {
        match node {
            Value::Object(map) => {
                if let Some(name) = ref_name(node) {
                    if wanted.insert(name.to_owned()) {
                        if let Some(def) = defs.get(name) {
                            queue.push(def);
                        }
                    }
                }
                queue.extend(map.values());
            }
            Value::Array(items) => queue.extend(items.iter()),
            _ => {}
        }
    }
    let mut closure = Map::new();
    for name in wanted {
        if let Some(def) = defs.get(&name) {
            closure.insert(name, def.clone());
        }
    }
    Value::Object(closure)
}
