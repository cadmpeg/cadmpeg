// SPDX-License-Identifier: Apache-2.0
//! Fuzz target for SolidWorks Parasolid entity scanning.
//!
//! Drives `cadmpeg_codec_sldprt::fuzzing::entity` to exercise entity facts
//! scanning. Contract: no input may panic.

#![no_main]

use cadmpeg_codec_sldprt::fuzzing::entity;
use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| entity(data));
