// SPDX-License-Identifier: Apache-2.0
#![allow(clippy::unwrap_used)]

use serde::de::DeserializeOwned;
use serde::Serialize;
use serde_json::{json, Value};

use crate::products::{
    ExternalDocument, GeneralTolerance, MemberAction, Occurrence, ProductDefinition,
    ProductDefinitionKind, Representation, RepresentationContent, Storage,
};

/// The value reads, and writes back as the same JSON.
fn reads_back<T: DeserializeOwned + Serialize>(wire: &Value) -> T {
    let value: T = serde_json::from_value(wire.clone()).unwrap();
    assert_eq!(&serde_json::to_value(&value).unwrap(), wire);
    value
}

/// The value is refused, and the refusal names `named`.
fn refused<T: DeserializeOwned + std::fmt::Debug>(wire: Value, named: &str) {
    let text = wire.to_string();
    let error = serde_json::from_value::<T>(wire).unwrap_err();
    assert!(error.to_string().contains(named), "{text}: {error}");
}

fn path(occurrence: &str) -> Value {
    json!([{"occurrence": {"kind": "occurrence", "value": occurrence}}])
}

fn definition() -> Value {
    json!({
        "id": "test:model:product#root",
        "kind": "assembly",
        "source_name": "root",
        "bom_properties": {"Title": "Root"},
        "generated_from": {
            "definition": "test:model:product#factory",
            "configuration": {"kind": "named", "value": "Default"}
        },
        "storage": {"kind": "file", "document": {"document_id": "uid-1"}},
        "bodies": ["test:model:body#0"],
        "occurrences": ["test:model:occurrence#0"],
        "instance_bodies": [{
            "occurrence": path("test:model:occurrence#0"),
            "bodies": ["test:model:body#1"]
        }],
        "representations": [{
            "name": "Simplified",
            "content": {"kind": "members", "value": {
                "unlisted": {"kind": "exclude"},
                "members": [
                    {"path": path("test:model:occurrence#0"), "action": {"kind": "include"}},
                    {"path": path("test:model:occurrence#0"), "action": {"kind": "detail", "value": "bounding_box"}}
                ]
            }},
            "features": [],
            "default": true
        }],
        "units": {"length": "mm", "angle": "deg"},
        "general_tolerance": {
            "linear": {"kind": "iso2768", "value": "medium"},
            "angular": {"kind": "by_decimals", "value": [
                [1, {"lower": -0.01, "upper": 0.01}],
                [2, {"lower": -0.005, "upper": 0.005}]
            ]}
        },
        "active": false
    })
}

#[test]
fn a_definition_reads_back_with_every_field_stated() {
    let read: ProductDefinition = reads_back(&definition());
    assert_eq!(read.kind, Some(ProductDefinitionKind::Assembly));
    assert!(read.is_root());
    let minimal = json!({
        "id": "test:model:product#part",
        "bom_properties": {},
        "storage": {"kind": "embedded"},
        "bodies": [],
        "occurrences": [],
        "instance_bodies": [],
        "representations": []
    });
    let read: ProductDefinition = reads_back(&minimal);
    assert_eq!(read.kind, None);
    assert!(!read.is_root());
}

#[test]
fn an_unstated_definition_field_has_one_spelling() {
    for key in [
        "kind",
        "source_name",
        "generated_from",
        "units",
        "general_tolerance",
        "active",
    ] {
        let mut wire = definition();
        wire[key] = Value::Null;
        refused::<ProductDefinition>(wire, key);
    }
    for key in [
        "storage",
        "bodies",
        "occurrences",
        "instance_bodies",
        "representations",
        "bom_properties",
    ] {
        let mut wire = definition();
        wire.as_object_mut().unwrap().remove(key);
        refused::<ProductDefinition>(wire, key);
    }
    let mut wire = definition();
    wire["zz_bogus"] = json!(1);
    refused::<ProductDefinition>(wire, "zz_bogus");
}

#[test]
fn storage_states_its_kind_and_the_root_has_no_source() {
    let embedded: Storage = reads_back(&json!({"kind": "embedded"}));
    assert_eq!(embedded, Storage::Embedded);
    let external: Storage = reads_back(&json!({
        "kind": "external", "path": "parts/widget.FCStd", "stamp": "2026", "object": "Body"
    }));
    assert_eq!(
        external,
        Storage::External(ExternalDocument {
            stamp: cadmpeg_core::text::NonBlankString::new("2026"),
            ..ExternalDocument::path("parts/widget.FCStd").with_object("Body")
        })
    );
    let empty: Storage = reads_back(&json!({"kind": "external"}));
    assert_eq!(empty, Storage::External(ExternalDocument::default()));
    let root: Storage = reads_back(&json!({"kind": "file", "document": {}}));
    assert!(matches!(root, Storage::File { source: None, .. }));

    refused::<Storage>(json!({"kind": "embedded", "value": null}), "value");
    refused::<Storage>(json!({"kind": "embedded", "document": {}}), "document");
    refused::<Storage>(json!({"kind": "file"}), "document");
    refused::<Storage>(
        json!({"kind": "file", "document": {}, "source": null}),
        "source",
    );
    refused::<Storage>(json!({"kind": "external", "path": ""}), "blank");
    refused::<Storage>(json!({"kind": "external", "path": null}), "path");
    refused::<Storage>(json!({"kind": "external", "zz_bogus": 1}), "zz_bogus");
}

#[test]
fn an_external_document_keeps_every_identity_and_no_blank_one() {
    assert_eq!(ExternalDocument::path("  "), ExternalDocument::default());
    assert_eq!(
        ExternalDocument::document_id(""),
        ExternalDocument::default()
    );
    assert_eq!(
        ExternalDocument::document_id("x").with_object(""),
        ExternalDocument::document_id("x")
    );
    let full = json!({
        "path": "a.prt", "document_id": "id", "version": "v2", "stamp": "s", "object": "o"
    });
    reads_back::<ExternalDocument>(&full);
    for key in ["path", "document_id", "version", "stamp", "object"] {
        let mut wire = full.clone();
        wire[key] = Value::Null;
        refused::<ExternalDocument>(wire.clone(), key);
        wire[key] = json!(" ");
        refused::<ExternalDocument>(wire, "blank");
    }
}

#[test]
fn an_occurrence_names_its_prototype_and_states_only_what_the_source_does() {
    let full = json!({
        "id": "test:model:occurrence#0",
        "prototype": "test:model:product#part",
        "transform": [[1.0, 0.0, 0.0, 5.0], [0.0, 1.0, 0.0, 0.0], [0.0, 0.0, 1.0, 0.0]],
        "suppressed": false,
        "visible": true,
        "representation": "Simplified",
        "overrides": [{"path": path("test:model:occurrence#1"), "visible": false}],
        "name": "Part<1>",
        "native_ref": "native#0"
    });
    reads_back::<Occurrence>(&full);
    let minimal = json!({
        "id": "test:model:occurrence#0",
        "prototype": "test:model:product#part",
        "overrides": []
    });
    let read: Occurrence = reads_back(&minimal);
    assert_eq!(read.transform, None);
    for key in [
        "transform",
        "suppressed",
        "visible",
        "representation",
        "name",
        "native_ref",
    ] {
        let mut wire = full.clone();
        wire[key] = Value::Null;
        refused::<Occurrence>(wire, key);
    }
    for key in ["parent", "ordinal", "linked_prototype", "scale", "link"] {
        let mut wire = full.clone();
        wire[key] = json!(0);
        refused::<Occurrence>(wire, key);
    }
    let mut wire = full;
    wire["prototype"] = json!({"kind": "local", "definition": "test:model:product#part"});
    refused::<Occurrence>(wire, "invalid type");
}

#[test]
fn a_tag_only_arm_refuses_a_value() {
    reads_back::<RepresentationContent>(&json!({"kind": "unread"}));
    refused::<RepresentationContent>(json!({"kind": "unread", "value": null}), "value");
    for kind in ["include", "exclude"] {
        reads_back::<MemberAction>(&json!({"kind": kind}));
        refused::<MemberAction>(json!({"kind": kind, "value": null}), "value");
    }
    reads_back::<MemberAction>(&json!({"kind": "substitute", "value": "Light"}));
    reads_back::<MemberAction>(&json!({
        "kind": "replace_by", "value": [path("test:model:occurrence#2")]
    }));
    reads_back::<MemberAction>(&json!({"kind": "native", "value": "creo:simprep_status:9"}));
    refused::<MemberAction>(json!({"kind": "substitute", "value": " "}), "blank");
    let representation = json!({
        "name": "Bodies",
        "content": {"kind": "bodies", "value": ["test:model:body#0"]},
        "features": []
    });
    reads_back::<Representation>(&representation);
    let mut wire = representation;
    wire["default"] = Value::Null;
    refused::<Representation>(wire, "default");
}

#[test]
fn a_general_tolerance_map_is_sorted_pairs_with_one_entry_per_key() {
    reads_back::<GeneralTolerance>(&json!({}));
    reads_back::<GeneralTolerance>(&json!({
        "linear": {"kind": "uniform", "value": {"lower": -0.1, "upper": 0.1}}
    }));
    refused::<GeneralTolerance>(json!({"linear": null}), "linear");
    refused::<GeneralTolerance>(
        json!({"angular": {"kind": "by_decimals", "value": [
            [2, {"lower": -0.1, "upper": 0.1}],
            [2, {"lower": -0.2, "upper": 0.2}]
        ]}}),
        "",
    );
    refused::<GeneralTolerance>(
        json!({"linear": {"kind": "iso2768", "value": "extra_fine"}}),
        "extra_fine",
    );
    refused::<GeneralTolerance>(
        json!({"linear": {"kind": "by_decimals", "value": {"2": {"lower": -0.1, "upper": 0.1}}}}),
        "",
    );
}
