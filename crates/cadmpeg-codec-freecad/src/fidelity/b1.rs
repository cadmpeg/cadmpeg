// SPDX-License-Identifier: Apache-2.0
//! Complete source inventories for the adopted XML and opaque-member grammars.

use std::collections::BTreeMap;
use std::ops::Range;

use cadmpeg_core::decode::{u64_from_index, DecodeContext, ResourceDimension, ResourceLimit, View};
use cadmpeg_core::CodecError;
use cadmpeg_ir::source_fidelity::b1::{
    classification::ClassificationIndex, partition_root_stream, partition_stream, SourceRootDomain,
};
use cadmpeg_ir::source_fidelity::parts::PartState;
use cadmpeg_ir::{CadIr, SourceFidelity};

const INVENTORY: &str = "FreeCAD source framing inventory";

#[derive(Default)]
struct Stream {
    byte_len: u64,
    root: Option<SourceRootDomain>,
    framing: Vec<Range<u64>>,
    insignificant: Vec<Range<u64>>,
}

pub(crate) struct Inventory(BTreeMap<String, Stream>);

pub(crate) fn prepare(
    ctx: &DecodeContext<'_>,
    root: View<'_>,
    scan: &crate::container::Scan<'_>,
) -> Result<Inventory, CodecError> {
    prepare_sources(
        ctx,
        root,
        &scan.archive,
        &scan.data,
        &scan.source_whitespace,
    )
}

pub(crate) fn prepare_sources(
    ctx: &DecodeContext<'_>,
    root: View<'_>,
    archive: &cadmpeg_container::ArchiveSnapshot<'_>,
    sources: &BTreeMap<String, View<'_>>,
    source_whitespace: &[View<'_>],
) -> Result<Inventory, CodecError> {
    let mut actual = ctx.source_streams()?;
    actual.sort_unstable_by(|left, right| left.name().cmp(right.name()));
    let mut inventory = Inventory(BTreeMap::new());
    inventory.root(ctx, root, &actual)?;
    for source in sources.values() {
        inventory.stream(ctx, *source, &actual)?;
    }
    for stream in &actual {
        inventory.registered(ctx, stream)?;
    }
    if !inventory.is_complete(ctx)? {
        return Err(CodecError::malformed(
            "source framing inventory misses a registered stream",
        ));
    }
    for view in archive.framing_views(ctx)? {
        inventory.range(ctx, view, false)?;
    }
    for view in source_whitespace {
        inventory.range(ctx, *view, true)?;
    }
    Ok(inventory)
}

impl Inventory {
    fn root(
        &mut self,
        ctx: &DecodeContext<'_>,
        source: View<'_>,
        actual: &[cadmpeg_core::decode::SourceStream],
    ) -> Result<(), CodecError> {
        let domain = SourceRootDomain::capture(ctx, source)?;
        let at = actual
            .binary_search_by(|stream| stream.name().cmp(domain.stream()))
            .map_err(|_| CodecError::malformed("codec root has no actual source inventory"))?;
        self.registered(ctx, &actual[at])?;
        let stream = self
            .0
            .get_mut(domain.stream())
            .ok_or_else(|| CodecError::malformed("codec root source inventory disappeared"))?;
        stream.root = Some(domain);
        Ok(())
    }
    fn stream(
        &mut self,
        ctx: &DecodeContext<'_>,
        source: View<'_>,
        actual: &[cadmpeg_core::decode::SourceStream],
    ) -> Result<(), CodecError> {
        let capture = ctx.capture_part_extent(source)?;
        let at = actual
            .binary_search_by(|stream| stream.name().cmp(capture.stream()))
            .map_err(|_| CodecError::malformed("source framing has no actual registered stream"))?;
        let actual = &actual[at];
        if capture.offset() != 0 || capture.byte_len() != actual.byte_len() {
            return Err(CodecError::malformed(
                "source framing requires the whole actual stream View",
            ));
        }
        self.registered(ctx, actual)
    }

    fn registered(
        &mut self,
        ctx: &DecodeContext<'_>,
        actual: &cadmpeg_core::decode::SourceStream,
    ) -> Result<(), CodecError> {
        if let Some(previous) = self.0.get(actual.name()) {
            if previous.byte_len != actual.byte_len() {
                return Err(CodecError::malformed(
                    "conflicting source framing stream length",
                ));
            }
            return Ok(());
        }
        let bytes = actual
            .name()
            .len()
            .checked_add(size_of::<(String, Stream)>())
            .map(u64_from_index)
            .ok_or_else(|| CodecError::malformed("source framing stream storage overflows"))?;
        let mut name = String::new();
        name.try_reserve_exact(actual.name().len()).map_err(|_| {
            ResourceLimit::allocation_failed(
                ResourceDimension::RetainedBytes,
                ctx.policy().limits.max_retained_bytes,
                bytes,
                INVENTORY,
            )
        })?;
        name.push_str(actual.name());
        self.0.insert(
            name,
            Stream {
                byte_len: actual.byte_len(),
                ..Stream::default()
            },
        );
        Ok(())
    }

    fn range(
        &mut self,
        ctx: &DecodeContext<'_>,
        view: View<'_>,
        insignificant: bool,
    ) -> Result<(), CodecError> {
        let capture = ctx.capture_part_extent(view)?;
        let stream = self.0.get_mut(capture.stream()).ok_or_else(|| {
            CodecError::malformed("source framing range has no admitted actual stream")
        })?;
        let end = capture
            .offset()
            .checked_add(capture.byte_len())
            .filter(|end| *end <= stream.byte_len)
            .ok_or_else(|| {
                CodecError::malformed("source framing range escapes its actual stream")
            })?;
        if capture.byte_len() == 0 {
            return Ok(());
        }
        let ranges = if insignificant {
            &mut stream.insignificant
        } else {
            &mut stream.framing
        };
        reserve(ctx, ranges, 1)?;
        ranges.push(capture.offset()..end);
        Ok(())
    }

    fn is_complete(&self, ctx: &DecodeContext<'_>) -> Result<bool, CodecError> {
        let actual = ctx.source_streams()?;
        Ok(self.matches_actual(&actual))
    }

    fn matches_actual(&self, actual: &[cadmpeg_core::decode::SourceStream]) -> bool {
        actual.len() == self.0.len()
            && actual.iter().all(|stream| {
                self.0
                    .get(stream.name())
                    .is_some_and(|entry| entry.byte_len == stream.byte_len())
            })
    }

    pub(crate) fn finalize(
        mut self,
        ctx: &DecodeContext<'_>,
        ir: &CadIr,
        fidelity: &mut SourceFidelity,
        losses: &mut Vec<cadmpeg_ir::report::loss::LossNote>,
    ) -> Result<(), CodecError> {
        let mut actual = ctx.source_streams()?;
        actual.sort_unstable_by(|left, right| left.name().cmp(right.name()));
        if !self.matches_actual(&actual) {
            return Err(CodecError::malformed(
                "B1 framing inventory no longer covers every actual stream",
            ));
        }
        for part in fidelity.parts().values() {
            let stream = self
                .0
                .get_mut(part.stream())
                .ok_or_else(|| CodecError::malformed("part framing has no actual source stream"))?;
            for range in part.framing_bytes() {
                reserve(ctx, &mut stream.framing, 1)?;
                stream.framing.push(range.clone());
            }
        }
        self.project_alias_ranges(ctx, &actual)?;
        let mut given_up = BTreeMap::<&str, Vec<Range<u64>>>::new();
        for part in fidelity
            .parts()
            .values()
            .filter(|part| part.state() == &PartState::GivenUp {})
        {
            if !self.0.contains_key(part.stream()) {
                return Err(CodecError::malformed(
                    "given-up part has no actual framing stream",
                ));
            }
            let end = part
                .offset()
                .checked_add(part.byte_len())
                .ok_or_else(|| CodecError::malformed("given-up source extent overflows"))?;
            // A failed empty member still has a real part/stub, but contains
            // no bytes to subtract from semantic reads or classify as B1.
            if part.byte_len() == 0 {
                continue;
            }
            let ranges = match given_up.entry(part.stream()) {
                std::collections::btree_map::Entry::Occupied(entry) => entry.into_mut(),
                std::collections::btree_map::Entry::Vacant(entry) => entry.insert(Vec::new()),
            };
            reserve(ctx, ranges, 1)?;
            ranges.push(part.offset()..end);
        }
        let mut partitions = Vec::new();
        reserve(ctx, &mut partitions, self.0.len())?;
        for (name, stream) in self.0 {
            let failed = given_up.get(name.as_str()).map_or(&[][..], Vec::as_slice);
            partitions.push(if let Some(root) = stream.root {
                partition_root_stream(
                    ctx,
                    root,
                    fidelity.record_layout(),
                    &stream.framing,
                    &stream.insignificant,
                    failed,
                )?
            } else {
                partition_stream(
                    ctx,
                    &name,
                    stream.byte_len,
                    fidelity.record_layout(),
                    &stream.framing,
                    &stream.insignificant,
                    failed,
                )?
            });
        }
        let owners = ClassificationIndex::new_with_owner_resolver(ctx, ir, &super::owners::Owners)?;
        fidelity.finalize_b1(ctx, &owners, "fcstd", &partitions, losses)
    }
}

fn reserve<T>(
    ctx: &DecodeContext<'_>,
    values: &mut Vec<T>,
    additional: usize,
) -> Result<(), CodecError> {
    let bytes = u64_from_index(additional)
        .checked_mul(u64_from_index(size_of::<T>()))
        .ok_or_else(|| CodecError::malformed("source framing inventory allocation overflows"))?;
    values.try_reserve(additional).map_err(|_| {
        ResourceLimit::allocation_failed(
            ResourceDimension::RetainedBytes,
            ctx.policy().limits.max_retained_bytes,
            bytes,
            INVENTORY,
        )
        .into()
    })
}

mod projection;

#[cfg(test)]
mod tests;

#[cfg(test)]
pub(crate) fn assert_classified_spans(decoded: &cadmpeg_ir::codec::DecodeResult) {
    let fidelity = decoded.source_fidelity();
    let mut classes = cadmpeg_ir::ArenaClasses::new();
    for (stream, coverage) in fidelity.b1_spans() {
        for span in coverage.spans() {
            classes.extend(span.classes());
            let range = span.range();
            assert!(
                decoded.report().losses.iter().any(|loss| {
                    loss.provenance.as_ref().is_some_and(|location| {
                        location.stream_name() == Some(stream.as_str())
                            && location.offset <= range.start
                            && location
                                .byte_len()
                                .and_then(|len| location.offset.checked_add(len))
                                .is_some_and(|end| end >= range.end)
                    })
                }),
                "B1 {stream}:{range:?} has no located diagnostic"
            );
        }
    }
    assert_eq!(fidelity.unrepresented(), classes);
}
