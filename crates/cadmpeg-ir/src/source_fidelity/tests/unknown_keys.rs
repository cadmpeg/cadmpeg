// SPDX-License-Identifier: Apache-2.0
use std::collections::BTreeSet;

use serde_json::{json, Value};

use crate::source_fidelity::DecodeSidecar;

const FIRST: &str = "synthetic:source:record#inline";
const SECOND: &str = "synthetic:source:record#digest";
const DIGEST: &str = "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad";

// These objects are maps whose keys name records, fields, or declared measures.
// Their values are still visited through the complete sidecar reader.
const MAP_PATHS: &[&str] = &[
    "/fidelity/retained_records",
    "/fidelity/annotations/provenance",
    "/fidelity/annotations/exactness",
    "/fidelity/annotations/exactness/synthetic:source:record#inline/fields",
    "/fidelity/annotations/exactness/synthetic:source:record#digest/fields",
    "/report/coverage",
    "/report/identity/dialects/primary/declared",
];

fn fixture() -> Value {
    json!({
        "ir_sha256": DIGEST,
        "report": {
            "identity": {"classification": "unclassified", "format": "synthetic"},
            "transfer": {"transfer": "full", "geometry_transferred": true},
            "coverage": {"source_records": 2},
            "losses": [
                {
                    "code": {"scope": "shared", "kind": "pcurve_omitted"},
                    "severity": "warning", "message": "pcurve absent",
                    "provenance": {"format": "synthetic", "offset": 0}
                },
                {
                    "code": {"scope": "namespaced", "namespace": "synthetic", "code": "geometry.pcurve-omitted", "kind": "pcurve_omitted", "strict_floor": "error"},
                    "severity": "warning", "message": "pcurve absent",
                    "provenance": {"format": "synthetic", "stream": "source", "offset": 7, "tag": "pcurve"}
                }
            ],
            "notes": [],
            "transfer_ledger": {"entries": [
                {"source": "one", "outcome": {"disposition": "emitted", "target": FIRST}},
                {"source": "two", "outcome": {"disposition": "retained", "target": SECOND, "note": "retained"}},
                {"source": "three", "outcome": {"disposition": "approximated", "target": FIRST, "note": "approximate"}},
                {"source": "four", "outcome": {"disposition": "omitted", "note": "absent"}}
            ]}
        },
        "fidelity": {
            "annotations": {
                "provenance": {FIRST: {"stream": "source", "offset": 7, "tag": "record"}},
                "exactness": {
                    FIRST: {"scope": "entity", "entity": "inferred", "fields": {"position": "byte_exact"}},
                    SECOND: {"scope": "fields", "fields": {"position": "derived"}}
                }
            },
            "retained_records": {
                FIRST: {"stream": "", "offset": 0, "bytes": {"retention": "inline", "data": "YWJj"}},
                SECOND: {"stream": "source", "offset": 7, "bytes": {"retention": "digest", "byte_len": 3, "sha256": DIGEST}}
            }
        }
    })
}

fn object_paths(value: &Value, path: &str, paths: &mut Vec<String>) {
    match value {
        Value::Object(fields) => {
            paths.push(path.to_owned());
            for (key, value) in fields {
                let key = key.replace('~', "~0").replace('/', "~1");
                object_paths(value, &format!("{path}/{key}"), paths);
            }
        }
        Value::Array(values) => {
            for (index, value) in values.iter().enumerate() {
                object_paths(value, &format!("{path}/{index}"), paths);
            }
        }
        _ => {}
    }
}

#[test]
fn complete_sidecar_refuses_unknown_fields_at_every_owned_object() {
    let mut fixtures = vec![fixture()];
    for admission in [
        json!("admitted"),
        json!("residual"),
        json!("refused"),
        json!({"unverified": {"using": "ap242e1"}}),
    ] {
        let mut classified = fixture();
        classified["report"]["identity"] = json!({
            "classification": "classified",
            "dialects": {
                "primary": {"dialect": "step:ap242e1", "declared": {"schema": "AP242"}, "admission": admission},
                "extra": [{"dialect": "acis:binary-700", "instance": "body", "admission": "admitted"}]
            }
        });
        classified["report"]["transfer"] = json!({"transfer": "container_only"});
        fixtures.push(classified);
    }
    let mut visited = BTreeSet::new();
    for fixture in fixtures {
        DecodeSidecar::from_json(&fixture.to_string())
            .expect("independent complete sidecar fixture");
        let mut paths = Vec::new();
        object_paths(&fixture, "", &mut paths);
        for path in paths {
            visited.insert(path.clone());
            if MAP_PATHS.contains(&path.as_str()) {
                continue;
            }
            for value in [
                Value::Null,
                json!(1),
                json!(1.5),
                json!("probe"),
                json!(true),
                json!({}),
                json!([]),
            ] {
                let mut probe = fixture.clone();
                let fields = probe.pointer_mut(&path).unwrap().as_object_mut().unwrap();
                assert!(
                    !fields.contains_key("zz_bogus"),
                    "sentinel collision at {path}"
                );
                fields.insert("zz_bogus".into(), value);
                let error = DecodeSidecar::from_json(&probe.to_string())
                    .expect_err(&format!("unknown field at {path}"));
                assert!(error.to_string().contains("zz_bogus"), "{path}: {error}");
            }
        }
    }
    for map in MAP_PATHS {
        assert!(visited.contains(*map), "unvisited map declaration {map}");
    }
    assert!(visited.contains("/report/identity/dialects/primary/admission/unverified"));
}

#[test]
fn complete_sidecar_refuses_repeated_retained_keys_before_replacement() {
    let fixture = fixture();
    let text = fixture.to_string();
    let record = &fixture["fidelity"]["retained_records"][FIRST];
    let member = format!("{}:{record}", serde_json::to_string(FIRST).unwrap());
    assert_eq!(text.matches(&member).count(), 1);
    let repeated = text.replace(&member, &format!("{member},{member}"));
    let error = DecodeSidecar::from_json(&repeated).unwrap_err();
    assert!(
        error
            .to_string()
            .contains(&format!("duplicate key {FIRST}")),
        "{error}"
    );
}
