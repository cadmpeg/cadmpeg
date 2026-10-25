// SPDX-License-Identifier: Apache-2.0
//! Delivery of a genuinely given-up archive before any document records escape.

use std::collections::BTreeMap;

use cadmpeg_core::decode::{u64_from_index, DecodeContext, ResourceDimension, ResourceLimit};
use cadmpeg_core::CodecError;
use cadmpeg_ir::codec::{DecodeBody, Decoded};
use cadmpeg_ir::source_fidelity::b1::{classification::ClassificationIndex, partition_stream};
use cadmpeg_ir::source_fidelity::parts::PartOutput;
use cadmpeg_ir::{CadIr, ContainerSummary, SourceMeta};

pub(crate) struct Source<'source> {
    pub(crate) archive: std::rc::Rc<cadmpeg_container::ArchiveSnapshot<'source>>,
    pub(crate) entries: BTreeMap<String, cadmpeg_core::decode::View<'source>>,
    pub(crate) entry_parents: BTreeMap<String, cadmpeg_ir::source_fidelity::parts::PartParent>,
    pub(crate) entry_records: Vec<crate::native::EntryRecord>,
    pub(crate) entry_owners: crate::container::parts::EntryOwners<'source>,
    pub(crate) entry_interpretations: crate::gui::payloads::sources::EntryInterpretations<'source>,
    pub(crate) gui: crate::gui::Graph,
    pub(crate) source_whitespace: Vec<cadmpeg_core::decode::View<'source>>,
}

pub(crate) fn decoded(
    ctx: &DecodeContext<'_>,
    root: cadmpeg_core::decode::View<'_>,
    summary: ContainerSummary,
    mut output: PartOutput,
    mut pointers: BTreeMap<String, BTreeMap<String, u64>>,
    mut recovered: Option<Source<'_>>,
) -> Result<Decoded, CodecError> {
    if let Some(recovered) = &mut recovered {
        crate::container::parts::merge_entry_contents(
            ctx,
            &mut recovered.entry_records,
            std::mem::take(&mut recovered.gui.entry_contents),
        )?;
        crate::container::parts::stage_entries(
            ctx,
            std::mem::take(&mut recovered.entry_records),
            &[],
            &recovered.gui,
            &mut output,
        )?;
    }
    let source = match summary.dialects().cloned() {
        Some(dialects) => SourceMeta::classified(dialects, BTreeMap::new()),
        None => SourceMeta::unclassified(cadmpeg_ir::CodecFormat::Fcstd, BTreeMap::new()),
    };
    let mut payloads = BTreeMap::new();
    if let Some(recovered) = &recovered {
        crate::fidelity::gui_layout_payloads(
            ctx,
            &recovered.entries,
            &recovered.gui,
            &mut payloads,
            &mut pointers,
        )?;
    }
    let ((model, native, mut fidelity, mut losses), dropped, capture_pruning) = output
        .into_parts(ctx)?
        .into_parts_with_dropped_and_capture_pruning();
    for id in dropped {
        payloads.remove(&id);
        pointers.remove(&id);
    }
    let mut ir = CadIr::decoded(source);
    ir.model = model;
    ir.native = native;
    // A located document class remains held even when its part, or an
    // enclosing archive, gave up. Covered descendants still prove it was found.
    let scope = crate::container::located_scope(ctx.depth(), &fidelity);
    ir.set_scope(scope).map_err(CodecError::malformed)?;
    losses.extend(summary.losses);
    if let Some(recovered) = &recovered {
        losses.extend(recovered.gui.losses.iter().cloned());
        if !ctx.container_only() {
            let entries = ir
                .native
                .namespace("fcstd")
                .map(|namespace| namespace.arena_as::<crate::native::EntryRecord>("entries"))
                .transpose()?
                .unwrap_or_default();
            let declared_entries = entries
                .iter()
                .map(|entry| entry.name().to_owned())
                .collect();
            let mut graph = crate::gui::Graph {
                documents: ir
                    .native
                    .namespace("fcstd")
                    .map(|namespace| namespace.arena_as("gui_documents"))
                    .transpose()?
                    .unwrap_or_default(),
                providers: ir
                    .native
                    .namespace("fcstd")
                    .map(|namespace| namespace.arena_as("gui_view_providers"))
                    .transpose()?
                    .unwrap_or_default(),
                properties: ir
                    .native
                    .namespace("fcstd")
                    .map(|namespace| namespace.arena_as("gui_properties"))
                    .transpose()?
                    .unwrap_or_default(),
                ..crate::gui::Graph::default()
            };
            let reader = crate::property_reader::PropertyReader::typed(ctx).with_entries(entries);
            let mut admitted_entities =
                cadmpeg_core::decode::u64_from_index(ir.model.entity_count());
            let entry_sources = crate::entry_sources::EntrySources::new(ctx, &recovered.entries)?
                .with_parents(&recovered.entry_parents)
                .with_owners(&recovered.entry_owners);
            crate::application_geometry::parts::transfer(
                &reader,
                &mut ir,
                &[],
                &mut admitted_entities,
                Some(&entry_sources),
                recovered.entries.get("GuiDocument.xml").copied(),
                &recovered.gui.property_sources,
            )?;
            crate::gui::project_records(
                ctx,
                &mut ir,
                &mut graph,
                &crate::gui::GuiSources {
                    entries: &recovered.entries,
                    declared_entries: Some(&declared_entries),
                    declared_objects: Some(&std::collections::BTreeSet::new()),
                    admitted_objects: Some(&std::collections::BTreeSet::new()),
                    entry_parents: Some(&recovered.entry_parents),
                    entry_owners: Some(&recovered.entry_owners),
                    entry_interpretations: None,
                    gui_entry_interpretations: Some(&recovered.entry_interpretations),
                    objects: &[],
                    object_sources: None,
                    properties: &[],
                    payloads: &[],
                    element_maps: &[],
                    topology_occurrences: &[],
                    requires_alpha_conversion: false,
                },
            )?;
            losses.append(&mut graph.losses);
        }
    }
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
        )?;
    }
    let inventory = recovered
        .as_ref()
        .map(|recovered| {
            crate::fidelity::b1::prepare_sources(
                ctx,
                root,
                &recovered.archive,
                &recovered.entries,
                &recovered.source_whitespace,
            )
        })
        .transpose()?;
    let mut root_domain = if inventory.is_none() {
        Some(cadmpeg_ir::source_fidelity::b1::SourceRootDomain::capture(
            ctx, root,
        )?)
    } else {
        None
    };
    crate::fidelity::install_layouts(
        ctx,
        &mut fidelity,
        payloads,
        pointers,
        Some(capture_pruning),
    )?;
    // discarded-value: validate the sole retained archive owner before releasing the document.
    let _ = crate::source_archive::image(&fidelity)?;
    if let Some(inventory) = inventory {
        inventory.finalize(ctx, &ir, &mut fidelity, &mut losses)?;
    } else {
        let mut partitions = Vec::new();
        let streams = ctx.source_streams()?;
        reserve(ctx, &mut partitions, streams.len())?;
        type Ranges = (Vec<std::ops::Range<u64>>, Vec<std::ops::Range<u64>>);
        let mut ranges = BTreeMap::<&str, Ranges>::new();
        for part in fidelity.parts().values() {
            let (framing, given_up) = match ranges.entry(part.stream()) {
                std::collections::btree_map::Entry::Occupied(entry) => entry.into_mut(),
                std::collections::btree_map::Entry::Vacant(entry) => {
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
            partitions.push(
                if root_domain
                    .as_ref()
                    .is_some_and(|root| root.stream() == stream.name())
                {
                    let domain = root_domain.take().ok_or_else(|| {
                        CodecError::malformed("archive recovery lost its root domain")
                    })?;
                    cadmpeg_ir::source_fidelity::b1::partition_root_stream(
                        ctx,
                        domain,
                        fidelity.record_layout(),
                        framing,
                        &[],
                        given_up,
                    )?
                } else {
                    partition_stream(
                        ctx,
                        stream.name(),
                        stream.byte_len(),
                        fidelity.record_layout(),
                        framing,
                        &[],
                        given_up,
                    )?
                },
            );
        }
        let owners = ClassificationIndex::new_with_owner_resolver(
            ctx,
            &ir,
            &crate::fidelity::owners::Owners,
        )?;
        fidelity.finalize_b1(ctx, &owners, "fcstd", &partitions, &mut losses)?;
    }
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
