// SPDX-License-Identifier: Apache-2.0
//! Sketch-constraint and native-operand design-loss tests.
#![allow(clippy::unwrap_used)]

use super::super::*;
use cadmpeg_ir::features::{BodySelection, BooleanOp, Feature, FeatureDefinition, FeatureId};
use cadmpeg_ir::report::DecodeReport;
use cadmpeg_ir::sketches::{
    SketchConstraintDefinition, SketchConstraintId, SpatialSketchConstraint,
    SpatialSketchConstraintDefinition, SpatialSketchEntityId, SpatialSketchId,
};
use cadmpeg_ir::units::Units;
use cadmpeg_ir::CadIr;
use std::collections::BTreeMap;

#[test]
fn sketch_constraint_completeness_distinguishes_neutral_and_native_semantics() {
    assert!(sketch_constraint_has_complete_neutral_semantics(
        &SketchConstraintDefinition::Disabled
    ));
    assert!(!sketch_constraint_has_complete_neutral_semantics(
        &SketchConstraintDefinition::Native {
            native_kind: "unresolved".into(),
            native_state: None,
            native_flags: None,
            native_properties: BTreeMap::new(),
            entities: Vec::new(),
            parameter: None,
            operands: Vec::new(),
        }
    ));
    assert!(spatial_sketch_constraint_has_complete_neutral_semantics(
        &SpatialSketchConstraintDefinition::Coincident {
            first: SpatialSketchEntityId("first".into()),
            second: SpatialSketchEntityId("second".into()),
        }
    ));
    assert!(!spatial_sketch_constraint_has_complete_neutral_semantics(
        &SpatialSketchConstraintDefinition::Native {
            native_kind: "unresolved".into(),
            native_state: None,
            parameter: None,
            operands: Vec::new(),
        }
    ));
}

#[test]
fn native_spatial_sketch_constraints_are_reported_as_design_losses() {
    let mut ir = CadIr::empty(Units::default());
    ir.model
        .spatial_sketch_constraints
        .push(SpatialSketchConstraint {
            id: SketchConstraintId("native-spatial".into()),
            sketch: SpatialSketchId("spatial-sketch".into()),
            definition: SpatialSketchConstraintDefinition::Native {
                native_kind: "unresolved".into(),
                native_state: None,
                parameter: None,
                operands: Vec::new(),
            },
            native_ref: None,
        });
    let mut report = DecodeReport {
        format: "sldprt".into(),
        container_only: false,
        geometry_transferred: true,
        coverage: BTreeMap::new(),
        transfer_ledger: cadmpeg_ir::report::TransferLedger::default(),
        losses: Vec::new(),
        notes: Vec::new(),
    };

    append_design_losses(&ir, &mut report);

    assert!(report.losses.iter().any(|loss| {
        loss.message
            == "1 planar or spatial sketch constraint(s) retain native relation kinds and operands without complete neutral geometric semantics."
    }));
}

#[test]
fn typed_native_operands_are_reported_as_design_losses() {
    let mut ir = CadIr::empty(Units::default());
    ir.model.features.push(Feature {
        id: FeatureId("combine".into()),
        ordinal: 0,
        name: None,
        suppressed: Some(false),
        parent: None,
        dependencies: Vec::new(),
        source_properties: BTreeMap::new(),
        source_tag: None,
        source_text: None,
        source_content: Vec::new(),
        outputs: Vec::new(),
        definition: FeatureDefinition::Combine {
            target: BodySelection::Native("target".into()),
            tools: BodySelection::Native("tools".into()),
            op: BooleanOp::Unresolved,
            keep_tools: false,
        },
        native_ref: None,
    });
    let mut report = DecodeReport {
        format: "sldprt".into(),
        container_only: false,
        geometry_transferred: true,
        coverage: std::collections::BTreeMap::new(),
        transfer_ledger: cadmpeg_ir::report::TransferLedger::default(),
        losses: Vec::new(),
        notes: Vec::new(),
    };

    append_design_losses(&ir, &mut report);

    assert!(report.losses.iter().any(|loss| {
        loss.message
            == "1 typed feature(s) retain native or unresolved required operation operands."
    }));
}
