// SPDX-License-Identifier: Apache-2.0
//! IGES representation dispatch and physical-envelope classification.

use crate::card;
use crate::layout::binary_flag;
use cadmpeg_core::{CodecError, ReadSeek};
use cadmpeg_ir::codec::Confidence;
use std::io::{ErrorKind, SeekFrom};

const DETECTION_PREFIX_BYTES: usize = 512;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Representation {
    FixedAscii,
    CompressedAscii,
    Binary,
}

impl Representation {
    /// The discriminant value `docs/dialects.toml` states for this
    /// representation, and the value the `representation` source attribute and
    /// the non-Fixed-ASCII container kind carry.
    pub(crate) const fn as_str(self) -> &'static str {
        match self {
            Self::FixedAscii => "fixed-ascii",
            Self::CompressedAscii => "compressed-ascii",
            Self::Binary => "binary",
        }
    }
}

fn compressed_ascii(prefix: &[u8]) -> bool {
    let Some(flag) = prefix.get(..80) else {
        return false;
    };
    flag[72] == b'C' && flag.iter().all(|byte| (b' '..=b'~').contains(byte))
}

fn binary(prefix: &[u8]) -> bool {
    let Some(flag) = prefix.get(..binary_flag::LEN) else {
        return false;
    };
    flag[binary_flag::IDENTIFIER] == b'B'
        && flag[binary_flag::REMAINING_BYTE_COUNT..binary_flag::PRIMITIVE_BIT_LENGTHS]
            == binary_flag::REMAINING_BYTE_COUNT_VALUE.to_be_bytes()
        && flag[binary_flag::SECTION_DISPLACEMENTS] == b'B'
        && flag[16] == b'S'
        && flag[21] == b'G'
        && flag[26] == b'D'
        && flag[31] == b'P'
        && flag[36] == b'T'
        && flag[binary_flag::SECTION_MARKER] == b'B'
        && flag[binary_flag::SEQUENCE_PADDING..binary_flag::SEQUENCE]
            .iter()
            .all(|byte| matches!(byte, b' ' | b'0'))
        && flag[binary_flag::SEQUENCE] == b'1'
}

pub(crate) fn classify_prefix(prefix: &[u8]) -> Option<Representation> {
    if compressed_ascii(prefix) {
        Some(Representation::CompressedAscii)
    } else if binary(prefix) {
        Some(Representation::Binary)
    } else if card::detect_fixed_ascii(prefix) == Confidence::High {
        Some(Representation::FixedAscii)
    } else {
        None
    }
}

pub(crate) fn confidence(prefix: &[u8]) -> Confidence {
    match classify_prefix(prefix) {
        Some(_) => Confidence::High,
        None => Confidence::No,
    }
}

pub(crate) fn classify(reader: &mut dyn ReadSeek) -> Result<Option<Representation>, CodecError> {
    let position = reader.stream_position()?;
    let mut prefix = [0; DETECTION_PREFIX_BYTES];
    let mut count = 0;
    while count < prefix.len() {
        match reader.read(&mut prefix[count..]) {
            Ok(0) => break,
            Ok(read) => count += read,
            Err(error) if error.kind() == ErrorKind::Interrupted => {}
            Err(error) => return Err(CodecError::Io(error)),
        }
    }
    reader.seek(SeekFrom::Start(position))?;
    Ok(classify_prefix(&prefix[..count]))
}

#[cfg(test)]
mod tests;
