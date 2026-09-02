// SPDX-License-Identifier: Apache-2.0
//! Decode and inspection reports assembled from F3D identity and transfer facts.

use std::collections::BTreeMap;

use cadmpeg_core::dialect::DialectLayers;
use cadmpeg_ir::codec::DecodeBody;
use cadmpeg_ir::document::SourceMeta;
use cadmpeg_ir::report::LossNote;
use cadmpeg_ir::ContainerSummary;

use crate::container::ContainerScan;

/// Identity owner of a decoded F3D member.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum ReportScope {
    /// The member is the decoded document and owns its classified layers.
    Standalone,
    /// The containing F3Z archive owns identity and classifies every member.
    ArchiveMember(DialectLayers),
}

/// Build a decode body from route-owned losses. Identity is authored once, on
/// the document, by [`classify_document`].
pub(crate) fn build_decode_report(
    scan: &ContainerScan<'_>,
    container_only: bool,
    geometry_transferred: bool,
    losses: Vec<LossNote>,
) -> DecodeBody {
    DecodeBody {
        geometry_transferred,
        coverage: std::collections::BTreeMap::new(),
        losses,
        notes: report_notes(crate::container::summary_notes(scan), container_only),
        transfer_ledger: cadmpeg_ir::report::TransferLedger::default(),
    }
}

/// Constructs the document's source metadata once from route-owned identity.
///
/// A standalone document classifies its own layers and charges classification
/// and dialect losses ahead of route losses. An archive member uses the outer
/// archive layers already classified by the F3Z session.
pub(crate) fn classify_document(
    scan: &ContainerScan<'_>,
    scope: ReportScope,
    attributes: BTreeMap<String, String>,
    body: &mut DecodeBody,
) -> SourceMeta {
    let dialects = match scope {
        ReportScope::Standalone => {
            let (dialects, mut losses) = crate::dialect::classify_layers(scan);
            losses.extend(crate::dialect::dialect_losses(&dialects));
            body.losses.splice(0..0, losses);
            dialects
        }
        ReportScope::ArchiveMember(dialects) => dialects,
    };
    SourceMeta::classified(dialects, attributes)
}

fn report_notes(notes: Vec<String>, container_only: bool) -> Vec<String> {
    notes
        .into_iter()
        .filter(|note| container_only || !note.starts_with("container-level inspection only"))
        .collect()
}

/// Build a single-document inspection summary with the same dialect facts that
/// decode projects into losses.
pub(crate) fn build_inspection_summary(scan: &ContainerScan<'_>) -> ContainerSummary {
    let (layers, classification_losses) = crate::dialect::classify_layers(scan);
    let losses = classification_losses
        .into_iter()
        .chain(crate::dialect::dialect_losses(&layers))
        .collect::<Vec<_>>();
    let mut summary = crate::container::summarize(scan, layers);
    summary.losses = losses;
    summary
}

#[cfg(test)]
mod tests {
    use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy};

    use super::*;
    use crate::test_support::synthetic_f3d;

    #[test]
    fn decode_report_includes_a_kernel_identity_collision_loss() {
        let bytes = synthetic_f3d(true);
        let arena = DecodeArena::new();
        let policy = DecodePolicy::default();
        let (ctx, root) = DecodeContext::from_root_bytes(&bytes, &arena, &policy).unwrap();
        let mut scan = crate::container::scan(&ctx, root).unwrap();
        scan.breps.push(scan.breps[0].clone());

        let mut report = build_decode_report(&scan, false, true, Vec::new());
        let source =
            classify_document(&scan, ReportScope::Standalone, BTreeMap::new(), &mut report);
        assert!(source.dialects().is_some());
        assert!(report
            .losses
            .iter()
            .any(|loss| loss.code == crate::loss::F3dLossCode::DialectLayerCollision.kind()));
    }
}
