// SPDX-License-Identifier: Apache-2.0
//! The exactness of a model feature, parameter or configuration that a design
//! record restates names the fields of the design record (§10).

use cadmpeg_ir::Exactness;
use cadmpeg_test_support::EditableDecodeResult;

use crate::test_support::test_om::segment_om_record_area_with_input_store_payload;
use crate::test_support::test_prt::{
    prt_with_arrangements, prt_with_indexed_om_section, prt_with_named_payloads,
};

fn fields(result: &EditableDecodeResult, id: &str) -> (Exactness, Vec<(String, Exactness)>) {
    let note = &result.source_fidelity().annotations.exactness()[id];
    let fields = note
        .fields()
        .iter()
        .map(|(field, exactness)| (field.to_string(), *exactness))
        .collect();
    (note.entity(), fields)
}

/// The expression's derived `ordinal` feeds no design field and leaves, and
/// its `owner` counts only toward `definition`. The declared unit in
/// `properties` feeds `kind`, `unit`, `definition` and `value`.
#[test]
fn a_restated_expression_names_the_fields_of_its_design_parameter() {
    let result = super::decode(prt_with_named_payloads(&[(
        "/Root/UG_PART/UG_PART",
        segment_om_record_area_with_input_store_payload(),
    )]));
    let derived = ["definition", "kind", "native_ref", "unit", "value"]
        .map(|field| (field.to_owned(), Exactness::Derived));
    assert_eq!(
        fields(&result, "nx:om-entry-0:parameter#399"),
        (Exactness::ByteExact, derived.to_vec())
    );
}

/// A part with inline geometry keeps the marks of the expressions and
/// arrangements its design records restate: the arrangement's `name` and
/// `native_ref` are derived from the arrangement table, and the expression's
/// fields as on a part with no geometry.
#[test]
fn a_part_with_geometry_restates_the_marks_of_its_design_records() {
    let arrangements = super::decode(prt_with_arrangements());
    let derived = ["name", "native_ref"].map(|field| (field.to_owned(), Exactness::Derived));
    for arrangement in [
        "nx:arrangements:arrangement#0",
        "nx:arrangements:arrangement#1",
    ] {
        assert_eq!(
            fields(&arrangements, arrangement),
            (Exactness::ByteExact, derived.to_vec()),
            "{arrangement}"
        );
    }
    let expressions = super::decode(prt_with_indexed_om_section());
    let derived = ["definition", "kind", "native_ref", "unit", "value"]
        .map(|field| (field.to_owned(), Exactness::Derived));
    assert_eq!(
        fields(&expressions, "nx:om-entry-0:parameter#173"),
        (Exactness::ByteExact, derived.to_vec())
    );
}
