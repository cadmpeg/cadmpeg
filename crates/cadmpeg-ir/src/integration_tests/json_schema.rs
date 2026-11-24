// SPDX-License-Identifier: Apache-2.0
//! The committed JSON Schema files are the schemas that the `JsonSchema`
//! derives generate. `UPDATE_GOLDEN=1` rewrites them from the derives.

use std::path::Path;

use schemars::transform::{RecursiveTransform, Transform};
use schemars::Schema;
use serde_json::{Map, Value};

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
        let derived = serde_json::to_string_pretty(&derived).unwrap() + "\n";
        let path = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("schema")
            .join(file);
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
            "{} differs from the derived schema at line {line}. Run \
             `UPDATE_GOLDEN=1 cargo test -p cadmpeg-ir --features schema --lib -- \
             committed_json_schemas` and commit the file.",
            path.display()
        );
    }
}

/// Whether `node` can admit `null`: false only where a keyword of the node,
/// or of the definition it references, refuses it.
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
        || refuses("allOf", &|branches| arms(branches, true)))
}

/// Every member that an object schema in `schema` declares and does not
/// require, as `(location, member schema)`. A schema without `"type":
/// "object"` refines members that another schema declares (`allOf`, `if`,
/// `then`, `else`) and is left out.
fn optional_members(schema: &Value) -> Vec<(String, Value)> {
    let mut found = Vec::new();
    let mut root: Schema = schema.clone().try_into().unwrap();
    RecursiveTransform(|node: &mut Schema| {
        let object = match node.get("type") {
            Some(Value::Array(kinds)) => kinds.iter().any(|kind| kind == "object"),
            Some(kind) => kind == "object",
            None => false,
        };
        if !object {
            return;
        }
        let Some(members) = node.get("properties").and_then(Value::as_object) else {
            return;
        };
        let required = node.get("required").and_then(Value::as_array);
        for (name, member) in members {
            if !required.is_some_and(|required| required.iter().any(|r| r == name.as_str())) {
                let at = node
                    .get("description")
                    .and_then(Value::as_str)
                    .unwrap_or("");
                found.push((format!("{name} (in: {at})"), member.clone()));
            }
        }
    })
    .transform(&mut root);
    found
}

/// §2.5: an `Option` member is omitted for `None` and is never written
/// `null`, so neither committed schema admits `null` for a member its object
/// does not require.
#[test]
fn an_optional_member_admits_no_null() {
    for committed in [crate::CADIR_JSON_SCHEMA, crate::DECODE_SIDECAR_JSON_SCHEMA] {
        let schema: Value = serde_json::from_str(committed).unwrap();
        let defs = schema["$defs"].as_object().unwrap();
        let members = optional_members(&schema);
        assert!(!members.is_empty());
        let nullable = members
            .iter()
            .filter(|(_, member)| admits_null(member, defs, 0))
            .map(|(at, _)| at.as_str())
            .collect::<Vec<_>>();
        assert!(nullable.is_empty(), "{}", nullable.join("\n"));
    }
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
