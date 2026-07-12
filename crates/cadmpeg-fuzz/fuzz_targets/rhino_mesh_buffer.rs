// SPDX-License-Identifier: Apache-2.0
//! Fuzzes Rhino stored and zlib-compressed mesh buffers.

#![no_main]

use cadmpeg_codec_rhino::fuzzing;
use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| fuzzing::mesh_buffer(data));
