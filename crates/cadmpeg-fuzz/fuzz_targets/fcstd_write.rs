// SPDX-License-Identifier: Apache-2.0
//! Fuzz retained semantic edits and FCStd write/read round trips.
#![no_main]

use std::io::Cursor;

use cadmpeg_codec_freecad::{FcstdCodec, FcstdPropertyOwner};
use cadmpeg_ir::codec::{CodecEntry, DecodeOptions, Encoder};
use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    let bounded = &data[..data.len().min(1 << 20)];
    let Ok(decoded) = FcstdCodec.decode(
        &mut Cursor::new(bounded),
        &DecodeOptions::default(),
    ) else {
        return;
    };
    let mut ir = decoded.ir;
    if let Some(discriminator) = data.first() {
        let _ = FcstdCodec.set_property_value_attribute(
            &mut ir,
            FcstdPropertyOwner::Document,
            "Label",
            0,
            "value",
            format!("fuzz-{discriminator}"),
        );
    }
    let mut output = Vec::new();
    if FcstdCodec.encode(&ir, &mut output).is_ok() {
        let _ = FcstdCodec.decode(
            &mut Cursor::new(output),
            &DecodeOptions::default(),
        );
    }
});
