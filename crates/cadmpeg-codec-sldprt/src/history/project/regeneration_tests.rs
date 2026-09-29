// SPDX-License-Identifier: Apache-2.0
//! Regeneration binding and feature projection refusal tests.
#![allow(clippy::unwrap_used)]

use crate::history::bind::bind_definition_sketch;
use crate::history::project::neutral_feature_id;
use crate::history::project::project_feature_dependencies;
use crate::history::project::project_feature_model;
use crate::history::project::project_features;
use crate::history::tests::feature;
use crate::records::FeatureHistory;
use cadmpeg_ir::features::BooleanOp;
use cadmpeg_ir::features::ExtrudeExtent;
use cadmpeg_ir::features::ExtrudeSide;
use cadmpeg_ir::features::FeatureDefinition;
use cadmpeg_ir::features::FeatureId;
use cadmpeg_ir::features::FeatureOperation;
use cadmpeg_ir::features::LinearTermination;
use cadmpeg_ir::features::ProfileRef;
use std::collections::BTreeMap;
use std::collections::HashMap;

fn with_test_ctx<T>(run: impl FnOnce(&cadmpeg_core::decode::DecodeContext<'_>) -> T) -> T {
    let arena = cadmpeg_core::decode::DecodeArena::new();
    let policy = cadmpeg_core::decode::DecodePolicy::default();
    let (ctx, _) = cadmpeg_core::decode::DecodeContext::from_root_bytes(&[0], &arena, &policy)
        .expect("test decode context");
    run(&ctx)
}

#[test]
fn profile_consumers_require_a_regeneration_profile() {
    let mut definition = FeatureDefinition::Operation(FeatureOperation::Extrude {
        profile: ProfileRef::Planar(cadmpeg_ir::features::PlanarProfileRef::Native(
            "sketch-native".into(),
        )),
        direction: cadmpeg_ir::features::ExtrudeDirection::ProfileNormal {},
        start: cadmpeg_ir::features::ExtrudeStart::ProfilePlane {},
        extent: ExtrudeExtent::OneSided {
            side: ExtrudeSide {
                termination: LinearTermination::Unresolved {},
                draft: None,
            },
        },
        op: BooleanOp::Unresolved,
        solid: None,
        face_maker: None,
        inner_wire_taper: None,
        length_along_profile_normal: None,
        allow_multi_profile_faces: None,
    });
    let sketch = cadmpeg_ir::sketches::SketchId::mint("synthetic:test:id#sketch").unwrap();

    assert!(!with_test_ctx(|ctx| bind_definition_sketch(
        ctx,
        &mut definition,
        "sketch-native",
        &FeatureId::mint("synthetic:test:id#sketch-feature").expect("identity grammar"),
        &sketch,
        false,
    )).unwrap());
    assert!(matches!(
        definition,
        FeatureDefinition::Operation(FeatureOperation::Extrude {
            profile: ProfileRef::Planar(cadmpeg_ir::features::PlanarProfileRef::Native(_)),
            ..
        })
    ));
    assert!(with_test_ctx(|ctx| bind_definition_sketch(
        ctx,
        &mut definition,
        "sketch-native",
        &FeatureId::mint("synthetic:test:id#sketch-feature").expect("identity grammar"),
        &sketch,
        true,
    )).unwrap());
    assert!(matches!(
        definition,
        FeatureDefinition::Operation(FeatureOperation::Extrude {
            profile: ProfileRef::Planar(cadmpeg_ir::features::PlanarProfileRef::Sketch(ref bound)),
            ..
        }) if bound == &sketch
    ));
}

#[test]
fn exact_native_profile_source_projects_a_feature_dependency() {
    let mut sketch = feature("sketch", Some("42"), 0);
    sketch.kind = "Sketch".into();
    sketch.input_class = Some("moProfileFeature_c".into());
    let mut extrusion = feature("extrusion", Some("43"), 1);
    extrusion.kind = "Extrusion".into();
    extrusion.input_class = Some("moExtrusion_c".into());
    extrusion
        .properties
        .insert(cadmpeg_core::nonblank_literal!("Profile"), "42".into());
    extrusion
        .properties
        .insert(cadmpeg_core::nonblank_literal!("Operation"), "Join".into());
    extrusion
        .parameters
        .insert(cadmpeg_core::nonblank_literal!("D1"), "5".into());
    let history = FeatureHistory {
        id: "history".into(),
        part_name: None,
        properties: BTreeMap::new(),
        content: Vec::new(),
        configurations: Vec::new(),
        features: vec![sketch, extrusion],
    };

    let projected = project_features(&cadmpeg_test_support::service_decode_context(), &[history]).unwrap();
    let sketch_id = neutral_feature_id("sketch");
    assert!(matches!(
        projected[1].evaluation.definition(),
        FeatureDefinition::Operation(FeatureOperation::Extrude {
            profile: ProfileRef::Planar(cadmpeg_ir::features::PlanarProfileRef::Feature(feature)),
            ..
        }) if feature == &sketch_id
    ));
    assert_eq!(projected[1].dependencies.as_slice(), [sketch_id]);
}

#[test]
fn a_regeneration_edge_the_model_refuses_is_reported_as_one_loss() {
    // The projection offers every edge the source states. A parent that does
    // not precede its child is the model's condition, and the projection does
    // not recompute it: the edge is offered, refused, and reported.
    let mut child = feature("sldprt:history:feature#0:0", None, 0);
    child.tree_parent = Some(crate::records::TreeParent::Record {
        record_id: "sldprt:history:feature#0:1".into(),
        source_id: None,
    });
    let history = FeatureHistory {
        id: "history".into(),
        part_name: None,
        properties: BTreeMap::new(),
        content: Vec::new(),
        configurations: Vec::new(),
        features: vec![child, feature("sldprt:history:feature#0:1", None, 1)],
    };
    let projection = project_feature_model(&cadmpeg_test_support::service_decode_context(), &[history]).unwrap();
    let (model, losses) = projection.into_model();
    let child_id = model.features[0].id.clone();
    assert!(model.feature_regeneration_parent(&child_id).is_none());
    assert_eq!(losses.len(), 1);
    assert!(
        losses[0].message.contains("was not installed")
            && losses[0].message.contains("does not precede"),
        "{}",
        losses[0].message
    );
}

#[test]
fn projected_tree_child_refuses_collection_limit() {
    let mut parent = feature("sldprt:history:feature#0:1", None, 0);
    parent.xml_tag = "Feature".to_owned();
    parent.kind = "EquationDriven".to_owned();
    let mut child = feature("sldprt:history:feature#0:2", None, 1);
    child.tree_parent = Some(crate::records::TreeParent::Record {
        record_id: parent.id.clone(),
        source_id: None,
    });
    let history = FeatureHistory {
        id: "history".to_owned(),
        part_name: None,
        properties: BTreeMap::new(),
        content: Vec::new(),
        configurations: Vec::new(),
        features: vec![parent, child],
    };
    let mut policy = cadmpeg_core::decode::DecodePolicy::service();
    policy.limits.max_collection_items = 0;
    let arena = cadmpeg_core::decode::DecodeArena::new();
    let (ctx, _) = cadmpeg_core::decode::DecodeContext::from_root_bytes(
        b"tree", &arena, &policy,
    ).unwrap();
    let error = project_feature_model(&ctx, &[history]).err().unwrap();
    assert!(matches!(error, cadmpeg_core::CodecError::ResourceLimit(_)));
}

#[test]
fn projected_feature_text_refuses_retained_limit() {
    let mut projected = feature("sldprt:history:feature#0:1", None, 0);
    projected.name = "Named feature".to_owned();
    let history = FeatureHistory {
        id: "history".to_owned(),
        part_name: None,
        properties: BTreeMap::new(),
        content: Vec::new(),
        configurations: Vec::new(),
        features: vec![projected],
    };
    let mut policy = cadmpeg_core::decode::DecodePolicy::service();
    policy.limits.max_retained_bytes = 0;
    let arena = cadmpeg_core::decode::DecodeArena::new();
    let (ctx, _) = cadmpeg_core::decode::DecodeContext::from_root_bytes(
        b"feature", &arena, &policy,
    ).unwrap();
    let error = project_feature_model(&ctx, &[history]).err().unwrap();
    assert!(matches!(error, cadmpeg_core::CodecError::ResourceLimit(_)));
}

#[test]
fn projected_source_index_refuses_collection_limit() {
    let history = FeatureHistory {
        id: "history".to_owned(),
        part_name: None,
        properties: BTreeMap::new(),
        content: Vec::new(),
        configurations: Vec::new(),
        features: vec![feature("sldprt:history:feature#0:1", None, 0)],
    };
    let mut policy = cadmpeg_core::decode::DecodePolicy::service();
    policy.limits.max_collection_items = 0;
    let arena = cadmpeg_core::decode::DecodeArena::new();
    let (ctx, _) = cadmpeg_core::decode::DecodeContext::from_root_bytes(
        b"source", &arena, &policy,
    ).unwrap();
    let error = project_feature_model(&ctx, &[history]).err().unwrap();
    assert!(matches!(
        error,
        cadmpeg_core::CodecError::ResourceLimit(limit)
            if limit.operation == "index SLDPRT projected source features"
    ));
}

#[test]
fn projected_dependencies_refuse_collection_limit() {
    let mut consumer = feature("sldprt:history:feature#0:1", None, 0);
    consumer.properties.insert(
        cadmpeg_core::nonblank_literal!("Dependency"),
        "2".to_owned(),
    );
    let source = HashMap::from([(
        "2".to_owned(),
        FeatureId::mint("sldprt:model:feature#0:2").unwrap(),
    )]);
    let mut policy = cadmpeg_core::decode::DecodePolicy::service();
    policy.limits.max_collection_items = 0;
    let arena = cadmpeg_core::decode::DecodeArena::new();
    let (ctx, _) = cadmpeg_core::decode::DecodeContext::from_root_bytes(
        b"dependencies", &arena, &policy,
    ).unwrap();
    let error = project_feature_dependencies(&ctx, &consumer, &source)
        .err()
        .unwrap();
    assert!(matches!(
        error,
        cadmpeg_core::CodecError::ResourceLimit(limit)
            if limit.operation == "collect SLDPRT feature dependencies"
    ));
}
