// SPDX-License-Identifier: Apache-2.0
use std::collections::BTreeMap;

use cadmpeg_core::decode::{ByteRange, DecodeArena, DecodeContext, DecodePolicy};
use cadmpeg_core::CodecError;
use cadmpeg_ir::SourceFidelity;

use super::capture_record;
use crate::decode::source_layout::{install, PendingLayouts};
use crate::design::test_support::parameter_record;
use crate::records::parameters::DesignParameter;

const STREAM: &str = "Design1/BulkStream.dat";

fn parsed(bytes: &[u8], offset: usize) -> DesignParameter {
    super::super::parse_design_parameter(bytes)
        .expect("parameter grammar")
        .expect("typed parameter")
        .into_record(STREAM, u64::try_from(offset).expect("offset fits"))
        .expect("located parameter")
}

fn expected(offset: u64) -> BTreeMap<String, u64> {
    // The fixture writes the indexed header, u64 discriminator, ordinal and null owner
    // before its five-code-unit expression. A text pointer follows its u32 count.
    BTreeMap::from([
        ("byte_offset".to_owned(), offset),
        ("family_discriminator_offset".to_owned(), offset + 22),
        ("expression_offset".to_owned(), offset + 40),
        ("source_kind_offset".to_owned(), offset + 63),
        ("unit_offset".to_owned(), offset + 99),
        ("name_offset".to_owned(), offset + 107),
        ("evaluated_value_offset".to_owned(), offset + 117),
    ])
}

#[test]
fn all_parameter_offsets_have_one_original_member_owner_and_no_metadata_copy() {
    let bytes = parameter_record(None, "12 mm", "User Parameter", Some("mm"), "Width", 1.2);
    assert_eq!(bytes.len(), 137);
    assert_eq!(&bytes[36..40], &5_u32.to_le_bytes());
    assert_eq!(
        &bytes[40..50],
        &[b'1', 0, b'2', 0, b' ', 0, b'm', 0, b'm', 0]
    );
    let mut enclosing = b"prefix!xyz".to_vec();
    enclosing.extend_from_slice(&bytes);
    enclosing.extend_from_slice(b"suffix");
    let arena = DecodeArena::new();
    let (ctx, root) = DecodeContext::from_root_bytes(&enclosing, &arena, &DecodePolicy::default())
        .expect("context");
    let member = ctx
        .register_named_slice(root, ByteRange { start: 7, end: 147 }, STREAM)
        .expect("original member");
    let view = member.child(3, 140).expect("actual indexed owner");
    assert_eq!(view.window(), bytes);
    let mut parameter = parsed(&bytes, 3);
    let original = serde_json::to_value(&parameter).expect("original semantic fields");
    let mut pointers = BTreeMap::new();
    let (_, batch) = ctx
        .record_batch(|| capture_record(&ctx, view, STREAM, 3, &mut parameter, &mut pointers))
        .expect("actual source producer");
    assert_eq!(
        serde_json::to_value(&parameter).expect("native fields"),
        original
    );
    assert_eq!(
        pointers,
        BTreeMap::from([(parameter.id.clone(), expected(3))])
    );
    let native = crate::native::F3dNative {
        design_parameters: vec![parameter.clone()],
        ..Default::default()
    };
    let mut fidelity = native.retained_fidelity().expect("source fidelity");
    assert!(fidelity.metadata.is_empty());
    assert!(fidelity.retained_records().is_empty());
    let mut pending = PendingLayouts::new(batch, pointers);
    pending.rescope(&ctx, "one").expect("occurrence");
    install(&ctx, &mut fidelity, vec![pending]).expect("final installation");
    let owner = crate::f3z::merge::rescope_record_id(&parameter.id, "one").expect("owner scope");
    assert_eq!(fidelity.record_layout().len(), 1);
    let layout = &fidelity.record_layout()[&owner];
    assert_eq!(layout.stream(), "f3d:xref/one/Design1/BulkStream.dat");
    assert_eq!((layout.offset(), layout.byte_len()), (3, 137));
    assert_eq!(layout.pointers(), &expected(3));
    assert!(layout.reads().is_empty());
    assert!(layout.payloads().is_empty());
    assert!(fidelity.metadata.is_empty());
}

#[test]
fn a_preexisting_owner_keeps_its_capture_and_gains_only_source_proven_offsets() {
    let bytes = parameter_record(None, "12 mm", "User Parameter", Some("mm"), "Width", 1.2);
    let arena = DecodeArena::new();
    let (ctx, root) =
        DecodeContext::from_root_bytes(&bytes, &arena, &DecodePolicy::default()).expect("context");
    let root = ctx.name_stream(root, STREAM).expect("stream");
    let mut parameter = parsed(&bytes, 0);
    let mut pointers = BTreeMap::from([(
        parameter.id.clone(),
        BTreeMap::from([("byte_offset".to_owned(), 0)]),
    )]);
    let (_, batch) = ctx
        .record_batch(|| {
            ctx.record(&parameter.id, root, || Ok(()))?;
            capture_record(&ctx, root, STREAM, 0, &mut parameter, &mut pointers)
        })
        .expect("one owner capture");
    let mut fidelity = SourceFidelity::default();
    install(
        &ctx,
        &mut fidelity,
        vec![PendingLayouts::new(batch, pointers)],
    )
    .expect("no duplicate capture");
    assert_eq!(fidelity.record_layout().len(), 1);
    let layout = &fidelity.record_layout()[&parameter.id];
    assert_eq!(layout.pointers(), &expected(0));
    assert!(layout.reads().is_empty());
}

#[test]
fn conflicting_pointer_and_foreign_source_cannot_replace_the_parameter() {
    let bytes = parameter_record(None, "12 mm", "User Parameter", Some("mm"), "Width", 1.2);
    let arena = DecodeArena::new();
    let (ctx, root) =
        DecodeContext::from_root_bytes(&bytes, &arena, &DecodePolicy::default()).expect("context");
    let mut parameter = parsed(&bytes, 0);
    parameter.set_parameter_state("f3d:Design1:parameter-state#401".to_owned());
    let original = serde_json::to_value(&parameter).expect("original fields");
    let mut pointers = BTreeMap::from([(
        parameter.id.clone(),
        BTreeMap::from([("byte_offset".to_owned(), 1)]),
    )]);
    let original_pointers = pointers.clone();
    assert!(capture_record(&ctx, root, STREAM, 0, &mut parameter, &mut pointers).is_err());
    assert_eq!(serde_json::to_value(&parameter).expect("fields"), original);
    assert_eq!(pointers, original_pointers);
    assert!(ctx.take_record_captures().expect("no capture").is_empty());

    let other_arena = DecodeArena::new();
    let (other, _) = DecodeContext::from_root_bytes(&bytes, &other_arena, &DecodePolicy::default())
        .expect("other context");
    let mut pointers = BTreeMap::new();
    assert!(capture_record(&other, root, STREAM, 0, &mut parameter, &mut pointers).is_err());
    assert_eq!(serde_json::to_value(&parameter).expect("fields"), original);
    assert!(pointers.is_empty());
    assert!(other
        .take_record_captures()
        .expect("no foreign capture")
        .is_empty());
}

#[test]
fn source_value_changes_feed_the_stored_parameter_without_offset_metadata() {
    let original_bytes =
        parameter_record(None, "12 mm", "User Parameter", Some("mm"), "Width", 1.2);
    let mut bytes = original_bytes.clone();
    bytes[117..125].copy_from_slice(&2.4_f64.to_le_bytes());
    let arena = DecodeArena::new();
    let (ctx, root) =
        DecodeContext::from_root_bytes(&bytes, &arena, &DecodePolicy::default()).expect("context");
    let mut parameter = parsed(&original_bytes, 0);
    assert_eq!(parameter.evaluated_value().get(), 1.2);
    let mut pointers = BTreeMap::new();
    capture_record(&ctx, root, STREAM, 0, &mut parameter, &mut pointers)
        .expect("actual source values");
    assert_eq!(parameter.evaluated_value().get(), 2.4);
    assert_eq!(parameter.expression(), "12 mm");
    assert_eq!(parameter.name(), "Width");
    assert_eq!(pointers[&parameter.id], expected(0));
    let captures = ctx
        .take_record_captures()
        .expect("committed source capture");
    assert_eq!(captures.len(), 1);
    assert_eq!(captures[0].id(), parameter.id);
}

#[test]
fn parameter_layout_resource_refusal_has_no_native_or_pointer_substitute() {
    let bytes = parameter_record(None, "12 mm", "User Parameter", Some("mm"), "Width", 1.2);
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::default();
    policy.limits.max_retained_bytes = 0;
    let (ctx, root) = DecodeContext::from_root_bytes(&bytes, &arena, &policy).expect("context");
    let mut parameter = parsed(&bytes, 0);
    let original = serde_json::to_value(&parameter).expect("original fields");
    let mut pointers = BTreeMap::new();
    assert!(matches!(
        capture_record(&ctx, root, STREAM, 0, &mut parameter, &mut pointers),
        Err(CodecError::ResourceLimit(_))
    ));
    assert_eq!(serde_json::to_value(&parameter).expect("fields"), original);
    assert!(pointers.is_empty());
    assert!(matches!(
        ctx.finish_session(),
        Err(CodecError::ResourceLimit(_))
    ));
}
