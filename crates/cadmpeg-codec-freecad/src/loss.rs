// SPDX-License-Identifier: Apache-2.0
//! Stable loss vocabulary for `FreeCAD` `.fcstd` decoding and writing.
//!
//! Every fallback, approximation, and drop the codec reports carries a
//! stable machine-readable code from [`FreecadLossCode`]. Codes are the gating
//! surface: harness oracles and downstream tooling key on them, never on the
//! human-readable message text, so a reworded message is not a contract change
//! and a new drop path without a code does not compile.
//!
//! [`FreecadLossCode::note`] is the single practical construction path for a
//! [`LossNote`] in this crate: it fixes the loss category and
//! severity from the code so the two cannot drift apart across sites, and it
//! leaves only the per-instance message to the caller.
//!
use cadmpeg_ir::report::{
    loss::{LossKind, LossNamespace, LossNote, LossTaxonomy, NamespacedLossKind},
    Severity,
};

pub(crate) mod payloads;

const NAMESPACE: LossNamespace<'static> = cadmpeg_ir::loss_namespace!("fcstd");

/// A stable, machine-readable identifier for one `.fcstd` transfer loss.
///
/// Variants are grouped by the record family whose transfer degraded. The
/// string form (via [`FreecadLossCode::code`]) is the stable contract.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub(crate) enum FreecadLossCode {
    /// A formula identifier has no unique parameter binding.
    ExpressionReferenceUnresolved,
    /// A property has no constructible typed attribute projection.
    AttributePropertyUnprojected,
    ExportAttributeNotTransferred,
    /// Model payload content survives only in opaque source bytes.
    SourceModelPayloadUnrepresented,
    /// Design payload content survives only in opaque source bytes.
    SourceDesignPayloadUnrepresented,
    /// A whole neutral arena has no `FCStd` representation.
    ExportArenaNotTransferred,
    /// A neutral construction has no writable `FreeCAD` operation.
    ExportFeatureNotTransferred,
    /// An occurrence has no source placement to write.
    ExportOccurrenceNotTransferred,
    /// A joint operand occurrence has no writable placement.
    ExportJointOperandNotTransferred,
    /// A neutral body has an unsupported exact carrier.
    ExportGeometryNotTransferred,
    /// An appearance member has no exact `FreeCAD` GUI encoding.
    ExportAppearanceNotTransferred,
    /// GUI carrier requirements or aliases alter neutral presentation state.
    ExportPresentationNotTransferred,
    /// A procedural carrier is written through its current solved cache.
    ExportProceduralReduced,
    /// Native-only property content has no synthesized encoding.
    ExportNativePropertyNotTransferred,
    /// A property payload cannot enter the admitted persistence graph.
    PersistencePayloadUnresolved,
    /// An optional file reference has no readable archive payload.
    PersistenceSideEntryUnresolved,
    /// Redundant persistence counts disagree with framed XML populations.
    PersistenceCountNoncanonical,
    /// Feature history cannot enter a neutral definition because its ordering is cyclic.
    FeatureCyclicHistory,
    /// Feature retains its native kind without a complete neutral operation.
    FeatureNativeKindRetained,
    /// Sketch geometry record retains a native kind without solved geometry.
    SketchNativeGeometry,
    /// Cached sketch geometry has no matching live external reference.
    SketchExternalReferenceUnresolved,
    /// Optional external sketch cache cannot be projected; live links remain authoritative.
    SketchExternalCacheUnresolved,
    /// Sketch constraint retains a native relation kind without neutral semantics.
    SketchNativeConstraint,
    /// Sketch profiles are not chained because endpoint matching reached its work cap.
    SketchProfileMatchingBounded,
    /// Topology color values were retained because their count did not match mapped topology.
    AppearanceTopologyColorCountMismatch,
    /// A primitive style size could not enter the neutral appearance.
    AppearancePrimitiveSizeNotTransferred,
    /// The declared persistence schema names no dialect this codec has a strategy for.
    SourceDialectUnverified,
    /// Synthesis writes a different dialect from the current source identity.
    SourceDialectDisplaced,
    /// The GUI document used schema-1 vocabulary under another declaration.
    SourceGuiSchemaUnverified,
    /// A GUI property states a blank key, so its value has no name to carry.
    SourceGuiPropertyKeyBlank,
    /// Optional GUI records could not be interpreted; exact XML remains retained.
    SourceGuiMetadataUnresolved,
    /// Optional persistent naming metadata could not be interpreted.
    ElementMapMetadataUnresolved,
    /// A parameter-space curve could not enter neutral geometry.
    PcurveNotTransferred,
    /// Producer version metadata used an alias or conflicting declarations.
    ProgramVersionNoncanonical,
    /// Persistence version metadata used a lowercase alias.
    FileVersionNoncanonical,
    /// Persistence file-version declaration cannot select a legacy map encoding.
    FileVersionUnverified,
    /// Nonempty inline legacy map has no verified file-version interpretation.
    ElementMapVersionUnresolved,
}

impl FreecadLossCode {
    /// Every code, in declaration order.
    #[cfg(test)]
    const ALL: &'static [FreecadLossCode] = &[
        Self::ExpressionReferenceUnresolved,
        Self::AttributePropertyUnprojected,
        Self::ExportAttributeNotTransferred,
        Self::SourceModelPayloadUnrepresented,
        Self::SourceDesignPayloadUnrepresented,
        Self::ExportArenaNotTransferred,
        Self::ExportFeatureNotTransferred,
        Self::ExportOccurrenceNotTransferred,
        Self::ExportJointOperandNotTransferred,
        Self::ExportGeometryNotTransferred,
        Self::ExportAppearanceNotTransferred,
        Self::ExportPresentationNotTransferred,
        Self::ExportProceduralReduced,
        Self::ExportNativePropertyNotTransferred,
        Self::PersistencePayloadUnresolved,
        Self::PersistenceSideEntryUnresolved,
        Self::PersistenceCountNoncanonical,
        Self::FeatureCyclicHistory,
        Self::FeatureNativeKindRetained,
        Self::SketchNativeGeometry,
        Self::SketchNativeConstraint,
        Self::SketchExternalReferenceUnresolved,
        Self::SketchExternalCacheUnresolved,
        Self::SketchProfileMatchingBounded,
        Self::AppearanceTopologyColorCountMismatch,
        Self::AppearancePrimitiveSizeNotTransferred,
        Self::SourceDialectUnverified,
        Self::SourceDialectDisplaced,
        Self::SourceGuiSchemaUnverified,
        Self::SourceGuiPropertyKeyBlank,
        Self::SourceGuiMetadataUnresolved,
        Self::ElementMapMetadataUnresolved,
        Self::PcurveNotTransferred,
        Self::ProgramVersionNoncanonical,
        Self::FileVersionNoncanonical,
        Self::FileVersionUnverified,
        Self::ElementMapVersionUnresolved,
    ];

    /// The stable string identifier. This is the gating contract.
    #[must_use]
    const fn code(self) -> &'static str {
        match self {
            Self::ExpressionReferenceUnresolved => "expression.reference-unresolved",
            Self::AttributePropertyUnprojected => "attribute.property-unprojected",
            Self::ExportAttributeNotTransferred => "attribute.not-transferred",
            Self::SourceModelPayloadUnrepresented => "source.model-payload-unrepresented",
            Self::SourceDesignPayloadUnrepresented => "source.design-payload-unrepresented",
            Self::ExportArenaNotTransferred => "arena.not-transferred",
            Self::ExportFeatureNotTransferred => "feature.not-transferred",
            Self::ExportOccurrenceNotTransferred => "occurrence.not-transferred",
            Self::ExportJointOperandNotTransferred => "joint.operand-not-transferred",
            Self::ExportGeometryNotTransferred => "geometry.not-transferred",
            Self::ExportAppearanceNotTransferred => "appearance.not-transferred",
            Self::ExportPresentationNotTransferred => "presentation.not-transferred",
            Self::ExportProceduralReduced => "geometry.procedural-reduced",
            Self::ExportNativePropertyNotTransferred => "metadata.native-property-not-transferred",
            Self::PersistencePayloadUnresolved => "metadata.persistence-payload-unresolved",
            Self::PersistenceSideEntryUnresolved => "metadata.persistence-side-entry-unresolved",
            Self::PersistenceCountNoncanonical => "metadata.persistence-count-noncanonical",
            Self::FeatureCyclicHistory => "feature.cyclic-history",
            Self::FeatureNativeKindRetained => "feature.native-kind-retained",
            Self::SketchNativeGeometry => "sketch.native-geometry",
            Self::SketchNativeConstraint => "sketch.native-constraint",
            Self::SketchExternalReferenceUnresolved => "sketch.external-reference-unresolved",
            Self::SketchExternalCacheUnresolved => "sketch.external-cache-unresolved",
            Self::SketchProfileMatchingBounded => "sketch.profile-matching-bounded",
            Self::AppearanceTopologyColorCountMismatch => {
                "appearance.topology-color-count-mismatch"
            }
            Self::AppearancePrimitiveSizeNotTransferred => {
                "appearance.primitive-size-not-transferred"
            }
            Self::SourceDialectUnverified => "source.dialect-unverified",
            Self::SourceDialectDisplaced => "target.source-dialect-displaced",
            Self::SourceGuiSchemaUnverified => "source.gui-schema-unverified",
            Self::SourceGuiPropertyKeyBlank => "source.gui-property-key-blank",
            Self::SourceGuiMetadataUnresolved => "source.gui-metadata-unresolved",
            Self::ElementMapMetadataUnresolved => "element-map.metadata-unresolved",
            Self::PcurveNotTransferred => "pcurve.not-transferred",
            Self::ProgramVersionNoncanonical => "metadata.program-version-noncanonical",
            Self::FileVersionNoncanonical => "metadata.file-version-noncanonical",
            Self::FileVersionUnverified => "metadata.file-version-unverified",
            Self::ElementMapVersionUnresolved => "element-map.version-unresolved",
        }
    }

    /// The severity of this loss.
    #[must_use]
    const fn severity(self) -> Severity {
        match self {
            Self::ExpressionReferenceUnresolved
            | Self::AttributePropertyUnprojected
            | Self::ExportAttributeNotTransferred
            | Self::ExportOccurrenceNotTransferred
            | Self::ExportJointOperandNotTransferred => Severity::Warning,
            Self::ExportFeatureNotTransferred | Self::ExportGeometryNotTransferred => {
                Severity::Error
            }
            Self::ExportNativePropertyNotTransferred => Severity::Info,
            Self::SourceModelPayloadUnrepresented
            | Self::SourceDesignPayloadUnrepresented
            | Self::ExportArenaNotTransferred
            | Self::ExportAppearanceNotTransferred
            | Self::ExportPresentationNotTransferred
            | Self::ExportProceduralReduced => Severity::Warning,
            Self::FeatureNativeKindRetained
            | Self::SketchNativeGeometry
            | Self::SketchNativeConstraint => Severity::Blocking,
            Self::FeatureCyclicHistory
            | Self::SketchExternalCacheUnresolved
            | Self::SketchExternalReferenceUnresolved
            | Self::SketchProfileMatchingBounded
            | Self::PersistencePayloadUnresolved
            | Self::PersistenceSideEntryUnresolved
            | Self::PersistenceCountNoncanonical
            | Self::AppearanceTopologyColorCountMismatch
            | Self::AppearancePrimitiveSizeNotTransferred
            | Self::SourceDialectUnverified
            | Self::SourceDialectDisplaced
            | Self::SourceGuiSchemaUnverified
            | Self::SourceGuiPropertyKeyBlank
            | Self::SourceGuiMetadataUnresolved
            | Self::ElementMapMetadataUnresolved
            | Self::PcurveNotTransferred
            | Self::ProgramVersionNoncanonical
            | Self::FileVersionNoncanonical
            | Self::FileVersionUnverified
            | Self::ElementMapVersionUnresolved => Severity::Warning,
        }
    }

    const fn shared_taxonomy(self) -> LossTaxonomy {
        match self {
            Self::ExportOccurrenceNotTransferred | Self::ExportJointOperandNotTransferred => {
                LossTaxonomy::AssemblyPlacementsNotTransferred
            }
            Self::ExpressionReferenceUnresolved
            | Self::AttributePropertyUnprojected
            | Self::ExportAttributeNotTransferred => LossTaxonomy::MetadataNotTransferred,
            Self::SourceModelPayloadUnrepresented | Self::SourceDesignPayloadUnrepresented => {
                LossTaxonomy::RecordNotTyped
            }
            Self::ExportArenaNotTransferred => LossTaxonomy::MetadataNotTransferred,
            Self::ExportFeatureNotTransferred => LossTaxonomy::ParametricRecordOmitted,
            Self::ExportAppearanceNotTransferred => LossTaxonomy::MaterialNotTransferred,
            Self::ExportPresentationNotTransferred => LossTaxonomy::MetadataNotTransferred,
            Self::ExportProceduralReduced => LossTaxonomy::ProceduralReduced,
            Self::ExportGeometryNotTransferred => LossTaxonomy::GeometryNotTransferred,
            Self::ExportNativePropertyNotTransferred => LossTaxonomy::MetadataNotTransferred,
            Self::PersistencePayloadUnresolved | Self::PersistenceSideEntryUnresolved => {
                LossTaxonomy::RecordNotTyped
            }
            Self::FeatureCyclicHistory | Self::FeatureNativeKindRetained => {
                LossTaxonomy::FeatureHistoryRetained
            }
            Self::SketchExternalCacheUnresolved
            | Self::SketchExternalReferenceUnresolved
            | Self::SketchNativeGeometry
            | Self::SketchNativeConstraint
            | Self::SketchProfileMatchingBounded
            | Self::SourceGuiPropertyKeyBlank
            | Self::SourceGuiMetadataUnresolved
            | Self::ElementMapMetadataUnresolved
            | Self::PcurveNotTransferred => LossTaxonomy::RecordNotTyped,
            Self::AppearanceTopologyColorCountMismatch
            | Self::AppearancePrimitiveSizeNotTransferred => LossTaxonomy::MaterialNotTransferred,
            Self::PersistenceCountNoncanonical
            | Self::ProgramVersionNoncanonical
            | Self::FileVersionNoncanonical
            | Self::FileVersionUnverified => LossTaxonomy::NoncanonicalSourceSyntax,
            Self::ElementMapVersionUnresolved => LossTaxonomy::RecordNotTyped,
            Self::SourceDialectUnverified => LossTaxonomy::SourceDialectUnverified,
            Self::SourceDialectDisplaced => LossTaxonomy::SourceDialectDisplaced,
            Self::SourceGuiSchemaUnverified => LossTaxonomy::SourceDialectUnverified,
        }
    }

    /// Namespaced [`LossKind`] for this local code, classified by taxonomy.
    #[must_use]
    fn kind(self) -> LossKind {
        let strict_floor = match self {
            Self::SourceGuiSchemaUnverified => None,
            other => other.shared_taxonomy().strict_floor(),
        };
        NamespacedLossKind::new(NAMESPACE, self.code(), self.shared_taxonomy())
            .with_strict_floor(strict_floor)
            .into()
    }

    /// Build a [`LossNote`] for this code with the given per-instance message.
    ///
    /// The structured code is `fcstd/<local>`. Severity comes from the local
    /// code; the strict floor comes from the taxonomy.
    #[must_use]
    pub(crate) fn note(self, message: impl Into<String>) -> LossNote {
        LossNote::new(self.kind(), message).with_severity(self.severity())
    }
}

/// Why a decode loss does or does not leave source content unrepresented.
/// A/B are source records absent from all typed arenas; C retains typed source
/// records with reduced neutral fidelity; D changes no arena content.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[cfg(test)]
enum SourceRecordCoverage {
    /// C: admitted native records retain the source content.
    Typed,
    /// D: syntax or dialect metadata only.
    Metadata,
    /// B: model-class content survives only as bytes or is absent.
    Model,
    /// A: design-class content survives only as bytes or is absent.
    Design,
    /// Writer losses do not describe source decode content.
    Writer,
    /// Per-property admission decides the served class.
    Property,
}

/// Complete audit of decoder losses. An object-unprojected loss is C when its
/// native object and properties are typed; an Unreadable property separately
/// marks the served class below. Diagnostic records never stand in for bytes.
#[cfg(test)]
const SOURCE_RECORD_COVERAGE: &[(FreecadLossCode, SourceRecordCoverage)] = &[
    (
        FreecadLossCode::ExpressionReferenceUnresolved,
        SourceRecordCoverage::Typed,
    ),
    (
        FreecadLossCode::AttributePropertyUnprojected,
        SourceRecordCoverage::Typed,
    ),
    (
        FreecadLossCode::SourceModelPayloadUnrepresented,
        SourceRecordCoverage::Model,
    ),
    (
        FreecadLossCode::SourceDesignPayloadUnrepresented,
        SourceRecordCoverage::Design,
    ),
    (
        FreecadLossCode::ExportAttributeNotTransferred,
        SourceRecordCoverage::Writer,
    ),
    (
        FreecadLossCode::ExportArenaNotTransferred,
        SourceRecordCoverage::Writer,
    ),
    (
        FreecadLossCode::ExportProceduralReduced,
        SourceRecordCoverage::Writer,
    ),
    (
        FreecadLossCode::ExportAppearanceNotTransferred,
        SourceRecordCoverage::Writer,
    ),
    (
        FreecadLossCode::ExportPresentationNotTransferred,
        SourceRecordCoverage::Writer,
    ),
    (
        FreecadLossCode::ExportFeatureNotTransferred,
        SourceRecordCoverage::Writer,
    ),
    (
        FreecadLossCode::ExportOccurrenceNotTransferred,
        SourceRecordCoverage::Writer,
    ),
    (
        FreecadLossCode::ExportJointOperandNotTransferred,
        SourceRecordCoverage::Writer,
    ),
    (
        FreecadLossCode::ExportGeometryNotTransferred,
        SourceRecordCoverage::Writer,
    ),
    (
        FreecadLossCode::ExportNativePropertyNotTransferred,
        SourceRecordCoverage::Writer,
    ),
    (
        FreecadLossCode::PersistencePayloadUnresolved,
        SourceRecordCoverage::Property,
    ),
    (
        FreecadLossCode::PersistenceSideEntryUnresolved,
        SourceRecordCoverage::Property,
    ),
    (
        FreecadLossCode::PersistenceCountNoncanonical,
        SourceRecordCoverage::Metadata,
    ),
    (
        FreecadLossCode::FeatureCyclicHistory,
        SourceRecordCoverage::Typed,
    ),
    (
        FreecadLossCode::FeatureNativeKindRetained,
        SourceRecordCoverage::Typed,
    ),
    (
        FreecadLossCode::SketchNativeGeometry,
        SourceRecordCoverage::Typed,
    ),
    (
        FreecadLossCode::SketchExternalReferenceUnresolved,
        SourceRecordCoverage::Typed,
    ),
    (
        FreecadLossCode::SketchExternalCacheUnresolved,
        SourceRecordCoverage::Design,
    ),
    (
        FreecadLossCode::SketchNativeConstraint,
        SourceRecordCoverage::Typed,
    ),
    (
        FreecadLossCode::SketchProfileMatchingBounded,
        SourceRecordCoverage::Typed,
    ),
    (
        FreecadLossCode::AppearanceTopologyColorCountMismatch,
        SourceRecordCoverage::Model,
    ),
    (
        FreecadLossCode::AppearancePrimitiveSizeNotTransferred,
        SourceRecordCoverage::Model,
    ),
    (
        FreecadLossCode::SourceDialectUnverified,
        SourceRecordCoverage::Metadata,
    ),
    (
        FreecadLossCode::SourceDialectDisplaced,
        SourceRecordCoverage::Writer,
    ),
    (
        FreecadLossCode::SourceGuiSchemaUnverified,
        SourceRecordCoverage::Metadata,
    ),
    (
        FreecadLossCode::SourceGuiPropertyKeyBlank,
        SourceRecordCoverage::Model,
    ),
    (
        FreecadLossCode::SourceGuiMetadataUnresolved,
        SourceRecordCoverage::Model,
    ),
    (
        FreecadLossCode::ElementMapMetadataUnresolved,
        SourceRecordCoverage::Model,
    ),
    (
        FreecadLossCode::PcurveNotTransferred,
        SourceRecordCoverage::Model,
    ),
    (
        FreecadLossCode::ProgramVersionNoncanonical,
        SourceRecordCoverage::Metadata,
    ),
    (
        FreecadLossCode::FileVersionNoncanonical,
        SourceRecordCoverage::Metadata,
    ),
    (
        FreecadLossCode::FileVersionUnverified,
        SourceRecordCoverage::Metadata,
    ),
    (
        FreecadLossCode::ElementMapVersionUnresolved,
        SourceRecordCoverage::Model,
    ),
];

/// Source content that exists in no typed neutral or native arena. This is
/// declared independently of neutral projection scope: geometry decodes retain
/// the complete typed native feature/property graph.
#[cfg(test)]
pub(crate) fn unrepresented(
    losses: &[LossNote],
    native: &cadmpeg_ir::native::NativeNamespace,
) -> Result<cadmpeg_ir::document::ArenaClasses, cadmpeg_core::CodecError> {
    use crate::native::{DocumentFacts, EntryRecord, ObjectRecord, PropertyBody, PropertyRecord};
    use cadmpeg_ir::document::{ArenaClass, ArenaClasses};
    let mut classes = ArenaClasses::new();
    for (code, coverage) in SOURCE_RECORD_COVERAGE {
        if losses
            .iter()
            .any(|loss| loss.code.namespace() == "fcstd" && loss.code.local_code() == code.code())
        {
            match coverage {
                SourceRecordCoverage::Model => {
                    classes.insert(ArenaClass::Model);
                }
                SourceRecordCoverage::Design => {
                    classes.insert(ArenaClass::Design);
                }
                SourceRecordCoverage::Typed
                | SourceRecordCoverage::Metadata
                | SourceRecordCoverage::Property
                | SourceRecordCoverage::Writer => {}
            }
        }
    }
    let objects = native.arena_as::<ObjectRecord>("objects")?;
    let properties = native.arena_as::<PropertyRecord>("properties")?;
    let entries = native.arena_as::<EntryRecord>("entries")?;
    let property_class = |property: &PropertyRecord| {
        if property.type_name == "Part::PropertyPartShape" {
            ArenaClass::Model
        } else if objects.iter().any(|object| {
            object.id().as_str() == property.owner
                && crate::design::is_design_kind(&object.type_name)
        }) || property.owner == crate::native::native_id("document", "0")
        {
            ArenaClass::Design
        } else {
            ArenaClass::Model
        }
    };
    for property in &properties {
        if matches!(property.body, Some(PropertyBody::Unreadable(_))) {
            classes.insert(property_class(property));
        }
        // Missing optional side entries remain in typed filename attributes,
        // but the named payload itself is not represented by that filename.
        if property.values().iter().any(|value| {
            value.attributes.iter().any(|(key, name)| {
                entry_attribute(property, key)
                    && !name.is_empty()
                    && !entries.iter().any(|entry| entry.name() == *name)
            })
        }) {
            classes.insert(property_class(property));
        }
    }

    if objects.is_empty()
        && native
            .arena_as::<DocumentFacts>("document")?
            .iter()
            .any(|document| document.object_count != 0)
    {
        classes.extend([ArenaClass::Model, ArenaClass::Design]);
    }
    Ok(classes)
}

#[cfg(test)]
fn entry_attribute(property: &crate::native::PropertyRecord, name: &str) -> bool {
    (matches!(name, "file" | "File") && !crate::persistence::is_xlink_type(&property.type_name))
        || (property.family == crate::native::PropertyFamily::File
            && matches!(name, "name" | "Name"))
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeSet;

    use super::FreecadLossCode;

    /// Value-level golden: the stable string form of every code, pinned.
    #[test]
    fn code_strings_are_pinned() {
        let codes: Vec<&str> = FreecadLossCode::ALL.iter().map(|c| c.code()).collect();
        assert_eq!(
            codes,
            [
                "expression.reference-unresolved",
                "attribute.property-unprojected",
                "attribute.not-transferred",
                "source.model-payload-unrepresented",
                "source.design-payload-unrepresented",
                "arena.not-transferred",
                "feature.not-transferred",
                "occurrence.not-transferred",
                "joint.operand-not-transferred",
                "geometry.not-transferred",
                "appearance.not-transferred",
                "presentation.not-transferred",
                "geometry.procedural-reduced",
                "metadata.native-property-not-transferred",
                "metadata.persistence-payload-unresolved",
                "metadata.persistence-side-entry-unresolved",
                "metadata.persistence-count-noncanonical",
                "feature.cyclic-history",
                "feature.native-kind-retained",
                "sketch.native-geometry",
                "sketch.native-constraint",
                "sketch.external-reference-unresolved",
                "sketch.external-cache-unresolved",
                "sketch.profile-matching-bounded",
                "appearance.topology-color-count-mismatch",
                "appearance.primitive-size-not-transferred",
                "source.dialect-unverified",
                "target.source-dialect-displaced",
                "source.gui-schema-unverified",
                "source.gui-property-key-blank",
                "source.gui-metadata-unresolved",
                "element-map.metadata-unresolved",
                "pcurve.not-transferred",
                "metadata.program-version-noncanonical",
                "metadata.file-version-noncanonical",
                "metadata.file-version-unverified",
                "element-map.version-unresolved",
            ]
        );
    }

    /// Codes are unique and use the stable `family.detail` kebab shape.
    #[test]
    fn codes_are_unique_and_well_formed() {
        let mut seen = BTreeSet::new();
        for code in FreecadLossCode::ALL {
            let text = code.code();
            assert!(seen.insert(text), "duplicate code {text}");
            let (family, detail) = text.split_once('.').expect("family.detail shape");
            assert!(!family.is_empty() && !detail.is_empty());
            assert!(
                text.bytes()
                    .all(|b| b.is_ascii_lowercase() || b == b'.' || b == b'-'),
                "code {text} is not lowercase kebab"
            );
        }
    }

    /// The note builder fixes severity from the codec-specific code.
    #[test]
    fn note_takes_severity_from_the_code() {
        for code in FreecadLossCode::ALL {
            let note = code.note("x");
            assert_eq!(note.severity, code.severity());
            assert_eq!(note.message, "x");
            assert!(note.provenance.is_none());
        }
    }

    #[test]
    fn gui_schema_recovery_does_not_trigger_document_dialect_strictness() {
        assert_eq!(
            FreecadLossCode::SourceGuiSchemaUnverified
                .kind()
                .strict_floor(),
            None
        );
        assert_eq!(
            FreecadLossCode::SourceDialectUnverified
                .kind()
                .strict_floor(),
            Some(cadmpeg_ir::report::Severity::Warning)
        );
    }

    #[test]
    fn every_decode_loss_has_one_source_record_disposition() {
        let audited: BTreeSet<_> = super::SOURCE_RECORD_COVERAGE
            .iter()
            .map(|(code, _)| *code)
            .collect();
        assert_eq!(audited.len(), super::SOURCE_RECORD_COVERAGE.len());
        assert_eq!(audited, FreecadLossCode::ALL.iter().copied().collect());
    }

    #[test]
    fn source_record_dispositions_distinguish_reduced_fidelity_and_untyped_content() {
        use cadmpeg_ir::document::{ArenaClass, ArenaClasses, CadIr};
        let mut ir = CadIr::empty();
        let namespace = ir.native.namespace_mut("fcstd");
        for code in [
            FreecadLossCode::FeatureCyclicHistory,
            FreecadLossCode::FeatureNativeKindRetained,
            FreecadLossCode::SketchNativeGeometry,
            FreecadLossCode::SketchNativeConstraint,
            FreecadLossCode::SketchProfileMatchingBounded,
        ] {
            assert!(
                super::unrepresented(&[code.note("retained typed content")], namespace)
                    .unwrap()
                    .is_empty()
            );
        }
        for code in [
            FreecadLossCode::AppearanceTopologyColorCountMismatch,
            FreecadLossCode::AppearancePrimitiveSizeNotTransferred,
            FreecadLossCode::PcurveNotTransferred,
        ] {
            assert_eq!(
                super::unrepresented(&[code.note("unrepresented model record")], namespace)
                    .unwrap(),
                ArenaClasses::from([ArenaClass::Model])
            );
        }
    }

    fn namespace_from_xml(xml: &str) -> cadmpeg_ir::document::CadIr {
        let graph = crate::persistence::parse_document(
            &roxmltree::Document::parse(xml).unwrap(),
            crate::persistence::Vocabulary::Objects,
        )
        .unwrap();
        let mut ir = cadmpeg_ir::document::CadIr::empty();
        let ctx = cadmpeg_test_support::service_decode_context();
        let native = ir.native.namespace_mut("fcstd");
        native.set_arena(&ctx, "objects", &graph.objects).unwrap();
        native
            .set_arena(&ctx, "properties", &graph.properties)
            .unwrap();
        ir
    }

    #[test]
    fn unreadable_property_marks_its_served_class_even_with_native_diagnostic() {
        use cadmpeg_ir::document::{ArenaClass, ArenaClasses};
        let xml = r#"<Document SchemaVersion="4"><Objects Count="1"><Object name="Extension" type="PartDesign::VendorFeature"/></Objects><ObjectData Count="1"><Object name="Extension"><Properties Count="1"><Property name="Input" type="App::PropertyLink"/></Properties></Object></ObjectData></Document>"#;
        let mut ir = namespace_from_xml(xml);
        let namespace = ir.native.namespace_mut("fcstd");
        let mut properties = namespace
            .arena_as::<crate::native::PropertyRecord>("properties")
            .unwrap();
        assert!(matches!(
            properties[0].body,
            Some(crate::native::PropertyBody::Unreadable(_))
        ));
        assert_eq!(
            super::unrepresented(&[], namespace).unwrap(),
            ArenaClasses::from([ArenaClass::Design])
        );
        properties[0].type_name = "Part::PropertyPartShape".into();
        namespace
            .set_arena(
                &cadmpeg_test_support::service_decode_context(),
                "properties",
                &properties,
            )
            .unwrap();
        assert_eq!(
            super::unrepresented(&[], namespace).unwrap(),
            ArenaClasses::from([ArenaClass::Model])
        );
    }

    #[test]
    fn unreadable_archive_payload_is_not_represented_by_its_typed_diagnostic() {
        use cadmpeg_ir::{Codec, DecodeOptions};
        use cadmpeg_test_support::zip_fixture::{self, RawEntry};
        let xml = r#"<Document SchemaVersion="4"><Objects Count="1"><Object name="Sketch" type="Sketcher::SketchObject"/></Objects><ObjectData Count="1"><Object name="Sketch"><Properties Count="1"><Property name="ExternalGeo" type="App::PropertyFile"><File file="cache.bin"/></Property></Properties></Object></ObjectData></Document>"#;
        for (name, expected) in [
            ("cache.bin", cadmpeg_ir::ArenaClass::Design),
            ("Shape.brp", cadmpeg_ir::ArenaClass::Model),
            ("Thumbnail.png", cadmpeg_ir::ArenaClass::Model),
        ] {
            let bytes = zip_fixture::archive(&[
                RawEntry::stored("Document.xml", xml.as_bytes()),
                RawEntry::compressed(name, 12, &[1], &[1]),
            ]);
            let decoded = crate::FcstdCodec
                .decode(&mut std::io::Cursor::new(bytes), &DecodeOptions::default())
                .unwrap();
            let unknowns = decoded.ir().native_unknowns("fcstd").unwrap();
            assert_eq!(unknowns.len(), 1);
            assert!(decoded
                .report()
                .losses
                .iter()
                .any(|loss| loss.code.local_code() == "record.unreadable"));
            assert!(!decoded
                .ir()
                .native
                .namespace("fcstd")
                .unwrap()
                .arenas()
                .contains_key("unreadable_entries"));
            assert_eq!(
                decoded
                    .source_fidelity()
                    .retained_record(unknowns[0].id.as_str())
                    .unwrap()
                    .data(),
                Some([1].as_slice())
            );
            assert_eq!(
                decoded.source_fidelity().unrepresented(),
                cadmpeg_ir::ArenaClasses::from([expected])
            );
        }
    }
}
