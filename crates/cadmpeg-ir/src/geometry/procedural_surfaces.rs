// SPDX-License-Identifier: Apache-2.0
//! Procedural surface carriers, spline parameters, and surface definitions.

use super::blends::RollingBallJetStations;
use super::helix::HelixSurfaceConstruction;
use super::legacy_cache::{
    clear_revision_form_tolerance, write_revision_form_tolerance, CacheContractError, FitTolerance,
    LegacyCache, LegacyCacheSlot, RevisionCacheWrite, ToleranceWrite, NO_LEGACY_SLOT,
};
use super::revision_forms::{
    RevisionCacheForm, RevisionCompoundLoftConstruction, RevisionG2BlendConstruction,
    RevisionSurfaceForm, RevisionSurfaceParameterization, VariableBlendCache,
};
use super::tsplines::TSplineSurfaceConstruction;
use super::OffsetExtension;
use crate::ids::{CurveId, PcurveId, SurfaceId, UnknownId};
use crate::math::Vector3;
use crate::scalar::FiniteReal;
use crate::transform::Transform;
#[cfg(feature = "schema")]
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

/// Parameter fields carried by exact and loft spline-surface constructions.
// A source states raw scalars; a `LoftSurfacePayload` holds the admitted
// fields, whose scalars are `FiniteReal` values.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, cadmpeg_derive::StructuralOrd)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(tag = "kind", rename_all = "snake_case")]
#[serde(deny_unknown_fields)]
pub enum SplineSurfaceParameters<R = f64> {
    /// Ordered semantic U and V intervals in the legacy layout.
    OrderedRanges {
        /// Ordered U and V intervals.
        ranges: [[R; 2]; 2],
    },
    /// Two parameter intervals in a revision-gated layout, each stored as an
    /// ordered `[lo, hi]` pair of optional bounds. For exact and t-spline
    /// surfaces these are the surface's unextended (pre-extension) parameter
    /// ranges; for loft surfaces they are wrap ranges, where a reversed pair
    /// (`lo > hi`) encodes an empty interval (no wrap). `None` is a false
    /// bound-presence flag.
    RevisionRanges {
        /// Two parameter intervals in serialized field order.
        intervals: [[Option<R>; 2]; 2],
    },
}

/// Mutually exclusive legacy and revision-gated exact-spline layouts.
// A source states raw scalars; an `ExactSurfacePayload` holds the admitted
// spline, whose scalars are `FiniteReal` values.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, cadmpeg_derive::StructuralOrd)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
#[cfg_attr(feature = "schema", schemars(bound = "R: JsonSchema + Serialize"))]
pub enum ExactSpline<R = f64> {
    /// Legacy solved-cache layout with ordered U/V ranges.
    Legacy {
        /// Ordered U and V parameter ranges.
        ranges: [[R; 2]; 2],
        /// Native ASM extension integer following the ranges.
        extension: i64,
        /// Solved-cache fit contract this layout states itself.
        #[serde(
            default,
            skip_serializing_if = "Option::is_none",
            deserialize_with = "deserialize_cache"
        )]
        cache: Option<LegacyCache>,
    },
    /// Revision-gated layout with optional interval bounds and shared form.
    Revision {
        /// Two optional-bound parameter intervals in wire order.
        intervals: [[Option<R>; 2]; 2],
        /// Native ASM extension enum following the intervals.
        extension: i64,
        /// Required revision-gated form.
        form: Box<RevisionSurfaceForm<Vec<bool>, R>>,
    },
}

/// Neutral semantics for a procedural surface.
#[derive(Debug, Clone, PartialEq, Serialize, cadmpeg_derive::StructuralOrd)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum ProceduralSurfaceDefinition {
    /// Exact native NURBS surface with retained parameter fields.
    Exact(super::surface_payloads::ExactSurfacePayload),
    /// Ordered native compound of a solved surface and component surfaces.
    Compound(super::surface_payloads::CompoundSurfacePayload),
    /// Exact rectangular restriction of an embedded support surface.
    SubSurface(super::surface_payloads::SubSurfaceConstruction),
    /// Taper of a support surface around a reference curve.
    Taper(super::surface_payloads::TaperSurfaceConstruction),
    /// Native loft defined by two section graphs and closure contracts.
    Loft(super::surface_payloads::LoftSurfacePayload),
    /// Native compound-loft construction.
    CompoundLoft(super::surface_payloads::CompoundLoftSurfacePayload),
    /// Revision-gated compound-loft construction.
    RevisionCompoundLoft {
        /// Complete native revision-gated compound-loft graph.
        construction: Box<RevisionCompoundLoftConstruction>,
    },
    /// Native scaled compound-loft construction.
    ScaledCompoundLoft(super::surface_payloads::ScaledCompoundLoftSurfacePayload),
    /// Native skinned spline surface.
    Skin(super::surface_payloads::SkinSurfacePayload),
    /// Native surface defined by recursive law formulas.
    Law(super::surface_payloads::LawSurfacePayload),
    /// Native curve-network spline surface.
    Net(super::surface_payloads::NetSurfacePayload),
    /// Native curvature-continuous two-sided blend.
    G2Blend(super::surface_payloads::G2BlendSurfacePayload),
    /// Revision-gated curvature-continuous blend in the variable-blend side
    /// layout.
    RevisionG2Blend {
        /// Complete native revision-gated G2 construction graph.
        construction: Box<RevisionG2BlendConstruction>,
    },
    /// Native variable-radius two-sided blend.
    VariableBlend(super::surface_payloads::VariableBlendSurfacePayload),
    /// Native vertex-blend patch.
    VertexBlend(super::surface_payloads::VertexBlendSurfacePayload),
    /// Translation of a directrix along a direction.
    Extrusion(super::surface_payloads::ExtrusionSurfaceConstruction),
    /// Unbounded linear sweep of a directrix.
    LinearSweep(super::surface_payloads::LinearSweepSurfaceConstruction),
    /// Revolution of a directrix about an axis.
    Revolution(super::surface_payloads::RevolutionSurfaceConstruction),
    /// Full revolution of a directrix about an axis.
    AxisRevolution(super::surface_payloads::AxisRevolutionSurfaceConstruction),
    /// Sum of two ordered curves from a base point.
    Sum(super::surface_payloads::SumSurfaceConstruction),
    /// Sweep of a profile along a spine.
    Sweep(super::surface_payloads::SweepSurfacePayload),
    /// T-spline face with its shared subtransform program.
    TSpline {
        /// Complete native T-spline wrapper construction.
        construction: Box<TSplineSurfaceConstruction>,
    },
    /// Surface generated along an inline circular or linear helix path.
    Helix {
        /// Complete native helix-surface construction.
        construction: Box<HelixSurfaceConstruction>,
    },
    /// Native deformable spline surface.
    Deformable(super::surface_payloads::DeformableSurfacePayload),
    /// Offset from a support surface.
    Offset(super::surface_payloads::OffsetSurfaceConstruction),
    /// Rectangular parameter sub-range of a support surface.
    Subset(super::surface_payloads::SubsetSurfaceConstruction),
    /// Affine replica of a surface carrier, retaining the parent surface
    /// construction and its parameter domain.
    Replica {
        /// Surface being replicated.
        source: SurfaceId,
        /// Affine map from the parent surface coordinates to this surface.
        transform: Transform,
    },
    /// Parallel offset from a support surface.
    ParallelOffset(super::surface_payloads::ParallelOffsetSurfaceConstruction),
    /// Self-intersecting torus with an explicitly selected outer or inner sheet.
    DegenerateTorus {
        /// Whether the outer sheet is selected at the self-intersection.
        select_outer: bool,
    },
    /// Surface domain bounded by ordered curves on a supporting surface.
    CurveBounded {
        /// Supporting surface whose parameterization defines the domain.
        support: SurfaceId,
        /// Boundary curves on the support.
        boundaries: Vec<CurveId>,
        /// Parameter-space carriers used by surface-curve boundary segments.
        ///
        /// These are separate from `boundaries`: one STEP surface curve can
        /// carry both a model-space curve and pcurves on the support surface.
        boundary_pcurves: Vec<PcurveId>,
        /// Whether the support's natural outer boundary is implicit.
        implicit_outer: bool,
    },
    /// Ruled surface joining two directrices.
    Ruled {
        /// First bounding curve of the ruled surface.
        first: CurveId,
        /// Second bounding curve of the ruled surface.
        second: CurveId,
        /// Solved-cache fit contract this construction states itself.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        cache: Option<LegacyCache>,
    },
    /// Rolling-ball or law-driven blend between two support surfaces.
    Blend(super::surface_payloads::BlendSurfacePayload),
    /// Rolling-ball surface defined by aligned quintic value/derivative jets.
    RollingBallJet(RollingBallJetStations),
    /// Preserved construction without a neutral interpretation.
    Unknown {
        /// Reference to the preserved raw source record, when retained.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        record: Option<UnknownId>,
        /// Solved-cache fit contract this construction states itself.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        cache: Option<LegacyCache>,
    },
}

#[derive(Deserialize)]
#[serde(
    remote = "ProceduralSurfaceDefinition",
    tag = "kind",
    rename_all = "snake_case",
    deny_unknown_fields
)]
pub(super) enum ProceduralSurfaceDefinitionWire {
    Exact(super::surface_payloads::ExactSurfacePayload),
    Compound(super::surface_payloads::CompoundSurfacePayload),
    SubSurface(super::surface_payloads::SubSurfaceConstruction),
    Taper(super::surface_payloads::TaperSurfaceConstruction),
    Loft(super::surface_payloads::LoftSurfacePayload),
    CompoundLoft(super::surface_payloads::CompoundLoftSurfacePayload),
    RevisionCompoundLoft {
        construction: Box<RevisionCompoundLoftConstruction>,
    },
    ScaledCompoundLoft(super::surface_payloads::ScaledCompoundLoftSurfacePayload),
    Skin(super::surface_payloads::SkinSurfacePayload),
    Law(super::surface_payloads::LawSurfacePayload),
    Net(super::surface_payloads::NetSurfacePayload),
    G2Blend(super::surface_payloads::G2BlendSurfacePayload),
    RevisionG2Blend {
        construction: Box<RevisionG2BlendConstruction>,
    },
    VariableBlend(super::surface_payloads::VariableBlendSurfacePayload),
    VertexBlend(super::surface_payloads::VertexBlendSurfacePayload),
    Extrusion(super::surface_payloads::ExtrusionSurfaceConstruction),
    LinearSweep(super::surface_payloads::LinearSweepSurfaceConstruction),
    Revolution(super::surface_payloads::RevolutionSurfaceConstruction),
    AxisRevolution(super::surface_payloads::AxisRevolutionSurfaceConstruction),
    Sum(super::surface_payloads::SumSurfaceConstruction),
    Sweep(super::surface_payloads::SweepSurfacePayload),
    TSpline {
        construction: Box<TSplineSurfaceConstruction>,
    },
    Helix {
        construction: Box<HelixSurfaceConstruction>,
    },
    Deformable(super::surface_payloads::DeformableSurfacePayload),
    Offset(super::surface_payloads::OffsetSurfaceConstruction),
    Subset(super::surface_payloads::SubsetSurfaceConstruction),
    Replica {
        source: SurfaceId,
        transform: Transform,
    },
    ParallelOffset(super::surface_payloads::ParallelOffsetSurfaceConstruction),
    DegenerateTorus {
        select_outer: bool,
    },
    CurveBounded {
        support: SurfaceId,
        boundaries: Vec<CurveId>,
        boundary_pcurves: Vec<PcurveId>,
        implicit_outer: bool,
    },
    Ruled {
        first: CurveId,
        second: CurveId,
        /// Solved-cache fit contract this construction states itself.
        #[serde(
            default,
            skip_serializing_if = "Option::is_none",
            deserialize_with = "deserialize_cache"
        )]
        cache: Option<LegacyCache>,
    },
    Blend(super::surface_payloads::BlendSurfacePayload),
    RollingBallJet(RollingBallJetStations),
    Unknown {
        #[serde(
            default,
            skip_serializing_if = "Option::is_none",
            deserialize_with = "deserialize_record"
        )]
        record: Option<UnknownId>,
        /// Solved-cache fit contract this construction states itself.
        #[serde(
            default,
            skip_serializing_if = "Option::is_none",
            deserialize_with = "deserialize_cache"
        )]
        cache: Option<LegacyCache>,
    },
}

impl<'de> Deserialize<'de> for ProceduralSurfaceDefinition {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        ProceduralSurfaceDefinitionWire::deserialize(deserializer)
    }
}

impl ProceduralSurfaceDefinition {
    pub(super) fn revision_cache(
        &self,
    ) -> Option<&RevisionCacheForm<RevisionSurfaceParameterization<FiniteReal>>> {
        match self {
            Self::Exact(payload) => payload.revision_cache(),
            Self::Taper(definition_payload) => {
                let revision_form = definition_payload.revision_form();
                revision_form.as_ref().map(|form| &form.cache)
            }
            Self::Extrusion(definition_payload) => {
                let revision_form = definition_payload.revision_form();
                revision_form.as_ref().map(|form| &form.cache)
            }
            Self::Revolution(definition_payload) => {
                let revision_form = definition_payload.revision_form();
                revision_form.as_ref().map(|form| &form.cache)
            }
            Self::Sum(payload) => payload.revision_form().map(|form| &form.cache),
            Self::Offset(payload) => match payload.extension() {
                OffsetExtension::Revision { form } => Some(&form.cache),
                OffsetExtension::Legacy { .. } => None,
            },
            Self::Loft(payload) => payload.revision_cache(),
            Self::RevisionCompoundLoft { construction } => Some(&construction.cache),
            Self::RevisionG2Blend { construction } => Some(&construction.cache),
            Self::Sweep(payload) => payload.revision_cache(),
            Self::TSpline { construction } => construction.cache.form().map(|form| &form.cache),
            Self::Deformable(payload) => payload.revision_cache(),
            Self::Blend(payload) => payload.revision_cache(),
            Self::Compound(_)
            | Self::SubSurface(_)
            | Self::CompoundLoft(_)
            | Self::ScaledCompoundLoft(_)
            | Self::Skin(_)
            | Self::Law(_)
            | Self::Net(_)
            | Self::G2Blend(_)
            | Self::VariableBlend(_)
            | Self::VertexBlend(_)
            | Self::LinearSweep(_)
            | Self::AxisRevolution(_)
            | Self::Helix { .. }
            | Self::Subset(_)
            | Self::Replica { .. }
            | Self::ParallelOffset(_)
            | Self::DegenerateTorus { .. }
            | Self::CurveBounded { .. }
            | Self::Ruled { .. }
            | Self::RollingBallJet(_)
            | Self::Unknown { .. } => None,
        }
    }

    /// Write the fit tolerance of the revision-gated solved cache.
    ///
    /// The narrow write route: the borrow of the cache form stays inside this
    /// method, so no construction lends its admitted interior for writing.
    pub(super) fn write_revision_fit_tolerance(
        &mut self,
        write: ToleranceWrite,
    ) -> RevisionCacheWrite {
        match self {
            Self::Exact(payload) => payload.write_revision_fit_tolerance(write),
            Self::Taper(payload) => payload.write_revision_fit_tolerance(write),
            Self::Extrusion(payload) => payload.write_revision_fit_tolerance(write),
            Self::Revolution(payload) => payload.write_revision_fit_tolerance(write),
            Self::Sum(payload) => payload.write_revision_fit_tolerance(write),
            Self::Offset(payload) => payload.write_revision_fit_tolerance(write),
            Self::Loft(payload) => payload.write_revision_fit_tolerance(write),
            Self::RevisionCompoundLoft { construction } => {
                construction.cache.write_fit_tolerance(write)
            }
            Self::RevisionG2Blend { construction } => construction.cache.write_fit_tolerance(write),
            Self::Sweep(payload) => payload.write_revision_fit_tolerance(write),
            Self::TSpline { construction } => write_revision_form_tolerance(
                construction.cache.form_mut().map(|form| &mut form.cache),
                write,
            ),
            Self::Deformable(payload) => payload.write_revision_fit_tolerance(write),
            Self::Blend(payload) => payload.write_revision_fit_tolerance(write),
            Self::Compound(_)
            | Self::SubSurface(_)
            | Self::CompoundLoft(_)
            | Self::ScaledCompoundLoft(_)
            | Self::Skin(_)
            | Self::Law(_)
            | Self::Net(_)
            | Self::G2Blend(_)
            | Self::VariableBlend(_)
            | Self::VertexBlend(_)
            | Self::LinearSweep(_)
            | Self::AxisRevolution(_)
            | Self::Helix { .. }
            | Self::Subset(_)
            | Self::Replica { .. }
            | Self::ParallelOffset(_)
            | Self::DegenerateTorus { .. }
            | Self::CurveBounded { .. }
            | Self::Ruled { .. }
            | Self::RollingBallJet(_)
            | Self::Unknown { .. } => RevisionCacheWrite::NoForm,
        }
    }

    /// Whether the construction owns a revision-gated cache form, which then
    /// states its solved-cache fit tolerance.
    #[must_use]
    pub fn owns_revision_cache(&self) -> bool {
        self.revision_cache().is_some() || matches!(self, Self::VariableBlend(..))
    }
}

impl ProceduralSurfaceDefinition {
    /// Solved-cache fit contract this construction states outside a
    /// revision-gated cache form.
    #[must_use]
    pub fn legacy_cache(&self) -> Option<LegacyCache> {
        match self {
            Self::Exact(payload) => payload.legacy_cache(),
            Self::Compound(payload) => payload.legacy_cache(),
            Self::Taper(payload) => payload.legacy_cache(),
            Self::Loft(payload) => payload.legacy_cache(),
            Self::CompoundLoft(payload) => payload.legacy_cache(),
            Self::ScaledCompoundLoft(payload) => payload.legacy_cache(),
            Self::Skin(payload) => payload.legacy_cache(),
            Self::Law(payload) => payload.legacy_cache(),
            Self::Net(payload) => payload.legacy_cache(),
            Self::G2Blend(payload) => payload.legacy_cache(),
            Self::Extrusion(payload) => payload.legacy_cache(),
            Self::Revolution(payload) => payload.legacy_cache(),
            Self::Sum(payload) => payload.legacy_cache(),
            Self::Sweep(payload) => payload.legacy_cache(),
            Self::TSpline { construction } => construction.legacy_cache(),
            Self::Deformable(payload) => payload.legacy_cache(),
            Self::Offset(payload) => payload.legacy_cache(),
            Self::Subset(payload) => payload.legacy_cache(),
            Self::Blend(payload) => payload.legacy_cache(),
            Self::Ruled { cache, .. } | Self::Unknown { cache, .. } => *cache,
            Self::SubSurface(_)
            | Self::RevisionCompoundLoft { .. }
            | Self::RevisionG2Blend { .. }
            | Self::VariableBlend(_)
            | Self::VertexBlend(_)
            | Self::LinearSweep(_)
            | Self::AxisRevolution(_)
            | Self::Helix { .. }
            | Self::Replica { .. }
            | Self::ParallelOffset(_)
            | Self::DegenerateTorus { .. }
            | Self::CurveBounded { .. }
            | Self::RollingBallJet(_) => None,
        }
    }

    /// Mutable legacy solved-cache slot of this construction, absent when the
    /// layout states none.
    ///
    /// The one write route to the legacy cache: both the setter and the raise
    /// go through it, so a construction cannot answer them differently.
    pub(super) fn legacy_cache_slot_mut(&mut self) -> Option<LegacyCacheSlot<'_>> {
        match self {
            Self::Exact(payload) => payload.legacy_cache_slot_mut(),
            Self::Compound(payload) => Some(payload.legacy_cache_slot_mut()),
            Self::Taper(payload) => payload.legacy_cache_slot_mut(),
            Self::Loft(payload) => payload.legacy_cache_slot_mut(),
            Self::CompoundLoft(payload) => Some(payload.legacy_cache_slot_mut()),
            Self::ScaledCompoundLoft(payload) => Some(payload.legacy_cache_slot_mut()),
            Self::Skin(payload) => Some(payload.legacy_cache_slot_mut()),
            Self::Law(payload) => payload.legacy_cache_slot_mut(),
            Self::Net(payload) => Some(payload.legacy_cache_slot_mut()),
            Self::G2Blend(payload) => Some(payload.legacy_cache_slot_mut()),
            Self::Extrusion(payload) => payload.legacy_cache_slot_mut(),
            Self::Revolution(payload) => payload.legacy_cache_slot_mut(),
            Self::Sum(payload) => payload.legacy_cache_slot_mut(),
            Self::Sweep(payload) => payload.legacy_cache_slot_mut(),
            Self::TSpline { construction } => construction.legacy_cache_slot_mut(),
            Self::Deformable(payload) => payload.legacy_cache_slot_mut(),
            Self::Offset(payload) => payload.legacy_cache_slot_mut(),
            Self::Subset(payload) => Some(payload.legacy_cache_slot_mut()),
            Self::Blend(payload) => payload.legacy_cache_slot_mut(),
            Self::Ruled { cache, .. } | Self::Unknown { cache, .. } => {
                Some(LegacyCacheSlot::Optional(cache))
            }
            Self::SubSurface(_)
            | Self::RevisionCompoundLoft { .. }
            | Self::RevisionG2Blend { .. }
            | Self::VariableBlend(_)
            | Self::VertexBlend(_)
            | Self::LinearSweep(_)
            | Self::AxisRevolution(_)
            | Self::Helix { .. }
            | Self::Replica { .. }
            | Self::ParallelOffset(_)
            | Self::DegenerateTorus { .. }
            | Self::CurveBounded { .. }
            | Self::RollingBallJet(_) => None,
        }
    }

    /// State or clear the legacy solved-cache fit contract.
    ///
    /// Fallible in both directions, because a surface construction can hold a
    /// cache it cannot lose. A layout that states none refuses `Some`, and a
    /// `LawSurfaceTail::Full` tail, whose `Required` slot always states a
    /// tolerance, refuses `None`.
    pub fn set_legacy_cache(
        &mut self,
        cache: Option<LegacyCache>,
    ) -> Result<(), CacheContractError> {
        match self.legacy_cache_slot_mut() {
            Some(mut slot) => slot.set(cache),
            None => cache.map_or(Ok(()), |_| Err(CacheContractError::Layout(NO_LEGACY_SLOT))),
        }
    }

    /// Effective fit tolerance of the solved cache: a revision-gated form
    /// states it in its cache form, a legacy layout in the cache it holds.
    #[must_use]
    pub fn cache_fit_tolerance(&self) -> Option<FitTolerance> {
        if let Self::VariableBlend(payload) = self {
            return payload.construction().cache.fit_tolerance();
        }
        match self.revision_cache() {
            Some(RevisionCacheForm::SolvedCache { fit_tolerance }) => Some(*fit_tolerance),
            Some(RevisionCacheForm::Parameterization(_)) => None,
            None => self.legacy_cache().map(|cache| cache.fit_tolerance),
        }
    }

    /// Change the effective fit tolerance without permitting a parameterized
    /// cache to acquire one or a solved cache to lose it.
    pub fn set_cache_fit_tolerance(
        &mut self,
        value: Option<FitTolerance>,
    ) -> Result<(), CacheContractError> {
        if let Self::VariableBlend(payload) = self {
            return payload.write_cache_fit_tolerance(value);
        }
        if self.owns_revision_cache() {
            return match value {
                Some(value) => self
                    .write_revision_fit_tolerance(ToleranceWrite::State(value))
                    .into_set_result(),
                None => clear_revision_form_tolerance(self.revision_cache()),
            };
        }
        self.set_legacy_cache(value.map(LegacyCache::new))
    }

    /// Scale only the fit tolerance already carried by this construction.
    pub(super) fn scale_cache_fit_tolerance(
        &mut self,
        scale: crate::scalar::PositiveReal,
    ) -> Result<(), CacheContractError> {
        if let Self::VariableBlend(payload) = self {
            return payload.scale_cache_fit_tolerance(scale);
        }
        match self.write_revision_fit_tolerance(ToleranceWrite::Scale(scale)) {
            RevisionCacheWrite::Written | RevisionCacheWrite::Parameterized => Ok(()),
            RevisionCacheWrite::NoForm => self
                .legacy_cache_slot_mut()
                .map_or(Ok(()), |mut slot| slot.scale_fit_tolerance(scale)),
            RevisionCacheWrite::InvalidValue(value) => {
                Err(CacheContractError::InvalidValue { value })
            }
        }
    }
}

pub(super) fn set_variable_blend_cache(
    cache: &mut VariableBlendCache<FiniteReal>,
    value: Option<FitTolerance>,
) -> Result<(), CacheContractError> {
    match (cache, value) {
        (VariableBlendCache::Parameterization { .. }, Some(_)) => {
            Err(CacheContractError::Parameterized)
        }
        (VariableBlendCache::Parameterization { .. } | VariableBlendCache::Stale {}, None) => {
            Ok(())
        }
        (VariableBlendCache::Current { fit_tolerance, .. }, Some(value)) => {
            *fit_tolerance = value;
            Ok(())
        }
        (VariableBlendCache::Current { .. }, None) => Err(CacheContractError::MissingSolved),
        (VariableBlendCache::Stale {}, Some(_)) => Err(CacheContractError::Layout(
            "a stale variable-blend cache states no fit tolerance",
        )),
    }
}

/// Subtype-specific tail of a native taper spline surface.
// A source states raw values; a `TaperSurfaceConstruction` holds the admitted
// tail, whose scalars and draft vector are checked.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, cadmpeg_derive::StructuralOrd)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(tag = "kind", rename_all = "snake_case")]
#[serde(deny_unknown_fields)]
pub enum TaperSurfaceKind<R = f64, V = Vector3> {
    /// Standard taper without a subtype-specific tail.
    Standard {},
    /// Orthogonal taper with a native sense flag.
    Orthogonal {
        /// Native orientation sense.
        sense: bool,
    },
    /// Edge taper with a model-space draft vector.
    Edge {
        /// Native draft vector.
        draft: V,
    },
    /// Shadow taper with a pre-factored draft angle.
    Shadow {
        /// Native draft vector.
        draft: V,
        /// Stored draft-angle sine.
        sine: R,
        /// Stored draft-angle cosine.
        cosine: R,
    },
    /// Ruled taper with a pre-factored angle and factor.
    Ruled {
        /// Native draft vector.
        draft: V,
        /// Stored draft-angle sine.
        sine: R,
        /// Stored draft-angle cosine.
        cosine: R,
        /// Native ruled-taper factor.
        factor: R,
    },
    /// Swept taper with a pre-factored draft angle.
    Swept {
        /// Native draft vector.
        draft: V,
        /// Stored draft-angle sine.
        sine: R,
        /// Stored draft-angle cosine.
        cosine: R,
    },
}

cadmpeg_core::named_optional_field!(deserialize_cache, LegacyCache, "cache");
cadmpeg_core::named_optional_field!(deserialize_record, UnknownId, "record");
