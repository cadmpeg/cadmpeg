// SPDX-License-Identifier: Apache-2.0
//! Classes of unconsumed spans from typed record ownership.

use std::ops::Range;

use cadmpeg_core::decode::{u64_from_index, DecodeContext};
use cadmpeg_core::CodecError;

use crate::document::{ArenaClass, ArenaClasses};
use crate::{CadIr, SourceFidelity};

use super::{normalize, StreamPartition};

const CLASSIFY: &str = "classify unconsumed source spans";
const MODEL: u8 = 1;
const DESIGN: u8 = 2;

/// An unconsumed span classified from the document's typed owners.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ClassifiedSpan {
    range: Range<u64>,
    mask: u8,
}

impl ClassifiedSpan {
    /// Maximal unconsumed range in the partition's source stream.
    pub fn range(&self) -> Range<u64> {
        self.range.clone()
    }

    /// Union of owner classes, or the conservative stream default.
    pub fn classes(&self) -> ArenaClasses {
        let mut classes = ArenaClasses::new();
        if self.mask & MODEL != 0 {
            classes.insert(ArenaClass::Model);
        }
        if self.mask & DESIGN != 0 {
            classes.insert(ArenaClass::Design);
        }
        classes
    }
}

#[derive(Debug, Clone, Copy)]
struct ClassFact<'ir> {
    id: &'ir str,
    mask: u8,
}

#[derive(Debug, Clone, Copy)]
struct NativeFact<'ir> {
    id: &'ir str,
    owners: u8,
    links: u8,
}

#[derive(Clone, Copy)]
struct Extent {
    start: u64,
    end: u64,
    mask: u8,
}

/// Reusable typed owner classes shared by every source stream of a document.
#[derive(Debug)]
pub struct ClassificationIndex<'ir> {
    classes: Vec<ClassFact<'ir>>,
    natives: Vec<NativeFact<'ir>>,
}

impl<'ir> ClassificationIndex<'ir> {
    /// Index actual neutral links and direct native owners once per document.
    pub fn new(ctx: &DecodeContext<'_>, ir: &'ir CadIr) -> Result<Self, CodecError> {
        let classes = class_facts(ctx, ir)?;
        let natives = native_facts(ctx, ir, &classes)?;
        Ok(Self { classes, natives })
    }

    /// Classify a partition from this document's indexed typed owners.
    ///
    /// Extents must be nested, equal or disjoint. Crossing extents are an
    /// inconsistent-fidelity error. Empty owner classes use both classes;
    /// design-only stream defaults need a separate proof over all goldens.
    /// Forced B1 intervals, fidelity mutation and losses are separate steps.
    pub fn classify_partition(
        &self,
        ctx: &DecodeContext<'_>,
        fidelity: &SourceFidelity,
        stream: &str,
        partition: &StreamPartition,
    ) -> Result<Vec<ClassifiedSpan>, CodecError> {
        if partition.stream() != stream {
            return Err(CodecError::malformed(
                "source partition belongs to a different stream",
            ));
        }
        let extents = owner_extents(
            ctx,
            fidelity,
            stream,
            partition.byte_len(),
            &self.classes,
            &self.natives,
        )?;
        let linked =
            unknown_link_ranges(ctx, fidelity, stream, partition.byte_len(), &self.natives)?;
        let mut active = allocate(ctx, extents.len(), 0_usize)?;
        for (index, extent) in extents.iter().enumerate() {
            while active
                .last()
                .is_some_and(|&parent| extents[parent].end <= extent.start)
            {
                active.pop();
            }
            if active
                .last()
                .is_some_and(|&parent| extents[parent].end < extent.end)
            {
                return Err(CodecError::malformed(
                    "crossing source record extents have ambiguous innermost ownership",
                ));
            }
            active.push(index);
        }
        active.clear();
        let mut output = allocate(
            ctx,
            partition.b1().len(),
            ClassifiedSpan {
                range: 0..0,
                mask: 0,
            },
        )?;
        let mut next = 0;
        for span in partition.b1() {
            while next < extents.len() && extents[next].start <= span.start {
                while active
                    .last()
                    .is_some_and(|&index| extents[index].end <= extents[next].start)
                {
                    active.pop();
                }
                active.push(next);
                next += 1;
            }
            while active
                .last()
                .is_some_and(|&index| extents[index].end < span.end)
            {
                active.pop();
            }
            let mut mask = active.last().map_or(0, |&index| extents[index].mask);
            for (bit, ranges) in [(MODEL, &linked[0]), (DESIGN, &linked[1])] {
                let index = ranges.partition_point(|range| range.end <= span.start);
                if ranges
                    .get(index)
                    .is_some_and(|range| range.start < span.end)
                {
                    mask |= bit;
                }
            }
            output.push(ClassifiedSpan {
                range: span.clone(),
                mask: if mask == 0 { MODEL | DESIGN } else { mask },
            });
        }
        Ok(output)
    }
}

/// Classify one partition without using loss codes as class evidence.
/// Reuse [`ClassificationIndex`] when classifying several streams.
pub fn classify_partition(
    ctx: &DecodeContext<'_>,
    ir: &CadIr,
    fidelity: &SourceFidelity,
    stream: &str,
    partition: &StreamPartition,
) -> Result<Vec<ClassifiedSpan>, CodecError> {
    ClassificationIndex::new(ctx, ir)?.classify_partition(ctx, fidelity, stream, partition)
}

fn allocate<T: Clone>(
    ctx: &DecodeContext<'_>,
    count: usize,
    value: T,
) -> Result<Vec<T>, CodecError> {
    ctx.retain_bytes(
        u64_from_index(count)
            .checked_mul(u64_from_index(std::mem::size_of::<T>()))
            .ok_or_else(|| {
                CodecError::malformed("source classification allocation size overflows")
            })?,
        CLASSIFY,
    )?;
    let mut values = ctx.alloc_filled(count, value, CLASSIFY)?;
    values.clear();
    Ok(values)
}

fn class_facts<'ir>(
    ctx: &DecodeContext<'_>,
    ir: &'ir CadIr,
) -> Result<Vec<ClassFact<'ir>>, CodecError> {
    let count = ir.model.entity_count().checked_mul(2).ok_or_else(|| {
        CodecError::malformed("source classification neutral record count overflows")
    })?;
    let mut facts = allocate(ctx, count, ClassFact { id: "", mask: 0 })?;
    ir.visit_neutral_provenance(&mut |record| {
        let mask = match record.class() {
            ArenaClass::Model => MODEL,
            ArenaClass::Design => DESIGN,
        };
        facts.push(ClassFact {
            id: record.id(),
            mask,
        });
        if let Some(id) = record.native_ref() {
            facts.push(ClassFact { id, mask });
        }
        Ok(())
    })?;
    facts.sort_unstable_by_key(|fact| fact.id);
    let mut count = 0;
    for index in 0..facts.len() {
        let fact = facts[index];
        if count > 0 && facts[count - 1].id == fact.id {
            facts[count - 1].mask |= fact.mask;
        } else {
            facts[count] = fact;
            count += 1;
        }
    }
    facts.truncate(count);
    Ok(facts)
}

fn class_mask(facts: &[ClassFact<'_>], id: &str) -> u8 {
    facts
        .binary_search_by(|fact| fact.id.cmp(id))
        .map_or(0, |index| facts[index].mask)
}

fn native_facts<'ir>(
    ctx: &DecodeContext<'_>,
    ir: &'ir CadIr,
    classes: &[ClassFact<'_>],
) -> Result<Vec<NativeFact<'ir>>, CodecError> {
    let count = ir
        .native
        .0
        .values()
        .flat_map(|namespace| namespace.arenas().values())
        .try_fold(0_usize, |count, records| count.checked_add(records.len()))
        .ok_or_else(|| {
            CodecError::malformed("source classification native record count overflows")
        })?;
    let mut facts = allocate(
        ctx,
        count,
        NativeFact {
            id: "",
            owners: 0,
            links: 0,
        },
    )?;
    ir.visit_native_provenance(ctx, &mut |record| {
        let owners = class_mask(classes, record.id())
            | record.owner().map_or(0, |owner| class_mask(classes, owner));
        let links = record
            .unknown_links()
            .iter()
            .fold(0, |mask, id| mask | class_mask(classes, id.as_str()));
        facts.push(NativeFact {
            id: record.id(),
            owners,
            links,
        });
        Ok(())
    })?;
    facts.sort_unstable_by_key(|fact| fact.id);
    if facts.windows(2).any(|pair| pair[0].id == pair[1].id) {
        return Err(CodecError::malformed(
            "duplicate native source owner identity",
        ));
    }
    Ok(facts)
}

fn native_fact<'a, 'ir>(facts: &'a [NativeFact<'ir>], id: &str) -> Option<&'a NativeFact<'ir>> {
    facts
        .binary_search_by(|fact| fact.id.cmp(id))
        .ok()
        .map(|index| &facts[index])
}

fn owner_extents(
    ctx: &DecodeContext<'_>,
    fidelity: &SourceFidelity,
    stream: &str,
    byte_len: u64,
    classes: &[ClassFact<'_>],
    natives: &[NativeFact<'_>],
) -> Result<Vec<Extent>, CodecError> {
    let mut extents = allocate(
        ctx,
        fidelity.record_layout().len(),
        Extent {
            start: 0,
            end: 0,
            mask: 0,
        },
    )?;
    for (id, layout) in fidelity.record_layout() {
        if layout.stream() != stream {
            continue;
        }
        let end = layout
            .offset()
            .checked_add(layout.byte_len())
            .filter(|&end| end <= byte_len)
            .ok_or_else(|| CodecError::malformed("source owner extent exceeds stream"))?;
        if layout.byte_len() == 0 {
            continue;
        }
        let mask =
            native_fact(natives, id).map_or_else(|| class_mask(classes, id), |fact| fact.owners);
        extents.push(Extent {
            start: layout.offset(),
            end,
            mask,
        });
    }
    extents.sort_unstable_by_key(|extent| (extent.start, std::cmp::Reverse(extent.end)));
    let mut count = 0;
    for index in 0..extents.len() {
        let extent = extents[index];
        if count > 0
            && extents[count - 1].start == extent.start
            && extents[count - 1].end == extent.end
        {
            extents[count - 1].mask |= extent.mask;
        } else {
            extents[count] = extent;
            count += 1;
        }
    }
    extents.truncate(count);
    Ok(extents)
}

fn unknown_link_ranges(
    ctx: &DecodeContext<'_>,
    fidelity: &SourceFidelity,
    stream: &str,
    byte_len: u64,
    natives: &[NativeFact<'_>],
) -> Result<[Vec<Range<u64>>; 2], CodecError> {
    let count = fidelity.retained_records().len();
    let mut ranges = [allocate(ctx, count, 0..0)?, allocate(ctx, count, 0..0)?];
    for (id, retained) in fidelity.retained_records() {
        if retained.stream() != stream {
            continue;
        }
        let Some(fact) = native_fact(natives, id.as_str()) else {
            continue;
        };
        let end = retained.end_offset();
        if end > byte_len {
            return Err(CodecError::malformed("unknown stub extent exceeds stream"));
        }
        if retained.byte_len() == 0 {
            continue;
        }
        for (index, bit) in [(0, MODEL), (1, DESIGN)] {
            if fact.links & bit != 0 {
                ranges[index].push(retained.offset()..end);
            }
        }
    }
    Ok([
        normalize(ctx, ranges[0].iter(), byte_len)?,
        normalize(ctx, ranges[1].iter(), byte_len)?,
    ])
}

#[cfg(test)]
mod tests;
