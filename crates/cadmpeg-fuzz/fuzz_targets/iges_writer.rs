// SPDX-License-Identifier: Apache-2.0
//! Fuzzes IGES semantic planning, versioned synthesis, rejection, and replay.

#![no_main]

use std::io::Cursor;

use cadmpeg_codec_iges::{IgesCodec, IgesVersion};
use cadmpeg_ir::codec::{Codec, DecodeOptions};
use cadmpeg_ir::codec::write::{EncodeInput, Encoder, TargetRequest};
use cadmpeg_ir::ids::UnknownId;
use cadmpeg_ir::report::WritePath;
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
    let encoder = IgesCodec;

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
            .plan(
                EncodeInput::new(&ir, None),
                TargetRequest::Explicit(version.descriptor().id.as_str()),
            )
            .is_err());
        return;
    }

        let Ok(plan) = encoder.plan(
            EncodeInput::new(&ir, None),
            TargetRequest::Explicit(version.descriptor().id.as_str()),
        ) else {
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
    let mut decoded = codec
        .decode(&mut decode, &DecodeOptions::default())
        .expect("writer output must decode");
    assert!(cadmpeg_ir::validate_neutral(decoded.ir(), decoded.report().losses.clone()).is_ok());

    if control & 0x40 != 0 {
        let mut decoded_ir = decoded.ir_mut();
        let source = decoded_ir
            .source
            .as_mut()
            .expect("IGES decode supplies source metadata");
        source
            .attributes
            .insert("iges_fuzz_edit".into(), "edited".into());
    }

    let replay = encoder
            .plan(
                EncodeInput::new(decoded.ir(), Some(decoded.source_fidelity())),
                TargetRequest::Explicit(version.descriptor().id.as_str()),
            )
        .expect("writer output must plan after the optional source edit");
    if control & 0x40 == 0 {
        assert_eq!(replay.report().write_path, WritePath::VerbatimReplay);
    } else {
        assert_ne!(replay.report().write_path, WritePath::VerbatimReplay);
    }
    let mut replayed = Vec::new();
    replay
        .write_to(&mut replayed)
        .expect("writer output must serialize after the optional source edit");
    if control & 0x40 == 0 {
        assert_eq!(replayed, encoded);
    } else {
        let mut edited_decode = Cursor::new(replayed);
        let edited = codec
            .decode(&mut edited_decode, &DecodeOptions::default())
            .expect("edited writer output must decode");
        assert!(
            cadmpeg_ir::validate_neutral(edited.ir(), edited.report().losses.clone()).is_ok()
        );
    }
});
