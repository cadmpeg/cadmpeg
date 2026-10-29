// SPDX-License-Identifier: Apache-2.0

use cadmpeg_ir::native::bytes::NativeBytes;
use serde_json::json;

use super::test_fixtures::{
    acis_kernel_stream, content, primary_envelope_fixture_with_design, push_u16, push_u32,
    push_utf16,
};

fn guid(value: &str) -> [u8; 16] {
    serde_json::from_value::<NativeBytes<[u8; 16]>>(json!(value))
        .expect("synthetic GUID")
        .into_inner()
}

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

pub(crate) fn document(operation: u16, keep_tools: bool) -> Vec<u8> {
    let mut feature = content(0);
    push_u32(&mut feature, 0);
    push_u32(&mut feature, 0);
    list(&mut feature, &[2, 3, 4, 5]);
    push_u32(&mut feature, 0);

    let mut target = content(1);
    list(&mut target, &[0x8000_0006]);
    let mut tools = content(2);
    list(&mut tools, &[7]);
    let mut enumeration = content(3);
    push_u16(&mut enumeration, 5);
    push_u16(&mut enumeration, operation);
    let mut keep = content(4);
    push_utf16(&mut keep, "");
    push_u32(&mut keep, 0);
    keep.push(u8::from(keep_tools));
    let mut target_body = content(5);
    push_u32(&mut target_body, 0);
    let mut tool_body = content(6);
    push_u32(&mut tool_body, 0);

    let mut label = Vec::new();
    push_u32(&mut label, 0);
    push_u16(&mut label, 0);
    for value in [0, 0, 1, 0, 0, 1] {
        push_u32(&mut label, value);
    }
    list(&mut label, &[]);
    push_utf16(&mut label, "");
    label.extend(guid("336723c07e40eacb2eff669e9add87f8"));

    let records = [
        (guid("914d8790d011f8d10008cabc0663dc09"), feature),
        (guid("83aadad54f4375dacd1003b260a8a422"), target),
        (guid("ae70680ed14a1e86d76248b03c2a96e1"), tools),
        (guid("28be9a72d111440900084eba32a3dc09"), enumeration),
        (guid("284d8790d011f8d10008cabc0663dc09"), keep),
        (guid("474d8790d011f8d10008cabc0663dc09"), target_body),
        (guid("474d8790d011f8d10008cabc0663dc09"), tool_body),
        (guid("2ba4482bd2115864600074b79b49ebb0"), label),
    ];
    primary_envelope_fixture_with_design(&acis_kernel_stream(700), &records)
}
