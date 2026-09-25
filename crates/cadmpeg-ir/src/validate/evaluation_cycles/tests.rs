// SPDX-License-Identifier: Apache-2.0

use cadmpeg_core::CodecError;

use super::admit_evaluation_cycles;
use crate::report::check::Check;
use crate::test_support::evaluation_cycles::cyclic_model;
use crate::validate::{admit, validate_neutral};

#[test]
fn model_admission_refuses_a_malformed_curve_surface_reference_cycle() {
    let (ir, curve, surface) = cyclic_model();
    let expected =
        format!("malformed curve/surface reference cycle: {curve} -> {surface} -> {curve}");
    let report = validate_neutral(&ir, Vec::new());
    assert!(report.findings.iter().any(|finding| {
        finding.check == Check::ReferentialIntegrity && finding.message == expected
    }));
    assert!(!admit::admit(&ir, admit::DRAFT_CORE_CHECKS, Vec::new()).is_ok());
    assert!(matches!(
        admit_evaluation_cycles(&ir),
        Err(CodecError::Malformed(message)) if message == expected
    ));
}
