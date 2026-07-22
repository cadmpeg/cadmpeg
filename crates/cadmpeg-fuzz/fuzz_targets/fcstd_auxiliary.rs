// SPDX-License-Identifier: Apache-2.0
//! Fuzz embedded and application-owned auxiliary payload retention.
#![no_main]

mod fcstd_support;
use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    let document = br#"<Document SchemaVersion="4" FileVersion="1"><Objects Count="1"><Object type="App::FeaturePython" name="Extension" id="1"/></Objects><ObjectData Count="1"><Object name="Extension"><Properties Count="1"><Property name="Payload" type="App::PropertyFileIncluded"><FileIncluded file="payload.bin"/></Property></Properties></Object></ObjectData></Document>"#;
    fcstd_support::decode(fcstd_support::archive(&[
        ("Document.xml", document),
        ("payload.bin", fcstd_support::bounded(data)),
    ]));
});
