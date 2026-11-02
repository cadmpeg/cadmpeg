// SPDX-License-Identifier: Apache-2.0

use super::test_fixtures::{
    acis_kernel_stream, content, primary_envelope_fixture_with_design_version, push_u16, push_u32,
    push_utf16,
};
use crate::feature::coil::CLASS_BYTES;

fn list(bytes: &mut Vec<u8>, references: &[u32]) {
    push_u16(bytes, 2);
    push_u16(bytes, 0x3000);
    let count = u32::try_from(references.len()).expect("synthetic list count");
    push_u32(bytes, count);
    if count != 0 {
        push_u32(bytes, count);
        push_u32(bytes, 0);
    }
    for reference in references {
        push_u32(bytes, *reference);
    }
}

pub(crate) fn document(first: bool, second: bool, selector: u16) -> Vec<u8> {
    let mut feature = content(0);
    push_u32(&mut feature, 0);
    push_u32(&mut feature, 0);
    let mut references = [0; 20];
    references[..3].copy_from_slice(&[0x8000_0006, 0x8000_0007, 0x8000_0008]);
    references[3..6].copy_from_slice(&[0x8000_0002, 0x8000_0003, 0x8000_0004]);
    list(&mut feature, &references);
    push_u32(&mut feature, 0);
    let boolean = |index, value| {
        let mut bytes = content(index);
        push_utf16(&mut bytes, "");
        push_u32(&mut bytes, 0);
        bytes.push(u8::from(value));
        bytes
    };
    let mut control = content(3);
    push_u16(&mut control, 3);
    push_u16(&mut control, selector);
    push_u32(&mut control, 0);
    let mut label = Vec::new();
    push_u32(&mut label, 0);
    push_u16(&mut label, 0);
    for value in [0, 0, 0x8000_0001, 0, 0, 1] {
        push_u32(&mut label, value);
    }
    list(&mut label, &[]);
    push_utf16(&mut label, "");
    label.extend(CLASS_BYTES);
    let mut operation = content(5);
    push_u16(&mut operation, 5);
    push_u16(&mut operation, 1);
    let mut patch = content(6);
    list(&mut patch, &[0x8000_0009]);
    let mut axis = content(7);
    for _ in 0..3 {
        push_u32(&mut axis, 0);
    }
    list(&mut axis, &[]);
    for value in [0.0_f64, 0.0, 0.0, 0.0, 0.0, 1.0] {
        axis.extend(value.to_le_bytes());
    }
    axis.push(1);
    let mut selection = content(8);
    push_u32(&mut selection, 0);
    selection.push(0);
    let mut root = Vec::new();
    push_u32(&mut root, 0);
    push_u16(&mut root, 1);
    push_u32(&mut root, 0);
    push_u32(&mut root, 0x40);
    root.extend_from_slice(&[0x31; 16]);
    push_utf16(&mut root, "");
    list(&mut root, &[]);
    for _ in 0..15 {
        push_u32(&mut root, 0);
    }
    push_utf16(&mut root, "PmBRepSegment");
    root.extend_from_slice(&1.0_f64.to_le_bytes());
    push_u32(&mut root, 0);
    let mut root_label = Vec::new();
    push_u32(&mut root_label, 0);
    push_u16(&mut root_label, 0);
    for value in [0, 0, 0x8000_000a, 0, 0, 0] {
        push_u32(&mut root_label, value);
    }
    list(&mut root_label, &[0x8000_0001]);
    push_utf16(&mut root_label, "");
    root_label.extend_from_slice(&[0; 16]);
    primary_envelope_fixture_with_design_version(
        &acis_kernel_stream(700),
        &[
            (crate::pmdc::inventor_id(0x9087_4d91), feature),
            (super::super::feature::BOOLEAN_TYPE, boolean(1, first)),
            (super::super::feature::BOOLEAN_TYPE, boolean(2, second)),
            ([0x42; 16], control),
            (
                [
                    0x2b, 0xa4, 0x48, 0x2b, 0xd2, 0x11, 0x58, 0x64, 0x60, 0x00, 0x74, 0xb7, 0x9b,
                    0x49, 0xeb, 0xb0,
                ],
                label,
            ),
            (
                [
                    0x28, 0xbe, 0x9a, 0x72, 0xd1, 0x11, 0x44, 0x09, 0x00, 0x08, 0x4e, 0xba, 0x32,
                    0xa3, 0xdc, 0x09,
                ],
                operation,
            ),
            (
                [
                    0x91, 0x73, 0x94, 0x22, 0xd1, 0x11, 0x07, 0xcf, 0x00, 0x08, 0x35, 0xbd, 0x06,
                    0x63, 0xdc, 0x09,
                ],
                patch,
            ),
            (
                [
                    0x89, 0x6c, 0xf0, 0x8e, 0xd1, 0x11, 0x3c, 0x04, 0x60, 0x00, 0x7c, 0xb8, 0x01,
                    0xf3, 0x1b, 0xb0,
                ],
                axis,
            ),
            (
                [
                    0x3b, 0x24, 0x77, 0xa4, 0xd1, 0x11, 0x8f, 0x96, 0x00, 0x08, 0x26, 0xbd, 0x06,
                    0x63, 0xdc, 0x09,
                ],
                selection,
            ),
            (crate::design::scope::MODEL_ROOT_TYPE, root),
            (
                [
                    0x2b, 0xa4, 0x48, 0x2b, 0xd2, 0x11, 0x58, 0x64, 0x60, 0x00, 0x74, 0xb7, 0x9b,
                    0x49, 0xeb, 0xb0,
                ],
                root_label,
            ),
        ],
        19,
    )
}
