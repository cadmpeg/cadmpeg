use super::ownership_tests::relation_lane;
use super::{
    project_relation_point_geometry, project_relation_solved_line_geometry,
    spatial_relation_point_line_entities, SpatialRelationMarkers,
};
use crate::records::{
    FeatureInputLane, FeatureInputOperand, FeatureInputOperandKind, FeatureInputRelationFamily,
    SketchInputEntity, SketchInputKind, SketchInputLink, SketchInputLinks,
};
use cadmpeg_core::decode::refusal_probe::RefusalProbe;
use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
use cadmpeg_core::CodecError;
use cadmpeg_ir::features::{
    DesignParameter, DistinctMembers, Feature, FeatureContent, FeatureDefinition,
    FeatureEvaluation, FeatureId, FeatureOperation, ParameterId, ParameterValue,
    SketchFeatureBinding,
};
use cadmpeg_ir::math::{Point2, Point3, Vector3};
use cadmpeg_ir::scalar::Length;
use cadmpeg_ir::sketches::{
    Sketch, SketchEntity, SketchEntityId, SketchGeometry, SketchGeometryDefinition, SketchId,
    SketchPlacement, SketchProfiles, SpatialSketchId,
};
use std::collections::BTreeMap;

const EMPTY_INPUT_WORKSPACE_BYTES: u64 = 16 * 1024 * 1024;

fn parameter() -> DesignParameter {
    DesignParameter {
        id: ParameterId::mint("synthetic:test:parameter#distance").unwrap(),
        owner: None,
        ordinal: 0,
        name: "distance".into(),
        expression: "1mm".into(),
        display: None,
        value: Some(ParameterValue::Length(Length::new(1.0).unwrap())),
        dependencies: DistinctMembers::default(),
        properties: BTreeMap::new(),
        pmi: None,
        native_ref: Some("scalar".into()),
    }
}

fn planar_fixture() -> (Sketch, Feature, FeatureInputLane) {
    let sketch = Sketch {
        id: SketchId::mint("synthetic:test:sketch#planar").unwrap(),
        name: None,
        configuration: None,
        visible: None,
        placement: SketchPlacement::try_resolved(
            Point3::new(0.0, 0.0, 0.0),
            Vector3::new(0.0, 0.0, 1.0),
            Vector3::new(1.0, 0.0, 0.0),
        )
        .unwrap(),
        profiles: SketchProfiles::default(),
        native_ref: Some("lane".into()),
    };
    let feature = Feature {
        id: FeatureId::mint("synthetic:test:feature#planar").unwrap(),
        ordinal: 0,
        name: None,
        suppressed: Some(false),
        dependencies: DistinctMembers::default(),
        source_properties: BTreeMap::new(),
        source_tag: None,
        source_text: None,
        source_content: FeatureContent::default(),
        evaluation: FeatureEvaluation::from_definition(FeatureDefinition::Operation(
            FeatureOperation::Sketch {
                sketch: SketchFeatureBinding::Planar(Some(sketch.id.clone())),
            },
        )),
        native_ref: Some("feature".into()),
    };
    (sketch, feature, relation_lane())
}

fn point(id: &str, offset: u32, u: f64, v: f64) -> SketchInputEntity {
    let mut point = SketchInputEntity::new(
        id,
        "lane",
        offset,
        u64::from(offset),
        SketchInputKind::Point,
    );
    point.feature_ref = Some("feature".into());
    point.coordinates_m = cadmpeg_ir::units::FiniteVector::new([u / 1000.0, v / 1000.0]);
    point
}

fn operand(index: u16, reference: Option<&str>) -> FeatureInputOperand {
    FeatureInputOperand {
        offset: u64::from(index),
        reference_ref: format!("reference-{index}"),
        kind: FeatureInputOperandKind::E1,
        entity_index: index,
        entity_ref: reference.map(str::to_owned),
    }
}

fn project_solver(ctx: &DecodeContext<'_>) -> Result<(), CodecError> {
    let (sketch, feature, mut lane) = planar_fixture();
    lane.relation_instances[0].family = FeatureInputRelationFamily::LineLineDistance;
    lane.relation_instances[0].operands = vec![operand(0, None), operand(1, None)];
    lane.sketch_entities = vec![
        point("first-start", 0, 0.0, 0.0),
        point("first-end", 1, 2.0, 0.0),
        point("second-start", 2, 0.0, 1.0),
        point("second-end", 3, 2.0, 1.0),
    ];
    let mut parameter = parameter();
    parameter.owner = Some(feature.id.clone());
    let mut entities = Vec::new();
    project_relation_solved_line_geometry(
        ctx,
        &mut entities,
        &[sketch],
        &[feature],
        &[parameter],
        &[lane],
    )?;
    assert_eq!(entities.len(), 2);
    assert_eq!(entities[0].endpoint_refs, ["first-start", "first-end"]);
    assert_eq!(entities[1].endpoint_refs, ["second-start", "second-end"]);
    Ok(())
}

fn project_marker_line(ctx: &DecodeContext<'_>) -> Result<(), CodecError> {
    let (sketch, feature, mut lane) = planar_fixture();
    lane.relation_instances[0].family = FeatureInputRelationFamily::LineLineDistance;
    lane.relation_instances[0].operands = vec![operand(0, Some("line"))];
    let mut line = SketchInputEntity::new("line", "lane", 2, 20, SketchInputKind::LineOrCircle);
    line.feature_ref = Some("feature".into());
    line.links = SketchInputLinks::new(
        0,
        vec![
            SketchInputLink {
                local_id: 0,
                entity_ref: "first-start".into(),
            },
            SketchInputLink {
                local_id: 1,
                entity_ref: "first-end".into(),
            },
        ],
    );
    lane.sketch_entities = vec![
        point("first-start", 10, 0.0, 0.0),
        point("first-end", 11, 2.0, 0.0),
        line,
    ];
    // The source-frame join needs the already published profile points.
    let mut entities = [("first-start", 0.0), ("first-end", 2.0)]
        .into_iter()
        .map(|(reference, u)| {
            SketchEntity::new(
                SketchEntityId::mint(format!("synthetic:test:entity#{reference}")).unwrap(),
                sketch.id.clone(),
                SketchGeometry::try_from(SketchGeometryDefinition::Point {
                    position: Point2::new(u, 0.0),
                })
                .unwrap(),
            )
            .with_native_ref(Some(reference.into()))
        })
        .collect();
    project_relation_point_geometry(ctx, &mut entities, &[sketch], &[feature], &[lane])?;
    let lines: Vec<_> = entities
        .iter()
        .filter(|entity| !entity.endpoint_refs.is_empty())
        .collect();
    assert_eq!(lines.len(), 1);
    assert_eq!(lines[0].endpoint_refs, ["first-start", "first-end"]);
    Ok(())
}

fn spatial_marker(
    payload: &mut [u8],
    offset: usize,
    code: u32,
    index: u32,
    position: Point3,
) -> SketchInputEntity {
    payload[offset - 4..offset].copy_from_slice(&index.to_le_bytes());
    payload[offset..offset + 5].copy_from_slice(&[0xff, 0xff, 0x1f, 0x00, 0x01]);
    payload[offset + 5..offset + 13].fill(0xff);
    payload[offset + 13..offset + 17].copy_from_slice(&[0x00, 0x00, 0x80, 0xbf]);
    payload[offset + 17..offset + 21].copy_from_slice(&code.to_le_bytes());
    let locus = if code == 2 {
        [0x05, 0x00, 0x01, 0x00]
    } else {
        [0x04, 0x00, 0x02, 0x00]
    };
    payload[offset + 23..offset + 27].copy_from_slice(&locus);
    payload[offset + 27..offset + 29].copy_from_slice(&1u16.to_le_bytes());
    payload[offset + 56..offset + 58].copy_from_slice(&[0x0e, 0x00]);
    for (axis, value) in [position.x, position.y, position.z].into_iter().enumerate() {
        payload[offset + 58 + axis * 8..offset + 66 + axis * 8]
            .copy_from_slice(&(value / 1000.0).to_le_bytes());
    }
    let mut marker = SketchInputEntity::new(
        format!("marker-{offset}"),
        "lane",
        u32::try_from(offset).unwrap(),
        u64::try_from(offset).unwrap(),
        SketchInputKind::Point,
    );
    marker.feature_ref = Some("feature".into());
    marker.with_test_identity(Some(index), None)
}

fn project_spatial(ctx: &DecodeContext<'_>) -> Result<(), CodecError> {
    let mut lane = relation_lane();
    lane.relation_instances[0].id = "synthetic:test:relation#point-line".into();
    lane.relation_instances[0].family = FeatureInputRelationFamily::PointLineDistance;
    lane.relation_instances[0].operands = vec![operand(0, None)];
    let mut payload = vec![0; 300];
    lane.sketch_entities = vec![
        spatial_marker(&mut payload, 4, 2, 0, Point3::new(0.0, 1.0, 0.0)),
        spatial_marker(&mut payload, 104, 0, 1, Point3::new(-1.0, 0.0, 0.0)),
        spatial_marker(&mut payload, 204, 0, 2, Point3::new(1.0, 0.0, 0.0)),
    ];
    lane.native_payload = payload;
    let sketch = SpatialSketchId::mint("synthetic:test:sketch#spatial").unwrap();
    let mut markers = SpatialRelationMarkers::new(ctx, &lane)?;
    let mut entities = Vec::new();
    let output = spatial_relation_point_line_entities(
        ctx,
        &lane.relation_instances[0],
        &sketch,
        &parameter(),
        &mut markers,
        &mut entities,
    )?;
    let (_, line) = output.expect("unique point-line witness");
    let entity = entities.iter().find(|entity| entity.id() == &line).unwrap();
    assert_eq!(
        entity.endpoint_refs,
        [
            "synthetic:test:sketch#spatial:relation-point:104",
            "synthetic:test:sketch#spatial:relation-point:204"
        ]
    );
    Ok(())
}

fn exercise(operation: &'static str, project: fn(&DecodeContext<'_>) -> Result<(), CodecError>) {
    let arena = DecodeArena::new();
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &DecodePolicy::service()).unwrap();
    project(&ctx).unwrap();
    ctx.finish_session().unwrap();
    for (dimension, additional) in [
        (
            ResourceDimension::RetainedBytes,
            u64::try_from(2 * std::mem::size_of::<String>()).unwrap(),
        ),
        (ResourceDimension::CollectionItems, 2),
    ] {
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        if dimension == ResourceDimension::RetainedBytes {
            policy.limits.max_retained_bytes = u64::MAX;
        } else {
            policy.limits.max_collection_items = u64::MAX;
        }
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
        let _probe = RefusalProbe::arm(dimension, operation, Some(additional));
        let error = project(&ctx).unwrap_err();
        let CodecError::ResourceLimit(limit) = error else {
            panic!("expected endpoint storage refusal");
        };
        assert_eq!(limit.dimension, dimension);
        assert_eq!(limit.operation, operation);
        assert_eq!(limit.additional, additional);
        assert_eq!(limit.limit, limit.used);
        assert_eq!(ctx.resource_refusal().as_ref(), Some(&limit));
        assert!(
            matches!(ctx.finish_session(), Err(CodecError::ResourceLimit(first)) if first == limit)
        );
    }
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_work_units = u64::MAX;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    let _probe = RefusalProbe::arm(ResourceDimension::WorkUnits, operation, None);
    project(&ctx).unwrap();
    ctx.finish_session().unwrap();
}

#[test]
fn spatial_line_endpoint_slots_admit_storage_without_fixed_work() {
    exercise("retain SLDPRT spatial line endpoints", project_spatial);
}

#[test]
fn marker_line_endpoint_slots_admit_storage_without_fixed_work() {
    exercise("retain SLDPRT relation-line endpoints", project_marker_line);
}

#[test]
fn solver_line_endpoint_slots_admit_storage_without_fixed_work() {
    exercise("retain SLDPRT solver-line endpoints", project_solver);
}

#[test]
fn linked_relation_line_skips_geometry_index_construction() {
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_work_units = u64::MAX;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    let _probe = RefusalProbe::arm(
        ResourceDimension::WorkUnits,
        "index SLDPRT marker geometry",
        None,
    );
    project_marker_line(&ctx).unwrap();
    let CodecError::ResourceLimit(limit) = ctx
        .charge_work(1, "index SLDPRT marker geometry")
        .unwrap_err()
    else {
        panic!("linked endpoints must leave the geometry-index probe armed");
    };
    assert_eq!(limit.operation, "index SLDPRT marker geometry");
    assert_eq!(limit.additional, 1);
    assert_eq!(ctx.resource_refusal(), Some(limit));
}

fn existing_solver_lines(ctx: &DecodeContext<'_>, dynamic: bool) -> Result<(), CodecError> {
    let (sketch, feature, mut lane) = planar_fixture();
    lane.relation_instances[0].family = FeatureInputRelationFamily::LineLineDistance;
    lane.relation_instances[0].operands = vec![operand(0, None), operand(1, None)];
    if dynamic {
        for operand in &mut lane.relation_instances[0].operands {
            operand.kind = FeatureInputOperandKind::Native(
                crate::records::operand_tag::NativeOperandTag::try_from(0x1234).unwrap(),
            );
        }
    }
    lane.sketch_entities = vec![
        point("first-start", 0, 0.0, 0.0),
        point("first-end", 1, 2.0, 0.0),
        point("second-start", 2, 0.0, 1.0),
        point("second-end", 3, 2.0, 1.0),
    ];
    let mut parameter = parameter();
    parameter.owner = Some(feature.id.clone());
    if dynamic {
        parameter.value = Some(ParameterValue::Length(Length::new(2.0).unwrap()));
    }
    let second_y = if dynamic { 2.0 } else { 1.0 };
    let mut entities: Vec<_> = [0.0, second_y]
        .into_iter()
        .enumerate()
        .map(|(index, y)| {
            SketchEntity::new(
                SketchEntityId::mint(format!("synthetic:test:entity#existing-{index}")).unwrap(),
                sketch.id.clone(),
                SketchGeometry::try_from(SketchGeometryDefinition::Line {
                    start: Point2::new(0.0, y),
                    end: Point2::new(2.0, y),
                })
                .unwrap(),
            )
            .with_construction(true)
            .with_geometry_ref(Some(format!("feature:solver-line:{index}")))
        })
        .collect();
    if dynamic {
        // Three noncollinear native anchors select one identity frame even
        // though the existing second line differs from its fallback pair.
        for (native_ref, u, v) in [
            ("first-start", 0.0, 0.0),
            ("first-end", 2.0, 0.0),
            ("second-start", 0.0, 1.0),
        ] {
            entities.push(
                SketchEntity::new(
                    SketchEntityId::mint(format!("synthetic:test:entity#anchor-{native_ref}"))
                        .unwrap(),
                    sketch.id.clone(),
                    SketchGeometry::try_from(SketchGeometryDefinition::Point {
                        position: Point2::new(u, v),
                    })
                    .unwrap(),
                )
                .with_native_ref(Some(native_ref.into())),
            );
        }
    }
    let before = entities.clone();
    for _ in 0..8 {
        project_relation_solved_line_geometry(
            ctx,
            &mut entities,
            std::slice::from_ref(&sketch),
            std::slice::from_ref(&feature),
            std::slice::from_ref(&parameter),
            std::slice::from_ref(&lane),
        )?;
        assert_eq!(entities, before);
    }
    Ok(())
}

fn duplicate_solver_reference_storage(dynamic: bool) {
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_retained_bytes = 0;
    policy.limits.max_materialized_bytes = EMPTY_INPUT_WORKSPACE_BYTES;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    existing_solver_lines(&ctx, dynamic).unwrap();
    ctx.reserve_scoped(
        policy.limits.max_materialized_bytes,
        "reuse duplicate solver workspace",
    )
    .unwrap();
    ctx.finish_session().unwrap();
    let operation = if dynamic {
        "format SLDPRT dynamic solver-line reference"
    } else {
        "format SLDPRT solver-line reference"
    };
    let error =
        crate::test_support::work_refusal_at(operation, |ctx| existing_solver_lines(ctx, dynamic));
    assert!(matches!(error, CodecError::ResourceLimit(limit) if limit.operation == operation));
}

#[test]
fn duplicate_solver_lines_release_lookup_strings_without_retained_charges() {
    duplicate_solver_reference_storage(false);
}

#[test]
fn duplicate_dynamic_solver_lines_release_lookup_strings_without_retained_charges() {
    duplicate_solver_reference_storage(true);
}

fn mixed_solver_retry(
    ctx: &DecodeContext<'_>,
    expected: f64,
    missing_fallback: bool,
) -> Result<(), CodecError> {
    let (sketch, feature, mut lane) = planar_fixture();
    lane.relation_instances[0].family = FeatureInputRelationFamily::LineLineDistance;
    lane.relation_instances[0].operands = vec![
        operand(0, Some("line")),
        operand(if missing_fallback { 100 } else { 1 }, None),
    ];
    lane.sketch_entities = vec![
        point("fallback-0-start", 0, 0.0, 0.0),
        point("fallback-0-end", 1, 10.0, 0.0),
        point("fallback-1-start", 2, 0.0, 5.0),
        point("fallback-1-end", 3, 10.0, 5.0),
        point("explicit-start", 4, 0.0, 20.0),
        point("explicit-end", 5, 10.0, 20.0),
    ];
    let mut line = SketchInputEntity::new("line", "lane", 6, 20, SketchInputKind::LineOrCircle);
    line.feature_ref = Some("feature".into());
    line.links = SketchInputLinks::new(
        0,
        ["explicit-start", "explicit-end"]
            .into_iter()
            .enumerate()
            .map(|(index, entity_ref)| SketchInputLink {
                local_id: u16::try_from(index).unwrap(),
                entity_ref: entity_ref.into(),
            })
            .collect(),
    );
    lane.sketch_entities.push(line);
    let mut parameter = parameter();
    parameter.owner = Some(feature.id.clone());
    parameter.value = Some(ParameterValue::Length(Length::new(expected).unwrap()));
    let mut entities = Vec::new();
    project_relation_solved_line_geometry(
        ctx,
        &mut entities,
        &[sketch],
        &[feature],
        &[parameter],
        &[lane],
    )?;
    if expected == 5.0 && !missing_fallback {
        assert_eq!(entities.len(), 2);
        assert_eq!(
            entities[0].endpoint_refs,
            ["fallback-0-start", "fallback-0-end"]
        );
        assert_eq!(
            entities[1].endpoint_refs,
            ["fallback-1-start", "fallback-1-end"]
        );
        assert_eq!(
            entities[0].geometry,
            SketchGeometry::try_from(SketchGeometryDefinition::Line {
                start: Point2::new(0.0, 0.0),
                end: Point2::new(10.0, 0.0)
            })
            .unwrap()
        );
        assert_eq!(
            entities[1].geometry,
            SketchGeometry::try_from(SketchGeometryDefinition::Line {
                start: Point2::new(0.0, 5.0),
                end: Point2::new(10.0, 5.0)
            })
            .unwrap()
        );
    } else {
        assert!(entities.is_empty());
    }
    Ok(())
}

#[test]
fn mixed_solver_retry_substitutes_explicit_lines_and_preserves_fallback_geometry() {
    mixed_solver_retry(&cadmpeg_test_support::service_decode_context(), 5.0, false).unwrap();
}

#[test]
fn invalid_mixed_solver_retry_keeps_scratch_scoped_and_geometry_unbuilt() {
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_work_units = u64::MAX;
    policy.limits.max_retained_bytes = 0;
    policy.limits.max_materialized_bytes = EMPTY_INPUT_WORKSPACE_BYTES;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    let probe = RefusalProbe::arm(
        ResourceDimension::WorkUnits,
        "index SLDPRT marker geometry",
        None,
    );
    mixed_solver_retry(&ctx, 2.0, false).unwrap();
    ctx.reserve_scoped(
        policy.limits.max_materialized_bytes,
        "reuse mixed retry workspace",
    )
    .unwrap();
    let CodecError::ResourceLimit(limit) = ctx
        .charge_work(1, "index SLDPRT marker geometry")
        .unwrap_err()
    else {
        panic!("solved linked endpoints must leave the geometry-index probe armed");
    };
    assert_eq!(limit.operation, "index SLDPRT marker geometry");
    drop(probe);
}

#[test]
fn mixed_solver_retry_preserves_failed_fallback_resolution() {
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_retained_bytes = 0;
    policy.limits.max_materialized_bytes = EMPTY_INPUT_WORKSPACE_BYTES;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    mixed_solver_retry(&ctx, 2.0, true).unwrap();
    ctx.reserve_scoped(
        policy.limits.max_materialized_bytes,
        "reuse failed fallback workspace",
    )
    .unwrap();
    ctx.finish_session().unwrap();
}

mod activity;
