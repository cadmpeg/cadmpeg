// SPDX-License-Identifier: Apache-2.0
//! Complete physical `RMFastLoad` allocations, without entity or token semantics.

use super::om::ClassDefinition;
use crate::container::Container;
use crate::layout::{rmfastload_header, rmfastload_prefix};
use crate::om::rmfastload::{Storage, TerminalAtom};
use cadmpeg_core::decode::{index_from_u32, u64_from_index, DecodeContext};
use cadmpeg_core::CodecError;
use cadmpeg_ir::hash::digest::Sha256Digest;
use cadmpeg_ir::native::bytes::NativeBytes;
use cadmpeg_ir::native::{NativeConvertError, NativeNamespace};
use serde::{Deserialize, Serialize};
use std::fmt::Write;

const ENTRY: &str = "/Root/FastLoad/RMFastLoad";

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
struct PhysicalSpan {
    source_offset: u64,
    byte_len: u64,
    sha256: Sha256Digest,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
struct TerminalRow {
    source_offset: u64,
    tokens: [TerminalAtom; 3],
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
struct StorageWire {
    id: String,
    source_entry: String,
    source_offset: u64,
    byte_len: u64,
    sha256: Sha256Digest,
    header_source_offset: u64,
    header_words: [u32; 9],
    boundary_source_offset: u64,
    boundaries: Vec<u32>,
    table_count_source_offset: u64,
    table_words: Vec<u32>,
    physical_spans: Vec<PhysicalSpan>,
    terminal_preamble: NativeBytes<[u8; 12]>,
    terminal_rows: Vec<TerminalRow>,
    terminal_trailer: NativeBytes<[u8; 12]>,
}

impl StorageWire {
    fn is_complete(&self) -> bool {
        let count = index_from_u32(self.header_words[0]);
        let rows = index_from_u32(self.header_words[4]);
        let terminal = u64::from(self.header_words[8]);
        if self.source_entry != ENTRY
            || self.id.is_empty()
            || count == 0
            || [2, 5, 6, 7].into_iter().any(|i| self.header_words[i] != 0)
            || count.checked_add(1) != Some(self.boundaries.len())
            || self.table_words.len() != count
            || self.physical_spans.len() != count
            || self.terminal_rows.len() != rows
            || self.boundaries.first() != Some(&0)
            || self.boundaries.get(1).is_none_or(|n| *n == 0)
            || self
                .boundaries
                .last()
                .is_none_or(|n| u64::from(*n) > terminal)
        {
            return false;
        }
        let Some(entry_end) = self.source_offset.checked_add(self.byte_len) else {
            return false;
        };
        let Some(terminal_start) = self.source_offset.checked_add(terminal) else {
            return false;
        };
        let Some(header_end) = self
            .header_source_offset
            .checked_add(u64_from_index(rmfastload_header::LEN))
        else {
            return false;
        };
        let Some(first_header_byte) = self
            .source_offset
            .checked_add(u64_from_index(rmfastload_prefix::PLATFORM_LENGTH + 8))
        else {
            return false;
        };
        if self.header_source_offset < first_header_byte
            || header_end > self.boundary_source_offset
            || u64_from_index(self.boundaries.len())
                .checked_mul(4)
                .and_then(|n| self.boundary_source_offset.checked_add(n))
                != Some(self.table_count_source_offset)
            || self
                .table_count_source_offset
                .checked_add(4)
                .and_then(|n| u64_from_index(count).checked_mul(4)?.checked_add(n))
                .is_none_or(|end| end > terminal_start)
        {
            return false;
        }
        for (span, pair) in self.physical_spans.iter().zip(self.boundaries.windows(2)) {
            if pair[1] < pair[0]
                || self.source_offset.checked_add(u64::from(pair[0])) != Some(span.source_offset)
                || span.byte_len != u64::from(pair[1] - pair[0])
            {
                return false;
            }
        }
        let Some(mut cursor) = terminal_start.checked_add(12) else {
            return false;
        };
        for row in &self.terminal_rows {
            if row.source_offset != cursor {
                return false;
            }
            let width = row
                .tokens
                .iter()
                .map(|t| u64_from_index(t.bytes().len()))
                .sum::<u64>();
            let Some(next) = cursor.checked_add(3 + width) else {
                return false;
            };
            cursor = next;
        }
        cursor.checked_add(12) == Some(entry_end)
    }
}

/// One admitted complete physical allocation. Rows carry no inferred class.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(transparent)]
pub(super) struct RmFastLoadStorage(StorageWire);

impl<'de> Deserialize<'de> for RmFastLoadStorage {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let wire = StorageWire::deserialize(deserializer)?;
        if !wire.is_complete() {
            return Err(serde::de::Error::custom("RMFastLoad storage must have complete consistent physical arrays and terminal rows"));
        }
        Ok(Self(wire))
    }
}

fn absolute(ctx: &DecodeContext<'_>, base: u64, at: usize) -> Result<u64, CodecError> {
    base.checked_add(u64_from_index(at))
        .ok_or_else(|| ctx.refuse_codec_limit("NX RMFastLoad source offset", 0, 1))
}

fn owning_class_count(
    ctx: &DecodeContext<'_>,
    classes: &[ClassDefinition],
    base: u64,
) -> Result<usize, CodecError> {
    // Equality checks at most the fixed canonical entry length, plus its length.
    let work = u64_from_index(classes.len())
        .checked_mul(u64_from_index(ENTRY.len() + 1))
        .ok_or_else(|| ctx.refuse_codec_limit("NX RMFastLoad owning classes", 0, 1))?;
    ctx.charge_work(work, "count NX RMFastLoad owning classes")?;
    Ok(classes
        .iter()
        .filter(|c| c.belongs_to_section(ENTRY, base))
        .count())
}

pub(super) fn collect(
    ctx: &DecodeContext<'_>,
    container: &Container<'_>,
    classes: &[ClassDefinition],
) -> Result<Vec<RmFastLoadStorage>, CodecError> {
    let mut output = Vec::new();
    for entry in &container.entries {
        ctx.charge_work(
            u64_from_index(entry.name.len()),
            "select NX RMFastLoad storage entry",
        )?;
        if entry.name != ENTRY {
            continue;
        }
        let Some((source_offset, byte_len)) = entry.file_span() else {
            continue;
        };
        let Some(bytes) = container.bounded_entry_bytes(ctx, source_offset, byte_len)? else {
            continue;
        };
        let class_count = owning_class_count(ctx, classes, source_offset)?;
        let Some(storage) = Storage::read(ctx, bytes, class_count)? else {
            continue;
        };
        let count = index_from_u32(storage.header_words[0]);
        let row_count = index_from_u32(storage.header_words[4]);
        let work = u64_from_index(count)
            .checked_mul(6)
            .and_then(|n| u64_from_index(row_count).checked_mul(30)?.checked_add(n))
            .and_then(|n| n.checked_add(u64_from_index(ENTRY.len()) + 10))
            .ok_or_else(|| ctx.refuse_codec_limit("NX RMFastLoad materialization work", 0, 1))?;
        ctx.charge_work(work, "materialize and recheck NX RMFastLoad storage")?;
        let mut boundaries = ctx.collection_vec(count + 1, "retain NX RMFastLoad boundaries")?;
        boundaries.extend(storage.boundaries());
        let mut table_words = ctx.collection_vec(count, "retain NX RMFastLoad table words")?;
        table_words.extend(storage.table_words());
        let mut physical_spans =
            ctx.collection_vec(count, "retain NX RMFastLoad physical spans")?;
        for pair in boundaries.windows(2) {
            let start = index_from_u32(pair[0]);
            let end = index_from_u32(pair[1]);
            physical_spans.push(PhysicalSpan {
                source_offset: absolute(ctx, source_offset, start)?,
                byte_len: u64_from_index(end - start),
                sha256: Sha256Digest::digest_for_decode(
                    ctx,
                    &bytes[start..end],
                    "hash NX RMFastLoad physical span",
                )?,
            });
        }
        ctx.charge_collection_items(
            u64_from_index(row_count)
                .checked_mul(3)
                .and_then(|n| n.checked_add(9))
                .ok_or_else(|| {
                    ctx.refuse_codec_limit("NX RMFastLoad fixed token collections", 0, 1)
                })?,
            "retain NX RMFastLoad fixed token collections",
        )?;
        let mut terminal_rows =
            ctx.collection_vec(row_count, "retain NX RMFastLoad terminal rows")?;
        for triple in storage.triples() {
            terminal_rows.push(TerminalRow {
                source_offset: absolute(ctx, source_offset, triple.offset)?,
                tokens: triple.atoms,
            });
        }
        let prefix = "nx:rmfastload:physical-storage#";
        let mut id =
            ctx.retained_string(prefix.len() + 20, "retain NX RMFastLoad storage identity")?;
        write!(id, "{prefix}{:010}", output.len())
            .map_err(|_| ctx.refuse_codec_limit("NX RMFastLoad storage identity", 0, 1))?;
        let preamble = bytes
            .get(storage.terminal_offset..storage.terminal_offset + 12)
            .and_then(|b| NativeBytes::try_from(b).ok())
            .ok_or_else(|| ctx.refuse_codec_limit("NX RMFastLoad terminal preamble", 0, 1))?;
        let trailer = NativeBytes::try_from(&bytes[storage.trailer_offset..])
            .map_err(|_| ctx.refuse_codec_limit("NX RMFastLoad terminal trailer", 0, 1))?;
        let wire = StorageWire {
            id,
            source_entry: ctx.copy_retained_text(ENTRY, "retain NX RMFastLoad source entry")?,
            source_offset,
            byte_len,
            sha256: Sha256Digest::digest_for_decode(
                ctx,
                bytes,
                "hash NX RMFastLoad complete entry",
            )?,
            header_source_offset: absolute(ctx, source_offset, storage.header_offset)?,
            header_words: storage.header_words,
            boundary_source_offset: absolute(ctx, source_offset, storage.boundary_offset)?,
            boundaries,
            table_count_source_offset: absolute(ctx, source_offset, storage.count_offset)?,
            table_words,
            physical_spans,
            terminal_preamble: preamble,
            terminal_rows,
            terminal_trailer: trailer,
        };
        if !wire.is_complete() {
            return Err(ctx.refuse_codec_limit("NX RMFastLoad complete allocation", 0, 1));
        }
        ctx.charge_entities(1, "admit NX RMFastLoad physical storage")?;
        ctx.reserve_vec(&mut output, 1, "retain NX RMFastLoad storage records")?;
        output.push(RmFastLoadStorage(wire));
    }
    Ok(output)
}

/// Recheck the class count against declarations in the exact owning section.
pub(crate) fn admit(
    ctx: &DecodeContext<'_>,
    namespace: &NativeNamespace,
) -> Result<(), NativeConvertError> {
    if namespace
        .arenas()
        .get("rmfastload_physical_storages")
        .is_none_or(Vec::is_empty)
    {
        return Ok(());
    }
    let classes = namespace.arena_as_for_decode::<ClassDefinition>(ctx, "class_definitions")?;
    for record in
        namespace.arena_iter_as_for_decode::<RmFastLoadStorage>(ctx, "rmfastload_physical_storages")
    {
        let record = record?;
        let count = owning_class_count(ctx, &classes, record.0.source_offset)?;
        if count != index_from_u32(record.0.header_words[3]) {
            return Err(NativeConvertError::InvalidCollection(
                "RMFastLoad class count must match its owning section".into(),
            ));
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests;
