// SPDX-License-Identifier: Apache-2.0
//! Fuzz FCStd GUI document and view-provider persistence.
#![no_main]

mod fcstd_support;
use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    fcstd_support::decode(fcstd_support::archive(&[
        ("Document.xml", fcstd_support::EMPTY_DOCUMENT),
        ("GuiDocument.xml", fcstd_support::bounded(data)),
    ]));
});
