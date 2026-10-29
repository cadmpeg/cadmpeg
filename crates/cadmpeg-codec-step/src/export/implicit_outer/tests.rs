// SPDX-License-Identifier: Apache-2.0
//! The STEP bounds of a face whose outer boundary is the natural boundary of
//! its curve-bounded surface's support.

use cadmpeg_ir::topology::Sense;

use crate::export::write_step;
use crate::loss::StepLossCode;
use crate::test_support::curve_bounded::{
    implicit_outer_sheet, unbounded_plane, unit_square_patch,
};
use crate::{StepSchema, StepWriteOptions};

fn written(ir: &cadmpeg_ir::CadIr) -> (String, Vec<cadmpeg_ir::report::loss::LossNote>) {
    let mut output = Vec::new();
    let report = write_step(
        ir,
        &mut output,
        StepSchema::Ap214,
        &StepWriteOptions::default(),
    )
    .expect("report mode writes the document");
    (String::from_utf8(output).expect("utf8"), report.losses)
}

fn records(text: &str, entity: &str) -> usize {
    text.matches(&format!("= {entity}(")).count()
}

/// A plane has no natural boundary, so the face's domain is not finite and
/// STEP has no face for it: the writer omits the face and states why.
#[test]
fn implicit_outer_on_an_unbounded_plane_omits_the_face_with_a_loss() {
    let (text, losses) = written(&implicit_outer_sheet(unbounded_plane(), Sense::Forward));
    assert!(losses.iter().any(|loss| {
        loss.code == StepLossCode::FaceImplicitOuterBoundUnwritable.kind()
            && loss.severity == cadmpeg_ir::report::Severity::Error
            && loss.message.contains("the support is unbounded")
    }));
    assert_eq!(records(&text, "ADVANCED_FACE"), 0);
    assert_eq!(records(&text, "FACE_BOUND"), 0);
}

/// A NURBS support states its parameter rectangle, which the writer emits as
/// the face's outer bound beside the hole's bound.
#[test]
fn implicit_outer_on_a_nurbs_patch_writes_the_parameter_rectangle() {
    for sense in [Sense::Forward, Sense::Reversed] {
        let (text, losses) = written(&implicit_outer_sheet(unit_square_patch(), sense));
        assert!(!losses
            .iter()
            .any(|loss| loss.code == StepLossCode::FaceImplicitOuterBoundUnwritable.kind()));
        assert_eq!(records(&text, "ADVANCED_FACE"), 1);
        assert_eq!(records(&text, "FACE_OUTER_BOUND"), 1);
        assert_eq!(records(&text, "FACE_BOUND"), 1);
        assert_eq!(records(&text, "EDGE_CURVE"), 8);
        assert_eq!(records(&text, "VERTEX_POINT"), 8);
    }
}
