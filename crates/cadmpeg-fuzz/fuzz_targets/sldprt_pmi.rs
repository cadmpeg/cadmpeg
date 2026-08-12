// SPDX-License-Identifier: Apache-2.0
//! Fuzz target for SolidWorks PMISemanticDataDB MessagePack parsing.
//! Parse, optional in-place patch, and reparse must agree; no input may panic.

#![no_main]

use cadmpeg_codec_sldprt::fuzz::pmi;
use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| pmi(data));
