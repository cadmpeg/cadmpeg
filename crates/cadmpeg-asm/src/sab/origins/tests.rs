// SPDX-License-Identifier: Apache-2.0
use super::{frame, frame_history, TokenRole};
use crate::kernel_header::RefWidth;
use crate::stream_error::StreamFailure;
use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension, View};

fn integer(value: i64, width: RefWidth) -> Vec<u8> {
    match width {
        RefWidth::Four => i32::try_from(value).unwrap().to_le_bytes().to_vec(),
        RefWidth::Eight => value.to_le_bytes().to_vec(),
    }
}

#[test]
fn every_binary_token_origin_has_its_authored_extent_and_value_payload() {
    for width in [RefWidth::Four, RefWidth::Eight] {
        let mut cases = vec![
            (vec![0x02, 0x7f], 1),
            (vec![0x03, 0xfe, 0xff], 1),
            ([vec![0x04], integer(-7, width)].concat(), 1),
            ([vec![0x05], 1.5_f32.to_le_bytes().to_vec()].concat(), 1),
            ([vec![0x06], 2.5_f64.to_le_bytes().to_vec()].concat(), 1),
            (vec![0x07, 2, 0xc3, 0xa9], 2),
            (vec![0x08, 0, 0], 3),
            (
                [vec![0x09], integer(1, width), vec![b'a']].concat(),
                1 + width.bytes(),
            ),
            ([vec![0x12], integer(0, width)].concat(), 1 + width.bytes()),
            (vec![0x0a], 0),
            (vec![0x0b], 0),
            ([vec![0x0c], integer(-1, width)].concat(), 1),
            (vec![0x0d, 1, b'q'], 2),
            (vec![0x0e, 1, b'p'], 2),
        ];
        for tag in [0x13, 0x14] {
            cases.push((
                [
                    vec![tag],
                    [1.0_f64, 2.0, 3.0]
                        .into_iter()
                        .flat_map(f64::to_le_bytes)
                        .collect(),
                ]
                .concat(),
                1,
            ));
        }
        cases.push(([vec![0x15], integer(3, width)].concat(), 1));
        cases.push((
            [
                vec![0x16],
                [4.0_f64, 5.0]
                    .into_iter()
                    .flat_map(f64::to_le_bytes)
                    .collect(),
            ]
            .concat(),
            1,
        ));
        cases.push(([vec![0x17], (-9_i64).to_le_bytes().to_vec()].concat(), 1));
        let mut bytes = vec![0xfa, 0xfb, 0x0d, 1, b'x'];
        for (value, _) in &cases {
            bytes.extend_from_slice(value);
        }
        bytes.push(0x11);
        bytes.push(0xfc);
        let arena = DecodeArena::new();
        let (ctx, root) =
            DecodeContext::from_root_bytes(&bytes, &arena, &DecodePolicy::service()).unwrap();
        let source = root.child(2, bytes.len() - 1).unwrap();
        let records = frame(&ctx, source, width).unwrap();
        assert_eq!(records.len(), 1);
        assert_eq!(records[0].index(), 0);
        assert_eq!(records[0].extent(&ctx).unwrap().start(), 2);
        assert_eq!(records[0].extent(&ctx).unwrap().end(), bytes.len() - 1);
        let payload: Vec<_> = records[0].payload_tokens().collect();
        assert_eq!(payload.len(), cases.len());
        let mut offset = 5;
        for (origin, (authored, prefix)) in payload.into_iter().zip(&cases) {
            let extent = origin.extent(&ctx).unwrap();
            let value = origin.payload(&ctx).unwrap().unwrap();
            assert_eq!(extent.start(), offset);
            assert_eq!(extent.end(), offset + authored.len());
            assert_eq!(extent.window(), authored);
            assert_eq!(value.start(), offset + prefix);
            assert_eq!(value.end(), offset + authored.len());
            assert_eq!(value.window(), &authored[*prefix..]);
            offset += authored.len();
        }
        assert_eq!(records[0].tokens()[0].role(), TokenRole::RecordName);
        let last = records[0].tokens().last().unwrap();
        assert_eq!(last.role(), TokenRole::RecordTerminator);
        assert!(last.payload(&ctx).unwrap().is_none());
        let old = crate::sab::frame(&ctx, &bytes, 2, bytes.len() - 1, width, None).unwrap();
        assert_eq!(old[0].index, records[0].index());
        assert_eq!(old[0].tokens.len(), records[0].payload_tokens().count());
        ctx.finish_session().unwrap();
    }
}

#[test]
fn origins_reject_detached_and_foreign_source_domains_even_for_identical_bytes() {
    let bytes = [0x0d, 1, b'x', 0x0a, 0x11];
    let a = DecodeArena::new();
    let b = DecodeArena::new();
    let (ctx, source) =
        DecodeContext::from_root_bytes(&bytes, &a, &DecodePolicy::service()).unwrap();
    let (foreign, other_source) =
        DecodeContext::from_root_bytes(&bytes, &b, &DecodePolicy::service()).unwrap();
    assert!(frame(&ctx, View::over_retained(&bytes), RefWidth::Eight).is_err());
    assert!(frame(&ctx, other_source, RefWidth::Eight).is_err());
    let rows = frame(&ctx, source, RefWidth::Eight).unwrap();
    assert!(rows[0].extent(&foreign).is_err());
    assert!(rows[0].tokens()[1].payload(&foreign).is_err());
    assert_eq!(
        rows[0].tokens()[1].payload(&ctx).unwrap().unwrap().window(),
        [0x0a]
    );
    assert!(ctx.resource_refusal().is_none());
    assert!(foreign.resource_refusal().is_none());
}

#[test]
fn origin_storage_is_working_storage_and_preserves_the_real_callers_first_refusal() {
    let bytes = [0x0d, 1, b'x', 0x0a, 0x11];
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_retained_bytes = 3;
    policy.limits.max_entities = 0;
    let (ctx, source) = DecodeContext::from_root_bytes(&bytes, &arena, &policy).unwrap();
    ctx.retain_bytes(3, "existing retained record").unwrap();
    assert_eq!(ctx.retained_bytes_available(), 0);
    let rows = frame(&ctx, source, RefWidth::Eight).unwrap();
    assert_eq!(rows.len(), 1);
    assert_eq!(ctx.retained_bytes_available(), 0);
    assert!(ctx.resource_refusal().is_none());
    assert!(ctx.take_record_captures().unwrap().is_empty());
    let error = ctx
        .charge_entities(1, "real caller entity refusal")
        .unwrap_err();
    let first = ctx.resource_refusal().unwrap();
    assert_eq!(first.dimension, ResourceDimension::Entities);
    assert!(matches!(error, cadmpeg_core::CodecError::ResourceLimit(actual) if actual == first));
    assert!(
        matches!(frame(&ctx, source, RefWidth::Eight), Err(StreamFailure::Resource(actual)) if actual == first)
    );
    assert_eq!(ctx.resource_refusal(), Some(first));
    assert!(
        matches!(ctx.finish_session(), Err(cadmpeg_core::CodecError::ResourceLimit(actual)) if actual == first)
    );
}

#[test]
fn metadata_creates_no_semantic_reads_and_explicit_value_reads_stay_exact() {
    let mut bytes = vec![0xfa, 0xfb, 0x0d, 1, b'x', 0x06];
    bytes.extend_from_slice(&1.25_f64.to_le_bytes());
    bytes.push(0x11);
    let arena = DecodeArena::new();
    let (ctx, root) =
        DecodeContext::from_root_bytes(&bytes, &arena, &DecodePolicy::service()).unwrap();
    let source = root.child(2, bytes.len()).unwrap();
    let rows = ctx
        .record("framing-only", source, || {
            frame(&ctx, source, RefWidth::Eight)
                .map_err(|e| e.into_codec_error(&ctx, cadmpeg_core::CodecError::malformed))
        })
        .unwrap();
    let value = rows[0]
        .payload_tokens()
        .next()
        .unwrap()
        .payload(&ctx)
        .unwrap()
        .unwrap();
    ctx.record("one-value", source, || {
        assert_eq!(
            value.read_slice(value.start()..value.end()).unwrap(),
            1.25_f64.to_le_bytes()
        );
        Ok(())
    })
    .unwrap();
    let captures = ctx.take_record_captures().unwrap();
    assert_eq!(captures.len(), 2);
    assert!(captures
        .iter()
        .find(|c| c.id() == "framing-only")
        .unwrap()
        .reads()
        .is_empty());
    let read = captures.iter().find(|c| c.id() == "one-value").unwrap();
    assert_eq!(
        read.reads().values().flatten().cloned().collect::<Vec<_>>(),
        vec![6..14]
    );
    ctx.finish_session().unwrap();
}

#[test]
fn subtype_history_delta_and_truncation_boundaries_match_the_real_framer() {
    for width in [RefWidth::Four, RefWidth::Eight] {
        let bytes = [0x0d, 1, b'x', 0x0f, 0x0d, 1, b'n', 0x0a, 0x10, 0x11];
        let arena = DecodeArena::new();
        let (ctx, root) =
            DecodeContext::from_root_bytes(&bytes, &arena, &DecodePolicy::service()).unwrap();
        let rows = frame(&ctx, root, width).unwrap();
        assert_eq!(rows[0].payload_tokens().count(), 4);
        assert_eq!(rows[0].tokens()[1].role(), TokenRole::SubtypeDelimiter);
        assert!(rows[0].tokens()[1].payload(&ctx).unwrap().is_none());
        let unterminated = root.child(0, bytes.len() - 1).unwrap();
        assert!(frame(&ctx, unterminated, width).is_err());
        assert_eq!(
            frame_history(&ctx, unterminated, width).unwrap()[0]
                .extent(&ctx)
                .unwrap()
                .end(),
            bytes.len() - 1
        );
        let delta = [
            0x0d, 1, b'x', 0x11, 0x0d, 11, b'd', b'e', b'l', b't', b'a', b'_', b's', b't', b'a',
            b't', b'e', 0xff,
        ];
        let arena2 = DecodeArena::new();
        let (ctx2, source2) =
            DecodeContext::from_root_bytes(&delta, &arena2, &DecodePolicy::service()).unwrap();
        assert_eq!(frame(&ctx2, source2, width).unwrap().len(), 1);
        let old = crate::sab::frame(&ctx2, &delta, 0, delta.len(), width, None).unwrap();
        assert_eq!(old.len(), 1);
    }
}
