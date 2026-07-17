// SPDX-License-Identifier: Apache-2.0
//! IGES representation dispatch and unsupported-layout inspection.

use crate::card;
use cadmpeg_ir::codec::{CodecError, Confidence, ContainerEntry, ContainerSummary, ReadSeek};
use std::collections::BTreeMap;
use std::io::SeekFrom;

const DETECTION_PREFIX_BYTES: usize = 512;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Representation {
    FixedAscii,
    CompressedAscii,
    Binary,
    Unknown,
}

fn physical_line(input: &[u8]) -> Option<(&[u8], &[u8])> {
    let end = input
        .iter()
        .position(|byte| matches!(byte, b'\r' | b'\n'))?;
    let ending = usize::from(input[end] == b'\r' && input.get(end + 1) == Some(&b'\n')) + 1;
    Some((&input[..end], &input[end + ending..]))
}

fn compressed_ascii(prefix: &[u8]) -> bool {
    let Some((flag, rest)) = physical_line(prefix) else {
        return false;
    };
    let Some((start, _)) = physical_line(rest) else {
        return false;
    };
    flag.len() == 80
        && flag[72] == b'C'
        && flag.iter().all(|byte| (b' '..=b'~').contains(byte))
        && start.len() == 80
        && start[72] == b'S'
        && start[73..80] == *b"      1"
}

fn binary(prefix: &[u8]) -> bool {
    let Some(flag) = prefix.get(..80) else {
        return false;
    };
    let count = &flag[1..5];
    flag[0] == b'B'
        && (count == 75_u32.to_be_bytes() || count == 75_u32.to_le_bytes())
        && flag[72] == b'B'
        && flag[79] == b'1'
}

pub(crate) fn classify_prefix(prefix: &[u8]) -> Representation {
    if compressed_ascii(prefix) {
        Representation::CompressedAscii
    } else if binary(prefix) {
        Representation::Binary
    } else if card::detect_fixed_ascii(prefix) == Confidence::High {
        Representation::FixedAscii
    } else {
        Representation::Unknown
    }
}

pub(crate) fn confidence(prefix: &[u8]) -> Confidence {
    match classify_prefix(prefix) {
        Representation::FixedAscii | Representation::CompressedAscii | Representation::Binary => {
            Confidence::High
        }
        Representation::Unknown => Confidence::No,
    }
}

pub(crate) fn classify(reader: &mut dyn ReadSeek) -> Result<Representation, CodecError> {
    let position = reader.stream_position()?;
    let mut prefix = vec![0; DETECTION_PREFIX_BYTES];
    let count = reader.read(&mut prefix)?;
    prefix.truncate(count);
    reader.seek(SeekFrom::Start(position))?;
    Ok(classify_prefix(&prefix))
}

pub(crate) fn unsupported_summary(representation: Representation) -> ContainerSummary {
    let kind = match representation {
        Representation::CompressedAscii => "compressed-ascii",
        Representation::Binary => "binary",
        Representation::FixedAscii | Representation::Unknown => "unknown",
    };
    ContainerSummary {
        format: "iges".into(),
        container_kind: kind.into(),
        entries: vec![ContainerEntry {
            name: "flag".into(),
            role: "representation-flag".into(),
            compression: "none".into(),
            compressed_size: 80,
            uncompressed_size: 80,
            attributes: BTreeMap::from([("representation".into(), kind.into())]),
        }],
        notes: vec![format!("unsupported_representation={kind}")],
    }
}

pub(crate) fn unsupported_error(representation: Representation) -> CodecError {
    let name = match representation {
        Representation::CompressedAscii => "Compressed ASCII",
        Representation::Binary => "Binary",
        Representation::FixedAscii => "Fixed ASCII",
        Representation::Unknown => "unknown",
    };
    CodecError::NotImplemented(format!("IGES {name} representation decode"))
}
