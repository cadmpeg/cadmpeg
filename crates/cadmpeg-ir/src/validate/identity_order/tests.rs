// SPDX-License-Identifier: Apache-2.0
#![allow(clippy::unwrap_used)]

use crate::examples::unit_cube;
use crate::report::Check;
use crate::validate::validate_neutral;

#[test]
fn ids_are_globally_unique_across_arenas() {
    let mut ir = unit_cube();
    ir.model.points[0].id.0 = ir.model.vertices[0].id.0.clone();
    assert!(validate_neutral(&ir, Vec::new())
        .findings
        .iter()
        .any(|finding| finding.check == Check::Identity));
}

#[test]
fn arena_ids_must_be_sorted() {
    let mut ir = unit_cube();
    ir.model.points.swap(0, 1);
    assert!(validate_neutral(&ir, Vec::new())
        .findings
        .iter()
        .any(|finding| finding.check == Check::ArenaOrder));
}
