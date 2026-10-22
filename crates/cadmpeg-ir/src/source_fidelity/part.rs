// SPDX-License-Identifier: Apache-2.0
//! Checked source layout and the record-capture handoff for part decoding.

use std::collections::{btree_map::Entry, BTreeMap};
use std::ops::Range;

use cadmpeg_core::decode::{
    u64_from_index, DecodeContext, RecordCapture, ResourceDimension, ResourceLimit,
};
use cadmpeg_core::CodecError;
use serde::{Deserialize, Serialize};

use crate::native::bytes::NativeBytes;

/// Source extent, semantic reads, and retained layout fields of one record.
///
/// Only a completed core capture supplies new read attribution. Wire hydration
/// checks the range structure; actual stream bounds require the input registry.
///
/// ```compile_fail
/// use cadmpeg_ir::source_fidelity::RecordLayout;
/// fn overwrite(layout: &mut RecordLayout) { layout.reads.clear(); }
/// ```
///
/// ```compile_fail
/// use cadmpeg_ir::source_fidelity::RecordLayout;
/// fn overwrite(layout: &mut RecordLayout) { layout.reads().clear(); }
/// ```
///
/// ```compile_fail
/// use cadmpeg_ir::source_fidelity::RecordLayout;
/// let layout = RecordLayout {
///     stream: "file".into(), offset: 0, byte_len: 1,
///     reads: Default::default(), pointers: Default::default(), payloads: Default::default(),
/// };
/// ```
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[cfg_attr(feature = "schema", schemars(with = "RecordLayoutWire"))]
#[serde(try_from = "RecordLayoutWire")]
pub struct RecordLayout {
    stream: String,
    offset: u64,
    byte_len: u64,
    reads: BTreeMap<String, Vec<Range<u64>>>,
    pointers: BTreeMap<String, u64>,
    payloads: BTreeMap<String, NativeBytes>,
}

#[derive(Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[serde(deny_unknown_fields)]
struct RecordLayoutWire {
    /// Named source stream that contains the record.
    stream: String,
    /// First byte of the record in its source stream.
    offset: u64,
    /// Length of the complete record extent, including unread bytes.
    byte_len: u64,
    /// Sorted, coalesced semantic read intervals, grouped by source stream.
    #[serde(deserialize_with = "cadmpeg_core::distinct_keys::btree_map")]
    reads: BTreeMap<String, Vec<ReadRangeWire>>,
    /// Source offsets retained outside document content.
    #[serde(deserialize_with = "cadmpeg_core::distinct_keys::btree_map")]
    pointers: BTreeMap<String, u64>,
    /// Exact hexadecimal payload bytes retained without semantic reads.
    #[serde(deserialize_with = "cadmpeg_core::distinct_keys::btree_map")]
    #[cfg_attr(feature = "schema", schemars(with = "BTreeMap<String, String>"))]
    payloads: BTreeMap<String, NativeBytes>,
}

#[derive(Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[serde(deny_unknown_fields)]
struct ReadRangeWire {
    /// Inclusive first byte of the read interval.
    start: u64,
    /// Exclusive end of the read interval.
    end: u64,
}

impl TryFrom<RecordLayoutWire> for RecordLayout {
    type Error = String;

    fn try_from(wire: RecordLayoutWire) -> Result<Self, Self::Error> {
        if wire.stream.is_empty() {
            return Err("record layout stream must not be empty".into());
        }
        super::require_extent(&wire.stream, wire.offset, wire.byte_len)
            .map_err(|error| error.to_string())?;
        let mut reads = BTreeMap::new();
        for (stream, ranges) in wire.reads {
            if stream.is_empty() || ranges.is_empty() {
                return Err("record layout reads require a named stream and ranges".into());
            }
            let mut previous_end = None;
            let mut admitted = Vec::with_capacity(ranges.len());
            for range in ranges {
                if range.start >= range.end {
                    return Err(format!(
                        "record layout reads for {stream} contain an empty or inverted range"
                    ));
                }
                if previous_end.is_some_and(|end| range.start <= end) {
                    return Err(format!(
                        "record layout reads for {stream} must be sorted and coalesced"
                    ));
                }
                previous_end = Some(range.end);
                admitted.push(range.start..range.end);
            }
            reads.insert(stream, admitted);
        }
        Ok(Self {
            stream: wire.stream,
            offset: wire.offset,
            byte_len: wire.byte_len,
            reads,
            pointers: wire.pointers,
            payloads: wire.payloads,
        })
    }
}

impl RecordLayout {
    /// A given-up part has a real source extent and no committed semantic reads.
    pub(super) fn from_part_capture(capture: &cadmpeg_core::decode::PartCapture) -> Self {
        Self {
            stream: capture.stream().to_owned(),
            offset: capture.offset(),
            byte_len: capture.byte_len(),
            reads: BTreeMap::new(),
            pointers: BTreeMap::new(),
            payloads: BTreeMap::new(),
        }
    }

    /// Consumes a completed tracked capture, preserving its record identity.
    ///
    /// Core admits actual stream bounds and merges read intervals before this
    /// handoff. This conversion neither scans nor claims any additional bytes.
    pub(crate) fn from_capture(capture: RecordCapture) -> (String, Self) {
        let (id, stream, offset, byte_len, reads) = capture.into_parts();
        (
            id,
            Self {
                stream,
                offset,
                byte_len,
                reads,
                pointers: BTreeMap::new(),
                payloads: BTreeMap::new(),
            },
        )
    }

    /// Stream containing the record extent.
    pub fn stream(&self) -> &str {
        &self.stream
    }

    /// First byte of the record in its stream.
    pub const fn offset(&self) -> u64 {
        self.offset
    }

    /// Length of the record extent, including any unread bytes.
    pub const fn byte_len(&self) -> u64 {
        self.byte_len
    }

    /// Sorted, coalesced half-open semantic read intervals by stream.
    pub const fn reads(&self) -> &BTreeMap<String, Vec<Range<u64>>> {
        &self.reads
    }

    /// Source offsets moved out of document fields.
    pub const fn pointers(&self) -> &BTreeMap<String, u64> {
        &self.pointers
    }

    /// Exact payload bytes retained without semantic read attribution.
    pub const fn payloads(&self) -> &BTreeMap<String, NativeBytes> {
        &self.payloads
    }

    /// Prefixes the containing stream and every read stream for composition.
    ///
    /// A fixed prefix is injective. Byte coordinates, pointers, and payloads
    /// stay unchanged. Stream-name allocation and length overflow are checked.
    pub(crate) fn with_stream_prefix(self, prefix: &str) -> Result<Self, CodecError> {
        self.prefix_streams(|stream| prefixed_stream(prefix, stream, u64::MAX, |_| Ok(())))
    }

    /// [`Self::with_stream_prefix`] under the caller's retained-byte budget.
    ///
    /// Core charged the captured stream names and read-map entries. Each
    /// prefixed name replaces its unprefixed name, so only the added prefix
    /// bytes are charged.
    pub(super) fn with_stream_prefix_in(
        self,
        ctx: &DecodeContext<'_>,
        prefix: &str,
    ) -> Result<Self, CodecError> {
        self.prefix_streams(|stream| {
            prefixed_stream(
                prefix,
                stream,
                ctx.policy().limits.max_retained_bytes,
                |_| ctx.retain_bytes(u64_from_index(prefix.len()), "prefix record layout stream"),
            )
        })
    }

    fn prefix_streams(
        mut self,
        mut name: impl FnMut(&str) -> Result<String, CodecError>,
    ) -> Result<Self, CodecError> {
        self.stream = name(&self.stream)?;
        let mut mapped = BTreeMap::new();
        for (stream, ranges) in self.reads {
            insert_field(&mut mapped, name(&stream)?, ranges)?;
        }
        self.reads = mapped;
        Ok(self)
    }

    /// Inserts a source offset without replacing an existing field.
    pub fn insert_pointer(&mut self, field: String, value: u64) -> Result<(), CodecError> {
        insert_field(&mut self.pointers, field, value)
    }

    /// Inserts exact payload bytes without claiming them as semantic reads.
    pub fn insert_payload(&mut self, field: String, value: NativeBytes) -> Result<(), CodecError> {
        insert_field(&mut self.payloads, field, value)
    }
}

fn insert_field<T>(
    fields: &mut BTreeMap<String, T>,
    field: String,
    value: T,
) -> Result<(), CodecError> {
    match fields.entry(field) {
        Entry::Vacant(entry) => {
            entry.insert(value);
            Ok(())
        }
        Entry::Occupied(entry) => Err(CodecError::malformed(format_args!(
            "duplicate record layout field {}",
            entry.key()
        ))),
    }
}

fn prefixed_stream(
    prefix: &str,
    stream: &str,
    allocation_limit: u64,
    admit: impl FnOnce(u64) -> Result<(), CodecError>,
) -> Result<String, CodecError> {
    let failure = |additional| {
        ResourceLimit::allocation_failed(
            ResourceDimension::RetainedBytes,
            allocation_limit,
            additional,
            "prefix record layout stream",
        )
    };
    let length = prefix
        .len()
        .checked_add(stream.len())
        .ok_or_else(|| failure(u64::MAX))?;
    if length == 0 {
        return Err(CodecError::malformed(
            "record layout stream must not be empty",
        ));
    }
    admit(u64_from_index(length))?;
    let mut name = String::new();
    name.try_reserve_exact(length)
        .map_err(|_| failure(u64_from_index(length)))?;
    name.push_str(prefix);
    name.push_str(stream);
    Ok(name)
}
