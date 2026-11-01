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
    let source = root.child(7, 7 + bytes.len()).expect("source window");
    let frames = supported_frames(&ctx, source, &meta).expect("primary frames");
    let mut output = PartOutput::new(CodecFormat::F3d);
    decode_stream(&ctx, source, "Design1/BulkStream.dat", &frames, &mut output).expect("decode");
    let (_, native, fidelity, losses) = output.into_parts();
    assert!(losses.is_empty());
    assert!(fidelity.retained_records().is_empty());
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
    let headers = namespace
        .arena_as::<DesignParameterDocumentHeader>("design_parameter_document_headers")
        .expect("headers");
    assert_eq!(headers[0].record_index, 93);
    assert_eq!(states[0].document_root, headers[0].id);
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
    let frames = supported_frames(&ctx, root, &meta).expect("frames");
    let mut output = PartOutput::new(CodecFormat::F3d);
    decode_stream(&ctx, root, "Design1/BulkStream.dat", &frames, &mut output)
        .expect("partial decode");
    let (_, native, fidelity, losses) = output.into_parts();
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
    assert_eq!(fidelity.retained_records().len(), 2);
    // #215 rule 1: each given-up part keeps one unknowns home.
    let unknowns = namespace
        .arenas()
        .get("unknowns")
        .expect("one unknowns home");
    assert_eq!(unknowns.len(), 2);
    let mut stubbed: Vec<_> = unknowns
        .iter()
        .map(cadmpeg_ir::native::NativeRecord::id)
        .collect();
    stubbed.sort_unstable();
    let mut retained: Vec<_> = fidelity
        .retained_records()
        .keys()
        .map(cadmpeg_ir::ids::UnknownId::as_str)
        .collect();
    retained.sort_unstable();
    assert_eq!(stubbed, retained);
}

#[test]
fn parameter_state_source_header_flip_is_part_local_and_foreign_source_is_refused() {
    let (mut bytes, meta) = fixture(true);
    bytes[28] ^= 1;
    let arena = DecodeArena::new();
    let (ctx, root) =
        DecodeContext::from_root_bytes(&bytes, &arena, &DecodePolicy::default()).expect("context");
    let frames = supported_frames(&ctx, root, &meta).expect("frames");
    let mut output = PartOutput::new(CodecFormat::F3d);
    decode_stream(&ctx, root, "Design1/BulkStream.dat", &frames, &mut output)
        .expect("partial decode");
    assert_eq!(output.losses().len(), 2);
}

#[test]
fn parameter_state_session_budget_refusal_escapes_without_a_given_up_substitute() {
    let (bytes, meta) = fixture(false);
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::default();
    policy.limits.max_entities = 0;
    let (ctx, root) = DecodeContext::from_root_bytes(&bytes, &arena, &policy).expect("context");
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
    assert!(output.fidelity().retained_records().is_empty());
    assert!(matches!(
        ctx.finish_session(),
        Err(CodecError::ResourceLimit(limit)) if limit == refusal
    ));
}

/// Parse the typed parameter frame `view`.
fn parsed_parameter(view: View<'_>, offset: usize) -> crate::records::parameters::DesignParameter {
    match super::super::parameters::parse_frame(
        view,
        "Design1/BulkStream.dat",
        offset,
        super::super::parameters::Expected {
            entity_id: None,
            emitted: &std::collections::HashSet::new(),
        },
    )
    .expect("parameter frame")
    {
        super::super::parameters::ParsedFrame::Parameter {
            parameter,
            state_target,
        } => {
            assert_eq!(state_target, Some(401));
            *parameter
        }
        _ => panic!("expected a typed parameter"),
    }
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
    let source = root.child(0, parameter_start).expect("state source");
    let frames = supported_frames(&ctx, source, &meta).expect("frames");
    let mut output = PartOutput::new(CodecFormat::F3d);
    decode_stream(&ctx, source, "Design1/BulkStream.dat", &frames, &mut output).expect("states");
    let (_, native, _, losses) = output.into_parts();
    assert!(losses.is_empty());
    let states = native
        .namespace("f3d")
        .expect("namespace")
        .arena_as::<DesignParameterState>("design_parameter_states")
        .expect("states");
    let view = root
        .child(parameter_start, bytes.len())
        .expect("parameter extent");
    let mut parsed = parsed_parameter(view, parameter_start);
    let state = linked_state("Design1/BulkStream.dat", 401, &frames, &states).expect("state link");
    parsed.set_parameter_state(state);
    let wire = serde_json::to_value(&parsed).expect("wire");
    assert_eq!(wire["parameter_state"], states[0].id);
    assert_eq!(wire["record_index"], 905);
    assert_eq!(wire["expression"], "12 mm");
    let restored: crate::records::parameters::DesignParameter =
        serde_json::from_value(wire.clone()).expect("strict native hydration");
    assert_eq!(serde_json::to_value(restored).expect("restored wire"), wire);
    assert_eq!(tail_start, 112);
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
    let (_, native, _, losses) = output.into_parts();
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
