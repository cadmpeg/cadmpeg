// SPDX-License-Identifier: Apache-2.0
//! Inventor compound-container classification.

use cadmpeg_container::compound::{CompoundEntry, CompoundSnapshot};
use cadmpeg_core::decode::{DecodeContext, View};
use cadmpeg_core::{CodecError, ContainerSummary};

use crate::rse::{database_band, direct_rse_child, RseInventory};

/// One parsed Inventor compound container.
pub(crate) struct InventorContainer<'a> {
    pub(crate) snapshot: CompoundSnapshot<'a>,
    pub(crate) rse: RseInventory,
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
        let rse = RseInventory::build(&snapshot);
        Ok(Self { snapshot, rse })
    }

    pub(crate) fn summary(&self) -> ContainerSummary {
        let entries = self.snapshot.container_entries(classify);
        ContainerSummary {
            format: "inventor".into(),
            container_kind: "cfb".into(),
            entries,
            notes: vec![format!(
                "CFB v{} with {} RSe segment pair(s) and {} versioned database(s)",
                self.snapshot.major_version(),
                self.rse.segments.len(),
                self.rse.storage_bands.len()
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
