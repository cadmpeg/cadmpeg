// SPDX-License-Identifier: Apache-2.0
//! Stable loss vocabulary for `OpenUSD` layer decoding and encoding.
//!
//! Every fallback, approximation, and drop carries a stable code from
//! [`UsdLossCode`]. Codes are the gating surface: harness oracles and
//! downstream tooling key on them, never on the message text.
//!
//! [`UsdLossCode::note`] is the single construction path for a [`LossNote`]
//! in this crate: it fixes the category and severity from the code and
//! leaves only the per-instance message to the caller.

use cadmpeg_ir::report::loss::{LossKind, LossNamespace, LossNote, LossTaxonomy};
use cadmpeg_ir::report::Severity;

const NAMESPACE: LossNamespace<'static> = cadmpeg_ir::loss_namespace!("usd");

/// A stable, machine-readable identifier for one `OpenUSD` transfer loss.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub(crate) enum UsdLossCode {
    /// The header names no declared text version.
    SourceDialectUnverified,
    /// Sublayers of the layer were not composed.
    CompositionSublayersNotComposed,
    /// A reference or payload to another layer was kept as an external
    /// prototype; its content was not loaded.
    CompositionExternalArc,
    /// A layer that the layer names did not open or did not parse.
    CompositionLayerUnavailable,
    /// An internal reference or payload names no prim of the layer.
    CompositionTargetMissing,
    /// Variants that no selection names were not composed.
    CompositionVariantNotSelected,
    /// An `over` that no arc of the layer defines was not composed.
    CompositionOverUndefined,
    /// A composition arc forms a cycle and was not expanded.
    CompositionCycle,
    /// Opinions of a referencing prim on referenced descendants were not
    /// applied.
    CompositionOverrideNotApplied,
    /// Prims of a schema type with no neutral form were not transferred.
    PrimTypeNotTransferred,
    /// Intrinsic primitives (`Cube`, `Sphere`, …) were not transferred.
    PrimIntrinsicNotTransferred,
    /// The properties of an applied API schema were not transferred.
    ApiSchemaNotTransferred,
    /// Properties of group prims that no reader carries were not
    /// transferred.
    PropertyNotTransferred,
    /// An attribute value does not have its declared type.
    AttributeValueMalformed,
    /// Time samples and splines were not transferred; the static value is
    /// the default value, or the first sample when no default is authored.
    AttributeAnimationNotTransferred,
    /// Layer, prim, and property metadata with no neutral form were not
    /// transferred.
    MetadataNotTransferred,
    /// USDC field values of a type the crate reader does not decode were
    /// not read.
    CrateValueNotRead,
    /// A transform-op stack does not resolve; the prim was placed at its
    /// parent.
    XformMalformed,
    /// A subdivision mesh was transferred as its control cage.
    MeshSubdivisionNotApplied,
    /// Mesh faces that are degenerate or index no point were not
    /// transferred.
    MeshFaceRejected,
    /// A mesh failed tessellation admission and was not transferred.
    MeshRejected,
    /// Mesh primvars other than normals and display color were not
    /// transferred.
    MeshPrimvarNotTransferred,
    /// Normals with an interpolation the tessellation does not carry were not
    /// transferred.
    MeshNormalsNotTransferred,
    /// Geometry subsets that do not partition faces were not transferred.
    MeshSubsetNotTransferred,
    /// A curve batch, or one curve of it, does not satisfy its topology and
    /// was not transferred.
    CurvesRejected,
    /// A NURBS patch does not satisfy its counts and was not transferred.
    NurbsPatchRejected,
    /// Trim curves of a NURBS patch were not transferred.
    NurbsTrimNotTransferred,
    /// A NURBS range narrower than the knot domain was not transferred.
    NurbsRangeNotTransferred,
    /// Display color or opacity with a non-constant interpolation was not
    /// transferred.
    DisplayColorNotTransferred,
    /// A material that is not a constant `UsdPreviewSurface` was not
    /// transferred, or a texture connection of one was not transferred.
    MaterialNotTransferred,
    /// A texture path names no package member and no companion file; the
    /// texture keeps the path as written.
    TextureUnlocated,
    /// A `BrepArray` prim, or one B-rep of it, whose arrays do not describe
    /// a neutral B-rep was not transferred.
    BrepRejected,
    /// Wire edges and point shells of a `BrepArray` prim were not
    /// transferred.
    BrepElementNotTransferred,
    /// Geometry subsets of a `BrepArray` prim that are not `face` or `brep`
    /// subsets binding a material were not transferred.
    BrepSubsetNotTransferred,
    /// A sketch element that does not satisfy its schema, or has no neutral
    /// form, was not transferred.
    SketchElementRejected,
    /// Sketch properties with no neutral form were not transferred.
    SketchPropertyNotTransferred,
    /// A `CadParameter` prim that does not satisfy its schema was not
    /// transferred.
    ParameterRejected,
    /// A `CadFeature` or `CadConfiguration` prim that does not satisfy its
    /// schema, or names a record the stage does not hold, was not
    /// transferred.
    FeatureRejected,
    /// Feature or configuration properties that name records the stage
    /// does not hold were not transferred.
    FeaturePropertyNotTransferred,
    /// A joint whose bodies or frames do not resolve was not transferred.
    JointRejected,
    /// Joint properties with no neutral form were not transferred.
    JointPropertyNotTransferred,
    /// A `PhysX` property was not carried: its value or its target did not
    /// resolve.
    SolverPropertyNotTransferred,
    /// A `UsdPhysics` property of a rigid body, mass, or collider was not
    /// carried.
    PhysicsPropertyNotTransferred,
    /// The writer wrote coordinates as binary32 values.
    ExportVertexPrecision,
    /// A face with no tessellation and no NURBS surface was not written.
    ExportFaceNotWritten,
    /// A curve with no NURBS form was not written.
    ExportCurveNotWritten,
    /// Tessellation channels and texture assignments were not written.
    ExportChannelNotWritten,
    /// A material property with no `UsdPreviewSurface` input was not written.
    ExportAppearanceReduced,
    /// A texture image was written as a path that the output does not hold.
    ExportTextureExternal,
    /// B-rep topology was written as the body's tessellation.
    ExportTopologyTessellated,
    /// A sketch entity or constraint with no `CadSketch` form was not
    /// written.
    ExportSketchElementNotWritten,
    /// A design parameter property with no `CadParameter` form was not
    /// written.
    ExportParameterPropertyNotWritten,
    /// A feature or configuration with no `CadFeature` or
    /// `CadConfiguration` form was not written.
    ExportFeatureNotWritten,
    /// A body with faces has no `BrepArray` prim.
    ExportBrepNotWritten,
    /// A `BrepArray` prim has no trim curves.
    ExportBrepUntrimmed,
    /// Points, curves, or surfaces that no topology uses were not written.
    ExportCarrierNotWritten,
    /// An assembly joint or a joint drive with no `UsdPhysics` form was not
    /// written.
    ExportJointNotWritten,
    /// A solver property has no prim, attribute, or target to write.
    ExportSolverPropertyNotWritten,
}

impl UsdLossCode {
    /// Every code, in declaration order.
    #[cfg(test)]
    pub(crate) const ALL: &'static [UsdLossCode] = &[
        Self::SourceDialectUnverified,
        Self::CompositionSublayersNotComposed,
        Self::CompositionExternalArc,
        Self::CompositionLayerUnavailable,
        Self::CompositionTargetMissing,
        Self::CompositionVariantNotSelected,
        Self::CompositionOverUndefined,
        Self::CompositionCycle,
        Self::CompositionOverrideNotApplied,
        Self::PrimTypeNotTransferred,
        Self::PrimIntrinsicNotTransferred,
        Self::ApiSchemaNotTransferred,
        Self::PropertyNotTransferred,
        Self::AttributeValueMalformed,
        Self::AttributeAnimationNotTransferred,
        Self::MetadataNotTransferred,
        Self::CrateValueNotRead,
        Self::XformMalformed,
        Self::MeshSubdivisionNotApplied,
        Self::MeshFaceRejected,
        Self::MeshRejected,
        Self::MeshPrimvarNotTransferred,
        Self::MeshNormalsNotTransferred,
        Self::MeshSubsetNotTransferred,
        Self::CurvesRejected,
        Self::NurbsPatchRejected,
        Self::NurbsTrimNotTransferred,
        Self::NurbsRangeNotTransferred,
        Self::DisplayColorNotTransferred,
        Self::MaterialNotTransferred,
        Self::TextureUnlocated,
        Self::BrepRejected,
        Self::BrepElementNotTransferred,
        Self::BrepSubsetNotTransferred,
        Self::SketchElementRejected,
        Self::SketchPropertyNotTransferred,
        Self::ParameterRejected,
        Self::FeatureRejected,
        Self::FeaturePropertyNotTransferred,
        Self::JointRejected,
        Self::JointPropertyNotTransferred,
        Self::SolverPropertyNotTransferred,
        Self::PhysicsPropertyNotTransferred,
        Self::ExportVertexPrecision,
        Self::ExportFaceNotWritten,
        Self::ExportCurveNotWritten,
        Self::ExportChannelNotWritten,
        Self::ExportAppearanceReduced,
        Self::ExportTextureExternal,
        Self::ExportTopologyTessellated,
        Self::ExportSketchElementNotWritten,
        Self::ExportParameterPropertyNotWritten,
        Self::ExportFeatureNotWritten,
        Self::ExportBrepNotWritten,
        Self::ExportBrepUntrimmed,
        Self::ExportCarrierNotWritten,
        Self::ExportJointNotWritten,
        Self::ExportSolverPropertyNotWritten,
    ];

    /// The stable string identifier, severity, and shared category.
    const fn spec(self) -> (&'static str, Severity, LossTaxonomy) {
        use LossTaxonomy as T;
        use Severity::{Info, Warning};
        match self {
            Self::SourceDialectUnverified => (
                "source.dialect-unverified",
                Warning,
                T::SourceDialectUnverified,
            ),
            Self::CompositionSublayersNotComposed => (
                "composition.sublayers-not-composed",
                Warning,
                T::AssemblyComponentsExternal,
            ),
            Self::CompositionExternalArc => (
                "composition.external-arc",
                Info,
                T::AssemblyComponentsExternal,
            ),
            Self::CompositionLayerUnavailable => (
                "composition.layer-unavailable",
                Warning,
                T::AssemblyComponentsExternal,
            ),
            Self::CompositionTargetMissing => (
                "composition.target-missing",
                Warning,
                T::ReferenceGraphNotClosed,
            ),
            Self::CompositionVariantNotSelected => (
                "composition.variant-not-selected",
                Info,
                T::ParametricRecordOmitted,
            ),
            Self::CompositionOverUndefined => (
                "composition.over-undefined",
                Warning,
                T::ReferenceGraphNotClosed,
            ),
            Self::CompositionCycle => ("composition.cycle", Warning, T::ReferenceGraphNotClosed),
            Self::CompositionOverrideNotApplied => (
                "composition.override-not-applied",
                Warning,
                T::AssemblyPlacementsNotTransferred,
            ),
            Self::PrimTypeNotTransferred => (
                "prim.type-not-transferred",
                Info,
                T::UnsupportedObjectFamily,
            ),
            Self::PrimIntrinsicNotTransferred => (
                "prim.intrinsic-not-transferred",
                Warning,
                T::GeometryNotTransferred,
            ),
            Self::ApiSchemaNotTransferred => (
                "prim.api-schema-not-transferred",
                Info,
                T::AttributesNotTransferred,
            ),
            Self::PropertyNotTransferred => (
                "attribute.property-not-transferred",
                Info,
                T::AttributesNotTransferred,
            ),
            Self::AttributeValueMalformed => {
                ("attribute.value-malformed", Warning, T::DecodeDiagnostic)
            }
            Self::AttributeAnimationNotTransferred => (
                "attribute.animation-not-transferred",
                Info,
                T::AttributesNotTransferred,
            ),
            Self::MetadataNotTransferred => {
                ("metadata.not-transferred", Info, T::MetadataNotTransferred)
            }
            Self::CrateValueNotRead => {
                ("crate.value-not-read", Warning, T::AttributesNotTransferred)
            }
            Self::XformMalformed => (
                "xform.malformed",
                Warning,
                T::AssemblyPlacementsNotTransferred,
            ),
            Self::MeshSubdivisionNotApplied => {
                ("mesh.subdivision-not-applied", Info, T::SubdOmitted)
            }
            Self::MeshFaceRejected => ("mesh.face-rejected", Warning, T::GeometryNotTransferred),
            Self::MeshRejected => ("mesh.rejected", Warning, T::GeometryNotTransferred),
            Self::MeshPrimvarNotTransferred => (
                "mesh.primvar-not-transferred",
                Info,
                T::AttributesNotTransferred,
            ),
            Self::MeshNormalsNotTransferred => (
                "mesh.normals-not-transferred",
                Info,
                T::AttributesNotTransferred,
            ),
            Self::MeshSubsetNotTransferred => (
                "mesh.subset-not-transferred",
                Info,
                T::AttributesNotTransferred,
            ),
            Self::CurvesRejected => ("curves.rejected", Warning, T::GeometryNotTransferred),
            Self::NurbsPatchRejected => {
                ("nurbs.patch-rejected", Warning, T::GeometryNotTransferred)
            }
            Self::NurbsTrimNotTransferred => {
                ("nurbs.trim-not-transferred", Warning, T::PcurveOmitted)
            }
            Self::NurbsRangeNotTransferred => (
                "nurbs.range-not-transferred",
                Warning,
                T::GeometryNotTransferred,
            ),
            Self::DisplayColorNotTransferred => (
                "appearance.display-color-not-transferred",
                Info,
                T::AppearanceReduced,
            ),
            Self::MaterialNotTransferred => (
                "appearance.material-not-transferred",
                Info,
                T::MaterialNotTransferred,
            ),
            Self::TextureUnlocated => (
                "appearance.texture-unlocated",
                Info,
                T::ReferenceGraphNotClosed,
            ),
            Self::BrepRejected => ("brep.rejected", Warning, T::TopologyNotTransferred),
            Self::BrepElementNotTransferred => (
                "brep.element-not-transferred",
                Info,
                T::TopologyNotTransferred,
            ),
            Self::BrepSubsetNotTransferred => (
                "brep.subset-not-transferred",
                Info,
                T::MaterialNotTransferred,
            ),
            Self::SketchElementRejected => (
                "sketch.element-rejected",
                Warning,
                T::ParametricRecordOmitted,
            ),
            Self::ParameterRejected => ("parameter.rejected", Warning, T::ParametricRecordOmitted),
            Self::FeatureRejected => ("feature.rejected", Warning, T::ParametricRecordOmitted),
            Self::FeaturePropertyNotTransferred => (
                "feature.property-not-transferred",
                Warning,
                T::ParametricRecordOmitted,
            ),
            Self::SketchPropertyNotTransferred => (
                "sketch.property-not-transferred",
                Info,
                T::ParametricRecordOmitted,
            ),
            Self::JointRejected => (
                "physics.joint-rejected",
                Warning,
                T::AssemblyPlacementsNotTransferred,
            ),
            Self::JointPropertyNotTransferred => (
                "physics.joint-property-not-transferred",
                Info,
                T::PhysicsNotTransferred,
            ),
            Self::SolverPropertyNotTransferred => (
                "physics.solver-property-not-transferred",
                Info,
                T::PhysicsNotTransferred,
            ),
            Self::PhysicsPropertyNotTransferred => (
                "physics.property-not-transferred",
                Info,
                T::PhysicsNotTransferred,
            ),
            Self::ExportVertexPrecision => {
                ("export.vertex-precision", Info, T::MeshVertexPrecision)
            }
            Self::ExportFaceNotWritten => (
                "export.face-not-written",
                Warning,
                T::GeometryNotTransferred,
            ),
            Self::ExportCurveNotWritten => (
                "export.curve-not-written",
                Warning,
                T::GeometryNotTransferred,
            ),
            Self::ExportChannelNotWritten => (
                "export.channel-not-written",
                Info,
                T::AttributesNotTransferred,
            ),
            Self::ExportTextureExternal => {
                ("export.texture-external", Info, T::ReferenceGraphNotClosed)
            }
            Self::ExportAppearanceReduced => {
                ("export.appearance-reduced", Info, T::AppearanceReduced)
            }
            Self::ExportTopologyTessellated => (
                "export.topology-tessellated",
                Warning,
                T::TopologyNotTransferred,
            ),
            Self::ExportSketchElementNotWritten => (
                "export.sketch-element-not-written",
                Warning,
                T::ParametricRecordOmitted,
            ),
            Self::ExportParameterPropertyNotWritten => (
                "export.parameter-property-not-written",
                Warning,
                T::ParametricRecordOmitted,
            ),
            Self::ExportFeatureNotWritten => (
                "export.feature-not-written",
                Warning,
                T::ParametricRecordOmitted,
            ),
            Self::ExportBrepNotWritten => (
                "export.brep-not-written",
                Warning,
                T::TopologyNotTransferred,
            ),
            Self::ExportBrepUntrimmed => ("export.brep-untrimmed", Info, T::GeometryNotTransferred),
            Self::ExportCarrierNotWritten => (
                "export.carrier-not-written",
                Warning,
                T::GeometryNotTransferred,
            ),
            Self::ExportJointNotWritten => (
                "export.joint-not-written",
                Warning,
                T::AssemblyPlacementsNotTransferred,
            ),
            Self::ExportSolverPropertyNotWritten => (
                "export.solver-property-not-written",
                Warning,
                T::PhysicsNotTransferred,
            ),
        }
    }

    /// The stable string identifier. This is the gating contract.
    #[cfg(test)]
    pub(crate) const fn code(self) -> &'static str {
        self.spec().0
    }

    /// Namespaced [`LossKind`] for this local code.
    pub(crate) fn kind(self) -> LossKind {
        let (code, _, taxonomy) = self.spec();
        LossKind::namespaced(NAMESPACE, code, taxonomy)
    }

    /// Builds a [`LossNote`] for this code with the per-instance message.
    pub(crate) fn note(self, message: impl Into<String>) -> LossNote {
        LossNote::new(self.kind(), message).with_severity(self.spec().1)
    }
}

#[cfg(test)]
mod tests;
