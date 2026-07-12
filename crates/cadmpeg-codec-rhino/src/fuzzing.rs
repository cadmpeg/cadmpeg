// SPDX-License-Identifier: Apache-2.0
//! Feature-gated entry points for focused parser fuzzing.

use std::mem::size_of;

use crate::chunks::{self, ArchiveVersion};
use crate::container::Record;
use crate::wire::Uuid;

const ARCHIVES: [ArchiveVersion; 9] = [
    ArchiveVersion::V1,
    ArchiveVersion::V2,
    ArchiveVersion::V3,
    ArchiveVersion::V4,
    ArchiveVersion::LegacyV5,
    ArchiveVersion::V5,
    ArchiveVersion::V6,
    ArchiveVersion::V7,
    ArchiveVersion::V8,
];

fn selected_archive(selector: u8) -> ArchiveVersion {
    ARCHIVES[usize::from(selector) % ARCHIVES.len()]
}

fn uuid(mut canonical: [u8; 16]) -> Uuid {
    canonical[..4].reverse();
    canonical[4..6].reverse();
    canonical[6..8].reverse();
    Uuid::from_wire(canonical)
}

/// Exercises header, table, record, and EOF framing.
pub fn container(data: &[u8]) {
    let _ = crate::container::scan(data.to_vec());
}

/// Exercises chunk framing at sequential and arbitrary bounded offsets.
pub fn chunks(data: &[u8]) {
    if data.is_empty() {
        return;
    }
    let selected_offset = data
        .iter()
        .take(size_of::<usize>())
        .fold(0_usize, |value, byte| {
            value.rotate_left(8) ^ usize::from(*byte)
        })
        % data.len();
    for archive in ARCHIVES {
        for offset in [0, selected_offset] {
            if let Ok(chunk) = chunks::chunk_at(data, offset, data.len(), archive, false) {
                let _ = chunks::verify_checksum(data, &chunk);
            }
        }
    }
    let Ok(header) = chunks::parse_header(data) else {
        return;
    };
    let mut offset = 32;
    for _ in 0..1024 {
        let Ok(chunk) = chunks::chunk_at(data, offset, data.len(), header.archive_version, false)
        else {
            break;
        };
        let _ = chunks::verify_checksum(data, &chunk);
        if chunk.next_offset <= offset {
            break;
        }
        offset = chunk.next_offset;
    }
}

/// Exercises object-record, class, userdata, and attribute framing.
pub fn object_record(data: &[u8]) {
    if data.len() < 2 {
        return;
    }
    let record = Record {
        typecode: 0x2000_8070,
        range: 1..data.len(),
        body: 1..data.len(),
        short: false,
        value: 0,
    };
    let mut warnings = Vec::new();
    let _ = crate::objects::parse_object_record(
        data,
        &record,
        selected_archive(data[0]),
        &mut warnings,
    );
}

/// Exercises NURBS curve, surface, and plane payload reconstruction.
pub fn nurbs(data: &[u8]) {
    if data.len() < 3 {
        return;
    }
    let class = match data[0] % 3 {
        0 => uuid([
            0x4e, 0xd7, 0xd4, 0xdd, 0xe9, 0x47, 0x11, 0xd3, 0xbf, 0xe5, 0x00, 0x10, 0x83, 0x01,
            0x22, 0xf0,
        ]),
        1 => uuid([
            0x4e, 0xd7, 0xd4, 0xde, 0xe9, 0x47, 0x11, 0xd3, 0xbf, 0xe5, 0x00, 0x10, 0x83, 0x01,
            0x22, 0xf0,
        ]),
        _ => uuid([
            0x4e, 0xd7, 0xd4, 0xdf, 0xe9, 0x47, 0x11, 0xd3, 0xbf, 0xe5, 0x00, 0x10, 0x83, 0x01,
            0x22, 0xf0,
        ]),
    };
    let _ = crate::curves::decode(data, class, 2..data.len(), 1.0, selected_archive(data[1]));
}

/// Exercises compressed-buffer inflation and checksum handling.
pub fn mesh_buffer(data: &[u8]) {
    crate::mesh::fuzz_buffer(data);
}

/// Exercises `RawBrep` framing and semantic validation.
pub fn brep(data: &[u8]) {
    if data.len() < 2 {
        return;
    }
    let _ = crate::brep::parse(data, 1..data.len(), selected_archive(data[0]), None);
}

/// Exercises `SubD` framing, archive ID maps, and directed rings.
pub fn subd(data: &[u8]) {
    if data.len() < 2 {
        return;
    }
    let id = "rhino:fuzz:subd#0".into();
    let _ = crate::subd::decode(data, 1..data.len(), selected_archive(data[0]), 1.0, id);
}
