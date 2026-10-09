use super::{
    project_relation_point_geometry, project_relation_solved_line_geometry,
    spatial_relation_point_line_entities, SpatialRelationMarkers,
};
use super::ownership_tests::relation_lane;
use crate::records::{
    FeatureInputLane, FeatureInputOperand, FeatureInputOperandKind, FeatureInputRelationFamily,
    SketchInputEntity, SketchInputKind, SketchInputLink, SketchInputLinks,
};
use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
use cadmpeg_core::decode::refusal_probe::RefusalProbe;
use cadmpeg_core::CodecError;
use cadmpeg_ir::features::{
    DesignParameter, DistinctMembers, Feature, FeatureContent, FeatureDefinition,
    FeatureEvaluation, FeatureId, FeatureOperation, ParameterId, ParameterValue,
    SketchFeatureBinding,
};
use cadmpeg_ir::math::{Point2, Point3, Vector3};
use cadmpeg_ir::scalar::Length;
use cadmpeg_ir::sketches::{Sketch, SketchEntity, SketchEntityId, SketchGeometry, SketchGeometryDefinition, SketchId, SketchPlacement, SketchProfiles, SpatialSketchId};
use std::collections::BTreeMap;

fn parameter() -> DesignParameter {
    DesignParameter {
        id: ParameterId::mint("synthetic:test:parameter#distance").unwrap(),
        owner: None, ordinal: 0, name: "distance".into(), expression: "1mm".into(),
        display: None, value: Some(ParameterValue::Length(Length::new(1.0).unwrap())),
        dependencies: DistinctMembers::default(), properties: BTreeMap::new(),
        pmi: None, native_ref: Some("scalar".into()),
    }
}

fn planar_fixture() -> (Sketch, Feature, FeatureInputLane) {
    let sketch = Sketch {
        id: SketchId::mint("synthetic:test:sketch#planar").unwrap(), name: None,
        configuration: None, visible: None,
        placement: SketchPlacement::try_resolved(
            Point3::new(0.0, 0.0, 0.0), Vector3::new(0.0, 0.0, 1.0),
            Vector3::new(1.0, 0.0, 0.0),
        ).unwrap(),
        profiles: SketchProfiles::default(), native_ref: Some("lane".into()),
    };
    let feature = Feature {
        id: FeatureId::mint("synthetic:test:feature#planar").unwrap(), ordinal: 0,
        name: None, suppressed: Some(false), dependencies: DistinctMembers::default(),
        source_properties: BTreeMap::new(), source_tag: None, source_text: None,
        source_content: FeatureContent::default(),
        evaluation: FeatureEvaluation::from_definition(FeatureDefinition::Operation(
            FeatureOperation::Sketch { sketch: SketchFeatureBinding::Planar(Some(sketch.id.clone())) },
        )), native_ref: Some("feature".into()),
    };
    (sketch, feature, relation_lane())
}

fn point(id: &str, offset: u32, u: f64, v: f64) -> SketchInputEntity {
    let mut point = SketchInputEntity::new(id, "lane", offset, u64::from(offset), SketchInputKind::Point);
    point.feature_ref = Some("feature".into());
    point.coordinates_m = cadmpeg_ir::units::FiniteVector::new([u / 1000.0, v / 1000.0]);
    point
}

fn operand(index: u16, reference: Option<&str>) -> FeatureInputOperand {
    FeatureInputOperand {
        offset: u64::from(index), reference_ref: format!("reference-{index}"),
        kind: FeatureInputOperandKind::E1, entity_index: index,
        entity_ref: reference.map(str::to_owned),
    }
}

fn project_solver(ctx: &DecodeContext<'_>) -> Result<(), CodecError> {
    let (sketch, feature, mut lane) = planar_fixture();
    lane.relation_instances[0].family = FeatureInputRelationFamily::LineLineDistance;
    lane.relation_instances[0].operands = vec![operand(0, None), operand(1, None)];
    lane.sketch_entities = vec![
        point("first-start", 0, 0.0, 0.0), point("first-end", 1, 2.0, 0.0),
        point("second-start", 2, 0.0, 1.0), point("second-end", 3, 2.0, 1.0),
    ];
    let mut parameter = parameter();
    parameter.owner = Some(feature.id.clone());
    let mut entities = Vec::new();
    project_relation_solved_line_geometry(ctx, &mut entities, &[sketch], &[feature], &[parameter], &[lane])?;
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
    line.links = SketchInputLinks::new(0, vec![
        SketchInputLink { local_id: 0, entity_ref: "first-start".into() },
        SketchInputLink { local_id: 1, entity_ref: "first-end".into() },
    ]);
    lane.sketch_entities = vec![point("first-start", 10, 0.0, 0.0), point("first-end", 11, 2.0, 0.0), line];
    // The source-frame join needs the already published profile points.
    let mut entities = [("first-start", 0.0), ("first-end", 2.0)].into_iter().map(|(reference, u)| {
        SketchEntity::new(
            SketchEntityId::mint(format!("synthetic:test:entity#{reference}")).unwrap(),
            sketch.id.clone(), SketchGeometry::try_from(SketchGeometryDefinition::Point {
                position: Point2::new(u, 0.0),
            }).unwrap(),
        ).with_native_ref(Some(reference.into()))
    }).collect();
    project_relation_point_geometry(ctx, &mut entities, &[sketch], &[feature], &[lane])?;
    let lines: Vec<_> = entities.iter().filter(|entity| !entity.endpoint_refs.is_empty()).collect();
    assert_eq!(lines.len(), 1);
    assert_eq!(lines[0].endpoint_refs, ["first-start", "first-end"]);
    Ok(())
}

fn spatial_marker(payload: &mut [u8], offset: usize, code: u32, index: u32, position: Point3) -> SketchInputEntity {
    payload[offset - 4..offset].copy_from_slice(&index.to_le_bytes());
    payload[offset..offset + 5].copy_from_slice(&[0xff, 0xff, 0x1f, 0x00, 0x01]);
    payload[offset + 5..offset + 13].fill(0xff);
    payload[offset + 13..offset + 17].copy_from_slice(&[0x00, 0x00, 0x80, 0xbf]);
    payload[offset + 17..offset + 21].copy_from_slice(&code.to_le_bytes());
    let locus = if code == 2 { [0x05, 0x00, 0x01, 0x00] } else { [0x04, 0x00, 0x02, 0x00] };
    payload[offset + 23..offset + 27].copy_from_slice(&locus);
    payload[offset + 27..offset + 29].copy_from_slice(&1u16.to_le_bytes());
    payload[offset + 56..offset + 58].copy_from_slice(&[0x0e, 0x00]);
    for (axis, value) in [position.x, position.y, position.z].into_iter().enumerate() {
        payload[offset + 58 + axis * 8..offset + 66 + axis * 8].copy_from_slice(&(value / 1000.0).to_le_bytes());
    }
    let mut marker = SketchInputEntity::new(format!("marker-{offset}"), "lane", u32::try_from(offset).unwrap(), u64::try_from(offset).unwrap(), SketchInputKind::Point);
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
    let output = spatial_relation_point_line_entities(ctx, &lane.relation_instances[0], &sketch, &parameter(), &mut markers, &mut entities)?;
    let (_, line) = output.expect("unique point-line witness");
    let entity = entities.iter().find(|entity| entity.id() == &line).unwrap();
    assert_eq!(entity.endpoint_refs, ["synthetic:test:sketch#spatial:relation-point:104", "synthetic:test:sketch#spatial:relation-point:204"]);
    Ok(())
}

fn exercise(operation: &'static str, project: fn(&DecodeContext<'_>) -> Result<(), CodecError>) {
    let arena = DecodeArena::new();
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &DecodePolicy::service()).unwrap();
    project(&ctx).unwrap();
    ctx.finish_session().unwrap();
    for (dimension, additional) in [
        (ResourceDimension::RetainedBytes, u64::try_from(2 * std::mem::size_of::<String>()).unwrap()),
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
        let CodecError::ResourceLimit(limit) = error else { panic!("expected endpoint storage refusal"); };
        assert_eq!(limit.dimension, dimension);
        assert_eq!(limit.operation, operation);
        assert_eq!(limit.additional, additional);
        assert_eq!(limit.limit, limit.used);
        assert_eq!(ctx.resource_refusal().as_ref(), Some(&limit));
        assert!(matches!(ctx.finish_session(), Err(CodecError::ResourceLimit(first)) if first == limit));
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
