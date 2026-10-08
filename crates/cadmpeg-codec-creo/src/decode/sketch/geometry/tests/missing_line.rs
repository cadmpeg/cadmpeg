// SPDX-License-Identifier: Apache-2.0

use super::super::{
    resolved_section_segment_geometry_with_missing_line, saved_section_missing_line_geometry,
};
use crate::feature::definitions::{FeatureSegment, FeatureSegmentKind};
use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
use cadmpeg_core::CodecError;
use cadmpeg_ir::sketches::{SketchGeometry, SketchGeometryDefinition};
use std::collections::BTreeMap;

fn fixture() -> crate::feature::definitions::FeatureDefinition {
    use crate::feature::definitions::{
        DefinitionIdentity, FeatureDefinition, FeatureOrderRow, FeatureOrderTable,
        FeatureSavedEntity, FeatureSavedLine, FeatureSavedSection, FeatureSegment,
        FeatureSegmentKind, FeatureSegmentTable, FeatureTrimEntity, FeatureTrimEntityTable,
        TrimEntityKind,
    };
    let segment = |external_id, offset, vertical_horizontal| FeatureSegment {
        kind: FeatureSegmentKind::Line([7, 9]),
        directions: [None; 3],
        center_id: None,
        arc_orientation: None,
        vertical_horizontal,
        radius_ref: None,
        radius2_ref: None,
        external_id,
        body: Vec::new(),
        offset,
    };
    FeatureDefinition {
        identity: DefinitionIdentity::Parsed {
            schema_id: std::num::NonZeroU32::new(5),
            owner_feature_id: Some(6),
        },
        body: Vec::new(),
        parameter_frames: Vec::new(),
        outlines: Vec::new(),
        variables: None,
        segments: Some(FeatureSegmentTable {
            declared_count: 2,
            has_elided_prototype: false,
            entity_ref: None,
            rows: vec![segment(42, 40, None), segment(43, 41, Some(1))]
                .into_iter()
                .map(crate::feature::segment_rows::SegmentRow::Ordinary)
                .collect(),
            offset: 4,
        }),
        trim_entities: Some(FeatureTrimEntityTable {
            declared_count: None,
            entity_ref: None,
            entry_ref: None,
            buckets: Vec::new(),
            rows: vec![FeatureTrimEntity {
                external_id: 43,
                mode: Some(0),
                vertices: [3, 4],
                kind: TrimEntityKind::Line,
                offset: 7,
            }],
            solved_external_ids: vec![43],
            offset: 5,
        }),
        trim_vertices: None,
        order_table: Some(FeatureOrderTable {
            declared_count: 1,
            has_prototype: false,
            entity_ref: None,
            rows: vec![FeatureOrderRow {
                external_id: 42,
                internal_id: 3,
                bitmask: 0,
                offset: 10,
            }]
            .into(),
            offset: 8,
        }),
        section_3d: None,
        dimensions: None,
        relations: None,
        saved_section: Some(FeatureSavedSection {
            entities: vec![FeatureSavedEntity::Line(FeatureSavedLine {
                entity_id: 3,
                references: Vec::new(),
                attributes: Vec::new(),
                endpoints: [
                    [Some(-8.0), Some(-0.85), Some(0.0)],
                    [Some(8.0), Some(-0.85), None],
                ],
                body: Vec::new(),
                offset: 20,
            })],
            offset: 18,
        }),
        offset: 0,
    }
}

fn multiple_mate_fixture() -> crate::feature::definitions::FeatureDefinition {
    let mut definition = fixture();
    let order = definition.order_table.as_mut().expect("order table");
    order.declared_count = 3;
    order.rows.extend([
        crate::feature::definitions::FeatureOrderRow {
            external_id: 44,
            internal_id: 4,
            bitmask: 0,
            offset: 11,
        },
        crate::feature::definitions::FeatureOrderRow {
            external_id: 45,
            internal_id: 5,
            bitmask: 0,
            offset: 12,
        },
    ]);
    let saved = definition.saved_section.as_mut().expect("saved section");
    for (entity_id, endpoints, offset) in [
        (
            4,
            [
                [Some(-8.0), Some(-0.85), Some(0.0)],
                [Some(10.0), Some(2.0), Some(0.0)],
            ],
            21,
        ),
        (
            5,
            [
                [Some(-8.0), Some(-0.85), Some(0.0)],
                [Some(20.0), Some(3.0), Some(0.0)],
            ],
            22,
        ),
    ] {
        saved
            .entities
            .push(crate::feature::definitions::FeatureSavedEntity::Line(
                crate::feature::definitions::FeatureSavedLine {
                    entity_id,
                    references: Vec::new(),
                    attributes: Vec::new(),
                    endpoints,
                    body: Vec::new(),
                    offset,
                },
            ));
    }
    definition
}

fn run(policy: &DecodePolicy) -> Result<Option<(usize, SketchGeometry)>, CodecError> {
    let definition = fixture();
    let arena = DecodeArena::new();
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, policy)?;
    saved_section_missing_line_geometry(&ctx, &definition)
}

fn run_definition(
    definition: &crate::feature::definitions::FeatureDefinition,
    policy: &DecodePolicy,
) -> Result<Option<(usize, SketchGeometry)>, CodecError> {
    let arena = DecodeArena::new();
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, policy)?;
    saved_section_missing_line_geometry(&ctx, definition)
}

fn assert_item_refusal(operation: &'static str) {
    let limit = crate::test_support::allocation_limit_at(
        ResourceDimension::CollectionItems,
        Some(operation),
        |cap| {
            let mut policy = DecodePolicy::service();
            policy.limits.max_collection_items = cap;
            run(&policy)
        },
    );
    let mut policy = DecodePolicy::service();
    policy.limits.max_collection_items = limit;
    let error = run(&policy).expect_err("missing-line collection exceeds cap");
    assert!(
        matches!(error, CodecError::ResourceLimit(resource)
        if resource.dimension == ResourceDimension::CollectionItems
            && resource.operation == operation),
        "{error:?}"
    );
}

#[test]
fn missing_line_refuses_trimmed_id_tree_node() {
    assert_item_refusal("creo missing-line trimmed ID nodes");
}

#[test]
fn missing_line_refuses_saved_geometry_vec_growth() {
    assert_item_refusal("creo missing-line saved geometries");
}

#[test]
fn missing_line_refuses_ordered_id_tree_node() {
    assert_item_refusal("creo missing-line ordered ID nodes");
}

#[test]
fn missing_line_refuses_geometry_id_tree_node() {
    assert_item_refusal("creo missing-line geometry ID nodes");
}

#[test]
fn missing_line_refuses_endpoint_vec_growth() {
    assert_item_refusal("creo missing-line endpoints");
}

#[test]
fn missing_line_refuses_endpoint_pair_work() {
    let mut policy = DecodePolicy::service();
    // All identity-tree shifts and key reads are admitted before the endpoint-pair comparison.
    let error = cadmpeg_test_support::refusal::resource_limit_at(
        ResourceDimension::WorkUnits,
        "creo missing-line endpoint candidates",
        |cap| {
            policy.limits.max_work_units = cap;
            run(&policy)
        },
    );
    assert!(matches!(error, CodecError::ResourceLimit(resource)
        if resource.dimension == ResourceDimension::WorkUnits
            && resource.operation == "creo missing-line endpoint candidates"));
}

#[test]
fn missing_line_stops_endpoint_pairs_after_ambiguous_mates() {
    let definition = multiple_mate_fixture();
    let error = crate::test_support::last_refusal_at(
        &[],
        ResourceDimension::WorkUnits,
        "creo missing-line endpoint candidates",
        |ctx| saved_section_missing_line_geometry(ctx, &definition),
    );
    let CodecError::ResourceLimit(refusal) = error else {
        panic!("named endpoint-pair refusal");
    };
    let mut policy = DecodePolicy::service();
    policy.limits.max_work_units = refusal
        .used
        .checked_add(refusal.additional)
        .expect("last visited pair fits");
    assert!(run_definition(&definition, &policy)
        .expect("the ambiguous endpoint scan is admitted")
        .is_none());
}

#[test]
fn missing_line_keeps_service_geometry_and_offset() {
    let (offset, geometry) = run(&DecodePolicy::service())
        .expect("service resources")
        .expect("missing line resolved");
    assert_eq!(offset, 41);
    assert_eq!(
        geometry,
        SketchGeometry::try_from(SketchGeometryDefinition::Line {
            start: cadmpeg_ir::math::Point2::new(-8.0, -0.85),
            end: cadmpeg_ir::math::Point2::new(8.0, -0.85),
        })
        .expect("line geometry")
    );
}

#[test]
fn missing_line_fallback_geometry_copy_refuses_work_and_retained_bytes() {
    let mut definition = fixture();
    definition.segments = None;
    definition.saved_section = None;
    let segment = FeatureSegment {
        kind: FeatureSegmentKind::Line([7, 9]),
        directions: [None; 3],
        center_id: None,
        arc_orientation: None,
        vertical_horizontal: None,
        radius_ref: None,
        radius2_ref: None,
        external_id: 90,
        body: Vec::new(),
        offset: 90,
    };
    let native_kind = cadmpeg_core::text::NonBlankString::try_from("fallback geometry")
        .expect("nonblank native geometry kind");
    let geometry = SketchGeometry::try_from(SketchGeometryDefinition::Native { native_kind })
        .expect("native geometry");
    let missing_line = (segment.offset, geometry);
    let points = BTreeMap::new();
    let arena = DecodeArena::new();

    let work_error = crate::test_support::last_refusal_at(
        &[],
        ResourceDimension::WorkUnits,
        "creo missing line fallback geometry",
        |ctx| {
            resolved_section_segment_geometry_with_missing_line(
                ctx,
                &definition,
                &points,
                &segment,
                Some(&missing_line),
            )
        },
    );
    assert!(
        matches!(work_error, CodecError::ResourceLimit(resource)
        if resource.dimension == ResourceDimension::WorkUnits
            && resource.operation == "creo missing line fallback geometry"),
        "{work_error:?}"
    );

    let mut retained_policy = DecodePolicy::service();
    let native_bytes = u64::try_from("fallback geometry".len()).expect("short fixture length");
    retained_policy.limits.max_retained_bytes = native_bytes - 1;
    let (ctx, _) =
        DecodeContext::from_root_bytes(&[], &arena, &retained_policy).expect("empty root");
    let retained_error = resolved_section_segment_geometry_with_missing_line(
        &ctx,
        &definition,
        &points,
        &segment,
        Some(&missing_line),
    )
    .expect_err("the fallback native kind exceeds the retained cap");
    assert!(
        matches!(retained_error, CodecError::ResourceLimit(resource)
        if resource.dimension == ResourceDimension::RetainedBytes
            && resource.operation == "creo missing line fallback geometry"),
        "{retained_error:?}"
    );

    let result = crate::decode::with_test_decode_ctx(|ctx| {
        resolved_section_segment_geometry_with_missing_line(
            ctx,
            &definition,
            &points,
            &segment,
            Some(&missing_line),
        )
    })
    .expect("service fallback copy")
    .expect("matching missing-line geometry");
    assert_eq!(result, missing_line.1);
}
