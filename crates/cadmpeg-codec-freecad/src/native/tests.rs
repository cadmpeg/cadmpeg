// SPDX-License-Identifier: Apache-2.0
//! Native record invariants and exact value-text tests.

mod property_body;
mod value_text;

use super::{model_id, native_child_id, native_id, ValueRecord};

#[test]
fn decoded_json_wire_distinguishes_absence_from_held_null() {
    let value = super::ValueRecord {
        tag: "Python".into(),
        order: 0,
        parent: None,
        attributes: std::collections::BTreeMap::new(),
        decoded_json: None,
        text: None,
    };
    let mut wire = serde_json::to_value(value).unwrap();
    assert!(wire.get("decoded_json").is_none());
    assert!(serde_json::from_value::<super::ValueRecord>(wire.clone())
        .unwrap()
        .decoded_json
        .is_none());
    wire["decoded_json"] = serde_json::json!({"value":null});
    let decoded: super::ValueRecord = serde_json::from_value(wire.clone()).unwrap();
    assert_eq!(decoded.decoded_json, Some(serde_json::Value::Null.into()));
    assert_eq!(serde_json::to_value(decoded).unwrap(), wire);
    wire["decoded_json"]["unknown"] = serde_json::json!(true);
    assert!(serde_json::from_value::<super::ValueRecord>(wire.clone()).is_err());
    wire["decoded_json"] = serde_json::Value::Null;
    let error = serde_json::from_value::<super::ValueRecord>(wire).unwrap_err();
    assert!(error.to_string().contains("decoded_json:"));
}

#[test]
fn entries_check_names_identities_references_and_cache_the_digest() {
    for (id, name, references) in [
        ("invalid", "safe", vec![]),
        ("fcstd:native:entry#safe", "../x", vec![]),
        (
            "fcstd:native:entry#safe",
            "safe",
            vec!["invalid".to_owned()],
        ),
    ] {
        assert!(crate::test_support::source_fidelity::EntryFixture::try_new(
            id.into(),
            name.into(),
            cadmpeg_core::container::ContainerRole::Auxiliary,
            references,
            vec![1, 2, 3]
        )
        .is_err());
    }
    let record = crate::test_support::source_fidelity::EntryFixture::try_new(
        "fcstd:native:entry#safe".into(),
        "safe".into(),
        cadmpeg_core::container::ContainerRole::Auxiliary,
        Vec::new(),
        vec![1, 2, 3],
    )
    .expect("entry");
    let fidelity = crate::test_support::source_fidelity::entry_layout_fixture(&record);
    let wire = serde_json::to_value(&record.record).expect("wire");
    assert_eq!(
        fidelity.retained_record(record.id()).unwrap().sha256(),
        "039058c6f2c0cb492c533b0a4d14ef77cc0f78abccced5287d84a1a2011cfb81"
    );
    for (key, value) in [
        ("id", serde_json::json!("invalid")),
        ("name", serde_json::json!("../x")),
        ("referenced_by", serde_json::json!(["invalid"])),
    ] {
        let mut invalid = wire.clone();
        invalid[key] = value;
        assert!(serde_json::from_value::<super::EntryRecord>(invalid).is_err());
    }
    let mut record = record;
    assert!(record.add_reference("invalid").is_err());
    assert!(record.referenced_by().is_empty());
}

#[test]
fn duplicate_property_is_malformed() {
    let property = super::PropertyRecord {
        dynamic: None,
        id: "fcstd:native:property#LongProperty".into(),
        owner: "fcstd:native:object#Owner".into(),
        name: "LongProperty".into(),
        type_name: "App::PropertyString".into(),
        family: super::PropertyFamily::Unknown,
        status: None,
        body: Some(super::PropertyBody::Transient),
        order: 0,
    };
    let _fidelity =
        crate::test_support::source_fidelity::xml_layout_fixture(&property.id, "<Property/>");
    let error = super::sole_named_property("product", &[&property, &property], &property.name)
        .expect_err("duplicate property");
    assert_eq!(
        error.to_string(),
        "malformed container: product property LongProperty occurs more than once"
    );
}

#[test]
fn boolean_tokens_parse_without_a_lowercase_copy() {
    for (text, expected) in [
        ("true", Some(true)),
        ("TRUE", Some(true)),
        ("TrUe", Some(true)),
        ("1", Some(true)),
        ("false", Some(false)),
        ("FALSE", Some(false)),
        ("FaLsE", Some(false)),
        ("0", Some(false)),
        ("truex", None),
        ("1 ", None),
        ("Å", None),
    ] {
        assert_eq!(super::parse_bool(text), expected, "{text}");
    }
}

#[test]
fn link_targets_reject_absence_on_every_admission_route() {
    assert!(super::LinkTarget::try_new(None, None, vec![]).is_err());
    let wire =
        serde_json::json!({"document":null,"document_attribute":null,"object":"","subelements":[]});
    assert!(serde_json::from_value::<super::LinkTarget>(wire).is_err());
    let target = super::LinkTarget::try_new(None, None, vec!["Face1".into()]).unwrap();
    assert_eq!(
        serde_json::to_value(&target).unwrap(),
        serde_json::json!({"document":null,"document_attribute":null,"object":"","subelements":["Face1"]})
    );
}

#[test]
fn design_census_neutral_is_derived_and_checked_on_the_wire() {
    for (semantic_kind, neutral) in [("native", false), ("pattern", true)] {
        let wire = serde_json::json!({"id":"census", "object":"object", "type_name":"type", "feature":"feature", "semantic_kind":semantic_kind, "neutral":neutral, "post_processed":false});
        let record = serde_json::from_value::<super::DesignCensusRecord>(wire.clone()).unwrap();
        assert_eq!(record.neutral(), neutral);
        assert_eq!(serde_json::to_value(record).unwrap(), wire);
        let mut invalid = wire;
        invalid["neutral"] = serde_json::json!(!neutral);
        assert!(serde_json::from_value::<super::DesignCensusRecord>(invalid)
            .unwrap_err()
            .to_string()
            .contains("neutral"));
    }
}

#[test]
fn string_table_admission_requires_distinct_backward_references() {
    let entry = |id, components| serde_json::json!({"string_id":id,"flags":0,"components":components,"payload":"value","raw":"raw"});
    for (entries, valid) in [
        (vec![entry(1, vec![]), entry(2, vec![1])], true),
        (vec![entry(1, vec![]), entry(1, vec![])], false),
        (vec![entry(1, vec![1])], false),
        (vec![entry(1, vec![2]), entry(2, vec![])], false),
        (vec![entry(1, vec![9])], false),
    ] {
        let wire = serde_json::json!({
            "id": native_id("string-table", "0"),
            "index": 0,
            "owner_property": null,
            "save_all": false,
            "threshold": 0,
            "declared_count": entries.len(),
            "source_entry": null,
            "entries": entries
        });
        let result = serde_json::from_value::<super::StringTableRecord>(wire.clone());
        assert_eq!(result.is_ok(), valid);
        if let Ok(record) = result {
            // Compare source-only fields through the owning table layout.
            let mut fidelity = cadmpeg_ir::SourceFidelity::default();
            let entries = wire["entries"].as_array().unwrap();
            let bytes = entries
                .iter()
                .flat_map(|entry| entry["raw"].as_str().unwrap().as_bytes().iter().copied())
                .collect::<Vec<_>>();
            let arena = cadmpeg_core::decode::DecodeArena::new();
            let (ctx, root) = cadmpeg_core::decode::DecodeContext::from_root_bytes(
                &bytes,
                &arena,
                &cadmpeg_core::decode::DecodePolicy::service(),
            )
            .unwrap();
            ctx.record(&record.id(), root, || Ok(())).unwrap();
            fidelity
                .install_record_captures(&ctx, |_, layout| {
                    let mut start = 0;
                    for (ordinal, entry) in entries.iter().enumerate() {
                        let end = start + entry["raw"].as_str().unwrap().len();
                        layout.insert_payload(
                            format!("entries.{ordinal}.raw"),
                            cadmpeg_ir::native::bytes::NativeBytes::from(
                                root.child(start, end).unwrap().window().to_vec(),
                            ),
                        )?;
                        start = end;
                    }
                    Ok(())
                })
                .unwrap();
            let mut restored = serde_json::to_value(&record).unwrap();
            for (ordinal, entry) in restored["entries"]
                .as_array_mut()
                .unwrap()
                .iter_mut()
                .enumerate()
            {
                assert!(entry.get("raw").is_none());
                entry["raw"] = serde_json::json!(std::str::from_utf8(
                    fidelity.record_layout()[&record.id()].payloads()
                        [&format!("entries.{ordinal}.raw")]
                        .as_ref()
                )
                .unwrap());
            }
            assert_eq!(restored, wire);
        }
    }
}

#[test]
fn string_table_admission_derives_and_checks_identity_from_index() {
    let mut wire = serde_json::json!({
        "id": native_id("string-table", "0"),
        "index": 0,
        "owner_property": null,
        "save_all": false,
        "threshold": 0,
        "declared_count": 0,
        "source_entry": null,
        "entries": []
    });
    let table = serde_json::from_value::<super::StringTableRecord>(wire.clone()).unwrap();
    assert_eq!(table.index, 0);
    assert_eq!(table.id(), native_id("string-table", "0"));
    assert_eq!(serde_json::to_value(&table).unwrap(), wire);

    wire["id"] = serde_json::json!("fcstd:native:string-table#not-the-index");
    let error = serde_json::from_value::<super::StringTableRecord>(wire.clone()).unwrap_err();
    assert!(error.to_string().contains("string table id"));

    let mut ir = cadmpeg_ir::CadIr::empty();
    ir.native
        .namespace_mut("fcstd")
        .set_arena(
            &cadmpeg_test_support::service_decode_context(),
            "string_tables",
            &[wire],
        )
        .unwrap();
    let roundtrip: cadmpeg_ir::CadIr =
        serde_json::from_value(serde_json::to_value(&ir).unwrap()).unwrap();
    let findings = crate::test_support::validate_native(&roundtrip);
    assert!(findings.iter().any(|finding| {
        finding.message.contains("string table id")
            && finding.check == cadmpeg_ir::report::check::Check::NativeLinks
    }));
}

#[test]
fn string_tables_admit_numeric_positions_from_canonical_native_order() {
    let records = (0..12)
        .map(|index| {
            super::StringTableRecord::try_new(
                index,
                None,
                Some(false),
                Some(0),
                None,
                Some(Vec::new()),
            )
            .unwrap()
        })
        .collect::<Vec<_>>();
    let mut namespace = cadmpeg_ir::native::NativeNamespace::default();
    namespace
        .set_arena(
            &cadmpeg_test_support::service_decode_context(),
            "string_tables",
            &records,
        )
        .unwrap();
    assert_eq!(
        namespace.arenas()["string_tables"][2].id(),
        "fcstd:native:string-table#10"
    );
    let mut ordered = namespace
        .arena_as::<super::StringTableRecord>("string_tables")
        .unwrap();
    ordered.sort_by_key(|table| table.index);
    let tables = super::StringTables::try_from(ordered).unwrap();
    assert_eq!(
        tables
            .as_slice()
            .iter()
            .map(|table| table.index)
            .collect::<Vec<_>>(),
        (0..12).collect::<Vec<_>>()
    );
    let mut rewritten = cadmpeg_ir::native::NativeNamespace::default();
    rewritten
        .set_arena(
            &cadmpeg_test_support::service_decode_context(),
            "string_tables",
            tables.as_slice(),
        )
        .unwrap();
    assert_eq!(rewritten, namespace);
    let wire = serde_json::to_value(&tables).unwrap();
    assert!(wire.is_array());
    assert_eq!(
        serde_json::from_value::<super::StringTables>(wire).unwrap(),
        tables
    );
}

#[test]
fn string_tables_reject_duplicate_and_missing_numeric_positions() {
    for indices in [vec![1], vec![0, 0], vec![0, 2]] {
        let records = indices
            .into_iter()
            .map(|index| {
                super::StringTableRecord::try_new(
                    index,
                    None,
                    Some(false),
                    Some(0),
                    None,
                    Some(Vec::new()),
                )
                .unwrap()
            })
            .collect::<Vec<_>>();
        assert!(super::StringTables::try_from(records.clone()).is_err());
        let wire = serde_json::to_value(&records).unwrap();
        assert!(serde_json::from_value::<super::StringTables>(wire).is_err());
        let mut namespace = cadmpeg_ir::native::NativeNamespace::default();
        namespace
            .set_arena(
                &cadmpeg_test_support::service_decode_context(),
                "string_tables",
                &records,
            )
            .unwrap();
        assert!(namespace
            .arena_as_collection::<super::StringTableRecord, super::StringTables>("string_tables")
            .is_err());
    }
}

#[test]
fn ledger_spans_reject_empty_and_reversed_wire_intervals() {
    for (start, end, valid) in [(0, 1, true), (1, 1, false), (2, 1, false)] {
        let physical = serde_json::json!({"id":"span", "start":start, "end":end, "role":"end-record", "entry":null});
        let logical = serde_json::json!({"id":"span", "entry":"Document.xml", "start":start, "end":end, "classification":"structural", "owner":null});
        assert_eq!(
            serde_json::from_value::<cadmpeg_ir::source_fidelity::freecad::ArchiveSpan>(physical)
                .is_ok(),
            valid
        );
        assert_eq!(
            serde_json::from_value::<cadmpeg_ir::source_fidelity::freecad::LogicalSpan>(logical)
                .is_ok(),
            valid
        );
    }
}

#[test]
fn gui_state_order_is_derived_from_collection_position() {
    let mut state = serde_json::json!({"id":"camera", "kind":"Camera", "order":0, "attributes":{}, "values":[], "side_entries":[], "length":4, "sha256":cadmpeg_ir::hash::sha256_hex(b"<A/>")});
    assert!(
        serde_json::from_value::<super::GuiDocumentRecord>(serde_json::json!({
            "id":"gui", "schema_version":null, "attributes":{}, "states":[state.clone()]
        }))
        .is_err()
    );
    state.as_object_mut().unwrap().remove("length");
    state.as_object_mut().unwrap().remove("sha256");
    let mut wire = serde_json::json!({"id":"gui", "schema_version":null, "attributes":{}, "states":[state.clone(),state]});
    assert!(
        serde_json::from_value::<super::GuiDocumentRecord>(wire.clone())
            .unwrap_err()
            .to_string()
            .contains("order")
    );
    wire["states"][1]["order"] = serde_json::json!(1);
    let mut record = serde_json::from_value::<super::GuiDocumentRecord>(wire.clone()).unwrap();
    assert_eq!(serde_json::to_value(&record).unwrap(), wire);
    record.states.remove(0);
    let moved = serde_json::to_value(record).unwrap();
    assert_eq!(moved["states"][0]["order"], 0);
}

#[test]
fn attachment_accepts_accumulated_rigid_tolerance() {
    let mut rows = cadmpeg_ir::transform::Transform::identity().rows();
    rows[0][0] += 4.0e-10;
    let frame = super::FiniteFrame::try_from(rows).unwrap();
    let product = frame.transform().compose(frame.transform()).unwrap();
    assert!(!product.is_proper_rigid());
    assert!(super::AttachmentRecord::try_new(
        "attachment".into(),
        "object".into(),
        vec![],
        None,
        Some(frame),
        Some(frame)
    )
    .is_ok());
}

#[test]
fn attachment_and_product_wire_admission_reject_nonfinite_frames() {
    for bad in [f64::NAN, f64::INFINITY, f64::NEG_INFINITY] {
        let mut matrix = cadmpeg_ir::transform::Transform::identity().rows();
        matrix[0][3] = bad;
        for offset in [false, true] {
            let wire = super::AttachmentRecordWire {
                id: "attachment".into(),
                object: "object".into(),
                supports: vec![],
                map_mode: None,
                placement: (!offset).then_some(matrix),
                offset: offset.then_some(matrix),
                effective_frame: matrix,
            };
            let error = super::AttachmentRecord::try_from(wire).unwrap_err();
            assert!(error.contains(if offset { "offset" } else { "placement" }));
        }
        for field in [
            "element_transforms",
            "element_scales",
            "local_transform",
            "scale",
        ] {
            let mut wire: super::ProductNodeRecordWire = serde_json::from_value(serde_json::json!({
                "id":"link", "object":"object", "kind":"occurrence", "members":[],
                "element_transforms":[], "element_scales":[], "linked_subelements":[], "element_visibility":[], "element_objects":[]
            })).unwrap();
            match field {
                "element_transforms" => wire.element_transforms.push(matrix),
                "element_scales" => wire.element_scales.push([bad, 1.0, 1.0]),
                "local_transform" => wire.local_transform = Some(matrix),
                _ => wire.scale = Some([bad, 1.0, 1.0]),
            }
            assert!(super::ProductNodeRecord::try_from(wire)
                .unwrap_err()
                .contains(field));
        }
    }
}

#[test]
fn file_version_preserves_spelling_and_unverified_declarations() {
    let wire = serde_json::json!({"id":"document", "file_version":"+001", "program_version":null, "root_name":"Document", "object_count":0, "domains":[], "document_kind":"empty"});
    let record = serde_json::from_value::<super::DocumentFacts>(wire.clone()).unwrap();
    assert_eq!(record.file_version.value(), Some(1));
    assert_eq!(serde_json::to_value(record).unwrap(), wire);
    for spelling in ["-1", "", "abc", "184467440737095516160"] {
        let mut invalid = wire.clone();
        invalid["file_version"] = serde_json::json!(spelling);
        let record = serde_json::from_value::<super::DocumentFacts>(invalid.clone()).unwrap();
        assert_eq!(record.file_version.value(), None);
        assert_eq!(serde_json::to_value(record).unwrap(), invalid);
    }
}

#[test]
fn gui_provider_object_rejects_empty_wire_identity() {
    let mut wire =
        serde_json::json!({"id":"provider", "object":"", "name":"A", "expanded":null, "order":0});
    assert!(serde_json::from_value::<super::GuiViewProviderRecord>(wire.clone()).is_err());
    for object in [serde_json::Value::Null, serde_json::json!("object")] {
        wire["object"] = object;
        let record = serde_json::from_value::<super::GuiViewProviderRecord>(wire.clone()).unwrap();
        assert_eq!(serde_json::to_value(record).unwrap(), wire);
    }
}

#[test]
fn native_records_reject_retired_physical_xml_evidence_fields() {
    let bases = [
        serde_json::json!({"id":"state", "kind":"Camera", "order":0, "attributes":{}, "values":[], "side_entries":[]}),
        serde_json::json!({"id":"property", "owner":"provider", "name":"Color", "type_name":"App::PropertyColor", "order":0, "values":[], "side_entries":[]}),
        serde_json::json!({"id":"fcstd:native:object#A", "name":"A", "type_name":"App::Feature", "attributes":{}, "dependencies":[], "order":0}),
        serde_json::json!({"id":"property", "owner":"object", "name":"Label", "type_name":"App::PropertyString", "family":"scalar", "transient":false, "order":0, "values":[], "links":[], "side_entries":[]}),
    ];
    let hash = cadmpeg_ir::hash::sha256_hex(b"<A/>");
    for (kind, base) in bases.into_iter().enumerate() {
        let admit = |wire: serde_json::Value| match kind {
            0 => serde_json::from_value::<super::GuiDocumentRecord>(serde_json::json!({
                "id":"gui", "schema_version":null, "attributes":{}, "states":[wire]
            }))
            .map(|_| ()),
            1 => serde_json::from_value::<super::GuiPropertyRecord>(wire).map(|_| ()),
            2 => serde_json::from_value::<super::ObjectRecord>(wire).map(|_| ()),
            _ => serde_json::from_value::<super::PropertyRecord>(wire).map(|_| ()),
        };
        assert!(admit(base.clone()).is_ok(), "semantic base kind={kind}");
        for (length, digest) in [(4, hash.as_str()), (0, hash.as_str()), (4, "short")] {
            let mut wire = base.clone();
            let evidence = serde_json::json!({"length":length,"sha256":digest});
            if kind == 2 {
                wire["data_evidence"] = evidence;
            } else {
                wire.as_object_mut()
                    .unwrap()
                    .extend(evidence.as_object().unwrap().clone());
            }
            let admitted = admit(wire);
            assert!(
                admitted.is_err(),
                "retired physical fields: kind={kind}, evidence={length}/{digest}"
            );
        }
    }
}

#[test]
fn document_kind_wire_must_match_domains_and_count() {
    let mut wire = serde_json::json!({"id":"document",
        "file_version":"1", "program_version":null, "root_name":"Document",
        "object_count":1, "domains":["Part"], "document_kind":"empty"});
    assert!(serde_json::from_value::<super::DocumentFacts>(wire.clone()).is_err());
    wire["document_kind"] = serde_json::json!("part");
    let facts = serde_json::from_value::<super::DocumentFacts>(wire.clone()).unwrap();
    assert_eq!(serde_json::to_value(facts).unwrap(), wire);
}

#[test]
fn link_array_wire_rejects_negative_and_mismatched_counts() {
    let base = serde_json::json!({
        "id": "link", "object": "object", "kind": "occurrence",
        "members": [], "element_transforms": [], "element_scales": [],
        "linked_subelements": [], "element_visibility": [], "element_objects": []
    });
    let mut negative = base.clone();
    negative["element_count"] = serde_json::json!(-1);
    assert!(serde_json::from_value::<super::ProductNodeRecord>(negative).is_err());
    for field in [
        "element_transforms",
        "element_scales",
        "element_visibility",
        "element_objects",
    ] {
        let mut wire = base.clone();
        wire["element_count"] = serde_json::json!(2);
        wire[field] = match field {
            "element_transforms" => serde_json::json!([[
                [1.0, 0.0, 0.0, 0.0],
                [0.0, 1.0, 0.0, 0.0],
                [0.0, 0.0, 1.0, 0.0],
                [0.0, 0.0, 0.0, 1.0]
            ]]),
            "element_scales" => serde_json::json!([[1.0, 1.0, 1.0]]),
            "element_visibility" => serde_json::json!([true]),
            _ => serde_json::json!(["object"]),
        };
        assert!(serde_json::from_value::<super::ProductNodeRecord>(wire.clone()).is_err());
        wire["element_count"] = serde_json::json!(1);
        assert!(serde_json::from_value::<super::ProductNodeRecord>(wire).is_ok());
    }

    let mut object_only = base;
    object_only["element_objects"] = serde_json::json!(["object"]);
    let record = serde_json::from_value::<super::ProductNodeRecord>(object_only)
        .expect("an object carrier establishes absent-count cardinality");
    assert!(matches!(
        record.element_cardinality(),
        Some(super::LinkArrayCardinality::Elements(elements)) if elements.get() == 1
    ));
}

#[test]
fn copy_on_change_payload_requires_policy_on_wire() {
    for (field, value) in [
        (
            "copy_on_change_source",
            serde_json::json!({"document":null,"document_attribute":null,"object":"source","subelements":[]}),
        ),
        (
            "copy_on_change_group",
            serde_json::json!({"document":null,"document_attribute":null,"object":"group","subelements":[]}),
        ),
        ("copy_on_change_touched", serde_json::json!(false)),
    ] {
        let mut wire = serde_json::json!({
            "id": "link", "object": "object", "kind": "occurrence",
            "members": [], "element_transforms": [], "element_scales": [],
            "linked_subelements": [], "element_visibility": [], "element_objects": []
        });
        wire[field] = value;
        assert!(serde_json::from_value::<super::ProductNodeRecord>(wire.clone()).is_err());
        wire["copy_on_change"] = serde_json::json!("2");
        let admitted = serde_json::from_value::<super::ProductNodeRecord>(wire.clone()).unwrap();
        assert_eq!(serde_json::to_value(admitted).unwrap()[field], wire[field]);
    }

    for raw in ["Owned", "abc", ""] {
        let mut wire = serde_json::json!({
            "id": "link", "object": "object", "kind": "occurrence",
            "members": [], "element_transforms": [], "element_scales": [],
            "linked_subelements": [], "element_visibility": [], "element_objects": [],
            "copy_on_change": raw,
        });
        assert!(serde_json::from_value::<super::ProductNodeRecord>(wire.clone()).is_err());
        wire["copy_on_change"] = serde_json::json!("+00099");
        let admitted = serde_json::from_value::<super::ProductNodeRecord>(wire.clone())
            .expect("numeric future policies retain their spelling");
        assert_eq!(
            serde_json::to_value(admitted).unwrap()["copy_on_change"],
            "+00099"
        );
    }
}

#[test]
fn object_wire_rejects_nonpositive_partial_load_capability() {
    for value in [0, -1] {
        let wire = serde_json::json!({
            "id": "fcstd:native:object#A", "name": "A", "type_name": "App::Feature",
            "persistent_id": null, "view_type": null, "attributes": {},
            "dependencies": [], "dependency_allow_partial": value, "order": 0,
            "data_evidence": null
        });
        let error = serde_json::from_value::<super::ObjectRecord>(wire).unwrap_err();
        assert!(error.to_string().contains("dependency_allow_partial"));
    }
}

#[test]
fn canonical_ids_escape_names_without_aliasing_literal_escapes() {
    assert_eq!(
        native_id("object", "A B#C"),
        "fcstd:native:object#A%20B%23C"
    );
    assert_eq!(native_id("object", "A%20B"), "fcstd:native:object#A%2520B");
    assert_eq!(native_id("object", "A:B"), "fcstd:native:object#A%3AB");
    assert_eq!(native_id("object", ""), "fcstd:native:object#%EMPTY");
    let property = native_child_id("property", &native_id("object", "A B"), "Shape Value");
    assert_eq!(property, "fcstd:native:property#A%20B:Shape%20Value");
    assert_eq!(
        model_id("body", &property, "root#1"),
        "fcstd:model:body#A%20B:Shape%20Value:root%231"
    );
    assert_eq!(
        model_id("body", &property, ""),
        "fcstd:model:body#A%20B:Shape%20Value:"
    );
}

/// Identity frame shared by the written-form tests below.
fn identity() -> [[f64; 4]; 4] {
    cadmpeg_ir::transform::Transform::identity().rows()
}

/// A link target wire object naming one object and its subelements.
fn link(object: &str, subelements: &[&str]) -> serde_json::Value {
    serde_json::json!({
        "document": null,
        "document_attribute": null,
        "object": object,
        "subelements": subelements
    })
}

#[test]
fn an_attachment_writes_its_supports_frames_and_effective_frame() {
    let wire = serde_json::json!({
        "id": "attachment",
        "object": "object",
        "supports": [null, link("Pad", &["Face1"])],
        "map_mode": "3",
        "placement": identity(),
        "offset": identity(),
        "effective_frame": identity()
    });
    let record = serde_json::from_value::<super::AttachmentRecord>(wire.clone()).unwrap();
    assert_eq!(serde_json::to_value(&record).unwrap(), wire);

    let mut bare = wire;
    bare["supports"] = serde_json::json!([]);
    bare["map_mode"] = serde_json::Value::Null;
    bare["placement"] = serde_json::Value::Null;
    bare["offset"] = serde_json::Value::Null;
    let record = serde_json::from_value::<super::AttachmentRecord>(bare.clone()).unwrap();
    assert_eq!(serde_json::to_value(record).unwrap(), bare);
}

#[test]
fn archive_and_logical_spans_write_their_role_and_owner() {
    for (role, entry) in [
        ("compressed-payload", serde_json::json!("Document.xml")),
        ("end-record", serde_json::Value::Null),
    ] {
        let wire = serde_json::json!({
            "id": "span", "start": 0, "end": 4, "role": role, "entry": entry
        });
        let record = serde_json::from_value::<cadmpeg_ir::source_fidelity::freecad::ArchiveSpan>(
            wire.clone(),
        )
        .unwrap();
        assert_eq!(serde_json::to_value(record).unwrap(), wire);
    }

    for (classification, owner) in [
        ("structural", serde_json::Value::Null),
        ("typed", serde_json::json!("fcstd:native:object#A")),
        (
            "named_opaque",
            serde_json::json!("fcstd:native:property#A:Shape"),
        ),
    ] {
        let wire = serde_json::json!({
            "id": "span", "entry": "Document.xml", "start": 0, "end": 4,
            "classification": classification, "owner": owner
        });
        let record = serde_json::from_value::<cadmpeg_ir::source_fidelity::freecad::LogicalSpan>(
            wire.clone(),
        )
        .unwrap();
        assert_eq!(serde_json::to_value(record).unwrap(), wire);
    }
}

#[test]
fn a_drawing_record_writes_page_views_and_a_non_page_without_them() {
    let base = serde_json::json!({
        "id": "drawing", "object": "object",
        "kind": "TechDraw::DrawPage",
        "views": ["fcstd:native:object#View"],
        "template": "fcstd:native:object#Template",
        "sources": [null, link("Body", &["Face2"])],
        "relationships": {"Source": [link("Body", &[])]},
        "parameters": {"Scale": "1.0"},
        "side_entries": ["template.svg"]
    });
    let record = serde_json::from_value::<super::DrawingRecord>(base.clone()).unwrap();
    assert_eq!(serde_json::to_value(record).unwrap(), base);

    let mut other = base;
    other["kind"] = serde_json::json!("TechDraw::DrawViewPart");
    other["views"] = serde_json::json!([]);
    other["template"] = serde_json::Value::Null;
    let record = serde_json::from_value::<super::DrawingRecord>(other.clone()).unwrap();
    assert_eq!(serde_json::to_value(record).unwrap(), other);
}

#[test]
fn an_object_record_writes_xml_evidence_and_its_absence() {
    let mut wire = serde_json::json!({
        "id": "fcstd:native:object#A", "name": "A", "type_name": "App::Feature",
        "persistent_id": 7, "view_type": "Gui::ViewProviderFeature",
        "attributes": {"Touched": "1"},
        "dependencies": ["fcstd:native:object#B"],
        "dependency_allow_partial": 2, "order": 0,
        "data_evidence": {"length":4,"sha256":cadmpeg_ir::hash::sha256_hex(b"<A/>")}
    });
    assert!(serde_json::from_value::<super::ObjectRecord>(wire.clone()).is_err());
    wire.as_object_mut().unwrap().remove("data_evidence");
    let record = serde_json::from_value::<super::ObjectRecord>(wire.clone()).unwrap();
    assert_eq!(serde_json::to_value(record).unwrap(), wire);

    let mut bare = wire;
    bare["persistent_id"] = serde_json::Value::Null;
    bare["view_type"] = serde_json::Value::Null;
    bare["dependency_allow_partial"] = serde_json::Value::Null;
    let mut legacy_bare = bare.clone();
    legacy_bare["data_evidence"] = serde_json::Value::Null;
    assert!(serde_json::from_value::<super::ObjectRecord>(legacy_bare).is_err());
    let record = serde_json::from_value::<super::ObjectRecord>(bare.clone()).unwrap();
    assert_eq!(serde_json::to_value(record).unwrap(), bare);
}

#[test]
fn a_property_record_writes_its_payload_and_a_transient_declaration_without_one() {
    let mut wire = serde_json::json!({
        "id": "property", "owner": "object", "name": "Label",
        "type_name": "App::PropertyString", "family": "string", "status": 1,
        "transient": false,
        "dynamic": {"group": "Base", "documentation": "label", "attributes": 0,
                    "read_only": false, "hidden": null},
        "order": 2,
        "values": [{"tag": "String", "order": 0, "parent": null, "attributes": {"value": "A"},
                    "text": null}],
        "links": [null, link("B", &["Face1"])],
        "side_entries": ["Body.brp"],
        "length": 4, "sha256": cadmpeg_ir::hash::sha256_hex(b"<A/>")
    });
    assert!(serde_json::from_value::<super::PropertyRecord>(wire.clone()).is_err());
    wire.as_object_mut().unwrap().remove("length");
    wire.as_object_mut().unwrap().remove("sha256");
    let record = serde_json::from_value::<super::PropertyRecord>(wire.clone()).unwrap();
    assert_eq!(serde_json::to_value(record).unwrap(), wire);

    let mut transient = wire;
    transient["transient"] = serde_json::json!(true);
    transient["dynamic"] = serde_json::Value::Null;
    transient["values"] = serde_json::json!([]);
    transient["links"] = serde_json::json!([]);
    transient["side_entries"] = serde_json::json!([]);
    let record = serde_json::from_value::<super::PropertyRecord>(transient.clone()).unwrap();
    assert_eq!(serde_json::to_value(record).unwrap(), transient);
}

#[test]
fn an_entry_record_writes_evidence_without_opaque_bytes() {
    let mut wire = serde_json::json!({
        "id": "fcstd:native:entry#Document.xml",
        "name": "Document.xml",
        "role": "document",
        "byte_len": 3,
        "sha256": "039058c6f2c0cb492c533b0a4d14ef77cc0f78abccced5287d84a1a2011cfb81",
        "referenced_by": ["fcstd:native:property#A:Shape"]
    });
    assert!(serde_json::from_value::<super::EntryRecord>(wire.clone()).is_err());
    wire.as_object_mut().unwrap().remove("byte_len");
    wire.as_object_mut().unwrap().remove("sha256");
    let record = serde_json::from_value::<super::EntryRecord>(wire.clone()).unwrap();
    assert_eq!(serde_json::to_value(&record).unwrap(), wire);

    let json = serde_json::to_string(&record).unwrap();
    assert_eq!(
        serde_json::from_str::<super::EntryRecord>(&json).unwrap(),
        record
    );

    let mut empty = wire;
    empty["byte_len"] = serde_json::json!(0);
    empty["sha256"] =
        serde_json::json!("e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855");
    empty["referenced_by"] = serde_json::json!([]);
    assert!(serde_json::from_value::<super::EntryRecord>(empty.clone()).is_err());
    empty.as_object_mut().unwrap().remove("byte_len");
    empty.as_object_mut().unwrap().remove("sha256");
    let record = serde_json::from_value::<super::EntryRecord>(empty.clone()).unwrap();
    assert_eq!(serde_json::to_value(record).unwrap(), empty);
}

#[test]
fn entry_evidence_rejects_invalid_digests_without_needing_the_payload() {
    let record = crate::test_support::entry_fixture(
        "fcstd:native:entry#Payload.bin".into(),
        "Payload.bin".into(),
        cadmpeg_core::container::ContainerRole::Auxiliary,
        Vec::new(),
        vec![0xab],
    );
    let fidelity = crate::test_support::source_fidelity::entry_layout_fixture(&record);
    let source = fidelity.retained_record(record.id()).unwrap();
    let id = record.id().to_owned();
    let wire = serde_json::to_value(record.record).unwrap();
    assert!(wire.get("data").is_none());
    for digest in ["a", "ag", "AB", &"A".repeat(64)] {
        let mut source_evidence =
            serde_json::to_value(cadmpeg_ir::source_fidelity::SourceEvidence::of(&[0xab])).unwrap();
        source_evidence["sha256"] = serde_json::json!(digest);
        assert!(
            serde_json::from_value::<cadmpeg_ir::source_fidelity::SourceEvidence>(source_evidence)
                .is_err()
        );
        let mut invalid = wire.clone();
        invalid["sha256"] = serde_json::json!(digest);
        assert!(serde_json::from_value::<super::EntryRecord>(invalid)
            .unwrap_err()
            .to_string()
            .contains("sha256"));
    }
    let record: super::EntryRecord = serde_json::from_value(wire).unwrap();
    assert_eq!(fidelity.record_layout()[&id].byte_len(), 1);
    assert_eq!(source.sha256(), cadmpeg_ir::hash::sha256_hex(&[0xab]));
    assert!(serde_json::to_value(record).unwrap().get("data").is_none());
}

#[test]
fn entry_wire_keeps_evidence_and_references_without_payload_bytes() {
    let bytes: Vec<u8> = (0..=255).cycle().take(256 * 1024).collect();
    let record = crate::test_support::entry_fixture(
        "fcstd:native:entry#Payload.bin".into(),
        "Payload.bin".into(),
        cadmpeg_core::container::ContainerRole::Auxiliary,
        Vec::new(),
        bytes.clone(),
    );
    let fidelity = crate::test_support::source_fidelity::entry_layout_fixture(&record);
    let source = fidelity.retained_record(record.id()).unwrap();
    assert_eq!(source.data().unwrap(), bytes);
    let arena = cadmpeg_core::decode::DecodeArena::new();
    let policy = cadmpeg_core::decode::DecodePolicy::service();
    let (ctx, _) =
        cadmpeg_core::decode::DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    let mut namespace = cadmpeg_ir::native::NativeNamespace::default();
    namespace
        .set_arena(&ctx, "entries", &[record.record])
        .expect("entry arena");
    let records = namespace.arena_as::<super::EntryRecord>("entries").unwrap();
    assert!(serde_json::to_value(&records[0])
        .unwrap()
        .get("data")
        .is_none());
    assert_eq!(
        fidelity.record_layout()[records[0].id()].byte_len(),
        u64::try_from(bytes.len()).unwrap()
    );
    assert_eq!(source.sha256(), cadmpeg_ir::hash::sha256_hex(&bytes));
    let wire = namespace.arena_as::<serde_json::Value>("entries").unwrap();
    assert!(wire[0].get("data").is_none());
    assert_eq!(source.byte_len(), u64::try_from(bytes.len()).unwrap());
    assert!(wire[0].get("byte_len").is_none());
    assert!(wire[0].get("sha256").is_none());
}

#[test]
fn a_product_node_writes_occurrence_fields_and_a_container_without_them() {
    let occurrence = serde_json::json!({
        "id": "link", "object": "object", "kind": "occurrence",
        "members": ["fcstd:native:object#Child"],
        "prototype": "fcstd:native:object#Proto",
        "external_document": "Other.FCStd",
        "external_document_attribute": "file",
        "local_transform": identity(),
        "placement_property": "Placement",
        "element_count": 1,
        "link_transform": true,
        "element_transforms": [identity()],
        "element_scales": [[2.0, 2.0, 2.0]],
        "linked_subelements": ["Face1"],
        "claim_child": false,
        "copy_on_change": "2",
        "copy_on_change_source": link("Source", &[]),
        "copy_on_change_group": link("Group", &[]),
        "copy_on_change_touched": true,
        "scale": [3.0, 3.0, 3.0],
        "element_visibility": [true],
        "element_objects": ["fcstd:native:object#Element"]
    });
    let record = serde_json::from_value::<super::ProductNodeRecord>(occurrence.clone()).unwrap();
    assert_eq!(serde_json::to_value(record).unwrap(), occurrence);

    let container = serde_json::json!({
        "id": "group", "object": "object", "kind": "group",
        "members": ["fcstd:native:object#Child"],
        "prototype": null,
        "external_document": null,
        "external_document_attribute": null,
        "local_transform": identity(),
        "placement_property": "Placement",
        "element_count": null,
        "link_transform": null,
        "element_transforms": [],
        "element_scales": [],
        "linked_subelements": [],
        "claim_child": null,
        "copy_on_change": null,
        "copy_on_change_source": null,
        "copy_on_change_group": null,
        "copy_on_change_touched": null,
        "scale": null,
        "element_visibility": [],
        "element_objects": []
    });
    let record = serde_json::from_value::<super::ProductNodeRecord>(container.clone()).unwrap();
    assert_eq!(serde_json::to_value(record).unwrap(), container);
}

mod provenance;
