// SPDX-License-Identifier: Apache-2.0
//! Sketch-constraint and native-operand design-loss tests.
#![allow(clippy::unwrap_used)]

use super::super::*;
use cadmpeg_ir::features::{BodySelection, Feature, FeatureDefinition, FeatureId};
use cadmpeg_ir::sketches::{
    SketchConstraintDefinitionInput, SketchConstraintId, SpatialSketchConstraint,
    SpatialSketchConstraintDefinitionInput, SpatialSketchEntityId, SpatialSketchId,
};
use cadmpeg_ir::CadIr;
use std::collections::BTreeMap;

#[test]
fn sketch_constraint_completeness_distinguishes_neutral_and_native_semantics() {
    assert!(sketch_constraint_has_complete_neutral_semantics(
        &SketchConstraintDefinitionInput::Disabled
    ));
    assert!(!sketch_constraint_has_complete_neutral_semantics(
        &SketchConstraintDefinitionInput::Native {
            native_kind: cadmpeg_ir::products::NonEmptyString::new("unresolved").unwrap(),
            native_state: None,
            native_flags: None,
            native_properties: BTreeMap::new(),
            entities: Vec::new(),
            parameter: None,
            operands: Vec::new(),
        }
    ));
    assert!(spatial_sketch_constraint_has_complete_neutral_semantics(
        &SpatialSketchConstraintDefinitionInput::Coincident {
            first: SpatialSketchEntityId::mint("synthetic:test:id#first").unwrap(),
            second: SpatialSketchEntityId::mint("synthetic:test:id#second").unwrap(),
        }
    ));
    assert!(!spatial_sketch_constraint_has_complete_neutral_semantics(
        &SpatialSketchConstraintDefinitionInput::Native {
            native_kind: cadmpeg_ir::products::NonEmptyString::new("unresolved").unwrap(),
            native_state: None,
            parameter: None,
            operands: Vec::new(),
        }
    ));
}

#[test]
fn native_spatial_sketch_constraints_are_reported_as_design_losses() {
    let mut ir = CadIr::empty();
    ir.model
        .spatial_sketch_constraints
        .push(SpatialSketchConstraint {
            id: SketchConstraintId::mint("synthetic:test:id#native-spatial").unwrap(),
            sketch: SpatialSketchId::mint("synthetic:test:id#spatial-sketch").unwrap(),
            definition: cadmpeg_ir::sketches::SpatialSketchConstraintDefinition::try_from(
                SpatialSketchConstraintDefinitionInput::Native {
                    native_kind: cadmpeg_ir::products::NonEmptyString::new("unresolved").unwrap(),
                    native_state: None,
                    parameter: None,
                    operands: vec![cadmpeg_ir::sketches::SketchNativeOperand {
                        native_kind: cadmpeg_ir::products::NonEmptyString::new("entity").unwrap(),
                        field: None,
                        object_index: 1,
                        native_ref: None,
                    }],
                },
            )
            .unwrap(),
            native_ref: None,
        });
    let mut report = super::empty_report(true);

    append_design_losses(&ir, &mut report);

    assert!(report.losses.iter().any(|loss| {
        loss.message
            == "1 planar or spatial sketch constraint(s) retain native relation kinds and operands without complete neutral geometric semantics."
    }));
}

#[test]
fn typed_native_operands_are_reported_as_design_losses() {
    let mut ir = CadIr::empty();
    ir.model.features.push(Feature {
        id: FeatureId::mint("synthetic:test:id#combine").expect("identity grammar"),
        ordinal: 0,
        name: None,
        suppressed: Some(false),
        dependencies: cadmpeg_ir::features::DistinctMembers::default(),
        source_properties: BTreeMap::new(),
        source_tag: None,
        source_text: None,
        source_content: cadmpeg_ir::features::FeatureContent::default(),

        evaluation: cadmpeg_ir::features::FeatureEvaluation::from_definition(
            FeatureDefinition::Combine {
                operands: cadmpeg_ir::features::CombineOperands::new(
                    BodySelection::Native("target".into()),
                    BodySelection::Native("tools".into()),
                )
                .unwrap(),

                op: cadmpeg_ir::features::BooleanKind::Join,
                keep_tools: false,
            },
        ),
        native_ref: None,
    });
    let mut report = super::empty_report(true);

    append_design_losses(&ir, &mut report);

    assert!(report.losses.iter().any(|loss| {
        loss.message
            == "1 typed feature(s) retain native or unresolved required operation operands."
    }));
}
