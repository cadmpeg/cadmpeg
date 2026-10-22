// SPDX-License-Identifier: Apache-2.0
//! Source-representation assertions used by the golden depth test.

use crate::loss::F3dLossCode;
use cadmpeg_ir::report::loss::LossNote;

/// Whether a decode loss leaves source content without a typed representation.
#[derive(Clone, Copy)]
enum Representation {
    Design,
    Model,
    Both,
    /// The lost neutral projection still has a complete typed native record.
    Typed,
    /// A feature envelope counts only when its operation payload is typed.
    ScopePayload,
    /// Curve identity and framing alone do not represent the curve's shape.
    SketchCurve,
    Metadata,
    Export,
}

/// Every local loss code has an explicit source-content disposition. Codes
/// emitted from native projection-gap counts retain their actual typed rows;
/// raw payloads, unresolved source owners and missing semantic records do not.
const UNREPRESENTED: &[(F3dLossCode, Representation)] = &[
    (F3dLossCode::ActRecordUntyped, Representation::Both),
    (
        F3dLossCode::DimensionCompanionUntyped,
        Representation::Design,
    ),
    (
        F3dLossCode::ConfigurationMemberUnassigned,
        Representation::Design,
    ),
    (
        F3dLossCode::ConfigurationRuleUnbound,
        Representation::Design,
    ),
    (
        F3dLossCode::ConfigurationParameterOverrideUnbound,
        Representation::Typed,
    ),
    (
        F3dLossCode::ConfigurationFeatureSuppressionUnbound,
        Representation::Typed,
    ),
    (
        F3dLossCode::ActComponentLinkUnresolved,
        Representation::Model,
    ),
    (F3dLossCode::ImageAssetUntransferred, Representation::Model),
    (F3dLossCode::LocalOccurrenceUnplaced, Representation::Typed),
    (F3dLossCode::ComponentInsertUnplaced, Representation::Model),
    (
        F3dLossCode::ComponentInsertOrderUnknown,
        Representation::Model,
    ),
    (F3dLossCode::AssemblyJointUnprojected, Representation::Model),
    (
        F3dLossCode::AssemblyJointMotionUndecoded,
        Representation::Typed,
    ),
    (F3dLossCode::DrawingDocumentOmitted, Representation::Model),
    (
        F3dLossCode::HistoryBindingBudgetExceeded,
        Representation::Design,
    ),
    (
        F3dLossCode::HistoryBindingWorkExceeded,
        Representation::Design,
    ),
    (
        F3dLossCode::RecipeSideSearchBudgetExceeded,
        Representation::Design,
    ),
    (
        F3dLossCode::ParameterOrderBudgetExceeded,
        Representation::Typed,
    ),
    (
        F3dLossCode::HistoryRecordFramingFailed,
        Representation::Design,
    ),
    (
        F3dLossCode::DesignBodyBindingUnresolved,
        Representation::Typed,
    ),
    (
        F3dLossCode::ReferenceImageNativeRetained,
        Representation::Model,
    ),
    (F3dLossCode::DecalNativeRetained, Representation::Model),
    (
        F3dLossCode::EdgeReferenceLostUnrepaired,
        Representation::Typed,
    ),
    (
        F3dLossCode::FeatureDefinitionIncomplete,
        Representation::Typed,
    ),
    (
        F3dLossCode::FeatureScopeUnprojected,
        Representation::ScopePayload,
    ),
    (
        F3dLossCode::FeatureScopePayloadUninterpreted,
        Representation::Design,
    ),
    (F3dLossCode::RecipePrefixUntyped, Representation::Design),
    (F3dLossCode::ParameterUnprojected, Representation::Typed),
    (F3dLossCode::ParameterRecordUnparsed, Representation::Design),
    (
        F3dLossCode::ParameterOwnerUnrecognized,
        Representation::Design,
    ),
    (F3dLossCode::ParameterUnitUntyped, Representation::Typed),
    (
        F3dLossCode::MaterialDistanceUnitUntyped,
        Representation::Model,
    ),
    (
        F3dLossCode::ParameterExpressionUnbound,
        Representation::Typed,
    ),
    (
        F3dLossCode::HistoryDependencyUnprojected,
        Representation::Typed,
    ),
    (
        F3dLossCode::HistoryDependencyAmbiguous,
        Representation::Typed,
    ),
    (
        F3dLossCode::HistoricalFaceSelectionUnbound,
        Representation::Typed,
    ),
    (
        F3dLossCode::SketchRelationNativeRetained,
        Representation::Typed,
    ),
    (
        F3dLossCode::SketchDimensionNativeRetained,
        Representation::Typed,
    ),
    (
        F3dLossCode::SketchPlacementUnprojected,
        Representation::Typed,
    ),
    (F3dLossCode::SketchPointUnprojected, Representation::Typed),
    (
        F3dLossCode::SketchCurveUnprojected,
        Representation::SketchCurve,
    ),
    (F3dLossCode::SketchSurfaceUnprojected, Representation::Typed),
    (F3dLossCode::SketchTextUnprojected, Representation::Typed),
    (
        F3dLossCode::SketchRelationUnprojected,
        Representation::Typed,
    ),
    (
        F3dLossCode::SketchProfileSearchCapped,
        Representation::Typed,
    ),
    (F3dLossCode::DimensionUnprojected, Representation::Design),
    (
        F3dLossCode::FeatureProfileSelectionNative,
        Representation::Typed,
    ),
    (
        F3dLossCode::FeatureProfileArrangementCapped,
        Representation::Typed,
    ),
    (
        F3dLossCode::FeatureProfileDensificationCapped,
        Representation::Typed,
    ),
    (
        F3dLossCode::FeaturePathSelectionNative,
        Representation::Typed,
    ),
    (
        F3dLossCode::FeatureFaceSelectionNative,
        Representation::Typed,
    ),
    (
        F3dLossCode::FeatureFaceSelectionActiveSubstituted,
        Representation::Typed,
    ),
    (
        F3dLossCode::FeatureBodySelectionNative,
        Representation::Typed,
    ),
    (
        F3dLossCode::FeatureFaceOperandUnresolved,
        Representation::Typed,
    ),
    (
        F3dLossCode::FeatureEdgeSelectionNative,
        Representation::Typed,
    ),
    (
        F3dLossCode::FeatureEdgeOperandUnresolved,
        Representation::Typed,
    ),
    (F3dLossCode::FeatureEdgeSelectionLost, Representation::Typed),
    (
        F3dLossCode::FeaturePatternInstanceSearchCapped,
        Representation::Design,
    ),
    (F3dLossCode::BrepBlobUndecoded, Representation::Model),
    (F3dLossCode::MeshContainerUnjoined, Representation::Model),
    (F3dLossCode::MeshContainerUndecoded, Representation::Model),
    (F3dLossCode::MeshContainerMissing, Representation::Model),
    (
        F3dLossCode::MeshAttributeNotTransferred,
        Representation::Model,
    ),
    (F3dLossCode::XrefTableUndecoded, Representation::Model),
    (F3dLossCode::XrefPlacementUndecoded, Representation::Model),
    (F3dLossCode::XrefPlacementSuperseded, Representation::Model),
    (
        F3dLossCode::MeshVertexPrecisionReduced,
        Representation::Typed,
    ),
    (F3dLossCode::BodylessDesignCarrier, Representation::Typed),
    (
        F3dLossCode::AssemblyComponentsExternal,
        Representation::Typed,
    ),
    (F3dLossCode::NurbsSurfaceCarrier, Representation::Typed),
    (F3dLossCode::NurbsCurveCarrier, Representation::Typed),
    (
        F3dLossCode::FaceSurfaceReferenceDangling,
        Representation::Model,
    ),
    (F3dLossCode::SurfaceShapeNotDecoded, Representation::Model),
    (F3dLossCode::MeshSurfaceSentinel, Representation::Typed),
    (F3dLossCode::ProceduralCurveUndecoded, Representation::Model),
    (F3dLossCode::PcurveUndecoded, Representation::Model),
    (F3dLossCode::BlendSupportPartial, Representation::Model),
    (F3dLossCode::SolvedRecordUntyped, Representation::Model),
    (F3dLossCode::MaterialNotTransferred, Representation::Model),
    (F3dLossCode::GeometryNotTransferred, Representation::Model),
    (F3dLossCode::TopologyNotTransferred, Representation::Model),
    (F3dLossCode::MissingGeometryStream, Representation::Model),
    (F3dLossCode::XrefCycle, Representation::Model),
    (F3dLossCode::XrefMemberMissing, Representation::Model),
    (F3dLossCode::XrefMemberUndecoded, Representation::Model),
    (F3dLossCode::TsplineRecordUntyped, Representation::Model),
    (F3dLossCode::TsplineCageUndecoded, Representation::Model),
    (
        F3dLossCode::SourcePreservedImageUnavailable,
        Representation::Export,
    ),
    (
        F3dLossCode::SourceDialectUnverified,
        Representation::Metadata,
    ),
    (
        F3dLossCode::WriterAppearancePropertiesOmitted,
        Representation::Export,
    ),
    (F3dLossCode::SourceDialectDisplaced, Representation::Export),
    (
        F3dLossCode::KernelDialectUnverified,
        Representation::Metadata,
    ),
    (
        F3dLossCode::KernelCarrierUnparseable,
        Representation::Metadata,
    ),
    (F3dLossCode::DialectLayerCollision, Representation::Metadata),
    (
        F3dLossCode::TextHeaderNoncanonical,
        Representation::Metadata,
    ),
    (
        F3dLossCode::KernelHeaderToleranceUnresolved,
        Representation::Model,
    ),
    (
        F3dLossCode::KernelHeaderMetadataUnresolved,
        Representation::Metadata,
    ),
    (
        F3dLossCode::ManifestMetadataUnresolved,
        Representation::Metadata,
    ),
    (F3dLossCode::ArchiveEntryUnreadable, Representation::Both),
    (
        F3dLossCode::WriterNativeRecordsOmitted,
        Representation::Export,
    ),
    (
        F3dLossCode::WriterDesignRecordsOmitted,
        Representation::Export,
    ),
    (F3dLossCode::WriterGeometryOmitted, Representation::Export),
    (F3dLossCode::WriterAuxiliaryOmitted, Representation::Export),
    (F3dLossCode::WriterProceduralReduced, Representation::Export),
    (F3dLossCode::WriterSubdOmitted, Representation::Export),
    (F3dLossCode::WriterAssetOmitted, Representation::Export),
    (
        F3dLossCode::WriterTessellationOmitted,
        Representation::Export,
    ),
    (F3dLossCode::WriterPcurveOmitted, Representation::Export),
    (F3dLossCode::WriterPmiOmitted, Representation::Export),
    (F3dLossCode::WriterProductOmitted, Representation::Export),
    (
        F3dLossCode::WriterSourceAssociationOmitted,
        Representation::Export,
    ),
    (
        F3dLossCode::WriterNoExportableSolids,
        Representation::Export,
    ),
    (F3dLossCode::DesignIntentUndecoded, Representation::Design),
];

/// Classes with source records that have no typed document representation.
pub(super) fn unrepresented(
    losses: &[LossNote],
    native: &crate::native::F3dNative,
) -> cadmpeg_ir::document::ArenaClasses {
    use cadmpeg_ir::document::{ArenaClass, ArenaClasses};
    let mut classes = ArenaClasses::default();
    // Framing and references do not type the historical geometry payload.
    if native
        .asm_histories
        .iter()
        .flat_map(crate::history_records::AsmHistory::states)
        .flat_map(|state| &state.records)
        .any(|record| !record.raw_bytes.is_empty())
    {
        classes.insert(ArenaClass::Design);
    }
    // Indexed envelopes and raw recipe words do not supply the operation's
    // semantic payload. Typed recipe clauses, rather than token framing, are
    // required for native representation.
    let raw_recipe = native.design_edge_operands.iter().any(|operand| {
        !operand.recipe_program.is_empty()
            && operand.recipe_structure.is_none()
            && operand.surface_patch_recipe_structure.is_none()
    }) || native.design_face_operands.iter().any(|operand| {
        !operand.recipe_program.is_empty()
            && (operand.recipe_nodes.is_empty()
                || operand
                    .recipe_nodes
                    .iter()
                    .any(|node| node.recipe_structure.is_none()))
    });
    let native_name_only = native.design_parameter_scopes.iter().any(|scope| {
        matches!(
            scope.payload(),
            crate::records::feature::scope::DesignScopePayload::Native(_)
        )
    });
    if raw_recipe || native_name_only {
        classes.insert(ArenaClass::Design);
    }
    for &(code, disposition) in UNREPRESENTED {
        if !losses.iter().any(|loss| loss.code == code.kind()) {
            continue;
        }
        match disposition {
            Representation::Design => {
                classes.insert(ArenaClass::Design);
            }
            Representation::Model => {
                classes.insert(ArenaClass::Model);
            }
            Representation::Both => {
                classes.insert(ArenaClass::Model);
                classes.insert(ArenaClass::Design);
            }
            Representation::ScopePayload => {
                if native
                    .design_parameter_scopes
                    .iter()
                    .any(|scope| scope.payload().is_uninterpreted())
                {
                    classes.insert(ArenaClass::Design);
                }
            }
            Representation::SketchCurve => {
                if native
                    .sketch_curve_identities
                    .iter()
                    .any(|curve| curve.geometry.is_none())
                {
                    classes.insert(ArenaClass::Design);
                }
            }
            Representation::Typed | Representation::Metadata | Representation::Export => {}
        }
    }
    classes
}

#[test]
fn every_loss_has_a_source_representation_classification() {
    assert_eq!(
        UNREPRESENTED.len(),
        crate::test_support::loss_codes::ALL.len()
    );
    for code in crate::test_support::loss_codes::ALL {
        assert_eq!(
            UNREPRESENTED
                .iter()
                .filter(|(entry, _)| entry == code)
                .count(),
            1
        );
    }
}

#[test]
fn every_source_only_loss_declares_its_missing_class() {
    use cadmpeg_ir::document::ArenaClass::{Design, Model};
    let native = crate::native::F3dNative::default();
    for &(code, disposition) in UNREPRESENTED {
        let classes = unrepresented(&[code.note("source-only test record")], &native);
        match disposition {
            Representation::Design => assert!(classes.contains(&Design), "{code:?}"),
            Representation::Model => assert!(classes.contains(&Model), "{code:?}"),
            Representation::Both => assert_eq!(classes, [Model, Design].into(), "{code:?}"),
            Representation::Typed | Representation::Metadata | Representation::Export => {
                assert!(classes.is_empty(), "{code:?}");
            }
            Representation::ScopePayload | Representation::SketchCurve => {}
        }
    }
}
