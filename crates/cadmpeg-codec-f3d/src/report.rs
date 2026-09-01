// SPDX-License-Identifier: Apache-2.0
//! Decode and inspection reports assembled from F3D identity and transfer facts.

use cadmpeg_ir::report::{DecodeReport, LossNote};
use cadmpeg_ir::ContainerSummary;

use crate::container::ContainerScan;

/// Identity owner of a decoded F3D member.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ReportScope {
    /// The member is the decoded document and owns its classified layers.
    Standalone,
    /// The containing F3Z archive owns identity and classifies every member.
    ArchiveMember,
}

/// Build a decode report from classified identity and route-owned losses.
pub(crate) fn build_decode_report(
    scan: &ContainerScan<'_>,
    container_only: bool,
    geometry_transferred: bool,
    mut losses: Vec<LossNote>,
    scope: ReportScope,
) -> DecodeReport {
    let transfer = if container_only {
        cadmpeg_ir::DecodeTransfer::ContainerOnly
    } else {
        cadmpeg_ir::DecodeTransfer::full(geometry_transferred)
    };
    let notes = report_notes(crate::container::summary_notes(scan), container_only);
    match scope {
        ReportScope::Standalone => {
            let classification = crate::dialect::classify_layers(scan);
            let (dialects, classification_losses) = classification.into_parts();
            losses.extend(classification_losses);
            losses.extend(crate::dialect::dialect_losses(&dialects));
            DecodeReport::classified(
                dialects,
                transfer,
                std::collections::BTreeMap::new(),
                losses,
                notes,
                cadmpeg_ir::report::TransferLedger::default(),
            )
        }
        ReportScope::ArchiveMember => DecodeReport::unclassified(
            crate::dialect::FORMAT,
            transfer,
            std::collections::BTreeMap::new(),
            losses,
            notes,
            cadmpeg_ir::report::TransferLedger::default(),
        ),
    }
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
    let classification = crate::dialect::classify_layers(scan);
    let (layers, classification_losses) = classification.into_parts();
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

        let report = build_decode_report(&scan, false, true, Vec::new(), ReportScope::Standalone);
        assert!(report
            .losses
            .iter()
            .any(|loss| loss.code == crate::loss::F3dLossCode::DialectLayerCollision.kind()));
    }
}
