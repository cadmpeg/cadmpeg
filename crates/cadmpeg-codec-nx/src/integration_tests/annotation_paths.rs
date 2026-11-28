// SPDX-License-Identifier: Apache-2.0
//! The exactness of a model feature or parameter that a design record
//! restates names the fields of the design record (§10).

use cadmpeg_ir::Exactness;
use cadmpeg_test_support::EditableDecodeResult;

use crate::test_support::test_om::segment_om_record_area_with_input_store_payload;
use crate::test_support::test_prt::prt_with_named_payloads;

fn fields(result: &EditableDecodeResult, id: &str) -> (Exactness, Vec<(String, Exactness)>) {
    let note = &result.source_fidelity().annotations.exactness()[id];
    let fields = note
        .fields()
        .iter()
        .map(|(field, exactness)| (field.to_string(), *exactness))
        .collect();
    (note.entity(), fields)
}

/// The native expression reference supplies the source-table position.
/// The declared unit feeds `kind`, `unit`, `definition` and `value`.
#[test]
fn a_restated_expression_names_the_fields_of_its_design_parameter() {
    let result = super::decode(prt_with_named_payloads(&[(
        "/Root/UG_PART/UG_PART",
        segment_om_record_area_with_input_store_payload(),
    )]));
    let derived = [
        "definition",
        "kind",
        "native_ref",
        "position",
        "unit",
        "value",
    ]
    .map(|field| (field.to_owned(), Exactness::Derived));
    assert_eq!(
        fields(&result, "nx:om-entry-0:parameter#399"),
        (Exactness::ByteExact, derived.to_vec())
    );
}
