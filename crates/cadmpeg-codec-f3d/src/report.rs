// SPDX-License-Identifier: Apache-2.0
//! Decode and inspection reports assembled from F3D identity and transfer facts.

use std::collections::BTreeMap;

use cadmpeg_core::dialect::DialectLayers;
use cadmpeg_core::decode::DecodeContext;
use cadmpeg_ir::codec::DecodeBody;
use cadmpeg_ir::document::SourceMeta;
use cadmpeg_ir::report::loss::LossNote;
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
    ctx: &DecodeContext<'_>,
    scan: &ContainerScan<'_>,
    transfer: cadmpeg_ir::report::decode::DecodeTransfer,
    losses: Vec<LossNote>,
) -> Result<DecodeBody, cadmpeg_core::CodecError> {
    Ok(DecodeBody {
        transfer,
        coverage: cadmpeg_ir::report::decode::Coverage::default(),
        losses,
        notes: crate::container::summary_notes(
            ctx,
            scan,
            if transfer.container_only() {
                crate::container::SummaryScope::ContainerOnly
            } else {
                crate::container::SummaryScope::FullDecode
            },
        )?,
        transfer_ledger: cadmpeg_ir::report::decode::TransferLedger::default(),
    })
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
) -> Result<SourceMeta, cadmpeg_core::CodecError> {
    let dialects = match scope {
        ReportScope::Standalone => {
            let (dialects, mut losses) = crate::dialect::classify_layers(scan);
            losses.extend(crate::dialect::dialect_losses(&dialects));
            body.losses.splice(0..0, losses);
            dialects
        }
        ReportScope::ArchiveMember(dialects) => dialects,
    };
    Ok(SourceMeta::classified(
        dialects,
        cadmpeg_core::text::named_entries("the f3d document", attributes)?,
    ))
}

/// Build a single-document inspection summary with the same dialect facts that
/// decode projects into losses.
pub(crate) fn build_inspection_summary(
    ctx: &DecodeContext<'_>,
    scan: &ContainerScan<'_>,
) -> Result<ContainerSummary, cadmpeg_core::CodecError> {
    let (layers, classification_losses) = crate::dialect::classify_layers(scan);
    let losses = classification_losses
        .into_iter()
        .chain(crate::dialect::dialect_losses(&layers))
        .collect::<Vec<_>>();
    let mut summary = crate::container::summarize(ctx, scan, layers)?;
    summary.losses = losses;
    Ok(summary)
}

#[cfg(test)]
mod tests {
    use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy};

    use super::{build_decode_report, classify_document, ReportScope};
    use crate::test_support::zip_test::synthetic_f3d;
    use std::collections::BTreeMap;

    #[test]
    fn decode_report_includes_a_kernel_identity_collision_loss() {
        let bytes = synthetic_f3d(true);
        let arena = DecodeArena::new();
        let policy = DecodePolicy::default();
        let (ctx, root) = DecodeContext::from_root_bytes(&bytes, &arena, &policy).unwrap();
        let mut scan = crate::container::scan(&ctx, root).unwrap();
        scan.breps.push(scan.breps[0].clone());

        let mut report = build_decode_report(
            &ctx,
            &scan,
            cadmpeg_ir::report::decode::DecodeTransfer::full(true),
            Vec::new(),
        ).unwrap();
        let source =
            classify_document(&scan, ReportScope::Standalone, BTreeMap::new(), &mut report)
                .unwrap();
        assert!(source.dialects().is_some());
        assert!(report
            .losses
            .iter()
            .any(|loss| loss.code == crate::loss::F3dLossCode::DialectLayerCollision.kind()));
    }

    #[test]
    fn summary_notes_refuse_collection_limit() {
        let bytes = synthetic_f3d(true);
        let arena = DecodeArena::new();
        let policy = DecodePolicy::default();
        let (ctx, root) = DecodeContext::from_root_bytes(&bytes, &arena, &policy).unwrap();
        let scan = crate::container::scan(&ctx, root).unwrap();
        let mut limited_policy = DecodePolicy::service();
        limited_policy.limits.max_collection_items = 0;
        let (limited, _) =
            DecodeContext::from_root_bytes(&[], &arena, &limited_policy).unwrap();
        let error = crate::container::summary_notes(
            &limited,
            &scan,
            crate::container::SummaryScope::FullDecode,
        )
        .unwrap_err();
        assert!(matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
            if limit.operation == "collect F3D summary notes"));
    }

    #[test]
    fn summary_entry_copy_refuses_collection_limit() {
        let bytes = synthetic_f3d(true);
        let arena = DecodeArena::new();
        let policy = DecodePolicy::default();
        let (ctx, root) = DecodeContext::from_root_bytes(&bytes, &arena, &policy).unwrap();
        let scan = crate::container::scan(&ctx, root).unwrap();
        let mut limited_policy = DecodePolicy::service();
        limited_policy.limits.max_collection_items = 0;
        let (limited, _) =
            DecodeContext::from_root_bytes(&[], &arena, &limited_policy).unwrap();
        let error = crate::container::summarize(
            &limited,
            &scan,
            cadmpeg_core::dialect::DialectLayers::of(scan.kind.dialect().clone()),
        )
        .unwrap_err();
        assert!(matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
            if limit.operation == "copy F3D summary entries"));
    }

    #[test]
    fn summary_attributes_refuse_collection_limit() {
        let bytes = synthetic_f3d(true);
        let arena = DecodeArena::new();
        let policy = DecodePolicy::default();
        let (ctx, root) = DecodeContext::from_root_bytes(&bytes, &arena, &policy).unwrap();
        let scan = crate::container::scan(&ctx, root).unwrap();
        let mut limited_policy = DecodePolicy::service();
        limited_policy.limits.max_collection_items = scan.entries.len() as u64;
        let (limited, _) =
            DecodeContext::from_root_bytes(&[], &arena, &limited_policy).unwrap();
        let error = crate::container::summarize(
            &limited,
            &scan,
            cadmpeg_core::dialect::DialectLayers::of(scan.kind.dialect().clone()),
        )
        .unwrap_err();
        assert!(matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
            if limit.operation == "copy F3D summary attributes"));
    }

    #[test]
    fn summary_note_text_refuses_retained_limit() {
        let bytes = synthetic_f3d(true);
        let arena = DecodeArena::new();
        let policy = DecodePolicy::default();
        let (ctx, root) = DecodeContext::from_root_bytes(&bytes, &arena, &policy).unwrap();
        let scan = crate::container::scan(&ctx, root).unwrap();
        let mut limited_policy = DecodePolicy::service();
        limited_policy.limits.max_retained_bytes = 0;
        let (limited, _) =
            DecodeContext::from_root_bytes(&[], &arena, &limited_policy).unwrap();
        let error = crate::container::summary_notes(
            &limited,
            &scan,
            crate::container::SummaryScope::FullDecode,
        )
        .unwrap_err();
        assert!(matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
            if limit.operation == "retain F3D summary note"));
    }
}
