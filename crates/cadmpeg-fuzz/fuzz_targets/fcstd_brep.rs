// SPDX-License-Identifier: Apache-2.0
//! Fuzz text and binary exact-shape carrier parsing.
#![no_main]

mod fcstd_support;
use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    let document = fcstd_support::shape_document("Shape.brp");
    fcstd_support::decode(fcstd_support::archive(&[
        ("Document.xml", &document),
        ("Shape.brp", fcstd_support::bounded(data)),
    ]));
});
