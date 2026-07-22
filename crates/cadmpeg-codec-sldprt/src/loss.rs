// SPDX-License-Identifier: Apache-2.0
//! Stable loss vocabulary for `.sldprt` decoding.
//!
//! Every fallback, approximation, and drop the decoder reports carries a
//! stable machine-readable code from [`SldprtLossCode`]. Codes are the gating
//! surface: harness oracles and downstream tooling key on them, never on the
//! human-readable message text, so a reworded message is not a contract change
//! and a new drop path without a code does not compile.
//!
//! [`SldprtLossCode::note`] is the single practical construction path for a
//! decode-time [`LossNote`] in this crate: it fixes the loss category and
//! severity from the code so the two cannot drift apart across sites, and it
//! leaves only the per-instance message to the caller.
//!
//! The code is not yet persisted onto [`LossNote`]: the shared IR type has no
//! `code` field in this branch (doc §6.2 assigns it one). Until that field
//! lands, the code is enforced at construction and greppable at every site;
//! the mapping here is the source of truth a follow-up wires into the report.

use cadmpeg_ir::report::{LossCategory, LossNote, Severity};

/// A stable, machine-readable identifier for one `.sldprt` transfer loss.
///
/// Variants are grouped by the record family whose transfer degraded. The
/// string form (via [`SldprtLossCode::code`]) is the stable contract; the Rust
/// variant name may be refactored freely.
#[non_exhaustive]
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum SldprtLossCode {
    /// Active configuration identity does not resolve to exactly one record.
    ConfigActiveIdentityUnresolved,
    /// Active configuration does not resolve to the active geometry partition.
    ConfigActivePartitionMismatch,
    /// Configuration state inferred from geometry without a native definition.
    ConfigInferredWithoutNative,
    /// Configuration-scoped feature-input lane has unresolved configuration id.
    ConfigLaneIdentityUnresolved,
    /// Configuration records share non-unique geometry partition identities.
    ConfigAmbiguousPartition,
    /// Configuration records have empty, duplicate, or colliding names/ordinals.
    ConfigAmbiguousNaming,
    /// Configuration record references missing or repeated bodies.
    ConfigIncoherentBodyRefs,
    /// Configuration lacks a complete evaluated feature/parameter snapshot.
    ConfigIncompleteSnapshot,
    /// Parameter lacks an evaluated scalar or a regenerable finite value.
    ParameterUnevaluated,
    /// Parameter records have empty, duplicate, or colliding names/ordinals.
    ParameterAmbiguousIdentity,
    /// Semantic dimension record is unbound or retains a native subtype.
    PmiDimensionUnbound,
    /// Feature history record has duplicate identity or unresolved references.
    HistoryIncompleteReferences,
    /// Feature record has missing, repeated, or non-preceding tree edges.
    FeatureIncoherentEdges,
    /// Feature record has inconsistent source-content references.
    FeatureIncoherentContent,
    /// Feature retains a native output scope that does not resolve to a body.
    FeatureUnresolvedOutputScope,
    /// Feature record has missing or repeated output body references.
    FeatureIncoherentOutputs,
    /// Sketch constraint retains a native relation kind without neutral semantics.
    SketchNativeConstraint,
    /// Sketch geometry record retains a native kind without solved geometry.
    SketchNativeGeometry,
    /// Native sketch relation has no projected neutral constraint.
    SketchRelationUnprojected,
    /// Native sketch relation is claimed by multiple neutral objects.
    SketchRelationMultiplyProjected,
    /// Feature retains its native kind without a complete neutral operation.
    FeatureNativeKindRetained,
    /// Native feature-input operation object does not bind uniquely to a feature.
    FeatureInputObjectUnbound,
    /// Typed feature retains native or unresolved required operation operands.
    FeatureTypedOperandIncomplete,
    /// Body delete/keep feature retains native selection without a decoded mode.
    FeatureBodyRetentionUnresolved,
    /// Face rests on a support surface this codec does not type; shape is opaque.
    GeometryFaceSupportSurfaceUntyped,
    /// Edge references an untyped support curve carried opaque.
    GeometryEdgeSupportCurveUntyped,
    /// No body record was available; a body hierarchy was derived.
    TopologyBodyHierarchyDerived,
    /// Parasolid B-rep geometry was not transferred (no resolved stream).
    GeometryParasolidNotTransferred,
    /// B-rep topology graph was not built for this file.
    TopologyGraphNotTransferred,
    /// Materials, tessellation, and metadata were not transferred.
    MaterialMetadataNotTransferred,
    /// No Parasolid partition/deltas stream was located in the container.
    ContainerNoParasolidStream,
}

impl SldprtLossCode {
    /// Every code, in declaration order. Used by tests to assert stability.
    pub const ALL: &'static [SldprtLossCode] = &[
        Self::ConfigActiveIdentityUnresolved,
        Self::ConfigActivePartitionMismatch,
        Self::ConfigInferredWithoutNative,
        Self::ConfigLaneIdentityUnresolved,
        Self::ConfigAmbiguousPartition,
        Self::ConfigAmbiguousNaming,
        Self::ConfigIncoherentBodyRefs,
        Self::ConfigIncompleteSnapshot,
        Self::ParameterUnevaluated,
        Self::ParameterAmbiguousIdentity,
        Self::PmiDimensionUnbound,
        Self::HistoryIncompleteReferences,
        Self::FeatureIncoherentEdges,
        Self::FeatureIncoherentContent,
        Self::FeatureUnresolvedOutputScope,
        Self::FeatureIncoherentOutputs,
        Self::SketchNativeConstraint,
        Self::SketchNativeGeometry,
        Self::SketchRelationUnprojected,
        Self::SketchRelationMultiplyProjected,
        Self::FeatureNativeKindRetained,
        Self::FeatureInputObjectUnbound,
        Self::FeatureTypedOperandIncomplete,
        Self::FeatureBodyRetentionUnresolved,
        Self::GeometryFaceSupportSurfaceUntyped,
        Self::GeometryEdgeSupportCurveUntyped,
        Self::TopologyBodyHierarchyDerived,
        Self::GeometryParasolidNotTransferred,
        Self::TopologyGraphNotTransferred,
        Self::MaterialMetadataNotTransferred,
        Self::ContainerNoParasolidStream,
    ];

    /// The stable string identifier. This is the gating contract.
    #[must_use]
    pub const fn code(self) -> &'static str {
        match self {
            Self::ConfigActiveIdentityUnresolved => "config.active-identity-unresolved",
            Self::ConfigActivePartitionMismatch => "config.active-partition-mismatch",
            Self::ConfigInferredWithoutNative => "config.inferred-without-native",
            Self::ConfigLaneIdentityUnresolved => "config.lane-identity-unresolved",
            Self::ConfigAmbiguousPartition => "config.ambiguous-partition",
            Self::ConfigAmbiguousNaming => "config.ambiguous-naming",
            Self::ConfigIncoherentBodyRefs => "config.incoherent-body-refs",
            Self::ConfigIncompleteSnapshot => "config.incomplete-snapshot",
            Self::ParameterUnevaluated => "parameter.unevaluated",
            Self::ParameterAmbiguousIdentity => "parameter.ambiguous-identity",
            Self::PmiDimensionUnbound => "pmi.dimension-unbound",
            Self::HistoryIncompleteReferences => "history.incomplete-references",
            Self::FeatureIncoherentEdges => "feature.incoherent-edges",
            Self::FeatureIncoherentContent => "feature.incoherent-content",
            Self::FeatureUnresolvedOutputScope => "feature.unresolved-output-scope",
            Self::FeatureIncoherentOutputs => "feature.incoherent-outputs",
            Self::SketchNativeConstraint => "sketch.native-constraint",
            Self::SketchNativeGeometry => "sketch.native-geometry",
            Self::SketchRelationUnprojected => "sketch.relation-unprojected",
            Self::SketchRelationMultiplyProjected => "sketch.relation-multiply-projected",
            Self::FeatureNativeKindRetained => "feature.native-kind-retained",
            Self::FeatureInputObjectUnbound => "feature.input-object-unbound",
            Self::FeatureTypedOperandIncomplete => "feature.typed-operand-incomplete",
            Self::FeatureBodyRetentionUnresolved => "feature.body-retention-unresolved",
            Self::GeometryFaceSupportSurfaceUntyped => "geometry.face-support-surface-untyped",
            Self::GeometryEdgeSupportCurveUntyped => "geometry.edge-support-curve-untyped",
            Self::TopologyBodyHierarchyDerived => "topology.body-hierarchy-derived",
            Self::GeometryParasolidNotTransferred => "geometry.parasolid-not-transferred",
            Self::TopologyGraphNotTransferred => "topology.graph-not-transferred",
            Self::MaterialMetadataNotTransferred => "material.metadata-not-transferred",
            Self::ContainerNoParasolidStream => "container.no-parasolid-stream",
        }
    }

    /// The subsystem category this loss belongs to.
    #[must_use]
    pub const fn category(self) -> LossCategory {
        match self {
            Self::GeometryFaceSupportSurfaceUntyped
            | Self::GeometryEdgeSupportCurveUntyped
            | Self::GeometryParasolidNotTransferred
            | Self::ContainerNoParasolidStream => LossCategory::Geometry,
            Self::TopologyBodyHierarchyDerived | Self::TopologyGraphNotTransferred => {
                LossCategory::Topology
            }
            Self::MaterialMetadataNotTransferred => LossCategory::Material,
            _ => LossCategory::Other,
        }
    }

    /// The severity of this loss.
    #[must_use]
    pub const fn severity(self) -> Severity {
        match self {
            Self::GeometryParasolidNotTransferred | Self::TopologyGraphNotTransferred => {
                Severity::Blocking
            }
            Self::ContainerNoParasolidStream => Severity::Error,
            _ => Severity::Warning,
        }
    }

    /// Build a [`LossNote`] for this code with the given per-instance message.
    ///
    /// Category and severity come from the code, so a site cannot mislabel a
    /// loss it names. Provenance is left absent; the decoder attributes losses
    /// through the message and record identity, not a source span.
    #[must_use]
    pub fn note(self, message: impl Into<String>) -> LossNote {
        LossNote {
            category: self.category(),
            severity: self.severity(),
            message: message.into(),
            provenance: None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::SldprtLossCode;
    use std::collections::BTreeSet;

    /// Value-level golden: the stable string form of every code, pinned. A
    /// diff here is an intentional contract change to a gating identifier.
    #[test]
    fn code_strings_are_pinned() {
        let codes: Vec<&str> = SldprtLossCode::ALL.iter().map(|c| c.code()).collect();
        assert_eq!(
            codes,
            [
                "config.active-identity-unresolved",
                "config.active-partition-mismatch",
                "config.inferred-without-native",
                "config.lane-identity-unresolved",
                "config.ambiguous-partition",
                "config.ambiguous-naming",
                "config.incoherent-body-refs",
                "config.incomplete-snapshot",
                "parameter.unevaluated",
                "parameter.ambiguous-identity",
                "pmi.dimension-unbound",
                "history.incomplete-references",
                "feature.incoherent-edges",
                "feature.incoherent-content",
                "feature.unresolved-output-scope",
                "feature.incoherent-outputs",
                "sketch.native-constraint",
                "sketch.native-geometry",
                "sketch.relation-unprojected",
                "sketch.relation-multiply-projected",
                "feature.native-kind-retained",
                "feature.input-object-unbound",
                "feature.typed-operand-incomplete",
                "feature.body-retention-unresolved",
                "geometry.face-support-surface-untyped",
                "geometry.edge-support-curve-untyped",
                "topology.body-hierarchy-derived",
                "geometry.parasolid-not-transferred",
                "topology.graph-not-transferred",
                "material.metadata-not-transferred",
                "container.no-parasolid-stream",
            ]
        );
    }

    /// Codes are unique and use the stable `family.detail` kebab shape.
    #[test]
    fn codes_are_unique_and_well_formed() {
        let mut seen = BTreeSet::new();
        for code in SldprtLossCode::ALL {
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

    /// The note builder fixes category and severity from the code so a call
    /// site cannot mislabel a loss it names.
    #[test]
    fn note_takes_category_and_severity_from_the_code() {
        for code in SldprtLossCode::ALL {
            let note = code.note("x");
            assert_eq!(note.category, code.category());
            assert_eq!(note.severity, code.severity());
            assert_eq!(note.message, "x");
            assert!(note.provenance.is_none());
        }
    }
}
