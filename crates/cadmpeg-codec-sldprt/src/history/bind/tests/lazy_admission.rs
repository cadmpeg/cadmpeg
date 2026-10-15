// SPDX-License-Identifier: Apache-2.0
//! Binding routes admit only their live consumers.

use super::super::{bind_unique_sketch_feature, derive_feature_outputs, FeatureOutputSources};
use super::{face_owner_fixture, ordering_feature};
use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};

#[test]
fn empty_output_sources_do_not_index_history_ordinals() {
    let mut policy = DecodePolicy::service();
    policy.limits.max_materialized_bytes = 0;
    policy.limits.max_work_units = 0;
    let arena = DecodeArena::new();
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    let histories = [crate::records::FeatureHistory {
        id: "history".into(),
        part_name: None,
        properties: std::collections::BTreeMap::default(),
        content: Vec::new(),
        configurations: Vec::new(),
        features: vec![crate::history::tests::feature("native", Some("700"), 0)],
    }];
    let mut features = [ordering_feature()];
    let original = features.clone();
    derive_feature_outputs(
        &ctx,
        &mut features,
        &histories,
        FeatureOutputSources {
            face_producers: &[],
            body_modifiers: &[],
        },
        &[],
        &[],
        &[],
    )
    .unwrap();
    assert_eq!(features, original);
    ctx.finish_session().unwrap();
}

#[test]
fn binding_without_sketch_features_does_not_allocate_indexes() {
    let fixture = face_owner_fixture();
    let histories = [crate::records::FeatureHistory {
        id: "history".into(),
        part_name: None,
        properties: std::collections::BTreeMap::default(),
        content: Vec::new(),
        configurations: Vec::new(),
        features: vec![crate::history::tests::feature("native", Some("700"), 0)],
    }];
    let mut features = [ordering_feature()];
    let original = features.clone();
    let mut policy = DecodePolicy::service();
    policy.limits.max_materialized_bytes = 0;
    let arena = DecodeArena::new();
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    bind_unique_sketch_feature(
        &ctx,
        &mut features,
        &fixture.ir().model.sketches,
        &histories,
    )
    .unwrap();
    assert_eq!(features, original);
    ctx.finish_session().unwrap();
}

#[test]
fn duplicate_body_modifiers_copy_only_the_published_body() {
    let mut native = crate::history::tests::feature("native", Some("700"), 0);
    native.kind = "Custom".into();
    let history = crate::records::FeatureHistory {
        id: "history".into(),
        part_name: None,
        properties: std::collections::BTreeMap::default(),
        content: Vec::new(),
        configurations: Vec::new(),
        features: vec![native],
    };
    let body = "synthetic:test:body#published";
    let modifiers = vec![(body.to_owned(), 1); 64];
    let mut feature = ordering_feature();
    feature.native_ref = Some("native".into());
    let mut features = [feature];
    let mut policy = DecodePolicy::service();
    // One body identity and the first output-vector growth survive.
    policy.limits.max_retained_bytes =
        u64::try_from(body.len() + 4 * std::mem::size_of::<cadmpeg_ir::ids::BodyId>()).unwrap();
    let arena = DecodeArena::new();
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    derive_feature_outputs(
        &ctx,
        &mut features,
        &[history],
        FeatureOutputSources {
            face_producers: &[],
            body_modifiers: &modifiers,
        },
        &[],
        &[],
        &[],
    )
    .unwrap();
    assert_eq!(
        features[0].evaluation.outputs(),
        &[cadmpeg_ir::ids::BodyId::mint(body).unwrap()]
    );
    ctx.finish_session().unwrap();
}

#[test]
fn invalid_body_modifier_does_not_retain_candidate_storage() {
    let native = crate::history::tests::feature("native", Some("700"), 0);
    let history = crate::records::FeatureHistory {
        id: "history".into(),
        part_name: None,
        properties: std::collections::BTreeMap::default(),
        content: Vec::new(),
        configurations: Vec::new(),
        features: vec![native],
    };
    let modifiers = [("invalid body identity".to_owned(), 1)];
    let mut feature = ordering_feature();
    feature.native_ref = Some("native".into());
    let mut features = [feature];
    let mut policy = DecodePolicy::service();
    policy.limits.max_retained_bytes = 0;
    let arena = DecodeArena::new();
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    derive_feature_outputs(
        &ctx,
        &mut features,
        &[history],
        FeatureOutputSources {
            face_producers: &[],
            body_modifiers: &modifiers,
        },
        &[],
        &[],
        &[],
    )
    .unwrap();
    assert!(features[0].evaluation.outputs().is_empty());
    ctx.finish_session().unwrap();
}

#[test]
fn face_only_outputs_do_not_index_body_modifier_ordinals() {
    let fixture = face_owner_fixture();
    let model = &fixture.ir().model;
    let histories = [crate::records::FeatureHistory {
        id: "history".into(),
        part_name: None,
        properties: std::collections::BTreeMap::default(),
        content: Vec::new(),
        configurations: Vec::new(),
        features: vec![crate::history::tests::feature("native", Some("700"), 0)],
    }];
    let mut feature = ordering_feature();
    feature.native_ref = Some("native".into());
    let mut features = [feature];
    for dimension in [
        ResourceDimension::MaterializedBytes,
        ResourceDimension::WorkUnits,
    ] {
        let mut policy = DecodePolicy::service();
        policy.limits.max_materialized_bytes = u64::MAX;
        policy.limits.max_work_units = u64::MAX;
        let arena = DecodeArena::new();
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
        let _probe = cadmpeg_core::decode::refusal_probe::RefusalProbe::arm(
            dimension,
            "index SLDPRT body modifier ordinals",
            None,
        );
        derive_feature_outputs(
            &ctx,
            &mut features,
            &histories,
            FeatureOutputSources {
                face_producers: &[(model.faces[0].id.as_str(), 700)],
                body_modifiers: &[],
            },
            &model.faces,
            &model.shells,
            &model.regions,
        )
        .unwrap();
        assert_eq!(
            features[0].evaluation.outputs(),
            std::slice::from_ref(&model.bodies[0].id)
        );
        ctx.finish_session().unwrap();
    }
}

#[test]
fn existing_sketch_alias_dependency_does_not_copy_the_base_identity() {
    use cadmpeg_ir::features::{
        DistinctMembers, FeatureDefinition, FeatureId, FeatureOperation, SketchFeatureBinding,
    };
    let base =
        FeatureId::mint(format!("synthetic:test:feature#{}", "x".repeat(32 * 1024))).unwrap();
    let mut first = ordering_feature();
    first.id = base.clone();
    first.name = Some("Profile".into());
    first.native_ref = Some("native-base".into());
    first
        .evaluation
        .set_definition(FeatureDefinition::Operation(FeatureOperation::Sketch {
            sketch: SketchFeatureBinding::Planar(None),
        }));
    let mut alias = first.clone();
    alias.id = FeatureId::mint("synthetic:test:feature#alias").unwrap();
    alias.name = Some("Profile<1>".into());
    alias.native_ref = Some("native-alias".into());
    alias.dependencies = DistinctMembers::try_from(
        vec![base.clone()],
        &cadmpeg_test_support::service_decode_context(),
    )
    .unwrap();
    let native = |id: &str, name: &str| {
        let mut feature = crate::history::tests::feature(id, None, 0);
        feature.name = name.into();
        feature.kind = "Sketch".into();
        feature.xml_tag = "Sketch".into();
        feature.input_class = Some("moProfileFeature_c".into());
        feature
    };
    let histories = [crate::records::FeatureHistory {
        id: "history".into(),
        part_name: None,
        properties: std::collections::BTreeMap::default(),
        content: Vec::new(),
        configurations: Vec::new(),
        features: vec![
            native("native-base", "Profile"),
            native("native-alias", "Profile<1>"),
        ],
    }];
    let sketches = [cadmpeg_ir::sketches::Sketch {
        id: cadmpeg_ir::sketches::SketchId::mint("synthetic:test:sketch#profile").unwrap(),
        name: Some("Profile".into()),
        configuration: None,
        visible: None,
        placement: cadmpeg_ir::sketches::SketchPlacement::Unresolved {},
        profiles: cadmpeg_ir::sketches::SketchProfiles::default(),
        native_ref: None,
    }];
    let mut policy = DecodePolicy::service();
    policy.limits.max_retained_bytes = 1024;
    let arena = DecodeArena::new();
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    let mut features = [first, alias];
    bind_unique_sketch_feature(&ctx, &mut features, &sketches, &histories).unwrap();
    assert_eq!(features[1].dependencies.as_slice(), &[base]);
    ctx.finish_session().unwrap();
}

#[test]
fn unnamed_sketch_index_admits_every_filtered_source_visit() {
    use cadmpeg_ir::features::{FeatureDefinition, FeatureOperation, SketchFeatureBinding};
    let mut feature = ordering_feature();
    feature.name = None;
    feature.native_ref = None;
    feature
        .evaluation
        .set_definition(FeatureDefinition::Operation(FeatureOperation::Sketch {
            sketch: SketchFeatureBinding::Planar(None),
        }));
    let sketches = vec![
        cadmpeg_ir::sketches::Sketch {
            id: cadmpeg_ir::sketches::SketchId::mint("synthetic:test:sketch#unnamed").unwrap(),
            name: None,
            configuration: None,
            visible: None,
            placement: cadmpeg_ir::sketches::SketchPlacement::Unresolved {},
            profiles: cadmpeg_ir::sketches::SketchProfiles::default(),
            native_ref: None,
        };
        64
    ];
    let mut features = [feature];
    let expected = features.clone();
    bind_unique_sketch_feature(
        &cadmpeg_test_support::service_decode_context(),
        &mut features,
        &sketches,
        &[],
    )
    .unwrap();
    assert_eq!(features, expected);
    let error = cadmpeg_test_support::refusal::resource_limit_at(
        ResourceDimension::WorkUnits,
        "scan SLDPRT sketch names",
        |cap| {
            let arena = DecodeArena::new();
            let mut policy = DecodePolicy::service();
            policy.limits.max_work_units = cap;
            let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy)?;
            bind_unique_sketch_feature(&ctx, &mut expected.clone(), &sketches, &[])?;
            ctx.finish_session()
        },
    );
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
        if limit.dimension == ResourceDimension::WorkUnits
            && limit.operation == "scan SLDPRT sketch names"
            && limit.additional >= 64)
    );
}

#[test]
fn output_identity_constructor_preserves_copy_refusals() {
    const OPERATION: &str = "retain SLDPRT feature output body";
    for dimension in [
        ResourceDimension::MaterializedBytes,
        ResourceDimension::WorkUnits,
    ] {
        let error = cadmpeg_test_support::refusal::resource_limit_at(dimension, OPERATION, |cap| {
            let arena = DecodeArena::new();
            let mut policy = DecodePolicy::service();
            match dimension {
                ResourceDimension::MaterializedBytes => policy.limits.max_materialized_bytes = cap,
                ResourceDimension::WorkUnits => policy.limits.max_work_units = cap,
                _ => unreachable!(),
            }
            let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy)?;
            super::super::output_body_id(&ctx, "synthetic:test:body#output")?;
            ctx.finish_session()
        });
        assert!(
            matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
            if limit.dimension == dimension && limit.operation == OPERATION)
        );
    }
}

#[test]
fn rejected_output_identity_releases_candidate_storage() {
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_retained_bytes = 0;
    policy.limits.max_materialized_bytes = 4096;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    assert_eq!(super::super::output_body_id(&ctx, "invalid").unwrap(), None);
    ctx.reserve_scoped(4096, "released output candidate")
        .unwrap();
    ctx.finish_session().unwrap();
}

#[test]
fn output_identity_constructor_preserves_visited_grammar_refusal() {
    const OPERATION: &str = "retain SLDPRT feature output body";
    let error = cadmpeg_test_support::refusal::resource_limit_at(
        ResourceDimension::WorkUnits,
        OPERATION,
        |cap| {
            let arena = DecodeArena::new();
            let mut policy = DecodePolicy::service();
            policy.limits.max_work_units = cap;
            let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy)?;
            // Grammar admits one unit per visited character. The identity copy
            // has a larger byte count, and its initial growth relocates no bytes.
            let _grammar = cadmpeg_core::decode::refusal_probe::RefusalProbe::arm(
                ResourceDimension::WorkUnits,
                OPERATION,
                Some(1),
            );
            super::super::output_body_id(&ctx, "synthetic:test:body#output")?;
            ctx.finish_session()
        },
    );
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
        if limit.dimension == ResourceDimension::WorkUnits && limit.operation == OPERATION && limit.additional == 1)
    );
}
