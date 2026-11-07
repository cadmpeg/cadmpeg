// SPDX-License-Identifier: Apache-2.0
//! A history body is invisible to measurement.

use crate::select::Selection;
use crate::test_support::builders::measure;

#[test]
fn a_document_with_a_history_body_measures_as_one_without() {
    let plain = cadmpeg_ir::examples::unit_cube().expect("fixture");
    let nested = cadmpeg_ir::examples::unit_cube_with_history_body().expect("fixture");
    assert_eq!(nested.model.history_bodies.len(), 1);
    let selection = Selection {
        all_faces: true,
        ..Selection::default()
    };
    let without = measure(&plain, &selection);
    let with = measure(&nested, &selection);
    assert_eq!(
        serde_json::to_value(&without).expect("serialize"),
        serde_json::to_value(&with).expect("serialize")
    );
    assert_eq!(with.faces().len(), without.faces().len());
    let bodies = measure(&nested, &Selection::default());
    assert_eq!(bodies.bodies().len(), 1, "the nest adds no body row");
}
