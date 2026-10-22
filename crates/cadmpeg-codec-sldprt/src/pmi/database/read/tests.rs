// SPDX-License-Identifier: Apache-2.0
//! Actual inline and overflow cell extents for semantic PMI source capture.

use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, SourceOrigin};

const KEY: &str = "00000000-0000-0000-0000-000000000001";

#[test]
fn inline_cell_view_preserves_key_value_and_nonzero_parent_coordinates() {
    let value = b"native dimension bytes".to_vec();
    let database =
        crate::pmi::database::encode(&[(KEY.into(), value.clone())]).expect("valid source fixture");
    let mut source = b"prefix".to_vec();
    let start = source.len();
    source.extend_from_slice(&database);
    let end = source.len();
    source.extend_from_slice(b"tail");
    let arena = DecodeArena::new();
    let (ctx, root) = DecodeContext::from_root_bytes(&source, &arena, &DecodePolicy::service())
        .expect("valid source fixture");
    let images = super::images(&ctx, root.child(start, end).expect("valid source fixture"))
        .expect("valid source fixture")
        .expect("valid source fixture");
    let [image] = images.as_slice() else {
        panic!("one source cell")
    };
    assert_eq!(image.key, KEY);
    let mut expected = KEY.as_bytes().to_vec();
    expected.extend_from_slice(&value);
    assert_eq!(image.encoded.window(), expected);
    assert_eq!(image.source_offset(0), 2 * 4096 + 38 + 36);
    let physical_value_start =
        u64::try_from(start + 2 * 4096 + 38 + KEY.len()).expect("valid source fixture");
    ctx.record(
        "sldprt:test:pmi-value#0",
        image
            .encoded
            .child(KEY.len(), expected.len())
            .expect("valid source fixture"),
        || {
            assert_eq!(
                image.encoded.read_at(KEY.len(), value.len()),
                Some(value.as_slice())
            );
            Ok(())
        },
    )
    .expect("valid source fixture");
    let captures = ctx.take_record_captures().expect("valid source fixture");
    assert_eq!(
        captures[0].reads().get("file"),
        Some(&vec![
            physical_value_start
                ..physical_value_start + u64::try_from(value.len()).expect("valid source fixture")
        ])
    );
}

#[test]
fn overflow_cell_view_maps_a_semantic_read_across_exact_overflow_page_pieces() {
    let value = vec![b'v'; 10_000];
    let source =
        crate::pmi::database::encode(&[(KEY.into(), value.clone())]).expect("valid source fixture");
    let arena = DecodeArena::new();
    let (ctx, root) = DecodeContext::from_root_bytes(&source, &arena, &DecodePolicy::service())
        .expect("valid source fixture");
    let images = super::images(&ctx, root)
        .expect("valid source fixture")
        .expect("valid source fixture");
    let [image] = images.as_slice() else {
        panic!("one overflow source cell")
    };
    assert_eq!(image.key, KEY);
    assert_eq!(&image.encoded.window()[KEY.len()..], value);
    let seam = 4096 - 18;
    ctx.record(
        "sldprt:test:pmi-seam#0",
        image
            .encoded
            .child(seam - 1, seam + 1)
            .expect("valid source fixture"),
        || {
            assert_eq!(
                image.encoded.read_at(seam - 1, 2),
                Some([b'v'; 2].as_slice())
            );
            Ok(())
        },
    )
    .expect("valid source fixture");
    let capture = ctx
        .capture_part_extent(image.encoded)
        .expect("valid source fixture");
    assert!(ctx.source_streams().expect("valid source fixture").iter().any(|stream| {
        stream.name() == capture.stream() && matches!(stream.origin(), SourceOrigin::Composite { segments } if segments.len() == 3)
    }));
    let captures = ctx.take_record_captures().expect("valid source fixture");
    assert_eq!(
        captures[0].reads().get("file"),
        Some(&vec![16383..16384, 16392..16393])
    );

}

#[test]
fn independent_cells_keep_distinct_actual_owners_without_semantic_reads() {
    let other = "00000000-0000-0000-0000-000000000002";
    let source = crate::pmi::database::encode(&[(KEY.into(), vec![1]), (other.into(), vec![2])])
        .expect("valid source fixture");
    let arena = DecodeArena::new();
    let (ctx, root) = DecodeContext::from_root_bytes(&source, &arena, &DecodePolicy::service())
        .expect("valid source fixture");
    let images = super::images(&ctx, root)
        .expect("valid source fixture")
        .expect("valid source fixture");
    assert_eq!(images.len(), 2);
    assert_ne!(
        ctx.capture_part_extent(images[0].encoded)
            .expect("valid source fixture")
            .stream(),
        ctx.capture_part_extent(images[1].encoded)
            .expect("valid source fixture")
            .stream()
    );
    assert_eq!(&images[0].encoded.window()[KEY.len()..], &[1]);
    assert_eq!(&images[1].encoded.window()[other.len()..], &[2]);
    assert!(ctx
        .take_record_captures()
        .expect("valid source fixture")
        .is_empty());
}

#[test]
fn semantic_pmi_parser_uses_actual_cell_bytes_without_changing_native_values() {
    let encoded = crate::test_support::pmi::pmi_semantic_payload_for_with_guid("D1@Sketch1", KEY);
    let source = crate::pmi::database::encode(&[(KEY.into(), encoded[KEY.len()..].to_vec())])
        .expect("semantic source database");
    let arena = DecodeArena::new();
    let (ctx, root) = DecodeContext::from_root_bytes(&source, &arena, &DecodePolicy::service())
        .expect("semantic database source");
    let mut losses = Vec::new();
    let records = crate::pmi::parse_payload(&ctx, root, &mut losses).expect("PMI source cells");
    assert!(losses.is_empty(), "{losses:?}");
    let [record] = records.as_slice() else {
        panic!("one PMI dimension")
    };
    assert_eq!(record.guid, KEY);
    assert_eq!(record.cad_text, "D1@Sketch1");
    assert_eq!(record.value.get(), 0.025);
    let streams = ctx.source_streams().expect("source streams");
    assert_eq!(streams.len(), 2);
    assert!(streams.iter().any(|stream| matches!(stream.origin(), SourceOrigin::Composite { segments } if segments.len() == 1)));
}
