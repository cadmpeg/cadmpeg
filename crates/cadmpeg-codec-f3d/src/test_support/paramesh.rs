// SPDX-License-Identifier: Apache-2.0
//! Synthetic triangle mesh container.

fn varint(mut value: u64) -> Vec<u8> {
    let mut bytes = Vec::new();
    loop {
        let byte = u8::try_from(value & 0x7f).unwrap();
        value >>= 7;
        bytes.push(if value == 0 { byte } else { byte | 0x80 });
        if value == 0 {
            return bytes;
        }
    }
}

fn field(number: u64, data: &[u8]) -> Vec<u8> {
    let mut result = varint(number << 3 | 2);
    result.extend(varint(u64::try_from(data.len()).unwrap()));
    result.extend_from_slice(data);
    result
}

fn raw_chunk(descriptor: &[u8], data: &[u8]) -> Vec<u8> {
    let mut body = Vec::new();
    body.extend_from_slice(&u16::try_from(descriptor.len()).unwrap().to_le_bytes());
    body.extend_from_slice(descriptor);
    body.extend_from_slice(&u32::try_from(data.len()).unwrap().to_le_bytes());
    body.extend_from_slice(&[0x5d, 0xfe]);
    body.extend_from_slice(data);
    let mut chunk = u64::try_from(body.len()).unwrap().to_le_bytes().to_vec();
    chunk.extend_from_slice(&4_u32.to_le_bytes());
    chunk.extend(body);
    chunk
}

pub(crate) fn triangle() -> Vec<u8> {
    let mut property = field(1, b"fusion_uuid");
    property.extend(field(3, b"AAAAAAAA-BBBB-4CCC-8DDD-EEEEEEEEEEEE"));
    let mut registry = field(8, &property);
    registry.extend([0x48, 0]);
    registry.extend(field(12, b"11111111-2222-4333-8444-555555555555"));
    registry.extend(field(21, b"v"));
    registry.extend(field(22, b"t"));
    let mut bytes = b"\x89UDPMESH\r\n\x1a\n".to_vec();
    bytes.extend_from_slice(&2_u32.to_le_bytes());
    bytes.extend_from_slice(&[0; 32]);
    bytes.extend_from_slice(&u64::try_from(registry.len()).unwrap().to_le_bytes());
    bytes.extend_from_slice(&1_u32.to_le_bytes());
    bytes.extend(registry);
    let names = [0x82, 0xa1, b'v', 2, 0xa1, b't', 3];
    bytes.extend_from_slice(&u64::try_from(names.len()).unwrap().to_le_bytes());
    bytes.extend_from_slice(&3_u32.to_le_bytes());
    bytes.extend_from_slice(&names);
    let vertices: Vec<_> = [0.0_f32, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 1.0, 0.0]
        .into_iter()
        .flat_map(f32::to_le_bytes)
        .collect();
    let corners: Vec<_> = [1_i32, 1, 0]
        .into_iter()
        .flat_map(i32::to_le_bytes)
        .collect();
    bytes.extend(raw_chunk(&[0x82, 0xa1, b'D', 3, 0xa1, b'T', 3], &vertices));
    bytes.extend(raw_chunk(&[0x82, 0xa1, b'T', 1, 0xa1, b'd', 1], &corners));
    bytes
}
