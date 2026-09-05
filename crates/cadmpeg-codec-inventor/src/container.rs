// SPDX-License-Identifier: Apache-2.0
//! Inventor compound-container classification.

use cadmpeg_core::container::ContainerRole;

use cadmpeg_container::compound::{CompoundEntry, CompoundSnapshot};
use cadmpeg_core::CodecError;
use cadmpeg_core::decode::{DecodeContext, View};
use cadmpeg_ir::ContainerSummary;

use crate::external_reference::{UfrxState, parse as parse_ufrx};
use crate::property_set::{PropertySetDescriptor, inventory as property_set_inventory};
use crate::protein::{ProteinState, parse as parse_protein};
use crate::rse::SegmentBulkState;
use crate::rse::{RseInventory, SegmentMetaState, database_band, direct_rse_child};

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

    pub(crate) fn summary(&self) -> ContainerSummary {
        let mut entries = self.snapshot.container_entries(classify);
        for segment in &self.rse.segments {
            let directory_id = segment.pair.metadata.directory_id().to_string();
            let Some(entry) = entries
                .iter_mut()
                .find(|entry| entry.attributes.get("directory_id") == Some(&directory_id))
            else {
                continue;
            };
            match &segment.meta {
                SegmentMetaState::Parsed(meta) => {
                    entry
                        .attributes
                        .insert("inner_framing".into(), "zlib".into());
                    entry
                        .attributes
                        .insert("expanded_size".into(), meta.body.window().len().to_string());
                    entry
                        .attributes
                        .insert("meta_marker".into(), meta.declared.marker.clone());
                    entry.attributes.insert(
                        "meta_stream_version".into(),
                        meta.declared.version.to_string(),
                    );
                    entry
                        .attributes
                        .insert("segment_kind".into(), segment.kind.label().into());
                    entry
                        .attributes
                        .insert("display_name".into(), meta.display_name.clone());
                }
                SegmentMetaState::Malformed { declared, detail } => {
                    if let Some(declared) = declared {
                        entry
                            .attributes
                            .insert("meta_marker".into(), declared.marker.clone());
                        entry
                            .attributes
                            .insert("meta_stream_version".into(), declared.version.to_string());
                    }
                    entry
                        .attributes
                        .insert("framing_error".into(), detail.clone());
                }
            }
            let bulk_directory_id = segment.pair.bulk.directory_id().to_string();
            let Some(bulk_entry) = entries
                .iter_mut()
                .find(|entry| entry.attributes.get("directory_id") == Some(&bulk_directory_id))
            else {
                continue;
            };
            match &segment.bulk {
                SegmentBulkState::Framed(bulk) => {
                    bulk_entry
                        .attributes
                        .insert("inner_framing".into(), "zlib".into());
                    bulk_entry
                        .attributes
                        .insert("bulk_form".into(), format!("0x{:04x}", bulk.form.value()));
                    bulk_entry.attributes.insert(
                        "expanded_size".into(),
                        bulk.expanded.window().len().to_string(),
                    );
                }
                SegmentBulkState::Malformed(error) => {
                    bulk_entry
                        .attributes
                        .insert("framing_error".into(), error.clone());
                }
            }
        }
        let recovery = crate::dialect::DialectRecovery::of(self);
        let matched = recovery.classify();
        let mut losses = Vec::new();
        losses.extend(crate::dialect::dialect_loss(&matched, &recovery));
        let dialects = crate::dialect::layers(matched, &self.rse.active_carrier);
        losses.extend(
            dialects
                .iter()
                .find(|matched| matched.format() == cadmpeg_asm::dialect::FORMAT)
                .and_then(crate::dialect::kernel_dialect_loss),
        );
        ContainerSummary::classified(
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
        )
    }
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
