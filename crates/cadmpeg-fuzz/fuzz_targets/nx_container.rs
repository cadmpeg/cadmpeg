// SPDX-License-Identifier: Apache-2.0
//! Fuzz target for the Siemens NX `.prt` codec.
//!
//! Feeds arbitrary bytes through `NxCodec::detect`, `inspect`, and `decode`.
//! Contract: no input may panic. Malformed input must surface as `CodecError`.
//!
//! Run: cargo +nightly fuzz run --fuzz-dir crates/cadmpeg-fuzz nx_container

#![no_main]

use cadmpeg_codec_nx::NxCodec;
use cadmpeg_ir::codec::{Codec, DecodeOptions};
use libfuzzer_sys::fuzz_target;
use std::io::Cursor;

fuzz_target!(|data: &[u8]| {
    let codec = NxCodec;

    let _ = codec.detect(data);

    let mut inspect_cur = Cursor::new(data);
    let _ = codec.inspect(&mut inspect_cur);

    let mut decode_cur = Cursor::new(data);
    let _ = codec.decode(&mut decode_cur, &DecodeOptions::default());
});
