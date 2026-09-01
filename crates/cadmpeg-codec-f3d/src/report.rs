// SPDX-License-Identifier: Apache-2.0
//! Decode and inspection reports assembled from F3D identity and transfer facts.

use cadmpeg_core::dialect::DialectLayers;
use cadmpeg_core::ContainerSummary;
use cadmpeg_ir::report::{DecodeReport, LossNote};

use crate::container::ContainerScan;
use crate::loss::F3dLossCode;

/// Identity owner of a decoded F3D member.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ReportScope {
    /// The member is the decoded document and owns its classified layers.
    Standalone,
    /// The containing F3Z archive owns identity and classifies every member.
    ArchiveMember,
}

/// One document's admitted identity layers and recoverable classification loss.
pub(crate) struct DocumentClassification {
    layers: DialectLayers,
    losses: Vec<LossNote>,
}

impl DocumentClassification {
    pub(crate) fn layers(&self) -> &DialectLayers {
        &self.layers
    }

    pub(crate) fn into_parts(self) -> (DialectLayers, Vec<LossNote>) {
        (self.layers, self.losses)
    }
}

/// Classify the document and every kernel carrier without refusing on a layer
/// identity collision.
pub(crate) fn classify_document(scan: &ContainerScan<'_>) -> DocumentClassification {
    let mut layers = DialectLayers::of(scan.dialect.clone());
    let mut losses = Vec::new();
    for layer in crate::dialect::kernel_layers(scan) {
        let format = layer.format().to_owned();
        let instance = layer.instance().unwrap_or("unidentified").to_owned();
        if layers.try_push(layer).is_err() {
            losses.push(F3dLossCode::DialectLayerCollision.note(format!(
                "the document produced a duplicate {format} dialect layer at instance {instance}; the later layer was omitted"
            )));
        }
    }
    DocumentClassification { layers, losses }
}

/// Dialect-derived losses implied by a report's final classified layers.
pub(crate) fn dialect_losses(layers: &DialectLayers) -> Vec<LossNote> {
    let mut losses = layers
        .iter()
        .filter(|matched| matched.format() == crate::dialect::FORMAT)
        .filter_map(crate::dialect::dialect_loss)
        .collect::<Vec<_>>();
    losses.extend(
        layers
            .iter()
            .filter(|matched| matched.format() == cadmpeg_asm::dialect::FORMAT)
            .filter_map(crate::dialect::kernel_dialect_loss),
    );
    losses
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
            let classification = classify_document(scan);
            let (dialects, classification_losses) = classification.into_parts();
            losses.extend(classification_losses);
            losses.extend(dialect_losses(&dialects));
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
    let classification = classify_document(scan);
    let classification_notes = classification
        .losses
        .iter()
        .cloned()
        .chain(dialect_losses(classification.layers()))
        .map(|loss| format!("dialect classification loss: {}", loss.message))
        .collect::<Vec<_>>();
    let mut summary = crate::container::summarize(scan, classification.layers);
    summary.notes.extend(classification_notes);
    summary
}

#[cfg(test)]
mod tests {
    use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy};

    use super::*;
    use crate::test_support::synthetic_f3d;

    #[test]
    fn duplicate_kernel_identity_is_omitted_with_a_typed_loss() {
        let bytes = synthetic_f3d(true);
        let arena = DecodeArena::new();
        let policy = DecodePolicy::default();
        let (ctx, root) = DecodeContext::from_root_bytes(&bytes, &arena, &policy).unwrap();
        let mut scan = crate::container::scan(&ctx, root).unwrap();
        scan.breps.push(scan.breps[0].clone());

        let classification = classify_document(&scan);
        assert_eq!(classification.losses.len(), 1);
        assert_eq!(
            classification.losses[0].code,
            F3dLossCode::DialectLayerCollision.kind()
        );
        assert_eq!(
            classification
                .layers()
                .iter()
                .filter(|layer| layer.format() == cadmpeg_asm::dialect::FORMAT)
                .count(),
            2
        );

        let report = build_decode_report(&scan, false, true, Vec::new(), ReportScope::Standalone);
        assert!(report
            .losses
            .iter()
            .any(|loss| loss.code == F3dLossCode::DialectLayerCollision.kind()));
    }
}
