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
use crate::record_issue::admit_formatted;
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
            snapshot.entry("RSeStorage"),
            Some(CompoundEntry::Storage(_))
        ) {
            return Err(CodecError::Malformed(
                "Inventor document has no RSeStorage storage".into(),
            ));
        }
        let rse = RseInventory::build(ctx, &snapshot)?;
        let property_sets = property_set_inventory(ctx, &snapshot)?;
        let protein = parse_protein(ctx, &snapshot)?;
        let ufrx = parse_ufrx(ctx, &snapshot, &rse.document_kind())?;
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
        let mut entries = self.snapshot.container_entries(classify);
        for segment in &self.rse.segments {
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
        let recovery = crate::dialect::DialectRecovery::of(ctx, self)?;
        let matched = recovery.classify(ctx)?;
        let mut losses = Vec::new();
        if let Some(loss) = crate::dialect::dialect_loss(ctx, &matched, &recovery)? {
            ctx.charge_collection_items(1, "collect Inventor summary loss")?;
            losses.push(loss);
        }
        let dialects = crate::dialect::layers(matched, &self.rse.active_carrier)?;
        if let Some(loss) = dialects
            .iter()
            .find(|matched| matched.format() == cadmpeg_asm::dialect::FORMAT)
            .and_then(crate::dialect::kernel_dialect_loss)
        {
            ctx.charge_collection_items(1, "collect Inventor summary loss")?;
            losses.push(loss);
        }
        ctx.charge_collection_items(1, "collect Inventor summary note")?;
        admit_formatted(
            ctx,
            format_args!(
                "CFB v{} with {} RSe segment pair(s) and {} versioned database(s)",
                self.snapshot.major_version(),
                self.rse.segments.len(),
                self.rse.databases.len()
            ),
            "retain Inventor summary note",
        )?;
        Ok(ContainerSummary::classified(
            dialects,
            cadmpeg_ir::ContainerKind::Cfb,
            entries,
            losses,
            vec![format!(
                "CFB v{} with {} RSe segment pair(s) and {} versioned database(s)",
                self.snapshot.major_version(),
                self.rse.segments.len(),
                self.rse.databases.len()
            )],
        ))
    }
}

fn admit_container_entries(
    ctx: &DecodeContext<'_>,
    snapshot: &CompoundSnapshot<'_>,
) -> Result<(), CodecError> {
    let count = u64::try_from(snapshot.entries().len()).map_err(|_| {
        ctx.refuse_codec_limit("Inventor summary entry count", u64::MAX - 1, u64::MAX)
    })?;
    ctx.charge_collection_items(count, "collect Inventor container summary entries")?;
    for entry in snapshot.entries() {
        let path_len = u64::try_from(entry.path().len()).map_err(|_| {
            ctx.refuse_codec_limit("Inventor summary path length", u64::MAX - 1, u64::MAX)
        })?;
        ctx.charge_retained(path_len, "retain Inventor summary entry path")?;
        ctx.charge_collection_items(1, "collect Inventor summary directory attribute")?;
        ctx.charge_retained(12, "retain Inventor summary directory key")?;
        admit_formatted(
            ctx,
            format_args!("{}", entry.directory_id()),
            "retain Inventor summary directory id",
        )?;
        if let CompoundEntry::Stream(stream) = entry {
            if let Some(allocation) = stream.allocation() {
                ctx.charge_collection_items(1, "collect Inventor summary allocation attribute")?;
                ctx.charge_retained(10, "retain Inventor summary allocation key")?;
                ctx.charge_retained(
                    u64::try_from(allocation.label().len()).map_err(|_| {
                        ctx.refuse_codec_limit(
                            "Inventor allocation label length",
                            u64::MAX - 1,
                            u64::MAX,
                        )
                    })?,
                    "retain Inventor summary allocation label",
                )?;
            }
            ctx.charge_collection_items(1, "collect Inventor summary start-sector attribute")?;
            ctx.charge_retained(12, "retain Inventor summary start-sector key")?;
            admit_formatted(
                ctx,
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
        if entry
            .attributes
            .get("directory_id")
            .is_some_and(|value| value.parse::<u32>() == Ok(directory_id))
        {
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
    ctx.charge_collection_items(1, "collect Inventor summary attribute")?;
    ctx.charge_retained(
        u64::try_from(key.len()).map_err(|_| {
            ctx.refuse_codec_limit(
                "Inventor summary attribute key length",
                u64::MAX - 1,
                u64::MAX,
            )
        })?,
        "retain Inventor summary attribute key",
    )?;
    admit_formatted(ctx, value, "retain Inventor summary attribute value")?;
    entry.attributes.insert(key.into(), value.to_string());
    Ok(())
}

pub(crate) fn has_inventor_evidence(paths: &[String]) -> bool {
    let has_storage = paths
        .iter()
        .any(|path| path.eq_ignore_ascii_case("RSeStorage"));
    let corroborated = paths.iter().any(|path| {
        path.eq_ignore_ascii_case("RSeStorage/RSeSegInfo") || database_band(path).is_some()
    });
    has_storage && corroborated
}

fn classify(entry: &CompoundEntry) -> ContainerRole {
    let path = entry.path();
    if path.eq_ignore_ascii_case("RSeStorage") {
        return ContainerRole::RseStorage;
    }
    if database_band(path).is_some() {
        return ContainerRole::RseDatabase;
    }
    if path.eq_ignore_ascii_case("RSeStorage/RSeSegInfo") {
        return ContainerRole::RseSegmentRegistry;
    }
    if path.eq_ignore_ascii_case("RSeStorage/RSeDbRevisionInfo") {
        return ContainerRole::RseRevisionTable;
    }
    if path.eq_ignore_ascii_case("Protein") {
        return ContainerRole::Protein;
    }
    if path.eq_ignore_ascii_case("UFRxDoc") || is_reference_file(path) {
        return ContainerRole::ExternalReference;
    }
    if let Some(name) = direct_rse_child(path) {
        if name.starts_with('M') {
            return ContainerRole::RseSegmentMetadata;
        }
        if name.starts_with('B') {
            return ContainerRole::RseSegmentBulk;
        }
    }
    match entry {
        CompoundEntry::Storage(_) => ContainerRole::Storage,
        CompoundEntry::Stream(_) => ContainerRole::Stream,
    }
}

fn is_reference_file(path: &str) -> bool {
    let mut components = path.split('/');
    components
        .next()
        .is_some_and(|name| name.eq_ignore_ascii_case("RSeStorage"))
        && components
            .next()
            .is_some_and(|name| name.eq_ignore_ascii_case("RefdFiles"))
        && components.next().is_some()
}

#[cfg(test)]
mod tests;
