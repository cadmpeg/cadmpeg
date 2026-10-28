// SPDX-License-Identifier: Apache-2.0
//! Delivery of a genuinely given-up archive before any document records escape.

use std::collections::BTreeMap;

use cadmpeg_core::decode::{u64_from_index, DecodeContext, ResourceDimension, ResourceLimit};
use cadmpeg_core::CodecError;
use cadmpeg_ir::codec::{DecodeBody, Decoded};
use cadmpeg_ir::source_fidelity::b1::{classification::ClassificationIndex, partition_stream};
use cadmpeg_ir::source_fidelity::parts::PartOutput;
use cadmpeg_ir::{CadIr, ContainerSummary, SourceMeta};

pub(crate) fn decoded(
    ctx: &DecodeContext<'_>,
    root: cadmpeg_core::decode::View<'_>,
    summary: ContainerSummary,
    output: PartOutput,
    pointers: BTreeMap<String, BTreeMap<String, u64>>,
) -> Result<Decoded, CodecError> {
    let source = match summary.dialects().cloned() {
        Some(dialects) => SourceMeta::classified(dialects, BTreeMap::new()),
        None => SourceMeta::unclassified(cadmpeg_ir::CodecFormat::Fcstd, BTreeMap::new()),
    };
    let (model, native, mut fidelity, mut losses) = output.into_parts(ctx)?.into_parts();
    let mut ir = CadIr::decoded(source);
    ir.model = model;
    ir.native = native;
    // A located document class remains held even when its part, or an
    // enclosing archive, gave up. Covered descendants still prove it was found.
    let document_found = fidelity.parts().values().any(|part| {
        matches!(
            part.level().as_str(),
            "freecad.document_xml" | "freecad.document"
        )
    });
    let scope = if document_found {
        ctx.depth().required_scope()
    } else {
        cadmpeg_ir::ContentScope::container()
    };
    ir.set_scope(scope).map_err(CodecError::malformed)?;
    losses.extend(summary.losses);
    if !fidelity.parts().values().any(|part| {
        part.parent().is_none()
            && part.state() == &cadmpeg_ir::source_fidelity::parts::PartState::GivenUp {}
    }) {
        let capture = ctx.capture_part_extent(root)?;
        ctx.retain_bytes(
            u64_from_index(root.window().len()),
            "retain FCStd source image",
        )?;
        crate::fidelity::retain(
            &mut fidelity,
            crate::fidelity::SOURCE_IMAGE,
            capture.stream(),
            capture.offset(),
            root.window(),
            cadmpeg_ir::ArenaClasses::new(),
        )?;
    }
    crate::fidelity::install_layouts(ctx, &mut fidelity, BTreeMap::new(), pointers)?;
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
    let owners =
        ClassificationIndex::new_with_owner_resolver(ctx, &ir, &crate::fidelity::owners::Owners)?;
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
