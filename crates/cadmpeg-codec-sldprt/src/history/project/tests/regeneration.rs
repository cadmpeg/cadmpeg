// SPDX-License-Identifier: Apache-2.0
//! Regeneration binding and feature projection tests.
#![allow(clippy::unwrap_used)]

use crate::history::bind::bind_definition_sketch;
use crate::history::project::neutral_feature_id;
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

    assert!(!bind_definition_sketch(
        &mut definition,
        "sketch-native",
        &FeatureId::mint("synthetic:test:id#sketch-feature").expect("identity grammar"),
        &sketch,
        false,
    ));
    assert!(matches!(
        definition,
        FeatureDefinition::Operation(FeatureOperation::Extrude {
            profile: ProfileRef::Planar(cadmpeg_ir::features::PlanarProfileRef::Native(_)),
            ..
        })
    ));
    assert!(bind_definition_sketch(
        &mut definition,
        "sketch-native",
        &FeatureId::mint("synthetic:test:id#sketch-feature").expect("identity grammar"),
        &sketch,
        true,
    ));
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

    let projected =
        project_features(&cadmpeg_test_support::service_decode_context(), &[history]).unwrap();
    let sketch_id = neutral_feature_id("sketch").unwrap();
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
    let projection =
        project_feature_model(&cadmpeg_test_support::service_decode_context(), &[history]).unwrap();
    let (model, losses) = projection.into_model().unwrap();
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

fn section_sketch() -> cadmpeg_ir::sketches::SketchId {
    cadmpeg_ir::sketches::SketchId::mint("synthetic:test:id#section").unwrap()
}

/// Projected features whose first is a sweep on `section_sketch` and whose
/// others each own that sketch.
fn sweep_then_sketch_owners(owners: usize) -> Vec<cadmpeg_ir::features::Feature> {
    let native = (0..=owners)
        .map(|index| {
            feature(
                &format!("sldprt:history:feature#0:{index}"),
                None,
                u32::try_from(index).unwrap(),
            )
        })
        .collect();
    let history = FeatureHistory {
        id: "history".into(),
        part_name: None,
        properties: BTreeMap::new(),
        content: Vec::new(),
        configurations: Vec::new(),
        features: native,
    };
    let mut features = project_features(&cadmpeg_test_support::service_decode_context(), &[history]).unwrap();
    features[0].evaluation.edit(|definition, _| {
        *definition = FeatureDefinition::Operation(FeatureOperation::Sweep {
            shape: cadmpeg_ir::features::SweepShape::sheet_sections(
                cadmpeg_ir::features::SweepMode::Surface {},
                cadmpeg_ir::features::SweepSection::Profile(section_sketch().into()),
                Vec::new(),
            ),
            path: None,
            orientation: None,
            transition: None,
            transformation: None,
            path_tangent: false,
            linearize: false,
            twist: None,
            path_extent: None,
            guide_rail: None,
            taper: None,
            scale: None,
            allow_multi_profile_faces: None,
        });
    });
    for owner in &mut features[1..] {
        owner.evaluation.edit(|definition, _| {
            *definition = FeatureDefinition::Operation(FeatureOperation::Sketch {
                sketch: cadmpeg_ir::features::SketchFeatureBinding::Planar(Some(section_sketch())),
            });
        });
    }
    features
}

fn order(features: &mut [cadmpeg_ir::features::Feature]) -> bool {
    let arena = cadmpeg_core::decode::DecodeArena::new();
    let (ctx, _) = cadmpeg_core::decode::DecodeContext::from_root_bytes(
        &[],
        &arena,
        &cadmpeg_core::decode::DecodePolicy::service(),
    )
    .expect("test context");
    crate::history::bind::order_features_for_regeneration(&ctx, features).unwrap()
}

#[test]
fn a_sweep_regenerates_after_the_sketch_feature_that_owns_its_section() {
    let mut features = sweep_then_sketch_owners(1);
    assert!(order(&mut features));
    assert_eq!(
        features[0].dependencies.as_slice(),
        [features[1].id.clone()]
    );
    assert!(features[1].ordinal < features[0].ordinal);
}

#[test]
fn a_section_sketch_that_several_features_own_adds_no_dependency() {
    let mut features = sweep_then_sketch_owners(2);
    let before = features[0].ordinal;
    assert!(order(&mut features));
    assert!(features[0].dependencies.as_slice().is_empty());
    assert_eq!(features[0].ordinal, before);
}

#[test]
fn a_sweep_section_owner_that_depends_on_the_sweep_keeps_the_stored_order() {
    let mut features = sweep_then_sketch_owners(1);
    let sweep = features[0].id.clone();
    features[1].dependencies.insert(sweep);
    let before = [features[0].ordinal, features[1].ordinal];
    assert!(!order(&mut features));
    assert_eq!([features[0].ordinal, features[1].ordinal], before);
}
