// SPDX-License-Identifier: Apache-2.0
//! Literal primary-frame controls for parameter-state source ownership.

use super::*;
use crate::records::{
    entity_header::{BaseTypeGuid, SegmentTypeData},
    identity::ReferenceRun,
};
use cadmpeg_core::decode::{DecodeArena, DecodePolicy};

fn indexed(tag: &str, index: u32) -> Vec<u8> {
    let mut bytes = 3_u32.to_le_bytes().to_vec();
    bytes.extend_from_slice(tag.as_bytes());
    bytes.extend_from_slice(&index.to_le_bytes());
    bytes
}
fn local(bytes: &mut Vec<u8>, index: u64) {
    bytes.push(1);
    bytes.extend_from_slice(&index.to_le_bytes());
    bytes.extend_from_slice(&[0, 0]);
}
fn state_bytes(index: u32, other: u64, root: u64) -> Vec<u8> {
    let mut bytes = indexed("257", index);
    bytes.extend_from_slice(&[0; 23]);
    local(&mut bytes, other);
    bytes.extend_from_slice(&[0; 5]);
    local(&mut bytes, root);
    assert_eq!(bytes.len(), 61);
    bytes
}
fn companion_bytes(index: u32, other: u64) -> Vec<u8> {
    let mut bytes = indexed("258", index);
    bytes.extend_from_slice(&[0; 8]);
    bytes.push(1);
    bytes.extend_from_slice(&1_u32.to_le_bytes());
    local(&mut bytes, other);
    bytes.extend_from_slice(&[0; 9]);
    assert_eq!(bytes.len(), 44);
    bytes
}
fn registration(tag: &str, guid: &str, version: u32, entities: Vec<u64>) -> SegmentTypeData {
    SegmentTypeData {
        class_tag: tag.to_owned().try_into().expect("class"),
        byte_offset: None,
        type_guid: guid.to_owned().try_into().expect("GUID"),
        type_guid_offset: None,
        base_type_guid: BaseTypeGuid::Absent,
        version,
        version_offset: None,
        module: if guid == ROOT_GUID {
            "Fusion"
        } else {
            "CommonData"
        }
        .into(),
        entities: ReferenceRun::unlocated(entities),
    }
}
fn fixture(second: bool) -> (Vec<u8>, crate::metastream::MetaStream) {
    let pairs = [(401, 702), (503, 804)];
    fixture_with_indices(&pairs[..if second { 2 } else { 1 }])
}

fn fixture_with_indices(pairs: &[(u64, u64)]) -> (Vec<u8>, crate::metastream::MetaStream) {
    let mut bytes = indexed("256", 93);
    bytes.extend_from_slice(b"root-body!");
    assert_eq!(bytes.len(), 21);
    let mut records = vec![crate::metastream::RecordIndexEntry {
        entity_id: 93,
        bulk_offset: 0,
    }];
    let mut states = vec![];
    let mut companions = vec![];
    for &(state, companion) in pairs {
        records.push(crate::metastream::RecordIndexEntry {
            entity_id: state,
            bulk_offset: u64_from_index(bytes.len()),
        });
        bytes.extend(state_bytes(
            u32::try_from(state).expect("state"),
            companion,
            93,
        ));
        records.push(crate::metastream::RecordIndexEntry {
            entity_id: companion,
            bulk_offset: u64_from_index(bytes.len()),
        });
        bytes.extend(companion_bytes(
            u32::try_from(companion).expect("companion"),
            state,
        ));
        states.push(state);
        companions.push(companion);
    }
    (
        bytes,
        crate::metastream::MetaStream {
            types: vec![
                registration("256", ROOT_GUID, 23, vec![93]),
                registration("257", STATE_GUID, 4, states),
                registration("258", COMPANION_GUID, 1, companions),
            ],
            records,
            secondary_records: vec![],
        },
    )
}

#[test]
fn state_records_keep_actual_nonconstant_links_and_only_owned_reads() {
    let (bytes, meta) = fixture(false);
    let mut enclosing = b"prefix!".to_vec();
    enclosing.extend_from_slice(&bytes);
    enclosing.extend_from_slice(b"suffix");
    let arena = DecodeArena::new();
    let (ctx, root) = DecodeContext::from_root_bytes(&enclosing, &arena, &DecodePolicy::default())
        .expect("context");
    let root = ctx.name_stream(root, "file").expect("source name");
    let source = root.child(7, 7 + bytes.len()).expect("source window");
    let frames = supported_frames(&ctx, source, &meta).expect("primary frames");
    let mut output = PartOutput::new(CodecFormat::F3d);
    decode_stream(&ctx, source, "Design1/BulkStream.dat", &frames, &mut output).expect("decode");
    let (_, native, fidelity, losses) = output.into_parts(&ctx).expect("closure").into_parts();
    assert!(losses.is_empty());
    assert_eq!(fidelity.parts().len(), 3);
    let namespace = native.namespace("f3d").expect("native");
    let states = namespace
        .arena_as::<DesignParameterState>("design_parameter_states")
        .expect("states");
    let companions = namespace
        .arena_as::<DesignParameterStateCompanion>("design_parameter_state_companions")
        .expect("companions");
    assert_eq!(states[0].record_index, 401);
    assert_eq!(companions[0].record_index, 702);
    assert_eq!(companions[0].state_record_index, 401);
    assert_eq!(states[0].companion, companions[0].id);
    assert_eq!((companions[0].control, companions[0].control_word), (1, 1));
    let captures = ctx.take_record_captures().expect("capture drain");
    let ranges = |id: &str| {
        captures
            .iter()
            .find(|capture| capture.id() == id)
            .expect("owner")
            .reads()["file"]
            .clone()
    };
    assert_eq!(ranges(&states[0].id), vec![32..39, 63..71, 79..87]);
    assert_eq!(ranges(&companions[0].id), vec![93..100, 108..113, 114..122]);
    let headers = namespace
        .arena_as::<DesignParameterDocumentHeader>("design_parameter_document_headers")
        .expect("headers");
    assert_eq!(headers[0].record_index, 93);
    assert_eq!(states[0].document_root, headers[0].id);
    assert_eq!(ranges(&headers[0].id), vec![11..18]);
    assert_eq!(
        captures
            .iter()
            .find(|capture| capture.id() == headers[0].id)
            .expect("root header")
            .byte_len(),
        11
    );
    let serialized = serde_json::to_vec(&states[0]).expect("serialize state");
    assert_eq!(
        serde_json::from_slice::<DesignParameterState>(&serialized).expect("hydrate state"),
        states[0]
    );
}

#[test]
fn invalid_linked_companion_gives_up_only_its_pair_and_keeps_the_independent_peer() {
    let (mut bytes, meta) = fixture(true);
    // The first companion's control is at 21 + 61 + 19.
    bytes[101] = 2;
    let arena = DecodeArena::new();
    let (ctx, root) =
        DecodeContext::from_root_bytes(&bytes, &arena, &DecodePolicy::default()).expect("context");
    let root = ctx.name_stream(root, "file").expect("source name");
    let frames = supported_frames(&ctx, root, &meta).expect("frames");
    let mut output = PartOutput::new(CodecFormat::F3d);
    decode_stream(&ctx, root, "Design1/BulkStream.dat", &frames, &mut output)
        .expect("partial decode");
    let (_, native, fidelity, losses) = output.into_parts(&ctx).expect("closure").into_parts();
    assert_eq!(losses.len(), 2);
    assert!(losses
        .iter()
        .all(|loss| loss.code.local_code() == "record.unreadable"));
    let namespace = native.namespace("f3d").expect("native");
    let states = namespace
        .arena_as::<DesignParameterState>("design_parameter_states")
        .expect("states");
    let companions = namespace
        .arena_as::<DesignParameterStateCompanion>("design_parameter_state_companions")
        .expect("companions");
    assert_eq!(
        states
            .iter()
            .map(|state| state.record_index)
            .collect::<Vec<_>>(),
        [503]
    );
    assert_eq!(
        companions
            .iter()
            .map(|companion| companion.record_index)
            .collect::<Vec<_>>(),
        [804]
    );
    let given_up = fidelity
        .parts()
        .values()
        .filter(|part| {
            matches!(
                part.state(),
                cadmpeg_ir::source_fidelity::parts::PartState::GivenUp { .. }
            )
        })
        .collect::<Vec<_>>();
    assert_eq!(given_up.len(), 2);
    let captures = ctx.take_record_captures().expect("captures");
    assert!(captures
        .iter()
        .all(|capture| capture.offset() == 0 || capture.offset() >= 126));
    assert_eq!(fidelity.retained_records().len(), 2);
    let mut native = native;
    let mut records = native
        .namespace_mut("f3d")
        .arenas_mut()
        .remove("unknowns")
        .expect("given-up records");
    let second = records.split_off(1);
    let expected = second
        .iter()
        .chain(&records)
        .map(|record| record.id().to_owned())
        .collect::<Vec<_>>();
    let original_buffer = second.as_ptr();
    let mut document = cadmpeg_ir::CadIr::empty();
    install_unknowns(&ctx, &mut document, second).expect("move first arena");
    assert_eq!(
        document
            .native
            .namespace("f3d")
            .expect("namespace")
            .arenas()["unknowns"]
            .as_ptr(),
        original_buffer
    );
    // The part helper already charged these records; moving them charges
    // no retained bytes again (#215 §4.5, `max_retained_bytes`).
    let available = ctx.retained_bytes_available();
    install_unknowns(&ctx, &mut document, records).expect("compose owned peer");
    assert_eq!(ctx.retained_bytes_available(), available);
    assert_eq!(
        document
            .native
            .namespace("f3d")
            .expect("namespace")
            .arenas()["unknowns"]
            .iter()
            .map(|record| record.id().to_owned())
            .collect::<Vec<_>>(),
        expected
    );
}

#[test]
fn parameter_state_source_header_flip_is_part_local_and_foreign_source_is_refused() {
    let (mut bytes, meta) = fixture(true);
    bytes[28] ^= 1;
    let arena = DecodeArena::new();
    let (ctx, root) =
        DecodeContext::from_root_bytes(&bytes, &arena, &DecodePolicy::default()).expect("context");
    let root = ctx.name_stream(root, "file").expect("name");
    let frames = supported_frames(&ctx, root, &meta).expect("frames");
    let mut output = PartOutput::new(CodecFormat::F3d);
    decode_stream(&ctx, root, "Design1/BulkStream.dat", &frames, &mut output)
        .expect("partial decode");
    assert_eq!(output.losses().len(), 2);
    let other_arena = DecodeArena::new();
    let (other, _) = DecodeContext::from_root_bytes(&bytes, &other_arena, &DecodePolicy::default())
        .expect("other context");
    let mut foreign_output = PartOutput::new(CodecFormat::F3d);
    assert!(decode_stream(
        &other,
        root,
        "Design1/BulkStream.dat",
        &frames,
        &mut foreign_output
    )
    .is_err());
    assert!(foreign_output.native().namespace("f3d").is_none());
}

#[test]
fn parameter_state_session_budget_refusal_escapes_without_a_given_up_substitute() {
    let (bytes, meta) = fixture(false);
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::default();
    policy.limits.max_entities = 0;
    let (ctx, root) = DecodeContext::from_root_bytes(&bytes, &arena, &policy).expect("context");
    let root = ctx.name_stream(root, "file").expect("name");
    let frames = supported_frames(&ctx, root, &meta).expect("frames");
    let mut output = PartOutput::new(CodecFormat::F3d);
    let result = decode_stream(&ctx, root, "Design1/BulkStream.dat", &frames, &mut output);
    assert!(matches!(&result, Err(CodecError::ResourceLimit(_))));
    let refusal = match result {
        Err(CodecError::ResourceLimit(limit)) => limit,
        other => panic!("Session entity refusal expected, got {other:?}"),
    };
    assert_eq!(refusal.scope, cadmpeg_core::decode::LimitScope::Session);
    assert_eq!(
        refusal.dimension,
        cadmpeg_core::decode::ResourceDimension::Entities
    );
    assert_eq!(refusal.limit, 0);
    assert_eq!(refusal.used, 0);
    assert_eq!(refusal.additional, 1);
    assert!(output.losses().is_empty());
    assert!(output.native().namespace("f3d").is_none());
    assert_eq!(output.model().entity_count(), 0);
    assert!(output.fidelity().parts().is_empty());
    assert!(output.fidelity().record_layout().is_empty());
    assert!(output.fidelity().retained_records().is_empty());
    assert!(matches!(
        ctx.take_record_captures(),
        Err(CodecError::ResourceLimit(limit)) if limit == refusal
    ));
    assert!(matches!(
        ctx.finish_session(),
        Err(CodecError::ResourceLimit(limit)) if limit == refusal
    ));
}

/// Parse the typed parameter frame `view` inside its own record scope.
fn captured(
    ctx: &DecodeContext<'_>,
    view: View<'_>,
    offset: usize,
    pointers: &mut BTreeMap<String, BTreeMap<String, u64>>,
) -> crate::records::parameters::DesignParameter {
    match super::super::parameters::source_layout::capture_record(
        ctx,
        view,
        "Design1/BulkStream.dat",
        offset,
        super::super::parameters::source_layout::Expected {
            entity_id: None,
            emitted: &std::collections::HashSet::new(),
        },
        pointers,
    )
    .expect("parameter record scope")
    {
        super::super::parameters::source_layout::Captured::Parameter {
            parameter,
            state_target,
        } => {
            assert_eq!(state_target, Some(401));
            parameter
        }
        _ => panic!("expected a typed parameter"),
    }
}

/// Source range of the 164-byte typed compact document parameter at `offset`.
///
/// The f3d.md typed compact grammar consults every byte of the frame: the
/// indexed header (0..11), the fifteen-byte zero run (11..26), the u32 source
/// ordinal (26..30), the null owner reference (30), the counted expression and
/// its five-byte trailer, the counted source kind, unit and name, the f64
/// scalar, and the 52-byte typed tail reference that ends the frame.
fn typed_frame_reads(offset: usize) -> std::ops::Range<u64> {
    u64_from_index(offset)..u64_from_index(offset + 164)
}

#[test]
fn compact_parameter_keeps_its_actual_tail_target_through_native_serde() {
    let (mut bytes, meta) = fixture(false);
    let parameter_start = bytes.len();
    let mut parameter = indexed("259", 905);
    parameter.extend_from_slice(&[0; 15]);
    parameter.extend_from_slice(&13_u32.to_le_bytes());
    parameter.push(0);
    crate::test_support::lp_utf16(&mut parameter, "12 mm");
    parameter.extend_from_slice(&[0, 0, 0, 0, 1]);
    crate::test_support::lp_utf16(&mut parameter, "User Parameter");
    crate::test_support::lp_utf16(&mut parameter, "mm");
    crate::test_support::lp_utf16(&mut parameter, "Width");
    parameter.extend_from_slice(&1.2_f64.to_le_bytes());
    let tail_start = parameter.len();
    parameter.extend_from_slice(&[0, 1]);
    parameter.extend_from_slice(&401_u64.to_le_bytes());
    parameter.extend_from_slice(&36_u32.to_le_bytes());
    parameter.extend_from_slice(STATE_GUID.as_bytes());
    parameter.extend_from_slice(&[0, 0]);
    bytes.extend_from_slice(&parameter);
    let arena = DecodeArena::new();
    let (ctx, root) =
        DecodeContext::from_root_bytes(&bytes, &arena, &DecodePolicy::default()).expect("context");
    let root = ctx.name_stream(root, "file").expect("source");
    let source = root.child(0, parameter_start).expect("state source");
    let frames = supported_frames(&ctx, source, &meta).expect("frames");
    let mut output = PartOutput::new(CodecFormat::F3d);
    decode_stream(&ctx, source, "Design1/BulkStream.dat", &frames, &mut output).expect("states");
    let (_, native, _, losses) = output.into_parts(&ctx).expect("closure").into_parts();
    assert!(losses.is_empty());
    let states = native
        .namespace("f3d")
        .expect("namespace")
        .arena_as::<DesignParameterState>("design_parameter_states")
        .expect("states");
    let view = root
        .child(parameter_start, bytes.len())
        .expect("parameter extent");
    let mut parsed = captured(&ctx, view, parameter_start, &mut BTreeMap::new());
    let state = linked_state("Design1/BulkStream.dat", 401, &frames, &states).expect("state link");
    parsed.set_parameter_state(state);
    let wire = serde_json::to_value(&parsed).expect("wire");
    assert_eq!(wire["parameter_state"], states[0].id);
    assert_eq!(wire["record_index"], 905);
    assert_eq!(wire["expression"], "12 mm");
    let restored: crate::records::parameters::DesignParameter =
        serde_json::from_value(wire.clone()).expect("strict native hydration");
    assert_eq!(serde_json::to_value(restored).expect("restored wire"), wire);
    let captures = ctx.take_record_captures().expect("captures");
    let capture = captures
        .iter()
        .find(|capture| capture.id() == parsed.id)
        .expect("parameter capture");
    assert_eq!(capture.offset(), u64_from_index(parameter_start));
    assert_eq!(tail_start, 112);
    assert_eq!(
        capture.reads()["file"],
        [typed_frame_reads(parameter_start)]
    );
}

#[test]
fn captured_parameter_byte_offset_moves_only_to_its_actual_layout_and_occurrence() {
    let (mut bytes, meta) = fixture(false);
    let parameter_start = bytes.len();
    let mut parameter = indexed("259", 905);
    parameter.extend_from_slice(&[0; 15]);
    parameter.extend_from_slice(&13_u32.to_le_bytes());
    parameter.push(0);
    crate::test_support::lp_utf16(&mut parameter, "12 mm");
    parameter.extend_from_slice(&[0, 0, 0, 0, 1]);
    crate::test_support::lp_utf16(&mut parameter, "User Parameter");
    crate::test_support::lp_utf16(&mut parameter, "mm");
    crate::test_support::lp_utf16(&mut parameter, "Width");
    parameter.extend_from_slice(&1.2_f64.to_le_bytes());
    let tail_start = parameter.len();
    parameter.extend_from_slice(&[0, 1]);
    parameter.extend_from_slice(&401_u64.to_le_bytes());
    parameter.extend_from_slice(&36_u32.to_le_bytes());
    parameter.extend_from_slice(STATE_GUID.as_bytes());
    parameter.extend_from_slice(&[0, 0]);
    bytes.extend_from_slice(&parameter);
    let mut enclosing = b"prefix!".to_vec();
    enclosing.extend_from_slice(&bytes);
    enclosing.extend_from_slice(b"suffix");
    let arena = DecodeArena::new();
    let (ctx, file) = DecodeContext::from_root_bytes(&enclosing, &arena, &DecodePolicy::default())
        .expect("context");
    let root = ctx
        .register_named_slice(
            file,
            cadmpeg_core::decode::ByteRange {
                start: 7,
                end: u64::try_from(7 + bytes.len()).expect("member endpoint fits u64"),
            },
            "Design1/BulkStream.dat",
        )
        .expect("actual nonzero member");
    let source = root.child(0, parameter_start).expect("state source");
    let frames = supported_frames(&ctx, source, &meta).expect("frames");
    let mut output = PartOutput::new(CodecFormat::F3d);
    decode_stream(&ctx, source, "Design1/BulkStream.dat", &frames, &mut output).expect("states");
    let (_, native, _, losses) = output.into_parts(&ctx).expect("closure").into_parts();
    assert!(losses.is_empty());
    let states = native
        .namespace("f3d")
        .expect("namespace")
        .arena_as::<DesignParameterState>("design_parameter_states")
        .expect("states");
    let view = root
        .child(parameter_start, bytes.len())
        .expect("parameter extent");
    let mut drafts = BTreeMap::new();
    let (mut parsed, batch) = ctx
        .record_batch(|| Ok(captured(&ctx, view, parameter_start, &mut drafts)))
        .expect("actual parameter source callback");
    let state = linked_state("Design1/BulkStream.dat", 401, &frames, &states).expect("state link");
    parsed.set_parameter_state(state);
    let wire = serde_json::to_value(&parsed).expect("native scalar content");
    assert_eq!(wire["expression"], "12 mm");
    assert_eq!(wire["name"], "Width");
    assert_eq!(wire["evaluated_value"], 1.2);
    let native = crate::native::F3dNative {
        design_parameters: vec![parsed.clone()],
        ..Default::default()
    };
    assert_eq!(tail_start, 112);
    assert_eq!(parameter.len(), 164);
    let expected_pointers = BTreeMap::from([
        ("byte_offset".to_owned(), u64_from_index(parameter_start)),
        (
            "expression_offset".to_owned(),
            u64_from_index(parameter_start + 35),
        ),
        (
            "source_kind_offset".to_owned(),
            u64_from_index(parameter_start + 54),
        ),
        (
            "unit_offset".to_owned(),
            u64_from_index(parameter_start + 86),
        ),
        (
            "name_offset".to_owned(),
            u64_from_index(parameter_start + 94),
        ),
        (
            "evaluated_value_offset".to_owned(),
            u64_from_index(parameter_start + 104),
        ),
    ]);
    let mut wrong_offset = BTreeMap::from([(
        parsed.id.clone(),
        BTreeMap::from([(
            "byte_offset".to_owned(),
            u64_from_index(parameter_start + 1),
        )]),
    )]);
    let original_wrong_offset = wrong_offset.clone();
    assert!(super::super::parameters::source_layout::capture_record(
        &ctx,
        view,
        "Design1/BulkStream.dat",
        parameter_start,
        super::super::parameters::source_layout::Expected {
            entity_id: None,
            emitted: &std::collections::HashSet::new(),
        },
        &mut wrong_offset,
    )
    .is_err());
    assert_eq!(wrong_offset, original_wrong_offset);
    assert_eq!(drafts[&parsed.id], expected_pointers);
    let mut fidelity = native
        .retained_fidelity()
        .expect("layout-only source fields");
    assert!(fidelity.metadata.is_empty());
    let mut pending = crate::decode::source_layout::PendingLayouts::new(batch, drafts);
    pending
        .rescope(&ctx, "one")
        .expect("actual occurrence mapping");
    crate::decode::source_layout::install(&ctx, &mut fidelity, vec![pending])
        .expect("committed capture handoff");
    let target_id =
        crate::f3z::merge::rescope_record_id(&parsed.id, "one").expect("native identity mapping");
    assert!(!fidelity.record_layout().contains_key(&parsed.id));
    let layout = &fidelity.record_layout()[&target_id];
    assert_eq!(layout.stream(), "f3d:xref/one/Design1/BulkStream.dat");
    assert_eq!(
        (layout.offset(), layout.byte_len()),
        (
            u64_from_index(parameter_start),
            u64_from_index(parameter.len())
        )
    );
    assert_eq!(layout.pointers(), &expected_pointers);
    // The member starts at byte 7 of the root, so each member read is also
    // the root range seven bytes later.
    assert_eq!(
        layout.reads(),
        &BTreeMap::from([
            (
                "f3d:xref/one/Design1/BulkStream.dat".to_owned(),
                vec![typed_frame_reads(parameter_start)]
            ),
            (
                "f3d:xref/one/file".to_owned(),
                vec![typed_frame_reads(parameter_start + 7)]
            ),
        ])
    );
    assert_eq!(
        serde_json::to_value(parsed).expect("unchanged native scalar content"),
        wire
    );
}

#[test]
fn companion_state_record_index_follows_the_physical_reference_counterfactual() {
    let pairs = [(0x1122_3344_u64, 702), (0x5566_7788, 804)];
    let (bytes, meta) = fixture_with_indices(&pairs);
    let arena = DecodeArena::new();
    let (ctx, source) =
        DecodeContext::from_root_bytes(&bytes, &arena, &DecodePolicy::default()).expect("context");
    let frames = supported_frames(&ctx, source, &meta).expect("frames");
    let mut output = PartOutput::new(CodecFormat::F3d);
    decode_stream(&ctx, source, "Design1/BulkStream.dat", &frames, &mut output).expect("decode");
    let (_, native, _, losses) = output.into_parts(&ctx).expect("closure").into_parts();
    assert!(losses.is_empty());
    let namespace = native.namespace("f3d").expect("native");
    let states = namespace
        .arena_as::<DesignParameterState>("design_parameter_states")
        .expect("states");
    let companions = namespace
        .arena_as::<DesignParameterStateCompanion>("design_parameter_state_companions")
        .expect("companions");
    assert_eq!(states.len(), 2);
    assert_eq!(companions.len(), 2);
    for ((state, companion), (state_index, companion_index)) in
        states.iter().zip(&companions).zip(pairs)
    {
        assert_eq!(u64::from(state.record_index), state_index);
        assert_eq!(u64::from(companion.record_index), companion_index);
        assert_eq!(companion.state_record_index, state_index);
        assert_eq!(state.companion, companion.id);
        let wire = serde_json::to_value(companion).expect("companion wire");
        assert_eq!(wire["state_record_index"], state_index);
        assert!(wire.get("state").is_none());
        assert_eq!(
            serde_json::from_value::<DesignParameterStateCompanion>(wire)
                .expect("hydrate companion"),
            *companion
        );
    }
    assert_ne!(
        companions[0].state_record_index,
        companions[1].state_record_index
    );
}
