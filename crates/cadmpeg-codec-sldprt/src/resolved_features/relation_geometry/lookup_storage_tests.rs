//! Borrowed relation parameter lookups own only temporary table backing.

use super::{relation_parameter_by_display_name, relation_parameter_by_driving_name};
use crate::records::relation_scalars::RelationScalars;
use crate::records::{
    FeatureInputLane, FeatureInputName, FeatureInputRelationFamily, FeatureInputScalar,
    FeatureInputScalarRole,
};
use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
use cadmpeg_core::CodecError;
use cadmpeg_ir::features::{
    DesignParameter, DistinctMembers, Feature, FeatureContent, FeatureDefinition,
    FeatureEvaluation, FeatureId, FeatureOperation, ParameterId, ParameterValue,
    SketchFeatureBinding,
};
use cadmpeg_ir::scalar::{FiniteReal, Length};
use std::collections::BTreeMap;

// Core admits four buckets, four control bytes, sixteen trailing controls,
// and max(element alignment, sixteen)-one padding for a first table.
const SCALAR_TABLE_BYTES: u64 = 4 * cadmpeg_core::decode::u64_from_index(std::mem::size_of::<(&str, &FeatureInputScalar)>())
    + 4
    + 16
    + cadmpeg_core::decode::u64_from_index((if std::mem::align_of::<(&str, &FeatureInputScalar)>() > 16 {
        std::mem::align_of::<(&str, &FeatureInputScalar)>()
    } else {
        16
    }) - 1);
const NAME_TABLE_BYTES: u64 = 4 * cadmpeg_core::decode::u64_from_index(std::mem::size_of::<(&str, &str)>())
    + 4
    + 16
    + cadmpeg_core::decode::u64_from_index((if std::mem::align_of::<(&str, &str)>() > 16 {
        std::mem::align_of::<(&str, &str)>()
    } else {
        16
    }) - 1);
const DRIVING_BYTES: u64 = SCALAR_TABLE_BYTES + NAME_TABLE_BYTES;

fn fixture(role: FeatureInputScalarRole) -> (Feature, DesignParameter, FeatureInputLane) {
    let feature = Feature {
        id: FeatureId::mint("synthetic:test:feature#relation-lookup").unwrap(),
        ordinal: 0,
        name: None,
        suppressed: None,
        dependencies: DistinctMembers::default(),
        source_properties: BTreeMap::new(),
        source_tag: None,
        source_text: None,
        source_content: FeatureContent::default(),
        evaluation: FeatureEvaluation::from_definition(FeatureDefinition::Operation(
            FeatureOperation::Sketch {
                sketch: SketchFeatureBinding::Planar(None),
            },
        )),
        native_ref: Some("feature".into()),
    };
    let parameter = DesignParameter {
        id: ParameterId::mint("synthetic:test:parameter#relation-lookup").unwrap(),
        owner: Some(feature.id.clone()),
        ordinal: 0,
        name: "D1".into(),
        expression: "12mm".into(),
        display: None,
        value: Some(ParameterValue::Length(Length::new(12.0).unwrap())),
        dependencies: DistinctMembers::default(),
        properties: BTreeMap::new(),
        pmi: None,
        native_ref: None,
    };
    let mut lane = super::ownership_tests::relation_lane();
    let relation = &mut lane.relation_instances[0];
    relation.family = FeatureInputRelationFamily::PointPointDistance;
    relation.scalars = match role {
        FeatureInputScalarRole::Driving => {
            RelationScalars::from_refs(vec!["scalar".into()], Some("scalar".into()), None)
        }
        FeatureInputScalarRole::Display => {
            RelationScalars::from_refs(vec!["scalar".into()], None, Some("scalar".into()))
        }
        FeatureInputScalarRole::Native => unreachable!("fixture uses driving or display scalars"),
    }
    .unwrap();
    lane.names = ["D0", "D1"]
        .into_iter()
        .enumerate()
        .map(|(ordinal, value)| FeatureInputName {
            id: "name".into(),
            parent: "lane".into(),
            ordinal: u32::try_from(ordinal).unwrap(),
            offset: u64::try_from(ordinal).unwrap(),
            object_id: None,
            value: value.into(),
        })
        .collect();
    let scalar = |role| FeatureInputScalar {
        id: "scalar".into(),
        parent: "lane".into(),
        feature_ref: Some("feature".into()),
        ordinal: 0,
        offset: 0,
        object_id: 0,
        name: "name".into(),
        value: FiniteReal::new(0.012).unwrap(),
        role,
        operands: Vec::new(),
    };
    if role == FeatureInputScalarRole::Driving {
        lane.scalars.push(scalar(FeatureInputScalarRole::Display));
    }
    lane.scalars.push(scalar(role));
    (feature, parameter, lane)
}

#[test]
fn relation_lookup_storage_driving_reuses_tables_and_keeps_last_values() {
    let (feature, parameter, lane) = fixture(FeatureInputScalarRole::Driving);
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_retained_bytes = 0;
    policy.limits.max_materialized_bytes = DRIVING_BYTES;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    for _ in 0..2 {
        let selected = relation_parameter_by_driving_name(
            &ctx,
            &lane.relation_instances[0],
            &lane,
            std::slice::from_ref(&feature),
            std::slice::from_ref(&parameter),
        )
        .unwrap()
        .unwrap();
        assert!(std::ptr::eq(selected, &raw const parameter));
        ctx.reserve_scoped(DRIVING_BYTES, "reuse relation driving lookup storage")
            .unwrap();
    }
    assert!(ctx.resource_refusal().is_none());
}

#[test]
fn relation_lookup_storage_display_reuses_tables_and_preserves_value_gate() {
    let (feature, mut parameter, lane) = fixture(FeatureInputScalarRole::Display);
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_retained_bytes = 0;
    policy.limits.max_materialized_bytes = NAME_TABLE_BYTES;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    for _ in 0..2 {
        let selected = relation_parameter_by_display_name(
            &ctx,
            &lane.relation_instances[0],
            &lane,
            std::slice::from_ref(&feature),
            std::slice::from_ref(&parameter),
        )
        .unwrap()
        .unwrap();
        assert!(std::ptr::eq(selected, &raw const parameter));
        ctx.reserve_scoped(NAME_TABLE_BYTES, "reuse relation display lookup storage")
            .unwrap();
    }
    parameter.value = Some(ParameterValue::Length(Length::new(20.0).unwrap()));
    assert!(relation_parameter_by_display_name(
        &ctx,
        &lane.relation_instances[0],
        &lane,
        std::slice::from_ref(&feature),
        std::slice::from_ref(&parameter),
    )
    .unwrap()
    .is_none());
    ctx.reserve_scoped(NAME_TABLE_BYTES, "reuse rejected relation display lookup")
        .unwrap();
    assert!(ctx.resource_refusal().is_none());
}

#[test]
fn relation_lookup_storage_driving_preserves_original_refusal() {
    let (feature, parameter, lane) = fixture(FeatureInputScalarRole::Driving);
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_retained_bytes = 0;
    policy.limits.max_materialized_bytes = DRIVING_BYTES - 1;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    let run = || {
        relation_parameter_by_driving_name(
            &ctx,
            &lane.relation_instances[0],
            &lane,
            std::slice::from_ref(&feature),
            std::slice::from_ref(&parameter),
        )
    };
    let CodecError::ResourceLimit(limit) = run().unwrap_err() else {
        panic!("expected driving lookup storage refusal");
    };
    assert_eq!(limit.dimension, ResourceDimension::MaterializedBytes);
    assert_eq!(limit.operation, "index SLDPRT relation driving names");
    assert_eq!(limit.used, SCALAR_TABLE_BYTES);
    assert_eq!(limit.additional, NAME_TABLE_BYTES);
    assert_eq!(limit.limit, DRIVING_BYTES - 1);
    assert_eq!(ctx.resource_refusal().as_ref(), Some(&limit));
    let CodecError::ResourceLimit(repeated) = run().unwrap_err() else {
        panic!("expected sticky driving lookup refusal");
    };
    assert_eq!(repeated, limit);
}

#[test]
fn relation_lookup_storage_display_preserves_original_refusal() {
    let (feature, parameter, lane) = fixture(FeatureInputScalarRole::Display);
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_retained_bytes = 0;
    policy.limits.max_materialized_bytes = NAME_TABLE_BYTES - 1;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    let run = || {
        relation_parameter_by_display_name(
            &ctx,
            &lane.relation_instances[0],
            &lane,
            std::slice::from_ref(&feature),
            std::slice::from_ref(&parameter),
        )
    };
    let CodecError::ResourceLimit(limit) = run().unwrap_err() else {
        panic!("expected display lookup storage refusal");
    };
    assert_eq!(limit.dimension, ResourceDimension::MaterializedBytes);
    assert_eq!(limit.operation, "index SLDPRT relation display names");
    assert_eq!(limit.used, 0);
    assert_eq!(limit.additional, NAME_TABLE_BYTES);
    assert_eq!(limit.limit, NAME_TABLE_BYTES - 1);
    assert_eq!(ctx.resource_refusal().as_ref(), Some(&limit));
    let CodecError::ResourceLimit(repeated) = run().unwrap_err() else {
        panic!("expected sticky display lookup refusal");
    };
    assert_eq!(repeated, limit);
}
