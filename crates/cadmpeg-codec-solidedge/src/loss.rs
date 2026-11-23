// SPDX-License-Identifier: Apache-2.0
//! Stable Solid Edge transfer-loss vocabulary.

use cadmpeg_ir::report::loss::{LossKind, LossNamespace, LossNote, LossTaxonomy};

const NAMESPACE: LossNamespace<'static> = match LossNamespace::new("solidedge") {
    Ok(value) => value,
    Err(_) => panic!("invalid loss namespace"),
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum SolidEdgeLossCode {
    ContainerNoncanonical,
    BodiesPending,
    UnitsUnread,
    UnitCodeUnknown,
    UnitNotNeutral,
    FeatureStorageUnread,
    FeatureNavigatorAbsent,
    FeatureOperandsPending,
    FeatureKindUnknown,
    FeatureRollbackUnread,
    FeatureFolderUnread,
    SketchUnread,
    SketchRelationsUnread,
    FeatureExtentUnread,
    FeatureDressupUnread,
    FeatureThreadUnread,
    FeatureSelectionUnread,
    FeaturePatternUnread,
    AssemblyOccurrenceUnread,
    AssemblyReferenceUnread,
    AssemblyVisibilityUnread,
    SheetMetalRuleUnread,
    SheetMetalBendMethodUnread,
    SheetMetalBaseTabUnread,
    SheetMetalFlangeFieldUnread,
    SheetMetalHemFieldUnread,
    SheetMetalContourFlangeUnread,
    SheetMetalBendOperandUnread,
}

impl SolidEdgeLossCode {
    pub(crate) fn note(self, message: impl Into<String>) -> LossNote {
        let (code, taxonomy) = match self {
            Self::ContainerNoncanonical => (
                "container.noncanonical",
                LossTaxonomy::NoncanonicalSourceSyntax,
            ),
            Self::BodiesPending => ("body.decode-pending", LossTaxonomy::GeometryNotTransferred),
            Self::UnitsUnread => ("units.unread", LossTaxonomy::MetadataNotTransferred),
            Self::UnitCodeUnknown => ("units.code-unknown", LossTaxonomy::MetadataNotTransferred),
            Self::UnitNotNeutral => ("units.not-neutral", LossTaxonomy::MetadataNotTransferred),
            Self::FeatureStorageUnread => {
                ("feature.storage-unread", LossTaxonomy::DecodeDiagnostic)
            }
            Self::FeatureNavigatorAbsent => {
                ("feature.navigator-absent", LossTaxonomy::DecodeDiagnostic)
            }
            Self::FeatureOperandsPending => {
                ("feature.operands-pending", LossTaxonomy::RecordNotTyped)
            }
            Self::FeatureKindUnknown => ("feature.kind-unknown", LossTaxonomy::RecordNotTyped),
            Self::FeatureFolderUnread => (
                "feature.folder-unread",
                LossTaxonomy::FeatureHistoryRetained,
            ),
            Self::SketchUnread => ("sketch.unread", LossTaxonomy::FeatureHistoryRetained),
            Self::SketchRelationsUnread => {
                ("sketch.relations-unread", LossTaxonomy::RecordNotTyped)
            }
            Self::FeatureExtentUnread => ("feature.extent-unread", LossTaxonomy::RecordNotTyped),
            Self::FeatureDressupUnread => ("feature.dressup-unread", LossTaxonomy::RecordNotTyped),
            Self::FeatureThreadUnread => ("feature.thread-unread", LossTaxonomy::RecordNotTyped),
            Self::FeatureSelectionUnread => {
                ("feature.selection-unread", LossTaxonomy::RecordNotTyped)
            }
            Self::FeaturePatternUnread => ("feature.pattern-unread", LossTaxonomy::RecordNotTyped),
            Self::AssemblyOccurrenceUnread => {
                ("assembly.occurrence-unread", LossTaxonomy::DecodeDiagnostic)
            }
            Self::AssemblyReferenceUnread => {
                ("assembly.reference-unread", LossTaxonomy::DecodeDiagnostic)
            }
            Self::AssemblyVisibilityUnread => (
                "assembly.visibility-unread",
                LossTaxonomy::MetadataNotTransferred,
            ),
            Self::SheetMetalRuleUnread => ("sheet-metal.rule-unread", LossTaxonomy::RecordNotTyped),
            Self::SheetMetalBendMethodUnread => (
                "sheet-metal.bend-method-unread",
                LossTaxonomy::MetadataNotTransferred,
            ),
            Self::SheetMetalBaseTabUnread => {
                ("sheet-metal.base-tab-unread", LossTaxonomy::RecordNotTyped)
            }
            Self::SheetMetalFlangeFieldUnread => (
                "sheet-metal.flange-field-unread",
                LossTaxonomy::RecordNotTyped,
            ),
            Self::SheetMetalHemFieldUnread => {
                ("sheet-metal.hem-field-unread", LossTaxonomy::RecordNotTyped)
            }
            Self::SheetMetalContourFlangeUnread => (
                "sheet-metal.contour-flange-unread",
                LossTaxonomy::RecordNotTyped,
            ),
            Self::SheetMetalBendOperandUnread => (
                "sheet-metal.bend-operand-unread",
                LossTaxonomy::RecordNotTyped,
            ),
            Self::FeatureRollbackUnread => (
                "feature.rollback-unread",
                LossTaxonomy::FeatureHistoryRetained,
            ),
        };
        LossNote::new(LossKind::namespaced(NAMESPACE, code, taxonomy), message)
    }
}
