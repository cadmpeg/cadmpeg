// SPDX-License-Identifier: Apache-2.0
use super::{Storage, TerminalAtom};
use crate::test_support::{rmfastload::payload, with_decode_context_over};

#[test]
fn rmfastload_physical_boundaries_cross_logical_records() {
    let bytes = payload();
    with_decode_context_over(
        &bytes,
        |_| {},
        |ctx| {
            let storage = Storage::read(ctx, &bytes, 0).unwrap().unwrap();
            assert_eq!(
                (
                    storage.header_offset,
                    storage.boundary_offset,
                    storage.count_offset,
                    storage.table_end,
                    storage.terminal_offset,
                    storage.trailer_offset
                ),
                (41, 77, 93, 109, 132, 162)
            );
            assert_eq!(storage.boundaries().collect::<Vec<_>>(), [0, 112, 120, 132]);
            assert_eq!(storage.table_words().collect::<Vec<_>>(), [1, 17, 99]);
            let rows = storage.triples().collect::<Vec<_>>();
            assert_eq!(
                rows.iter().map(|r| r.offset).collect::<Vec<_>>(),
                [144, 153]
            );
            assert_eq!(rows[1].atoms[0].bytes(), [0xc0, 0, 0, 0]);
            assert_eq!(&bytes[109..117], b"\x04\x01\x06NX x\0");
            assert_eq!(&bytes[117..122], b"\x01\xc0\x44\x04\0");
            assert!(Storage::read(ctx, &bytes, 1).unwrap().is_none());
        },
    );
}

#[test]
fn rmfastload_storage_rejects_incomplete_or_inconsistent_framing() {
    let source = payload();
    // Envelope, marker, reserved word, class count, row count, terminal offset,
    // first/non-monotone boundary, count word, token leader and row terminator.
    for offset in [8, 12, 49, 53, 57, 73, 77, 85, 93, 146, 152] {
        let mut bytes = source.clone();
        bytes[offset] ^= 0xff;
        with_decode_context_over(
            &bytes,
            |_| {},
            |ctx| {
                assert!(
                    Storage::read(ctx, &bytes, 0).unwrap().is_none(),
                    "offset{offset}"
                );
            },
        );
    }
    for len in [0, 31, 76, 131, 161, 173] {
        with_decode_context_over(
            &source[..len],
            |_| {},
            |ctx| {
                assert!(Storage::read(ctx, &source[..len], 0).unwrap().is_none());
            },
        );
    }
}

#[test]
fn rmfastload_terminal_token_serde_preserves_width_without_numeric_meaning() {
    for text in ["00", "7f", "8000", "8fff", "a00000", "afffff", "c0000000"] {
        let json = format!("\"{text}\"");
        let atom: TerminalAtom = serde_json::from_str(&json).unwrap();
        assert_eq!(serde_json::to_string(&atom).unwrap(), json);
    }
    for text in [
        "", "80", "800000", "9000", "a000", "b00000", "c00000", "c1000000", "AF0000",
    ] {
        assert!(serde_json::from_str::<TerminalAtom>(&format!("\"{text}\"")).is_err());
    }
}

#[test]
fn rmfastload_boundary_scan_honors_work_limit() {
    let bytes = payload();
    with_decode_context_over(
        &bytes,
        |p| p.limits.max_work_units = 1,
        |ctx| {
            let error = Storage::read(ctx, &bytes, 0).unwrap_err();
            assert!(matches!(error, cadmpeg_core::CodecError::ResourceLimit(_)));
            assert!(ctx.resource_refusal().is_some());
        },
    );
}
