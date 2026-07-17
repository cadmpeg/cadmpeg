// SPDX-License-Identifier: Apache-2.0
//! Fuzz persistent element-map and string-table side data.
#![no_main]

mod fcstd_support;
use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    let document = br#"<Document SchemaVersion="4" FileVersion="1"><Objects Count="1"><Object type="Part::Feature" name="Shape" id="1"/></Objects><ObjectData Count="1"><Object name="Shape"><Properties Count="1"><Property name="Shape" type="Part::PropertyPartShape"><Part file="Shape.brp" hasher="ElementMap.bin"/></Property></Properties></Object></ObjectData></Document>"#;
    fcstd_support::decode(fcstd_support::archive(&[
        ("Document.xml", document),
        ("Shape.brp", b""),
        ("ElementMap.bin", fcstd_support::bounded(data)),
    ]));
});
