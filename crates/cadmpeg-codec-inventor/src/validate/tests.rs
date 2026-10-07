// SPDX-License-Identifier: Apache-2.0
use super::{
    equal_btree_sets, validate_active_carrier, validate_design, validate_sketches, NativeData,
};
use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
use cadmpeg_core::CodecError;
use std::collections::{BTreeMap, BTreeSet};

#[test]
fn ordered_set_equality_matches_contents_across_insertion_order() {
    let arena = DecodeArena::new();
    let (ctx, _) =
        DecodeContext::from_root_bytes(&[], &arena, &DecodePolicy::service()).expect("context");
    let left = BTreeSet::from(["north", "south"]);
    let equal = BTreeSet::from(["south", "north"]);
    let different = BTreeSet::from(["north", "east"]);

    assert!(equal_btree_sets(&ctx, &left, &equal, "compare test sets").expect("equal sets"));
    assert!(
        !equal_btree_sets(&ctx, &left, &different, "compare test sets").expect("different sets")
    );
}

#[test]
fn ordered_set_equality_propagates_work_refusal() {
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_work_units = 0;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("context");
    let left = BTreeSet::from(["north"]);
    let right = BTreeSet::from(["north"]);

    assert!(matches!(
        equal_btree_sets(&ctx, &left, &right, "compare test sets"),
        Err(CodecError::ResourceLimit(limit))
            if limit.dimension == ResourceDimension::WorkUnits
                && limit.operation == "compare test sets"
    ));
}

fn empty_native_data() -> NativeData {
    NativeData {
        storage_bands: Vec::new(),
        databases: Vec::new(),
        database_issues: Vec::new(),
        registry: Vec::new(),
        revisions: Vec::new(),
        pairs: Vec::new(),
        metadata: Vec::new(),
        meta_sections: Vec::new(),
        meta_types: Vec::new(),
        metadata_issues: Vec::new(),
        bulk: Vec::new(),
        records: Vec::new(),
        bulk_issues: Vec::new(),
        unpaired: Vec::new(),
        structural_issues: Vec::new(),
        property_sets: Vec::new(),
        property_sections: Vec::new(),
        properties: Vec::new(),
        property_issues: Vec::new(),
        protein_assets: Vec::new(),
        protein_rejections: Vec::new(),
        assembly_occurrences: Vec::new(),
        assembly_placements: Vec::new(),
        assembly_record_issues: Vec::new(),
        pm_app_default_styles: Vec::new(),
        pm_app_rendering_styles: Vec::new(),
        pm_graphics_faces: Vec::new(),
        pm_graphics_style_collections: Vec::new(),
        pm_graphics_primary_color_styles: Vec::new(),
        face_native_keys: Vec::new(),
        presentation_record_issues: Vec::new(),
        pm_dc_parameters: Vec::new(),
        pm_dc_expressions: Vec::new(),
        pm_dc_units: Vec::new(),
        design_record_issues: Vec::new(),
        pm_dc_sketches: Vec::new(),
        pm_dc_sketch_entities: Vec::new(),
        pm_dc_sketch_constraints: Vec::new(),
        pm_dc_transforms: Vec::new(),
        pm_dc_directions: Vec::new(),
        sketch_record_issues: Vec::new(),
        pm_dc_features: Vec::new(),
        pm_dc_pattern_features: Vec::new(),
        pm_dc_feature_terminators: Vec::new(),
        pm_dc_feature_properties: Vec::new(),
        pm_dc_feature_labels: Vec::new(),
        pm_dc_entity_style_links: Vec::new(),
        feature_record_issues: Vec::new(),
        unknowns: Vec::new(),
        protein: super::ProteinRecord::Absent {
            id: "protein".into(),
        },
        ufrx: super::UfrxRecord::Absent { id: "ufrx".into() },
        active_carrier: super::ActiveCarrierRecord::NotApplicable {
            id: "carrier".into(),
        },
    }
}

fn carrier_record() -> super::RseRecordRecord {
    serde_json::from_value(serde_json::json!({
        "id": "record", "token": "t", "ordinal": 0, "selector": 0,
        "type_index": 0, "type_id": "5c5945f6d5113313100060a6bba647b5",
        "payload_offset": 0, "payload_len": 0, "payload_sha256": "",
        "trailing_payload_len": 0, "trailer_len": 0, "trailer_sha256": ""
    }))
    .expect("record")
}

#[test]
fn active_carrier_search_charges_only_visited_records() {
    let mut data = empty_native_data();
    data.records = (0..100).map(|_| carrier_record()).collect();
    data.active_carrier = selected_carrier_record();
    // One source step and two one-byte tokens: 1 + 1 + 1 = 3. Type
    // equality checks length first and compares at most the fixed 32-byte GUID.
    let need = 3;
    for budget in [need, need - 1] {
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        policy.limits.max_work_units = budget;
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("context");
        let mut findings = Vec::new();
        let result = validate_active_carrier(&ctx, &data, &mut findings);
        if budget == need {
            result.expect("first record resolves within exact budget");
            assert!(findings.is_empty());
            assert!(matches!(ctx.charge_work(1, "probe"),
                Err(CodecError::ResourceLimit(limit)) if limit.used == need));
        } else {
            assert!(matches!(result, Err(CodecError::ResourceLimit(limit))
                if limit.dimension == ResourceDimension::WorkUnits
                    && limit.operation == "compare Inventor active-carrier segment token"
                    && limit.used == 2 && limit.additional == 1));
        }
    }
}

#[test]
fn design_record_collector_charges_one_source_traversal() {
    let mut data = empty_native_data();
    data.records.push(carrier_record());
    // Two source steps (record and end probe), two hashes of the one-byte
    // token plus four-byte ordinal, and growth for four map buckets with
    // alignment padding and control bytes: 2 + 2 * 5 + 4 * slot_size + 15 + 4 + 16.
    let need = 12
        + cadmpeg_core::decode::u64_from_index(
            4 * std::mem::size_of::<((&str, u32), &str)>() + 15 + 4 + 16,
        );
    for budget in [need, need - 1] {
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        policy.limits.max_work_units = budget;
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("context");
        let mut findings = Vec::new();
        let result = validate_design(&ctx, &data, &cadmpeg_ir::CadIr::empty(), &mut findings);
        if budget == need {
            result.expect("one record index within exact budget");
            assert!(findings.is_empty());
            assert!(matches!(ctx.charge_work(1, "probe"),
                Err(CodecError::ResourceLimit(limit)) if limit.used == need));
        } else {
            assert!(matches!(result, Err(CodecError::ResourceLimit(limit))
                if limit.dimension == ResourceDimension::WorkUnits
                    && limit.operation == "collect Inventor RSe design record index"
                    && limit.used == need - 1 && limit.additional == 1));
        }
    }
}

#[test]
fn sketch_endpoint_search_admits_the_first_lookup_before_refusal() {
    let data = empty_native_data();
    let mut ir = cadmpeg_ir::CadIr::empty();
    let entity = cadmpeg_ir::sketches::SketchEntity::new(
        cadmpeg_ir::sketches::SketchEntityId::mint("inventor:test:entity#1").expect("entity id"),
        cadmpeg_ir::sketches::SketchId::mint("inventor:test:sketch#1").expect("sketch id"),
        cadmpeg_ir::sketches::SketchGeometry::try_from(
            cadmpeg_ir::sketches::SketchGeometryDefinition::Point {
                position: cadmpeg_ir::math::Point2::new(0.0, 0.0),
            },
        )
        .expect("point geometry"),
    )
    .with_endpoint_refs((0..100).map(|_| "x".to_owned()).collect());
    ir.model.sketch_entities.push(entity);
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    // Empty record-index end probe + neutral entity visit + first endpoint step = 3.
    // The next charge is the one-byte endpoint key; the other 99 endpoints are unvisited.
    policy.limits.max_work_units = 3;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("context");
    let mut findings = Vec::new();
    assert!(matches!(validate_sketches(&ctx, &data, &ir, &mut findings),
        Err(CodecError::ResourceLimit(limit))
            if limit.dimension == ResourceDimension::WorkUnits
                && limit.operation == "resolve Inventor neutral endpoint source"
                && limit.used == 3 && limit.additional == 1));
    assert!(findings.is_empty());

    let (ctx, _) =
        DecodeContext::from_root_bytes(&[], &arena, &DecodePolicy::service()).expect("context");
    validate_sketches(&ctx, &data, &ir, &mut findings).expect("unresolved endpoint finding");
    assert_eq!(findings.len(), 1);
    assert_eq!(
        findings[0].check,
        cadmpeg_ir::report::check::Check::NativeLinks
    );
}

#[test]
fn expression_references_validate_without_collection_slots() {
    for kind in [
        serde_json::json!({"form": "value", "value": 0.0, "value_type": 0, "state": 0}),
        serde_json::json!({"form": "parameter_reference", "operand": {"index": 1, "qualified": false}}),
        serde_json::json!({"form": "unary", "operation": "negate", "operand": {"index": 1, "qualified": false}}),
        serde_json::json!({"form": "binary", "operation": "add", "left": {"index": 1, "qualified": false}, "right": {"index": 1, "qualified": false}}),
    ] {
        let mut data = empty_native_data();
        let raw = carrier_record();
        let payload =
            serde_json::from_value::<crate::design::PmDcExpressionPayload>(serde_json::json!({
                "save_version_major": 1, "header_value": 0, "header_id": 0,
                "unit": {"index": 1, "qualified": false}, "kind": kind
            }))
            .expect("expression payload");
        data.pm_dc_expressions.push(super::Located::new(
            payload,
            crate::record_identity::RecordTypeId::try_from(raw.type_id.as_str().to_owned())
                .expect("type id"),
            cadmpeg_ir::ids::IdentityKey::encode_segment("t"),
            0,
        ));
        data.records.push(raw);
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        // One raw-record index slot and one expression uniqueness slot. References
        // stay in their record and allocate no collection slots.
        policy.limits.max_collection_items = 2;
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("context");
        let mut findings = Vec::new();
        validate_design(&ctx, &data, &cadmpeg_ir::CadIr::empty(), &mut findings)
            .expect("references resolve");
        assert!(findings.is_empty());
    }
}

fn located_payload<T: serde::de::DeserializeOwned>(
    payload: serde_json::Value,
) -> super::Located<T> {
    super::Located::new(
        serde_json::from_value(payload).expect("payload"),
        crate::record_identity::RecordTypeId::try_from(
            carrier_record().type_id.as_str().to_owned(),
        )
        .expect("type id"),
        cadmpeg_ir::ids::IdentityKey::encode_segment("t"),
        0,
    )
}

fn content_header() -> serde_json::Value {
    serde_json::json!({
        "header_value": 0, "header_id": 0, "next": {"index": 1, "qualified": false},
        "flags": 0, "context": {"index": 1, "qualified": false}, "source_index": 0
    })
}

fn reference_list(index: u32) -> serde_json::Value {
    serde_json::json!({
        "marker": 8, "metadata": {"width": "u16", "values": [0, 0]},
        "references": [{"index": index, "qualified": false}]
    })
}

#[test]
fn sketch_entity_references_validate_in_place_and_stop_at_failure() {
    for index in [1, 2] {
        let mut data = empty_native_data();
        data.records.push(carrier_record());
        data.pm_dc_sketch_entities
            .push(located_payload(serde_json::json!({
                "save_version_major": 1, "header": content_header(), "entity_flags": 0,
                "sketch": {"index": 1, "qualified": false},
                "kind": {"form": "line", "points": reference_list(index),
                    "auxiliary": [reference_list(1)], "origin": [0.0, 0.0], "direction": [1.0, 0.0]}
            })));
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        // Raw-record index, entity uniqueness and native-entity identity: three
        // slots. A failed reference adds one finding; no reference copies exist.
        policy.limits.max_collection_items = 3 + u64::from(index == 2);
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("context");
        let mut findings = Vec::new();
        validate_sketches(&ctx, &data, &cadmpeg_ir::CadIr::empty(), &mut findings)
            .expect("entity validation");
        assert_eq!(findings.len(), usize::from(index == 2));
        if let Some(finding) = findings.first() {
            assert_eq!(
                finding.message,
                "Inventor PmDc sketch-entity record or reference does not resolve"
            );
        }
    }
}

#[test]
fn sketch_entity_failure_skips_large_auxiliary_reference_list() {
    let mut data = empty_native_data();
    data.records.push(carrier_record());
    let mut auxiliary = reference_list(1);
    auxiliary["references"] = serde_json::json!((0..10_000)
        .map(|_| serde_json::json!({"index": 1, "qualified": false}))
        .collect::<Vec<_>>());
    data.pm_dc_sketch_entities
        .push(located_payload(serde_json::json!({
            "save_version_major": 1, "header": content_header(), "entity_flags": 0,
            "sketch": {"index": 1, "qualified": false},
            "kind": {"form": "line", "points": reference_list(2),
                "auxiliary": [auxiliary], "origin": [0.0, 0.0], "direction": [1.0, 0.0]}
        })));
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    // One raw index, one uniqueness key, one canonical identity and one finding.
    policy.limits.max_collection_items = 4;
    // Indexes, the failed point lookup and the finding fit here. Visiting all
    // 10,000 auxiliary references would exceed this allowance before lookups.
    policy.limits.max_work_units = 10_000;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("context");
    let mut findings = Vec::new();
    validate_sketches(&ctx, &data, &cadmpeg_ir::CadIr::empty(), &mut findings)
        .expect("failed point skips auxiliary references");
    assert_eq!(findings.len(), 1);
    assert_eq!(
        findings[0].message,
        "Inventor PmDc sketch-entity record or reference does not resolve"
    );
}

#[test]
fn sketch_constraint_reference_maps_validate_without_copies() {
    for index in [1, 2] {
        let mut data = empty_native_data();
        data.records.push(carrier_record());
        let reference = serde_json::json!({"index": 1, "qualified": false});
        data.pm_dc_sketch_constraints.push(located_payload(serde_json::json!({
            "save_version_major": 1,
            "header": {"content": content_header(), "state": 0, "group": reference,
                "parameter": reference,
                "scalar_map": {"metadata": [0, 0], "entries": [[reference, 0.0]]},
                "reference_map": {"metadata": [0, 0], "entries": [[reference, {"index": index, "qualified": false}]]}},
            "kind": {"form": "coincident", "first": reference, "second": reference}
        })));
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        // Raw-record index, constraint uniqueness and native-constraint identity:
        // three slots. A failed map value adds one finding slot.
        policy.limits.max_collection_items = 3 + u64::from(index == 2);
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("context");
        let mut findings = Vec::new();
        validate_sketches(&ctx, &data, &cadmpeg_ir::CadIr::empty(), &mut findings)
            .expect("constraint validation");
        assert_eq!(findings.len(), usize::from(index == 2));
        if let Some(finding) = findings.first() {
            assert_eq!(
                finding.message,
                "Inventor PmDc sketch-constraint record or reference does not resolve"
            );
        }
    }
}

#[test]
fn feature_property_references_validate_without_copies() {
    for index in [1, 2] {
        let mut data = empty_native_data();
        data.records.push(carrier_record());
        data.pm_dc_feature_properties.push(located_payload(serde_json::json!({
            "save_version_major": 1, "header": content_header(),
            "kind": {"form": "references", "family": "object_collection", "items": reference_list(index)}
        })));
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        // Raw-record index, property uniqueness and two property indexes: four
        // slots. A failed list reference adds one finding slot.
        policy.limits.max_collection_items = 4 + u64::from(index == 2);
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("context");
        let mut findings = Vec::new();
        super::validate_features(&ctx, &cadmpeg_ir::CadIr::empty(), &data, &mut findings)
            .expect("property validation");
        assert_eq!(findings.len(), usize::from(index == 2));
        if let Some(finding) = findings.first() {
            assert_eq!(
                finding.message,
                "Inventor PmDc feature-property record or reference does not resolve"
            );
        }
    }
}

fn ufrx_payload() -> crate::native::ufrx::UfrxParsedPrefix {
    crate::native::ufrx::UfrxParsedPrefix {
        id: "inventor:ufrx:state#root".into(),
        directory_id: 0,
        schema: 1,
        section_versions: Vec::new(),
        original_file_name: String::new(),
        caption: String::new(),
        representation: None,
        model_states: Vec::new(),
        external_references: Vec::new(),
        embedded_references: Vec::new(),
        occurrences: Vec::new(),
        tail_len: 0,
        tail_sha256: cadmpeg_ir::hash::digest::Sha256Digest::try_from("0".repeat(64))
            .expect("digest"),
    }
}

#[test]
fn ufrx_contiguity_checks_distinct_ordinals_without_expected_set() {
    for (ordinals, expected_findings) in [([1, 0], 0), ([0, 2], 1), ([0, 0], 2)] {
        let mut data = empty_native_data();
        let mut payload = ufrx_payload();
        let setup = cadmpeg_test_support::service_decode_context();
        for ordinal in ordinals {
            payload.model_states.push(
                crate::native::ufrx::UfrxModelStateRecordWire {
                    id: format!("inventor:ufrx:model-state#{ordinal}"),
                    ordinal,
                    prefix: 0,
                    name: "state".into(),
                    state: [0, 0],
                    prefix_count: 0,
                    parameters: Vec::new(),
                    suffix_len: 77,
                    suffix_sha256: "0".repeat(64),
                }
                .into_record(&setup)
                .expect("model state"),
            );
        }
        data.ufrx = super::UfrxRecord::ParsedPrefix(Box::new(payload));
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        // Valid distinct ordinals need two uniqueness slots. A gap adds one
        // finding: three slots. A repeated ordinal needs one uniqueness slot
        // and two findings: three slots. Contiguity reuses the uniqueness set.
        policy.limits.max_collection_items = 2 + u64::from(expected_findings != 0);
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("context");
        let mut findings = Vec::new();
        super::validate_ufrx(&ctx, &cadmpeg_ir::CadIr::empty(), &data, &mut findings)
            .expect("ordinal validation");
        assert_eq!(findings.len(), expected_findings);
        assert_eq!(
            findings
                .iter()
                .filter(|finding| finding.message
                    == "Inventor UFRxDoc model-state ordinals are not contiguous")
                .count(),
            usize::from(expected_findings != 0)
        );
    }
}

#[test]
fn assembly_validation_releases_temporary_projection_storage() {
    let mut data = empty_native_data();
    let mut payload = ufrx_payload();
    let setup = cadmpeg_test_support::service_decode_context();
    let wire = serde_json::from_value::<crate::native::ufrx::UfrxOccurrenceRecordWire>(serde_json::json!({
        "id": "inventor:ufrx:occurrence#0", "ordinal": 0, "end_string_flag": 0,
        "file_reference_id": 1, "occurrence_id": 1, "header_value": 0,
        "title": null, "header_padding_words": 0, "record_len": 1, "record_sha256": "0".repeat(64)
    })).expect("occurrence wire");
    payload
        .occurrences
        .push(wire.into_record(&setup).expect("occurrence"));
    data.ufrx = super::UfrxRecord::ParsedPrefix(Box::new(payload));
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    // Keep the policy ceiling below the input-proportional allowance.
    policy.limits.max_materialized_bytes = 4_096;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("context");
    let mut findings = Vec::new();
    super::validate_assembly(&ctx, &cadmpeg_ir::CadIr::empty(), &data, &mut findings)
        .expect("assembly validation");
    assert!(findings.is_empty());
    // The unresolved-cause table belongs to the temporary projection. All of
    // its storage is released, so the full allowance is available again.
    ctx.reserve_scoped(
        policy.limits.max_materialized_bytes,
        "probe released projection",
    )
    .expect("projection storage released");
}

#[test]
fn feature_result_bodies_compare_in_order_without_id_copies() {
    let overlong_body = "x".repeat(100_000);
    for (references, bodies, expected_findings) in [
        (
            [2, 3],
            [
                "inventor:pmdc:feature-property#t-1",
                "inventor:pmdc:feature-property#t-2",
            ],
            0,
        ),
        (
            [2, 3],
            [
                "inventor:pmdc:feature-property#t-2",
                "inventor:pmdc:feature-property#t-1",
            ],
            1,
        ),
        (
            [2, 3],
            [
                "inventor:pmdc:feature-property#t-0",
                "inventor:pmdc:feature-property#t-2",
            ],
            1,
        ),
        (
            [2, 3],
            [
                "inventor:pmdc:feature-property#t-01",
                "inventor:pmdc:feature-property#t-2",
            ],
            1,
        ),
        (
            [2, 3],
            [overlong_body.as_str(), "inventor:pmdc:feature-property#t-2"],
            1,
        ),
        (
            [0, 3],
            [
                "inventor:pmdc:feature-property#t-1",
                "inventor:pmdc:feature-property#t-2",
            ],
            1,
        ),
        (
            [4, 3],
            [
                "inventor:pmdc:feature-property#t-1",
                "inventor:pmdc:feature-property#t-2",
            ],
            2,
        ),
    ] {
        let mut data = empty_native_data();
        for ordinal in 0..3 {
            let mut raw = carrier_record();
            raw.ordinal = ordinal;
            data.records.push(raw);
            let kind = if ordinal == 0 {
                let mut items = reference_list(1);
                items["references"] =
                    serde_json::json!(references
                        .map(|index| serde_json::json!({"index": index, "qualified": false})));
                serde_json::json!({"form": "references", "family": "object_collection", "items": items})
            } else {
                serde_json::json!({"form": "surface_body", "body": {"index": 1, "qualified": false}})
            };
            let mut property: super::PmDcFeatureProperty = located_payload(serde_json::json!({
                "save_version_major": 1, "header": content_header(), "kind": kind
            }));
            property.identity.record_ordinal = ordinal;
            data.pm_dc_feature_properties.push(property);
        }
        let setup = cadmpeg_test_support::service_decode_context();
        let members = cadmpeg_ir::features::FeatureResultMembers::new(
            bodies
                .into_iter()
                .map(|body| {
                    cadmpeg_core::text::NonBlankString::try_from(body.to_owned()).expect("body id")
                })
                .collect(),
            Vec::new(),
            Vec::new(),
            Vec::new(),
            &setup,
            "fixture members",
        )
        .expect("member budget")
        .expect("distinct members");
        let mut ir = cadmpeg_ir::CadIr::empty();
        ir.model
            .feature_result_topologies
            .push(cadmpeg_ir::features::FeatureResultTopology::new(
                cadmpeg_ir::ids::FeatureResultTopologyId::mint("inventor:test:result#0")
                    .expect("result id"),
                cadmpeg_ir::features::FeatureId::mint("inventor:test:feature#0")
                    .expect("feature id"),
                members,
                Some("inventor:pmdc:feature-property#t-0".into()),
            ));
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        // Three raw-index slots, three uniqueness slots, six property-index
        // slots and one result-index slot: 3 + 3 + 6 + 1 = 13. Add findings only.
        policy.limits.max_collection_items = 13 + expected_findings;
        // The indexes and finding fit here. Unequal ID lengths must be rejected
        // before hashing the 100,000-byte neutral body reference.
        policy.limits.max_work_units = 10_000;
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("context");
        let mut findings = Vec::new();
        super::validate_features(&ctx, &ir, &data, &mut findings).expect("body validation");
        assert_eq!(
            cadmpeg_core::decode::u64_from_index(findings.len()),
            expected_findings
        );
        assert_eq!(
            findings
                .iter()
                .filter(|finding| finding.message
                    == "Inventor feature result bodies do not match its PmDc object collection")
                .count(),
            usize::from(expected_findings != 0)
        );
    }
}

#[test]
fn protein_coverage_tracks_maximum_for_unsorted_repeated_and_gapped_positions() {
    for (ordinals, expected_findings) in [([1, 0], 0), ([0, 0], 0), ([0, 2], 1), ([0, u64::MAX], 1)]
    {
        let mut data = empty_native_data();
        let setup = cadmpeg_test_support::service_decode_context();
        for ordinal in ordinals {
            data.protein_rejections.push(
                super::ProteinRejectionRecordWire {
                    id: format!("inventor:protein:rejection#{ordinal}"),
                    entry_name: "InstanceProperties.bin".into(),
                    ordinal,
                    detail: "unavailable".into(),
                }
                .into_record(&setup)
                .expect("rejection"),
            );
        }
        let arena = DecodeArena::new();
        let (ctx, _) =
            DecodeContext::from_root_bytes(&[], &arena, &DecodePolicy::service()).expect("context");
        let mut findings = Vec::new();
        super::validate_protein_record_coverage(&ctx, &data, &mut findings).expect("coverage");
        assert_eq!(findings.len(), expected_findings);
        if let Some(finding) = findings.first() {
            assert_eq!(
                finding.message,
                r#"Inventor Protein logical-record positions are not contiguous for "InstanceProperties.bin""#
            );
        }
    }
}

#[test]
fn feature_output_collection_uses_existing_identity_index() {
    use cadmpeg_ir::features::{
        DistinctMembers, Feature, FeatureContent, FeatureDefinition, FeatureEvaluation,
        FeatureOperation, FeatureResultMembers, NonEmptyMembers,
    };
    for (class, collection, expected_findings) in [
        (
            crate::feature::FeatureFamily::Fillet,
            "inventor:pmdc:feature-property#t-1",
            0,
        ),
        (
            crate::feature::FeatureFamily::Extrusion,
            "inventor:pmdc:feature-property#t-1",
            1,
        ),
        (
            crate::feature::FeatureFamily::Fillet,
            "inventor:pmdc:feature-property#t-01",
            2,
        ),
    ] {
        let mut data = empty_native_data();
        for ordinal in 0..3 {
            let mut record = carrier_record();
            record.ordinal = ordinal;
            data.records.push(record);
        }
        let mut slots = reference_list(0);
        slots["references"] = serde_json::json!((0..16)
            .map(
                |slot| serde_json::json!({"index": if slot == 15 {2} else {0}, "qualified": false})
            )
            .collect::<Vec<_>>());
        data.pm_dc_features.push(located_payload(serde_json::json!({
            "save_version_major": 1, "header": content_header(), "state": 0,
            "outline_value": 0, "properties": slots, "value": 0
        })));
        let empty_list = serde_json::json!({"marker": 8, "metadata": null, "references": []});
        let mut property: super::PmDcFeatureProperty = located_payload(serde_json::json!({
            "save_version_major": 1, "header": content_header(),
            "kind": {"form": "references", "family": "object_collection", "items": empty_list}
        }));
        property.identity.record_ordinal = 1;
        data.pm_dc_feature_properties.push(property);
        let setup = cadmpeg_test_support::service_decode_context();
        let wire = serde_json::from_value::<super::PmDcFeatureLabelPayloadWire>(serde_json::json!({
            "save_version_major": 1, "header": {"header_value": 0, "header_id": 0, "values": [0, 0],
                "owner": {"index": 1, "qualified": false},
                "parent": {"index": 0, "qualified": false}, "next": {"index": 0, "qualified": false}},
            "index": 0, "participants": empty_list, "name": "fillet", "class_id": class.class_id().to_string()
        })).expect("label wire");
        data.pm_dc_feature_labels.push(super::Located::new(
            wire.into_record(&setup).expect("label"),
            crate::record_identity::RecordTypeId::try_from(
                carrier_record().type_id.as_str().to_owned(),
            )
            .expect("type id"),
            cadmpeg_ir::ids::IdentityKey::encode_segment("t"),
            2,
        ));
        let feature_id =
            cadmpeg_ir::features::FeatureId::mint("inventor:test:feature#0").expect("feature id");
        let mut ir = cadmpeg_ir::CadIr::empty();
        ir.model.features.push(Feature {
            id: feature_id.clone(),
            ordinal: 0,
            name: None,
            suppressed: None,
            dependencies: DistinctMembers::default(),
            source_properties: BTreeMap::new(),
            source_tag: None,
            source_text: None,
            source_content: FeatureContent::default(),
            evaluation: FeatureEvaluation::from_definition(FeatureDefinition::Operation(
                FeatureOperation::Fillet {
                    groups: NonEmptyMembers::one(
                        cadmpeg_ir::features::edge_treatments::FilletGroup {
                            edges: cadmpeg_ir::features::EdgeSelection::Unresolved,
                            radius: cadmpeg_ir::features::edge_treatments::RadiusSpec::Unresolved {
                                form: None,
                            },
                            tangency_weight: None,
                        },
                    ),
                },
            )),
            native_ref: Some("inventor:pmdc:feature#t-0".into()),
        });
        let members = FeatureResultMembers::new(
            Vec::new(),
            vec![cadmpeg_core::text::NonBlankString::try_from("face").expect("face")],
            Vec::new(),
            Vec::new(),
            &setup,
            "fixture members",
        )
        .expect("member budget")
        .expect("members");
        ir.model
            .feature_result_topologies
            .push(cadmpeg_ir::features::FeatureResultTopology::new(
                cadmpeg_ir::ids::FeatureResultTopologyId::mint("inventor:test:result#0")
                    .expect("result id"),
                feature_id,
                members,
                Some(collection.into()),
            ));
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        // Three raw records, three uniqueness entries, one feature, one label,
        // two property indexes and one result: 3 + 3 + 1 + 1 + 2 + 1 = 11 slots.
        policy.limits.max_collection_items = 11 + expected_findings;
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("context");
        let mut findings = Vec::new();
        super::validate_features(&ctx, &ir, &data, &mut findings)
            .expect("output collection validation");
        assert_eq!(
            cadmpeg_core::decode::u64_from_index(findings.len()),
            expected_findings
        );
    }
}

#[test]
fn missing_arena_report_uses_only_one_finding_slot() {
    let mut ir = cadmpeg_ir::CadIr::empty();
    ir.native.namespace_mut("inventor");
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    // The absent namespace arenas need no index slots. The fixed expected
    // arena names and missing-name vector need no input-sized admission.
    // Only the returned finding occupies a charged collection slot.
    policy.limits.max_collection_items = 1;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("context");
    let findings = super::validate_native(&ctx, &ir).expect("missing arena report");
    assert_eq!(findings.len(), 1);
    assert_eq!(
        findings[0].check,
        cadmpeg_ir::report::check::Check::NativeLinks
    );
    assert!(findings[0]
        .message
        .starts_with("Inventor native namespace has missing arenas ["));
    assert!(findings[0].message.ends_with("and unexpected arenas []"));
    for name in super::ARENAS {
        assert!(findings[0].message.contains(&format!("{name:?}")));
    }
}

#[test]
fn unexpected_arena_report_preserves_tree_order_without_sorting() {
    let mut ir = cadmpeg_ir::CadIr::empty();
    let namespace = ir.native.namespace_mut("inventor");
    for name in ["z", "a"] {
        namespace.arenas_mut().insert(name.to_owned(), Vec::new());
    }
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    // Two actual-name set slots, two unexpected-name vector slots and one
    // finding slot: 2 + 2 + 1 = 5. Expected/missing names have fixed bounds.
    policy.limits.max_collection_items = 5;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("context");
    let findings = super::validate_native(&ctx, &ir).expect("unexpected arena report");
    assert_eq!(findings.len(), 1);
    assert!(findings[0]
        .message
        .ends_with(r#"and unexpected arenas ["a", "z"]"#));
}

#[test]
fn empty_segment_validation_needs_no_constant_table_slots() {
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    // Empty sources create no stored entries. The fixed eleven-section
    // membership check needs neither collection slots nor temporary storage.
    policy.limits.max_collection_items = 0;
    policy.limits.max_materialized_bytes = 0;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("context");
    let mut findings = Vec::new();
    super::validate_segments(&ctx, &empty_native_data(), &mut findings).expect("empty segments");
    assert!(findings.is_empty());
}

fn selected_carrier_record() -> super::ActiveCarrierRecord {
    super::ActiveCarrierRecord::Selected {
        id: "carrier".into(),
        segment_token: "t".into(),
        record_ordinal: 0,
        segment_version_major: 1,
        family: crate::kernel::KernelFamily::Asm,
        header_state: 0,
        header_kind: 0,
        header_value: 0,
        schema: 0,
        carrier_len: std::num::NonZeroU64::new(1).expect("nonzero"),
        carrier_offset: 0,
        carrier_sha256: String::new(),
        selected_key: 0,
        enabled: true,
        delta_state: 0,
        history_reference: 0,
    }
}

#[test]
fn active_carrier_type_comparison_does_not_scan_overlong_type_text() {
    let mut data = empty_native_data();
    let mut record = carrier_record();
    record.type_id = "x".repeat(100_000);
    data.records.push(record);
    data.active_carrier = selected_carrier_record();
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    // Length mismatch is constant work. The allowance covers the record visit,
    // token comparison and returned finding, and cannot cover a 100,000-byte scan.
    policy.limits.max_work_units = 1_024;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("context");
    let mut findings = Vec::new();
    super::validate_active_carrier(&ctx, &data, &mut findings).expect("bounded type comparison");
    assert_eq!(findings.len(), 1);
    assert_eq!(
        findings[0].message,
        "Inventor active carrier does not resolve to its typed RSe record"
    );
}

#[test]
fn document_kind_comparison_is_bounded_by_literal_names() {
    for (kind, expected) in [
        ("assembly".to_owned(), true),
        ("part".to_owned(), false),
        ("x".repeat(100_000), false),
    ] {
        let mut ir = cadmpeg_ir::CadIr::empty();
        ir.source = Some(
            serde_json::from_value(serde_json::json!({
                "identity": {"classification": "unclassified", "format": "inventor"},
                "attributes": {"document_kind": kind}
            }))
            .expect("source metadata"),
        );
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        // This covers the source-attribute B-tree lookup. Literal-name equality
        // does not scan an overlong value and needs no input-sized admission.
        policy.limits.max_work_units = 1_024;
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("context");
        assert_eq!(
            super::is_assembly_document(&ctx, &ir).expect("document kind"),
            expected
        );
    }
}
