// SPDX-License-Identifier: Apache-2.0
//! Fuzzes IGES semantic planning, versioned synthesis, rejection, and replay.

#![no_main]

use std::io::Cursor;

use cadmpeg_codec_iges::{IgesCodec, IgesEncoder, IgesVersion, IgesWriteOptions};
use cadmpeg_ir::codec::{Codec, DecodeOptions, EncodeInput, Encoder};
use cadmpeg_ir::ids::UnknownId;
use cadmpeg_ir::{CadIr, UnknownRecord};
use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    let Some((&first, rest)) = data.split_first() else {
        return;
    };
    let (control, json) = if first == b'{' {
        (0, data)
    } else {
        (first, rest)
    };
    let Ok(text) = std::str::from_utf8(json) else {
        return;
    };
    let Ok(mut ir) = CadIr::from_json(text) else {
        return;
    };
    let version = match control % 3 {
        0 => IgesVersion::V5_1,
        1 => IgesVersion::V5_2,
        _ => IgesVersion::V5_3,
    };
    let encoder = IgesEncoder::new(IgesWriteOptions { version });

    if control & 0x80 != 0 {
        ir.set_native_unknowns_owned(
            "iges",
            vec![UnknownRecord {
                id: UnknownId("iges:fuzz:unsupported#0".into()),
                offset: 0,
                byte_len: 1,
                sha256: cadmpeg_ir::hash::sha256_hex(&[control]),
                data: Some(vec![control]),
                links: Vec::new(),
            }],
        );
        assert!(encoder
            .plan(EncodeInput {
                ir: &ir,
                fidelity: None,
            })
            .is_err());
        return;
    }

    let Ok(plan) = encoder.plan(EncodeInput {
        ir: &ir,
        fidelity: None,
    }) else {
        return;
    };
    let mut encoded = Vec::new();
    if plan.write_to(&mut encoded).is_err() {
        return;
    }

    let codec = IgesCodec;
    let mut inspect = Cursor::new(encoded.as_slice());
    assert!(codec
        .inspect(
            &mut inspect,
            &cadmpeg_core::decode::InspectOptions::default()
        )
        .is_ok());
    let mut decode = Cursor::new(encoded.as_slice());
    let decoded = codec
        .decode(&mut decode, &DecodeOptions::default())
        .expect("writer output must decode");
    assert!(cadmpeg_ir::validate_neutral(&decoded.ir, decoded.report.losses.clone()).is_ok());

    let replay = encoder
        .plan(EncodeInput {
            ir: &decoded.ir,
            fidelity: Some(&decoded.source_fidelity),
        })
        .expect("unchanged writer output must plan for replay");
    let mut replayed = Vec::new();
    replay
        .write_to(&mut replayed)
        .expect("unchanged writer output must replay");
    assert_eq!(replayed, encoded);
});
