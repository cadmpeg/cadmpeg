// SPDX-License-Identifier: Apache-2.0
//! Kernel header metadata shared by binary ASM, binary ACIS, and text streams.

use cadmpeg_core::decode::View;

/// The recognized metadata fields of an ASM or ACIS model stream.
#[derive(Debug, Clone, PartialEq)]
pub struct KernelHeader {
    /// Integer and reference width used by the record stream.
    pub width: u8,
    /// ACIS save-format version, encoded as `100 * major + minor`.
    pub save_format_version: Option<u32>,
    /// Record-count word when the header carries one.
    pub record_count: Option<u32>,
    /// Entity-count word.
    pub entity_count: Option<u64>,
    /// Kernel flags word.
    pub flags: Option<u64>,
    /// Product family string.
    pub product_family: Option<String>,
    /// Product version string.
    pub product_version: Option<String>,
    /// Save date string.
    pub save_date: Option<String>,
    /// Kernel scale metadata slot. Coordinate decoding does not apply it.
    pub scale: Option<f64>,
    /// Absolute distance tolerance `resabs`.
    pub linear: Option<f64>,
    /// Normal tolerance `resnor`.
    pub angular: Option<f64>,
}

/// Flag bit selecting the optional construction-history partition.
pub const HISTORY_PARTITION_FLAG: u64 = 1;

/// Flag bits 1 to 7, which hold the save format's revision number.
pub const FORMAT_REVISION_FLAGS: u64 = 0xfe;

const FORMAT_REVISION_SHIFT: u32 = 1;

impl KernelHeader {
    /// Major component of the encoded ACIS save-format version.
    pub fn save_format_major(&self) -> Option<u32> {
        self.save_format_version.map(|version| version / 100)
    }

    /// Minor component of the encoded ACIS save-format version.
    pub fn save_format_minor(&self) -> Option<u32> {
        self.save_format_version.map(|version| version % 100)
    }

    /// Whether the stream header declares a construction-history partition.
    pub fn has_history_partition(&self) -> bool {
        self.flags
            .is_some_and(|flags| flags & HISTORY_PARTITION_FLAG != 0)
    }

    /// Save format revision from flag bits 1 to 7.
    pub fn format_revision(&self) -> Option<u32> {
        self.flags
            .map(|flags| ((flags & FORMAT_REVISION_FLAGS) >> FORMAT_REVISION_SHIFT) as u32)
    }

    /// Flags outside the history and revision fields.
    pub fn unassigned_flags(&self) -> Option<u64> {
        self.flags
            .map(|flags| flags & !(HISTORY_PARTITION_FLAG | FORMAT_REVISION_FLAGS))
    }
}

pub(crate) fn read_string_region(bytes: &[u8], start: usize) -> (Vec<String>, Vec<f64>, usize) {
    let mut cur = start;
    let mut strings = Vec::new();
    while strings.len() < 3 {
        match read_u8_string(bytes, cur) {
            Some((value, next)) => {
                strings.push(value);
                cur = next;
            }
            None => break,
        }
    }
    let mut doubles = Vec::new();
    while doubles.len() < 3 {
        match read_tagged_f64(bytes, cur) {
            Some((value, next)) => {
                doubles.push(value);
                cur = next;
            }
            None => break,
        }
    }
    (strings, doubles, cur)
}

fn read_u8_string(bytes: &[u8], at: usize) -> Option<(String, usize)> {
    if *bytes.get(at)? != 0x07 {
        return None;
    }
    let len = *bytes.get(at + 1)? as usize;
    let start = at + 2;
    let value = std::str::from_utf8(bytes.get(start..start + len)?)
        .ok()?
        .to_string();
    Some((value, start + len))
}

fn read_tagged_f64(bytes: &[u8], at: usize) -> Option<(f64, usize)> {
    if *bytes.get(at)? != 0x06 {
        return None;
    }
    let value = View::f64_le_at(bytes, at + 1)?;
    Some((value, at + 9))
}
