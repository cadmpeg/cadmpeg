use super::owned_relation_parameters;
use crate::records::relation_scalars::RelationScalars;
use crate::records::{FeatureInputLane, FeatureInputRelationFamily, FeatureInputRelationInstance};
use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
use cadmpeg_core::CodecError;
use cadmpeg_ir::features::ParameterId;
use std::collections::HashMap;

pub(super) fn relation_lane() -> FeatureInputLane {
    FeatureInputLane {
        id: "lane".into(),
        configuration: None,
        native_payload: Vec::new(),
        classes: Vec::new(),
        names: Vec::new(),
        scalars: Vec::new(),
        relation_bindings: Vec::new(),
        relation_instances: vec![FeatureInputRelationInstance {
            id: "relation".into(),
            parent: "lane".into(),
            ordinal: 0,
            offset: 0,
            family: FeatureInputRelationFamily::CircleDiameter,
            class_ref: "class".into(),
            feature_ref: "feature".into(),
            scalars: RelationScalars::from_refs(vec!["scalar".into()], Some("scalar".into()), None)
                .unwrap(),
            operands: Vec::new(),
        }],
        body_selections: Vec::new(),
        edge_selections: Vec::new(),
        surface_selections: Vec::new(),
        generated_surface_identities: Vec::new(),
        references: Vec::new(),
        sketch_entities: Vec::new(),
    }
}

fn project_with_policy(
    policy: DecodePolicy,
) -> Result<HashMap<String, Option<ParameterId>>, CodecError> {
    let lane = relation_lane();
    let arena = DecodeArena::new();
    let (ctx, _) = DecodeContext::from_root_bytes(b"relation ownership", &arena, &policy)?;
    owned_relation_parameters(&ctx, &[], &[], &[lane])
}

#[test]
fn relation_ownership_keeps_unmatched_driver_unowned() {
    let ownership = project_with_policy(DecodePolicy::service()).unwrap();
    assert_eq!(ownership.get("relation"), Some(&None));
}

#[test]
fn relation_ownership_refuses_collection_limit() {
    let mut policy = DecodePolicy::service();
    policy.limits.max_collection_items = 0;
    let error = project_with_policy(policy).unwrap_err();
    assert!(matches!(
        error,
        CodecError::ResourceLimit(limit)
            if limit.dimension == ResourceDimension::CollectionItems
                && limit.operation == "collect SLDPRT relation lanes"
    ));
}

#[test]
fn relation_ownership_refuses_retained_limit() {
    let error = cadmpeg_test_support::refusal::resource_limit_at(
        cadmpeg_core::decode::ResourceDimension::RetainedBytes,
        "copy SLDPRT relation identity",
        |cap| {
            let mut policy = DecodePolicy::service();
            policy.limits.max_retained_bytes = cap;
            project_with_policy(policy).map(|_| ())
        },
    );
    assert!(matches!(
        error,
        CodecError::ResourceLimit(limit)
            if limit.dimension == ResourceDimension::RetainedBytes
                && limit.operation == "copy SLDPRT relation identity"
    ));
}

#[test]
fn relation_ownership_refuses_work_limit() {
    let mut policy = DecodePolicy::service();
    policy.limits.max_work_units = 0;
    let error = project_with_policy(policy).unwrap_err();
    assert!(matches!(
        error,
        CodecError::ResourceLimit(limit)
            if limit.dimension == ResourceDimension::WorkUnits
                && limit.operation == "scan SLDPRT relation ownership"
    ));
}
