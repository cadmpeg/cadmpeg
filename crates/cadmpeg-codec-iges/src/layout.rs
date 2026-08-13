// SPDX-License-Identifier: Apache-2.0
//! IGES representation dispatch and unsupported-layout inspection.

use crate::card;
use cadmpeg_core::{CodecError, ContainerEntry, ContainerSummary, ReadSeek};
use cadmpeg_ir::codec::Confidence;
use std::collections::BTreeMap;
use std::io::{ErrorKind, SeekFrom};

const DETECTION_PREFIX_BYTES: usize = 512;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Representation {
    FixedAscii,
    CompressedAscii,
    Binary,
    Unknown,
}

fn compressed_ascii(prefix: &[u8]) -> bool {
    let Some(flag) = prefix.get(..80) else {
        return false;
    };
    flag[72] == b'C' && flag.iter().all(|byte| (b' '..=b'~').contains(byte))
}

fn binary(prefix: &[u8]) -> bool {
    let Some(flag) = prefix.get(..80) else {
        return false;
    };
    flag[0] == b'B'
        && flag[1..5] == 75_u32.to_be_bytes()
        && flag[11] == b'B'
        && flag[16] == b'S'
        && flag[21] == b'G'
        && flag[26] == b'D'
        && flag[31] == b'P'
        && flag[36] == b'T'
        && flag[72] == b'B'
        && flag[73..79].iter().all(|byte| matches!(byte, b' ' | b'0'))
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
    let mut count = 0;
    while count < prefix.len() {
        match reader.read(&mut prefix[count..]) {
            Ok(0) => break,
            Ok(read) => count += read,
            Err(error) if error.kind() == ErrorKind::Interrupted => {}
            Err(error) => return Err(CodecError::Io(error)),
        }
    }
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

#[cfg(test)]
mod tests;
