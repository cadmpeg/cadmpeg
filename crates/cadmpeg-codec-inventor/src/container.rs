// SPDX-License-Identifier: Apache-2.0
//! Inventor compound-container classification.

use cadmpeg_container::compound::{CompoundEntry, CompoundSnapshot};
use cadmpeg_core::decode::{DecodeContext, View};
use cadmpeg_core::{CodecError, ContainerSummary};

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
                        meta.version.value().to_string(),
                    );
                    entry
                        .attributes
                        .insert("segment_kind".into(), segment.kind.label().into());
                    entry
                        .attributes
                        .insert("display_name".into(), meta.display_name.clone());
                }
                SegmentMetaState::Malformed { detail, .. } => {
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
        let kernel = match &self.rse.active_carrier {
            crate::kernel::ActiveCarrierState::Selected(carrier) => {
                Some(crate::kernel::parse_kernel_header(carrier).map_or_else(
                    |_| crate::dialect::unknown_kernel_layer(),
                    |header| crate::dialect::kernel_layer(carrier.family, &header),
                ))
            }
            crate::kernel::ActiveCarrierState::NotExpanded
            | crate::kernel::ActiveCarrierState::Unavailable(_) => {
                Some(crate::dialect::unknown_kernel_layer())
            }
            crate::kernel::ActiveCarrierState::NotApplicable => None,
        };
        let dialects = Some(
            cadmpeg_core::dialect::DialectLayers::new(
                crate::dialect::DialectRecovery::of(self).classify().matched,
                kernel.into_iter().collect(),
            )
            .expect("the kernel layer has a distinct format"),
        );
        ContainerSummary {
            dialects,
            format: crate::dialect::FORMAT.into(),
            container_kind: "cfb".into(),
            entries,
            notes: vec![format!(
                "CFB v{} with {} RSe segment pair(s) and {} versioned database(s)",
                self.snapshot.major_version(),
                self.rse.segments.len(),
                self.rse.databases.len()
            )],
        }
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

fn classify(entry: &CompoundEntry) -> &'static str {
    let path = entry.path();
    if path.eq_ignore_ascii_case("RSeStorage") {
        return "rse-storage";
    }
    if database_band(path).is_some() {
        return "rse-database";
    }
    if path.eq_ignore_ascii_case("RSeStorage/RSeSegInfo") {
        return "rse-segment-registry";
    }
    if path.eq_ignore_ascii_case("RSeStorage/RSeDbRevisionInfo") {
        return "rse-revision-table";
    }
    if path.eq_ignore_ascii_case("Protein") {
        return "protein";
    }
    if path.eq_ignore_ascii_case("UFRxDoc") || is_reference_file(path) {
        return "external-reference";
    }
    if let Some(name) = direct_rse_child(path) {
        if name.starts_with('M') {
            return "rse-segment-metadata";
        }
        if name.starts_with('B') {
            return "rse-segment-bulk";
        }
    }
    match entry {
        CompoundEntry::Storage(_) => "storage",
        CompoundEntry::Stream(_) => "stream",
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
