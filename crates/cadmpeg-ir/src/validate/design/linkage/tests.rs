// SPDX-License-Identifier: Apache-2.0

use cadmpeg_core::decode::ContentScope;

use crate::design::feature::OperationFamily;
use crate::validate::design::fixture::{assert_finding, document, unresolved};

#[test]
fn the_baseline_document_breaks_no_rule() {
    let ir = document();
    let report = crate::validate::validate_neutral(&ir, Vec::new()).unwrap();
    assert!(report.findings.is_empty(), "{:#?}", report.findings);
}

#[test]
fn design_records_outside_the_scope_break_the_scope_invariants() {
    let mut ir = document();
    let design = std::mem::take(&mut ir.design);
    ir.set_scope(ContentScope::geometry()).unwrap();
    ir.design = design;
    assert_finding(&ir, "breaks the scope invariants");
}

#[test]
fn design_identities_are_unique() {
    let mut ir = document();
    ir.design
        .features
        .push(unresolved("fillet", OperationFamily::Fillet));
    assert_finding(&ir, "is not unique among the design records");
}
