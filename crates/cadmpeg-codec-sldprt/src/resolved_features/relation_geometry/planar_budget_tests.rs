use super::ownership_tests::relation_lane;
use super::project_relation_bindings;
use crate::records::{FeatureInputOperand, FeatureInputOperandKind};
use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
use cadmpeg_core::CodecError;
use cadmpeg_ir::features::{
    DistinctMembers, Feature, FeatureContent, FeatureDefinition, FeatureEvaluation, FeatureId,
    FeatureOperation, SketchFeatureBinding,
};
use cadmpeg_ir::sketches::{Sketch, SketchId, SketchPlacement, SketchProfiles};
use std::collections::BTreeMap;

fn project_with_policy(policy: DecodePolicy) -> Result<(), CodecError> {
    let arena = DecodeArena::new();
    let (ctx, _) = DecodeContext::from_root_bytes(b"planar relation", &arena, &policy)?;
    let sketch = Sketch {
        id: SketchId::mint("synthetic:test:id#sketch").unwrap(),
        name: None,
        configuration: None,
        visible: None,
        placement: SketchPlacement::Unresolved {},
        profiles: SketchProfiles::default(),
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
            FeatureOperation::Sketch {
                sketch: SketchFeatureBinding::Planar(Some(sketch.id.clone())),
            },
        )),
        native_ref: Some("feature".into()),
    };
    let mut lane = relation_lane();
    lane.relation_instances[0].operands = vec![FeatureInputOperand {
        offset: 0,
        reference_ref: "reference".into(),
        kind: FeatureInputOperandKind::D6,
        entity_index: 0,
        entity_ref: None,
    }];
    let mut constraints = Vec::new();
    project_relation_bindings(
        &ctx,
        &mut constraints,
        &[sketch],
        &[feature],
        &[],
        &[],
        &[lane],
    )?;
    assert_eq!(constraints.len(), 1);
    Ok(())
}

#[test]
fn planar_relation_projection_refuses_collection_limit() {
    let mut policy = DecodePolicy::service();
    policy.limits.max_collection_items = 0;
    let error = project_with_policy(policy).unwrap_err();
    assert!(matches!(error, CodecError::ResourceLimit(limit)
        if limit.dimension == ResourceDimension::CollectionItems
            && limit.operation == "index SLDPRT planar relation sketches"));
}

#[test]
fn planar_relation_projection_refuses_retained_limit() {
    let mut policy = DecodePolicy::service();
    policy.limits.max_retained_bytes = "relation".len() as u64;
    let error = project_with_policy(policy).unwrap_err();
    assert!(matches!(error, CodecError::ResourceLimit(limit)
        if limit.dimension == ResourceDimension::RetainedBytes
            && limit.operation == "format SLDPRT planar relation constraint identity"));
}

#[test]
fn planar_relation_projection_refuses_work_limit() {
    let mut policy = DecodePolicy::service();
    policy.limits.max_work_units = 0;
    let error = project_with_policy(policy).unwrap_err();
    assert!(matches!(error, CodecError::ResourceLimit(limit)
        if limit.dimension == ResourceDimension::WorkUnits
            && limit.operation == "index SLDPRT planar relation sketches"));
}
