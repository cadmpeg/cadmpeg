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

    pub(crate) fn summary(&self) -> Result<ContainerSummary, CodecError> {
        let mut entries = self.snapshot.container_entries(classify);
        for segment in &self.rse.segments {
            let Some(entry) =
                find_summary_entry(&mut entries, segment.pair.metadata.directory_id())
            else {
                continue;
            };
            match &segment.meta {
                SegmentMetaState::Parsed(meta) => {
                    insert_attribute(entry, "inner_framing", "zlib");
                    insert_attribute(entry, "expanded_size", meta.body.window().len());
                    insert_attribute(entry, "meta_marker", &meta.declared.marker);
                    insert_attribute(entry, "meta_stream_version", meta.declared.version);
                    insert_attribute(entry, "segment_kind", segment.kind.label());
                    insert_attribute(entry, "display_name", &meta.display_name);
                }
                SegmentMetaState::Malformed { declared, detail } => {
                    if let Some(declared) = declared {
                        insert_attribute(entry, "meta_marker", &declared.marker);
                        insert_attribute(entry, "meta_stream_version", declared.version);
                    }
                    insert_attribute(entry, "framing_error", detail);
                }
            }
            let Some(bulk_entry) =
                find_summary_entry(&mut entries, segment.pair.bulk.directory_id())
            else {
                continue;
            };
            match &segment.bulk {
                SegmentBulkState::Framed(bulk) => {
                    insert_attribute(bulk_entry, "inner_framing", "zlib");
                    insert_attribute(
                        bulk_entry,
                        "bulk_form",
                        format_args!("0x{:04x}", bulk.form.value()),
                    );
                    insert_attribute(bulk_entry, "expanded_size", bulk.expanded.window().len());
                }
                SegmentBulkState::Malformed(error) => {
                    insert_attribute(bulk_entry, "framing_error", error);
                }
            }
        }
        let recovery = crate::dialect::DialectRecovery::of(self);
        let matched = recovery.classify();
        let mut losses = Vec::new();
        if let Some(loss) = crate::dialect::dialect_loss(&matched, &recovery) {
            losses.push(loss);
        }
        let dialects = crate::dialect::layers(&matched, &self.rse.active_carrier)?;
        if let Some(kernel) = dialects
            .iter()
            .find(|matched| matched.format() == cadmpeg_asm::dialect::FORMAT)
        {
            if let Some(loss) = crate::dialect::kernel_dialect_loss(kernel) {
                losses.push(loss);
            }
        }
        let note = format!(
            "CFB v{} with {} RSe segment pair(s) and {} versioned database(s)",
            self.snapshot.major_version(),
            self.rse.segments.len(),
            self.rse.databases.len(),
        );
        Ok(ContainerSummary::classified(
            dialects,
            cadmpeg_ir::ContainerKind::Cfb,
            entries,
            losses,
            vec![note],
        ))
    }
}

fn find_summary_entry(
    entries: &mut [ContainerEntry],
    directory_id: u32,
) -> Option<&mut ContainerEntry> {
    entries.iter_mut().find(|entry| {
        entry
            .attributes
            .get("directory_id")
            .is_some_and(|value| value.parse::<u32>() == Ok(directory_id))
    })
}

fn insert_attribute(entry: &mut ContainerEntry, key: &str, value: impl std::fmt::Display) {
    entry.attributes.insert(key.to_owned(), value.to_string());
}

pub(crate) fn has_inventor_evidence(paths: &[String]) -> bool {
    let mut has_storage = false;
    for path in paths {
        if path.eq_ignore_ascii_case("RSeStorage") {
            has_storage = true;
            break;
        }
    }
    for path in paths {
        if path.eq_ignore_ascii_case("RSeStorage/RSeSegInfo") || database_band(path).is_some() {
            return has_storage;
        }
    }
    false
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
