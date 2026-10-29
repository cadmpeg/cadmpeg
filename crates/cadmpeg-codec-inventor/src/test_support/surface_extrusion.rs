// SPDX-License-Identifier: Apache-2.0

use super::test_fixtures::{
    acis_sphere_kernel_stream, primary_envelope_fixture_with_design, push_u16, push_u32, push_utf16,
};

fn type_id(value: &str) -> [u8; 16] {
    assert_eq!(value.len(), 32);
    std::array::from_fn(|index| {
        u8::from_str_radix(&value[index * 2..index * 2 + 2], 16).expect("fixture type id")
    })
}

fn references(bytes: &mut Vec<u8>, values: &[u32]) {
    push_u16(bytes, 2);
    push_u16(bytes, 0x3000);
    let count = u32::try_from(values.len()).expect("fixture reference count");
    push_u32(bytes, count);
    if !values.is_empty() {
        push_u32(bytes, count);
        push_u32(bytes, 0);
    }
    for &value in values {
        push_u32(bytes, value);
    }
}

fn content(source: u32) -> Vec<u8> {
    let mut bytes = Vec::new();
    push_u32(&mut bytes, 0);
    push_u16(&mut bytes, 16);
    for value in [0, 0, 0x8000_0001, source] {
        push_u32(&mut bytes, value);
    }
    bytes
}

fn label(owner: u32, participants: &[u32], class: [u8; 16]) -> Vec<u8> {
    let mut bytes = Vec::new();
    push_u32(&mut bytes, 0);
    push_u16(&mut bytes, 18);
    for value in [0, 0, owner, 0, 0, 1] {
        push_u32(&mut bytes, value);
    }
    references(&mut bytes, participants);
    push_utf16(&mut bytes, "Synthetic feature");
    bytes.extend_from_slice(&class);
    bytes
}

pub(crate) fn fixture() -> Vec<u8> {
    let mut root = Vec::new();
    push_u32(&mut root, 0);
    push_u16(&mut root, 16);
    push_u32(&mut root, 0);
    push_u32(&mut root, 0);
    root.extend_from_slice(&[0x39; 16]);
    push_utf16(&mut root, "Synthetic model");
    references(&mut root, &[0x8000_0002]);
    let mut values = [0; 15];
    values[12] = 1;
    for value in values {
        push_u32(&mut root, value);
    }
    push_utf16(&mut root, "PmBRepSegment");
    root.extend_from_slice(&1.0f64.to_le_bytes());
    push_u32(&mut root, 0);

    let mut feature = content(2);
    push_u32(&mut feature, 0);
    push_u32(&mut feature, 0);
    let mut slots = [0; 30];
    slots[0] = 0x8000_0003;
    references(&mut feature, &slots);
    push_u32(&mut feature, 0);

    let mut operation = content(3);
    push_u16(&mut operation, 5);
    push_u16(&mut operation, 5);
    let label_type = type_id("2ba4482bd2115864600074b79b49ebb0");
    let records = vec![
        (type_id("164d8790d011f8d10008cabc0663dc09"), root),
        (type_id("914d8790d011f8d10008cabc0663dc09"), feature),
        (type_id("28be9a72d111440900084eba32a3dc09"), operation),
        (label_type, label(0x8000_0001, &[0x8000_0002], [0; 16])),
        (
            label_type,
            label(
                0x8000_0002,
                &[],
                type_id("f0a497e5d411d8841000faa5bba647b5"),
            ),
        ),
    ];
    primary_envelope_fixture_with_design(&acis_sphere_kernel_stream(21_800), &records)
}
