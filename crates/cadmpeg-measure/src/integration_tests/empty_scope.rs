// SPDX-License-Identifier: Apache-2.0
//! A document whose scope holds no model class: no measured body and
//! `measure:output.empty`.

use cadmpeg_core::decode::ContentScope;
use cadmpeg_ir::report::strictness::{code_name, Strictness};
use cadmpeg_ir::report::Severity;
use cadmpeg_ir::CadIr;

use crate::loss::MeasureLossCode;
use crate::select::Selection;
use crate::session::{MeasureError, MeasureInput, MeasureOptions, Measurer};
use crate::test_support::builders::{measure, plate_with_hole, service_context};

fn scope_without_model() -> CadIr {
    let mut ir = CadIr::empty();
    ir.set_scope(ContentScope::container());
    ir
}

#[test]
fn a_scope_without_the_model_class_charges_output_empty() {
    let ir = scope_without_model();
    let report = measure(&ir, &Selection::default());
    assert!(report.bodies().is_empty());
    let [loss] = report.losses().as_slice() else {
        panic!("one loss: {:?}", report.losses());
    };
    assert_eq!(code_name(&loss.code), "measure:output.empty");
    assert_eq!(loss.code, MeasureLossCode::OutputEmpty.note("").code);
    assert_eq!(loss.severity, Severity::Warning);
    assert_eq!(loss.code.strict_floor(), None);
}

#[test]
fn a_model_document_charges_no_loss() {
    let report = measure(&plate_with_hole(false), &Selection::default());
    assert!(report.losses().is_empty());
}

#[test]
fn a_caller_strictness_on_output_empty_refuses_the_measurement() {
    let ir = scope_without_model();
    let ctx = service_context();
    let input = MeasureInput {
        ir: &ir,
        fidelity: None,
        geometry_transferred: true,
    };
    let options = MeasureOptions {
        strictness: Strictness::default().with_code("measure:output.empty"),
        ..MeasureOptions::default()
    };
    let Err(MeasureError::StrictRejected { report, entry }) =
        Measurer::new(input, options).measure(&ctx, &Selection::default())
    else {
        panic!("the strictness refuses measure:output.empty");
    };
    assert_eq!(
        code_name(&report.losses()[entry].code),
        "measure:output.empty"
    );
}
