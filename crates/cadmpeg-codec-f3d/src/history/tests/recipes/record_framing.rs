// SPDX-License-Identifier: Apache-2.0

#[test]
fn opaque_history_span_retains_the_precise_framing_error() {
    let bytes = [0x33];
    let arena = cadmpeg_core::decode::DecodeArena::new();
    let (ctx, _) = cadmpeg_core::decode::DecodeContext::from_root_bytes(
        &bytes,
        &arena,
        &cadmpeg_core::decode::DecodePolicy::service(),
    )
    .expect("test input is within the service limit");
    let records = crate::history::decode_history_records(
        &ctx,
        &bytes,
        0,
        None,
        "stream",
        "state",
        cadmpeg_asm::kernel_header::RefWidth::Eight,
    )
    .expect("malformed history remains an opaque record");
    let [record] = records.as_slice() else {
        panic!("one opaque record");
    };
    assert_eq!(record.name(), "opaque_history_payload");
    assert!(record
        .framing_error()
        .is_some_and(|error| error.contains("byte 0") && error.contains("0x33")));
}

#[test]
fn wrapped_face_history_record_keeps_dispatch_references_and_bytes() {
    let mut bytes = Vec::new();
    for part in ["End", "of", "ASM", "History"] {
        bytes.extend_from_slice(&[0x0e, u8::try_from(part.len()).unwrap()]);
        bytes.extend_from_slice(part.as_bytes());
    }
    bytes.extend_from_slice(&[0x0d, 7]);
    bytes.extend_from_slice(b"Section");
    bytes.extend_from_slice(&[0x0d, 4]);
    bytes.extend_from_slice(b"face");
    let references = [4i64, -1, 5, 6, 7, -1, 8];
    for reference in references {
        bytes.push(0x0c);
        bytes.extend_from_slice(&reference.to_le_bytes());
    }
    bytes.push(0x11);
    let arena = cadmpeg_core::decode::DecodeArena::new();
    let policy = cadmpeg_core::decode::DecodePolicy::service();
    let (ctx, _) =
        cadmpeg_core::decode::DecodeContext::from_root_bytes(&bytes, &arena, &policy).unwrap();
    let records = crate::history::decode_history_records(
        &ctx,
        &bytes,
        0,
        None,
        "stream",
        "state",
        cadmpeg_asm::kernel_header::RefWidth::Eight,
    )
    .unwrap();
    let [record] = records.as_slice() else {
        panic!("one wrapped face record");
    };
    assert_eq!(record.name(), "face");
    assert_eq!(record.byte_offset, 0);
    assert_eq!(record.raw_bytes, bytes);
    let crate::history_records::AsmHistoryRecordFraming::Framed {
        index,
        entity_references,
        ..
    } = &record.framing
    else {
        panic!("framed face");
    };
    assert_eq!(*index, 0);
    assert_eq!(entity_references, &references);
}
