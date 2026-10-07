// SPDX-License-Identifier: Apache-2.0
use super::{
    equal_btree_sets, validate_active_carrier, validate_design, validate_sketches, NativeData,
};
use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
use cadmpeg_core::CodecError;
use std::collections::BTreeSet;

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
    data.active_carrier = super::ActiveCarrierRecord::Selected {
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
    };
    // One source step, two one-byte tokens, and two 32-byte type ids: 1 + 2 + 64.
    let need = 67;
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
                    && limit.operation == "compare Inventor active-carrier type"
                    && limit.used == 35 && limit.additional == 32));
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
