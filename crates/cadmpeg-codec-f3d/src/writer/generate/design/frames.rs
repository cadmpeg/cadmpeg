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
    /// `(record index, offset of a paired indexed header in bytes)`: the
    /// secondary index of the segment.
    pub(in crate::writer::generate) secondaries: Vec<(u32, usize)>,
}

/// The value written into one unstated slot of a layout template.
#[derive(Clone, Copy)]
pub(in crate::writer::generate) enum Bind<'a> {
    /// A paired indexed header of the ancestor class, with the record's
    /// index.
    Paired(&'a DesignClassTag),
    /// A same-segment reference.
    Ref(u32),
    /// A cross-segment reference to `target` of segment `segment`.
    Cross {
        target: u32,
        segment: u32,
    },
    U32(u32),
    U64(u64),
    F32s(&'a [f32]),
    F64(f64),
    F64s(&'a [f64]),
    /// UTF-16LE code units with no length prefix.
    Units(&'a str),
    /// An LP-UTF16 string.
    Utf16(&'a str),
}

impl DesignRecords {
    /// Start the record `record_index` of class `class` with its indexed
    /// header.
    pub(super) fn record(&mut self, class: &DesignClassTag, record_index: u32) -> &mut Self {
        self.primaries.push((record_index, self.bytes.len()));
        self.indexed_header(class, record_index)
    }

    /// A paired indexed header of the ancestor class `class`, which repeats
    /// its record's index.
    pub(super) fn header(&mut self, class: &DesignClassTag, record_index: u32) -> &mut Self {
        self.secondaries.push((record_index, self.bytes.len()));
        self.indexed_header(class, record_index)
    }

    /// An indexed header: `u32 3`, the three class-tag digits and the record
    /// index.
    fn indexed_header(&mut self, class: &DesignClassTag, record_index: u32) -> &mut Self {
        self.bytes.extend_from_slice(&3u32.to_le_bytes());
        self.bytes.extend_from_slice(class.as_str().as_bytes());
        self.u32(record_index)
    }

    /// The record `record_index` written from its layout template: the
    /// template bytes, its indexed header, and `binds` in the template's
    /// unstated slots. Every unstated slot after the header is bound once,
    /// by a value of the slot's width.
    pub(super) fn templated(
        &mut self,
        class: &DesignClassTag,
        record_index: u32,
        template: &[u8],
        unstated: &[(usize, usize)],
        binds: &[(usize, Bind<'_>)],
    ) -> Result<&mut Self, CodecError> {
        let start = self.bytes.len();
        self.primaries.push((record_index, start));
        self.bytes.extend_from_slice(template);
        let mut header = DesignRecords::default();
        header.indexed_header(class, record_index);
        self.place(start, 0, &header.bytes, unstated)?;
        let mut bound = vec![(0, header.bytes.len())];
        for (offset, bind) in binds {
            let mut value = DesignRecords::default();
            match bind {
                Bind::Paired(class) => {
                    self.secondaries.push((record_index, start + offset));
                    value.indexed_header(class, record_index);
                }
                Bind::Ref(target) => {
                    value.reference(*target);
                }
                Bind::Cross { target, segment } => {
                    value.cross_segment_reference(u64::from(*target), *segment);
                }
                Bind::U32(number) => {
                    value.u32(*number);
                }
                Bind::U64(number) => {
                    value.u64(*number);
                }
                Bind::F32s(numbers) => {
                    for number in *numbers {
                        value.f32(*number);
                    }
                }
                Bind::F64(number) => {
                    value.f64(*number);
                }
                Bind::F64s(numbers) => {
                    for number in *numbers {
                        value.f64(*number);
                    }
                }
                Bind::Units(text) => {
                    for unit in text.encode_utf16() {
                        value.bytes.extend_from_slice(&unit.to_le_bytes());
                    }
                }
                Bind::Utf16(text) => {
                    value.lp_utf16(text)?;
                }
            }
            self.place(start, *offset, &value.bytes, unstated)?;
            bound.push((*offset, value.bytes.len()));
        }
        bound.sort_unstable();
        if bound != unstated {
            return Err(CodecError::malformed(format_args!(
                "Design record {record_index} binds the slots {bound:?} of a template whose unstated slots are {unstated:?}"
            )));
        }
        Ok(self)
    }

    /// Write `value` over the template slot at `offset` of the record at
    /// `start`.
    fn place(
        &mut self,
        start: usize,
        offset: usize,
        value: &[u8],
        unstated: &[(usize, usize)],
    ) -> Result<(), CodecError> {
        if !unstated.contains(&(offset, value.len())) {
            return Err(CodecError::malformed(format_args!(
                "a {}-byte value at offset {offset} fills no unstated template slot",
                value.len()
            )));
        }
        self.bytes[start + offset..start + offset + value.len()].copy_from_slice(value);
        Ok(())
    }

    /// The records reordered by ascending record index, with their primary
    /// and secondary offsets moved with them.
    pub(super) fn sorted(self) -> Result<Self, CodecError> {
        let mut starts = self.primaries.clone();
        starts.sort_unstable_by_key(|&(_, offset)| offset);
        let mut frames = starts
            .iter()
            .enumerate()
            .map(|(position, &(index, start))| {
                let end = starts
                    .get(position + 1)
                    .map_or(self.bytes.len(), |&(_, next)| next);
                (index, start, end)
            })
            .collect::<Vec<_>>();
        frames.sort_unstable_by_key(|&(index, _, _)| index);
        if frames.windows(2).any(|pair| pair[0].0 == pair[1].0) {
            return Err(CodecError::Malformed(
                "two Design records share one record index".into(),
            ));
        }
        let mut sorted = DesignRecords::default();
        for (index, start, end) in frames {
            let moved = sorted.bytes.len();
            sorted.primaries.push((index, moved));
            sorted.secondaries.extend(
                self.secondaries
                    .iter()
                    .filter(|&&(_, offset)| (start..end).contains(&offset))
                    .map(|&(record, offset)| (record, offset - start + moved)),
            );
            sorted.bytes.extend_from_slice(&self.bytes[start..end]);
        }
        Ok(sorted)
    }

    pub(super) fn zeros(&mut self, count: usize) -> &mut Self {
        self.bytes.resize(self.bytes.len() + count, 0);
        self
    }

    pub(super) fn byte(&mut self, value: u8) -> &mut Self {
        self.bytes.push(value);
        self
    }

    pub(super) fn raw(&mut self, bytes: &[u8]) -> &mut Self {
        self.bytes.extend_from_slice(bytes);
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

    pub(super) fn f32(&mut self, value: f32) -> &mut Self {
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
