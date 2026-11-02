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
    let reparsed = crate::directory::parse(&scan, normalized.admission.global.global_table(), &ctx)
        .expect("rendered Directory reparses to the admitted entries");
    assert_eq!(reparsed.len(), normalized.admission.directory.len());
    for (reparsed, admitted) in reparsed.iter().zip(&normalized.admission.directory) {
        assert_eq!(reparsed.sequence, admitted.sequence);
        assert_eq!(reparsed.entity_type, admitted.entity_type);
        assert_eq!(reparsed.form, admitted.form);
    }
    assert_eq!(normalized.admission.parameters.records.len(), 2);
}
