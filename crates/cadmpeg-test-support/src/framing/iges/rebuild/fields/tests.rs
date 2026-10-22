// SPDX-License-Identifier: Apache-2.0
use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, View};

use super::{frames, rebuild_fields, signatures, validate_candidate, FieldEdit};
use crate::framing::FramedPart;

fn card(marker: u8, sequence: usize, payload: &[u8], ending: &[u8]) -> Vec<u8> {
    assert!(payload.len() <= 72);
    let mut bytes = [b' '; 80].to_vec();
    bytes[..payload.len()].copy_from_slice(payload);
    bytes[72] = marker;
    bytes[73..80].copy_from_slice(format!("{sequence:>7}").as_bytes());
    bytes.extend_from_slice(ending);
    bytes
}

fn directory() -> Vec<u8> {
    let mut first = [b' '; 72];
    first[..8].copy_from_slice(b"     212");
    first[8..16].copy_from_slice(b"       1");
    let mut second = [b' '; 72];
    second[..8].copy_from_slice(b"     212");
    second[24..32].copy_from_slice(b"       2");
    let mut bytes = card(b'D', 1, &first, b"\n");
    bytes.extend(card(b'D', 2, &second, b"\n"));
    bytes
}

fn parameter(sequence: usize, payload: &[u8]) -> Vec<u8> {
    assert!(payload.len() <= 64);
    let mut bytes = [b' '; 72];
    bytes[..payload.len()].copy_from_slice(payload);
    bytes[64..72].copy_from_slice(b"       1");
    card(b'P', sequence, &bytes, b"\n")
}

fn sample() -> Vec<u8> {
    let mut global = b",,80H".to_vec();
    global.extend_from_slice(&[b'a'; 80]);
    global.extend_from_slice(b",7;");
    let mut data = b"212,70H".to_vec();
    data.extend_from_slice(&[b'x'; 70]);
    data.extend_from_slice(b",1;");
    let mut bytes = b"pre".to_vec();
    bytes.extend(card(b'G', 1, &global[..72], b"\r\n"));
    bytes.extend(card(b'G', 2, &global[72..], b"\r\n"));
    bytes.extend(directory());
    bytes.extend(parameter(1, &data[..64]));
    bytes.extend(parameter(2, &data[64..]));
    bytes.extend_from_slice(b"post");
    bytes
}

fn field<'a>(parts: &[FramedPart<'a>], level: &str, index: usize) -> View<'a> {
    parts
        .iter()
        .filter(|part| part.level().as_str() == level)
        .nth(index)
        .unwrap()
        .view()
}

#[test]
fn composite_seam_replacements_preserve_every_unrelated_physical_byte() {
    let bytes = sample();
    let arena = DecodeArena::new();
    let (ctx, root) =
        DecodeContext::from_root_bytes(&bytes, &arena, &DecodePolicy::service()).unwrap();
    let source = root.child(3, bytes.len() - 4).unwrap();
    let parts = frames(&ctx, source).unwrap();
    let global = field(&parts, "iges.global_field", 0);
    let parameter = field(&parts, "iges.parameter_field", 1);
    let rebuilt = ctx
        .record("rebuild", source, || {
            rebuild_fields(
                &ctx,
                source,
                &[
                    FieldEdit {
                        source: global.child(70, 74).unwrap(),
                        replacement: b";G,!",
                    },
                    FieldEdit {
                        source: parameter.child(62, 66).unwrap(),
                        replacement: b",;G!",
                    },
                ],
            )
            .map_err(cadmpeg_core::CodecError::malformed)
        })
        .unwrap();
    let mut expected = source.window().to_vec();
    expected[70..72].copy_from_slice(b";G");
    expected[82..84].copy_from_slice(b",!");
    expected[388..390].copy_from_slice(b",;");
    expected[407..409].copy_from_slice(b"G!");
    assert_eq!(rebuilt, expected);
    assert_eq!(&rebuilt[72..82], &source.window()[72..82]);
    assert_eq!(&rebuilt[390..407], &source.window()[390..407]);
    let captures = ctx.take_record_captures().unwrap();
    assert_eq!(captures.len(), 1);
    assert_eq!(captures[0].id(), "rebuild");
    assert!(captures[0].reads().is_empty());
}

#[test]
fn empty_and_single_byte_edits_preserve_original_card_layout() {
    let bytes = sample();
    let arena = DecodeArena::new();
    let (ctx, root) =
        DecodeContext::from_root_bytes(&bytes, &arena, &DecodePolicy::service()).unwrap();
    let source = root.child(3, bytes.len() - 4).unwrap();
    assert_eq!(rebuild_fields(&ctx, source, &[]).unwrap(), source.window());
    let parts = frames(&ctx, source).unwrap();
    let value = field(&parts, "iges.global_field", 0).child(5, 6).unwrap();
    assert_eq!(
        rebuild_fields(
            &ctx,
            source,
            &[FieldEdit {
                source: value,
                replacement: b"a"
            }]
        )
        .unwrap(),
        source.window()
    );
    let edited = rebuild_fields(
        &ctx,
        source,
        &[FieldEdit {
            source: value,
            replacement: b"z",
        }],
    )
    .unwrap();
    let mut expected = source.window().to_vec();
    expected[5] = b'z';
    assert_eq!(edited, expected);
}

#[test]
fn counts_delimiters_dispatch_and_de_parameter_locators_are_protected() {
    let bytes = sample();
    let arena = DecodeArena::new();
    let (ctx, root) =
        DecodeContext::from_root_bytes(&bytes, &arena, &DecodePolicy::service()).unwrap();
    let source = root.child(3, bytes.len() - 4).unwrap();
    let parts = frames(&ctx, source).unwrap();
    let global = field(&parts, "iges.global_field", 0);
    let entity_type = field(&parts, "iges.parameter_field", 0);
    for target in [
        global.child(2, 3).unwrap(),
        global.child(85, 86).unwrap(),
        entity_type.child(0, 1).unwrap(),
        root.child(175, 176).unwrap(),
        root.child(272, 273).unwrap(),
        root.child(393, 394).unwrap(),
        root.child(75, 76).unwrap(),
    ] {
        assert!(rebuild_fields(
            &ctx,
            source,
            &[FieldEdit {
                source: target,
                replacement: b"9"
            }]
        )
        .is_err());
    }
}

#[test]
fn candidate_reframe_rejects_new_field_boundaries_and_new_hollerith_grammar() {
    let bytes = card(b'G', 1, b",,1234;", b"\n");
    let arena = DecodeArena::new();
    let (ctx, root) =
        DecodeContext::from_root_bytes(&bytes, &arena, &DecodePolicy::service()).unwrap();
    let parts = frames(&ctx, root).unwrap();
    let value = field(&parts, "iges.global_field", 0);
    for replacement in [b"1,34".as_slice(), b"1Ha "] {
        assert!(rebuild_fields(
            &ctx,
            root,
            &[FieldEdit {
                source: value.child(2, 6).unwrap(),
                replacement
            }]
        )
        .is_err());
    }
    assert!(rebuild_fields(
        &ctx,
        root,
        &[FieldEdit {
            source: value.child(2, 3).unwrap(),
            replacement: b"\n"
        }]
    )
    .is_err());
}

#[test]
fn unequal_overlapping_and_foreign_field_edits_are_refused() {
    let bytes = sample();
    let arena = DecodeArena::new();
    let (ctx, root) =
        DecodeContext::from_root_bytes(&bytes, &arena, &DecodePolicy::service()).unwrap();
    let source = root.child(3, bytes.len() - 4).unwrap();
    let parts = frames(&ctx, source).unwrap();
    let value = field(&parts, "iges.global_field", 0).child(70, 74).unwrap();
    assert!(rebuild_fields(
        &ctx,
        source,
        &[FieldEdit {
            source: value,
            replacement: b"x"
        }]
    )
    .is_err());
    assert!(rebuild_fields(
        &ctx,
        source,
        &[
            FieldEdit {
                source: value,
                replacement: b"1234"
            },
            FieldEdit {
                source: value.child(71, 72).unwrap(),
                replacement: b"x"
            },
        ]
    )
    .is_err());
    let detached = View::over_retained(value.window());
    assert!(rebuild_fields(
        &ctx,
        source,
        &[FieldEdit {
            source: detached,
            replacement: b"1234"
        }]
    )
    .is_err());
    let (_, foreign) =
        DecodeContext::from_root_bytes(value.window(), &arena, &DecodePolicy::service()).unwrap();
    assert!(rebuild_fields(
        &ctx,
        source,
        &[FieldEdit {
            source: foreign,
            replacement: b"1234"
        }]
    )
    .is_err());
    let other = ctx.concat_named_views(&[value], "other-composite").unwrap();
    assert!(rebuild_fields(
        &ctx,
        source,
        &[FieldEdit {
            source: other,
            replacement: b"1234"
        }]
    )
    .is_err());
}

#[test]
fn global_numeric_capability_and_version_dispatch_values_are_not_content_edits() {
    let bytes = card(
        b'G',
        1,
        b",,3,4,5,6,7,8,9,10,11,12,13,14,15,16,17,18,19,20,21,22,23;",
        b"\n",
    );
    let arena = DecodeArena::new();
    let (ctx, root) =
        DecodeContext::from_root_bytes(&bytes, &arena, &DecodePolicy::service()).unwrap();
    let parts = frames(&ctx, root).unwrap();
    for index in [4, 5, 6, 7, 8, 20] {
        let value = field(&parts, "iges.global_field", index);
        assert!(rebuild_fields(
            &ctx,
            root,
            &[FieldEdit {
                source: value.child(value.start(), value.start() + 1).unwrap(),
                replacement: b"0",
            }]
        )
        .is_err());
    }
}

#[test]
fn independent_candidate_validation_obeys_the_callers_remaining_retained_budget() {
    let bytes = card(b'G', 1, b",,1234;", b"\n");
    let arena = DecodeArena::new();
    let (ctx, root) =
        DecodeContext::from_root_bytes(&bytes, &arena, &DecodePolicy::service()).unwrap();
    let parts = frames(&ctx, root).unwrap();
    let streams = ctx.source_streams().unwrap();
    let expected = signatures(&ctx, &streams, &parts, "file", 0).unwrap();
    let before = ctx.retained_bytes_available();
    validate_candidate(&ctx, &bytes, &expected).unwrap();
    assert!(ctx.retained_bytes_available() < before);
    let mut policy = DecodePolicy::service();
    policy.limits.max_retained_bytes = 1;
    let (bounded, _) = DecodeContext::from_root_bytes(&bytes, &arena, &policy).unwrap();
    assert!(validate_candidate(&bounded, &bytes, &expected).is_err());
}
