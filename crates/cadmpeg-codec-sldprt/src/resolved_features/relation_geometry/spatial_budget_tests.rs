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
