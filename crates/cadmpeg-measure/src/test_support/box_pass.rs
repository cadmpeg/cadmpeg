// SPDX-License-Identifier: Apache-2.0
//! The box pass against the full pass, body by body: the two passes may
//! differ only where [`crate::body_boxes`] says they do.

use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy};
use cadmpeg_ir::CadIr;

use crate::report::{FaceMeasures, MeasureReport};
use crate::select::Selection;
use crate::value::{BoxValue, Measured, Quantity, Reading};
use crate::{body_boxes, MeasureError, MeasureInput, MeasureOptions, Measurer, WorkProfile};

/// How the box pass's box of one body relates to the full pass's.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Agreement {
    /// Both passes give the same box.
    Equal,
    /// Neither pass gives a box.
    Unboxed,
    /// The box pass's box rests on a declared orientation, and the full
    /// pass gives another box or none.
    Declared,
    /// Only the box pass gives a box, and every face the full pass leaves
    /// unboxed has a NURBS leaf carrier, which the box pass bounds by its
    /// control hull.
    ControlHull,
    /// Any other pair.
    Disagree,
}

/// The agreement of one body.
#[derive(Debug, Clone)]
pub struct BodyAgreement {
    /// The body ID.
    pub body: String,
    /// How its two boxes relate.
    pub agreement: Agreement,
}

/// The agreement of each body of `ir`, in body arena order. Both passes run
/// under [`DecodePolicy::service`] and [`WorkProfile::Service`], as
/// [`body_boxes`] does; a session refusal of either is the error.
pub fn agreements(ir: &CadIr) -> Result<Vec<BodyAgreement>, MeasureError> {
    let boxes = body_boxes(ir)?;
    let full = full_pass(ir, &Selection::default())?;
    let mut faces: Option<MeasureReport> = None;
    let mut agreements = Vec::with_capacity(boxes.len());
    for body_box in &boxes {
        let row = full.bodies.iter().find(|row| row.body == body_box.body);
        let agreement = match (row.map(|row| &row.bbox), &body_box.bbox) {
            (Some(Quantity::Measured(full)), Quantity::Measured(pass)) if same(full, pass) => {
                Agreement::Equal
            }
            (Some(Quantity::Unavailable(_)), Quantity::Unavailable(_)) => Agreement::Unboxed,
            (Some(_), Quantity::Measured(pass)) if pass.reading() == Some(Reading::Declared) => {
                Agreement::Declared
            }
            (Some(Quantity::Unavailable(_)), Quantity::Measured(_)) => {
                if faces.is_none() {
                    let all_faces = Selection {
                        all_faces: true,
                        ..Selection::default()
                    };
                    faces = Some(full_pass(ir, &all_faces)?);
                }
                faces.as_ref().map_or(Agreement::Disagree, |report| {
                    control_hull(report, &body_box.body)
                })
            }
            _ => Agreement::Disagree,
        };
        agreements.push(BodyAgreement {
            body: body_box.body.clone(),
            agreement,
        });
    }
    Ok(agreements)
}

/// [`Agreement::ControlHull`] when the faces of `body` that `report` leaves
/// unboxed all have a NURBS leaf carrier, plain or as a procedural cache,
/// and there is at least one.
fn control_hull(report: &MeasureReport, body: &str) -> Agreement {
    let mut unboxed = report
        .faces
        .iter()
        .filter(|face| face.body.as_deref() == Some(body))
        .filter(|face| face.measures.bbox.measured().is_none())
        .peekable();
    let nurbs = |face: &FaceMeasures| {
        face.carrier
            .chain
            .last()
            .is_some_and(|leaf| leaf.ends_with("nurbs"))
    };
    if unboxed.peek().is_some() && unboxed.all(nurbs) {
        Agreement::ControlHull
    } else {
        Agreement::Disagree
    }
}

/// A full pass under the settings of [`body_boxes`].
fn full_pass(ir: &CadIr, selection: &Selection) -> Result<MeasureReport, MeasureError> {
    let arena = DecodeArena::new();
    let ctx = DecodeContext::for_loaded_input(&arena, &DecodePolicy::service(), 0)?;
    let input = MeasureInput {
        ir,
        fidelity: None,
        geometry_transferred: true,
    };
    let options = MeasureOptions {
        work: WorkProfile::Service,
        ..MeasureOptions::default()
    };
    let report = Measurer::new(input, options).measure(&ctx, selection)?;
    ctx.finish_session()?;
    Ok(report)
}

/// Whether two measured boxes state the same value, method, errors and
/// readings.
fn same(a: &Measured<BoxValue>, b: &Measured<BoxValue>) -> bool {
    a.value() == b.value()
        && a.unit() == b.unit()
        && a.method() == b.method()
        && a.numeric_error() == b.numeric_error()
        && a.model_uncertainty() == b.model_uncertainty()
        && a.reading() == b.reading()
        && a.candidates().len() == b.candidates().len()
        && a.candidates().iter().zip(b.candidates()).all(|(a, b)| {
            a.reading() == b.reading()
                && a.value() == b.value()
                && a.numeric_error() == b.numeric_error()
                && a.model_uncertainty() == b.model_uncertainty()
        })
}
