// SPDX-License-Identifier: Apache-2.0
use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy};

#[test]
fn actual_data_callback_records_match_the_existing_parameter_grammar_without_reparsing_on_decode() {
    let original = crate::test_support::compressed_parts::points(false, false);
    let mut bytes = b"prefix:".to_vec();
    bytes.extend_from_slice(&original);
    bytes.extend_from_slice(b":suffix");
    let arena = DecodeArena::new();
    let (ctx, root) = DecodeContext::from_root_bytes(&bytes, &arena, &DecodePolicy::service())
        .expect("actual source session");
    let source = root
        .child(7, 7 + original.len())
        .expect("original source window");
    let normalized = super::normalize(source, &ctx).expect("actual Compressed callbacks");
    let scan = crate::card::scan(&normalized.bytes).expect("existing normalized grammar");
    let expected = crate::parameter::assemble(
        &scan,
        &normalized.admission.directory,
        [],
        &normalized.admission.global,
    )
    .expect("existing Parameter parser oracle");
    assert_eq!(normalized.admission.parameters.records, expected.records);
    assert_eq!(
        normalized.admission.parameters.trailing_pointer_analysis,
        expected.trailing_pointer_analysis
    );
    assert_eq!(normalized.admission.parameters.records.len(), 2);
    assert!(normalized.admission.parameters.quarantined.is_empty());
}
