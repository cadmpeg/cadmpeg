// SPDX-License-Identifier: Apache-2.0
//! A Global value that fails is absent; the file is not refused (#222 C.5).

use cadmpeg_ir::codec::DecodeResult;

use crate::test_support::test_curves_and_surfaces::point_file_with_global;

const GLOBAL_FIELDS: [&str; 26] = [
    "1H,",
    "1H;",
    "7Hproduct",
    "8Hpart.igs",
    "7Hcadmpeg",
    "3H0.1",
    "32",
    "38",
    "6",
    "308",
    "15",
    "0H",
    "1.0",
    "2",
    "2HMM",
    "1",
    "1.0",
    "15H20260714.000000",
    "0.001",
    "1000.0",
    "6Hauthor",
    "3Horg",
    "11",
    "0",
    "0H",
    "0H",
];

/// The only given-up part of a decode, with its one `record.unreadable`.
fn only_given_up(result: &DecodeResult) -> (String, u64, u64) {
    let given_up = crate::test_support::given_up_parts(result.source_fidelity());
    assert_eq!(given_up.len(), 1);
    let unreadable: Vec<_> = result
        .report()
        .losses
        .iter()
        .filter(|loss| loss.code.local_code() == "record.unreadable")
        .collect();
    assert_eq!(unreadable.len(), 1);
    let level = unreadable[0]
        .message
        .strip_prefix("part ")
        .and_then(|message| message.split(' ').next())
        .expect("part label");
    (
        level.to_owned(),
        given_up[0].offset(),
        given_up[0].byte_len(),
    )
}

fn attribute<'a>(result: &'a DecodeResult, key: &str) -> Option<&'a str> {
    result
        .ir()
        .source
        .as_ref()
        .expect("IGES source metadata")
        .attributes
        .get(key)
        .map(String::as_str)
}

fn entity(result: &DecodeResult) -> serde_json::Value {
    let native = result.ir().native.namespace("iges").unwrap();
    let entities = &native.arenas()["entities"];
    assert_eq!(entities.len(), 1);
    entities[0].fields().unwrap()["parameters"].clone()
}

#[test]
fn a_global_field_with_no_end_gives_up_the_rest_of_the_global_stream() {
    let global = |fields: &[&str]| format!("{};", fields.join(","));
    let baseline =
        crate::test_support::decode(point_file_with_global(global(&GLOBAL_FIELDS).as_bytes()));
    // Field 7 (integer bits) declares a Hollerith count larger than the
    // stream, so its end and every later boundary are unknown.
    let mut fields = GLOBAL_FIELDS;
    fields[6] = "99999H";
    let text = global(&fields);
    let decoded = crate::test_support::decode(point_file_with_global(text.as_bytes()));

    let start = fields[..6]
        .iter()
        .map(|field| field.len() + 1)
        .sum::<usize>();
    assert_eq!(
        only_given_up(&decoded),
        (
            "iges.global_field".to_owned(),
            u64::try_from(start).unwrap(),
            u64::try_from(text.len().div_ceil(72) * 72 - start).unwrap(),
        )
    );
    // The fields before it keep their values; the later fields are absent.
    assert_eq!(attribute(&decoded, "sender_product"), Some("product"));
    assert_eq!(attribute(&decoded, "native_file_name"), Some("part.igs"));
    assert_eq!(attribute(&baseline, "native_units"), Some("MM"));
    assert_eq!(attribute(&decoded, "native_units"), None);
    assert_eq!(attribute(&decoded, "parameter_delimiter"), Some(","));
    assert_eq!(attribute(&decoded, "record_delimiter"), Some(";"));
    // The Directory and Parameter Data sections are read with the declared
    // delimiters.
    assert_eq!(entity(&decoded), entity(&baseline));
}

#[test]
fn a_binary_global_value_that_does_not_parse_gives_up_only_the_rest_of_its_section() {
    let (original, _) = crate::test_support::binary_parts::point_with_global_control(6, 1);
    let baseline = crate::test_support::decode(original);
    // Control byte format 7 is not a Binary format (IGES 5.3 Appendix H.1).
    let (bytes, rest) = crate::test_support::binary_parts::point_with_global_control(6, 7);
    assert_eq!(bytes[rest.start], 7);
    let decoded = crate::test_support::decode(bytes);
    assert_eq!(
        only_given_up(&decoded),
        (
            "iges.binary_global_value".to_owned(),
            u64::try_from(rest.start).unwrap(),
            u64::try_from(rest.len()).unwrap(),
        )
    );
    assert_eq!(attribute(&decoded, "sender_product"), Some("product"));
    assert_eq!(attribute(&decoded, "native_file_name"), Some("part.igs"));
    assert_eq!(attribute(&baseline, "native_units"), Some("MM"));
    assert_eq!(attribute(&decoded, "native_units"), None);
    assert_eq!(entity(&decoded), entity(&baseline));
}
