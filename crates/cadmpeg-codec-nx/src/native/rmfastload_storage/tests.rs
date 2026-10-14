// SPDX-License-Identifier: Apache-2.0
use super::{admit, collect, RmFastLoadStorage};
use crate::test_support::{rmfastload, with_decode_context_over};

#[test]
fn rmfastload_storage_retains_entry_relative_spans_and_opaque_terminal_bytes() {
    let bytes = rmfastload::prt();
    with_decode_context_over(
        &bytes,
        |_| {},
        |ctx| {
            let container = crate::container::scan_bytes(ctx, bytes.clone()).unwrap();
            let records = collect(ctx, &container, &[]).unwrap();
            assert_eq!(records.len(), 1);
            let wire = &records[0].0;
            assert_eq!(wire.byte_len, 174);
            assert_eq!(wire.header_source_offset, wire.source_offset + 41);
            assert_eq!(wire.boundary_source_offset, wire.source_offset + 77);
            assert_eq!(wire.table_count_source_offset, wire.source_offset + 93);
            assert_eq!(wire.boundaries, [0, 112, 120, 132]);
            assert_eq!(wire.table_words, [1, 17, 99]);
            let source = rmfastload::payload();
            for (span, pair) in wire.physical_spans.iter().zip(wire.boundaries.windows(2)) {
                let lo = usize::try_from(pair[0]).unwrap();
                let hi = usize::try_from(pair[1]).unwrap();
                assert_eq!(span.source_offset, wire.source_offset + u64::from(pair[0]));
                assert_eq!(span.byte_len, u64::from(pair[1] - pair[0]));
                assert_eq!(
                    span.sha256,
                    cadmpeg_ir::hash::digest::Sha256Digest::digest(&source[lo..hi])
                );
            }
            let value = serde_json::to_value(&records[0]).unwrap();
            assert_eq!(value["terminal_trailer"], "ffffffffffffffffffffffff");
            assert_eq!(value["terminal_rows"][1]["tokens"][0], "c0000000");
            assert_eq!(
                serde_json::from_value::<RmFastLoadStorage>(value).unwrap(),
                records[0]
            );
        },
    );
}

#[test]
fn rmfastload_native_wire_rejects_missing_arrays_and_inconsistent_source_positions() {
    let bytes = rmfastload::prt();
    with_decode_context_over(
        &bytes,
        |_| {},
        |ctx| {
            let container = crate::container::scan_bytes(ctx, bytes.clone()).unwrap();
            let value = serde_json::to_value(&collect(ctx, &container, &[]).unwrap()[0]).unwrap();
            for field in [
                "boundaries",
                "table_words",
                "physical_spans",
                "terminal_rows",
            ] {
                let mut bad = value.clone();
                bad[field].as_array_mut().unwrap().pop();
                assert!(
                    serde_json::from_value::<RmFastLoadStorage>(bad).is_err(),
                    "{field}"
                );
            }
            for field in [
                "header_source_offset",
                "boundary_source_offset",
                "table_count_source_offset",
                "byte_len",
            ] {
                let mut bad = value.clone();
                bad[field] = serde_json::json!(u64::MAX);
                assert!(
                    serde_json::from_value::<RmFastLoadStorage>(bad).is_err(),
                    "{field}"
                );
            }
            let mut bad = value.clone();
            bad["header_source_offset"] = bad["source_offset"].clone();
            assert!(serde_json::from_value::<RmFastLoadStorage>(bad).is_err());
            let mut bad = value.clone();
            bad["physical_spans"][0]["byte_len"] = serde_json::json!(113);
            assert!(serde_json::from_value::<RmFastLoadStorage>(bad).is_err());
            let mut bad = value;
            bad["terminal_rows"][0]["source_offset"] = serde_json::json!(0);
            assert!(serde_json::from_value::<RmFastLoadStorage>(bad).is_err());
        },
    );
}

#[test]
fn rmfastload_native_admission_checks_the_owning_section_class_count() {
    use cadmpeg_ir::codec::{Codec, CodecBackend, DecodeOptions};
    let result = crate::NxCodec
        .decode(
            &mut std::io::Cursor::new(rmfastload::prt()),
            &DecodeOptions::default(),
        )
        .unwrap();
    let (mut ir, _, _) = result.into_parts();
    crate::test_support::with_decode_context(|ctx| {
        assert!(<crate::NxCodec as CodecBackend>::validate_native(ctx, &ir)
            .unwrap()
            .is_empty());
        let namespace = ir.native.namespace_mut("nx");
        let original =
            serde_json::to_value(&namespace.arenas()["rmfastload_physical_storages"][0]).unwrap();
        let mut bad = original.clone();
        bad["header_words"][3] = serde_json::json!(1);
        assert!(serde_json::from_value::<RmFastLoadStorage>(bad.clone()).is_ok());
        namespace
            .set_arena(ctx, "rmfastload_physical_storages", &[bad])
            .unwrap();
        assert!(admit(ctx, namespace).is_err());
        namespace
            .set_arena(ctx, "rmfastload_physical_storages", &[original])
            .unwrap();
        assert!(admit(ctx, namespace).is_ok());
    });
}
