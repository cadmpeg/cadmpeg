// SPDX-License-Identifier: Apache-2.0
use super::super::{sketch_records, sketch_section_point_records};
use crate::decode::native_records::CreoSketchSectionPoint;
use crate::feature::definitions::{
    DefinitionIdentity, FeatureDefinition, FeatureSectionPoint, FeatureVariableRow, FeatureVariableTable,
    ScalarLane, VariableType,
};
use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
use cadmpeg_core::CodecError;

// Core bounds scratch by min(policy ceiling, 16 MiB + 1000 * input bytes).
const EMPTY_INPUT_MATERIALIZED_ALLOWANCE: u64 = 16 * 1024 * 1024;

fn definition(with_variables: bool) -> FeatureDefinition {
    FeatureDefinition {
        identity: DefinitionIdentity::Parsed {
            schema_id: std::num::NonZeroU32::new(7),
            owner_feature_id: None,
        },
        body: Vec::new(),
        parameter_frames: Vec::new(),
        outlines: Vec::new(),
        variables: with_variables.then(|| {
            crate::feature::definitions::test_support::with_points(
                FeatureVariableTable {
                    declared_count: 0,
                    entity_ref: None,
                    rows: Vec::new(),
                    offset: 2,
                },
                vec![
                    FeatureSectionPoint { point_id: 7, u: Some(1.0), v: Some(2.0) },
                    FeatureSectionPoint { point_id: 3, u: Some(3.0), v: Some(4.0) },
                    FeatureSectionPoint { point_id: 4, u: Some(2.0), v: None },
                    FeatureSectionPoint { point_id: 5, u: None, v: Some(6.0) },
                    FeatureSectionPoint { point_id: 6, u: None, v: None },
                    FeatureSectionPoint { point_id: 7, u: Some(2.0), v: Some(2.0) },
                ],
            )
        }),
        segments: None,
        trim_entities: None,
        trim_vertices: None,
        order_table: None,
        section_3d: None,
        dimensions: None,
        relations: None,
        saved_section: None,
        offset: 1,
    }
}

fn expected_points() -> serde_json::Value {
    serde_json::json!([
        {"point_id": 3, "state": "resolved", "u": 3.0, "v": 4.0},
        {"point_id": 4, "state": "partial", "u": 2.0, "v": null},
        {"point_id": 5, "state": "partial", "u": null, "v": 6.0},
        {"point_id": 6, "state": "unresolved", "u": null, "v": null},
        {"point_id": 7, "state": "conflicting", "u": null, "v": null}
    ])
}

#[test]
fn sketch_point_absence_is_free_and_preserves_original_refusal() {
    let definition = definition(false);
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_work_units = 0;
    policy.limits.max_collection_items = 0;
    policy.limits.max_retained_bytes = 0;
    policy.limits.max_materialized_bytes = 0;
    policy.limits.max_recursion_depth = 0;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("root");
    let mut storage = ctx.reserve_scoped(0, "point projection storage").expect("empty storage");
    assert!(sketch_section_point_records(&ctx, &definition, &mut storage).expect("absent variables").is_empty());
    let original = ctx.charge_collection_items_limit(1, "after absent sketch points").expect_err("zero items");
    assert!(matches!(sketch_section_point_records(&ctx, &definition, &mut storage),
        Err(CodecError::ResourceLimit(actual)) if actual == original));
    assert_eq!(ctx.resource_refusal(), Some(original));
}

#[test]
fn sketch_point_indices_release_while_projection_backing_stays_live() {
    for point_count in [1, 5] {
        let mut definition = definition(true);
        if point_count == 1 {
            let variables = definition.variables.as_mut().expect("table");
            variables.rows.truncate(2);
            variables.declared_count = 2;
        }
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        policy.limits.max_retained_bytes = 0;
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("root");
        let mut storage = ctx.reserve_scoped(0, "point projection storage").expect("storage");
        let points = sketch_section_point_records(&ctx, &definition, &mut storage).expect("scoped points");
        assert_eq!(points.len(), point_count);
        if point_count == 5 {
            assert_eq!(serde_json::to_value(&points).expect("serialize"), expected_points());
        } else {
            assert_eq!(serde_json::to_value(&points).expect("serialize"),
                serde_json::json!([{"point_id": 7, "state": "resolved", "u": 1.0, "v": 2.0}]));
        }
        // Amortized backing starts at four slots and doubles on the fifth item.
        let slots = if point_count == 1 { 4 } else { 8 };
        let backing = (slots * std::mem::size_of::<CreoSketchSectionPoint>()) as u64;
        let allowance = policy.limits.max_materialized_bytes.min(EMPTY_INPUT_MATERIALIZED_ALLOWANCE);
        let remaining = ctx.reserve_scoped(allowance - backing, "point scratch released").expect("only output backing remains");
        drop(remaining);
        drop(points);
        drop(storage);
        ctx.reserve_scoped(allowance, "point projection released").expect("all backing released");
    }
}

#[test]
fn sketch_point_collection_prefixes_preserve_exact_original_refusal() {
    let definition = definition(true);
    // Five group keys, four reconciled points plus one conflict, five identity
    // union keys, and five output records each require one collection item.
    for cap in 0..=20 {
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        policy.limits.max_collection_items = cap;
        policy.limits.max_retained_bytes = 0;
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("root");
        let mut storage = ctx.reserve_scoped(0, "point projection storage").expect("storage");
        let result = sketch_section_point_records(&ctx, &definition, &mut storage);
        let original = if cap == 20 {
            let points = result.expect("exact item count");
            assert_eq!(serde_json::to_value(&points).expect("serialize"), expected_points());
            drop(points);
            ctx.charge_collection_items_limit(1, "after sketch point projection").expect_err("exact items")
        } else {
            let original = ctx.resource_refusal().expect("item refusal");
            assert!(matches!(result, Err(CodecError::ResourceLimit(actual)) if actual == original));
            let operation = match cap {
                0..=4 => "creo reconciled point ID nodes",
                5..=8 => "creo reconciled point nodes",
                9 => "creo ambiguous point nodes",
                10..=14 => "creo sketch section point ID nodes",
                _ => "creo sketch section point records",
            };
            assert_eq!(original.operation, operation);
            original
        };
        assert_eq!((original.dimension, original.limit, original.used, original.additional),
            (ResourceDimension::CollectionItems, cap, cap, 1));
        assert!(matches!(sketch_section_point_records(&ctx, &definition, &mut storage),
            Err(CodecError::ResourceLimit(actual)) if actual == original));
        assert_eq!(ctx.resource_refusal(), Some(original));
    }
}

#[test]
fn sketch_projection_keeps_resolved_point_maps_scoped() {
    let mut scan = crate::test_support::empty_container_scan();
    let mut definition = definition(true);
    let variables = definition.variables.as_mut().expect("table");
    for (variable_type, key, value) in [
        (VariableType::Radius, 17, 2.0),
        (VariableType::Parameter, 42, 7.0),
    ] {
        variables.rows.push(FeatureVariableRow {
            variable_type,
            key,
            value: ScalarLane::Value(value),
            value_body: Vec::new(),
            guess: ScalarLane::Undefined,
            guess_body: Vec::new(),
            known: None,
            homogeneity: None,
            uvar_id: None,
            offset: 3,
        });
        variables.declared_count += 1;
    }
    scan.features.definitions.push(definition);
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_retained_bytes = 0;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("root");
    let projection = sketch_records(&ctx, &scan).expect("scoped sketch projection");
    let storage = projection.1;
    let records = projection.0;
    assert_eq!(records.len(), 1);
    let value = serde_json::to_value(&records[0]).expect("serialize");
    assert_eq!(value["section_points"], expected_points());
    assert_eq!(value["variables"][12]["resolved_value"], serde_json::json!(2.0));
    assert!(value["variables"][13]["resolved_value"].is_null());
    drop(records);
    drop(storage);
    let allowance = policy.limits.max_materialized_bytes.min(EMPTY_INPUT_MATERIALIZED_ALLOWANCE);
    ctx.reserve_scoped(allowance, "sketch projection released").expect("scratch and output released");
}

#[test]
fn sketch_equation_projection_is_scoped_and_preserves_wire_rows() {
    let prefix = b"eqtn_arr\0\xf2\xf8\x04\xf7\x80\x9f\xfb\xe2\
        \xe0\x01id\0\x00\
        \xe0\x05fcn_id\0\x02\
        \xe0\x08arg_arr\0\xf8\x02\x2f\x08\
        \xe0\x01aux_data\0\xf6\
        \xf1\xf7\x80\x9f\xe2";
    let bodies = [b"\x01\x04\x11\x12\xf6\xe2".as_slice(),
        b"\x02\x05\xf8\x04\x13\xe4\xe5\xf6\xe2".as_slice(),
        b"\x03\x06\xf8\x02\xf6\x14\xf6\xe2".as_slice()];
    let mut definition = definition(true);
    definition.offset = 100;
    definition.body = prefix.to_vec();
    for body in bodies { definition.body.extend_from_slice(body); }
    definition.body.extend_from_slice(b"\xe0\x02scale\0\x99\x88");
    let mut scan = crate::test_support::empty_container_scan();
    scan.features.definitions.push(definition);
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_retained_bytes = 0;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("root");
    let (records, storage) = sketch_records(&ctx, &scan).expect("equations are projection scratch");
    assert_eq!(records.len(), 1);
    let wire = serde_json::to_value(&records[0]).expect("sketch wire");
    assert_eq!(wire["equations"], serde_json::json!([
        {"equation_id": 1, "function_id": 4, "explicit_argument_count": null,
         "arguments": [17, 18], "arguments_body": [17, 18], "auxiliary_body": [246],
         "body": bodies[0], "offset": 100 + prefix.len()},
        {"equation_id": 2, "function_id": 5, "explicit_argument_count": 4,
         "arguments": [19, 1, 0, 0], "arguments_body": [19, 228, 229], "auxiliary_body": [246],
         "body": bodies[1], "offset": 100 + prefix.len() + bodies[0].len()},
        {"equation_id": 3, "function_id": 6, "explicit_argument_count": 2,
         "arguments": [null, 20], "arguments_body": [246, 20], "auxiliary_body": [246],
         "body": bodies[2], "offset": 100 + prefix.len() + bodies[0].len() + bodies[1].len()}
    ]));
    drop(records);
    drop(storage);
    let resource = ctx.reserve_scoped_limit(u64::MAX, "equation projection release")
        .expect_err("read live backing");
    assert_eq!(resource.used, 0);
}
