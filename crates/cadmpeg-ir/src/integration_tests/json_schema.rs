// SPDX-License-Identifier: Apache-2.0
//! The committed JSON Schema files are the schemas that the `JsonSchema`
//! derives generate. Under the `schema-order` feature, which lists each
//! object's `properties` in declared order, the test compares their text and
//! `UPDATE_GOLDEN=1` rewrites them; under `schema` alone it compares their
//! content.

use std::collections::BTreeSet;
use std::path::Path;

use serde_json::{Map, Value};

const REGENERATE: &str =
    "UPDATE_GOLDEN=1 cargo test -p cadmpeg-ir --features schema-order --lib -- committed_json_schemas";

#[test]
fn committed_json_schemas_equal_the_derived_schemas() {
    let update = std::env::var_os("UPDATE_GOLDEN").is_some();
    for (file, committed, derived) in [
        (
            "cadir.schema.json",
            crate::CADIR_JSON_SCHEMA,
            crate::cadir_json_schema(),
        ),
        (
            "decode-sidecar.schema.json",
            crate::DECODE_SIDECAR_JSON_SCHEMA,
            crate::decode_sidecar_json_schema(),
        ),
    ] {
        let path = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("schema")
            .join(file);
        if !cfg!(feature = "schema-order") {
            let committed: Value = serde_json::from_str(committed).unwrap();
            assert!(
                committed == serde_json::to_value(&derived).unwrap(),
                "{} differs from the derived schema. Run `{REGENERATE}` and commit the file.",
                path.display()
            );
            continue;
        }
        let derived = serde_json::to_string_pretty(&derived).unwrap() + "\n";
        if update {
            std::fs::write(&path, &derived).unwrap();
            continue;
        }
        let line = committed
            .lines()
            .zip(derived.lines())
            .position(|(committed, derived)| committed != derived)
            .unwrap_or_else(|| committed.lines().count().min(derived.lines().count()))
            + 1;
        assert!(
            committed == derived,
            "{} differs from the derived schema at line {line}. Run `{REGENERATE}` and commit \
             the file.",
            path.display()
        );
    }
}

/// Whether `node` can admit `null`: false only where a keyword of the node,
/// or of the definition it references, refuses it. `not` refuses `null`
/// where its schema admits it, so `{"not": {}}`, the schema of an enum with
/// no variant, admits no value.
fn admits_null(node: &Value, defs: &Map<String, Value>, depth: usize) -> bool {
    let Value::Object(node) = node else {
        return node.as_bool().unwrap_or(false);
    };
    if depth > 64 {
        return true;
    }
    let refuses =
        |keyword: &str, refused: &dyn Fn(&Value) -> bool| node.get(keyword).is_some_and(refused);
    let arms = |arms: &Value, all: bool| {
        let arms = arms.as_array().map_or(&[][..], Vec::as_slice);
        let admit = |arm| admits_null(arm, defs, depth + 1);
        if all {
            !arms.iter().all(admit)
        } else {
            !arms.iter().any(admit)
        }
    };
    !(refuses("type", &|kind| match kind {
        Value::Array(kinds) => !kinds.iter().any(|kind| kind == "null"),
        kind => kind != "null",
    }) || refuses("const", &|value| !value.is_null())
        || refuses("enum", &|values| {
            values
                .as_array()
                .is_some_and(|values| !values.contains(&Value::Null))
        })
        || refuses("$ref", &|reference| {
            reference
                .as_str()
                .and_then(|reference| reference.strip_prefix("#/$defs/"))
                .and_then(|name| defs.get(name))
                .is_none_or(|target| !admits_null(target, defs, depth + 1))
        })
        || refuses("anyOf", &|branches| arms(branches, false))
        || refuses("oneOf", &|branches| arms(branches, false))
        || refuses("allOf", &|branches| arms(branches, true))
        || refuses("not", &|negated| admits_null(negated, defs, depth + 1)))
}

/// Every place in `schema` that admits `null` for a value that is not a list
/// element, an array element, or a map value, as the path that reaches it.
///
/// Each such value is the root or a member that an object schema declares,
/// required or not; `admits_null` follows the `$ref`, `anyOf`, `oneOf`, and
/// `allOf` it states. An element or a map value may admit `null`, and the
/// members of the objects it holds are checked in turn. A schema without
/// `"type": "object"` refines members that another schema declares (`allOf`,
/// `if`, `then`, `else`), so its members are left out.
fn null_values(schema: &Value) -> BTreeSet<String> {
    let defs = schema["$defs"].as_object().unwrap();
    let mut walk = NullWalk {
        defs,
        visited: BTreeSet::new(),
        found: BTreeSet::new(),
    };
    walk.value(schema, "$", false);
    walk.found
}

struct NullWalk<'a> {
    defs: &'a Map<String, Value>,
    visited: BTreeSet<&'a str>,
    found: BTreeSet<String>,
}

impl<'a> NullWalk<'a> {
    /// A value at `at`: an element or a map value where `element` is true.
    fn value(&mut self, node: &'a Value, at: &str, element: bool) {
        if !element && admits_null(node, self.defs, 0) {
            self.found.insert(at.to_owned());
        }
        self.members(node, at);
    }

    /// The values that `node`, or a schema it references or combines, holds.
    fn members(&mut self, node: &'a Value, at: &str) {
        let Value::Object(node) = node else {
            return;
        };
        if let Some(name) = node
            .get("$ref")
            .and_then(Value::as_str)
            .and_then(|reference| reference.strip_prefix("#/$defs/"))
        {
            if let Some((name, target)) = self.defs.get_key_value(name) {
                if self.visited.insert(name) {
                    self.members(target, &format!("{at} -> {name}"));
                }
            }
        }
        for keyword in ["anyOf", "oneOf", "allOf"] {
            for arm in node
                .get(keyword)
                .and_then(Value::as_array)
                .into_iter()
                .flatten()
            {
                self.members(arm, at);
            }
        }
        let object = match node.get("type") {
            Some(Value::Array(kinds)) => kinds.iter().any(|kind| kind == "object"),
            Some(kind) => kind == "object",
            None => false,
        };
        if object {
            for (name, member) in node
                .get("properties")
                .and_then(Value::as_object)
                .into_iter()
                .flatten()
            {
                self.value(member, &format!("{at}.{name}"), false);
            }
        }
        for (pattern, member) in node
            .get("patternProperties")
            .and_then(Value::as_object)
            .into_iter()
            .flatten()
        {
            self.value(member, &format!("{at}{{{pattern}}}"), true);
        }
        if let Some(member) = node
            .get("additionalProperties")
            .filter(|member| member.is_object())
        {
            self.value(member, &format!("{at}{{}}"), true);
        }
        if let Some(member) = node.get("items") {
            self.value(member, &format!("{at}[]"), true);
        }
        for (position, member) in node
            .get("prefixItems")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
            .enumerate()
        {
            self.value(member, &format!("{at}[{position}]"), true);
        }
    }
}

/// Each member name and each string a schema node fixes (`const`, `enum`) in
/// `node` and the schemas below it.
fn wire_names<'a>(node: &'a Value, names: &mut BTreeSet<&'a str>) {
    match node {
        Value::Object(node) => {
            for (keyword, value) in node {
                match (keyword.as_str(), value) {
                    ("properties" | "$defs" | "patternProperties", Value::Object(members)) => {
                        for (name, member) in members {
                            if keyword == "properties" {
                                names.insert(name);
                            }
                            wire_names(member, names);
                        }
                    }
                    ("const", Value::String(name)) => {
                        names.insert(name);
                    }
                    ("enum", Value::Array(values)) => {
                        names.extend(values.iter().filter_map(Value::as_str));
                    }
                    ("description" | "title" | "pattern" | "$ref" | "$schema", _) => {}
                    (_, value) => wire_names(value, names),
                }
            }
        }
        Value::Array(values) => values.iter().for_each(|value| wire_names(value, names)),
        _ => {}
    }
}

/// §2.5: field and variant names are snake_case.
#[test]
fn every_wire_name_is_snake_case() {
    for committed in [crate::CADIR_JSON_SCHEMA, crate::DECODE_SIDECAR_JSON_SCHEMA] {
        let schema: Value = serde_json::from_str(committed).unwrap();
        let mut names = BTreeSet::new();
        wire_names(&schema, &mut names);
        let other: Vec<&str> = names
            .into_iter()
            .filter(|name| {
                !name
                    .bytes()
                    .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'_')
            })
            .collect();
        assert!(other.is_empty(), "{other:?}");
    }
}

/// §2.5: an `Option` field is omitted for `None` and is never written
/// `null`, and no newtype variant wraps an `Option`; only a list element, an
/// array element, or a map value writes `null`. So neither committed schema
/// admits `null` anywhere else.
#[test]
fn null_is_admitted_only_as_an_element_or_a_map_value() {
    for committed in [crate::CADIR_JSON_SCHEMA, crate::DECODE_SIDECAR_JSON_SCHEMA] {
        let schema: Value = serde_json::from_str(committed).unwrap();
        let found = null_values(&schema);
        assert!(
            found.is_empty(),
            "{}",
            found.into_iter().collect::<Vec<_>>().join("\n")
        );
    }
}

/// The walk finds the two spellings of an `Option` outside an element: a
/// newtype variant's `value`, required, and an optional member whose
/// definition is a newtype struct over an `Option`. It passes `null` as a
/// list element, a pair key, and a map value, and checks the members of the
/// objects they hold. A member with no value, `{"not": {}}`, admits no `null`.
#[test]
fn the_null_walk_finds_an_option_outside_an_element() {
    let schema = serde_json::json!({
        "type": "object",
        "properties": {
            "section": {"$ref": "#/$defs/Section"},
            "continuity": {"$ref": "#/$defs/State"},
            "list": {"type": "array", "items": {"type": ["string", "null"]}},
            "pairs": {
                "type": "array",
                "items": {
                    "type": "array",
                    "prefixItems": [{"type": ["string", "null"]}, {"$ref": "#/$defs/Entry"}]
                }
            },
            "map": {"type": "object", "additionalProperties": {"type": ["string", "null"]}},
            "never": {"not": {}},
            "named": {
                "type": "object",
                "patternProperties": {"^a": {"anyOf": [{"type": "string"}, {"type": "null"}]}}
            }
        },
        "required": ["section", "list", "pairs", "map", "named", "never"],
        "$defs": {
            "Section": {
                "oneOf": [{
                    "type": "object",
                    "properties": {
                        "kind": {"type": "string", "const": "unresolved"},
                        "value": {"type": ["string", "null"]}
                    },
                    "required": ["kind", "value"]
                }]
            },
            "State": {"anyOf": [{"type": "string"}, {"type": "null"}]},
            "Entry": {
                "type": "object",
                "properties": {"tolerance": {"type": ["number", "null"]}}
            }
        }
    });
    assert_eq!(
        null_values(&schema),
        BTreeSet::from([
            "$.continuity".to_owned(),
            "$.pairs[][1] -> Entry.tolerance".to_owned(),
            "$.section -> Section.value".to_owned(),
        ])
    );
}

/// A vertex `tolerance` stated `null` is refused by the reader and by the
/// committed schema at its position.
#[test]
fn a_null_optional_member_fails_the_schema_and_the_reader() {
    let mut document =
        serde_json::to_value(crate::examples::unit_cube().expect("the example builds")).unwrap();
    let vertex = &mut document["geometry"]["vertices"][0];
    vertex["tolerance"] = Value::Null;
    let refusal = serde_json::from_value::<crate::CadIr>(document).unwrap_err();
    assert!(
        refusal.to_string().contains("it does not state null"),
        "{refusal}"
    );

    let schema: Value = serde_json::from_str(crate::CADIR_JSON_SCHEMA).unwrap();
    let defs = schema["$defs"].as_object().unwrap();
    let resolve = |node: &Value| -> Value {
        node["$ref"]
            .as_str()
            .and_then(|reference| reference.strip_prefix("#/$defs/"))
            .map_or_else(|| node.clone(), |name| defs[name].clone())
    };
    let topology = resolve(&schema["properties"]["geometry"]);
    let vertex = resolve(&topology["properties"]["vertices"]["items"]);
    let tolerance = &vertex["properties"]["tolerance"];
    assert!(tolerance.is_object());
    assert!(!vertex["required"]
        .as_array()
        .unwrap()
        .iter()
        .any(|name| name == "tolerance"));
    assert!(!admits_null(tolerance, defs, 0));
}

/// The writer writes a procedural carrier's `cache`, and the schema of the
/// carrier's `procedural` arm, which admits no other member, states it.
#[test]
fn a_procedural_carrier_cache_is_a_member_of_its_schema() {
    use crate::geometry::{
        CurveGeometry, SolvedCurveGeometry, SolvedSurfaceGeometry, SurfaceGeometry,
    };
    use crate::ids::{ProceduralCurveId, ProceduralSurfaceId};

    let surface = SurfaceGeometry::Procedural {
        construction: ProceduralSurfaceId::mint("test:model:construction#surface").unwrap(),
        cache: Some(SolvedSurfaceGeometry::Unknown { record: None }),
    };
    let curve = CurveGeometry::Procedural {
        construction: ProceduralCurveId::mint("test:model:construction#curve").unwrap(),
        cache: Some(SolvedCurveGeometry::Unknown { record: None }),
    };
    let schema = serde_json::to_value(crate::cadir_json_schema()).unwrap();
    let defs = schema["$defs"].as_object().unwrap();
    for (name, written) in [
        ("SurfaceGeometry", serde_json::to_value(surface).unwrap()),
        ("CurveGeometry", serde_json::to_value(curve).unwrap()),
    ] {
        assert_eq!(written["kind"], "procedural");
        let arm = defs[name]["oneOf"]
            .as_array()
            .unwrap()
            .iter()
            .find(|arm| arm["properties"]["kind"]["const"] == "procedural")
            .unwrap();
        let mut payload = &arm["properties"]["value"];
        if let Some(reference) = payload["$ref"].as_str() {
            payload = &defs[reference.strip_prefix("#/$defs/").unwrap()];
        }
        assert_eq!(payload["additionalProperties"], false, "{name}");
        let members = payload["properties"].as_object().unwrap();
        let stated = written["value"].as_object().unwrap();
        assert!(stated.contains_key("cache"), "{name}");
        for key in stated.keys() {
            assert!(members.contains_key(key), "{name}.{key}");
        }
    }
}

/// §2.5: an `Option` that is a list element, an array element, or a map
/// value writes `null` for `None`. An assembly context's participants are
/// `[effect, selection]` pairs, and a participant whose effect the source
/// does not state has `null` as its pair's first element: the writer writes
/// it, the reader reads it back, and the committed schema admits it there.
#[test]
fn a_null_pair_key_is_written_read_and_admitted() {
    use std::collections::BTreeMap;

    use crate::design::ops::assembly::AssemblyContext;
    use crate::design::selection::{Anchor, Picks, Selection};

    let context = AssemblyContext {
        participants: BTreeMap::from([(
            None,
            Selection {
                at: Anchor::Current,
                picks: Picks::Unread,
                native: None,
            },
        )]),
    };
    let written = serde_json::to_value(&context).unwrap();
    assert_eq!(written["participants"][0][0], Value::Null, "{written}");
    let read: AssemblyContext = serde_json::from_value(written).unwrap();
    assert_eq!(read, context);

    let schema: Value = serde_json::from_str(crate::CADIR_JSON_SCHEMA).unwrap();
    let defs = schema["$defs"].as_object().unwrap();
    let participants = &defs["AssemblyContext"]["properties"]["participants"];
    assert!(admits_null(
        &participants["items"]["prefixItems"][0],
        defs,
        0
    ));
    assert!(!admits_null(participants, defs, 0));
}
