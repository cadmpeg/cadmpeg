// SPDX-License-Identifier: Apache-2.0
//! Fuzz FCStd persistence XML and property-value framing.
#![no_main]

mod fcstd_support;
use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    fcstd_support::decode(fcstd_support::archive(&[(
        "Document.xml",
        fcstd_support::bounded(data),
    )]));
});
