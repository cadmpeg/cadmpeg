// SPDX-License-Identifier: Apache-2.0
use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy};

#[test]
fn cut_global_card_keeps_readable_fields_and_gives_up_the_unfinished_field() {
    let original = crate::test_support::compressed_parts::points(false, false);
    let date = b"15H20260714.000000";
    let start = original
        .windows(date.len())
        .position(|window| window == date)
        .unwrap();
    let bytes = &original[..start + 7];
    let arena = DecodeArena::new();
    let (ctx, source) = DecodeContext::from_root_bytes(bytes, &arena, &DecodePolicy::service())
        .expect("source session");
    let normalized = super::normalize(source, &ctx).expect("cut compressed source decodes");
    assert_eq!(
        normalized.admission.global.sender_product(),
        Some("product")
    );
    assert_eq!(
        normalized.admission.global.native_file_name(),
        Some("part.igs")
    );
    let (_, _, fidelity, losses) = normalized.parts.into_parts();
    let parts = crate::test_support::given_up_parts(&fidelity);
    assert_eq!(parts.len(), 1);
    assert_eq!(parts[0].data(), Some(&bytes[start..]));
    assert_eq!(losses.len(), 1);
    assert_eq!(losses[0].code.local_code(), "record.unreadable");
    assert!(losses[0].message.contains("iges.global_field"));
}

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
