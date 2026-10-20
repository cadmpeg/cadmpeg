// SPDX-License-Identifier: Apache-2.0
//! Primary WORLD edit programs and status-framed instances.

fn pointer(bytes: &mut Vec<u8>, op: u8, name: &str, class: u16) {
    bytes.extend_from_slice(&[op, u8::try_from(name.len()).unwrap()]);
    bytes.extend_from_slice(name.as_bytes());
    bytes.extend_from_slice(&class.to_be_bytes());
    bytes.extend_from_slice(&[0, 1]);
}

fn integer(bytes: &mut Vec<u8>, name: &str) {
    pointer(bytes, b'A', name, 0);
    bytes.extend_from_slice(&[1, b'd']);
}

pub(crate) fn schema(count: u8) -> Vec<u8> {
    assert!(matches!(count, 15 | 18 | 19));
    let mut bytes = vec![0, 101, count];
    bytes.extend_from_slice(b"CCCCCCC");
    if count >= 18 {
        pointer(&mut bytes, b'I', "mesh", 1006);
        pointer(&mut bytes, b'I', "polyline", 1008);
    }
    if count == 19 {
        pointer(&mut bytes, b'I', "lattice", 222);
    }
    bytes.extend_from_slice(b"CC");
    pointer(&mut bytes, b'I', "attdef_list", 74);
    bytes.extend_from_slice(b"CC");
    integer(&mut bytes, "index_map_offset");
    pointer(&mut bytes, b'A', "index_map", 82);
    pointer(&mut bytes, b'A', "schema_embedding_map", 82);
    if count >= 18 {
        pointer(&mut bytes, b'A', "mesh_offset_data", 206);
    }
    bytes.push(b'Z');
    bytes
}

pub(crate) fn record(count: u8, escaped: bool) -> Vec<u8> {
    assert!(matches!(count, 15 | 18 | 19));
    let mut bytes = vec![0, 101];
    if escaped {
        bytes.push(255);
    }
    bytes.extend_from_slice(&2u16.to_be_bytes());
    let leading: u16 = match count {
        15 => 7,
        18 => 9,
        _ => 10,
    };
    for target in 3..leading + 3 {
        bytes.extend_from_slice(&target.to_be_bytes());
        bytes.push(1);
    }
    bytes.push(1);
    for target in leading + 3..leading + 5 {
        bytes.extend_from_slice(&target.to_be_bytes());
        bytes.push(1);
    }
    for scalar in [u32::MAX, 0, 0x0001_0001] {
        bytes.extend_from_slice(&scalar.to_be_bytes());
    }
    let trailing = if count == 15 { 2 } else { 3 };
    for target in leading + 5..leading + 5 + trailing {
        bytes.extend_from_slice(&target.to_be_bytes());
        bytes.push(1);
    }
    bytes
}

pub(crate) fn first_node(count: u8) -> Vec<u8> {
    let mut bytes = schema(count);
    bytes.extend_from_slice(&record(count, false)[2..]);
    bytes
}
