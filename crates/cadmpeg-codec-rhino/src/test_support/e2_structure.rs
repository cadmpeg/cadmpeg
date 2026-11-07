// SPDX-License-Identifier: Apache-2.0
//! Synthetic document structure for the cross-format contract.

use crate::chunks::ArchiveVersion;
use crate::chunks::TCODE_CRC;
use crate::test_support::test_dump as wire;
use crate::test_support::test_dump as bytes;
use crate::test_support::test_dump::{point, utf16_bytes};
use crate::wire::Uuid;

const ARCHIVE: ArchiveVersion = ArchiveVersion::V8;
const LAYER_CLASS: [u8; 16] = [
    0x13, 0x98, 0x80, 0x95, 0x85, 0xe9, 0xd3, 0x11, 0xbf, 0xe5, 0x00, 0x10, 0x83, 0x01, 0x22, 0xf0,
];
const GROUP_CLASS: [u8; 16] = [
    0x97, 0x9f, 0x1d, 0x72, 0x45, 0x36, 0xc4, 0x44, 0x8b, 0xe6, 0xb2, 0xcf, 0x69, 0x7d, 0x25, 0xce,
];

fn layer(index: i32, id: [u8; 16], parent: [u8; 16], name: &str, color: [u8; 4]) -> Vec<u8> {
    let mut payload = vec![0x1f];
    for value in [0_i32, index, 0, -1, 0] {
        payload.extend(value.to_le_bytes());
    }
    payload.extend(color);
    payload.extend([0_u8; 4]);
    payload.extend(0.0_f64.to_le_bytes());
    payload.extend(1.0_f64.to_le_bytes());
    payload.extend(wire::utf16_bytes(name));
    payload.push(1);
    payload.extend((-1_i32).to_le_bytes());
    payload.extend(color);
    payload.extend(0.0_f64.to_le_bytes());
    payload.push(0);
    payload.extend(id);
    payload.extend(parent);
    payload.push(1);
    payload.extend(wire::crc_chunk(
        ARCHIVE,
        0x4000_8000,
        &[1, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0],
    ));
    payload.extend([0_u8; 16]);
    payload.push(0);
    let class = wire::class_wrapper(ARCHIVE, LAYER_CLASS, &payload);
    wire::crc_chunk_excluding(
        ARCHIVE,
        0x2000_8050,
        &class,
        std::slice::from_ref(&(0..class.len())),
    )
}

fn group() -> Vec<u8> {
    let mut payload = vec![0x11];
    payload.extend(0_i32.to_le_bytes());
    payload.extend(wire::utf16_bytes("Witness group"));
    payload.extend([0x31; 16]);
    let class = wire::class_wrapper(ARCHIVE, GROUP_CLASS, &payload);
    wire::crc_chunk_excluding(
        ARCHIVE,
        0x2000_8073,
        &class,
        std::slice::from_ref(&(0..class.len())),
    )
}

fn object(
    kind: i64,
    class_id: [u8; 16],
    payload: &[u8],
    id: [u8; 16],
    name: &str,
    layer: i32,
    material: i32,
) -> Vec<u8> {
    let mut groups = 1_i32.to_le_bytes().to_vec();
    groups.extend(0_i32.to_le_bytes());
    let mut attributes = wire::tagged_attributes(
        &[
            (1, wire::utf16_bytes(name)),
            (4, material.to_le_bytes().to_vec()),
            (16, vec![1]),
            (18, groups),
        ],
        0,
    );
    attributes[1..17].copy_from_slice(&id);
    attributes[17..21].copy_from_slice(&layer.to_le_bytes());
    let body = [
        wire::short_chunk(ARCHIVE, 0x8200_0071, kind),
        wire::class_wrapper(ARCHIVE, class_id, payload),
        wire::crc_chunk(ARCHIVE, 0x0200_8072, &attributes),
        wire::short_chunk(ARCHIVE, 0x8200_007f, 0),
    ]
    .concat();
    wire::nested_crc_chunk(ARCHIVE, 0x2000_8070 | crate::chunks::TCODE_CRC, &body)
}

fn named_view() -> Vec<u8> {
    let viewport = wire::crc_chunk(ARCHIVE, 0x2000_823b, &viewport_body());
    let name = wire::crc_chunk(ARCHIVE, 0x2000_8a3b, &wire::utf16_bytes("Witness view"));
    let end = wire::short_chunk(ARCHIVE, crate::chunks::TCODE_ENDOFTABLE, 0);
    let children = [viewport, name, end];
    let mut body = Vec::new();
    let mut ranges = Vec::new();
    for child in children {
        let start = body.len();
        body.extend(child);
        ranges.push(start..body.len());
    }
    let view = wire::crc_chunk_excluding(ARCHIVE, 0x2000_803b, &body, &ranges);
    let mut list = 1_i32.to_le_bytes().to_vec();
    let start = list.len();
    list.extend(view);
    wire::crc_chunk_excluding(
        ARCHIVE,
        0x2000_8036,
        &list,
        std::slice::from_ref(&(start..list.len())),
    )
}

pub(crate) fn build() -> Vec<u8> {
    let definition_id = [0x61; 16];
    let member_id = [0x51; 16];
    let definition = wire::definition_record(
        ARCHIVE,
        &wire::v6_definition_payload(ARCHIVE, definition_id, &[member_id], 1, false, false),
    );
    let member = object(
        1,
        wire::POINT_CLASS,
        &wire::point_payload([1.0, 2.0, 3.0]),
        member_id,
        "Member",
        1,
        0,
    );
    let first = object(
        0x1000,
        wire::INSTANCE_REFERENCE_CLASS,
        &wire::instance_reference_payload(definition_id, wire::transform(1.0, [10.0, 0.0, 0.0])),
        [0x71; 16],
        "First",
        1,
        1,
    );
    let second = object(
        0x1000,
        wire::INSTANCE_REFERENCE_CLASS,
        &wire::instance_reference_payload(definition_id, wire::transform(1.0, [20.0, 0.0, 0.0])),
        [0x72; 16],
        "Second",
        0,
        0,
    );
    let text = text_record(ARCHIVE, &[]);
    let materials = [
        material_record(0x81, "Copper", 0, false, &[]),
        material_record(0x82, "Blue", 1, false, &[]),
    ];
    let layers = [
        layer(0, [0x41; 16], [0; 16], "Parent", [180, 80, 40, 255]),
        layer(1, [0x42; 16], [0x41; 16], "Child", [40, 80, 180, 255]),
    ];
    wire::minimal_document(
        "80",
        &[
            wire::table(
                ARCHIVE,
                0x1000_0014,
                &[wire::short_chunk(ARCHIVE, 0xa000_0026, 202_608_010)],
            ),
            wire::table(
                ARCHIVE,
                0x1000_0015,
                &[wire::units_record(ARCHIVE, 2), named_view()],
            ),
            wire::table(ARCHIVE, 0x1000_0010, &materials),
            wire::table(ARCHIVE, 0x1000_0011, &layers),
            wire::table(ARCHIVE, 0x1000_0018, &[group()]),
            wire::table(ARCHIVE, 0x1000_0021, &[definition]),
            wire::table(ARCHIVE, 0x1000_0013, &[member, first, second, text]),
        ],
    )
}

const LEGACY_TEXT: [u8; 16] = [
    0x46, 0xf7, 0x55, 0x41, 0xf4, 0x6b, 0x48, 0xbe, 0xaa, 0x7e, 0xb3, 0x53, 0xbb, 0xe0, 0x68, 0xa7,
];
/// The world plane an annotation userdata payload carries.
pub(crate) fn plane() -> Vec<u8> {
    [
        0.0, 0.0, 0.0, // origin
        1.0, 0.0, 0.0, // x axis
        0.0, 1.0, 0.0, // y axis
        0.0, 0.0, 1.0, // z axis
        0.0, 0.0, 1.0, 0.0, // equation
    ]
    .into_iter()
    .flat_map(f64::to_le_bytes)
    .collect()
}

fn legacy_text_payload(archive: ArchiveVersion) -> Vec<u8> {
    let mut fields = 7_i32.to_le_bytes().to_vec();
    fields.extend(0_i32.to_le_bytes());
    fields.extend(plane());
    fields.extend(0_i32.to_le_bytes());
    fields.extend(utf16_bytes("legacy text"));
    fields.extend(0_i32.to_le_bytes());
    fields.extend(0_i32.to_le_bytes());
    fields.extend(1.5_f64.to_le_bytes());
    fields.extend(0_i32.to_le_bytes());
    fields.push(0);
    fields.extend(utf16_bytes(""));
    fields.extend(0_i32.to_le_bytes());
    fields.extend((-1_i32).to_le_bytes());

    let base = crate::test_support::test_dump::anonymous_chunk(archive, 3, &fields);
    crate::test_support::test_dump::anonymous_chunk(archive, 0, &base)
}

pub(crate) fn text_record(archive: ArchiveVersion, userdata: &[u8]) -> Vec<u8> {
    let object_type = crate::test_support::test_dump::short_chunk(archive, 0x8200_0071, 0x20);
    let legacy_text_wire = Uuid::from_canonical(LEGACY_TEXT).to_wire();
    let mut uuid_body = legacy_text_wire.to_vec();
    uuid_body.extend(crc32fast::hash(&legacy_text_wire).to_le_bytes());
    let class_uuid = crate::test_support::test_dump::long_chunk(archive, 0x0002_fffb, &uuid_body);
    let class_data = crate::test_support::test_dump::crc_chunk(
        archive,
        0x0002_fffc,
        &legacy_text_payload(archive),
    );
    let class_end = crate::test_support::test_dump::short_chunk(archive, 0x8002_7fff, 0);
    let class = crate::test_support::test_dump::long_chunk(
        archive,
        0x0002_7ffa,
        &[class_uuid, class_data, userdata.to_vec(), class_end].concat(),
    );
    let object_end = crate::test_support::test_dump::short_chunk(archive, 0x8200_007f, 0);
    crate::test_support::test_dump::nested_crc_chunk(
        archive,
        0x2000_8070 | TCODE_CRC,
        &[object_type, class, object_end].concat(),
    )
}

pub(crate) fn viewport_body() -> Vec<u8> {
    let mut bytes = vec![0x15];
    for value in [1_i32, 1, 1, 2] {
        bytes.extend(value.to_le_bytes());
    }
    point(&mut bytes, [1.0, 2.0, 3.0]);
    for vector in [
        [0.0, 0.0, -1.0],
        [0.0, 1.0, 0.0],
        [1.0, 0.0, 0.0],
        [0.0, 1.0, 0.0],
        [0.0, 0.0, 1.0],
    ] {
        point(&mut bytes, vector);
    }
    for value in [-2.0_f64, 2.0, -1.0, 1.0, 0.1, 100.0] {
        bytes.extend(value.to_le_bytes());
    }
    for value in [0_i32, 1920, 0, 1080, 1, 100] {
        bytes.extend(value.to_le_bytes());
    }
    bytes.extend([0x11; 16]);
    bytes.extend([1, 0, 1, 0, 1]);
    point(&mut bytes, [4.0, 5.0, 6.0]);
    bytes.push(1);
    for value in [1.0_f64, 2.0, 3.0] {
        bytes.extend(value.to_le_bytes());
    }
    bytes
}

fn material_texture(uuid: u8) -> Vec<u8> {
    let archive = ArchiveVersion::V8;
    let mut body = vec![uuid; 16];
    body.extend(7_u32.to_le_bytes());
    body.extend(bytes::utf16_bytes("texture.png"));
    body.push(1);
    for value in 1..=7_u32 {
        body.extend(value.to_le_bytes());
    }
    for index in 0..16 {
        body.extend((if index % 5 == 0 { 1.0_f64 } else { 0.0 }).to_le_bytes());
    }
    body.extend([1, 2, 3, 4]);
    body.extend([5, 6, 7, 8]);
    body.extend([0x22; 16]);
    for value in [0.25_f64, 1.25, 0.1, 0.2, 0.3, 0.4, 0.5] {
        body.extend(value.to_le_bytes());
    }
    body.extend([9, 10, 11, 12]);
    for value in [0.6_f64, 0.7, 0.8, 0.9] {
        body.extend(value.to_le_bytes());
    }
    body.extend(4_i32.to_le_bytes());
    let texture_uuid = crate::wire::Uuid::from_canonical([
        0xd6, 0xff, 0x10, 0x6d, 0x32, 0x9b, 0x4f, 0x29, 0x97, 0xe2, 0xfd, 0x28, 0x2a, 0x61, 0x80,
        0x20,
    ])
    .to_wire();
    bytes::long_chunk(
        archive,
        0x0002_7ffa,
        &[
            bytes::crc_chunk(archive, 0x0002_fffb, &texture_uuid),
            bytes::nested_crc_chunk(
                archive,
                0x0002_fffc,
                &bytes::anonymous_chunk(archive, 0, &body),
            ),
            bytes::short_chunk(archive, 0x8002_7fff, 0),
        ]
        .concat(),
    )
}

pub(crate) fn material_record(
    uuid: u8,
    name: &str,
    index: i32,
    fresnel: bool,
    textures: &[u8],
) -> Vec<u8> {
    let archive = ArchiveVersion::V8;
    let mut body = vec![uuid; 16];
    body.extend(index.to_le_bytes());
    body.extend(bytes::utf16_bytes(name));
    body.extend([0xaa; 16]);
    for color in [
        [1, 2, 3, 4],
        [10, 20, 30, 40],
        [5, 6, 7, 8],
        [9, 10, 11, 12],
        [13, 14, 15, 16],
        [17, 18, 19, 20],
    ] {
        body.extend(color);
    }
    for value in [1.25_f64, 0.125, 2.5, 0.25] {
        body.extend(value.to_le_bytes());
    }
    let texture_start = body.len();
    let mut texture_array = i32::try_from(textures.len())
        .unwrap()
        .to_le_bytes()
        .to_vec();
    let mut children = Vec::new();
    for identity in textures {
        let start = texture_array.len();
        texture_array.extend(material_texture(*identity));
        children.push(start + 8..texture_array.len() + 8);
    }
    let mut texture_fields = 1_i32.to_le_bytes().to_vec();
    texture_fields.extend(0_i32.to_le_bytes());
    texture_fields.extend(texture_array);
    body.extend(bytes::crc_chunk_excluding(
        archive,
        0x4000_8000,
        &texture_fields,
        &children,
    ));
    let texture_end = body.len();
    body.extend(bytes::utf16_bytes(""));
    body.extend(0_i32.to_le_bytes());
    body.extend([1, 0]);
    if fresnel {
        body.push(1);
        for value in [0.375_f64, 0.625, 1.5] {
            body.extend(value.to_le_bytes());
        }
        body.extend([0xbb; 16]);
        body.push(1);
    }
    let mut fields = 1_i32.to_le_bytes().to_vec();
    fields.extend((if fresnel { 7_i32 } else { 3 }).to_le_bytes());
    fields.extend(body);
    let inner = bytes::crc_chunk_excluding(
        archive,
        0x4000_8000,
        &fields,
        &[std::ops::Range {
            start: texture_start + 8,
            end: texture_end + 8,
        }],
    );
    let mut payload = vec![0x20];
    payload.extend(inner);
    let material_uuid = crate::wire::Uuid::from_canonical([
        0x60, 0xb5, 0xdb, 0xbc, 0xe6, 0x60, 0x11, 0xd3, 0xbf, 0xe4, 0x00, 0x10, 0x83, 0x01, 0x22,
        0xf0,
    ])
    .to_wire();
    let class = bytes::long_chunk(
        archive,
        0x0002_7ffa,
        &[
            bytes::crc_chunk(archive, 0x0002_fffb, &material_uuid),
            bytes::crc_chunk_excluding(
                archive,
                0x0002_fffc,
                &payload,
                &[std::ops::Range {
                    start: 1,
                    end: payload.len(),
                }],
            ),
            bytes::short_chunk(archive, 0x8002_7fff, 0),
        ]
        .concat(),
    );
    bytes::nested_crc_chunk(archive, 0x2000_8040, &class)
}
