// SPDX-License-Identifier: Apache-2.0

use super::walk;
use crate::design::dimension::ParamRef;
use crate::validate::process::fixture::document;

/// The walk finds every `Choice`, `ParamRef`, workpiece index, and anchor
/// the record holds.
#[test]
fn the_walk_finds_the_values_a_record_holds() {
    let ir = document();
    let mill = &ir.process.operations[2];
    let found = walk(mill);
    let at: Vec<&str> = found
        .choices
        .iter()
        .map(|choice| choice.at.as_str())
        .collect();
    assert_eq!(at, ["/heights/bottom/from"]);
    assert!(matches!(
        found.references.as_slice(),
        [(at, ParamRef::Process(id))] if at == "/heights/bottom/offset/value" && id.as_str() == "test:process:parameter#depth"
    ));
    assert_eq!(
        found.workpieces,
        [("/geometry/0/source/value/0".to_owned(), Some(0))]
    );
    assert_eq!(found.anchors.len(), 1);
}
