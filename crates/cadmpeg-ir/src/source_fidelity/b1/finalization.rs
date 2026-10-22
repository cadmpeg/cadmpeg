// SPDX-License-Identifier: Apache-2.0
//! Exact source-span losses and atomic owner-derived B1 closure.

use std::collections::BTreeMap;

use cadmpeg_core::decode::{
    u64_from_index, DecodeContext, ResourceDimension, ResourceLimit, SourceStream,
};
use cadmpeg_core::CodecError;
use serde::{Deserialize, Serialize};

use crate::codec::DecodeLossCode;
use crate::document::ArenaClasses;
use crate::report::loss::LossNote;
use crate::{SourceFidelity, SourceProvenance, StreamName};

use super::classification::{ClassificationIndex, ClassifiedSpan};
use super::StreamPartition;

const FINALIZE: &str = "finalize source B1 coverage";

/// Complete owner-classified B1 spans of one actual source stream.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[cfg_attr(feature = "schema", schemars(with = "ClassifiedStreamWire"))]
#[serde(try_from = "ClassifiedStreamWire", into = "ClassifiedStreamWire")]
pub struct ClassifiedStream {
    byte_len: u64,
    spans: Vec<ClassifiedSpan>,
}

#[derive(Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[serde(deny_unknown_fields)]
struct ClassifiedStreamWire {
    /// Exact complete source stream length.
    byte_len: u64,
    /// Maximal owner-classified unconsumed source intervals.
    spans: Vec<ClassifiedSpan>,
}

impl TryFrom<ClassifiedStreamWire> for ClassifiedStream {
    type Error = String;
    fn try_from(wire: ClassifiedStreamWire) -> Result<Self, Self::Error> {
        let mut previous_end = None;
        for span in &wire.spans {
            let range = span.range();
            if range.end > wire.byte_len || previous_end.is_some_and(|end| end >= range.start) {
                return Err(
                    "B1 spans must be maximal, sorted, disjoint and inside their stream".into(),
                );
            }
            previous_end = Some(range.end);
        }
        Ok(Self {
            byte_len: wire.byte_len,
            spans: wire.spans,
        })
    }
}

impl From<ClassifiedStream> for ClassifiedStreamWire {
    fn from(stream: ClassifiedStream) -> Self {
        Self {
            byte_len: stream.byte_len,
            spans: stream.spans,
        }
    }
}

impl ClassifiedStream {
    /// Exact complete stream length admitted by the source context.
    pub const fn byte_len(&self) -> u64 {
        self.byte_len
    }

    /// Maximal unconsumed source spans with their owner-derived classes.
    pub fn spans(&self) -> &[ClassifiedSpan] {
        &self.spans
    }
}

fn reserve<T>(
    ctx: &DecodeContext<'_>,
    values: &mut Vec<T>,
    count: usize,
) -> Result<(), CodecError> {
    let bytes = u64_from_index(count)
        .checked_mul(u64_from_index(std::mem::size_of::<T>()))
        .ok_or_else(|| CodecError::malformed("source finalization allocation size overflows"))?;
    ctx.retain_bytes(bytes, FINALIZE)?;
    values.try_reserve_exact(count).map_err(|_| {
        ResourceLimit::allocation_failed(
            ResourceDimension::RetainedBytes,
            ctx.policy().limits.max_retained_bytes,
            bytes,
            FINALIZE,
        )
        .into()
    })
}

fn copy_string(ctx: &DecodeContext<'_>, value: &str) -> Result<String, CodecError> {
    let bytes = u64_from_index(value.len());
    ctx.retain_bytes(bytes, FINALIZE)?;
    let mut output = String::new();
    output.try_reserve_exact(value.len()).map_err(|_| {
        ResourceLimit::allocation_failed(
            ResourceDimension::RetainedBytes,
            ctx.policy().limits.max_retained_bytes,
            bytes,
            FINALIZE,
        )
    })?;
    output.push_str(value);
    Ok(output)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
struct LossKey<'a> {
    format: &'a str,
    stream: &'a str,
    offset: u64,
    byte_len: u64,
}

impl<'a> LossKey<'a> {
    fn from_loss(loss: &'a LossNote) -> Option<Self> {
        let source = loss.provenance.as_ref()?;
        Some(Self {
            format: source.format(),
            stream: source.stream_name()?,
            offset: source.offset,
            byte_len: source.byte_len()?,
        })
    }
}

struct LossIndex<'a> {
    keys: Vec<LossKey<'a>>,
}

impl<'a> LossIndex<'a> {
    fn new(
        ctx: &DecodeContext<'_>,
        losses: impl Iterator<Item = &'a LossNote> + Clone,
    ) -> Result<Self, CodecError> {
        let count = losses
            .clone()
            .filter_map(LossKey::from_loss)
            .try_fold(0_usize, |count, _| count.checked_add(1))
            .ok_or_else(|| CodecError::malformed("source loss index count overflows"))?;
        let mut keys = Vec::new();
        reserve(ctx, &mut keys, count)?;
        keys.extend(losses.filter_map(LossKey::from_loss));
        keys.sort_unstable();
        keys.dedup();
        Ok(Self { keys })
    }

    fn names_span(&self, format: &str, stream: &str, span: &ClassifiedSpan) -> bool {
        let range = span.range();
        self.names_extent(format, stream, range.start, range.end - range.start)
    }

    fn names_extent(&self, format: &str, stream: &str, offset: u64, byte_len: u64) -> bool {
        self.keys
            .binary_search_by(|key| {
                (key.format, key.stream, key.offset, key.byte_len)
                    .cmp(&(format, stream, offset, byte_len))
            })
            .is_ok()
    }
}

fn source_streams(ctx: &DecodeContext<'_>) -> Result<Vec<SourceStream>, CodecError> {
    let mut streams = ctx.source_streams()?;
    streams.sort_unstable_by(|left, right| left.name().cmp(right.name()));
    Ok(streams)
}

fn has_stream(streams: &[SourceStream], partition: &StreamPartition) -> bool {
    streams
        .binary_search_by(|stream| stream.name().cmp(partition.stream()))
        .is_ok_and(|index| streams[index].byte_len() == partition.byte_len())
}

fn check_partition(ctx: &DecodeContext<'_>, partition: &StreamPartition) -> Result<(), CodecError> {
    let count = partition
        .reads()
        .len()
        .checked_add(partition.exempt().len())
        .and_then(|count| count.checked_add(partition.b1().len()))
        .ok_or_else(|| CodecError::malformed("source closure interval count overflows"))?;
    let mut ranges = Vec::new();
    reserve(ctx, &mut ranges, count)?;
    ranges.extend(
        partition
            .reads()
            .iter()
            .chain(partition.exempt())
            .chain(partition.b1()),
    );
    ranges.sort_unstable_by_key(|range| (range.start, range.end));
    let mut end = 0;
    for range in ranges {
        if range.start != end || range.start >= range.end || range.end > partition.byte_len() {
            return Err(CodecError::malformed(
                "source closure has a gap, overlap or invalid extent",
            ));
        }
        end = range.end;
    }
    if end != partition.byte_len() {
        return Err(CodecError::malformed(
            "source closure does not cover the complete stream",
        ));
    }
    Ok(())
}

impl SourceFidelity {
    /// Classify every actual source stream, then atomically install coverage and losses.
    ///
    /// The index supplies classes from typed document ownership. Codec adoption
    /// supplies actual framing partitions; messages and enclosing losses cannot
    /// substitute for exact stream, offset and length provenance.
    pub fn finalize_b1(
        &mut self,
        ctx: &DecodeContext<'_>,
        index: &ClassificationIndex<'_>,
        format: &str,
        partitions: &[StreamPartition],
        losses: &mut Vec<LossNote>,
    ) -> Result<(), CodecError> {
        if format.is_empty() {
            return Err(CodecError::malformed("B1 loss attribution needs a format"));
        }
        let streams = source_streams(ctx)?;
        if streams.len() != partitions.len() {
            return Err(CodecError::malformed(
                "B1 finalization requires every actual source stream exactly once",
            ));
        }
        let existing_losses = LossIndex::new(ctx, losses.iter())?;
        let mut coverage = BTreeMap::new();
        let mut additions = Vec::new();
        let missing = partitions
            .iter()
            .try_fold(0_usize, |count, partition| {
                let missing = partition
                    .b1()
                    .iter()
                    .filter(|range| {
                        !existing_losses.names_extent(
                            format,
                            partition.stream(),
                            range.start,
                            range.end - range.start,
                        )
                    })
                    .count();
                count.checked_add(missing)
            })
            .ok_or_else(|| CodecError::malformed("unattributed source span count overflows"))?;
        reserve(ctx, &mut additions, missing)?;
        for partition in partitions {
            if !has_stream(&streams, partition) {
                return Err(CodecError::malformed(
                    "B1 partition does not name its actual source stream and length",
                ));
            }
            check_partition(ctx, partition)?;
            let spans = index.classify_partition(ctx, self, partition.stream(), partition)?;
            if spans.len() != partition.b1().len()
                || spans
                    .iter()
                    .zip(partition.b1())
                    .any(|(span, range)| span.range() != *range)
            {
                return Err(CodecError::malformed(
                    "B1 owner classification differs from the byte partition",
                ));
            }
            for span in &spans {
                if !existing_losses.names_span(format, partition.stream(), span) {
                    let range = span.range();
                    let provenance = SourceProvenance::in_stream(
                        copy_string(ctx, format)?,
                        StreamName::try_from(copy_string(ctx, partition.stream())?)
                            .map_err(CodecError::malformed)?,
                        range.start,
                    )
                    .with_byte_len(range.end - range.start)?;
                    additions.push(
                        DecodeLossCode::SourceBytesUndecoded
                            .note("No typed record was built from this source span")
                            .with_provenance(provenance),
                    );
                }
            }
            let stream = copy_string(ctx, partition.stream())?;
            if coverage
                .insert(
                    stream,
                    ClassifiedStream {
                        byte_len: partition.byte_len(),
                        spans,
                    },
                )
                .is_some()
            {
                return Err(CodecError::malformed("duplicate B1 source partition"));
            }
        }
        let mut image_classes = Vec::new();
        reserve(ctx, &mut image_classes, self.retained_records.len())?;
        for record in self.retained_records.values() {
            let stream = coverage.get(record.stream()).ok_or_else(|| {
                CodecError::malformed("retained source home has no actual stream partition")
            })?;
            let end = record
                .offset()
                .checked_add(record.byte_len())
                .filter(|&end| end <= stream.byte_len)
                .ok_or_else(|| CodecError::malformed("retained source home exceeds its stream"))?;
            let mut classes = ArenaClasses::new();
            let first = stream
                .spans
                .partition_point(|span| span.range().end <= record.offset());
            for span in &stream.spans[first..] {
                if span.range().start >= end {
                    break;
                }
                classes.extend(span.classes());
            }
            image_classes.push(classes);
        }
        let completed_losses = LossIndex::new(ctx, losses.iter().chain(&additions))?;
        check_losses(&coverage, format, &completed_losses)?;
        drop(completed_losses);
        drop(existing_losses);
        reserve(ctx, losses, additions.len())?;
        self.b1_spans = coverage;
        for (record, classes) in self.retained_records.values_mut().zip(image_classes) {
            record.untyped = classes;
        }
        losses.extend(additions);
        Ok(())
    }

    /// Verify exact span attribution, owner classes and every byte's disposition.
    pub fn check_b1_closure(
        &self,
        ctx: &DecodeContext<'_>,
        index: &ClassificationIndex<'_>,
        format: &str,
        partitions: &[StreamPartition],
        losses: &[LossNote],
    ) -> Result<(), CodecError> {
        let streams = source_streams(ctx)?;
        if streams.len() != partitions.len() || self.b1_spans.len() != partitions.len() {
            return Err(CodecError::malformed(
                "source closure lacks a complete stream inventory",
            ));
        }
        let mut partition_names = Vec::new();
        reserve(ctx, &mut partition_names, partitions.len())?;
        partition_names.extend(partitions.iter().map(StreamPartition::stream));
        partition_names.sort_unstable();
        if partition_names.windows(2).any(|pair| pair[0] == pair[1]) {
            return Err(CodecError::malformed("duplicate source closure stream"));
        }
        for partition in partitions {
            if !has_stream(&streams, partition) {
                return Err(CodecError::malformed(
                    "source closure stream differs from actual provenance",
                ));
            }
            check_partition(ctx, partition)?;
            let spans = index.classify_partition(ctx, self, partition.stream(), partition)?;
            let stored = self
                .b1_spans
                .get(partition.stream())
                .ok_or_else(|| CodecError::malformed("source closure lacks stored B1 spans"))?;
            if stored.byte_len != partition.byte_len() || stored.spans != spans {
                return Err(CodecError::malformed(
                    "stored B1 ownership differs from source partition",
                ));
            }
        }
        let losses = LossIndex::new(ctx, losses.iter())?;
        check_losses(&self.b1_spans, format, &losses)?;
        let derived: ArenaClasses = self
            .b1_spans
            .values()
            .flat_map(|stream| stream.spans.iter())
            .flat_map(ClassifiedSpan::classes)
            .collect();
        if derived != self.unrepresented() {
            return Err(CodecError::malformed(
                "B1 classes are absent from unrepresented",
            ));
        }
        Ok(())
    }
}

fn check_losses(
    coverage: &BTreeMap<String, ClassifiedStream>,
    format: &str,
    losses: &LossIndex<'_>,
) -> Result<(), CodecError> {
    for (stream, coverage) in coverage {
        for span in &coverage.spans {
            if !losses.names_span(format, stream, span) {
                return Err(CodecError::malformed(
                    "B1 source span has no exact loss provenance",
                ));
            }
        }
    }
    Ok(())
}

#[derive(Debug, Default)]
pub(crate) struct B1RescopeProof {
    streams: BTreeMap<String, (String, ClassifiedStream)>,
}

impl B1RescopeProof {
    pub(crate) fn stage(
        ctx: &DecodeContext<'_>,
        source: &BTreeMap<String, ClassifiedStream>,
        prefix: &str,
    ) -> Result<Self, CodecError> {
        let mut streams = BTreeMap::new();
        for (name, stream) in source {
            let length = prefix
                .len()
                .checked_add(name.len())
                .ok_or_else(|| CodecError::malformed("composed B1 stream length overflows"))?;
            ctx.retain_bytes(u64_from_index(length), FINALIZE)?;
            let mut mapped = String::new();
            mapped.try_reserve_exact(length).map_err(|_| {
                ResourceLimit::allocation_failed(
                    ResourceDimension::RetainedBytes,
                    ctx.policy().limits.max_retained_bytes,
                    u64_from_index(length),
                    FINALIZE,
                )
            })?;
            mapped.push_str(prefix);
            mapped.push_str(name);
            if mapped.is_empty() {
                return Err(CodecError::malformed("composed B1 stream name is empty"));
            }
            let mut spans = Vec::new();
            reserve(ctx, &mut spans, stream.spans.len())?;
            spans.extend(stream.spans.iter().cloned());
            streams.insert(
                copy_string(ctx, name)?,
                (
                    mapped,
                    ClassifiedStream {
                        byte_len: stream.byte_len,
                        spans,
                    },
                ),
            );
        }
        Ok(Self { streams })
    }

    pub(crate) fn append(
        self,
        destination: &mut SourceFidelity,
        mut source: BTreeMap<String, ClassifiedStream>,
    ) -> Result<(), CodecError> {
        if self.streams.len() != source.len()
            || self
                .streams
                .iter()
                .any(|(name, (_, expected))| source.get(name) != Some(expected))
        {
            return Err(CodecError::malformed(
                "B1 composition map differs from its admitted source proof",
            ));
        }
        if self
            .streams
            .values()
            .any(|(name, _)| destination.b1_spans.contains_key(name))
        {
            return Err(CodecError::malformed("duplicate composed B1 source stream"));
        }
        let mut mapped = BTreeMap::new();
        for (old, (name, _)) in self.streams {
            let stream = source
                .remove(&old)
                .ok_or_else(|| CodecError::malformed("B1 composition source stream is absent"))?;
            if mapped.insert(name, stream).is_some() {
                return Err(CodecError::malformed("composed B1 source names collide"));
            }
        }
        destination.b1_spans.append(&mut mapped);
        Ok(())
    }
}

#[cfg(test)]
mod tests;
