// SPDX-License-Identifier: Apache-2.0
use super::{crc32, frame, put, rebuild, source, Edit, PartTable};

struct Fixture {
    bytes: Vec<u8>,
    payload: usize,
    central: usize,
    extra: usize,
    end64: usize,
    locator: usize,
    descriptor: Option<(usize, bool, bool)>,
}

fn put64(bytes: &mut [u8], at: usize, value: u64) {
    bytes[at..at + 8].copy_from_slice(&value.to_le_bytes());
}

fn fixture(sentinels: [bool; 3], descriptor: Option<bool>) -> Fixture {
    let [uncompressed, compressed, offset] = sentinels;
    let sizes64 = uncompressed || compressed;
    let mut bytes = b"stub".to_vec();
    bytes.extend_from_slice(b"PK\x03\x04");
    bytes.extend_from_slice(&45_u16.to_le_bytes());
    let flags = if descriptor.is_some() { 8_u16 } else { 0 };
    bytes.extend_from_slice(&flags.to_le_bytes());
    bytes.extend_from_slice(&0_u16.to_le_bytes());
    bytes.extend_from_slice(&0x1234_u16.to_le_bytes());
    bytes.extend_from_slice(&0x5678_u16.to_le_bytes());
    bytes.extend_from_slice(
        &if descriptor.is_some() {
            0_u32
        } else {
            crc32(b"ABCDE")
        }
        .to_le_bytes(),
    );
    for _ in 0..2 {
        bytes.extend_from_slice(&if sizes64 { u32::MAX } else { 5 }.to_le_bytes());
    }
    bytes.extend_from_slice(&4_u16.to_le_bytes());
    bytes.extend_from_slice(&if sizes64 { 20_u16 } else { 0 }.to_le_bytes());
    bytes.extend_from_slice(b"data");
    if sizes64 {
        bytes.extend_from_slice(&1_u16.to_le_bytes());
        bytes.extend_from_slice(&16_u16.to_le_bytes());
        bytes.extend_from_slice(&5_u64.to_le_bytes());
        bytes.extend_from_slice(&5_u64.to_le_bytes());
    }
    let payload = bytes.len();
    bytes.extend_from_slice(b"ABCDE");
    let descriptor = descriptor.map(|signed| {
        let start = bytes.len();
        if signed {
            bytes.extend_from_slice(b"PK\x07\x08");
        }
        bytes.extend_from_slice(&crc32(b"ABCDE").to_le_bytes());
        for _ in 0..2 {
            if sizes64 {
                bytes.extend_from_slice(&5_u64.to_le_bytes());
            } else {
                bytes.extend_from_slice(&5_u32.to_le_bytes());
            }
        }
        (start, signed, sizes64)
    });
    let central = bytes.len();
    bytes.extend_from_slice(b"PK\x01\x02");
    bytes.extend_from_slice(&45_u16.to_le_bytes());
    bytes.extend_from_slice(&45_u16.to_le_bytes());
    bytes.extend_from_slice(&flags.to_le_bytes());
    bytes.extend_from_slice(&0_u16.to_le_bytes());
    bytes.extend_from_slice(&0x1234_u16.to_le_bytes());
    bytes.extend_from_slice(&0x5678_u16.to_le_bytes());
    bytes.extend_from_slice(&crc32(b"ABCDE").to_le_bytes());
    bytes.extend_from_slice(&if compressed { u32::MAX } else { 5 }.to_le_bytes());
    bytes.extend_from_slice(&if uncompressed { u32::MAX } else { 5 }.to_le_bytes());
    let scalar_count = sentinels.iter().filter(|value| **value).count();
    let extra_length = if scalar_count == 0 {
        0
    } else {
        4 + scalar_count * 8
    };
    bytes.extend_from_slice(&4_u16.to_le_bytes());
    bytes.extend_from_slice(
        &u16::try_from(extra_length)
            .expect("fixed ZIP64 extra")
            .to_le_bytes(),
    );
    bytes.extend_from_slice(&4_u16.to_le_bytes());
    bytes.extend_from_slice(&0_u16.to_le_bytes());
    bytes.extend_from_slice(&0_u16.to_le_bytes());
    bytes.extend_from_slice(&0_u32.to_le_bytes());
    bytes.extend_from_slice(&if offset { u32::MAX } else { 0 }.to_le_bytes());
    bytes.extend_from_slice(b"data");
    let extra = bytes.len();
    if scalar_count != 0 {
        bytes.extend_from_slice(&1_u16.to_le_bytes());
        bytes.extend_from_slice(
            &u16::try_from(scalar_count * 8)
                .expect("fixed scalar widths")
                .to_le_bytes(),
        );
        if uncompressed {
            bytes.extend_from_slice(&5_u64.to_le_bytes());
        }
        if compressed {
            bytes.extend_from_slice(&5_u64.to_le_bytes());
        }
        if offset {
            bytes.extend_from_slice(&0_u64.to_le_bytes());
        }
    }
    bytes.extend_from_slice(b"note");
    let end64 = bytes.len();
    bytes.extend_from_slice(b"PK\x06\x06");
    bytes.extend_from_slice(&49_u64.to_le_bytes());
    bytes.extend_from_slice(&45_u16.to_le_bytes());
    bytes.extend_from_slice(&45_u16.to_le_bytes());
    bytes.extend_from_slice(&0_u32.to_le_bytes());
    bytes.extend_from_slice(&0_u32.to_le_bytes());
    bytes.extend_from_slice(&1_u64.to_le_bytes());
    bytes.extend_from_slice(&1_u64.to_le_bytes());
    bytes.extend_from_slice(
        &u64::try_from(end64 - central)
            .expect("directory extent")
            .to_le_bytes(),
    );
    bytes.extend_from_slice(
        &u64::try_from(central - 4)
            .expect("directory declaration excludes prefix")
            .to_le_bytes(),
    );
    bytes.extend_from_slice(b"EXTRA");
    let locator = bytes.len();
    bytes.extend_from_slice(b"PK\x06\x07");
    bytes.extend_from_slice(&0_u32.to_le_bytes());
    bytes.extend_from_slice(
        &u64::try_from(end64 - 4)
            .expect("end declaration excludes prefix")
            .to_le_bytes(),
    );
    bytes.extend_from_slice(&1_u32.to_le_bytes());
    bytes.extend_from_slice(b"PK\x05\x06");
    bytes.extend_from_slice(&0_u16.to_le_bytes());
    bytes.extend_from_slice(&0_u16.to_le_bytes());
    bytes.extend_from_slice(&u16::MAX.to_le_bytes());
    bytes.extend_from_slice(&u16::MAX.to_le_bytes());
    bytes.extend_from_slice(&u32::MAX.to_le_bytes());
    bytes.extend_from_slice(&u32::MAX.to_le_bytes());
    bytes.extend_from_slice(&4_u16.to_le_bytes());
    bytes.extend_from_slice(b"tail");
    Fixture {
        bytes,
        payload,
        central,
        extra,
        end64,
        locator,
        descriptor,
    }
}

#[test]
fn actual_zip64_records_and_extra_words_keep_prefix_offsets_and_extension_content() {
    let input = fixture([true, true, true], None);
    source(&input.bytes, |ctx, view| {
        let mut parts = Vec::new();
        let entries = frame(ctx, view, &mut parts).expect("actual ZIP64 records");
        PartTable::load()
            .expect("table")
            .validate(ctx, &parts)
            .expect("source containment");
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].payload().window(), b"ABCDE");
        assert_eq!(entries[0].payload().start(), input.payload);
        let end = parts
            .iter()
            .find(|part| part.level().as_str() == "zip.zip64_end_record")
            .expect("actual end record");
        assert_eq!(
            (end.view().start(), end.view().end()),
            (input.end64, input.locator)
        );
        assert_eq!(
            end.framing(),
            [u64::try_from(input.end64).expect("offset")
                ..u64::try_from(input.end64 + 56).expect("offset")]
        );
        assert_eq!(
            end.content_ranges(),
            [u64::try_from(input.end64 + 56).expect("offset")
                ..u64::try_from(input.locator).expect("offset")]
        );
        assert_eq!(&input.bytes[input.end64 + 56..input.locator], b"EXTRA");
        let locator = parts
            .iter()
            .find(|part| part.level().as_str() == "zip.zip64_end_locator")
            .expect("actual locator");
        assert_eq!(
            locator.framing(),
            [u64::try_from(input.locator).expect("offset")
                ..u64::try_from(input.locator + 20).expect("offset")]
        );
        let central_extra = parts
            .iter()
            .find(|part| part.level().as_str() == "zip.central_extra_field")
            .expect("actual 64-bit scalar block");
        assert_eq!(
            central_extra.framing(),
            [u64::try_from(input.extra).expect("offset")
                ..u64::try_from(input.extra + 28).expect("offset")]
        );
        let local_extra = parts
            .iter()
            .find(|part| part.level().as_str() == "zip.local_extra_field")
            .expect("actual two local sizes");
        assert_eq!(local_extra.framing(), [38..58]);
        assert_eq!(
            rebuild(ctx, view, &[]).expect("all original metadata preserved"),
            input.bytes
        );
        assert!(ctx
            .take_record_captures()
            .expect("no semantic reads")
            .is_empty());
    });
}

#[test]
fn central_scalar_order_uses_only_actual_sentinels_and_offset_only_keeps_32bit_descriptor() {
    for sentinels in [
        [true, false, false],
        [false, true, false],
        [false, false, true],
        [true, false, true],
        [false, false, false],
    ] {
        let input = fixture(sentinels, Some(false));
        source(&input.bytes, |ctx, view| {
            let mut parts = Vec::new();
            let entries = frame(ctx, view, &mut parts).expect("selected source scalar words");
            assert_eq!(entries[0].payload().window(), b"ABCDE");
            let scalar_count = sentinels.iter().filter(|value| **value).count();
            if scalar_count != 0 {
                let extra = parts
                    .iter()
                    .find(|part| part.level().as_str() == "zip.central_extra_field")
                    .expect("actual selected words");
                assert_eq!(
                    extra.framing(),
                    [u64::try_from(input.extra).expect("offset")
                        ..u64::try_from(input.extra + 4 + 8 * scalar_count).expect("offset")]
                );
            }
            let (start, _, wide) = input.descriptor.expect("source descriptor");
            let descriptor = parts
                .iter()
                .find(|part| part.level().as_str() == "zip.data_descriptor")
                .expect("source descriptor frame");
            assert_eq!(descriptor.view().end() - start, if wide { 20 } else { 12 });
        });
    }
}

#[test]
fn wide_signed_and_unsigned_descriptors_reseal_only_payload_and_crc_fields() {
    for signed in [false, true] {
        let input = fixture([true, true, true], Some(signed));
        source(&input.bytes, |ctx, view| {
            let output = rebuild(
                ctx,
                view,
                &[Edit {
                    range: u64::try_from(input.payload).expect("offset")
                        ..u64::try_from(input.payload + 5).expect("offset"),
                    replacement: b"12345",
                }],
            )
            .expect("actual stored ZIP64 edit");
            let mut expected = input.bytes.clone();
            expected[input.payload..input.payload + 5].copy_from_slice(b"12345");
            let (start, signed, _) = input.descriptor.expect("source descriptor");
            put(&mut expected, input.central + 16, crc32(b"12345"));
            put(
                &mut expected,
                start + if signed { 4 } else { 0 },
                crc32(b"12345"),
            );
            assert_eq!(output, expected);
            assert_eq!(&output[input.end64..], &input.bytes[input.end64..]);
        });
    }
    let input = fixture([true, true, true], None);
    source(&input.bytes, |ctx, view| {
        let output = rebuild(
            ctx,
            view,
            &[Edit {
                range: u64::try_from(input.payload).expect("offset")
                    ..u64::try_from(input.payload + 5).expect("offset"),
                replacement: b"12345",
            }],
        )
        .expect("actual local CRC update");
        let mut expected = input.bytes.clone();
        expected[input.payload..input.payload + 5].copy_from_slice(b"12345");
        put(&mut expected, 18, crc32(b"12345"));
        put(&mut expected, input.central + 16, crc32(b"12345"));
        assert_eq!(output, expected);
    });
}

#[test]
fn locator_fallback_uses_an_actual_record_ending_at_the_locator_and_preserves_declaration() {
    let mut input = fixture([true, true, true], None);
    put64(&mut input.bytes, input.locator + 8, u64::MAX);
    source(&input.bytes, |ctx, view| {
        let mut parts = Vec::new();
        frame(ctx, view, &mut parts).expect("table's actual end-at-locator fallback");
        let end = parts
            .iter()
            .find(|part| part.level().as_str() == "zip.zip64_end_record")
            .expect("recovered actual end");
        assert_eq!(end.view().start(), input.end64);
        assert_eq!(
            rebuild(ctx, view, &[]).expect("no fabricated locator repair"),
            input.bytes
        );
    });
}

#[test]
fn zip64_member_words_do_not_require_unneeded_zip64_end_records() {
    let input = fixture([true, true, true], None);
    let mut bytes = input.bytes[..input.end64].to_vec();
    bytes.extend_from_slice(&input.bytes[input.locator + 20..]);
    bytes[input.end64 + 8..input.end64 + 12].copy_from_slice(&[1, 0, 1, 0]);
    put(
        &mut bytes,
        input.end64 + 12,
        u32::try_from(input.end64 - input.central).expect("directory size"),
    );
    put(
        &mut bytes,
        input.end64 + 16,
        u32::try_from(input.central - 4).expect("declared offset"),
    );
    source(&bytes, |ctx, view| {
        let mut parts = Vec::new();
        let entries = frame(ctx, view, &mut parts)
            .expect("actual ZIP64 member sizes in ZIP32 directory footer");
        assert_eq!(entries[0].payload().window(), b"ABCDE");
        assert!(!parts
            .iter()
            .any(|part| part.level().as_str() == "zip.zip64_end_record"));
    });
}

#[test]
fn cut_short_and_impossible_64bit_fields_never_become_whole_window_extents() {
    let input = fixture([true, true, true], Some(true));
    let mut bad_size = input.bytes.clone();
    put64(&mut bad_size, input.end64 + 4, 44);
    let mut huge_count = input.bytes.clone();
    put64(&mut huge_count, input.end64 + 24, u64::MAX);
    put64(&mut huge_count, input.end64 + 32, u64::MAX);
    let mut huge_payload = input.bytes.clone();
    put64(&mut huge_payload, input.extra + 12, u64::MAX);
    let mut absent_extra = input.bytes.clone();
    absent_extra[input.extra..input.extra + 2].copy_from_slice(&0x1234_u16.to_le_bytes());
    let mut short_local_extra = input.bytes.clone();
    short_local_extra[40..42].copy_from_slice(&8_u16.to_le_bytes());
    let mut lost_locator = input.bytes.clone();
    lost_locator[input.locator] = b'X';
    for bytes in [
        bad_size,
        huge_count,
        huge_payload,
        absent_extra,
        short_local_extra,
        lost_locator,
        input.bytes[..input.bytes.len() - 1].to_vec(),
    ] {
        source(&bytes, |ctx, view| {
            assert!(frame(ctx, view, &mut Vec::new()).is_err())
        });
    }
}

#[test]
fn recompressed_zip64_members_keep_sentinels_descriptors_and_shifted_end_records() {
    for mask in 0_u8..8 {
        let sentinels = [mask & 1 != 0, mask & 2 != 0, mask & 4 != 0];
        for descriptor in [None, Some(false), Some(true)] {
            let mut input = fixture(sentinels, descriptor);
            input.bytes[12..14].copy_from_slice(&8_u16.to_le_bytes());
            input.bytes[input.central + 10..input.central + 12]
                .copy_from_slice(&8_u16.to_le_bytes());
            if sentinels[0] || sentinels[1] {
                put64(&mut input.bytes, input.payload - 8, 10);
            } else {
                put(&mut input.bytes, 22, 10);
            }
            if sentinels[1] {
                put64(
                    &mut input.bytes,
                    input.extra + 4 + usize::from(sentinels[0]) * 8,
                    10,
                );
            } else {
                put(&mut input.bytes, input.central + 20, 10);
            }
            if let Some((start, signed, wide)) = input.descriptor {
                let compressed = start + usize::from(signed) * 4 + 4;
                if wide {
                    put64(&mut input.bytes, compressed, 10);
                } else {
                    put(&mut input.bytes, compressed, 10);
                }
            }
            put64(
                &mut input.bytes,
                input.end64 + 48,
                u64::try_from(input.central + 5 - 4).unwrap(),
            );
            put64(
                &mut input.bytes,
                input.locator + 8,
                u64::try_from(input.end64 + 5 - 4).unwrap(),
            );
            input
                .bytes
                .splice(input.payload..input.payload, [1, 5, 0, 250, 255]);
            source(&input.bytes, |ctx, view| {
                let mut frames = Vec::new();
                let original = frame(ctx, view, &mut frames).unwrap();
                assert_eq!(super::rebuild_member::plain(&original[0]), b"ABCDE");
                let rebuilt =
                    crate::framing::zip::rebuild_member(ctx, view, b"data", b"VWXYZ").unwrap();
                assert_eq!(&rebuilt[..4], b"stub");
                assert_eq!(&rebuilt[rebuilt.len() - 4..], b"tail");
                source(&rebuilt, |ctx, view| {
                    let mut parts = Vec::new();
                    let entries = frame(ctx, view, &mut parts).unwrap();
                    assert_eq!(entries[0].method(), 8);
                    assert_eq!(super::rebuild_member::plain(&entries[0]), b"VWXYZ");
                    let end = parts
                        .iter()
                        .find(|part| part.level().as_str() == "zip.zip64_end_record")
                        .unwrap();
                    assert_eq!(&end.view().window()[56..], b"EXTRA");
                    assert_eq!(
                        parts
                            .iter()
                            .filter(|part| part.level().as_str() == "zip.data_descriptor")
                            .count(),
                        usize::from(descriptor.is_some())
                    );
                });
            });
        }
    }
}

#[test]
fn resized_stored_zip64_members_preserve_sentinels_descriptor_forms_and_extensions() {
    for mask in 0_u8..8 {
        let sentinels = [mask & 1 != 0, mask & 2 != 0, mask & 4 != 0];
        for descriptor in [None, Some(false), Some(true)] {
            let input = fixture(sentinels, descriptor);
            source(&input.bytes, |ctx, view| {
                let rebuilt =
                    crate::framing::zip::resize_stored_member(ctx, view, b"data", b"AB").unwrap();
                assert_eq!(rebuilt.len(), input.bytes.len() - 3);
                assert_eq!(&rebuilt[..4], b"stub");
                assert_eq!(&rebuilt[rebuilt.len() - 4..], b"tail");
                source(&rebuilt, |ctx, view| {
                    let mut parts = Vec::new();
                    let entries = frame(ctx, view, &mut parts).unwrap();
                    PartTable::load().unwrap().validate(ctx, &parts).unwrap();
                    let [entry] = entries.as_slice() else {
                        panic!("one retained member")
                    };
                    assert_eq!(entry.method(), 0);
                    assert_eq!(entry.payload().window(), b"AB");
                    assert_eq!(entry.uncompressed, 2);
                    let local_expanded = b"stub".len() + 22;
                    if sentinels[0] || sentinels[1] {
                        assert_eq!(super::word(&rebuilt, local_expanded), u32::MAX);
                        let expanded = input.payload - 16;
                        assert_eq!(
                            u64::from_le_bytes(rebuilt[expanded..expanded + 8].try_into().unwrap()),
                            2,
                        );
                    } else {
                        assert_eq!(super::word(&rebuilt, local_expanded), 2);
                    }
                    if let Some((start, signed, wide)) = input.descriptor {
                        let expanded =
                            start - 3 + usize::from(signed) * 4 + 4 + if wide { 8 } else { 4 };
                        if wide {
                            assert_eq!(
                                u64::from_le_bytes(
                                    rebuilt[expanded..expanded + 8].try_into().unwrap()
                                ),
                                2,
                            );
                        } else {
                            assert_eq!(super::word(&rebuilt, expanded), 2);
                        }
                    }
                    let central = input.central - 3;
                    assert_eq!(
                        super::word(&rebuilt, central + 20),
                        if sentinels[1] { u32::MAX } else { 2 }
                    );
                    assert_eq!(
                        super::word(&rebuilt, central + 24),
                        if sentinels[0] { u32::MAX } else { 2 }
                    );
                    assert_eq!(
                        super::word(&rebuilt, central + 42),
                        if sentinels[2] { u32::MAX } else { 0 }
                    );
                    assert_eq!(super::word(&rebuilt, central + 16), crc32(b"AB"));
                    let end = parts
                        .iter()
                        .find(|part| part.level().as_str() == "zip.zip64_end_record")
                        .unwrap();
                    assert_eq!(&end.view().window()[56..], b"EXTRA");
                    assert_eq!(
                        parts
                            .iter()
                            .filter(|part| part.level().as_str() == "zip.data_descriptor")
                            .count(),
                        usize::from(descriptor.is_some())
                    );
                    assert!(ctx.take_record_captures().unwrap().is_empty());
                });
            });
        }
    }
}

#[test]
fn same_length_member_rebuild_preserves_noncanonical_local_zip64_expanded_size() {
    let mut input = fixture([true, true, true], None);
    put64(&mut input.bytes, input.payload - 16, 123);
    source(&input.bytes, |ctx, view| {
        let entries = frame(ctx, view, &mut Vec::new()).unwrap();
        let entry = &entries[0];
        assert_eq!(entry.uncompressed, 5);
        let expected = rebuild(
            ctx,
            view,
            &[Edit {
                range: u64::try_from(entry.payload().start()).unwrap()
                    ..u64::try_from(entry.payload().end()).unwrap(),
                replacement: b"VWXYZ",
            }],
        )
        .unwrap();
        let rebuilt = crate::framing::zip::rebuild_member(ctx, view, b"data", b"VWXYZ").unwrap();
        assert_eq!(rebuilt, expected);
        assert_eq!(
            u64::from_le_bytes(
                rebuilt[input.payload - 16..input.payload - 8]
                    .try_into()
                    .unwrap()
            ),
            123
        );
    });
}
