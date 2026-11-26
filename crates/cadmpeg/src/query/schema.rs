// SPDX-License-Identifier: Apache-2.0
//! Schema projection for `cadmpeg query schema`.
//!
//! The `types` subcommand describes the IR types this binary was built with,
//! generated from the `cadmpeg-ir` derives: every field, which are
//! optional, and every variant of a tagged union. The `file` subcommand
//! infers native (and other) arena fields from the records themselves —
//! presence, JSON type, an example, and a `relation` column per dotted path.

use anyhow::{bail, Context, Result};
use cadmpeg_ir::ArenaHome;
use clap::{Args, Subcommand};
use serde_json::{Map, Value};

use super::item::ArenaTarget;
use super::{cell, print_json};

/// Target selection for `query schema`.
#[derive(Debug, Args)]
pub(crate) struct SchemaArgs {
    /// Schema source; omit to list the neutral arenas.
    #[command(subcommand)]
    target: Option<SchemaTarget>,
    /// Print the schema as JSON.
    #[arg(long, global = true)]
    json: bool,
}

/// Source of a schema projection.
#[derive(Debug, Subcommand)]
enum SchemaTarget {
    /// Describe compiled IR types.
    Types {
        /// Neutral arena (`<home>.<arena>` or a bare `<arena>`); omit to list
        /// every neutral arena.
        arena: Option<String>,
    },
    /// Infer arena fields from a CADIR document.
    File {
        /// CADIR document path or `-` for standard input.
        file: String,
        /// Arena to describe.
        arena: Option<String>,
    },
    /// Describe the decode sidecar type.
    Sidecar,
}

const SHAPE: &str = "the generated schema does not have the expected shape";

/// Runs `query schema`.
pub(super) fn run(args: &SchemaArgs) -> Result<()> {
    match &args.target {
        None => compile_run(None, args.json),
        Some(SchemaTarget::Types { arena }) => compile_run(arena.as_deref(), args.json),
        Some(SchemaTarget::File { file, arena }) => {
            super::schema_infer::run(file, arena.as_deref(), args.json)
        }
        Some(SchemaTarget::Sidecar) => sidecar(args.json),
    }
}

fn compile_run(spec: Option<&str>, json: bool) -> Result<()> {
    let root: Value =
        serde_json::from_str(cadmpeg_ir::CADIR_JSON_SCHEMA).context("reading the schema")?;
    let defs = root
        .get("$defs")
        .and_then(Value::as_object)
        .context(SHAPE)?;
    let homes = ArenaHome::ALL
        .into_iter()
        .map(|home| {
            root.get("properties")
                .and_then(|p| p.get(home.as_str()))
                .map(|node| resolve(defs, node))
                .and_then(Value::as_object)
                .map(|schema| (home, arena_properties(schema)))
                .context(SHAPE)
        })
        .collect::<Result<Vec<_>>>()?;

    match spec {
        None => listing(&homes, defs, json),
        Some(spec) => {
            let target = ArenaTarget::parse(spec)?;
            match &target {
                ArenaTarget::Native { codec, arena } => bail!(
                    "native arena records are per-document. Infer their fields \
                     from a decoded CADIR file: `cadmpeg query schema file FILE \
                     native.{codec}.{arena}`"
                ),
                ArenaTarget::Neutral { home, arena } => {
                    arena_table(&homes, *home, arena, defs, json)
                }
            }
        }
    }
}

/// A home's arena schemas and the arenas its schema requires. A home with no
/// arenas has an object schema without `properties`.
type HomeArenas<'a> = (&'a Map<String, Value>, std::collections::BTreeSet<&'a str>);

fn arena_properties(schema: &Map<String, Value>) -> HomeArenas<'_> {
    static EMPTY: std::sync::OnceLock<Map<String, Value>> = std::sync::OnceLock::new();
    (
        schema
            .get("properties")
            .and_then(Value::as_object)
            .unwrap_or_else(|| EMPTY.get_or_init(Map::new)),
        required_set(schema.get("required")),
    )
}

fn listing(
    homes: &[(ArenaHome, HomeArenas<'_>)],
    defs: &Map<String, Value>,
    json: bool,
) -> Result<()> {
    if json {
        let mut payload = Map::new();
        for (home, (properties, _)) in homes {
            for (arena, node) in *properties {
                payload.insert(
                    format!("{home}.{arena}"),
                    Value::String(element_label(node, defs)),
                );
            }
        }
        print_json("schema", Value::Object(payload))?;
        return Ok(());
    }
    println!("arena\telement\trequired");
    for (home, (properties, required)) in homes {
        for (arena, node) in *properties {
            let flag = if required.contains(arena.as_str()) {
                "yes"
            } else {
                "no"
            };
            println!("{home}.{arena}\t{}\t{flag}", element_label(node, defs));
        }
    }
    eprintln!(
        "note: native arena fields are per-document — \
         `cadmpeg query schema file FILE native.<codec>.<arena>` infers them; \
         the decode sidecar shape is `cadmpeg query schema sidecar`"
    );
    Ok(())
}

fn arena_table(
    homes: &[(ArenaHome, HomeArenas<'_>)],
    home: ArenaHome,
    arena: &str,
    defs: &Map<String, Value>,
    json: bool,
) -> Result<()> {
    let node = homes
        .iter()
        .find(|(candidate, _)| *candidate == home)
        .and_then(|(_, (properties, _))| properties.get(arena));
    let Some(node) = node else {
        let known: Vec<String> = homes
            .iter()
            .flat_map(|(home, (properties, _))| {
                properties
                    .keys()
                    .map(move |arena| format!("{home}.{arena}"))
            })
            .collect();
        bail!(
            "unknown {home} arena {arena:?}; the IR defines: {}; a document's \
             actual arenas come from `cadmpeg query counts FILE`",
            known.join(", ")
        );
    };
    let items = node.get("items").context(SHAPE)?;
    let element = resolve(defs, items);
    if json {
        let payload = cadmpeg_core::json!({
            "arena": format!("{home}.{arena}"),
            "element": ref_name(items).unwrap_or("(inline)"),
            "schema": element,
            "defs": defs_closure(element, defs),
        });
        print_json("schema", payload)?;
        return Ok(());
    }
    print_type_table(element, defs);
    Ok(())
}

fn sidecar(json: bool) -> Result<()> {
    let root: Value = serde_json::from_str(cadmpeg_ir::DECODE_SIDECAR_JSON_SCHEMA)
        .context("reading the sidecar schema")?;
    let empty = Map::new();
    let defs = root
        .get("$defs")
        .and_then(Value::as_object)
        .unwrap_or(&empty);
    if json {
        let payload = cadmpeg_core::json!({
            "element": "DecodeSidecar",
            "schema": root,
        });
        print_json("schema", payload)?;
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
        // A `null` arm marks a list element or map value that may be `null`.
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
fn enum_suffix(def: &Value, defs: &Map<String, Value>) -> Option<String> {
    if let Some(values) = def.get("enum").and_then(Value::as_array) {
        let names: Vec<&str> = values.iter().filter_map(Value::as_str).collect();
        return Some(format!(" (enum: {})", names.join(", ")));
    }
    let variants = def
        .get("oneOf")
        .or_else(|| def.get("anyOf"))
        .and_then(Value::as_array)?;
    if variants.is_empty() {
        return None;
    }
    if let Some((tag, values)) = common_tag(variants, defs) {
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
fn common_tag(variants: &[Value], defs: &Map<String, Value>) -> Option<(String, Vec<String>)> {
    let first = variants
        .first()?
        .get("properties")
        .and_then(Value::as_object)?;
    'candidate: for key in first.keys() {
        let mut values = Vec::new();
        for variant in variants {
            if let Some(constant) = variant
                .get("properties")
                .and_then(|p| p.get(key))
                .and_then(|node| node.get("const"))
                .and_then(Value::as_str)
            {
                values.push(constant.to_owned());
                continue;
            }
            // A variant that is itself a union of the same tag contributes its
            // own tag values: a union nested to keep one wire flat.
            let Some((nested_tag, nested_values)) = ref_name(variant)
                .and_then(|name| defs.get(name))
                .and_then(|nested| {
                    nested
                        .get("oneOf")
                        .or_else(|| nested.get("anyOf"))
                        .and_then(Value::as_array)
                })
                .and_then(|nested| common_tag(nested, defs))
            else {
                continue 'candidate;
            };
            if nested_tag != *key {
                continue 'candidate;
            }
            values.extend(nested_values);
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
