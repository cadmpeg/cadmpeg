// SPDX-License-Identifier: Apache-2.0
//! Inventor compound-container classification.

use cadmpeg_core::container::ContainerRole;
use cadmpeg_core::ContainerEntry;

use cadmpeg_container::compound::{CompoundEntry, CompoundSnapshot};
use cadmpeg_core::decode::{DecodeContext, View};
use cadmpeg_core::CodecError;
use cadmpeg_ir::ContainerSummary;

use crate::external_reference::{parse as parse_ufrx, UfrxState};
use crate::property_set::{inventory as property_set_inventory, PropertySetDescriptor};
use crate::protein::{parse as parse_protein, ProteinState};
use crate::rse::SegmentBulkState;
use crate::rse::{database_band, direct_rse_child, RseInventory, SegmentMetaState};

/// One parsed Inventor compound container.
pub(crate) struct InventorContainer<'a> {
    pub(crate) snapshot: CompoundSnapshot<'a>,
    pub(crate) rse: RseInventory<'a>,
    pub(crate) property_sets: Vec<PropertySetDescriptor<'a>>,
    pub(crate) protein: ProteinState<'a>,
    pub(crate) ufrx: UfrxState<'a>,
}

impl<'a> InventorContainer<'a> {
    pub(crate) fn open(ctx: &DecodeContext<'a>, root: View<'a>) -> Result<Self, CodecError> {
        let snapshot = CompoundSnapshot::new(ctx, root)?;
        if !matches!(
            snapshot.entry(ctx, "RSeStorage")?,
            Some(CompoundEntry::Storage(_))
        ) {
            return Err(CodecError::Malformed(
                "Inventor document has no RSeStorage storage".into(),
            ));
        }
        let rse = RseInventory::build(ctx, &snapshot)?;
        let property_sets = property_set_inventory(ctx, &snapshot)?;
        let protein = parse_protein(ctx, &snapshot)?;
        let ufrx = parse_ufrx(ctx, &snapshot, &rse.document_kind(ctx)?)?;
        Ok(Self {
            snapshot,
            rse,
            property_sets,
            protein,
            ufrx,
        })
    }

    pub(crate) fn summary(&self, ctx: &DecodeContext<'_>) -> Result<ContainerSummary, CodecError> {
        admit_container_entries(ctx, &self.snapshot)?;
        let mut entries = self.snapshot.container_entries(ctx, |entry| match entry {
            CompoundEntry::Storage(_) => ContainerRole::Storage,
            CompoundEntry::Stream(_) => ContainerRole::Stream,
        })?;
        for (index, source) in ctx
            .admit_iter(self.snapshot.entries(), "classify Inventor summary entries")?
            .enumerate()
        {
            entries[index].role = classify(ctx, source)?;
        }
        for segment in ctx.admit_iter(&self.rse.segments, "visit Inventor summary segments")? {
            let Some(entry) =
                find_summary_entry(ctx, &mut entries, segment.pair.metadata.directory_id())?
            else {
                continue;
            };
            match &segment.meta {
                SegmentMetaState::Parsed(meta) => {
                    insert_attribute(ctx, entry, "inner_framing", format_args!("zlib"))?;
                    insert_attribute(
                        ctx,
                        entry,
                        "expanded_size",
                        format_args!("{}", meta.body.window().len()),
                    )?;
                    insert_attribute(
                        ctx,
                        entry,
                        "meta_marker",
                        format_args!("{}", meta.declared.marker),
                    )?;
                    insert_attribute(
                        ctx,
                        entry,
                        "meta_stream_version",
                        format_args!("{}", meta.declared.version),
                    )?;
                    insert_attribute(
                        ctx,
                        entry,
                        "segment_kind",
                        format_args!("{}", segment.kind.label()),
                    )?;
                    insert_attribute(
                        ctx,
                        entry,
                        "display_name",
                        format_args!("{}", meta.display_name),
                    )?;
                }
                SegmentMetaState::Malformed { declared, detail } => {
                    if let Some(declared) = declared {
                        insert_attribute(
                            ctx,
                            entry,
                            "meta_marker",
                            format_args!("{}", declared.marker),
                        )?;
                        insert_attribute(
                            ctx,
                            entry,
                            "meta_stream_version",
                            format_args!("{}", declared.version),
                        )?;
                    }
                    insert_attribute(ctx, entry, "framing_error", format_args!("{detail}"))?;
                }
            }
            let Some(bulk_entry) =
                find_summary_entry(ctx, &mut entries, segment.pair.bulk.directory_id())?
            else {
                continue;
            };
            match &segment.bulk {
                SegmentBulkState::Framed(bulk) => {
                    insert_attribute(ctx, bulk_entry, "inner_framing", format_args!("zlib"))?;
                    insert_attribute(
                        ctx,
                        bulk_entry,
                        "bulk_form",
                        format_args!("0x{:04x}", bulk.form.value()),
                    )?;
                    insert_attribute(
                        ctx,
                        bulk_entry,
                        "expanded_size",
                        format_args!("{}", bulk.expanded.window().len()),
                    )?;
                }
                SegmentBulkState::Malformed(error) => {
                    insert_attribute(ctx, bulk_entry, "framing_error", format_args!("{error}"))?;
                }
            }
        }
        let mut recovery_storage =
            ctx.reserve_scoped(0, "collect Inventor dialect declarations")?;
        let recovery =
            recovery_storage.with_storage(|| crate::dialect::DialectRecovery::of(ctx, self))?;
        let matched = recovery.classify(ctx)?;
        let mut losses = Vec::new();
        if let Some(loss) = crate::dialect::dialect_loss(ctx, &matched, &recovery)? {
            ctx.push_vec(&mut losses, loss, "collect Inventor summary loss")?;
        }
        let dialects = crate::dialect::layers(ctx, &matched, &self.rse.active_carrier)?;
        if let Some(kernel) = dialects
            .iter()
            .find(|matched| matched.format() == cadmpeg_asm::dialect::FORMAT)
        {
            if let Some(loss) = crate::dialect::kernel_dialect_loss(ctx, kernel)? {
                ctx.push_vec(&mut losses, loss, "collect Inventor summary loss")?;
            }
        }
        let note = summary_note(
            ctx,
            self.snapshot.major_version(),
            self.rse.segments.len(),
            self.rse.databases.len(),
        )?;
        Ok(ContainerSummary::classified(
            dialects,
            cadmpeg_ir::ContainerKind::Cfb,
            entries,
            losses,
            vec![note],
        ))
    }
}

fn summary_note(
    ctx: &DecodeContext<'_>,
    major: u16,
    segment_count: usize,
    database_count: usize,
) -> Result<String, CodecError> {
    ctx.charge_collection_items(1, "collect Inventor summary note")?;
    ctx.charge_formatted_retained(format_args!(
            "CFB v{major} with {segment_count} RSe segment pair(s) and {database_count} versioned database(s)"
        ), "retain Inventor summary note")?;
    Ok(format!(
        "CFB v{major} with {segment_count} RSe segment pair(s) and {database_count} versioned database(s)"
    ))
}

fn admit_container_entries(
    ctx: &DecodeContext<'_>,
    snapshot: &CompoundSnapshot<'_>,
) -> Result<(), CodecError> {
    let count = cadmpeg_core::decode::u64_from_index(snapshot.entries().len());
    ctx.charge_collection_items(count, "collect Inventor container summary entries")?;
    for entry in ctx.admit_iter(snapshot.entries(), "visit Inventor container entries")? {
        let path_len = cadmpeg_core::decode::u64_from_index(entry.path().len());
        ctx.charge_retained(path_len, "retain Inventor summary entry path")?;
        ctx.charge_collection_items(1, "collect Inventor summary directory attribute")?;
        ctx.charge_retained(12, "retain Inventor summary directory key")?;
        ctx.charge_formatted_retained(
            format_args!("{}", entry.directory_id()),
            "retain Inventor summary directory id",
        )?;
        if let CompoundEntry::Stream(stream) = entry {
            if let Some(allocation) = stream.allocation() {
                ctx.charge_collection_items(1, "collect Inventor summary allocation attribute")?;
                ctx.charge_retained(10, "retain Inventor summary allocation key")?;
                ctx.charge_retained(
                    cadmpeg_core::decode::u64_from_index(allocation.label().len()),
                    "retain Inventor summary allocation label",
                )?;
            }
            ctx.charge_collection_items(1, "collect Inventor summary start-sector attribute")?;
            ctx.charge_retained(12, "retain Inventor summary start-sector key")?;
            ctx.charge_formatted_retained(
                format_args!("{}", stream.start_sector()),
                "retain Inventor summary start sector",
            )?;
        }
    }
    Ok(())
}

fn find_summary_entry<'a>(
    ctx: &DecodeContext<'_>,
    entries: &'a mut [ContainerEntry],
    directory_id: u32,
) -> Result<Option<&'a mut ContainerEntry>, CodecError> {
    for entry in entries {
        ctx.charge_work(1, "find Inventor summary entry")?;
        let matches = match entry.attributes.get("directory_id") {
            Some(value) => {
                ctx.parse_text::<u32>(value, "parse Inventor summary directory id")?
                    == Ok(directory_id)
            }
            None => false,
        };
        if matches {
            return Ok(Some(entry));
        }
    }
    Ok(None)
}

fn insert_attribute(
    ctx: &DecodeContext<'_>,
    entry: &mut ContainerEntry,
    key: &'static str,
    value: std::fmt::Arguments<'_>,
) -> Result<(), CodecError> {
    let key = ctx.copy_retained_text(key, "retain Inventor summary attribute key")?;
    let value = ctx.format_retained(value, "retain Inventor summary attribute value")?;
    ctx.insert_btree_map(
        &mut entry.attributes,
        key,
        value,
        "collect Inventor summary attribute",
    )?;
    Ok(())
}

pub(crate) fn has_inventor_evidence(
    ctx: &DecodeContext<'_>,
    paths: &[String],
) -> Result<bool, CodecError> {
    let mut has_storage = false;
    for path in ctx.admit_iter(paths, "visit Inventor directory storage evidence")? {
        if ctx.eq_ignore_ascii_case(path, "RSeStorage", "Inventor directory storage evidence")? {
            has_storage = true;
            break;
        }
    }
    for path in ctx.admit_iter(paths, "visit Inventor directory corroboration")? {
        if ctx.eq_ignore_ascii_case(
            path,
            "RSeStorage/RSeSegInfo",
            "Inventor directory corroboration",
        )? || database_band(ctx, path)?.is_some()
        {
            return Ok(has_storage);
        }
    }
    Ok(false)
}

fn classify(ctx: &DecodeContext<'_>, entry: &CompoundEntry) -> Result<ContainerRole, CodecError> {
    let path = entry.path();
    if ctx.eq_ignore_ascii_case(path, "RSeStorage", "classify Inventor entry path")? {
        return Ok(ContainerRole::RseStorage);
    }
    if database_band(ctx, path)?.is_some() {
        return Ok(ContainerRole::RseDatabase);
    }
    if ctx.eq_ignore_ascii_case(
        path,
        "RSeStorage/RSeSegInfo",
        "classify Inventor entry path",
    )? {
        return Ok(ContainerRole::RseSegmentRegistry);
    }
    if ctx.eq_ignore_ascii_case(
        path,
        "RSeStorage/RSeDbRevisionInfo",
        "classify Inventor entry path",
    )? {
        return Ok(ContainerRole::RseRevisionTable);
    }
    if ctx.eq_ignore_ascii_case(path, "Protein", "classify Inventor entry path")? {
        return Ok(ContainerRole::Protein);
    }
    if ctx.eq_ignore_ascii_case(path, "UFRxDoc", "classify Inventor entry path")?
        || is_reference_file(ctx, path)?
    {
        return Ok(ContainerRole::ExternalReference);
    }
    if let Some(name) = direct_rse_child(ctx, path)? {
        if name.starts_with('M') {
            return Ok(ContainerRole::RseSegmentMetadata);
        }
        if name.starts_with('B') {
            return Ok(ContainerRole::RseSegmentBulk);
        }
    }
    Ok(match entry {
        CompoundEntry::Storage(_) => ContainerRole::Storage,
        CompoundEntry::Stream(_) => ContainerRole::Stream,
    })
}

fn is_reference_file(ctx: &DecodeContext<'_>, path: &str) -> Result<bool, CodecError> {
    let mut components = path.split('/');
    let Some(storage) = components.next() else {
        return Ok(false);
    };
    if !ctx.eq_ignore_ascii_case(storage, "RSeStorage", "match Inventor reference storage")? {
        return Ok(false);
    }
    let Some(directory) = components.next() else {
        return Ok(false);
    };
    Ok(
        ctx.eq_ignore_ascii_case(directory, "RefdFiles", "match Inventor reference directory")?
            && components.next().is_some(),
    )
}

#[cfg(test)]
mod tests;
