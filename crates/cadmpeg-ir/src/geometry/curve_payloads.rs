// SPDX-License-Identifier: Apache-2.0
//! Checked procedural curve payloads.

use super::{
    default_true, vector_offset_roles_wire, CacheFirstCurveForm, CurveOffsetRange,
    DeformableCurveData, DeformableCurveSource, IntcurveSupportContext, OffsetSide,
    ProceduralGeometryError, SilhouetteKind, VectorOffsetRoles,
};
use super::{IntcurveSupportSide, ProjectionTail, SpringLayout};
#[cfg(feature = "schema")]
use super::{OffsetSideWire, VectorOffsetRolesWire};
use crate::features::FiniteVector3;
use crate::ids::{CurveId, SurfaceId};
use crate::math::Vector3;
use crate::scalar::FiniteReal;
use crate::topology::ParameterInterval;
use crate::units::FiniteVector;
#[cfg(feature = "schema")]
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

const EPS_SPATIAL_CURVE_DIRECTION: f64 = 1.0e-9;
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
    base_endpoints: [Option<f64>; 2],
    /// Cache-first shared-context fields; absent from the context-first
    /// layout.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    cache_first: Option<CacheFirstCurveForm>,
    /// Signed model-space offset distance.
    distance: FiniteReal,
    /// Native unscaled parameter shift.
    shift: FiniteReal,
    /// Native unscaled parameter scale.
    scale: FiniteReal,
}

#[derive(Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
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
    /// Cache-first shared-context fields; absent from the context-first
    /// layout.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    cache_first: Option<CacheFirstCurveForm>,
    /// Signed model-space offset distance.
    distance: f64,
    /// Native unscaled parameter shift.
    shift: f64,
    /// Native unscaled parameter scale.
    scale: f64,
}

impl SurfaceOffsetCurveConstruction {
    pub(super) fn revision_cache_mut(
        &mut self,
    ) -> Option<&mut super::RevisionCacheForm<super::CacheFirstCurveParameterization>> {
        self.cache_first.as_mut().map(|form| &mut form.cache)
    }
    /// Admit the construction parameters.
    pub fn try_new(
        context: IntcurveSupportContext,
        discontinuity_flag: bool,
        [base_u_range, base_v_range]: [[f64; 2]; 2],
        (base, base_range, base_endpoints): (CurveId, [f64; 2], [Option<f64>; 2]),
        cache_first: Option<CacheFirstCurveForm>,
        distance: f64,
        [shift, scale]: [f64; 2],
    ) -> Result<Self, ProceduralGeometryError> {
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
            base_endpoints,
            cache_first,
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
    pub fn base_u_range(&self) -> &[f64; 2] {
        self.base_u_range.as_raw()
    }
    /// Return the base v range.
    pub fn base_v_range(&self) -> &[f64; 2] {
        self.base_v_range.as_raw()
    }
    /// Return the base.
    pub fn base(&self) -> &CurveId {
        &self.base
    }
    /// Return the base range.
    pub fn base_range(&self) -> &[f64; 2] {
        self.base_range.as_raw()
    }
    /// Return the base endpoints.
    pub fn base_endpoints(&self) -> &[Option<f64>; 2] {
        &self.base_endpoints
    }
    /// Return the cache first.
    pub fn cache_first(&self) -> &Option<CacheFirstCurveForm> {
        &self.cache_first
    }
    /// Return the distance.
    pub fn distance(&self) -> &f64 {
        self.distance.as_raw()
    }
    /// Return the shift.
    pub fn shift(&self) -> &f64 {
        self.shift.as_raw()
    }
    /// Return the scale.
    pub fn scale(&self) -> &f64 {
        self.scale.as_raw()
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
            wire.cache_first,
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
    cache_first: CacheFirstCurveForm,
    /// Curve being deformed or its unresolved native reference.
    source: DeformableCurveSource,
    /// Optional native bounds following the source curve.
    source_parameter_range: [Option<FiniteReal>; 2],
    /// Discriminator-specific deformation payload.
    data: DeformableCurveData,
}

#[derive(Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
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
    pub(super) fn revision_cache_mut(
        &mut self,
    ) -> &mut super::RevisionCacheForm<super::CacheFirstCurveParameterization> {
        &mut self.cache_first.cache
    }
    /// Admit the construction parameters.
    pub fn try_new(
        context: IntcurveSupportContext,
        cache_first: CacheFirstCurveForm,
        source: DeformableCurveSource,
        source_parameter_range: [Option<f64>; 2],
        data: DeformableCurveData,
    ) -> Result<Self, ProceduralGeometryError> {
        let finite_vector = |vector: &crate::math::Vector3| {
            vector.x.is_finite() && vector.y.is_finite() && vector.z.is_finite()
        };
        let payload_finite = match &data {
            crate::geometry::DeformableCurveData::VectorField {
                vectors,
                parameter_pairs,
            } => {
                vectors.iter().all(finite_vector)
                    && parameter_pairs
                        .iter()
                        .flatten()
                        .all(|value| value.is_finite())
            }
            crate::geometry::DeformableCurveData::Mode3 {
                leading_vectors,
                leading_parameter,
                trailing_point,
                trailing_vectors,
                frame_parameter,
                parameters,
                trailing_parameter,
                ..
            } => {
                leading_vectors.iter().all(finite_vector)
                    && leading_parameter.is_finite()
                    && [trailing_point.x, trailing_point.y, trailing_point.z]
                        .into_iter()
                        .all(f64::is_finite)
                    && trailing_vectors.iter().all(finite_vector)
                    && frame_parameter.is_finite()
                    && parameters.iter().all(|value| value.is_finite())
                    && trailing_parameter.is_finite()
            }
        };
        if !payload_finite {
            return Err(ProceduralGeometryError::Payload(
                "deformable curve payload is not finite",
            ));
        }

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
    pub fn cache_first(&self) -> &CacheFirstCurveForm {
        &self.cache_first
    }
    /// Return the source.
    pub fn source(&self) -> &DeformableCurveSource {
        &self.source
    }
    /// Return the source parameter range.
    pub fn source_parameter_range(&self) -> [Option<f64>; 2] {
        self.source_parameter_range
            .map(|value| value.map(FiniteReal::get))
    }
    /// Return the data.
    pub fn data(&self) -> &DeformableCurveData {
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
    #[serde(flatten)]
    #[cfg_attr(feature = "schema", schemars(with = "OffsetSideWire"))]
    side: OffsetSide,
    /// Retained parameter range, with its distance law when variable.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    range: Option<CurveOffsetRange>,
}

#[derive(Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
struct OffsetCurveConstructionWire {
    /// Curve this curve is offset from.
    source: CurveId,
    /// Signed offset distance, in document length units.
    distance: f64,
    /// Exclusive plane-normal or explicit-direction carrier.
    #[serde(flatten)]
    #[cfg_attr(feature = "schema", schemars(with = "OffsetSideWire"))]
    side: OffsetSide,
    /// Retained parameter range, with its distance law when variable.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    range: Option<CurveOffsetRange>,
}

impl OffsetCurveConstruction {
    /// Admit the construction parameters.
    pub fn try_new(
        source: CurveId,
        distance: f64,
        side: OffsetSide,
        range: Option<CurveOffsetRange>,
    ) -> Result<Self, ProceduralGeometryError> {
        let side_valid = match &side {
            crate::geometry::OffsetSide::PlaneNormal(normal) => {
                normal.x.is_finite()
                    && normal.y.is_finite()
                    && normal.z.is_finite()
                    && (normal.norm() - 1.0).abs() <= EPS_OFFSET_PLANE_NORMAL
            }
            crate::geometry::OffsetSide::Direction { direction, .. } => {
                direction.x.is_finite()
                    && direction.y.is_finite()
                    && direction.z.is_finite()
                    && direction.norm() > 0.0
            }
        };
        let range_valid = range.as_ref().is_none_or(|range| {
            let parameter_range = match range {
                crate::geometry::CurveOffsetRange::Uniform { parameter_range }
                | crate::geometry::CurveOffsetRange::Variable {
                    parameter_range, ..
                } => parameter_range,
            };
            parameter_range.iter().all(|value| value.is_finite())
                && parameter_range[0] < parameter_range[1]
        });
        let law_valid = match &range {
            Some(crate::geometry::CurveOffsetRange::Variable { distance_law, .. }) => {
                match distance_law {
                    crate::geometry::CurveOffsetDistanceLaw::Linear {
                        distances,
                        control_range,
                        ..
                    } => {
                        distances.iter().all(|value| value.is_finite())
                            && control_range.iter().all(|value| value.is_finite())
                            && control_range[0] < control_range[1]
                    }
                    crate::geometry::CurveOffsetDistanceLaw::Coordinate {
                        function_parameter_offset,
                        function_parameter_scale,
                        ..
                    } => {
                        function_parameter_offset.is_finite()
                            && function_parameter_scale.is_finite()
                            && *function_parameter_scale != 0.0
                    }
                }
            }
            None | Some(crate::geometry::CurveOffsetRange::Uniform { .. }) => true,
        };
        if !side_valid || !range_valid || !law_valid {
            return Err(ProceduralGeometryError::Payload(
                "curve offset distance, side, range, or law is invalid",
            ));
        }

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
    /// Return the distance.
    pub fn distance(&self) -> &f64 {
        self.distance.as_raw()
    }
    /// Return the side.
    pub fn side(&self) -> &OffsetSide {
        &self.side
    }
    /// Return the range.
    pub fn range(&self) -> &Option<CurveOffsetRange> {
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
    reference_direction: FiniteVector3,
    /// Whether the source classifies the result as self-intersecting.
    self_intersect: Option<bool>,
}

#[derive(Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
struct SpatialOffsetCurveConstructionWire {
    /// Curve being offset.
    source: CurveId,
    /// Signed offset distance.
    distance: f64,
    /// Reference direction controlling the offset frame.
    reference_direction: Vector3,
    /// Whether the source classifies the result as self-intersecting.
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
        let reference_direction = FiniteVector3::new(reference_direction).ok_or(
            ProceduralGeometryError::Payload("invalid spatial curve offset"),
        )?;
        if (reference_direction.as_raw().norm() - 1.0).abs() > EPS_SPATIAL_CURVE_DIRECTION {
            return Err(ProceduralGeometryError::Payload(
                "invalid spatial curve offset",
            ));
        }

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
    pub fn distance(&self) -> &f64 {
        self.distance.as_raw()
    }
    /// Return the reference direction.
    pub fn reference_direction(&self) -> &Vector3 {
        self.reference_direction.as_raw()
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
}

#[derive(Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
struct TwoSidedOffsetCurveConstructionWire {
    /// Shared surfaces, UV curves, interval, and discontinuity metadata.
    context: IntcurveSupportContext,
    /// Native boolean following the discontinuity arrays.
    discontinuity_flag: bool,
    /// Signed offset distance for each support side, in document length units.
    offsets: [f64; 2],
}

impl TwoSidedOffsetCurveConstruction {
    /// Admit the construction parameters.
    pub fn try_new(
        context: IntcurveSupportContext,
        discontinuity_flag: bool,
        offsets: [f64; 2],
    ) -> Result<Self, ProceduralGeometryError> {
        Ok(Self {
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
    pub fn offsets(&self) -> &[f64; 2] {
        self.offsets.as_raw()
    }
}

impl TryFrom<TwoSidedOffsetCurveConstructionWire> for TwoSidedOffsetCurveConstruction {
    type Error = ProceduralGeometryError;
    fn try_from(wire: TwoSidedOffsetCurveConstructionWire) -> Result<Self, Self::Error> {
        Self::try_new(wire.context, wire.discontinuity_flag, wire.offsets)
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
    /// Integer codes attached to the fixed `source` and `offset` roles.
    #[serde(flatten, with = "vector_offset_roles_wire")]
    #[cfg_attr(feature = "schema", schemars(with = "VectorOffsetRolesWire"))]
    roles: VectorOffsetRoles,
}

#[derive(Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
struct VectorOffsetCurveConstructionWire {
    /// Curve being offset.
    source: CurveId,
    /// Native parameter interval on the source curve.
    parameter_range: [f64; 2],
    /// Model-space offset vector.
    offset: Vector3,
    /// Integer codes attached to the fixed `source` and `offset` roles.
    #[serde(flatten, with = "vector_offset_roles_wire")]
    #[cfg_attr(feature = "schema", schemars(with = "VectorOffsetRolesWire"))]
    roles: VectorOffsetRoles,
}

impl VectorOffsetCurveConstruction {
    /// Admit the construction parameters.
    pub fn try_new(
        source: CurveId,
        parameter_range: [f64; 2],
        offset: Vector3,
        roles: VectorOffsetRoles,
    ) -> Result<Self, ProceduralGeometryError> {
        Ok(Self {
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
    pub fn parameter_range(&self) -> &[f64; 2] {
        self.parameter_range.as_raw()
    }
    /// Return the offset.
    pub fn offset(&self) -> &Vector3 {
        self.offset.as_raw()
    }
    /// Return the roles.
    pub fn roles(&self) -> &VectorOffsetRoles {
        &self.roles
    }
}

impl TryFrom<VectorOffsetCurveConstructionWire> for VectorOffsetCurveConstruction {
    type Error = ProceduralGeometryError;
    fn try_from(wire: VectorOffsetCurveConstructionWire) -> Result<Self, Self::Error> {
        Self::try_new(wire.source, wire.parameter_range, wire.offset, wire.roles)
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
    #[serde(default = "default_true")]
    sense: bool,
}

#[derive(Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
struct SubsetCurveConstructionWire {
    /// Parent curve being restricted.
    source: CurveId,
    /// Native parameter interval retained from the parent.
    parameter_range: [f64; 2],
    /// Whether the subset follows increasing parent parameters.
    #[serde(default = "default_true")]
    sense: bool,
}

impl SubsetCurveConstruction {
    /// Admit the construction parameters.
    pub fn try_new(
        source: CurveId,
        parameter_range: [f64; 2],
        sense: bool,
    ) -> Result<Self, ProceduralGeometryError> {
        Ok(Self {
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
    pub fn parameter_range(&self) -> &[f64; 2] {
        self.parameter_range.as_raw()
    }
    /// Return the sense.
    pub fn sense(&self) -> &bool {
        &self.sense
    }
}

impl TryFrom<SubsetCurveConstructionWire> for SubsetCurveConstruction {
    type Error = ProceduralGeometryError;
    fn try_from(wire: SubsetCurveConstructionWire) -> Result<Self, Self::Error> {
        Self::try_new(wire.source, wire.parameter_range, wire.sense)
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
    pub fn light_direction(&self) -> &Vector3 {
        self.light_direction.as_raw()
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

/// Admitted spring curve construction.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[cfg_attr(feature = "schema", schemars(with = "SpringCurvePayloadWire"))]
#[serde(try_from = "SpringCurvePayloadWire")]
pub struct SpringCurvePayload {
    layout: SpringLayout,

    direction: i64,
}
#[derive(Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
struct SpringCurvePayloadWire {
    layout: SpringLayout,

    direction: i64,
}
impl SpringCurvePayload {
    /// Admit the construction parameters.
    pub fn try_new(layout: SpringLayout, direction: i64) -> Result<Self, ProceduralGeometryError> {
        let context = layout.support_context();
        let inline_ranges_finite = match &layout {
            crate::geometry::SpringLayout::ContextFirst {
                supports,
                first_pcurve,
                ..
            } => {
                supports.iter().all(|support| match support {
                    crate::geometry::SpringSupport::Surface(_) => true,
                    crate::geometry::SpringSupport::Ranges(ranges) => ranges.iter().all(|range| {
                        range.iter().all(|value| value.is_finite()) && range[0] <= range[1]
                    }),
                }) && match first_pcurve {
                    crate::geometry::SpringPcurve::Pcurve(_) => true,
                    crate::geometry::SpringPcurve::Range(range) => {
                        range.iter().all(|value| value.is_finite()) && range[0] <= range[1]
                    }
                }
            }
            crate::geometry::SpringLayout::CacheFirst { .. } => true,
        };
        if context.is_err() || !inline_ranges_finite {
            return Err(ProceduralGeometryError::Payload(
                "spring context or null-support ranges are invalid",
            ));
        }
        Ok(Self { layout, direction })
    }
    /// Return the layout.
    pub fn layout(&self) -> &SpringLayout {
        &self.layout
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
struct ThreeSurfaceIntersectionCurvePayloadWire {
    context: IntcurveSupportContext,

    selector: i64,

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
            && context.parameter_range()[0] == context.parameter_range()[1]
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

    tail: ProjectionTail,
}
#[derive(Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
struct ProjectionCurvePayloadWire {
    context: IntcurveSupportContext,

    discontinuity_flag: bool,

    source: CurveId,

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
        let tail_finite = match &tail {
            crate::geometry::ProjectionTail::EarlyClose { .. } => true,
            crate::geometry::ProjectionTail::Ranged {
                parameter_range, ..
            } => {
                parameter_range.iter().all(|value| value.is_finite())
                    && parameter_range[0] <= parameter_range[1]
            }
        };
        if !tail_finite {
            return Err(ProceduralGeometryError::Payload(
                "projection fields are not finite and ordered",
            ));
        }
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
    pub fn tail(&self) -> &ProjectionTail {
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
    pub(super) fn revision_cache_mut(
        &mut self,
    ) -> Option<&mut super::RevisionCacheForm<super::CacheFirstCurveParameterization>> {
        self.layout.cache_first_mut().map(|form| &mut form.cache)
    }
}

#[cfg(test)]
mod tests;
