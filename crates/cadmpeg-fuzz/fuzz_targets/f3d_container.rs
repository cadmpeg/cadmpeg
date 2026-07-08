// SPDX-License-Identifier: Apache-2.0
//! Fuzz target for the `.f3d` container parser.
//!
//! Feeds arbitrary bytes through detection, container inspection, and decode.
//! The contract under test is *robustness*: no input may panic. Malformed input
//! must surface as a `CodecError`, never an abort. Run with:
//! `cargo +nightly fuzz run f3d_container`.
#![no_main]

use std::io::Cursor;

use cadmpeg_codec_f3d::F3dCodec;
use cadmpeg_ir::codec::{Codec, DecodeOptions};
use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    let codec = F3dCodec;

    // Detection works on a raw prefix and must never panic.
    let _ = codec.detect(data);

    // Inspection and decode read from a seekable cursor; errors are fine, panics
    // are not.
    let mut inspect_cur = Cursor::new(data);
    let _ = codec.inspect(&mut inspect_cur);

    let mut decode_cur = Cursor::new(data);
    let _ = codec.decode(&mut decode_cur, &DecodeOptions::default());
});
