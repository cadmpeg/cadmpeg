// SPDX-License-Identifier: Apache-2.0
#![allow(dead_code, clippy::disallowed_methods)]

//! Dump-test byte builders shared by owner suites.

use crate::chunks::{parse_header, ArchiveVersion, TCODE_CRC, TCODE_ENDOFFILE, TCODE_SHORT};
use crate::settings;
use crate::wire::Uuid;
use crate::MAGIC;

pub(crate) fn header(version: &str) -> Vec<u8> {
    let mut bytes = MAGIC.to_vec();
    let mut field = [b' '; 8];
    let start = 8 - version.len();
    field[start..].copy_from_slice(version.as_bytes());
    bytes.extend(field);
    bytes
}

pub(crate) fn long_chunk(archive: ArchiveVersion, typecode: u32, body: &[u8]) -> Vec<u8> {
    let mut bytes = typecode.to_le_bytes().to_vec();
    if archive.uses_eight_byte_values() {
        bytes.extend((body.len() as i64).to_le_bytes());
    } else {
        bytes.extend((body.len() as i32).to_le_bytes());
    }
    bytes.extend(body);
    bytes
}

pub(crate) fn crc_chunk(archive: ArchiveVersion, typecode: u32, body: &[u8]) -> Vec<u8> {
    let mut payload = body.to_vec();
    payload.extend(crc32fast::hash(body).to_le_bytes());
    long_chunk(archive, typecode, &payload)
}

pub(crate) fn crc_chunk_excluding(
    archive: ArchiveVersion,
    typecode: u32,
    body: &[u8],
    children: &[std::ops::Range<usize>],
) -> Vec<u8> {
    let direct = crate::chunks::direct_checksum_ranges(&(0..body.len()), children)
        .expect("valid test child ranges");
    let mut hasher = crc32fast::Hasher::new();
    for range in direct {
        hasher.update(&body[range]);
    }
    let mut payload = body.to_vec();
    payload.extend(hasher.finalize().to_le_bytes());
    long_chunk(archive, typecode, &payload)
}

pub(crate) fn eof(archive: ArchiveVersion, file_size: usize) -> Vec<u8> {
    long_chunk(
        archive,
        TCODE_ENDOFFILE,
        &if archive.uses_eight_byte_values() {
            (file_size as u64).to_le_bytes().to_vec()
        } else {
            (file_size as u32).to_le_bytes().to_vec()
        },
    )
}

pub(crate) fn uuid_bytes() -> Vec<u8> {
    vec![0; 16]
}

pub(crate) fn utf16_bytes(value: &str) -> Vec<u8> {
    let mut units: Vec<u16> = value.encode_utf16().collect();
    units.push(0);
    let mut bytes = (units.len() as u32).to_le_bytes().to_vec();
    for unit in units {
        bytes.extend(unit.to_le_bytes());
    }
    bytes
}

pub(crate) fn fixed_attributes(minor: u8, mode: u8, visible: Option<bool>) -> Vec<u8> {
    let mut bytes = vec![(0x10 | minor)];
    bytes.extend(uuid_bytes());
    bytes.extend((-1_i32).to_le_bytes());
    bytes.extend((-1_i32).to_le_bytes());
    bytes.extend([1, 2, 3, 4]);
    bytes.extend(0_i16.to_le_bytes());
    bytes.extend(0_i16.to_le_bytes());
    bytes.extend(0.0_f64.to_le_bytes());
    bytes.extend(1.0_f64.to_le_bytes());
    bytes.extend(1_i32.to_le_bytes());
    bytes.extend([mode, 0, 0, 0]);
    bytes.extend(utf16_bytes("name"));
    bytes.extend(utf16_bytes("https://example.test"));
    if minor >= 1 {
        bytes.extend(0_i32.to_le_bytes());
    }
    if minor >= 2 {
        bytes.push(u8::from(visible.unwrap_or(true)));
    }
    if minor >= 3 {
        bytes.extend(0_i32.to_le_bytes());
    }
    if minor >= 4 {
        bytes.extend(7_i32.to_le_bytes());
        bytes.push(0);
        bytes.extend([9, 8, 7, 6]);
        bytes.push(0);
        bytes.extend(0.25_f64.to_le_bytes());
    }
    if minor >= 5 {
        bytes.extend(4_i32.to_le_bytes());
    }
    if minor >= 6 {
        bytes.push(1);
        bytes.extend(0_i32.to_le_bytes());
    }
    if minor >= 7 {
        let rendering = crc_chunk(
            ArchiveVersion::V4,
            0x4000_8000,
            &[
                1, 0, 0, 0, 3, 0, 0, 0, // object rendering version 1.3
                0, 0, 0, 0, // material-reference count
                0, 0, 0, 0, // mapping-reference count
                1, 1, 0, // casts shadows, receives shadows, advanced preview
            ],
        );
        bytes.extend(rendering);
    }
    bytes
}

pub(crate) fn tagged_attributes(items: &[(u8, Vec<u8>)], minor: u8) -> Vec<u8> {
    let mut bytes = vec![0x20 | minor];
    bytes.extend(uuid_bytes());
    bytes.extend((-1_i32).to_le_bytes());
    for (item, payload) in items {
        bytes.push(*item);
        bytes.extend(payload);
    }
    bytes.push(0);
    bytes
}

pub(crate) fn descriptor(
    attributes: crate::objects::ObjectAttributes,
    offset: usize,
) -> crate::objects::ObjectDescriptor {
    crate::objects::ObjectDescriptor {
        range: offset..offset + 10,
        object_type: 0,
        class_uuid: Uuid::nil(),
        class_data_range: offset..offset,
        attributes: Some(attributes),
        attributes_degraded: false,
        attributes_userdata: Vec::new(),
        identity: None,
        userdata: Vec::new(),
        history: None,
        unknown_trailer: Vec::new(),
        checksum_warnings: Vec::new(),
        warnings: Vec::new(),
    }
}

pub(crate) fn short_chunk(archive: ArchiveVersion, typecode: u32, value: i64) -> Vec<u8> {
    let mut bytes = (typecode | TCODE_SHORT).to_le_bytes().to_vec();
    if archive.uses_eight_byte_values() {
        bytes.extend(value.to_le_bytes());
    } else {
        bytes.extend((value as i32).to_le_bytes());
    }
    bytes
}

pub(crate) fn table(archive: ArchiveVersion, typecode: u32, records: &[Vec<u8>]) -> Vec<u8> {
    let mut body = records.concat();
    body.extend(short_chunk(archive, crate::chunks::TCODE_ENDOFTABLE, 0));
    long_chunk(archive, typecode, &body)
}

pub(crate) const INSTANCE_DEFINITION_CLASS: [u8; 16] = [
    0xf6, 0xbf, 0xf8, 0x26, 0x18, 0x26, 0x7f, 0x41, 0xa1, 0x58, 0x15, 0x3d, 0x64, 0xa9, 0x49, 0x89,
];

pub(crate) const INSTANCE_REFERENCE_CLASS: [u8; 16] = [
    0x38, 0xb6, 0xcf, 0xf9, 0xd4, 0xb9, 0x40, 0x43, 0x87, 0xe3, 0xc5, 0x6e, 0x78, 0x65, 0xd9, 0x6a,
];

pub(crate) const POINT_CLASS: [u8; 16] = [
    0x1d, 0x1a, 0x10, 0xc3, 0x57, 0xf1, 0xd3, 0x11, 0xbf, 0xe7, 0x00, 0x10, 0x83, 0x01, 0x22, 0xf0,
];

pub(crate) const NURBS_CURVE_CLASS: [u8; 16] = [
    0xdd, 0xd4, 0xd7, 0x4e, 0x47, 0xe9, 0xd3, 0x11, 0xbf, 0xe5, 0x00, 0x10, 0x83, 0x01, 0x22, 0xf0,
];

pub(crate) const ARC_CURVE_CLASS: [u8; 16] = [
    0x2a, 0xbe, 0x33, 0xcf, 0xb4, 0x09, 0xd4, 0x11, 0xbf, 0xfb, 0x00, 0x10, 0x83, 0x01, 0x22, 0xf0,
];

pub(crate) const MESH_CLASS: [u8; 16] = [
    0xe4, 0xd4, 0xd7, 0x4e, 0x47, 0xe9, 0xd3, 0x11, 0xbf, 0xe5, 0x00, 0x10, 0x83, 0x01, 0x22, 0xf0,
];

pub(crate) const SUBD_CLASS: [u8; 16] = [
    0xd9, 0xa4, 0x9b, 0xf0, 0x5b, 0x45, 0xc3, 0x42, 0xba, 0x3b, 0xe6, 0xcc, 0xac, 0xef, 0x85, 0x3b,
];

pub(crate) const REV_SURFACE_CLASS: [u8; 16] = [
    0xd3, 0x20, 0x62, 0xa1, 0x3b, 0x16, 0xd4, 0x11, 0x80, 0x00, 0x00, 0x10, 0x83, 0x01, 0x22, 0xf0,
];

pub(crate) fn anonymous_chunk(archive: ArchiveVersion, minor: i32, body: &[u8]) -> Vec<u8> {
    versioned_anonymous_chunk(archive, 1, minor, body)
}

fn versioned_anonymous_chunk(
    archive: ArchiveVersion,
    major: i32,
    minor: i32,
    body: &[u8],
) -> Vec<u8> {
    let mut payload = major.to_le_bytes().to_vec();
    payload.extend(minor.to_le_bytes());
    payload.extend(body);
    crc_chunk(archive, 0x4000_8000, &payload)
}

pub(crate) fn unit_detail(archive: ArchiveVersion, unit: u32, meters_per_unit: f64) -> Vec<u8> {
    let mut body = unit.to_le_bytes().to_vec();
    body.extend(meters_per_unit.to_le_bytes());
    body.extend(utf16_bytes(""));
    anonymous_chunk(archive, 0, &body)
}

pub(crate) fn content_hash(archive: ArchiveVersion) -> Vec<u8> {
    let mut body = 123_u64.to_le_bytes().to_vec();
    body.extend(456_u64.to_le_bytes());
    body.extend(789_u64.to_le_bytes());
    body.extend(anonymous_chunk(archive, 0, &[0x11; 20]));
    body.extend(anonymous_chunk(archive, 0, &[0x22; 20]));
    anonymous_chunk(archive, 0, &body)
}

pub(crate) fn file_reference(archive: ArchiveVersion, full: &str, relative: &str) -> Vec<u8> {
    let mut body = utf16_bytes(full);
    body.extend(utf16_bytes(relative));
    body.extend(content_hash(archive));
    body.extend(7_u32.to_le_bytes());
    body.extend([0x44; 16]);
    anonymous_chunk(archive, 1, &body)
}

pub(crate) fn model_component_attributes(
    archive: ArchiveVersion,
    id: [u8; 16],
    index: i32,
    name: &str,
) -> Vec<u8> {
    let mut body = vec![1];
    body.extend(11_u32.to_le_bytes());
    body.extend(12_u32.to_le_bytes());
    body.extend(13_u32.to_le_bytes());
    body.push(1);
    body.extend(id);
    body.push(2);
    body.push(1);
    body.extend(index.to_le_bytes());
    body.push(1);
    body.extend(utf16_bytes(name));
    let mut payload = 1_i32.to_le_bytes().to_vec();
    payload.extend(0_i32.to_le_bytes());
    payload.extend(body);
    crc_chunk(archive, 0x4000_8002, &payload)
}

pub(crate) fn reference_settings(archive: ArchiveVersion) -> Vec<u8> {
    let mut implementation_body = 0_i32.to_le_bytes().to_vec();
    implementation_body.extend(0_i32.to_le_bytes());
    implementation_body.push(0);
    let implementation = anonymous_chunk(archive, 0, &implementation_body);
    let mut body = vec![1];
    body.extend(implementation);
    anonymous_chunk(archive, 0, &body)
}

pub(crate) fn definition_record(archive: ArchiveVersion, payload: &[u8]) -> Vec<u8> {
    definition_record_with_userdata(archive, payload, &[])
}

pub(crate) fn definition_record_with_userdata(
    archive: ArchiveVersion,
    payload: &[u8],
    userdata: &[u8],
) -> Vec<u8> {
    let mut uuid_body = INSTANCE_DEFINITION_CLASS.to_vec();
    uuid_body.extend(crc32fast::hash(&INSTANCE_DEFINITION_CLASS).to_le_bytes());
    let uuid = long_chunk(archive, 0x0002_fffb, &uuid_body);
    let class_data = crc_chunk(archive, 0x0002_fffc, payload);
    let class_end = short_chunk(archive, 0x8002_7fff, 0);
    let class = long_chunk(
        archive,
        0x0002_7ffa,
        &[uuid, class_data, userdata.to_vec(), class_end].concat(),
    );
    crc_chunk(archive, 0x2000_8076, &class)
}

pub(crate) fn v5_definition_payload(
    archive: ArchiveVersion,
    minor: u8,
    id: [u8; 16],
    members: &[[u8; 16]],
    linked: bool,
) -> Vec<u8> {
    v5_definition_payload_with_paths(
        archive,
        minor,
        id,
        members,
        linked,
        if linked { "/full/source.3dm" } else { "" },
        false,
    )
}

pub(crate) fn v5_definition_payload_with_paths(
    archive: ArchiveVersion,
    minor: u8,
    id: [u8; 16],
    members: &[[u8; 16]],
    linked: bool,
    linked_path: &str,
    relative_path: bool,
) -> Vec<u8> {
    let mut payload = vec![0x10 | minor];
    payload.extend(id);
    payload.extend((members.len() as i32).to_le_bytes());
    for member in members {
        payload.extend(member);
    }
    payload.extend(utf16_bytes("v5 definition"));
    payload.extend(utf16_bytes("description"));
    payload.extend(utf16_bytes("https://example.test"));
    payload.extend(utf16_bytes("tag"));
    for value in [0.0_f64, 0.0, 0.0, 1.0, 2.0, 3.0] {
        payload.extend(value.to_le_bytes());
    }
    payload.extend(if linked { 3_u32 } else { 0_u32 }.to_le_bytes());
    payload.extend(utf16_bytes(linked_path));
    payload.extend(123_u64.to_le_bytes());
    payload.extend(456_u64.to_le_bytes());
    for value in 0_u32..8 {
        payload.extend(value.to_le_bytes());
    }
    payload.extend(2_u32.to_le_bytes());
    payload.extend(0.001_f64.to_le_bytes());
    payload.push(u8::from(relative_path));
    payload.extend(unit_detail(archive, 2, 0.001));
    payload.extend(1_i32.to_le_bytes());
    payload.extend(0_u32.to_le_bytes());
    if minor >= 7 {
        payload.push(u8::from(linked));
        if linked {
            payload.extend(file_reference(archive, "/full/source.3dm", "source.3dm"));
        }
        payload.push(0);
    }
    payload
}

pub(crate) fn v6_definition_payload(
    archive: ArchiveVersion,
    id: [u8; 16],
    members: &[[u8; 16]],
    kind: u32,
    linked: bool,
    settings: bool,
) -> Vec<u8> {
    let mut body = model_component_attributes(archive, id, 17, "modern definition");
    body.extend(kind.to_le_bytes());
    body.extend(unit_detail(archive, 8, 0.0254));
    body.extend(utf16_bytes("description"));
    body.extend(utf16_bytes("https://example.test"));
    body.extend(utf16_bytes("tag"));
    for value in [0.0_f64, 0.0, 0.0, 4.0, 5.0, 6.0] {
        body.extend(value.to_le_bytes());
    }
    let members_present = kind != 3;
    body.push(u8::from(members_present));
    if members_present {
        body.extend((members.len() as i32).to_le_bytes());
        for member in members {
            body.extend(member);
        }
    }
    body.push(u8::from(linked));
    if linked {
        let mut linked_body = file_reference(archive, "/full/source.3dm", "source.3dm");
        linked_body.extend(2_i32.to_le_bytes());
        linked_body.extend(2_u32.to_le_bytes());
        linked_body.push(u8::from(settings));
        if settings {
            linked_body.extend(reference_settings(archive));
        }
        body.extend(anonymous_chunk(archive, 0, &linked_body));
    }
    anonymous_chunk(archive, 0, &body)
}

pub(crate) fn document_with_definitions(
    version: &str,
    archive: ArchiveVersion,
    definitions: &[Vec<u8>],
    objects: &[Vec<u8>],
) -> Vec<u8> {
    minimal_document(
        version,
        &[
            table(archive, 0x1000_0014, &[]),
            table(archive, 0x1000_0015, &[]),
            table(archive, 0x1000_0021, definitions),
            table(archive, 0x1000_0013, objects),
        ],
    )
}

pub(crate) fn crc_table(archive: ArchiveVersion, typecode: u32, records: &[Vec<u8>]) -> Vec<u8> {
    let mut body = records.concat();
    body.extend(short_chunk(archive, crate::chunks::TCODE_ENDOFTABLE, 0));
    nested_crc_chunk(archive, typecode | TCODE_CRC, &body)
}

pub(crate) fn nested_crc_chunk(archive: ArchiveVersion, typecode: u32, body: &[u8]) -> Vec<u8> {
    let mut payload = body.to_vec();
    payload.extend(0_u32.to_le_bytes());
    long_chunk(archive, typecode, &payload)
}

pub(crate) fn object_record(
    archive: ArchiveVersion,
    object_type: i64,
    class_uuid: [u8; 16],
) -> Vec<u8> {
    object_record_with_payload(archive, object_type, class_uuid, &[])
}

pub(crate) fn object_record_with_payload(
    archive: ArchiveVersion,
    object_type: i64,
    class_uuid: [u8; 16],
    payload: &[u8],
) -> Vec<u8> {
    let object_type = short_chunk(archive, 0x8200_0071, object_type);
    let mut uuid_body = class_uuid.to_vec();
    uuid_body.extend(crc32fast::hash(&class_uuid).to_le_bytes());
    let uuid = long_chunk(archive, 0x0002_fffb, &uuid_body);
    let class_data = crc_chunk(archive, 0x0002_fffc, payload);
    let class_end = short_chunk(archive, 0x8002_7fff, 0);
    let class = long_chunk(
        archive,
        0x0002_7ffa,
        &[uuid, class_data, class_end].concat(),
    );
    let object_end = short_chunk(archive, 0x8200_007f, 0);
    nested_crc_chunk(
        archive,
        0x2000_8070 | TCODE_CRC,
        &[object_type, class, object_end].concat(),
    )
}

pub(crate) fn object_record_with_attribute_userdata(
    archive: ArchiveVersion,
    object_type: i64,
    class_uuid: [u8; 16],
    attributes: &[u8],
    userdata: &[u8],
) -> Vec<u8> {
    let object_type = short_chunk(archive, 0x8200_0071, object_type);
    let mut uuid_body = class_uuid.to_vec();
    uuid_body.extend(crc32fast::hash(&class_uuid).to_le_bytes());
    let uuid = long_chunk(archive, 0x0002_fffb, &uuid_body);
    let class_data = crc_chunk(archive, 0x0002_fffc, &[]);
    let class_end = short_chunk(archive, 0x8002_7fff, 0);
    let class = long_chunk(
        archive,
        0x0002_7ffa,
        &[uuid, class_data, class_end].concat(),
    );
    let attributes = crc_chunk(archive, 0x0200_8072, attributes);
    let attribute_userdata = long_chunk(
        archive,
        0x0200_0073,
        &[userdata, &short_chunk(archive, 0x8002_7fff, 0)].concat(),
    );
    let object_end = short_chunk(archive, 0x8200_007f, 0);
    nested_crc_chunk(
        archive,
        0x2000_8070 | TCODE_CRC,
        &[
            object_type,
            class,
            attributes,
            attribute_userdata,
            object_end,
        ]
        .concat(),
    )
}

pub(crate) fn class_wrapper(
    archive: ArchiveVersion,
    class_uuid: [u8; 16],
    payload: &[u8],
) -> Vec<u8> {
    let mut uuid_body = class_uuid.to_vec();
    uuid_body.extend(crc32fast::hash(&class_uuid).to_le_bytes());
    let uuid = long_chunk(archive, 0x0002_fffb, &uuid_body);
    let class_data = crc_chunk(archive, 0x0002_fffc, payload);
    let class_end = short_chunk(archive, 0x8002_7fff, 0);
    long_chunk(
        archive,
        0x0002_7ffa,
        &[uuid, class_data, class_end].concat(),
    )
}

pub(crate) fn class_wrapper_with_userdata(
    archive: ArchiveVersion,
    class_uuid: [u8; 16],
    payload: &[u8],
    userdata: &[u8],
) -> Vec<u8> {
    let mut uuid_body = class_uuid.to_vec();
    uuid_body.extend(crc32fast::hash(&uuid_body).to_le_bytes());
    let uuid = long_chunk(archive, 0x0002_fffb, &uuid_body);
    let class_data = crc_chunk(archive, 0x0002_fffc, payload);
    let class_end = short_chunk(archive, 0x8002_7fff, 0);
    long_chunk(
        archive,
        0x0002_7ffa,
        &[uuid, class_data, userdata.to_vec(), class_end].concat(),
    )
}

pub(crate) fn class_userdata(
    archive: ArchiveVersion,
    class_uuid: [u8; 16],
    application_uuid: [u8; 16],
    path: &str,
    relative: bool,
) -> Vec<u8> {
    let mut userdata_body = utf16_bytes(path);
    userdata_body.push(u8::from(relative));
    class_userdata_with_anonymous_payload(archive, class_uuid, application_uuid, 1, &userdata_body)
}

pub(crate) fn class_userdata_with_payload(
    archive: ArchiveVersion,
    class_uuid: [u8; 16],
    application_uuid: [u8; 16],
    userdata_body: &[u8],
) -> Vec<u8> {
    class_userdata_with_anonymous_payload(archive, class_uuid, application_uuid, 1, userdata_body)
}

pub(crate) fn class_userdata_with_anonymous_payload(
    archive: ArchiveVersion,
    class_uuid: [u8; 16],
    application_uuid: [u8; 16],
    major: i32,
    userdata_body: &[u8],
) -> Vec<u8> {
    let userdata_payload = versioned_anonymous_chunk(archive, major, 0, userdata_body);
    let mut transform = Vec::with_capacity(16 * 8);
    for index in 0..16 {
        let value: f64 = if index % 5 == 0 { 1.0 } else { 0.0 };
        transform.extend(value.to_le_bytes());
    }
    let header_body = [
        class_uuid.to_vec(),
        class_uuid.to_vec(),
        1_i32.to_le_bytes().to_vec(),
        transform,
        application_uuid.to_vec(),
        vec![0],
        50_i32.to_le_bytes().to_vec(),
        0_i32.to_le_bytes().to_vec(),
    ]
    .concat();
    let header = crc_chunk(archive, 0x0002_fff9, &header_body);
    let mut body = vec![0x22];
    let header_range_start = body.len();
    body.extend(header);
    let header_range_end = body.len();
    let payload_range_start = body.len();
    body.extend(crc_chunk(archive, 0x4000_8000, &userdata_payload));
    let payload_range_end = body.len();
    crc_chunk_excluding(
        archive,
        0x0002_7ffd,
        &body,
        &[
            header_range_start..header_range_end,
            payload_range_start..payload_range_end,
        ],
    )
}

pub(crate) fn class_userdata_v1_with_direct_payload(
    archive: ArchiveVersion,
    class_uuid: [u8; 16],
    userdata_body: &[u8],
) -> Vec<u8> {
    let mut transform = Vec::with_capacity(16 * 8);
    for index in 0..16 {
        let value: f64 = if index % 5 == 0 { 1.0 } else { 0.0 };
        transform.extend(value.to_le_bytes());
    }
    let mut body = vec![0x10];
    body.extend(class_uuid);
    body.extend(class_uuid);
    body.extend(0_i32.to_le_bytes());
    body.extend(transform);
    let payload_start = body.len();
    body.extend(crc_chunk(archive, 0x4000_8000, userdata_body));
    let payload_end = body.len();
    let payload_range = payload_start..payload_end;
    crc_chunk_excluding(
        archive,
        0x0002_7ffd,
        &body,
        std::slice::from_ref(&payload_range),
    )
}

pub(crate) fn class_userdata_v2_with_direct_payload(
    archive: ArchiveVersion,
    class_uuid: [u8; 16],
    application_uuid: [u8; 16],
    archive_version: i32,
    writer_version: u32,
    userdata_body: &[u8],
) -> Vec<u8> {
    class_userdata_v2_with_class_and_item_direct_payload(
        archive,
        class_uuid,
        class_uuid,
        application_uuid,
        archive_version,
        writer_version,
        userdata_body,
    )
}

pub(crate) fn class_userdata_v2_with_class_and_item_direct_payload(
    archive: ArchiveVersion,
    class_uuid: [u8; 16],
    item_uuid: [u8; 16],
    application_uuid: [u8; 16],
    archive_version: i32,
    writer_version: u32,
    userdata_body: &[u8],
) -> Vec<u8> {
    let mut transform = Vec::with_capacity(16 * 8);
    for index in 0..16 {
        let value: f64 = if index % 5 == 0 { 1.0 } else { 0.0 };
        transform.extend(value.to_le_bytes());
    }
    let header_body = [
        class_uuid.to_vec(),
        item_uuid.to_vec(),
        0_i32.to_le_bytes().to_vec(),
        transform,
        application_uuid.to_vec(),
        vec![0],
        archive_version.to_le_bytes().to_vec(),
        writer_version.to_le_bytes().to_vec(),
    ]
    .concat();
    let header = crc_chunk(archive, 0x0002_fff9, &header_body);
    let mut body = vec![0x22];
    let header_range_start = body.len();
    body.extend(header);
    let header_range_end = body.len();
    let payload_range_start = body.len();
    body.extend(crc_chunk(archive, 0x4000_8000, userdata_body));
    let payload_range_end = body.len();
    crc_chunk_excluding(
        archive,
        0x0002_7ffd,
        &body,
        &[
            header_range_start..header_range_end,
            payload_range_start..payload_range_end,
        ],
    )
}

pub(crate) fn mesh_parameters(archive: ArchiveVersion) -> Vec<u8> {
    let mut body = vec![0x15];
    body.extend(1_i32.to_le_bytes());
    body.extend(1_i32.to_le_bytes());
    body.extend(0_i32.to_le_bytes());
    body.extend(1_i32.to_le_bytes());
    body.extend((-17_i32).to_le_bytes());
    body.extend(0.125_f64.to_le_bytes());
    body.extend(0.25_f64.to_le_bytes());
    body.extend(8.5_f64.to_le_bytes());
    body.extend(3.5_f64.to_le_bytes());
    body.extend(2_i32.to_le_bytes());
    body.extend(12_i32.to_le_bytes());
    body.extend(0.25_f64.to_le_bytes());
    body.extend(1.75_f64.to_le_bytes());
    body.extend(0.5_f64.to_le_bytes());
    body.extend(0.75_f64.to_le_bytes());
    body.extend(2_i32.to_le_bytes());
    body.extend(2_i32.to_le_bytes());
    body.push(0);
    body.extend(0.03125_f64.to_le_bytes());
    body.push(1);
    body.push(0);
    body.extend(anonymous_chunk(
        archive,
        3,
        &[
            5_i32.to_le_bytes().as_slice(),
            2_i32.to_le_bytes().as_slice(),
            [1].as_slice(),
            [0].as_slice(),
        ]
        .concat(),
    ));
    body
}

pub(crate) fn object_record_without_end(
    archive: ArchiveVersion,
    object_type: i64,
    class_uuid: [u8; 16],
) -> Vec<u8> {
    let object_type = short_chunk(archive, 0x8200_0071, object_type);
    let mut uuid_body = class_uuid.to_vec();
    uuid_body.extend(crc32fast::hash(&class_uuid).to_le_bytes());
    let uuid = long_chunk(archive, 0x0002_fffb, &uuid_body);
    let class_data = crc_chunk(archive, 0x0002_fffc, &[]);
    let class_end = short_chunk(archive, 0x8002_7fff, 0);
    let class = long_chunk(
        archive,
        0x0002_7ffa,
        &[uuid, class_data, class_end].concat(),
    );
    nested_crc_chunk(
        archive,
        0x2000_8070 | TCODE_CRC,
        &[object_type, class].concat(),
    )
}

pub(crate) fn object_record_with_unknown_trailer(
    archive: ArchiveVersion,
    class_uuid: [u8; 16],
) -> Vec<u8> {
    let object_type = short_chunk(archive, 0x8200_0071, 1);
    let mut uuid_body = class_uuid.to_vec();
    uuid_body.extend(crc32fast::hash(&class_uuid).to_le_bytes());
    let uuid = long_chunk(archive, 0x0002_fffb, &uuid_body);
    let class_data = crc_chunk(archive, 0x0002_fffc, &[]);
    let class_end = short_chunk(archive, 0x8002_7fff, 0);
    let class = long_chunk(
        archive,
        0x0002_7ffa,
        &[uuid, class_data, class_end].concat(),
    );
    let unknown = long_chunk(archive, 0x0200_1000, &[1, 2, 3]);
    let object_end = short_chunk(archive, 0x8200_007f, 0);
    nested_crc_chunk(
        archive,
        0x2000_8070 | TCODE_CRC,
        &[object_type, class, unknown, object_end].concat(),
    )
}

pub(crate) fn minimal_document(version: &str, tables: &[Vec<u8>]) -> Vec<u8> {
    let archive = parse_header(&header(version))
        .expect("required invariant")
        .archive_version;
    let mut bytes = header(version);
    bytes.extend(long_chunk(archive, 1, b"comment"));
    for table in tables {
        bytes.extend(table);
    }
    let eof_offset = bytes.len();
    bytes.extend(eof(archive, 0));
    let marker = eof(archive, bytes.len());
    bytes[eof_offset..].copy_from_slice(&marker);
    bytes
}

pub(crate) fn metadata_record(typecode: u32, data: Vec<u8>) -> (Vec<u8>, crate::container::Record) {
    let length = data.len();
    (
        data,
        crate::container::Record {
            typecode,
            range: 0..length,
            body: 0..length,
            short: false,
            value: 0,
        },
    )
}

pub(crate) fn set_test_units(scan: &mut crate::container::Scan<'_>, scale: f64) {
    scan.metadata.settings.units = Some(settings::UnitsAndTolerances {
        version: 1,
        unit_value: 2,
        unit: settings::UnitSystem::Standard(2),
        millimeters_per_unit: Some(scale),
        absolute_tolerance: 0.01,
        absolute_tolerance_millimeters: Some(0.01 * scale),
        angular_tolerance: 0.1,
        relative_tolerance: 0.01,
        distance_display_mode: None,
        distance_display_precision: None,
        source: settings::SourceRange { range: 0..0 },
    });
}

pub(crate) fn point_payload(point: [f64; 3]) -> Vec<u8> {
    let mut payload = vec![0x10];
    for value in point {
        payload.extend(value.to_le_bytes());
    }
    payload
}

pub(crate) fn nurbs_curve_payload(points: [[f64; 3]; 2]) -> Vec<u8> {
    let mut payload = vec![0x10];
    payload.extend(3_i32.to_le_bytes());
    payload.extend(0_i32.to_le_bytes());
    payload.extend(2_i32.to_le_bytes());
    payload.extend(2_i32.to_le_bytes());
    payload.extend(0_i32.to_le_bytes());
    payload.extend(0_i32.to_le_bytes());
    for value in [0.0_f64, 0.0, 0.0, 1.0, 1.0, 1.0] {
        payload.extend(value.to_le_bytes());
    }
    payload.extend(2_i32.to_le_bytes());
    payload.extend(0.0_f64.to_le_bytes());
    payload.extend(1.0_f64.to_le_bytes());
    payload.extend(2_i32.to_le_bytes());
    for point in points {
        for value in point {
            payload.extend(value.to_le_bytes());
        }
    }
    payload
}

pub(crate) fn circle_payload() -> Vec<u8> {
    let mut payload = vec![0x10];
    for value in [
        0.0_f64,
        0.0,
        0.0, // origin
        1.0,
        0.0,
        0.0, // x
        0.0,
        1.0,
        0.0, // y
        0.0,
        0.0,
        1.0, // z
        0.0,
        0.0,
        1.0,
        0.0, // equation
        1.0, // radius
        1.0,
        0.0,
        0.0, // zero
        0.0,
        1.0,
        0.0, // half pi
        -1.0,
        0.0,
        0.0, // pi
        0.0,
        std::f64::consts::TAU, // angle
        0.0,
        std::f64::consts::TAU, // domain
    ] {
        payload.extend(value.to_le_bytes());
    }
    payload.extend(3_i32.to_le_bytes());
    payload
}

pub(crate) fn mesh_payload() -> Vec<u8> {
    let mut payload = vec![0x10];
    payload.extend(3_i32.to_le_bytes());
    payload.extend(1_i32.to_le_bytes());
    for _ in 0..4 {
        payload.extend(0.0_f64.to_le_bytes());
        payload.extend(1.0_f64.to_le_bytes());
    }
    payload.extend(0.0_f64.to_le_bytes());
    payload.extend(0.0_f64.to_le_bytes());
    for _ in 0..16 {
        payload.extend(0.0_f32.to_le_bytes());
    }
    payload.extend(0_i32.to_le_bytes());
    payload.push(0);
    payload.extend([0; 4]);
    payload.extend(1_i32.to_le_bytes());
    payload.extend([0, 1, 2, 2]);
    payload.extend(3_i32.to_le_bytes());
    for value in [0.0_f32, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 1.0, 0.0] {
        payload.extend(value.to_le_bytes());
    }
    payload.extend(3_i32.to_le_bytes());
    for _ in 0..3 {
        for value in [1.0_f32, 0.0, 1.0] {
            payload.extend(value.to_le_bytes());
        }
    }
    payload.extend(0_i32.to_le_bytes());
    payload.extend(0_i32.to_le_bytes());
    payload.extend(0_i32.to_le_bytes());
    payload
}

pub(crate) fn instance_reference_payload(definition_id: [u8; 16], rows: [[f64; 4]; 4]) -> Vec<u8> {
    let mut payload = vec![0x10];
    payload.extend(definition_id);
    for value in rows.into_iter().flatten() {
        payload.extend(value.to_le_bytes());
    }
    for value in [0.0_f64, 0.0, 0.0, 1.0, 1.0, 1.0] {
        payload.extend(value.to_le_bytes());
    }
    payload
}

pub(crate) fn transform(scale_x: f64, translation: [f64; 3]) -> [[f64; 4]; 4] {
    [
        [scale_x, 0.0, 0.0, translation[0]],
        [0.0, 1.0, 0.0, translation[1]],
        [0.0, 0.0, 1.0, translation[2]],
        [0.0, 0.0, 0.0, 1.0],
    ]
}

pub(crate) fn static_definition(
    id: [u8; 16],
    members: &[[u8; 16]],
) -> crate::instances::InstanceDefinition {
    crate::instances::InstanceDefinition {
        source_range: 0..0,
        id: Uuid::from_wire(id),
        members: members.iter().copied().map(Uuid::from_wire).collect(),
        index: None,
        name: String::new(),
        description: String::new(),
        url: String::new(),
        url_tag: String::new(),
        kind: crate::instances::DefinitionKind::Static,
        units: crate::instances::UnitDetail {
            unit: 2,
            meters_per_unit: 0.001,
            custom_name: String::new(),
        },
        legacy_linked_path: String::new(),
        legacy_relative_linked_path: String::new(),
        legacy_checksum_range: None,
        legacy_relative_path: false,
        linked_depth: 0,
        linked_appearance: 0,
        file_reference_range: None,
        file_reference: None,
        reference_settings_range: None,
    }
}

pub(crate) fn set_identity(
    scan: &mut crate::container::Scan<'_>,
    source_order: usize,
    object_id: [u8; 16],
    source_key: &str,
    color: Option<[u8; 4]>,
    visible: bool,
) {
    let object = scan.objects[source_order]
        .framed_mut()
        .expect("test object is framed");
    object.identity = Some(crate::objects::SourceIdentity {
        source_id: format!("rhino:object:record#{source_key}"),
        object_id: Uuid::from_wire(object_id),
        class_uuid: object.class_uuid,
        name: String::new(),
        layer_index: -1,
        layer_id: None,
        layer_name: None,
        effective_color: color,
        effective_visible: visible,
        object_mode: 0,
        definition_member: false,
        object_frame: None,
        source: settings::SourceRange {
            range: object.range.clone(),
        },
    });
}

pub(crate) fn scan_with_objects(objects: &[Vec<u8>]) -> crate::container::Scan<'static> {
    let archive = ArchiveVersion::V5;
    let bytes = minimal_document(
        "50",
        &[
            table(archive, 0x1000_0014, &[]),
            table(archive, 0x1000_0015, &[]),
            table(archive, 0x1000_0013, objects),
        ],
    );
    let mut scan = crate::container::scan_owned(bytes).expect("required invariant");
    set_test_units(&mut scan, 1.0);
    scan
}

pub(crate) fn install_definitions(
    scan: &mut crate::container::Scan<'_>,
    definitions: Vec<crate::instances::InstanceDefinition>,
) {
    scan.definitions.definitions = definitions;
    scan.definitions.member_object_ids = scan
        .definitions
        .definitions
        .iter()
        .flat_map(|definition| definition.members.iter().copied())
        .collect();
}

/// Wire form of the object UUID that `polyedge::tests::polyedge_payload`
/// references from its single segment.
pub(crate) const POLYEDGE_SEGMENT_TARGET: [u8; 16] = {
    let mut wire = [0; 16];
    wire[15] = 9;
    wire
};

pub(crate) fn polyedge_scan_objects() -> Vec<Vec<u8>> {
    let archive = ArchiveVersion::V5;
    vec![
        object_record_with_payload(archive, 1, POINT_CLASS, &point_payload([1.0, 2.0, 3.0])),
        object_record_with_payload(
            archive,
            4,
            polyedge_class_wire(),
            &crate::polyedge::tests::polyedge_payload(),
        ),
    ]
}

pub(crate) fn polyedge_class_wire() -> [u8; 16] {
    crate::polyedge::CURVE_CLASS.to_wire()
}

pub(crate) fn polyedge_segment_parameter(
    result: &cadmpeg_ir::codec::DecodeResult,
) -> Option<String> {
    let feature = result
        .ir()
        .model
        .features
        .iter()
        .find(|feature| feature.source_tag.as_deref() == Some("RhinoPolyEdgeReference"))?;
    let cadmpeg_ir::features::FeatureDefinition::Native { parameters, .. } = &feature.definition
    else {
        return None;
    };
    parameters.get("segment_0_object").cloned()
}
