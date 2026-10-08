use super::ownership_tests::relation_lane;
use super::project_spatial_relation_bindings;
use crate::records::relation_scalars::RelationScalars;
use crate::records::{FeatureInputOperand, FeatureInputOperandKind};
use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
use cadmpeg_core::CodecError;
use cadmpeg_ir::features::{
    DistinctMembers, Feature, FeatureContent, FeatureDefinition, FeatureEvaluation, FeatureId,
    FeatureOperation,
};
use cadmpeg_ir::sketches::{SpatialSketch, SpatialSketchId};
use std::collections::BTreeMap;

fn project_with_policy(policy: DecodePolicy) -> Result<(), CodecError> {
    let arena = DecodeArena::new();
    let (ctx, _) = DecodeContext::from_root_bytes(b"spatial relation", &arena, &policy)?;
    let sketch = SpatialSketch {
        id: SpatialSketchId::mint("synthetic:test:id#spatial-sketch").unwrap(),
        name: None,
        configuration: None,
        visible: None,
        profiles: Vec::new(),
        native_ref: Some("lane".into()),
    };
    let feature = Feature {
        id: FeatureId::mint("synthetic:test:id#feature").unwrap(),
        ordinal: 0,
        name: None,
        suppressed: Some(false),
        dependencies: DistinctMembers::default(),
        source_properties: BTreeMap::new(),
        source_tag: None,
        source_text: None,
        source_content: FeatureContent::default(),
        evaluation: FeatureEvaluation::from_definition(FeatureDefinition::Operation(
            FeatureOperation::SpatialSketch {
                sketch: Some(sketch.id.clone()),
            },
        )),
        native_ref: Some("feature".into()),
    };
    let mut lane = relation_lane();
    lane.relation_instances[0].scalars =
        RelationScalars::from_refs(vec!["scalar".into()], None, None).unwrap();
    lane.relation_instances[0].operands = vec![FeatureInputOperand {
        offset: 0,
        reference_ref: "reference".into(),
        kind: FeatureInputOperandKind::D6,
        entity_index: 0,
        entity_ref: None,
    }];
    let mut constraints = Vec::new();
    let mut entities = Vec::new();
    project_spatial_relation_bindings(
        &ctx,
        &mut constraints,
        &mut entities,
        &[sketch],
        &[feature],
        &[],
        &[lane],
    )?;
    assert_eq!(constraints.len(), 1);
    Ok(())
}

#[test]
fn spatial_relation_projection_refuses_collection_limit() {
    let mut policy = DecodePolicy::service();
    policy.limits.max_collection_items = 0;
    let error = project_with_policy(policy).unwrap_err();
    assert!(matches!(error, CodecError::ResourceLimit(limit)
        if limit.dimension == ResourceDimension::CollectionItems
            && limit.operation == "index SLDPRT spatial sketch identities"));
}

#[test]
fn spatial_relation_projection_refuses_retained_limit() {
    let error = cadmpeg_test_support::refusal::resource_limit_at(
        cadmpeg_core::decode::ResourceDimension::RetainedBytes,
        "format SLDPRT spatial relation constraint identity",
        |cap| {
            let mut policy = DecodePolicy::service();
            policy.limits.max_retained_bytes = cap;
            project_with_policy(policy)
        },
    );
    assert!(matches!(error, CodecError::ResourceLimit(limit)
        if limit.dimension == ResourceDimension::RetainedBytes
            && limit.operation == "format SLDPRT spatial relation constraint identity"));
}

#[test]
fn spatial_relation_projection_refuses_work_limit() {
    let mut policy = DecodePolicy::service();
    policy.limits.max_work_units = 0;
    let error = project_with_policy(policy).unwrap_err();
    assert!(matches!(error, CodecError::ResourceLimit(limit)
        if limit.dimension == ResourceDimension::WorkUnits
            && limit.operation == "index SLDPRT spatial sketch identities"));
}

#[test]
fn missing_spatial_point_does_not_build_line_rosters() {
    use super::{spatial_relation_point_line_entities, SpatialRelationMarkers};
    use crate::records::{FeatureInputRelationFamily, SketchInputEntity, SketchInputKind};
    use cadmpeg_ir::features::{DesignParameter, ParameterId, ParameterValue};
    let mut lane = relation_lane();
    lane.native_payload = vec![0; 100];
    let offset = 4;
    let payload = &mut lane.native_payload;
    payload[..4].copy_from_slice(&7u32.to_le_bytes());
    payload[offset..offset + 5].copy_from_slice(&[0xff, 0xff, 0x1f, 0x00, 0x01]);
    payload[offset + 5..offset + 13].fill(0xff);
    payload[offset + 13..offset + 17].copy_from_slice(&[0x00, 0x00, 0x80, 0xbf]);
    payload[offset + 23..offset + 27].copy_from_slice(&[0x04, 0x00, 0x02, 0x00]);
    payload[offset + 27..offset + 29].copy_from_slice(&1u16.to_le_bytes());
    payload[offset + 56..offset + 58].copy_from_slice(&[0x0e, 0x00]);
    let mut marker = SketchInputEntity::new("line", "lane", 0, 4, SketchInputKind::Point)
        .with_test_identity(Some(7), None);
    marker.feature_ref = Some("feature".into());
    lane.sketch_entities.push(marker);
    lane.relation_instances[0].family = FeatureInputRelationFamily::PointLineDistance;
    lane.relation_instances[0].operands = vec![FeatureInputOperand {
        offset: 0,
        reference_ref: "reference".into(),
        kind: FeatureInputOperandKind::D6,
        entity_index: 0,
        entity_ref: None,
    }];
    let parameter = DesignParameter {
        id: ParameterId::mint("synthetic:test:id#parameter").unwrap(),
        owner: None,
        ordinal: 0,
        name: "distance".into(),
        expression: "1mm".into(),
        display: None,
        value: Some(ParameterValue::Length(
            cadmpeg_ir::scalar::Length::new(1.0).unwrap(),
        )),
        dependencies: DistinctMembers::default(),
        properties: BTreeMap::new(),
        pmi: None,
        native_ref: None,
    };
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_work_units = u64::MAX;
    policy.limits.max_retained_bytes = 0;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    let mut index = SpatialRelationMarkers::new(&ctx, &lane).unwrap();
    {
        let _probe = cadmpeg_core::decode::refusal_probe::RefusalProbe::arm(
            ResourceDimension::WorkUnits,
            "index SLDPRT spatial relation line markers",
            None,
        );
        let mut entities = Vec::new();
        assert!(spatial_relation_point_line_entities(
            &ctx,
            &lane.relation_instances[0],
            &SpatialSketchId::mint("synthetic:test:id#sketch").unwrap(),
            &parameter,
            &mut index,
            &mut entities
        )
        .unwrap()
        .is_none());
        assert!(entities.is_empty());
    }
    assert!(index.lines.is_empty());
    let lines = index
        .roster(&ctx, "feature", super::SpatialCarrier::Line)
        .unwrap();
    assert_eq!(lines.len(), 1);
    assert_eq!(lines[0].0.id(), "line");
}

#[test]
fn solved_line_fallback_rosters_only_collect_queried_features() {
    use super::SolvedLinePoints;
    use crate::records::{SketchInputEntity, SketchInputKind};
    let mut lane = relation_lane();
    for (index, feature) in ["queried", "unrelated", "queried"].into_iter().enumerate() {
        let mut marker = SketchInputEntity::new(
            format!("point-{index}"),
            "lane",
            0,
            u64::try_from(3 - index).unwrap(),
            SketchInputKind::Point,
        );
        marker.feature_ref = Some(feature.into());
        marker.coordinates_m = cadmpeg_ir::units::FiniteVector::new([0.0, 0.0]);
        lane.sketch_entities.push(marker);
    }
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_work_units = u64::MAX;
    policy.limits.max_retained_bytes = 0;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    let mut index = SolvedLinePoints::new(&ctx).unwrap();
    assert!(index.by_feature.is_empty());
    assert_eq!(
        index
            .roster(&ctx, &lane, "queried")
            .unwrap()
            .iter()
            .map(|marker| marker.id())
            .collect::<Vec<_>>(),
        ["point-2", "point-0"]
    );
    assert_eq!(index.by_feature.len(), 1);
    {
        let _probe = cadmpeg_core::decode::refusal_probe::RefusalProbe::arm(
            ResourceDimension::WorkUnits,
            "index SLDPRT solved-line point markers",
            Some(3),
        );
        assert_eq!(index.roster(&ctx, &lane, "queried").unwrap().len(), 2);
    }
}
