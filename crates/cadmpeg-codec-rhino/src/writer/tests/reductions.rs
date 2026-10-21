// SPDX-License-Identifier: Apache-2.0
//! Sheet reductions preserve admitted faces without repairing malformed input.

use super::adjacent_quad_sheet;
use crate::writer::{brep_payload, model::WritableModel, reduce};
use crate::{loss::RhinoLossCode, RhinoArchiveVersion};
use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy};
use cadmpeg_core::CodecError;
use cadmpeg_ir::document::CadIr;
use cadmpeg_ir::report::Severity;
use cadmpeg_ir::scalar::PositiveReal;
use std::cell::Cell;

const UNREPRESENTABLE_NATIVE_TOLERANCE: f64 = 1.0e-12;

fn admit(ir: &CadIr) -> Result<(), CodecError> {
    let arena = DecodeArena::new();
    let (context, _) = DecodeContext::from_root_bytes(&[], &arena, &DecodePolicy::default())?;
    let model = WritableModel::try_new(ir)?;
    brep_payload(&context, &model, RhinoArchiveVersion::V8)?;
    Ok(())
}

#[test]
fn sheet_reduction_omits_only_the_unsupported_face() {
    let mut ir = adjacent_quad_sheet();
    let body = ir.model.bodies[0].clone();
    let retained = ir.model.faces[0].id.clone();
    let omitted = ir.model.faces[1].id.clone();
    ir.model.faces[1].tolerance = PositiveReal::new(UNREPRESENTABLE_NATIVE_TOLERANCE);
    assert!(matches!(admit(&ir), Err(CodecError::UnsupportedContent(_))));

    let (written, losses) = reduce::sheet(&ir, admit).expect("entity degradation");
    let written = written.expect("the writable face remains");
    assert_eq!(written.model.bodies, vec![body]);
    assert_eq!(written.model.faces.len(), 1);
    assert_eq!(written.model.faces[0].id, retained);
    assert_eq!(written.model.shells[0].faces(), &[retained]);
    assert_eq!(written.model.edges.len(), 4);
    assert_eq!(written.model.vertices.len(), 4);
    assert_eq!(written.model.points.len(), 4);
    assert_eq!(written.model.curves.len(), 4);
    assert_eq!(written.model.surfaces.len(), 1);
    assert!(written
        .model
        .coedges
        .iter()
        .all(|coedge| coedge.radial_next == coedge.id));
    admit(&written).expect("reduced sheet really encodes");
    assert_eq!(losses.len(), 1);
    assert_eq!(losses[0].code, RhinoLossCode::WriterEntityOmitted.kind());
    assert_eq!(losses[0].severity, Severity::Error);
    assert!(losses[0].message.contains(omitted.as_str()));
}

#[test]
fn unsupported_first_face_does_not_discard_a_later_writable_face() {
    let mut ir = adjacent_quad_sheet();
    let retained = ir.model.faces[1].id.clone();
    ir.model.faces[0].tolerance = PositiveReal::new(UNREPRESENTABLE_NATIVE_TOLERANCE);
    let (written, losses) = reduce::sheet(&ir, admit).expect("reduce first face");
    let written = written.expect("later face remains");
    assert_eq!(written.model.faces[0].id, retained);
    assert_eq!(losses.len(), 1);
    admit(&written).expect("later face encodes");
}

#[test]
fn malformed_original_radial_cycle_is_not_repaired_by_omission() {
    let mut ir = adjacent_quad_sheet();
    ir.model.faces[1].tolerance = PositiveReal::new(UNREPRESENTABLE_NATIVE_TOLERANCE);
    let shared = ir
        .model
        .edges
        .iter()
        .find_map(|edge| {
            let uses = ir
                .model
                .coedges
                .iter()
                .enumerate()
                .filter(|(_, row)| row.edge == edge.id)
                .map(|(index, _)| index)
                .collect::<Vec<_>>();
            (uses.len() == 2).then_some(uses)
        })
        .expect("shared edge");
    // Split a valid two-use ring into a premature self-cycle. Pruning the
    // unsupported neighbor would make that same graph appear valid.
    ir.model.coedges[shared[0]].radial_next = ir.model.coedges[shared[0]].id.clone();
    let calls = Cell::new(0);
    let error = reduce::sheet(&ir, |candidate| {
        calls.set(calls.get() + 1);
        admit(candidate)
    })
    .expect_err("malformed original graph");
    assert!(matches!(error, CodecError::Malformed(_)));
    assert!(error.to_string().contains("radial ring"));
    assert_eq!(
        calls.get(),
        0,
        "validate the original graph before any candidate"
    );
}

#[test]
fn dangling_original_radial_reference_is_a_precondition() {
    let mut ir = adjacent_quad_sheet();
    ir.model.coedges[0].radial_next = "cadir:model:coedge#missing".try_into().expect("identity");
    let error = reduce::sheet(&ir, |_| Ok(())).expect_err("dangling radial use");
    assert!(matches!(error, CodecError::Malformed(_)));
}

#[test]
fn admission_preconditions_propagate_instead_of_becoming_entity_losses() {
    let ir = adjacent_quad_sheet();
    let error = reduce::sheet(&ir, |_| {
        Err(CodecError::InvalidInput("capacity sentinel".into()))
    })
    .expect_err("precondition");
    assert!(matches!(error, CodecError::InvalidInput(message) if message == "capacity sentinel"));
}

#[test]
fn no_admitted_faces_produces_no_body_and_one_loss_per_face() {
    let ir = adjacent_quad_sheet();
    let (written, losses) = reduce::sheet(&ir, |_| {
        Err(CodecError::UnsupportedContent("unsupported carrier".into()))
    })
    .expect("all faces degrade");
    assert!(written.is_none());
    assert_eq!(losses.len(), ir.model.faces.len());
    assert!(losses.iter().all(|loss| loss.severity == Severity::Error));
}
