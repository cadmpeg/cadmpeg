// SPDX-License-Identifier: Apache-2.0

use super::test_fixtures::{
    acis_sphere_kernel_stream, primary_envelope_fixture_with_design_version, push_u16, push_u32,
    push_utf16,
};

fn guid(value: &str) -> [u8; 16] {
    std::array::from_fn(|index| {
        u8::from_str_radix(&value[index * 2..index * 2 + 2], 16).expect("synthetic type")
    })
}

fn packed(index: u32) -> u32 {
    if index == 0 {
        0
    } else {
        index | 0x8000_0000
    }
}

fn references(bytes: &mut Vec<u8>, values: &[u32]) {
    push_u16(bytes, 2);
    push_u16(bytes, 0x3000);
    let count = u32::try_from(values.len()).expect("synthetic count");
    push_u32(bytes, count);
    if count != 0 {
        push_u32(bytes, count);
        push_u32(bytes, 0);
    }
    for &index in values {
        push_u32(bytes, packed(index));
    }
}

fn header() -> Vec<u8> {
    let mut bytes = Vec::new();
    push_u32(&mut bytes, 0);
    push_u16(&mut bytes, 16);
    push_u32(&mut bytes, 0);
    push_u32(&mut bytes, 0);
    bytes
}

fn content(source: u32, context: u32) -> Vec<u8> {
    let mut bytes = header();
    push_u32(&mut bytes, packed(context));
    push_u32(&mut bytes, source);
    bytes
}

fn enumeration(source: u32, kind: u16, value: u16) -> Vec<u8> {
    let mut bytes = content(source, 1);
    push_u16(&mut bytes, kind);
    push_u16(&mut bytes, value);
    bytes
}

fn boolean(source: u32, value: bool) -> Vec<u8> {
    let mut bytes = content(source, 1);
    push_utf16(&mut bytes, "");
    push_u32(&mut bytes, 0);
    bytes.push(u8::from(value));
    bytes
}

fn parameter(source: u32, name: &str, unit: u32, formula: u32, value: f64) -> Vec<u8> {
    let mut bytes = content(source, 2);
    push_utf16(&mut bytes, name);
    push_u32(&mut bytes, 0);
    push_u32(&mut bytes, packed(unit));
    push_u32(&mut bytes, packed(formula));
    bytes.extend_from_slice(&value.to_le_bytes());
    bytes.extend_from_slice(&value.to_le_bytes());
    push_u16(&mut bytes, 0);
    push_u16(&mut bytes, 0);
    bytes
}

fn literal(unit: u32, value: f64) -> Vec<u8> {
    let mut bytes = Vec::new();
    push_u32(&mut bytes, 0);
    push_u16(&mut bytes, 16);
    push_u32(&mut bytes, packed(unit));
    bytes.extend_from_slice(&value.to_le_bytes());
    push_u16(&mut bytes, 0);
    push_u32(&mut bytes, 0);
    bytes
}

fn unit(base: u32) -> Vec<u8> {
    let mut bytes = Vec::new();
    push_u32(&mut bytes, 0);
    push_u16(&mut bytes, 16);
    for value in [3, 0x3000] {
        push_u16(&mut bytes, value);
    }
    push_u32(&mut bytes, 1);
    push_u16(&mut bytes, 1);
    push_u16(&mut bytes, 0);
    push_u32(&mut bytes, packed(base));
    push_u16(&mut bytes, 3);
    push_u16(&mut bytes, 0x3000);
    push_u32(&mut bytes, 0);
    bytes.push(1);
    push_u32(&mut bytes, 0);
    bytes
}

fn base_unit() -> Vec<u8> {
    let mut bytes = Vec::new();
    push_u32(&mut bytes, 0);
    push_u16(&mut bytes, 16);
    for value in [1.0_f64, 1.0] {
        bytes.extend_from_slice(&value.to_le_bytes());
    }
    bytes
}

fn label(owner: u32, participants: &[u32], class: [u8; 16]) -> Vec<u8> {
    let mut bytes = Vec::new();
    push_u32(&mut bytes, 0);
    push_u16(&mut bytes, 18);
    for value in [0, 0, packed(owner), 0, 0, 1] {
        push_u32(&mut bytes, value);
    }
    references(&mut bytes, participants);
    push_utf16(&mut bytes, "Synthetic Thicken");
    bytes.extend_from_slice(&class);
    bytes
}

pub(crate) fn fixture(slot_two: bool, slot_sixteen: bool, operation: u16) -> Vec<u8> {
    build_fixture(slot_two, slot_sixteen, operation, false, 19)
}

pub(crate) fn offset_fixture() -> Vec<u8> {
    offset_fixture_with_controls(false, true, 3)
}

pub(crate) fn offset_fixture_with_controls(
    slot_two: bool,
    slot_sixteen: bool,
    operation: u16,
) -> Vec<u8> {
    build_fixture(slot_two, slot_sixteen, operation, true, 19)
}

pub(crate) fn offset_fixture_at_version(version: u8) -> Vec<u8> {
    build_fixture(false, true, 3, true, version)
}

fn build_fixture(
    slot_two: bool,
    slot_sixteen: bool,
    operation: u16,
    selected: bool,
    version: u8,
) -> Vec<u8> {
    let mut root = header();
    root.extend_from_slice(&[0x39; 16]);
    push_utf16(&mut root, "Synthetic model");
    references(&mut root, &[2, 11]);
    let mut root_values = [0; 15];
    root_values[3] = packed(2);
    root_values[9] = packed(2);
    root_values[12] = 1;
    for value in root_values {
        push_u32(&mut root, value);
    }
    push_utf16(&mut root, "PmBRepSegment");
    root.extend_from_slice(&1.0_f64.to_le_bytes());
    push_u32(&mut root, 0);

    let mut table = header();
    push_u32(&mut table, packed(1));
    let parameter_refs = if selected {
        &[8, 10, 26][..]
    } else {
        &[8, 10][..]
    };
    references(&mut table, parameter_refs);
    push_u16(&mut table, 6);
    push_u16(&mut table, 0x3000);
    let parameter_count = u32::try_from(parameter_refs.len()).expect("parameter count");
    for value in [parameter_count, parameter_count, 0] {
        push_u32(&mut table, value);
    }
    for (name, reference) in [("SyntheticThickness", 8), ("SyntheticControl", 10)] {
        push_utf16(&mut table, name);
        push_u32(&mut table, packed(reference));
    }

    if selected {
        push_utf16(&mut table, "SyntheticOffset");
        push_u32(&mut table, packed(26));
    }

    let mut feature = content(11, 1);
    push_u32(&mut feature, 0);
    push_u32(&mut feature, 0);
    let slot_two_reference = if slot_two { 17 } else { 18 };
    let slot_sixteen_reference = if selected {
        29
    } else if slot_sixteen {
        17
    } else {
        18
    };
    references(
        &mut feature,
        &[
            14,
            16,
            slot_two_reference,
            18,
            18,
            0,
            8,
            18,
            0,
            19,
            18,
            0,
            10,
            20,
            21,
            22,
            slot_sixteen_reference,
            18,
        ],
    );
    push_u32(&mut feature, 0);

    let mut selection = content(14, 1);
    references(&mut selection, &[15]);
    let mut dimensions = content(19, 1);
    references(&mut dimensions, if selected { &[28] } else { &[] });
    let mut results = content(22, 1);
    references(&mut results, &[23]);
    let mut body = content(23, 1);
    push_u32(&mut body, 0);

    let parameter_type = crate::pmdc::inventor_id(0x9087_4d26);
    let label_type = guid("2ba4482bd2115864600074b79b49ebb0");
    let boolean_type = crate::pmdc::inventor_id(0x9087_4d28);
    let mut selection_item = content(15, 1);
    if selected {
        push_u32(&mut selection_item, packed(24));
        push_u32(&mut selection_item, packed(26));
    }
    let mut records = vec![
        (crate::pmdc::inventor_id(0x9087_4d16), root),
        (crate::pmdc::inventor_id(0x9087_4d63), table),
        (guid("fd79a7f8d2118f09c0005a9a2378d04f"), unit(4)),
        (guid("bc204162d2119b0b60006ab760fec3b0"), base_unit()),
        (guid("fd79a7f8d2118f09c0005a9a2378d04f"), unit(6)),
        (guid("23009d5fd2118e09c0005a9a2378d04f"), base_unit()),
        (guid("047aa7f8d2118f09c0005a9a2378d04f"), literal(3, 0.25)),
        (
            parameter_type,
            parameter(8, "SyntheticThickness", 3, 7, 0.25),
        ),
        (guid("047aa7f8d2118f09c0005a9a2378d04f"), literal(5, 10.0)),
        (
            parameter_type,
            parameter(10, "SyntheticControl", 5, 9, 10.0),
        ),
        (crate::pmdc::inventor_id(0x9087_4d91), feature),
        (label_type, label(1, &[11], [0; 16])),
        (
            label_type,
            label(11, &[], guid("ecf481729c4d5f925f2192b4a83096ac")),
        ),
        (guid("7a25f2efd11105af000847b00524dc09"), selection),
        (
            if selected {
                guid("68888bbfd11105af000847b00524dc09")
            } else {
                [0x42; 16]
            },
            selection_item,
        ),
        (
            guid("28be9a72d111440900084eba32a3dc09"),
            enumeration(16, 5, operation),
        ),
        (boolean_type, boolean(17, true)),
        (boolean_type, boolean(18, false)),
        (guid("71f23ed8d2115094a00049803603c8c9"), dimensions),
        (
            guid("4c40b0a3ac4b9504e719b28a70e0e818"),
            enumeration(20, 3, 3),
        ),
        (
            guid("c11c7b63ea4d840b68b76e8c67bcbc48"),
            enumeration(21, 1, 0),
        ),
        (guid("ae70680ed14a1e86d76248b03c2a96e1"), results),
        (crate::pmdc::inventor_id(0x9087_4d47), body),
    ];
    if selected {
        let mut faces = content(24, 1);
        references(&mut faces, &[25]);
        let mut face = content(25, 1);
        push_u16(&mut face, 2);
        push_u16(&mut face, 0x3000);
        for value in [1, 1, 0x1000_0000, 1] {
            push_u32(&mut face, value);
        }
        face.extend_from_slice(&(-1i32).to_le_bytes());
        let mut dimension = Vec::new();
        push_u32(&mut dimension, 0);
        push_u16(&mut dimension, 16);
        push_u32(&mut dimension, 0x3000_0002);
        push_u32(&mut dimension, 0);
        dimension.extend_from_slice(&[0; 5]);
        push_u32(&mut dimension, packed(26));
        records.extend([
            (guid("f1b2bc24d01121dd0008d0bc0663dc09"), faces),
            (guid("ccb59f50d1114f6b0008a1ba32a3dc09"), face),
            (parameter_type, parameter(26, "SyntheticOffset", 3, 27, 0.4)),
            (guid("047aa7f8d2118f09c0005a9a2378d04f"), literal(3, 0.4)),
            (guid("b1377779d2116794a00049803603c8c9"), dimension),
            (boolean_type, boolean(29, slot_sixteen)),
        ]);
    }
    primary_envelope_fixture_with_design_version(
        &acis_sphere_kernel_stream(21_800),
        &records,
        version,
    )
}
