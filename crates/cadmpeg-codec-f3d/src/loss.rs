// SPDX-License-Identifier: Apache-2.0
//! Stable loss vocabulary for `.f3d` decoding.
//!
//! Every fallback, approximation, and drop the decoder reports carries a stable
//! machine-readable code from [`F3dLossCode`]. Codes are the gating surface:
//! harness oracles and downstream tooling key on them, never on the
//! human-readable message text, so a reworded message is not a contract change
//! and a new drop path without a code does not compile.
//!
//! [`F3dLossCode::note`] is the single construction path for a decode-time
//! [`LossNote`] in this crate: it fixes the shared loss code, the category, and
//! the severity from the variant so the three cannot drift apart across sites,
//! leaving only the per-instance message to the caller.
//!
//! Where two sites share a conceptual loss but disagree on category or severity,
//! they are kept as distinct variants rather than normalized: `RecordNotTyped`,
//! `ReferenceGraphNotClosed`, `GeometryNotTransferred`, `MetadataNotTransferred`,
//! and `AssemblyComponentsExternal` each back more than one variant so the fixed
//! category/severity stays faithful to the site it replaced.

use cadmpeg_ir::report::{LossCategory, LossCode, LossNote, Severity};

/// A stable, machine-readable identifier for one `.f3d` transfer loss.
///
/// Variants are grouped by the record family whose transfer degraded. The string
/// form (via [`F3dLossCode::code`]) is the stable contract; the Rust variant name
/// may be refactored freely.
#[non_exhaustive]
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum F3dLossCode {
    /// Design dimension companion record retained without a typed locus frame.
    DesignDimensionCompanionUntyped,
    /// Design configuration JSON member retained without neutral semantics.
    DesignConfigMemberUnassigned,
    /// Design configuration rule retained without a neutral activation target.
    DesignConfigRuleUnresolved,
    /// Design configuration parameter override without a neutral parameter.
    DesignConfigParameterOverrideUnresolved,
    /// Design configuration feature suppression without a neutral feature.
    DesignConfigFeatureSuppressionUnresolved,
    /// Design body-map pair does not resolve to a body in the named BREP blob.
    DesignBodyBindingUnresolved,
    /// Design feature/sketch/parameter scope retained as native passthrough
    /// because no complete neutral projection was resolved.
    DesignProjectionGapRetained,
    /// A Design-referenced BREP blob could not be decoded.
    DesignReferencedBrepUndecoded,
    /// Source parametric edge references were marked lost and cannot be replayed.
    ParametricEdgeReferenceLost,
    /// Present `RedirectionsStream.dat` external-reference table did not decode.
    ExternalReferenceTableUndecoded,
    /// Assembly document geometry is defined by external references (info).
    AssemblyGeometryExternal,
    /// Spline surface records decoded into NURBS carriers (census).
    SplineSurfaceCarrierDecoded,
    /// Procedural curve records decoded into NURBS carriers (census).
    ProceduralCurveCarrierDecoded,
    /// A face's required surface reference was null or dangling; face omitted.
    FaceSurfaceReferenceDangling,
    /// A face rests on a spline/procedural surface carried as unknown geometry.
    UnknownSurfaceFaceCarried,
    /// Faces use zero-payload `mesh_surface` sentinels (census).
    MeshSurfaceSentinelCarried,
    /// An edge references a procedural 3D curve emitted without a curve carrier.
    ProceduralCurveEdgeUnattributed,
    /// A coedge's UV pcurve reference had no decodable 2D carrier.
    PcurveReferenceUndecoded,
    /// A rolling-ball blend resolved only one of its two native supports.
    ProceduralSupportPartiallyResolved,
    /// Active-slice application/refinement records were not transferred.
    ActiveSliceRecordUntransferred,
    /// Materials, appearances, and design assignments were not transferred.
    MaterialsNotTransferred,
    /// ASM B-rep geometry was not transferred (undecodable SAB stream).
    AsmGeometryNotTransferred,
    /// The B-rep topology graph was not built for the stream.
    AsmTopologyNotTransferred,
    /// No ASM BREP stream (`.smb`/`.smbh`) was found in the container.
    NoAsmBrepStream,
    /// An assembly occurrence was not resolved (cycle, missing member, or unit
    /// mismatch).
    AssemblyOccurrenceUnresolved,
}

impl F3dLossCode {
    /// Every code, in declaration order. Used by tests to assert stability.
    pub const ALL: &'static [F3dLossCode] = &[
        Self::DesignDimensionCompanionUntyped,
        Self::DesignConfigMemberUnassigned,
        Self::DesignConfigRuleUnresolved,
        Self::DesignConfigParameterOverrideUnresolved,
        Self::DesignConfigFeatureSuppressionUnresolved,
        Self::DesignBodyBindingUnresolved,
        Self::DesignProjectionGapRetained,
        Self::DesignReferencedBrepUndecoded,
        Self::ParametricEdgeReferenceLost,
        Self::ExternalReferenceTableUndecoded,
        Self::AssemblyGeometryExternal,
        Self::SplineSurfaceCarrierDecoded,
        Self::ProceduralCurveCarrierDecoded,
        Self::FaceSurfaceReferenceDangling,
        Self::UnknownSurfaceFaceCarried,
        Self::MeshSurfaceSentinelCarried,
        Self::ProceduralCurveEdgeUnattributed,
        Self::PcurveReferenceUndecoded,
        Self::ProceduralSupportPartiallyResolved,
        Self::ActiveSliceRecordUntransferred,
        Self::MaterialsNotTransferred,
        Self::AsmGeometryNotTransferred,
        Self::AsmTopologyNotTransferred,
        Self::NoAsmBrepStream,
        Self::AssemblyOccurrenceUnresolved,
    ];

    /// The stable string identifier. This is the gating contract.
    #[must_use]
    pub const fn code(self) -> &'static str {
        match self {
            Self::DesignDimensionCompanionUntyped => "design.dimension-companion-untyped",
            Self::DesignConfigMemberUnassigned => "config.member-unassigned",
            Self::DesignConfigRuleUnresolved => "config.rule-unresolved",
            Self::DesignConfigParameterOverrideUnresolved => "config.parameter-override-unresolved",
            Self::DesignConfigFeatureSuppressionUnresolved => {
                "config.feature-suppression-unresolved"
            }
            Self::DesignBodyBindingUnresolved => "design.body-binding-unresolved",
            Self::DesignProjectionGapRetained => "design.projection-gap-retained",
            Self::DesignReferencedBrepUndecoded => "design.referenced-brep-undecoded",
            Self::ParametricEdgeReferenceLost => "attribute.parametric-edge-reference-lost",
            Self::ExternalReferenceTableUndecoded => "xref.table-undecoded",
            Self::AssemblyGeometryExternal => "assembly.geometry-external",
            Self::SplineSurfaceCarrierDecoded => "carrier.spline-surface-decoded",
            Self::ProceduralCurveCarrierDecoded => "carrier.procedural-curve-decoded",
            Self::FaceSurfaceReferenceDangling => "topology.face-surface-reference-dangling",
            Self::UnknownSurfaceFaceCarried => "geometry.unknown-surface-face-carried",
            Self::MeshSurfaceSentinelCarried => "carrier.mesh-surface-sentinel",
            Self::ProceduralCurveEdgeUnattributed => "geometry.procedural-curve-edge-unattributed",
            Self::PcurveReferenceUndecoded => "geometry.pcurve-reference-undecoded",
            Self::ProceduralSupportPartiallyResolved => "geometry.procedural-support-partial",
            Self::ActiveSliceRecordUntransferred => "attribute.active-slice-record-untransferred",
            Self::MaterialsNotTransferred => "material.not-transferred",
            Self::AsmGeometryNotTransferred => "geometry.asm-not-transferred",
            Self::AsmTopologyNotTransferred => "topology.asm-not-transferred",
            Self::NoAsmBrepStream => "container.no-brep-stream",
            Self::AssemblyOccurrenceUnresolved => "assembly.occurrence-unresolved",
        }
    }

    /// The subsystem category this loss belongs to.
    #[must_use]
    pub const fn category(self) -> LossCategory {
        match self {
            Self::DesignReferencedBrepUndecoded
            | Self::AssemblyGeometryExternal
            | Self::SplineSurfaceCarrierDecoded
            | Self::ProceduralCurveCarrierDecoded
            | Self::UnknownSurfaceFaceCarried
            | Self::MeshSurfaceSentinelCarried
            | Self::ProceduralCurveEdgeUnattributed
            | Self::PcurveReferenceUndecoded
            | Self::ProceduralSupportPartiallyResolved
            | Self::AsmGeometryNotTransferred
            | Self::NoAsmBrepStream
            | Self::AssemblyOccurrenceUnresolved => LossCategory::Geometry,
            Self::DesignBodyBindingUnresolved
            | Self::FaceSurfaceReferenceDangling
            | Self::AsmTopologyNotTransferred => LossCategory::Topology,
            Self::MaterialsNotTransferred => LossCategory::Material,
            Self::ExternalReferenceTableUndecoded => LossCategory::Metadata,
            Self::ParametricEdgeReferenceLost | Self::ActiveSliceRecordUntransferred => {
                LossCategory::Attribute
            }
            Self::DesignDimensionCompanionUntyped
            | Self::DesignConfigMemberUnassigned
            | Self::DesignConfigRuleUnresolved
            | Self::DesignConfigParameterOverrideUnresolved
            | Self::DesignConfigFeatureSuppressionUnresolved
            | Self::DesignProjectionGapRetained => LossCategory::Other,
        }
    }

    /// The severity of this loss.
    #[must_use]
    pub const fn severity(self) -> Severity {
        match self {
            Self::AssemblyGeometryExternal
            | Self::SplineSurfaceCarrierDecoded
            | Self::ProceduralCurveCarrierDecoded
            | Self::MeshSurfaceSentinelCarried => Severity::Info,
            Self::AsmGeometryNotTransferred | Self::AsmTopologyNotTransferred => Severity::Blocking,
            Self::NoAsmBrepStream | Self::AssemblyOccurrenceUnresolved => Severity::Error,
            _ => Severity::Warning,
        }
    }

    /// The shared IR loss code this variant serializes as.
    const fn shared_code(self) -> LossCode {
        match self {
            Self::DesignDimensionCompanionUntyped | Self::ActiveSliceRecordUntransferred => {
                LossCode::RecordNotTyped
            }
            Self::DesignConfigMemberUnassigned
            | Self::DesignConfigRuleUnresolved
            | Self::DesignConfigParameterOverrideUnresolved
            | Self::DesignConfigFeatureSuppressionUnresolved
            | Self::ExternalReferenceTableUndecoded => LossCode::MetadataNotTransferred,
            Self::DesignBodyBindingUnresolved
            | Self::FaceSurfaceReferenceDangling
            | Self::PcurveReferenceUndecoded => LossCode::ReferenceGraphNotClosed,
            Self::DesignProjectionGapRetained => LossCode::FeatureHistoryRetained,
            Self::DesignReferencedBrepUndecoded
            | Self::UnknownSurfaceFaceCarried
            | Self::AsmGeometryNotTransferred => LossCode::GeometryNotTransferred,
            Self::ParametricEdgeReferenceLost => LossCode::AttributesNotTransferred,
            Self::AssemblyGeometryExternal | Self::AssemblyOccurrenceUnresolved => {
                LossCode::AssemblyComponentsExternal
            }
            Self::SplineSurfaceCarrierDecoded
            | Self::ProceduralCurveCarrierDecoded
            | Self::MeshSurfaceSentinelCarried => LossCode::CarrierSummary,
            Self::ProceduralCurveEdgeUnattributed | Self::ProceduralSupportPartiallyResolved => {
                LossCode::ProceduralReduced
            }
            Self::MaterialsNotTransferred => LossCode::MaterialNotTransferred,
            Self::AsmTopologyNotTransferred => LossCode::TopologyNotTransferred,
            Self::NoAsmBrepStream => LossCode::MissingGeometryStream,
        }
    }

    /// Build a [`LossNote`] for this code with the given per-instance message.
    ///
    /// The shared code, category, and severity come from the variant, so a site
    /// cannot mislabel a loss it names. Provenance is left absent; the decoder
    /// attributes losses through the message and record identity, not a source
    /// span.
    #[must_use]
    pub fn note(self, message: impl Into<String>) -> LossNote {
        LossNote {
            code: self.shared_code(),
            category: self.category(),
            severity: self.severity(),
            message: message.into(),
            provenance: None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::F3dLossCode;
    use std::collections::BTreeSet;

    /// Value-level golden: the stable string form of every code, pinned. A diff
    /// here is an intentional contract change to a gating identifier.
    #[test]
    fn code_strings_are_pinned() {
        let codes: Vec<&str> = F3dLossCode::ALL.iter().map(|c| c.code()).collect();
        assert_eq!(
            codes,
            [
                "design.dimension-companion-untyped",
                "config.member-unassigned",
                "config.rule-unresolved",
                "config.parameter-override-unresolved",
                "config.feature-suppression-unresolved",
                "design.body-binding-unresolved",
                "design.projection-gap-retained",
                "design.referenced-brep-undecoded",
                "attribute.parametric-edge-reference-lost",
                "xref.table-undecoded",
                "assembly.geometry-external",
                "carrier.spline-surface-decoded",
                "carrier.procedural-curve-decoded",
                "topology.face-surface-reference-dangling",
                "geometry.unknown-surface-face-carried",
                "carrier.mesh-surface-sentinel",
                "geometry.procedural-curve-edge-unattributed",
                "geometry.pcurve-reference-undecoded",
                "geometry.procedural-support-partial",
                "attribute.active-slice-record-untransferred",
                "material.not-transferred",
                "geometry.asm-not-transferred",
                "topology.asm-not-transferred",
                "container.no-brep-stream",
                "assembly.occurrence-unresolved",
            ]
        );
    }

    /// Codes are unique and use the stable `family.detail` kebab shape.
    #[test]
    fn codes_are_unique_and_well_formed() {
        let mut seen = BTreeSet::new();
        for code in F3dLossCode::ALL {
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

    /// The note builder fixes the shared code, category, and severity from the
    /// variant so a call site cannot mislabel a loss it names.
    #[test]
    fn note_takes_code_category_and_severity_from_the_variant() {
        for code in F3dLossCode::ALL {
            let note = code.note("x");
            assert_eq!(note.code, code.shared_code());
            assert_eq!(note.category, code.category());
            assert_eq!(note.severity, code.severity());
            assert_eq!(note.message, "x");
            assert!(note.provenance.is_none());
        }
    }
}
