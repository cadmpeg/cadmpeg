// SPDX-License-Identifier: Apache-2.0
//! A writer of indexed Design records in the plain reference form of a
//! magic-1234 stream (f3d.md §3.1): eleven-byte indexed headers and
//! eleven-byte same-segment references `01 + u64 + 00 00`.

use cadmpeg_core::CodecError;

use crate::records::references::DesignClassTag;

/// Records in `BulkStream` order, each with its primary-index entry.
#[derive(Default)]
pub(in crate::writer::generate) struct DesignRecords {
    pub(in crate::writer::generate) bytes: Vec<u8>,
    /// `(record index, offset of its indexed header in bytes)`.
    pub(in crate::writer::generate) primaries: Vec<(u32, usize)>,
}

impl DesignRecords {
    /// Start the record `record_index` of class `class` with its indexed
    /// header.
    pub(super) fn record(&mut self, class: &DesignClassTag, record_index: u32) -> &mut Self {
        self.primaries.push((record_index, self.bytes.len()));
        self.header(class, record_index)
    }

    /// An indexed header: `u32 3`, the three class-tag digits and the record
    /// index. A paired header repeats its record's index.
    pub(super) fn header(&mut self, class: &DesignClassTag, record_index: u32) -> &mut Self {
        self.bytes.extend_from_slice(&3u32.to_le_bytes());
        self.bytes.extend_from_slice(class.as_str().as_bytes());
        self.u32(record_index)
    }

    pub(super) fn zeros(&mut self, count: usize) -> &mut Self {
        self.bytes.resize(self.bytes.len() + count, 0);
        self
    }

    pub(super) fn byte(&mut self, value: u8) -> &mut Self {
        self.bytes.push(value);
        self
    }

    pub(super) fn u32(&mut self, value: u32) -> &mut Self {
        self.bytes.extend_from_slice(&value.to_le_bytes());
        self
    }

    pub(super) fn u64(&mut self, value: u64) -> &mut Self {
        self.bytes.extend_from_slice(&value.to_le_bytes());
        self
    }

    pub(super) fn f64(&mut self, value: f64) -> &mut Self {
        self.bytes.extend_from_slice(&value.to_le_bytes());
        self
    }

    /// A same-segment reference to `record_index`.
    pub(super) fn reference(&mut self, record_index: u32) -> &mut Self {
        self.byte(1).u64(u64::from(record_index)).zeros(2)
    }

    /// A reference followed by its u32 member.
    pub(super) fn member(&mut self, record_index: u32, member: u32) -> &mut Self {
        self.reference(record_index).u32(member)
    }

    /// A cross-segment reference: marker, u64 target, `00`, segment flag `01`
    /// and the u32 segment index.
    pub(super) fn cross_segment_reference(&mut self, target: u64, segment: u32) -> &mut Self {
        self.byte(1).u64(target).byte(0).byte(1).u32(segment)
    }

    pub(super) fn lp_ascii(&mut self, value: &str) -> Result<&mut Self, CodecError> {
        self.u32(length(value.len())?);
        self.bytes.extend_from_slice(value.as_bytes());
        Ok(self)
    }

    pub(super) fn lp_utf16(&mut self, value: &str) -> Result<&mut Self, CodecError> {
        let units = value.encode_utf16().collect::<Vec<_>>();
        self.u32(length(units.len())?);
        for unit in units {
            self.bytes.extend_from_slice(&unit.to_le_bytes());
        }
        Ok(self)
    }
}

fn length(count: usize) -> Result<u32, CodecError> {
    u32::try_from(count)
        .map_err(|_| CodecError::NotImplemented("Design string exceeds u32::MAX".into()))
}
