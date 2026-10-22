// SPDX-License-Identifier: Apache-2.0
//! Actual stream inventory admission and owner-derived byte loss attribution.

use super::*;
use cadmpeg_core::decode::{DecodeArena, DecodePolicy};
use cadmpeg_ir::source_fidelity::RecordLayout;
use cadmpeg_ir::{ArenaClass, RetainedBytes, RetainedSourceRecord};

#[test]
fn a_missing_actual_stream_cannot_finalize_or_mutate_fidelity_and_losses() {
    let arena = DecodeArena::new();
    let policy = DecodePolicy::service();
    let (ctx, _) = DecodeContext::from_root_bytes(b"source", &arena, &policy).unwrap();
    let inventory = Inventory(BTreeMap::new());
    let mut fidelity = SourceFidelity::default();
    let original = fidelity.clone();
    let mut losses = Vec::new();
    assert!(inventory
        .finalize(&ctx, &CadIr::empty(), &mut fidelity, &mut losses)
        .is_err());
    assert_eq!(fidelity, original);
    assert!(losses.is_empty());
}

#[test]
fn a_foreign_view_or_partial_window_cannot_supply_a_whole_source_inventory() {
    let first_arena = DecodeArena::new();
    let second_arena = DecodeArena::new();
    let policy = DecodePolicy::service();
    let (ctx, root) = DecodeContext::from_root_bytes(b"same", &first_arena, &policy).unwrap();
    let (_foreign_ctx, foreign) =
        DecodeContext::from_root_bytes(b"same", &second_arena, &policy).unwrap();
    let actual = ctx.source_streams().unwrap();
    let mut inventory = Inventory(BTreeMap::new());
    assert!(inventory.stream(&ctx, foreign, &actual).is_err());
    assert!(inventory.0.is_empty());
    assert!(inventory
        .stream(&ctx, root.child(1, 3).unwrap(), &actual)
        .is_err());
    assert!(inventory.0.is_empty());
    assert!(ctx.take_record_captures().unwrap().is_empty());
}

#[test]
fn registering_another_real_stream_invalidates_the_complete_inventory_proof() {
    let arena = DecodeArena::new();
    let policy = DecodePolicy::service();
    let (ctx, root) = DecodeContext::from_root_bytes(b"source", &arena, &policy).unwrap();
    let mut inventory = Inventory(BTreeMap::new());
    inventory
        .stream(&ctx, root, &ctx.source_streams().unwrap())
        .unwrap();
    assert!(inventory.is_complete(&ctx).unwrap());
    ctx.register_named_slice(
        root,
        cadmpeg_core::decode::ByteRange { start: 1, end: 3 },
        "later",
    )
    .unwrap();
    assert!(!inventory.is_complete(&ctx).unwrap());
    let mut fidelity = SourceFidelity::default();
    let original = fidelity.clone();
    let mut losses = Vec::new();
    assert!(inventory
        .finalize(&ctx, &CadIr::empty(), &mut fidelity, &mut losses)
        .is_err());
    assert_eq!(fidelity, original);
    assert!(losses.is_empty());
}

#[test]
fn known_non_xml_grammars_remain_pending_and_vendor_payloads_remain_opaque() {
    for (value, pending) in [
        (
            "<Property type='App::PropertyFloatList'><FloatList file='values'/></Property>",
            true,
        ),
        (
            "<Property type='Vendor::PropertyFloatList'><FloatList file='values'/></Property>",
            false,
        ),
        (
            "<Property type='Part::PropertyPartShape'><Part file='shape'/></Property>",
            true,
        ),
        (
            "<Property type='App::PropertyFileIncluded'><FileIncluded file='opaque'/></Property>",
            false,
        ),
        ("<StringHasher count='0'/>", true),
    ] {
        let source = format!("<Document>{value}</Document>");
        let xml = roxmltree::Document::parse(&source).unwrap();
        assert_eq!(pending_grammar(&xml), pending, "{source}");
    }
}

#[test]
fn automatic_owner_classification_preserves_reads_and_names_the_exact_unread_span() {
    let arena = DecodeArena::new();
    let policy = DecodePolicy::service();
    let (ctx, root) = DecodeContext::from_root_bytes(b"abcd", &arena, &policy).unwrap();
    let id = "fcstd:native:entry#payload.bin";
    let mut inventory = Inventory(BTreeMap::new());
    inventory
        .stream(&ctx, root, &ctx.source_streams().unwrap())
        .unwrap();
    let entry = crate::native::EntryRecord::new(
        id.into(),
        "payload.bin".into(),
        cadmpeg_core::container::ContainerRole::Auxiliary,
        Vec::new(),
    )
    .unwrap();
    let mut ir = CadIr::empty();
    ir.native
        .namespace_mut("fcstd")
        .set_arena(&ctx, "entries", &[entry])
        .unwrap();
    ctx.record(id, root, || {
        let mut read = root;
        assert_eq!(read.u8(), Some(b'a'));
        Ok(())
    })
    .unwrap();
    let mut fidelity = SourceFidelity::default();
    let (id, layout) =
        RecordLayout::from_capture(ctx.take_record_captures().unwrap().pop().unwrap());
    fidelity.insert_record_layout(id, layout).unwrap();
    fidelity
        .insert_retained_record(
            crate::fidelity::SOURCE_IMAGE.try_into().unwrap(),
            RetainedSourceRecord::from_bytes(
                "file",
                0,
                RetainedBytes::Inline {
                    data: b"abcd".to_vec(),
                },
            )
            .unwrap(),
        )
        .unwrap();
    let mut losses = Vec::new();
    inventory
        .finalize(&ctx, &ir, &mut fidelity, &mut losses)
        .unwrap();
    assert_eq!(
        fidelity.record_layout()["fcstd:native:entry#payload.bin"].reads()["file"],
        [0..1]
    );
    let spans = fidelity.b1_spans()["file"].spans();
    assert_eq!(spans.len(), 1);
    assert_eq!(spans[0].range(), 1..4);
    assert_eq!(
        spans[0].classes(),
        [ArenaClass::Model, ArenaClass::Design].into()
    );
    assert_eq!(
        fidelity.unrepresented(),
        [ArenaClass::Model, ArenaClass::Design].into()
    );
    assert_eq!(
        fidelity
            .retained_record(crate::fidelity::SOURCE_IMAGE)
            .unwrap()
            .untyped(),
        &fidelity.unrepresented()
    );
    assert_eq!(losses.len(), 1);
    assert_eq!(losses[0].code.local_code(), "source.bytes-undecoded");
    let location = losses[0].provenance.as_ref().unwrap();
    assert_eq!(location.stream_name(), Some("file"));
    assert_eq!(location.offset, 1);
    assert_eq!(location.byte_len(), Some(3));
    let partition =
        partition_stream(&ctx, "file", 4, fidelity.record_layout(), &[], &[], &[]).unwrap();
    let index =
        ClassificationIndex::new_with_owner_resolver(&ctx, &ir, &super::super::owners::Owners)
            .unwrap();
    fidelity
        .check_b1_closure(&ctx, &index, "fcstd", &[partition], &losses)
        .unwrap();
}

#[test]
fn an_unlisted_registered_stream_prevents_complete_source_grammar_admission() {
    let document = "<Document SchemaVersion='4' FileVersion='1'><Objects Count='0'/><ObjectData Count='0'/></Document>";
    let bytes = crate::test_support::test_archive::archive(document);
    let arena = DecodeArena::new();
    let policy = DecodePolicy::service();
    let (ctx, root) = DecodeContext::from_root_bytes(&bytes, &arena, &policy).unwrap();
    let (scan, _) = crate::container::scan_for_decode(&ctx, root).unwrap();
    ctx.register_named_slice(
        root,
        cadmpeg_core::decode::ByteRange { start: 0, end: 2 },
        "unlisted",
    )
    .unwrap();
    assert!(matches!(
        prepare(&ctx, root, &scan).unwrap(),
        Admission::Pending
    ));
    assert!(ctx.take_record_captures().unwrap().is_empty());
}
