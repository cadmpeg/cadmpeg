// SPDX-License-Identifier: Apache-2.0
//! Delivery of a genuinely given-up archive before any document records escape.

use std::collections::BTreeMap;

use cadmpeg_core::decode::{u64_from_index, DecodeContext, ResourceDimension, ResourceLimit};
use cadmpeg_core::CodecError;
use cadmpeg_ir::codec::{DecodeBody, Decoded};
use cadmpeg_ir::source_fidelity::b1::{classification::ClassificationIndex, partition_stream};
use cadmpeg_ir::source_fidelity::parts::ParserPartOutput;
use cadmpeg_ir::{CadIr, ContainerSummary, SourceMeta};

pub(crate) fn decoded(
    ctx: &DecodeContext<'_>,
    summary: ContainerSummary,
    output: ParserPartOutput,
) -> Result<Decoded, CodecError> {
    let dialects = summary.dialects().cloned().ok_or_else(|| {
        CodecError::malformed("given-up FCStd archive has no admitted source identity")
    })?;
    let (model, native, mut fidelity, mut losses) = output.into_parts(ctx)?.into_parts();
    let mut ir = CadIr::decoded(SourceMeta::classified(dialects, BTreeMap::new()));
    ir.model = model;
    ir.native = native;
    ir.set_scope(cadmpeg_ir::ContentScope::container())
        .map_err(CodecError::malformed)?;
    losses.extend(summary.losses);
    crate::fidelity::install_layouts(ctx, &mut fidelity, BTreeMap::new(), BTreeMap::new())?;
    // discarded-value: validate the sole retained archive owner before releasing the document.
    let _ = crate::source_archive::image(&fidelity)?;
    let mut partitions = Vec::new();
    let streams = ctx.source_streams()?;
    reserve(ctx, &mut partitions, streams.len())?;
    type Ranges = (Vec<std::ops::Range<u64>>, Vec<std::ops::Range<u64>>);
    let mut ranges = BTreeMap::<&str, Ranges>::new();
    for part in fidelity.parts().values() {
        let (framing, given_up) = match ranges.entry(part.stream()) {
            std::collections::btree_map::Entry::Occupied(entry) => entry.into_mut(),
            std::collections::btree_map::Entry::Vacant(entry) => {
                ctx.retain_bytes(
                    u64_from_index(size_of::<(&str, Ranges)>()),
                    "FCStd failed archive part index",
                )?;
                entry.insert(Ranges::default())
            }
        };
        reserve(ctx, framing, part.framing_bytes().len())?;
        framing.extend(part.framing_bytes().iter().cloned());
        if matches!(
            part.state(),
            cadmpeg_ir::source_fidelity::parts::PartState::GivenUp {}
        ) && part.byte_len() != 0
        {
            let end = part
                .offset()
                .checked_add(part.byte_len())
                .ok_or_else(|| CodecError::malformed("failed archive extent overflows"))?;
            reserve(ctx, given_up, 1)?;
            given_up.push(part.offset()..end);
        }
    }
    for stream in streams {
        let (framing, given_up) = ranges
            .get(stream.name())
            .map_or((&[][..], &[][..]), |(framing, given_up)| {
                (framing.as_slice(), given_up.as_slice())
            });
        partitions.push(partition_stream(
            ctx,
            stream.name(),
            stream.byte_len(),
            fidelity.record_layout(),
            framing,
            &[],
            given_up,
        )?);
    }
    let owners = ClassificationIndex::new(ctx, &ir)?;
    fidelity.finalize_b1(ctx, &owners, "fcstd", &partitions, &mut losses)?;
    let digest = crate::writer::target::document_local_sha256(ctx, &ir)?;
    fidelity.metadata.insert(
        cadmpeg_ir::hash::DOCUMENT_FINGERPRINT_METADATA_KEY.into(),
        digest,
    );
    Ok(Decoded {
        body: DecodeBody {
            transfer: cadmpeg_ir::report::decode::DecodeTransfer {
                scope: ir.scope,
                geometry_transferred: false,
            },
            coverage: Default::default(),
            losses,
            notes: summary.notes,
            transfer_ledger: Default::default(),
        },
        ir,
        source_fidelity: fidelity,
    })
}

fn reserve<T>(
    ctx: &DecodeContext<'_>,
    values: &mut Vec<T>,
    count: usize,
) -> Result<(), CodecError> {
    let bytes = count
        .checked_mul(size_of::<T>())
        .map(u64_from_index)
        .ok_or_else(|| CodecError::malformed("failed archive partition storage overflows"))?;
    ctx.retain_bytes(bytes, "FCStd failed archive partition storage")?;
    values.try_reserve(count).map_err(|_| {
        ResourceLimit::allocation_failed(
            ResourceDimension::RetainedBytes,
            ctx.policy().limits.max_retained_bytes,
            bytes,
            "FCStd failed archive partition storage",
        )
        .into()
    })
}
