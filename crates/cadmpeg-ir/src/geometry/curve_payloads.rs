// SPDX-License-Identifier: Apache-2.0
//! Checked procedural curve payloads.

use super::{
    CacheContract, CurveOffsetDistanceLaw, IntcurveSupportSide, LegacyCache, ProjectionTail,
    SpringLayout, SpringPcurve, SpringSupport,
};
use super::{
    CacheFirstCurveForm, CurveOffsetRange, DeformableCurveData, DeformableCurveSource,
    IntcurveSupportContext, OffsetSide, ProceduralGeometryError, SilhouetteKind, VectorOffsetRoles,
};
use crate::features::{FinitePoint3, FiniteVector3};
use crate::ids::{CurveId, SurfaceId};
use crate::math::Vector3;
use crate::scalar::FiniteReal;
use crate::topology::ParameterInterval;
use crate::units::{FiniteVector, UnitVector3};
#[cfg(feature = "schema")]
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

fn admit_base_endpoint(value: Option<f64>) -> Result<Option<FiniteReal>, ProceduralGeometryError> {
    value
        .map(|value| {
            FiniteReal::new(value).ok_or(ProceduralGeometryError::Payload(
                "SurfaceOffset.base_endpoints is not finite",
            ))
        })
        .transpose()
}

const EPS_OFFSET_PLANE_NORMAL: f64 = 1.0e-10;

/// Admitted surface offset curve parameters.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[cfg_attr(
    feature = "schema",
    schemars(with = "SurfaceOffsetCurveConstructionWire")
)]
#[serde(try_from = "SurfaceOffsetCurveConstructionWire")]
pub struct SurfaceOffsetCurveConstruction {
    /// Shared first two support pairs.
    context: IntcurveSupportContext,
    /// Native boolean following the discontinuity arrays.
    discontinuity_flag: bool,
    /// Native U interval on the base surface.
    base_u_range: ParameterInterval,
    /// Native V interval on the base surface.
    base_v_range: ParameterInterval,
    /// Embedded base curve.
    base: CurveId,
    /// Native interval on `base`.
    base_range: ParameterInterval,
    /// Optional parameter endpoints following the embedded base curve in
    /// the cache-first layout.
    #[serde(default)]
    base_endpoints: [Option<FiniteReal>; 2],
    /// Cache contract: the cache-first shared-context fields, or the legacy
    /// solved-cache tolerance the context-first layout states instead.
    #[serde(default, skip_serializing_if = "CacheContract::is_bare_legacy")]
    cache: CacheContract<CacheFirstCurveForm<FiniteReal>>,
    /// Signed model-space offset distance.
    distance: FiniteReal,
    /// Native unscaled parameter shift.
    shift: FiniteReal,
    /// Native unscaled parameter scale.
    scale: FiniteReal,
}

#[derive(Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(deny_unknown_fields)]
struct SurfaceOffsetCurveConstructionWire {
    /// Shared first two support pairs.
    context: IntcurveSupportContext,
    /// Native boolean following the discontinuity arrays.
    discontinuity_flag: bool,
    /// Native U interval on the base surface.
    base_u_range: [f64; 2],
    /// Native V interval on the base surface.
    base_v_range: [f64; 2],
    /// Embedded base curve.
    base: CurveId,
    /// Native interval on `base`.
    base_range: [f64; 2],
    /// Optional parameter endpoints following the embedded base curve in
    /// the cache-first layout.
    #[serde(default)]
    base_endpoints: [Option<f64>; 2],
    /// Cache contract: the cache-first shared-context fields, or the legacy
    /// solved-cache tolerance the context-first layout states instead.
    #[serde(default)]
    cache: CacheContract<CacheFirstCurveForm>,
    /// Signed model-space offset distance.
    distance: f64,
    /// Native unscaled parameter shift.
    shift: f64,
    /// Native unscaled parameter scale.
    scale: f64,
}

impl SurfaceOffsetCurveConstruction {
    pub(super) fn write_revision_fit_tolerance(
        &mut self,
        value: super::FitTolerance,
        write: super::ToleranceWrite,
    ) -> super::RevisionCacheWrite {
        super::write_revision_form_tolerance(
            self.cache.form_mut().map(|form| &mut form.cache),
            value,
            write,
        )
    }

    /// Admit the construction parameters.
    pub fn try_new(
        context: IntcurveSupportContext,
        discontinuity_flag: bool,
        [base_u_range, base_v_range]: [[f64; 2]; 2],
        (base, base_range, base_endpoints): (CurveId, [f64; 2], [Option<f64>; 2]),
        cache: CacheContract<CacheFirstCurveForm>,
        distance: f64,
        [shift, scale]: [f64; 2],
    ) -> Result<Self, ProceduralGeometryError> {
        let cache = cache.admit_form(CacheFirstCurveForm::admit).ok_or(
            ProceduralGeometryError::Payload("SurfaceOffset cache-first form is not finite"),
        )?;
        Ok(Self {
            context,
            discontinuity_flag,
            base_u_range: ParameterInterval::new(base_u_range)
                .map_err(ProceduralGeometryError::Payload)?,
            base_v_range: ParameterInterval::new(base_v_range)
                .map_err(ProceduralGeometryError::Payload)?,
            base,
            base_range: ParameterInterval::new(base_range)
                .map_err(ProceduralGeometryError::Payload)?,
            base_endpoints: [
                admit_base_endpoint(base_endpoints[0])?,
                admit_base_endpoint(base_endpoints[1])?,
            ],
            cache,
            distance: FiniteReal::new(distance).ok_or(ProceduralGeometryError::Payload(
                "SurfaceOffset.distance is not finite",
            ))?,
            shift: FiniteReal::new(shift).ok_or(ProceduralGeometryError::Payload(
                "SurfaceOffset.shift is not finite",
            ))?,
            scale: FiniteReal::new(scale).ok_or(ProceduralGeometryError::Payload(
                "SurfaceOffset.scale is not finite",
            ))?,
        })
    }
    /// Return the context.
    pub fn context(&self) -> &IntcurveSupportContext {
        &self.context
    }
    /// Return the discontinuity flag.
    pub fn discontinuity_flag(&self) -> &bool {
        &self.discontinuity_flag
    }
    /// Return the base u range.
    pub fn base_u_range(&self) -> &ParameterInterval {
        &self.base_u_range
    }
    /// Return the base v range.
    pub fn base_v_range(&self) -> &ParameterInterval {
        &self.base_v_range
    }
    /// Return the base.
    pub fn base(&self) -> &CurveId {
        &self.base
    }
    /// Return the base range.
    pub fn base_range(&self) -> &ParameterInterval {
        &self.base_range
    }
    /// Return the base endpoints.
    pub fn base_endpoints(&self) -> [Option<FiniteReal>; 2] {
        self.base_endpoints
    }
    /// Return the cache first.
    pub const fn cache_first(&self) -> Option<&CacheFirstCurveForm<FiniteReal>> {
        self.cache.form()
    }
    /// Return the distance.
    pub fn distance(&self) -> FiniteReal {
        self.distance
    }
    /// Return the shift.
    pub fn shift(&self) -> FiniteReal {
        self.shift
    }
    /// Return the scale.
    pub fn scale(&self) -> FiniteReal {
        self.scale
    }
}

impl TryFrom<SurfaceOffsetCurveConstructionWire> for SurfaceOffsetCurveConstruction {
    type Error = ProceduralGeometryError;
    fn try_from(wire: SurfaceOffsetCurveConstructionWire) -> Result<Self, Self::Error> {
        Self::try_new(
            wire.context,
            wire.discontinuity_flag,
            [wire.base_u_range, wire.base_v_range],
            (wire.base, wire.base_range, wire.base_endpoints),
            wire.cache,
            wire.distance,
            [wire.shift, wire.scale],
        )
    }
}

/// Admitted deformable curve parameters.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[cfg_attr(feature = "schema", schemars(with = "DeformableCurveConstructionWire"))]
#[serde(try_from = "DeformableCurveConstructionWire")]
pub struct DeformableCurveConstruction {
    /// Shared cache-first support context.
    context: IntcurveSupportContext,
    /// Cache-first serializer fields surrounding the solved curve cache.
    cache_first: CacheFirstCurveForm<FiniteReal>,
    /// Curve being deformed or its unresolved native reference.
    source: DeformableCurveSource,
    /// Optional native bounds following the source curve.
    source_parameter_range: [Option<FiniteReal>; 2],
    /// Discriminator-specific deformation payload.
    data: DeformableCurveData<FiniteReal, FiniteVector3, FinitePoint3>,
}

#[derive(Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(deny_unknown_fields)]
struct DeformableCurveConstructionWire {
    /// Shared cache-first support context.
    context: IntcurveSupportContext,
    /// Cache-first serializer fields surrounding the solved curve cache.
    cache_first: CacheFirstCurveForm,
    /// Curve being deformed or its unresolved native reference.
    source: DeformableCurveSource,
    /// Optional native bounds following the source curve.
    source_parameter_range: [Option<f64>; 2],
    /// Discriminator-specific deformation payload.
    data: DeformableCurveData,
}

impl DeformableCurveConstruction {
    pub(super) fn write_revision_fit_tolerance(
        &mut self,
        value: super::FitTolerance,
        write: super::ToleranceWrite,
    ) -> super::RevisionCacheWrite {
        super::write_revision_form_tolerance(Some(&mut self.cache_first.cache), value, write)
    }

    /// Admit the construction parameters.
    pub fn try_new(
        context: IntcurveSupportContext,
        cache_first: CacheFirstCurveForm,
        source: DeformableCurveSource,
        source_parameter_range: [Option<f64>; 2],
        data: DeformableCurveData,
    ) -> Result<Self, ProceduralGeometryError> {
        let (Some(data), Some(cache_first)) = (data.admit(), cache_first.admit()) else {
            return Err(ProceduralGeometryError::Payload(
                "deformable curve payload is not finite",
            ));
        };

        Ok(Self {
            context,
            cache_first,
            source,
            source_parameter_range: [
                source_parameter_range[0]
                    .map(|v| {
                        FiniteReal::new(v).ok_or(ProceduralGeometryError::Payload(
                            "Deformable.source_parameter_range is not finite",
                        ))
                    })
                    .transpose()?,
                source_parameter_range[1]
                    .map(|v| {
                        FiniteReal::new(v).ok_or(ProceduralGeometryError::Payload(
                            "Deformable.source_parameter_range is not finite",
                        ))
                    })
                    .transpose()?,
            ],
            data,
        })
    }
    /// Return the context.
    pub fn context(&self) -> &IntcurveSupportContext {
        &self.context
    }
    /// Return the cache first.
    pub fn cache_first(&self) -> &CacheFirstCurveForm<FiniteReal> {
        &self.cache_first
    }
    /// Return the source.
    pub fn source(&self) -> &DeformableCurveSource {
        &self.source
    }
    /// Return the source parameter range.
    pub fn source_parameter_range(&self) -> [Option<FiniteReal>; 2] {
        self.source_parameter_range
    }
    /// Return the data.
    pub fn data(&self) -> &DeformableCurveData<FiniteReal, FiniteVector3, FinitePoint3> {
        &self.data
    }
}

impl TryFrom<DeformableCurveConstructionWire> for DeformableCurveConstruction {
    type Error = ProceduralGeometryError;
    fn try_from(wire: DeformableCurveConstructionWire) -> Result<Self, Self::Error> {
        Self::try_new(
            wire.context,
            wire.cache_first,
            wire.source,
            wire.source_parameter_range,
            wire.data,
        )
    }
}

/// Admitted offset curve parameters.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[cfg_attr(feature = "schema", schemars(with = "OffsetCurveConstructionWire"))]
#[serde(try_from = "OffsetCurveConstructionWire")]
pub struct OffsetCurveConstruction {
    /// Curve this curve is offset from.
    source: CurveId,
    /// Signed offset distance, in document length units.
    distance: FiniteReal,
    /// Exclusive plane-normal or explicit-direction carrier.
    side: OffsetSide<FiniteVector3>,
    /// Retained parameter range, with its distance law when variable.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    range: Option<CurveOffsetRange<FiniteReal>>,
}

#[derive(Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(deny_unknown_fields)]
struct OffsetCurveConstructionWire {
    /// Curve this curve is offset from.
    source: CurveId,
    /// Signed offset distance, in document length units.
    distance: f64,
    /// Exclusive plane-normal or explicit-direction carrier.
    side: OffsetSide,
    /// Retained parameter range, with its distance law when variable.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_range"
    )]
    range: Option<CurveOffsetRange>,
}

impl OffsetCurveConstruction {
    /// Build a uniform offset along an explicit direction from admitted
    /// parts. The distance type states finiteness, a unit direction is finite
    /// and nonzero, which is the whole side condition of [`Self::try_new`],
    /// and a uniform range has no law, so nothing is checked.
    #[must_use]
    pub fn along_direction(
        source: CurveId,
        distance: FiniteReal,
        direction: crate::units::UnitVector3,
        support: Option<SurfaceId>,
        parameter_range: crate::topology::IncreasingParameterInterval,
    ) -> Self {
        Self {
            source,
            distance,
            side: crate::geometry::OffsetSide::Direction {
                direction: FiniteVector3::from(direction),
                support,
            },
            range: Some(crate::geometry::CurveOffsetRange::Uniform { parameter_range }),
        }
    }
    /// Admit the construction parameters. The range and law interval types
    /// state the interval contract; the side, the law distances and the
    /// distance are checked here.
    pub fn try_new(
        source: CurveId,
        distance: f64,
        side: OffsetSide,
        range: Option<CurveOffsetRange>,
    ) -> Result<Self, ProceduralGeometryError> {
        let side = side.admit().filter(|side| match side {
            OffsetSide::PlaneNormal { normal } => {
                (normal.norm() - 1.0).abs() <= EPS_OFFSET_PLANE_NORMAL
            }
            OffsetSide::Direction { direction, .. } => direction.norm() > 0.0,
        });
        let range = match range {
            None => Some(None),
            Some(range) => range
                .admit()
                .filter(|range| match range {
                    CurveOffsetRange::Variable {
                        distance_law:
                            CurveOffsetDistanceLaw::Coordinate {
                                function_parameter_scale,
                                ..
                            },
                        ..
                    } => function_parameter_scale.get() != 0.0,
                    CurveOffsetRange::Variable { .. } | CurveOffsetRange::Uniform { .. } => true,
                })
                .map(Some),
        };
        let (Some(side), Some(range)) = (side, range) else {
            return Err(ProceduralGeometryError::Payload(
                crate::geometry::INVALID_CURVE_OFFSET,
            ));
        };

        Ok(Self {
            source,
            distance: FiniteReal::new(distance).ok_or(ProceduralGeometryError::Payload(
                "Offset.distance is not finite",
            ))?,
            side,
            range,
        })
    }
    /// Return the source.
    pub fn source(&self) -> &CurveId {
        &self.source
    }
    /// Return the side.
    pub fn side(&self) -> &OffsetSide<FiniteVector3> {
        &self.side
    }
    /// Return the range.
    pub fn range(&self) -> &Option<CurveOffsetRange<FiniteReal>> {
        &self.range
    }
}

impl TryFrom<OffsetCurveConstructionWire> for OffsetCurveConstruction {
    type Error = ProceduralGeometryError;
    fn try_from(wire: OffsetCurveConstructionWire) -> Result<Self, Self::Error> {
        Self::try_new(wire.source, wire.distance, wire.side, wire.range)
    }
}

/// Admitted spatial offset curve parameters.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[cfg_attr(
    feature = "schema",
    schemars(with = "SpatialOffsetCurveConstructionWire")
)]
#[serde(try_from = "SpatialOffsetCurveConstructionWire")]
pub struct SpatialOffsetCurveConstruction {
    /// Curve being offset.
    source: CurveId,
    /// Signed offset distance.
    distance: FiniteReal,
    /// Reference direction controlling the offset frame.
    reference_direction: UnitVector3,
    /// Whether the source classifies the result as self-intersecting.
    self_intersect: Option<bool>,
}

#[derive(Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(deny_unknown_fields)]
struct SpatialOffsetCurveConstructionWire {
    /// Curve being offset.
    source: CurveId,
    /// Signed offset distance.
    distance: f64,
    /// Reference direction controlling the offset frame.
    reference_direction: Vector3,
    /// Whether the source classifies the result as self-intersecting.
    #[serde(deserialize_with = "cadmpeg_core::absent_key::nullable")]
    self_intersect: Option<bool>,
}

impl SpatialOffsetCurveConstruction {
    /// Admit the construction parameters.
    pub fn try_new(
        source: CurveId,
        distance: f64,
        reference_direction: Vector3,
        self_intersect: Option<bool>,
    ) -> Result<Self, ProceduralGeometryError> {
        let reference_direction = UnitVector3::new(reference_direction).ok_or(
            ProceduralGeometryError::Payload("invalid spatial curve offset"),
        )?;

        Ok(Self {
            source,
            distance: FiniteReal::new(distance).ok_or(ProceduralGeometryError::Payload(
                "SpatialOffset.distance is not finite",
            ))?,
            reference_direction,
            self_intersect,
        })
    }
    /// Return the source.
    pub fn source(&self) -> &CurveId {
        &self.source
    }
    /// Return the distance.
    pub fn distance(&self) -> FiniteReal {
        self.distance
    }
    /// Return the reference direction.
    pub fn reference_direction(&self) -> &UnitVector3 {
        &self.reference_direction
    }
    /// Return the self intersect.
    pub fn self_intersect(&self) -> &Option<bool> {
        &self.self_intersect
    }
}

impl TryFrom<SpatialOffsetCurveConstructionWire> for SpatialOffsetCurveConstruction {
    type Error = ProceduralGeometryError;
    fn try_from(wire: SpatialOffsetCurveConstructionWire) -> Result<Self, Self::Error> {
        Self::try_new(
            wire.source,
            wire.distance,
            wire.reference_direction,
            wire.self_intersect,
        )
    }
}

/// Admitted two sided offset curve parameters.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[cfg_attr(
    feature = "schema",
    schemars(with = "TwoSidedOffsetCurveConstructionWire")
)]
#[serde(try_from = "TwoSidedOffsetCurveConstructionWire")]
pub struct TwoSidedOffsetCurveConstruction {
    /// Shared surfaces, UV curves, interval, and discontinuity metadata.
    context: IntcurveSupportContext,
    /// Native boolean following the discontinuity arrays.
    discontinuity_flag: bool,
    /// Signed offset distance for each support side, in document length units.
    offsets: FiniteVector<2>,
    /// Solved-cache fit contract this construction states itself.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    cache: Option<LegacyCache>,
}

#[derive(Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(deny_unknown_fields)]
struct TwoSidedOffsetCurveConstructionWire {
    /// Shared surfaces, UV curves, interval, and discontinuity metadata.
    context: IntcurveSupportContext,
    /// Native boolean following the discontinuity arrays.
    discontinuity_flag: bool,
    /// Signed offset distance for each support side, in document length units.
    offsets: [f64; 2],
    /// Solved-cache fit contract this construction states itself.
    #[serde(default, deserialize_with = "deserialize_cache")]
    cache: Option<LegacyCache>,
}

impl TwoSidedOffsetCurveConstruction {
    /// Admit the construction parameters.
    pub fn try_new(
        context: IntcurveSupportContext,
        discontinuity_flag: bool,
        offsets: [f64; 2],
        cache: Option<LegacyCache>,
    ) -> Result<Self, ProceduralGeometryError> {
        Ok(Self {
            cache,
            context,
            discontinuity_flag,
            offsets: FiniteVector::new(offsets).ok_or(ProceduralGeometryError::Payload(
                "TwoSidedOffset.offsets is not finite",
            ))?,
        })
    }
    /// Return the context.
    pub fn context(&self) -> &IntcurveSupportContext {
        &self.context
    }
    /// Return the discontinuity flag.
    pub fn discontinuity_flag(&self) -> &bool {
        &self.discontinuity_flag
    }
    /// Return the offsets.
    pub fn offsets(&self) -> &FiniteVector<2> {
        &self.offsets
    }
}

impl TryFrom<TwoSidedOffsetCurveConstructionWire> for TwoSidedOffsetCurveConstruction {
    type Error = ProceduralGeometryError;
    fn try_from(wire: TwoSidedOffsetCurveConstructionWire) -> Result<Self, Self::Error> {
        Self::try_new(
            wire.context,
            wire.discontinuity_flag,
            wire.offsets,
            wire.cache,
        )
    }
}

/// Admitted vector offset curve parameters.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[cfg_attr(
    feature = "schema",
    schemars(with = "VectorOffsetCurveConstructionWire")
)]
#[serde(try_from = "VectorOffsetCurveConstructionWire")]
pub struct VectorOffsetCurveConstruction {
    /// Curve being offset.
    source: CurveId,
    /// Native parameter interval on the source curve.
    parameter_range: ParameterInterval,
    /// Model-space offset vector.
    offset: FiniteVector3,
    /// Integer codes attached to the two native roles.
    roles: VectorOffsetRoles,
    /// Solved-cache fit contract this construction states itself.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    cache: Option<LegacyCache>,
}

#[derive(Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(deny_unknown_fields)]
struct VectorOffsetCurveConstructionWire {
    /// Curve being offset.
    source: CurveId,
    /// Native parameter interval on the source curve.
    parameter_range: [f64; 2],
    /// Model-space offset vector.
    offset: Vector3,
    /// Integer codes attached to the two native roles.
    roles: VectorOffsetRoles,
    /// Solved-cache fit contract this construction states itself.
    #[serde(default, deserialize_with = "deserialize_cache")]
    cache: Option<LegacyCache>,
}

impl VectorOffsetCurveConstruction {
    /// Admit the construction parameters.
    pub fn try_new(
        source: CurveId,
        parameter_range: [f64; 2],
        offset: Vector3,
        roles: VectorOffsetRoles,
        cache: Option<LegacyCache>,
    ) -> Result<Self, ProceduralGeometryError> {
        Ok(Self {
            cache,
            source,
            parameter_range: ParameterInterval::new(parameter_range)
                .map_err(ProceduralGeometryError::Payload)?,
            offset: FiniteVector3::new(offset).ok_or(ProceduralGeometryError::Payload(
                "VectorOffset.offset is not finite",
            ))?,
            roles,
        })
    }
    /// Return the source.
    pub fn source(&self) -> &CurveId {
        &self.source
    }
    /// Return the parameter range.
    pub fn parameter_range(&self) -> &ParameterInterval {
        &self.parameter_range
    }
    /// Return the offset.
    pub fn offset(&self) -> &FiniteVector3 {
        &self.offset
    }
    /// Return the roles.
    pub fn roles(&self) -> &VectorOffsetRoles {
        &self.roles
    }
}

impl TryFrom<VectorOffsetCurveConstructionWire> for VectorOffsetCurveConstruction {
    type Error = ProceduralGeometryError;
    fn try_from(wire: VectorOffsetCurveConstructionWire) -> Result<Self, Self::Error> {
        Self::try_new(
            wire.source,
            wire.parameter_range,
            wire.offset,
            wire.roles,
            wire.cache,
        )
    }
}

/// Admitted subset curve parameters.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[cfg_attr(feature = "schema", schemars(with = "SubsetCurveConstructionWire"))]
#[serde(try_from = "SubsetCurveConstructionWire")]
pub struct SubsetCurveConstruction {
    /// Parent curve being restricted.
    source: CurveId,
    /// Native parameter interval retained from the parent.
    parameter_range: ParameterInterval,
    /// Whether the subset follows increasing parent parameters.
    #[serde(default = "crate::default_true")]
    sense: bool,
    /// Solved-cache fit contract this construction states itself.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_cache"
    )]
    cache: Option<LegacyCache>,
}

#[derive(Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(deny_unknown_fields)]
struct SubsetCurveConstructionWire {
    /// Parent curve being restricted.
    source: CurveId,
    /// Native parameter interval retained from the parent.
    parameter_range: [f64; 2],
    /// Whether the subset follows increasing parent parameters.
    #[serde(default = "crate::default_true")]
    sense: bool,
    /// Solved-cache fit contract this construction states itself.
    #[serde(default, deserialize_with = "deserialize_cache")]
    cache: Option<LegacyCache>,
}

impl SubsetCurveConstruction {
    /// Admit the construction parameters.
    pub fn try_new(
        source: CurveId,
        parameter_range: [f64; 2],
        sense: bool,
        cache: Option<LegacyCache>,
    ) -> Result<Self, ProceduralGeometryError> {
        Ok(Self {
            cache,
            source,
            parameter_range: ParameterInterval::new(parameter_range)
                .map_err(ProceduralGeometryError::Payload)?,
            sense,
        })
    }
    /// Return the source.
    pub fn source(&self) -> &CurveId {
        &self.source
    }
    /// Return the parameter range.
    pub fn parameter_range(&self) -> &ParameterInterval {
        &self.parameter_range
    }
    /// Return the sense.
    pub fn sense(&self) -> &bool {
        &self.sense
    }
}

impl TryFrom<SubsetCurveConstructionWire> for SubsetCurveConstruction {
    type Error = ProceduralGeometryError;
    fn try_from(wire: SubsetCurveConstructionWire) -> Result<Self, Self::Error> {
        Self::try_new(wire.source, wire.parameter_range, wire.sense, wire.cache)
    }
}
/// Admitted silhouette curve parameters.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[cfg_attr(feature = "schema", schemars(with = "SilhouetteCurveConstructionWire"))]
#[serde(try_from = "SilhouetteCurveConstructionWire")]
pub struct SilhouetteCurveConstruction {
    /// Shared first two support pairs.
    context: IntcurveSupportContext,
    /// Standard, parametric, or taper silhouette semantics.
    silhouette: SilhouetteKind,
    /// Surface whose silhouette is constructed.
    cast_surface: SurfaceId,
    /// Native model-space light direction.
    light_direction: FiniteVector3,
}

#[derive(Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(deny_unknown_fields)]
struct SilhouetteCurveConstructionWire {
    /// Shared first two support pairs.
    context: IntcurveSupportContext,
    /// Standard, parametric, or taper silhouette semantics.
    silhouette: SilhouetteKind,
    /// Surface whose silhouette is constructed.
    cast_surface: SurfaceId,
    /// Native model-space light direction.
    light_direction: Vector3,
}

impl SilhouetteCurveConstruction {
    /// Admit the construction parameters.
    pub fn try_new(
        context: IntcurveSupportContext,
        silhouette: SilhouetteKind,
        cast_surface: SurfaceId,
        light_direction: Vector3,
    ) -> Result<Self, ProceduralGeometryError> {
        const DEGENERATE: ProceduralGeometryError = ProceduralGeometryError::Payload(
            "silhouette fields are not finite or the light direction is degenerate",
        );
        let light_direction = FiniteVector3::new(light_direction).ok_or(DEGENERATE)?;
        if light_direction.as_raw().norm() <= f64::EPSILON {
            return Err(DEGENERATE);
        }
        Ok(Self {
            context,
            silhouette,
            cast_surface,
            light_direction,
        })
    }
    /// Return the support context.
    pub fn context(&self) -> &IntcurveSupportContext {
        &self.context
    }
    /// Return the silhouette semantics.
    pub fn silhouette(&self) -> &SilhouetteKind {
        &self.silhouette
    }
    /// Return the cast surface.
    pub fn cast_surface(&self) -> &SurfaceId {
        &self.cast_surface
    }
    /// Return the light direction.
    pub fn light_direction(&self) -> &FiniteVector3 {
        &self.light_direction
    }
}

impl TryFrom<SilhouetteCurveConstructionWire> for SilhouetteCurveConstruction {
    type Error = ProceduralGeometryError;
    fn try_from(wire: SilhouetteCurveConstructionWire) -> Result<Self, Self::Error> {
        Self::try_new(
            wire.context,
            wire.silhouette,
            wire.cast_surface,
            wire.light_direction,
        )
    }
}

/// The support sides a context-first spring layout states.
fn spring_context_sides<R>(
    supports: &[SpringSupport<R>; 2],
    first_pcurve: &SpringPcurve<R>,
    second_pcurve: Option<&super::PcurveGeometry>,
) -> [IntcurveSupportSide; 2] {
    [
        IntcurveSupportSide {
            surface: match &supports[0] {
                SpringSupport::Surface(surface) => Some(surface.clone()),
                SpringSupport::Ranges(_) => None,
            },
            pcurve: match first_pcurve {
                SpringPcurve::Pcurve(pcurve) => {
                    Some(super::SupportPcurve::new(pcurve.clone(), None))
                }
                SpringPcurve::Range(_) => None,
            },
        },
        IntcurveSupportSide {
            surface: match &supports[1] {
                SpringSupport::Surface(surface) => Some(surface.clone()),
                SpringSupport::Ranges(_) => None,
            },
            pcurve: second_pcurve
                .cloned()
                .map(|pcurve| super::SupportPcurve::new(pcurve, None)),
        },
    ]
}

/// Admitted spring curve construction.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[cfg_attr(feature = "schema", schemars(with = "SpringCurvePayloadWire"))]
#[serde(try_from = "SpringCurvePayloadWire")]
pub struct SpringCurvePayload {
    layout: SpringLayout<FiniteReal, ParameterInterval>,
    #[serde(skip)]
    context: IntcurveSupportContext,
    direction: i64,
}
#[derive(Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(deny_unknown_fields)]
struct SpringCurvePayloadWire {
    /// Native spring record layout.
    layout: SpringLayout,

    /// Native direction code.
    direction: i64,
}
impl SpringCurvePayload {
    /// Admit the construction parameters.
    pub fn try_new(layout: SpringLayout, direction: i64) -> Result<Self, ProceduralGeometryError> {
        let layout = layout.admit().ok_or(ProceduralGeometryError::Payload(
            "spring context, null-support ranges, or cache-first form are invalid",
        ))?;
        let context = match &layout {
            SpringLayout::ContextFirst {
                supports,
                first_pcurve,
                second_pcurve,
                parameter_range,
                discontinuities,
                ..
            } => IntcurveSupportContext::from_parts(
                spring_context_sides(supports, first_pcurve, second_pcurve.as_ref()),
                *parameter_range,
                discontinuities.clone(),
            )
            .map_err(|_| {
                ProceduralGeometryError::Payload(
                    "spring context, null-support ranges, or cache-first form are invalid",
                )
            })?,
            SpringLayout::CacheFirst { context, .. } => context.clone(),
        };
        let valid_ranges = match &layout {
            SpringLayout::ContextFirst {
                supports,
                first_pcurve,
                ..
            } => {
                supports.iter().all(|support| match support {
                    SpringSupport::Surface(_) => true,
                    SpringSupport::Ranges(ranges) => ranges.iter().all(ordered),
                }) && match first_pcurve {
                    SpringPcurve::Pcurve(_) => true,
                    SpringPcurve::Range(range) => ordered(range),
                }
            }
            SpringLayout::CacheFirst { .. } => true,
        };
        if !valid_ranges {
            return Err(ProceduralGeometryError::Payload(
                "spring context, null-support ranges, or cache-first form are invalid",
            ));
        }
        Ok(Self {
            layout,
            context,
            direction,
        })
    }
    /// Return the layout.
    pub fn layout(&self) -> &SpringLayout<FiniteReal, ParameterInterval> {
        &self.layout
    }
    /// Return the support context admitted with this layout.
    pub fn support_context(&self) -> &IntcurveSupportContext {
        &self.context
    }
    /// Return the direction.
    pub fn direction(&self) -> &i64 {
        &self.direction
    }
}
impl TryFrom<SpringCurvePayloadWire> for SpringCurvePayload {
    type Error = ProceduralGeometryError;
    fn try_from(wire: SpringCurvePayloadWire) -> Result<Self, Self::Error> {
        Self::try_new(wire.layout, wire.direction)
    }
}

/// Admitted three surface intersection curve construction.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[cfg_attr(
    feature = "schema",
    schemars(with = "ThreeSurfaceIntersectionCurvePayloadWire")
)]
#[serde(try_from = "ThreeSurfaceIntersectionCurvePayloadWire")]
pub struct ThreeSurfaceIntersectionCurvePayload {
    context: IntcurveSupportContext,

    selector: i64,

    third: IntcurveSupportSide,
}
#[derive(Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(deny_unknown_fields)]
struct ThreeSurfaceIntersectionCurvePayloadWire {
    /// Shared support surfaces, UV curves, interval, and discontinuity arrays.
    context: IntcurveSupportContext,

    /// Native branch selector code.
    selector: i64,

    /// Third support side of the intersection.
    third: IntcurveSupportSide,
}
impl ThreeSurfaceIntersectionCurvePayload {
    /// Admit the construction parameters.
    pub fn try_new(
        context: IntcurveSupportContext,
        selector: i64,
        third: IntcurveSupportSide,
    ) -> Result<Self, ProceduralGeometryError> {
        if third
            .pcurve
            .as_ref()
            .is_some_and(|pcurve| pcurve.parameter_range.is_some())
            && context.parameter_range().endpoints()[0] == context.parameter_range().endpoints()[1]
        {
            return Err(ProceduralGeometryError::Payload(
                "three-surface intersection context is not finite and ordered",
            ));
        }
        Ok(Self {
            context,
            selector,
            third,
        })
    }
    /// Return the context.
    pub fn context(&self) -> &IntcurveSupportContext {
        &self.context
    }
    /// Return the selector.
    pub fn selector(&self) -> &i64 {
        &self.selector
    }
    /// Return the third.
    pub fn third(&self) -> &IntcurveSupportSide {
        &self.third
    }
}
impl TryFrom<ThreeSurfaceIntersectionCurvePayloadWire> for ThreeSurfaceIntersectionCurvePayload {
    type Error = ProceduralGeometryError;
    fn try_from(wire: ThreeSurfaceIntersectionCurvePayloadWire) -> Result<Self, Self::Error> {
        Self::try_new(wire.context, wire.selector, wire.third)
    }
}

/// Admitted projection curve construction.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[cfg_attr(feature = "schema", schemars(with = "ProjectionCurvePayloadWire"))]
#[serde(try_from = "ProjectionCurvePayloadWire")]
pub struct ProjectionCurvePayload {
    context: IntcurveSupportContext,

    discontinuity_flag: bool,

    source: CurveId,

    tail: ProjectionTail<FiniteReal>,
}
#[derive(Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(deny_unknown_fields)]
struct ProjectionCurvePayloadWire {
    /// Shared support surfaces, UV curves, interval, and discontinuity arrays.
    context: IntcurveSupportContext,

    /// Native boolean following the discontinuity arrays.
    discontinuity_flag: bool,

    /// Curve being projected.
    source: CurveId,

    /// Native projection tail.
    tail: ProjectionTail,
}
impl ProjectionCurvePayload {
    /// Admit the construction parameters.
    pub fn try_new(
        context: IntcurveSupportContext,
        discontinuity_flag: bool,
        source: CurveId,
        tail: ProjectionTail,
    ) -> Result<Self, ProceduralGeometryError> {
        let tail = tail
            .admit()
            .filter(|tail| match tail {
                ProjectionTail::EarlyClose { .. } => true,
                ProjectionTail::Ranged {
                    parameter_range, ..
                } => ordered(parameter_range),
            })
            .ok_or(ProceduralGeometryError::Payload(
                "projection fields are not finite and ordered",
            ))?;
        Ok(Self {
            context,
            discontinuity_flag,
            source,
            tail,
        })
    }
    /// Return the context.
    pub fn context(&self) -> &IntcurveSupportContext {
        &self.context
    }
    /// Return the discontinuity flag.
    pub fn discontinuity_flag(&self) -> &bool {
        &self.discontinuity_flag
    }
    /// Return the source.
    pub fn source(&self) -> &CurveId {
        &self.source
    }
    /// Return the tail.
    pub fn tail(&self) -> &ProjectionTail<FiniteReal> {
        &self.tail
    }
}
impl TryFrom<ProjectionCurvePayloadWire> for ProjectionCurvePayload {
    type Error = ProceduralGeometryError;
    fn try_from(wire: ProjectionCurvePayloadWire) -> Result<Self, Self::Error> {
        Self::try_new(
            wire.context,
            wire.discontinuity_flag,
            wire.source,
            wire.tail,
        )
    }
}

impl SpringCurvePayload {
    pub(super) fn write_revision_fit_tolerance(
        &mut self,
        value: super::FitTolerance,
        write: super::ToleranceWrite,
    ) -> super::RevisionCacheWrite {
        self.layout.write_revision_fit_tolerance(value, write)
    }
}

impl SubsetCurveConstruction {
    /// Solved-cache fit contract this construction states.
    #[must_use]
    pub const fn legacy_cache(&self) -> Option<LegacyCache> {
        self.cache
    }

    /// Mutable legacy solved-cache slot this construction states.
    pub(super) const fn legacy_cache_slot_mut(&mut self) -> &mut Option<LegacyCache> {
        &mut self.cache
    }
}

impl VectorOffsetCurveConstruction {
    /// Solved-cache fit contract this construction states.
    #[must_use]
    pub const fn legacy_cache(&self) -> Option<LegacyCache> {
        self.cache
    }

    /// Mutable legacy solved-cache slot this construction states.
    pub(super) const fn legacy_cache_slot_mut(&mut self) -> &mut Option<LegacyCache> {
        &mut self.cache
    }
}

impl TwoSidedOffsetCurveConstruction {
    /// Solved-cache fit contract this construction states.
    #[must_use]
    pub const fn legacy_cache(&self) -> Option<LegacyCache> {
        self.cache
    }

    /// Mutable legacy solved-cache slot this construction states.
    pub(super) const fn legacy_cache_slot_mut(&mut self) -> &mut Option<LegacyCache> {
        &mut self.cache
    }
}

impl SpringCurvePayload {
    /// Solved-cache fit contract the context-first layout states.
    #[must_use]
    pub const fn legacy_cache(&self) -> Option<LegacyCache> {
        match &self.layout {
            SpringLayout::ContextFirst { cache, .. } => *cache,
            SpringLayout::CacheFirst { .. } => None,
        }
    }

    /// Mutable legacy solved-cache slot of the context-first layout. The
    /// cache-first layout states its tolerance in its cache form.
    pub(super) fn legacy_cache_slot_mut(&mut self) -> Option<&mut Option<LegacyCache>> {
        match &mut self.layout {
            SpringLayout::ContextFirst { cache, .. } => Some(cache),
            SpringLayout::CacheFirst { .. } => None,
        }
    }
}

impl SurfaceOffsetCurveConstruction {
    /// Solved-cache fit contract the context-first layout states.
    #[must_use]
    pub const fn legacy_cache(&self) -> Option<LegacyCache> {
        match self.cache.legacy_fit_tolerance() {
            Some(fit_tolerance) => Some(LegacyCache { fit_tolerance }),
            None => None,
        }
    }

    /// Mutable legacy solved-cache slot, absent when a revision-gated form
    /// states the tolerance instead.
    pub(super) fn legacy_cache_slot_mut(&mut self) -> Option<&mut Option<LegacyCache>> {
        self.cache.legacy_cache_mut()
    }
}

/// Whether an admitted pair is ordered.
fn ordered(range: &[FiniteReal; 2]) -> bool {
    range[0] <= range[1]
}

#[cfg(test)]
mod tests;

// Each optional key below names itself in whatever it refuses.
cadmpeg_core::named_optional_field!(deserialize_range, CurveOffsetRange, "range");
cadmpeg_core::named_optional_field!(deserialize_cache, LegacyCache, "cache");
