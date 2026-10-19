// SPDX-License-Identifier: Apache-2.0
//! Fuzz typed native edits, synthesis, and guarded FCStd write/read round trips.
#![no_main]

use std::io::Cursor;

use cadmpeg_codec_freecad::{FcstdCodec, FcstdPropertyOwner};
use cadmpeg_ir::codec::write::{target::TargetRequest, EncodeInput, Encoder};
use cadmpeg_ir::codec::{Codec, DecodeOptions};
use cadmpeg_ir::report::export::WritePath;

use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    let bounded = &data[..data.len().min(1 << 20)];
    let Ok(decoded) = FcstdCodec.decode(&mut Cursor::new(bounded), &DecodeOptions::default())
    else {
        return;
    };
    let (mut ir, _, fidelity) = decoded.into_parts();
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
    let retained = data
        .first()
        .is_some_and(|byte| byte & 1 != 0)
        .then_some(&fidelity);
    let Ok(plan) = FcstdCodec.plan(EncodeInput::new(&ir, retained), TargetRequest::Inherit) else {
        return;
    };
    if retained.is_some() && !fidelity.unrepresented.is_empty() {
        assert!(matches!(
            plan.report().write_path(),
            WritePath::Synthesized { .. }
        ));
    }
    let mut output = Vec::new();
    if plan.write_to(&mut output).is_ok() {
        assert!(FcstdCodec
            .decode(&mut Cursor::new(output), &DecodeOptions::default())
            .is_ok());
    }
});
