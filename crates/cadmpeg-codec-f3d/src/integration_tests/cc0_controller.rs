// SPDX-License-Identifier: Apache-2.0

use std::io::Cursor;

use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy};
use cadmpeg_ir::features::{FeatureDefinition, FeatureOperation, PlanarProfileRef, ProfileRef};
use cadmpeg_ir::topology::BodyKind;
use cadmpeg_ir::{Codec, DecodeOptions};
use cadmpeg_measure::{MeasureInput, MeasureOptions, Measurer, Selection};

use crate::F3dCodec;

#[test]
fn controller_manifest_feature_families_and_closed_bodies() {
    let bytes = include_bytes!("../../../../corpus/1WireRGBWController.f3d");
    let decoded = F3dCodec
        .decode(&mut Cursor::new(bytes), &DecodeOptions::default())
        .unwrap();
    let ir = decoded.ir();
    let validation = cadmpeg_ir::validate_neutral(ir, decoded.report().losses.clone()).unwrap();
    assert!(validation.is_ok(), "{validation:#?}");
    assert!(ir.model.features.len() > 1);
    let operations = ir
        .model
        .features
        .iter()
        .map(|feature| match feature.evaluation.definition() {
            FeatureDefinition::Operation(op)
            | FeatureDefinition::PostProcess { operation: op, .. } => op,
        })
        .collect::<Vec<_>>();
    assert!(operations.iter().any(|op| matches!(
        op,
        FeatureOperation::Extrude {
            profile: ProfileRef::Planar(
                PlanarProfileRef::SketchProfiles { .. } | PlanarProfileRef::SketchRegions { .. }
            ),
            ..
        }
    )));
    assert!(operations
        .iter()
        .any(|op| matches!(op, FeatureOperation::Fillet { .. })));
    assert!(operations
        .iter()
        .any(|op| matches!(op, FeatureOperation::Chamfer { .. })));
    assert!(ir
        .model
        .sketch_entities
        .iter()
        .any(|entity| entity.construction));

    let bodies = ir
        .model
        .bodies
        .iter()
        .filter(|body| body.kind == BodyKind::Solid)
        .map(|body| body.id.clone())
        .collect::<Vec<_>>();
    assert!(!bodies.is_empty());
    let arena = DecodeArena::new();
    let policy = DecodePolicy::service();
    let ctx = DecodeContext::for_loaded_input(&arena, &policy, 0).unwrap();
    let report = Measurer::new(
        MeasureInput {
            ir,
            fidelity: None,
            geometry_transferred: true,
        },
        MeasureOptions::default(),
    )
    .measure(
        &ctx,
        &Selection {
            bodies: bodies.clone(),
            ..Selection::default()
        },
    )
    .unwrap();
    assert_eq!(report.bodies().len(), bodies.len());
    for body in report.bodies() {
        assert!(!body.shells().is_empty());
        assert!(body.shells().iter().all(|shell| *shell.closed()));
    }
    ctx.finish_session().unwrap();
}

#[test]
fn controller_1wire_rgbw_corpus_decodes_display_names() {
    let bytes = include_bytes!("../../../../corpus/1WireRGBWController.f3d");
    let decoded = F3dCodec
        .decode(&mut Cursor::new(bytes), &DecodeOptions::default())
        .unwrap();
    let ir = decoded.ir();

    let body_names: Vec<&str> = ir
        .model
        .bodies
        .iter()
        .filter_map(|b| b.name.as_deref())
        .collect();
    assert!(
        body_names.contains(&"Bottom"),
        "Bottom body name expected in {body_names:?}"
    );
    assert!(
        body_names.contains(&"Top"),
        "Top body name expected in {body_names:?}"
    );

    let sketch_names: Vec<&str> = ir
        .model
        .sketches
        .iter()
        .filter_map(|s| s.name.as_deref())
        .collect();
    assert!(
        sketch_names.contains(&"DJ Jack"),
        "DJ Jack sketch expected in {sketch_names:?}"
    );
    assert!(
        sketch_names.contains(&"InnerLip"),
        "InnerLip sketch expected in {sketch_names:?}"
    );
    assert!(
        sketch_names.contains(&"ClipesLeft"),
        "ClipesLeft sketch expected in {sketch_names:?}"
    );

    let feature_names: Vec<&str> = ir
        .model
        .features
        .iter()
        .filter_map(|f| f.name.as_deref())
        .collect();
    assert!(
        feature_names.contains(&"DJ Jack"),
        "DJ Jack feature expected in {feature_names:?}"
    );
    assert!(
        feature_names.contains(&"InnerLip"),
        "InnerLip feature expected in {feature_names:?}"
    );
    assert!(
        feature_names.contains(&"ClipesLeft"),
        "ClipesLeft feature expected in {feature_names:?}"
    );

    let losses = &decoded.report().losses;
    let composed_losses: Vec<_> = losses
        .iter()
        .filter(|loss| {
            loss.code
                == crate::loss::F3dLossCode::SketchNameComposedFromFeatureOrdinal
                    .note("")
                    .code
        })
        .collect();
    assert_eq!(
        composed_losses.len(),
        1,
        "expected exactly one composed sketch name loss note for the remaining 5 default-named sketches"
    );
    assert!(
        composed_losses[0].message.contains("5 sketch name(s)"),
        "composed sketch names loss note must report exactly 5 default sketches, got: {:?}",
        composed_losses[0].message
    );
}
