// SPDX-License-Identifier: Apache-2.0
use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy};

#[test]
fn compressed_flag_without_a_start_part_is_truncated() {
    use cadmpeg_ir::codec::{Codec, DecodeFailure, DecodeOptions};
    let original = crate::test_support::compressed_parts::points(false, false);
    let flag = &original[..82];
    let result =
        crate::IgesCodec.decode(&mut std::io::Cursor::new(flag), &DecodeOptions::default());
    assert!(matches!(
        result,
        Err(DecodeFailure::Codec(
            cadmpeg_core::CodecError::Truncated { .. }
        ))
    ));
}

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
    assert!(normalized.admission.losses.is_empty());
    let (_, _, fidelity, losses) = normalized.parts.into_parts();
    let parts = crate::test_support::given_up_parts(&fidelity);
    assert_eq!(parts.len(), 1);
    assert_eq!(parts[0].data(), Some(&bytes[start..]));
    assert_eq!(losses.len(), 1);
    assert_eq!(losses[0].code.local_code(), "record.unreadable");
    assert!(losses[0].message.contains("iges.global_field"));
    let decoded = crate::test_support::decode(bytes.to_vec());
    assert_eq!(decoded.ir().tolerances.linear, None);
    assert!(decoded.report().losses.iter().any(|loss| {
        loss.dropped_fields.iter().any(|field| {
            field.record == cadmpeg_ir::report::loss::DOCUMENT_HEADER
                && field.field == "tolerances.linear"
        })
    }));
    assert_eq!(
        crate::test_support::given_up_count(decoded.report(), "iges.global_field"),
        1
    );
    let baseline = crate::test_support::decode(original);
    let cards = &decoded.ir().native.namespace("iges").unwrap().arenas()["cards"];
    let original_cards = &baseline.ir().native.namespace("iges").unwrap().arenas()["cards"];
    for card in cards {
        let original = original_cards
            .iter()
            .find(|original| original.id() == card.id());
        assert_eq!(original, Some(card));
    }
    for loss in &decoded.report().losses {
        assert!(
            loss.code.local_code() == "record.unreadable"
                || baseline
                    .report()
                    .losses
                    .iter()
                    .any(|other| other.code == loss.code),
            "{loss:?}"
        );
    }
}

#[test]
fn cut_first_global_card_gives_up_the_incomplete_field() {
    let original = crate::test_support::compressed_parts::points(false, false);
    let product = b"7Hproduct";
    let start = original
        .windows(product.len())
        .position(|window| window == product)
        .unwrap();
    let bytes = &original[..start + 4];
    let arena = DecodeArena::new();
    let (ctx, source) = DecodeContext::from_root_bytes(bytes, &arena, &DecodePolicy::service())
        .expect("source session");
    let normalized = super::normalize(source, &ctx).expect("cut compressed source decodes");
    assert_eq!(normalized.admission.global.sender_product(), None);
    assert!(normalized.admission.losses.is_empty());
    let (_, _, fidelity, losses) = normalized.parts.into_parts();
    let parts = crate::test_support::given_up_parts(&fidelity);
    assert_eq!(parts.len(), 1);
    assert_eq!(parts[0].data(), Some(&bytes[start..]));
    assert_eq!(losses.len(), 1);
    assert_eq!(losses[0].code.local_code(), "record.unreadable");
    assert!(losses[0].message.contains("iges.global_field"));
    let decoded = crate::test_support::decode(bytes.to_vec());
    assert_eq!(
        crate::test_support::given_up_count(decoded.report(), "iges.global_field"),
        1
    );
    let baseline = crate::test_support::decode(original);
    for loss in &decoded.report().losses {
        assert!(
            loss.code.local_code() == "record.unreadable"
                || baseline
                    .report()
                    .losses
                    .iter()
                    .any(|other| other.code == loss.code),
            "{loss:?}"
        );
    }
}

#[test]
fn cut_before_global_gives_up_its_empty_section() {
    let original = crate::test_support::compressed_parts::points(false, false);
    let global = b"1H,,1H;";
    let start = original
        .windows(global.len())
        .position(|window| window == global)
        .unwrap();
    let decoded = crate::test_support::decode(original[..start].to_vec());
    assert_eq!(
        crate::test_support::given_up_count(decoded.report(), "iges.global_section"),
        1
    );
    let parts = crate::test_support::given_up_parts(decoded.source_fidelity());
    assert_eq!(parts.len(), 1);
    assert_eq!(parts[0].byte_len(), 0);
    assert_eq!(parts[0].offset(), u64::try_from(start).unwrap());
}

#[test]
fn given_up_global_holds_model_scope_without_start_metadata() {
    let mut original = crate::test_support::compressed_parts::points(false, false);
    let global = b"1H,,1H;";
    let start = original
        .windows(global.len())
        .position(|window| window == global)
        .unwrap();
    let start_card = original
        .chunks_exact(82)
        .position(|card| card[72] == b'S')
        .unwrap()
        * 82;
    original[start_card..start_card + 72].fill(b' ');
    let decoded = crate::test_support::decode(original[..start].to_vec());
    assert!(decoded.ir().scope.model);
    assert!(decoded
        .source_fidelity()
        .unrepresented()
        .contains(&cadmpeg_ir::ArenaClass::Model));
    assert_eq!(
        crate::test_support::given_up_count(decoded.report(), "iges.global_section"),
        1
    );
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
