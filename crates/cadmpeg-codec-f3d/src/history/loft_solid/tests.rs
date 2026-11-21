// SPDX-License-Identifier: Apache-2.0
use super::bind;
use cadmpeg_ir::features::{
    BooleanOp, Feature, FeatureDefinition, FeatureId, FeatureOperation, LoftGuidance,
};
use cadmpeg_ir::ids::BodyId;
use cadmpeg_ir::topology::{Body, BodyKind};
use std::collections::HashMap;

fn body_id(id: &str) -> BodyId {
    BodyId::mint(id).expect("identity grammar")
}

fn body(id: &str, kind: BodyKind) -> Body {
    Body {
        id: body_id(id),
        kind,
        transform: None,
        name: None,
        color: None,
        visible: None,
        native_ref: None,
    }
}

fn loft(id: &str, op: BooleanOp, outputs: &[&str]) -> Feature {
    Feature {
        product_definition: None,
        id: FeatureId::mint(id).expect("identity grammar"),
        ordinal: 0,
        name: None,
        suppressed: None,
        dependencies: Default::default(),
        source_properties: Default::default(),
        source_tag: None,
        source_text: None,
        source_content: Default::default(),
        evaluation: cadmpeg_ir::features::FeatureEvaluation::new(
            FeatureDefinition::Operation(FeatureOperation::Loft {
                sections: Vec::new(),
                guidance: LoftGuidance::Guides(Vec::new()),
                op,
                closed: false,
                solid: true,
                ruled: false,
                linearize: false,
                max_degree: None,
                allow_multi_profile_faces: None,
            }),
            cadmpeg_ir::features::DistinctMembers::try_from(
                outputs.iter().map(|id| body_id(id)).collect::<Vec<_>>(),
            )
            .unwrap(),
        ),
        native_ref: None,
    }
}

#[test]
fn new_body_loft_with_only_sheet_final_outputs_is_a_surface_loft() {
    let bodies = [
        body("test:model:body#sheet", BodyKind::Sheet),
        body("test:model:body#solid", BodyKind::Solid),
    ];
    let sheet = "test:model:body#sheet";
    let solid = "test:model:body#solid";
    let mut features = [
        loft("synthetic:test:id#sheet-loft", BooleanOp::NewBody, &[sheet]),
        loft("synthetic:test:id#solid-loft", BooleanOp::NewBody, &[solid]),
        loft(
            "synthetic:test:id#mixed-loft",
            BooleanOp::NewBody,
            &[sheet, solid],
        ),
        loft("synthetic:test:id#empty-loft", BooleanOp::NewBody, &[]),
        loft("synthetic:test:id#join-loft", BooleanOp::Join, &[sheet]),
    ];

    let losses = bind(&mut features, &HashMap::new(), &bodies);

    let solids = features
        .each_ref()
        .map(|feature| match feature.evaluation.definition() {
            FeatureDefinition::Operation(FeatureOperation::Loft { solid, .. }) => *solid,
            _ => unreachable!(),
        });
    assert_eq!(solids, [false, true, true, true, true]);
    assert_eq!(losses.len(), 5);
    assert!(losses
        .iter()
        .all(|loss| loss.code == crate::loss::F3dLossCode::FeatureLoftSolidInferred.kind()));
    assert_eq!(
        losses[0].message,
        "Loft feature synthetic:test:id#sheet-loft projects solid false because its 1 output bodies are sheet bodies in the final model (DR-331)."
    );
    assert_eq!(
        losses[2].message,
        "Loft feature synthetic:test:id#mixed-loft projects solid true because 1 of its 2 output bodies are not sheet bodies in the final model (DR-331)."
    );
    assert_eq!(
        losses[3].message,
        "Loft feature synthetic:test:id#empty-loft projects solid true because it has no output body (DR-331)."
    );
    assert_eq!(
        losses[4].message,
        "Loft feature synthetic:test:id#join-loft projects solid true because it combines with an existing body (DR-331)."
    );
}

#[test]
fn result_state_kinds_take_precedence_over_the_final_model() {
    let bodies = [
        body("test:model:body#sheet", BodyKind::Sheet),
        body("test:model:body#solid", BodyKind::Solid),
    ];
    let mut features = [
        loft(
            "synthetic:test:id#thickened-loft",
            BooleanOp::NewBody,
            &["test:model:body#solid"],
        ),
        loft(
            "synthetic:test:id#unstitched-loft",
            BooleanOp::NewBody,
            &["test:model:body#sheet"],
        ),
    ];
    let result_kinds = HashMap::from([
        (features[0].id.clone(), vec![BodyKind::Sheet]),
        (features[1].id.clone(), vec![BodyKind::Solid]),
    ]);

    let losses = bind(&mut features, &result_kinds, &bodies);

    let solids = features
        .each_ref()
        .map(|feature| match feature.evaluation.definition() {
            FeatureDefinition::Operation(FeatureOperation::Loft { solid, .. }) => *solid,
            _ => unreachable!(),
        });
    assert_eq!(solids, [false, true]);
    assert_eq!(
        losses[0].message,
        "Loft feature synthetic:test:id#thickened-loft projects solid false because its 1 output bodies are sheet bodies at its own history state (DR-331)."
    );
    assert_eq!(
        losses[1].message,
        "Loft feature synthetic:test:id#unstitched-loft projects solid true because 1 of its 1 output bodies are not sheet bodies at its own history state (DR-331)."
    );
}
