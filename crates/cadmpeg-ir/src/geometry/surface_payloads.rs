// SPDX-License-Identifier: Apache-2.0
//! Checked procedural surface payloads.

use super::{
    BlendCrossSection, BlendRadiusLaw, BlendSupport, CompoundComponent, CompoundLoftConstruction,
    CompoundLoftTail, DeformableSurfaceConstruction, ExactSpline, G2BlendConstruction,
    LawSurfaceConstruction, LoftBridgeToken, LoftRevisionForm, LoftSection, NetSurfaceConstruction,
    RollingBallConstruction, ScaledCompoundLoftConstruction, ScaledCompoundLoftShape,
    SkinSurfaceConstruction, SplineSurfaceParameters, SweepSurfaceConstruction,
    VariableBlendConstruction, VertexBlendBoundaryGeometry, VertexBlendConstruction,
};
use super::{CacheContract, LegacyCache, LegacyCacheSlot};
use crate::features::{FinitePoint3, FiniteVector3};
use crate::ids::{CurveId, SurfaceId};
use crate::math::{Point3, Vector3};
use crate::scalar::FiniteReal;
use crate::topology::IncreasingParameterInterval;
use crate::units::{FiniteVector, UnitVector3};
#[cfg(feature = "schema")]
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use {
    super::{
        DirectedParameterRange, OffsetExtension, ProceduralGeometryError, RevisionSurfaceForm,
        TaperSurfaceKind,
    },
    crate::geometry::pcurve::PcurveGeometry,
};

/// Admitted support surface restriction parameters.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[cfg_attr(feature = "schema", schemars(with = "SubSurfaceConstructionWire"))]
#[serde(try_from = "SubSurfaceConstructionWire")]
pub struct SubSurfaceConstruction {
    /// Embedded support surface whose parameterization is retained.
    support: SurfaceId,
    /// Finite U and V parameter intervals, each with its endpoints in stored order.
    parameter_ranges: [FiniteVector<2>; 2],
}

#[derive(Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(deny_unknown_fields)]
struct SubSurfaceConstructionWire {
    /// Embedded support surface whose parameterization is retained.
    support: SurfaceId,
    /// Finite U and V parameter intervals, each with its endpoints in stored order.
    parameter_ranges: [[f64; 2]; 2],
}

impl SubSurfaceConstruction {
    /// Admit the construction parameters.
    pub fn try_new(
        support: SurfaceId,
        parameter_ranges: [[f64; 2]; 2],
    ) -> Result<Self, ProceduralGeometryError> {
        Ok(Self {
            support,
            parameter_ranges: [
                FiniteVector::new(parameter_ranges[0]).ok_or(ProceduralGeometryError::Payload(
                    "SubSurface.parameter_ranges is not finite",
                ))?,
                FiniteVector::new(parameter_ranges[1]).ok_or(ProceduralGeometryError::Payload(
                    "SubSurface.parameter_ranges is not finite",
                ))?,
            ],
        })
    }
    /// Return the support.
    pub fn support(&self) -> &SurfaceId {
        &self.support
    }
    /// Return the parameter ranges.
    pub fn parameter_ranges(&self) -> [FiniteVector<2>; 2] {
        self.parameter_ranges
    }
}

impl TryFrom<SubSurfaceConstructionWire> for SubSurfaceConstruction {
    type Error = ProceduralGeometryError;
    fn try_from(wire: SubSurfaceConstructionWire) -> Result<Self, Self::Error> {
        Self::try_new(wire.support, wire.parameter_ranges)
    }
}

/// Admitted taper surface parameters.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[cfg_attr(feature = "schema", schemars(with = "TaperSurfaceConstructionWire"))]
#[serde(try_from = "TaperSurfaceConstructionWire")]
pub struct TaperSurfaceConstruction {
    /// Base surface being tapered.
    support: SurfaceId,
    /// Reference curve on the support.
    reference: CurveId,
    /// UV curve on the support, absent for `nullbs`.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_pcurve"
    )]
    pcurve: Option<PcurveGeometry>,
    /// Native taper parameter or draft magnitude.
    parameter: FiniteReal,
    /// Subtype-specific taper tail.
    taper: TaperSurfaceKind<FiniteReal, FiniteVector3>,
    /// Cache contract: the revision-gated form, or the legacy
    /// solved-cache tolerance this construction states instead.
    #[serde(default, skip_serializing_if = "CacheContract::is_bare_legacy")]
    cache: CacheContract<RevisionSurfaceForm<Vec<bool>, FiniteReal>>,
}

#[derive(Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(deny_unknown_fields)]
struct TaperSurfaceConstructionWire {
    /// Base surface being tapered.
    support: SurfaceId,
    /// Reference curve on the support.
    reference: CurveId,
    /// UV curve on the support, absent for `nullbs`.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_pcurve"
    )]
    pcurve: Option<PcurveGeometry>,
    /// Native taper parameter or draft magnitude.
    parameter: f64,
    /// Subtype-specific taper tail.
    taper: TaperSurfaceKind,
    /// Cache contract: the revision-gated form, or the legacy
    /// solved-cache tolerance this construction states instead.
    #[serde(default)]
    cache: CacheContract<RevisionSurfaceForm>,
}

impl TaperSurfaceConstruction {
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
        support: SurfaceId,
        reference: CurveId,
        pcurve: Option<PcurveGeometry>,
        parameter: f64,
        taper: TaperSurfaceKind,
        cache: CacheContract<RevisionSurfaceForm>,
    ) -> Result<Self, ProceduralGeometryError> {
        let (Some(taper), Some(cache)) =
            (taper.admit(), cache.admit_form(RevisionSurfaceForm::admit))
        else {
            return Err(ProceduralGeometryError::Payload(
                "taper surface parameter or subtype tail is invalid",
            ));
        };

        Ok(Self {
            support,
            reference,
            pcurve,
            parameter: FiniteReal::new(parameter).ok_or(ProceduralGeometryError::Payload(
                "Taper.parameter is not finite",
            ))?,
            taper,
            cache,
        })
    }
    /// Return the support.
    pub fn support(&self) -> &SurfaceId {
        &self.support
    }
    /// Return the reference.
    pub fn reference(&self) -> &CurveId {
        &self.reference
    }
    /// Return the pcurve.
    pub fn pcurve(&self) -> &Option<PcurveGeometry> {
        &self.pcurve
    }
    /// Return the parameter.
    pub fn parameter(&self) -> FiniteReal {
        self.parameter
    }
    /// Return the taper.
    pub fn taper(&self) -> &TaperSurfaceKind<FiniteReal, FiniteVector3> {
        &self.taper
    }
    /// Return the revision form.
    pub const fn revision_form(&self) -> Option<&RevisionSurfaceForm<Vec<bool>, FiniteReal>> {
        self.cache.form()
    }
}

impl TryFrom<TaperSurfaceConstructionWire> for TaperSurfaceConstruction {
    type Error = ProceduralGeometryError;
    fn try_from(wire: TaperSurfaceConstructionWire) -> Result<Self, Self::Error> {
        Self::try_new(
            wire.support,
            wire.reference,
            wire.pcurve,
            wire.parameter,
            wire.taper,
            wire.cache,
        )
    }
}

/// Admitted extrusion surface parameters.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[cfg_attr(
    feature = "schema",
    schemars(with = "ExtrusionSurfaceConstructionWire")
)]
#[serde(try_from = "ExtrusionSurfaceConstructionWire")]
pub struct ExtrusionSurfaceConstruction {
    /// Curve swept along `direction` to form the surface.
    directrix: CurveId,
    /// Native source directrix parameter interval, when carried by the
    /// source. The neutral surface-carrier interval is in
    /// `ProceduralSurface::record_bounds`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    parameter_interval: Option<FiniteVector<2>>,
    /// Length-bearing sweep direction, in document length units.
    direction: FiniteVector3,
    /// Native model-space position following the sweep direction, when carried.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    native_position: Option<FinitePoint3>,
    /// Cache contract: the revision-gated form, or the legacy
    /// solved-cache tolerance this construction states instead.
    /// The directrix parameter interval is `parameter_interval`.
    #[serde(default, skip_serializing_if = "CacheContract::is_bare_legacy")]
    cache: CacheContract<RevisionSurfaceForm<Vec<bool>, FiniteReal>>,
}

#[derive(Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(deny_unknown_fields)]
struct ExtrusionSurfaceConstructionWire {
    /// Curve swept along `direction` to form the surface.
    directrix: CurveId,
    /// Native source directrix parameter interval, when carried by the
    /// source. The neutral surface-carrier interval is in
    /// `ProceduralSurface::record_bounds`.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_parameter_interval"
    )]
    parameter_interval: Option<[f64; 2]>,
    /// Length-bearing sweep direction, in document length units.
    direction: Vector3,
    /// Native model-space position following the sweep direction, when carried.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_native_position"
    )]
    native_position: Option<Point3>,
    /// Cache contract: the revision-gated form, or the legacy
    /// solved-cache tolerance this construction states instead.
    /// The directrix parameter interval is `parameter_interval`.
    #[serde(default)]
    cache: CacheContract<RevisionSurfaceForm>,
}

impl ExtrusionSurfaceConstruction {
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
        directrix: CurveId,
        parameter_interval: Option<[f64; 2]>,
        direction: Vector3,
        native_position: Option<Point3>,
        cache: CacheContract<RevisionSurfaceForm>,
    ) -> Result<Self, ProceduralGeometryError> {
        let cache = cache.admit_form(RevisionSurfaceForm::admit).ok_or(
            ProceduralGeometryError::Payload("Extrusion.cache is invalid"),
        )?;
        Ok(Self {
            directrix,
            parameter_interval: parameter_interval
                .map(|range| {
                    FiniteVector::new(range).ok_or(ProceduralGeometryError::Payload(
                        "Extrusion.parameter_interval is not finite",
                    ))
                })
                .transpose()?,
            direction: FiniteVector3::new(direction).ok_or(ProceduralGeometryError::Payload(
                "Extrusion.direction is not finite",
            ))?,
            native_position: native_position
                .map(|point| {
                    FinitePoint3::new(point).ok_or(ProceduralGeometryError::Payload(
                        "Extrusion.native_position is not finite",
                    ))
                })
                .transpose()?,
            cache,
        })
    }
    /// Build the construction with a legacy cache contract from admitted
    /// parts. The interval, direction and position types state finiteness,
    /// and a legacy cache contract has no condition of its own, so nothing is
    /// checked.
    #[must_use]
    pub const fn legacy(
        directrix: CurveId,
        parameter_interval: Option<FiniteVector<2>>,
        direction: FiniteVector3,
        native_position: Option<FinitePoint3>,
        cache: Option<LegacyCache>,
    ) -> Self {
        Self {
            directrix,
            parameter_interval,
            direction,
            native_position,
            cache: CacheContract::Legacy { cache },
        }
    }
    /// Return the directrix.
    pub fn directrix(&self) -> &CurveId {
        &self.directrix
    }
    /// Return the parameter interval.
    pub fn parameter_interval(&self) -> Option<FiniteVector<2>> {
        self.parameter_interval
    }
    /// Return the direction.
    pub fn direction(&self) -> &FiniteVector3 {
        &self.direction
    }
    /// Replace the direction and keep every other admitted field. A finite
    /// direction is the whole direction condition, so nothing is checked.
    pub fn set_direction(&mut self, direction: FiniteVector3) {
        self.direction = direction;
    }
    /// Return the native position.
    pub fn native_position(&self) -> Option<FinitePoint3> {
        self.native_position
    }
    /// Replace the native position and keep every other admitted field. A
    /// finite position is the whole position condition, so nothing is
    /// checked.
    pub fn set_native_position(&mut self, native_position: Option<FinitePoint3>) {
        self.native_position = native_position;
    }
    /// Return the revision form.
    pub const fn revision_form(&self) -> Option<&RevisionSurfaceForm<Vec<bool>, FiniteReal>> {
        self.cache.form()
    }
}

impl TryFrom<ExtrusionSurfaceConstructionWire> for ExtrusionSurfaceConstruction {
    type Error = ProceduralGeometryError;
    fn try_from(wire: ExtrusionSurfaceConstructionWire) -> Result<Self, Self::Error> {
        Self::try_new(
            wire.directrix,
            wire.parameter_interval,
            wire.direction,
            wire.native_position,
            wire.cache,
        )
    }
}

const INVALID_REVOLUTION_AXIS: ProceduralGeometryError = ProceduralGeometryError::Payload(
    "revolution axis_origin and axis_direction must be finite, with unit axis_direction",
);

/// Admit a revolution axis: a point on the axis with finite coordinates and
/// a direction whose norm is within `1e-9` of one. Both revolution
/// constructions store the axis in this form.
pub fn admit_revolution_axis(
    origin: Point3,
    direction: Vector3,
) -> Result<(FinitePoint3, UnitVector3), ProceduralGeometryError> {
    let direction = UnitVector3::new(direction).ok_or(INVALID_REVOLUTION_AXIS)?;
    Ok((
        FinitePoint3::new(origin).ok_or(INVALID_REVOLUTION_AXIS)?,
        direction,
    ))
}

fn admit_revolution_interval(
    range: [f64; 2],
    message: &'static str,
) -> Result<IncreasingParameterInterval, ProceduralGeometryError> {
    IncreasingParameterInterval::new(range).ok_or(ProceduralGeometryError::Payload(message))
}

/// Admitted revolution surface parameters.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[cfg_attr(
    feature = "schema",
    schemars(with = "RevolutionSurfaceConstructionWire")
)]
#[serde(try_from = "RevolutionSurfaceConstructionWire")]
pub struct RevolutionSurfaceConstruction {
    /// Curve revolved about the axis to form the surface.
    directrix: CurveId,
    /// A point on the revolution axis, with finite coordinates.
    axis_origin: FinitePoint3,
    /// Direction of the revolution axis, with norm within `1e-9` of one.
    axis_direction: UnitVector3,
    /// Angular start and end parameters, in radians.
    angular_interval: IncreasingParameterInterval,
    /// Surface-parameter interval that maps affinely to
    /// `angular_interval`. Absence means the surface parameter is already
    /// the revolution angle in radians.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    angular_parameter_interval: Option<IncreasingParameterInterval>,
    /// Native source directrix parameter start and end values, when
    /// carried by the source representation. The neutral surface-carrier
    /// interval is in `ProceduralSurface::record_bounds`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    parameter_interval: Option<IncreasingParameterInterval>,
    /// Whether the source parameter directions are transposed.
    transposed: bool,
    /// Cache contract: the revision-gated form, or the legacy
    /// solved-cache tolerance this construction states instead.
    /// The profile curve's optional endpoints are `reference_endpoints`.
    #[serde(default, skip_serializing_if = "CacheContract::is_bare_legacy")]
    cache: CacheContract<RevisionSurfaceForm<Vec<bool>, FiniteReal>>,
}

#[derive(Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(deny_unknown_fields)]
struct RevolutionSurfaceConstructionWire {
    /// Curve revolved about the axis to form the surface.
    directrix: CurveId,
    /// A point on the revolution axis, with finite coordinates.
    axis_origin: Point3,
    /// Direction of the revolution axis, with norm within `1e-9` of one.
    axis_direction: Vector3,
    /// Angular start and end parameters, in radians.
    angular_interval: [f64; 2],
    /// Surface-parameter interval that maps affinely to
    /// `angular_interval`. Absence means the surface parameter is already
    /// the revolution angle in radians.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_angular_parameter_interval"
    )]
    angular_parameter_interval: Option<[f64; 2]>,
    /// Native source directrix parameter start and end values, when
    /// carried by the source representation. The neutral surface-carrier
    /// interval is in `ProceduralSurface::record_bounds`.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_parameter_interval"
    )]
    parameter_interval: Option<[f64; 2]>,
    /// Whether the source parameter directions are transposed.
    transposed: bool,
    /// Cache contract: the revision-gated form, or the legacy
    /// solved-cache tolerance this construction states instead.
    /// The profile curve's optional endpoints are `reference_endpoints`.
    #[serde(default)]
    cache: CacheContract<RevisionSurfaceForm>,
}

impl RevolutionSurfaceConstruction {
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

    /// Admit the construction parameters from raw intervals. The axis types
    /// state the axis contract. A refused cache form is reported first, then
    /// each refused interval with a refusal that names it.
    pub fn try_new(
        directrix: CurveId,
        (axis_origin, axis_direction): (FinitePoint3, UnitVector3),
        angular_interval: [f64; 2],
        angular_parameter_interval: Option<[f64; 2]>,
        parameter_interval: Option<[f64; 2]>,
        transposed: bool,
        cache: CacheContract<RevisionSurfaceForm>,
    ) -> Result<Self, ProceduralGeometryError> {
        let cache = cache.admit_form(RevisionSurfaceForm::admit).ok_or(
            ProceduralGeometryError::Payload("revolution cache form is invalid"),
        )?;
        let angular_interval = admit_revolution_interval(
            angular_interval,
            "revolution angular_interval must be finite and strictly increasing",
        )?;
        let angular_parameter_interval = angular_parameter_interval
            .map(|range| {
                admit_revolution_interval(
                    range,
                    "revolution angular_parameter_interval must be finite and strictly increasing",
                )
            })
            .transpose()?;
        let parameter_interval = parameter_interval
            .map(|range| {
                admit_revolution_interval(
                    range,
                    "revolution parameter_interval must be finite and strictly increasing",
                )
            })
            .transpose()?;
        Ok(Self {
            directrix,
            axis_origin,
            axis_direction,
            angular_interval,
            angular_parameter_interval,
            parameter_interval,
            transposed,
            cache,
        })
    }

    /// Build the construction with a legacy cache contract from admitted
    /// parts. The axis and interval types state their contracts, and a legacy
    /// cache contract has no condition of its own, so nothing is checked.
    #[must_use]
    pub const fn legacy(
        directrix: CurveId,
        (axis_origin, axis_direction): (FinitePoint3, UnitVector3),
        angular_interval: IncreasingParameterInterval,
        angular_parameter_interval: Option<IncreasingParameterInterval>,
        parameter_interval: Option<IncreasingParameterInterval>,
        transposed: bool,
        cache: Option<LegacyCache>,
    ) -> Self {
        Self {
            directrix,
            axis_origin,
            axis_direction,
            angular_interval,
            angular_parameter_interval,
            parameter_interval,
            transposed,
            cache: CacheContract::Legacy { cache },
        }
    }
    /// Return the directrix.
    pub fn directrix(&self) -> &CurveId {
        &self.directrix
    }
    /// Return the admitted axis origin.
    #[must_use]
    pub const fn axis_origin(&self) -> FinitePoint3 {
        self.axis_origin
    }
    /// Replace the axis origin and keep every other admitted field.
    pub fn set_axis_origin(&mut self, axis_origin: FinitePoint3) {
        self.axis_origin = axis_origin;
    }
    /// Return the admitted unit axis direction. A caller that passes it on
    /// keeps the unit-length guarantee and performs no new admission.
    pub const fn axis_direction(&self) -> UnitVector3 {
        self.axis_direction
    }
    /// Return the admitted angular interval. A caller that passes it on
    /// keeps the finite strictly increasing guarantee and performs no new
    /// admission.
    pub const fn angular_interval(&self) -> IncreasingParameterInterval {
        self.angular_interval
    }
    /// Return the admitted angular parameter interval, with the guarantee of
    /// [`Self::angular_interval`].
    pub const fn angular_parameter_interval(&self) -> Option<IncreasingParameterInterval> {
        self.angular_parameter_interval
    }
    /// Return the admitted parameter interval, with the guarantee of
    /// [`Self::angular_interval`].
    pub const fn parameter_interval(&self) -> Option<IncreasingParameterInterval> {
        self.parameter_interval
    }
    /// Return the transposed.
    pub fn transposed(&self) -> &bool {
        &self.transposed
    }
    /// Return the revision form.
    pub const fn revision_form(&self) -> Option<&RevisionSurfaceForm<Vec<bool>, FiniteReal>> {
        self.cache.form()
    }
}

impl TryFrom<RevolutionSurfaceConstructionWire> for RevolutionSurfaceConstruction {
    type Error = ProceduralGeometryError;
    fn try_from(wire: RevolutionSurfaceConstructionWire) -> Result<Self, Self::Error> {
        Self::try_new(
            wire.directrix,
            admit_revolution_axis(wire.axis_origin, wire.axis_direction)?,
            wire.angular_interval,
            wire.angular_parameter_interval,
            wire.parameter_interval,
            wire.transposed,
            wire.cache,
        )
    }
}

/// Admitted offset surface parameters.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[cfg_attr(feature = "schema", schemars(with = "OffsetSurfaceConstructionWire"))]
#[serde(try_from = "OffsetSurfaceConstructionWire")]
pub struct OffsetSurfaceConstruction {
    /// Surface this surface is offset from.
    support: SurfaceId,
    /// Signed offset distance, in document length units.
    distance: FiniteReal,
    /// Native U parameter-direction sense enum, when carried.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_u_sense"
    )]
    u_sense: Option<i64>,
    /// Native V parameter-direction sense enum, when carried.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_v_sense"
    )]
    v_sense: Option<i64>,
    /// Whether the support continues as ruled linear strips outside its
    /// active NURBS rectangle.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    linear_support_extension: bool,
    /// Legacy conditional extension flags or the revision-gated form.
    extension: OffsetExtension<FiniteReal>,
}

#[derive(Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(deny_unknown_fields)]
struct OffsetSurfaceConstructionWire {
    /// Surface this surface is offset from.
    support: SurfaceId,
    /// Signed offset distance, in document length units.
    distance: f64,
    /// Native U parameter-direction sense enum, when carried.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_u_sense"
    )]
    u_sense: Option<i64>,
    /// Native V parameter-direction sense enum, when carried.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_v_sense"
    )]
    v_sense: Option<i64>,
    /// Whether the support continues as ruled linear strips outside its
    /// active NURBS rectangle.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    linear_support_extension: bool,
    /// Legacy conditional extension flags or the revision-gated form.
    extension: OffsetExtension,
}

impl OffsetSurfaceConstruction {
    /// Replace the support identity.
    pub fn set_support(&mut self, support: SurfaceId) {
        self.support = support;
    }
    pub(super) fn write_revision_fit_tolerance(
        &mut self,
        value: super::FitTolerance,
        write: super::ToleranceWrite,
    ) -> super::RevisionCacheWrite {
        super::write_revision_form_tolerance(
            match &mut self.extension {
                OffsetExtension::Revision { form } => Some(&mut form.cache),
                OffsetExtension::Legacy { .. } => None,
            },
            value,
            write,
        )
    }

    /// Admit the construction parameters.
    pub fn try_new(
        support: SurfaceId,
        distance: f64,
        u_sense: Option<i64>,
        v_sense: Option<i64>,
        linear_support_extension: bool,
        extension: OffsetExtension,
    ) -> Result<Self, ProceduralGeometryError> {
        let extension = extension.admit().ok_or(ProceduralGeometryError::Payload(
            "Offset.extension is invalid",
        ))?;
        Ok(Self {
            support,
            distance: FiniteReal::new(distance).ok_or(ProceduralGeometryError::Payload(
                "Offset.distance is not finite",
            ))?,
            u_sense,
            v_sense,
            linear_support_extension,
            extension,
        })
    }
    /// Build an offset with a legacy extension from admitted parts. The
    /// distance type states finiteness, and a legacy extension has no
    /// condition of its own, so nothing is checked.
    #[must_use]
    pub const fn legacy(
        support: SurfaceId,
        distance: FiniteReal,
        u_sense: Option<i64>,
        v_sense: Option<i64>,
        linear_support_extension: bool,
        flags: super::LegacyExtensionFlags,
        cache: Option<LegacyCache>,
    ) -> Self {
        Self {
            support,
            distance,
            u_sense,
            v_sense,
            linear_support_extension,
            extension: OffsetExtension::Legacy { flags, cache },
        }
    }
    /// Return the support.
    pub fn support(&self) -> &SurfaceId {
        &self.support
    }
    /// Return the distance.
    pub fn distance(&self) -> FiniteReal {
        self.distance
    }
    /// Return the u sense.
    pub fn u_sense(&self) -> &Option<i64> {
        &self.u_sense
    }
    /// Return the v sense.
    pub fn v_sense(&self) -> &Option<i64> {
        &self.v_sense
    }
    /// Return whether the support extends as ruled linear strips.
    pub fn linear_support_extension(&self) -> bool {
        self.linear_support_extension
    }
    /// Return the extension.
    pub fn extension(&self) -> &OffsetExtension<FiniteReal> {
        &self.extension
    }
}

impl TryFrom<OffsetSurfaceConstructionWire> for OffsetSurfaceConstruction {
    type Error = ProceduralGeometryError;
    fn try_from(wire: OffsetSurfaceConstructionWire) -> Result<Self, Self::Error> {
        Self::try_new(
            wire.support,
            wire.distance,
            wire.u_sense,
            wire.v_sense,
            wire.linear_support_extension,
            wire.extension,
        )
    }
}

/// Admitted subset surface parameters.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[cfg_attr(feature = "schema", schemars(with = "SubsetSurfaceConstructionWire"))]
#[serde(try_from = "SubsetSurfaceConstructionWire")]
pub struct SubsetSurfaceConstruction {
    /// Surface being restricted.
    support: SurfaceId,
    /// U and V parameter endpoints in the support parameterization.
    ///
    /// The endpoint order is significant for cyclic and reversed
    /// trims. A producer that does not carry direction metadata may
    /// leave the sense fields absent and use increasing endpoints.
    parameter_ranges: [DirectedParameterRange; 2],
    /// Whether the trimmed surface U direction agrees with the support.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_subset_surface_construction_u_sense"
    )]
    u_sense: Option<bool>,
    /// Whether the trimmed surface V direction agrees with the support.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_subset_surface_construction_v_sense"
    )]
    v_sense: Option<bool>,
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
struct SubsetSurfaceConstructionWire {
    /// Surface being restricted.
    support: SurfaceId,
    /// U and V parameter endpoints in the support parameterization.
    ///
    /// The endpoint order is significant for cyclic and reversed
    /// trims. A producer that does not carry direction metadata may
    /// leave the sense fields absent and use increasing endpoints.
    parameter_ranges: [[f64; 2]; 2],
    /// Whether the trimmed surface U direction agrees with the support.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_subset_surface_construction_u_sense"
    )]
    u_sense: Option<bool>,
    /// Whether the trimmed surface V direction agrees with the support.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_subset_surface_construction_v_sense"
    )]
    v_sense: Option<bool>,
    /// Solved-cache fit contract this construction states itself.
    #[serde(default, deserialize_with = "deserialize_cache")]
    cache: Option<LegacyCache>,
}

impl SubsetSurfaceConstruction {
    /// Admit the construction parameters.
    pub fn try_new(
        support: SurfaceId,
        parameter_ranges: [[f64; 2]; 2],
        u_sense: Option<bool>,
        v_sense: Option<bool>,
        cache: Option<LegacyCache>,
    ) -> Result<Self, ProceduralGeometryError> {
        Ok(Self {
            cache,
            support,
            parameter_ranges: [
                DirectedParameterRange::new(parameter_ranges[0]).map_err(|_| {
                    ProceduralGeometryError::Payload(
                        "surface subset ranges are not finite and non-zero",
                    )
                })?,
                DirectedParameterRange::new(parameter_ranges[1]).map_err(|_| {
                    ProceduralGeometryError::Payload(
                        "surface subset ranges are not finite and non-zero",
                    )
                })?,
            ],
            u_sense,
            v_sense,
        })
    }
    /// Return the support.
    pub fn support(&self) -> &SurfaceId {
        &self.support
    }
    /// Return the parameter ranges.
    pub fn parameter_ranges(&self) -> [DirectedParameterRange; 2] {
        self.parameter_ranges
    }
    /// Return the u sense.
    pub fn u_sense(&self) -> &Option<bool> {
        &self.u_sense
    }
    /// Return the v sense.
    pub fn v_sense(&self) -> &Option<bool> {
        &self.v_sense
    }
}

impl TryFrom<SubsetSurfaceConstructionWire> for SubsetSurfaceConstruction {
    type Error = ProceduralGeometryError;
    fn try_from(wire: SubsetSurfaceConstructionWire) -> Result<Self, Self::Error> {
        Self::try_new(
            wire.support,
            wire.parameter_ranges,
            wire.u_sense,
            wire.v_sense,
            wire.cache,
        )
    }
}

/// Admitted parallel offset surface parameters.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[cfg_attr(
    feature = "schema",
    schemars(with = "ParallelOffsetSurfaceConstructionWire")
)]
#[serde(try_from = "ParallelOffsetSurfaceConstructionWire")]
pub struct ParallelOffsetSurfaceConstruction {
    /// Surface being offset.
    support: SurfaceId,
    /// Signed offset distance.
    distance: FiniteReal,
    /// Whether the source classifies the result as self-intersecting.
    self_intersect: Option<bool>,
}

#[derive(Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(deny_unknown_fields)]
struct ParallelOffsetSurfaceConstructionWire {
    /// Surface being offset.
    support: SurfaceId,
    /// Signed offset distance.
    distance: f64,
    /// Whether the source classifies the result as self-intersecting.
    #[serde(deserialize_with = "cadmpeg_core::absent_key::nullable")]
    self_intersect: Option<bool>,
}

impl ParallelOffsetSurfaceConstruction {
    /// Admit the construction parameters.
    pub fn try_new(
        support: SurfaceId,
        distance: f64,
        self_intersect: Option<bool>,
    ) -> Result<Self, ProceduralGeometryError> {
        Ok(Self {
            support,
            distance: FiniteReal::new(distance).ok_or(ProceduralGeometryError::Payload(
                "ParallelOffset.distance is not finite",
            ))?,
            self_intersect,
        })
    }
    /// Return the support.
    pub fn support(&self) -> &SurfaceId {
        &self.support
    }
    /// Return the distance.
    pub fn distance(&self) -> FiniteReal {
        self.distance
    }
    /// Return the self intersect.
    pub fn self_intersect(&self) -> &Option<bool> {
        &self.self_intersect
    }
}

impl TryFrom<ParallelOffsetSurfaceConstructionWire> for ParallelOffsetSurfaceConstruction {
    type Error = ProceduralGeometryError;
    fn try_from(wire: ParallelOffsetSurfaceConstructionWire) -> Result<Self, Self::Error> {
        Self::try_new(wire.support, wire.distance, wire.self_intersect)
    }
}

/// A finite sweep vector whose norm exceeds machine epsilon.
#[derive(Debug, Clone, Copy, PartialEq, Serialize)]
struct SweepDirectionAboveEpsilon(FiniteVector3);

impl SweepDirectionAboveEpsilon {
    fn new(direction: Vector3) -> Option<Self> {
        let direction = FiniteVector3::new(direction)?;
        (direction.as_raw().norm() > f64::EPSILON).then_some(Self(direction))
    }
}

/// Admitted unbounded linear sweep parameters.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[cfg_attr(
    feature = "schema",
    schemars(with = "LinearSweepSurfaceConstructionWire")
)]
#[serde(try_from = "LinearSweepSurfaceConstructionWire")]
pub struct LinearSweepSurfaceConstruction {
    /// Curve swept along `direction`.
    directrix: CurveId,
    /// Length-bearing sweep vector.
    direction: SweepDirectionAboveEpsilon,
}

#[derive(Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(deny_unknown_fields)]
struct LinearSweepSurfaceConstructionWire {
    /// Curve swept along `direction`.
    directrix: CurveId,
    /// Length-bearing sweep vector.
    direction: Vector3,
}

impl LinearSweepSurfaceConstruction {
    /// Admit the construction parameters.
    pub fn try_new(
        directrix: CurveId,
        direction: Vector3,
    ) -> Result<Self, ProceduralGeometryError> {
        let direction = SweepDirectionAboveEpsilon::new(direction).ok_or(
            ProceduralGeometryError::Payload("invalid linear-sweep direction"),
        )?;
        Ok(Self {
            directrix,
            direction,
        })
    }
    /// Return the directrix.
    pub fn directrix(&self) -> &CurveId {
        &self.directrix
    }
    /// Return the direction.
    pub fn direction(&self) -> &FiniteVector3 {
        &self.direction.0
    }
}

impl TryFrom<LinearSweepSurfaceConstructionWire> for LinearSweepSurfaceConstruction {
    type Error = ProceduralGeometryError;
    fn try_from(wire: LinearSweepSurfaceConstructionWire) -> Result<Self, Self::Error> {
        Self::try_new(wire.directrix, wire.direction)
    }
}

/// Admitted full axis revolution parameters.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[cfg_attr(
    feature = "schema",
    schemars(with = "AxisRevolutionSurfaceConstructionWire")
)]
#[serde(try_from = "AxisRevolutionSurfaceConstructionWire")]
pub struct AxisRevolutionSurfaceConstruction {
    /// Curve revolved about the axis.
    directrix: CurveId,
    /// Point on the revolution axis.
    axis_origin: FinitePoint3,
    /// Unit revolution-axis direction.
    axis_direction: UnitVector3,
}

#[derive(Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(deny_unknown_fields)]
struct AxisRevolutionSurfaceConstructionWire {
    /// Curve revolved about the axis.
    directrix: CurveId,
    /// Point on the revolution axis.
    axis_origin: Point3,
    /// Unit revolution-axis direction.
    axis_direction: Vector3,
}

impl AxisRevolutionSurfaceConstruction {
    /// Admit the construction parameters.
    pub fn try_new(
        directrix: CurveId,
        axis_origin: Point3,
        axis_direction: Vector3,
    ) -> Result<Self, ProceduralGeometryError> {
        let (axis_origin, axis_direction) = admit_revolution_axis(axis_origin, axis_direction)?;
        Ok(Self {
            directrix,
            axis_origin,
            axis_direction,
        })
    }
    /// Return the directrix.
    pub fn directrix(&self) -> &CurveId {
        &self.directrix
    }
    /// Return the admitted axis origin.
    #[must_use]
    pub const fn axis_origin(&self) -> FinitePoint3 {
        self.axis_origin
    }
    /// Replace the axis origin and keep the admitted directrix and direction.
    pub fn set_axis_origin(&mut self, axis_origin: FinitePoint3) {
        self.axis_origin = axis_origin;
    }
    /// Return the admitted unit axis direction. A caller that passes it on
    /// keeps the unit-length guarantee and performs no new admission.
    pub const fn axis_direction(&self) -> UnitVector3 {
        self.axis_direction
    }
}

impl TryFrom<AxisRevolutionSurfaceConstructionWire> for AxisRevolutionSurfaceConstruction {
    type Error = ProceduralGeometryError;
    fn try_from(wire: AxisRevolutionSurfaceConstructionWire) -> Result<Self, Self::Error> {
        Self::try_new(wire.directrix, wire.axis_origin, wire.axis_direction)
    }
}

/// Admitted ordered curve sum parameters.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[cfg_attr(feature = "schema", schemars(with = "SumSurfaceConstructionWire"))]
#[serde(try_from = "SumSurfaceConstructionWire")]
pub struct SumSurfaceConstruction {
    /// First curve, varying in the first surface parameter.
    first: CurveId,
    /// Second curve, varying in the second surface parameter.
    second: CurveId,
    /// Surface base point.
    basepoint: FiniteVector3,
    /// Cache contract: the revision-gated form, or the legacy
    /// solved-cache tolerance this construction states instead.
    /// The first curve's optional endpoints are `reference_endpoints`
    /// and the second curve's are `second_endpoints`.
    #[serde(default, skip_serializing_if = "CacheContract::is_bare_legacy")]
    cache: CacheContract<RevisionSurfaceForm<Vec<bool>, FiniteReal>>,
}

#[derive(Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(deny_unknown_fields)]
struct SumSurfaceConstructionWire {
    /// First curve, varying in the first surface parameter.
    first: CurveId,
    /// Second curve, varying in the second surface parameter.
    second: CurveId,
    /// Surface base point.
    basepoint: Vector3,
    /// Cache contract: the revision-gated form, or the legacy
    /// solved-cache tolerance this construction states instead.
    #[serde(default)]
    cache: CacheContract<RevisionSurfaceForm>,
}

impl SumSurfaceConstruction {
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
        first: CurveId,
        second: CurveId,
        basepoint: Vector3,
        cache: CacheContract<RevisionSurfaceForm>,
    ) -> Result<Self, ProceduralGeometryError> {
        let cache = cache.admit_form(RevisionSurfaceForm::admit).ok_or(
            ProceduralGeometryError::Payload("sum cache form is invalid"),
        )?;
        Ok(Self {
            first,
            second,
            basepoint: FiniteVector3::new(basepoint).ok_or(ProceduralGeometryError::Payload(
                "sum basepoint must be finite",
            ))?,
            cache,
        })
    }
    /// Return the first curve.
    pub fn first(&self) -> &CurveId {
        &self.first
    }
    /// Return the second curve.
    pub fn second(&self) -> &CurveId {
        &self.second
    }
    /// Return the basepoint.
    pub fn basepoint(&self) -> &FiniteVector3 {
        &self.basepoint
    }
    /// Replace the basepoint and keep every other admitted field. A finite
    /// basepoint is the whole basepoint condition, so nothing is checked.
    pub fn set_basepoint(&mut self, basepoint: FiniteVector3) {
        self.basepoint = basepoint;
    }
    /// Return the revision form.
    pub const fn revision_form(&self) -> Option<&RevisionSurfaceForm<Vec<bool>, FiniteReal>> {
        self.cache.form()
    }
}

impl TryFrom<SumSurfaceConstructionWire> for SumSurfaceConstruction {
    type Error = ProceduralGeometryError;
    fn try_from(wire: SumSurfaceConstructionWire) -> Result<Self, Self::Error> {
        Self::try_new(wire.first, wire.second, wire.basepoint, wire.cache)
    }
}

/// Admitted exact surface construction.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[cfg_attr(feature = "schema", schemars(with = "ExactSurfacePayloadWire"))]
#[serde(try_from = "ExactSurfacePayloadWire")]
pub struct ExactSurfacePayload {
    spline: ExactSpline<FiniteReal>,
}
#[derive(Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(deny_unknown_fields)]
struct ExactSurfacePayloadWire {
    /// Exact spline surface.
    spline: ExactSpline,
}
impl ExactSurfacePayload {
    /// Admit the construction parameters.
    pub fn try_new(spline: ExactSpline) -> Result<Self, ProceduralGeometryError> {
        let spline = spline
            .admit()
            .filter(|spline| match spline {
                ExactSpline::Legacy { ranges, .. } => ranges.iter().all(ordered),
                ExactSpline::Revision { .. } => true,
            })
            .ok_or(ProceduralGeometryError::Payload(
                "exact spline surface parameter fields are invalid",
            ))?;
        Ok(Self { spline })
    }
    /// Return the spline.
    pub fn spline(&self) -> &ExactSpline<FiniteReal> {
        &self.spline
    }
}
impl TryFrom<ExactSurfacePayloadWire> for ExactSurfacePayload {
    type Error = ProceduralGeometryError;
    fn try_from(wire: ExactSurfacePayloadWire) -> Result<Self, Self::Error> {
        Self::try_new(wire.spline)
    }
}

/// Admitted compound surface construction.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[cfg_attr(feature = "schema", schemars(with = "CompoundSurfacePayloadWire"))]
#[serde(try_from = "CompoundSurfacePayloadWire")]
pub struct CompoundSurfacePayload {
    components: Vec<CompoundComponent<SurfaceId, FiniteReal>>,
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
struct CompoundSurfacePayloadWire {
    /// Component surfaces in construction order.
    components: Vec<CompoundComponent<SurfaceId>>,
    /// Solved-cache fit contract this construction states itself.
    #[serde(default, deserialize_with = "deserialize_cache")]
    cache: Option<LegacyCache>,
}
impl CompoundSurfacePayload {
    /// Admit the construction parameters.
    pub fn try_new(
        components: Vec<CompoundComponent<SurfaceId>>,
        cache: Option<LegacyCache>,
    ) -> Result<Self, ProceduralGeometryError> {
        let components = components
            .into_iter()
            .map(CompoundComponent::admit)
            .collect::<Option<Vec<_>>>()
            .ok_or(ProceduralGeometryError::Payload(
                "compound surface parameters and components are inconsistent",
            ))?;
        Ok(Self { components, cache })
    }
    /// Return the components.
    pub fn components(&self) -> &Vec<CompoundComponent<SurfaceId, FiniteReal>> {
        &self.components
    }
}
impl TryFrom<CompoundSurfacePayloadWire> for CompoundSurfacePayload {
    type Error = ProceduralGeometryError;
    fn try_from(wire: CompoundSurfacePayloadWire) -> Result<Self, Self::Error> {
        Self::try_new(wire.components, wire.cache)
    }
}

/// Admitted loft surface construction.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[cfg_attr(feature = "schema", schemars(with = "LoftSurfacePayloadWire"))]
#[serde(try_from = "LoftSurfacePayloadWire")]
pub struct LoftSurfacePayload {
    sections: [LoftSection<FiniteReal, FiniteVector3>; 2],

    parameters: SplineSurfaceParameters<FiniteReal>,

    closures: [i64; 2],

    singularities: [i64; 2],

    mode: i64,

    bridge: Vec<LoftBridgeToken<FiniteReal>>,

    #[serde(default, skip_serializing_if = "CacheContract::is_bare_legacy")]
    cache: CacheContract<LoftRevisionForm<FiniteReal>>,
}
#[derive(Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(deny_unknown_fields)]
struct LoftSurfacePayloadWire {
    /// The two loft sections.
    sections: [LoftSection; 2],

    /// Spline surface parameters.
    parameters: SplineSurfaceParameters,

    /// Native closure codes of the two parameter directions.
    closures: [i64; 2],

    /// Native singularity codes of the two parameter directions.
    singularities: [i64; 2],

    /// Native loft mode code.
    mode: i64,

    /// Native bridge tokens in source order.
    bridge: Vec<LoftBridgeToken>,

    /// Cache contract: the revision-gated loft form, or the legacy solved-cache tolerance this construction states instead.
    #[serde(default)]
    cache: CacheContract<LoftRevisionForm>,
}
impl LoftSurfacePayload {
    /// Admit the construction parameters.
    pub fn try_new(
        sections: [LoftSection; 2],
        parameters: SplineSurfaceParameters,
        closures: [i64; 2],
        singularities: [i64; 2],
        mode: i64,
        bridge: Vec<LoftBridgeToken>,
        cache: CacheContract<LoftRevisionForm>,
    ) -> Result<Self, ProceduralGeometryError> {
        let [first, second] = sections;
        let (Some(parameters), Some(first), Some(second), Some(cache), Some(bridge)) = (
            parameters.admit().filter(|parameters| match parameters {
                SplineSurfaceParameters::OrderedRanges { ranges } => ranges.iter().all(ordered),
                SplineSurfaceParameters::RevisionRanges { .. } => true,
            }),
            first.admit(),
            second.admit(),
            cache.admit_form(LoftRevisionForm::admit),
            bridge
                .into_iter()
                .map(LoftBridgeToken::admit)
                .collect::<Option<Vec<_>>>(),
        ) else {
            return Err(ProceduralGeometryError::Payload(
                "loft construction payload is invalid",
            ));
        };
        let sections = [first, second];
        Ok(Self {
            sections,
            parameters,
            closures,
            singularities,
            mode,
            bridge,
            cache,
        })
    }
    /// Return the sections.
    pub fn sections(&self) -> &[LoftSection<FiniteReal, FiniteVector3>; 2] {
        &self.sections
    }
    /// Return the parameters.
    pub fn parameters(&self) -> &SplineSurfaceParameters<FiniteReal> {
        &self.parameters
    }
    /// Return the closures.
    pub fn closures(&self) -> &[i64; 2] {
        &self.closures
    }
    /// Return the singularities.
    pub fn singularities(&self) -> &[i64; 2] {
        &self.singularities
    }
    /// Return the mode.
    pub fn mode(&self) -> &i64 {
        &self.mode
    }
    /// Return the bridge.
    pub fn bridge(&self) -> &Vec<LoftBridgeToken<FiniteReal>> {
        &self.bridge
    }
    /// Return the revision form.
    pub const fn revision_form(&self) -> Option<&LoftRevisionForm<FiniteReal>> {
        self.cache.form()
    }
}
impl TryFrom<LoftSurfacePayloadWire> for LoftSurfacePayload {
    type Error = ProceduralGeometryError;
    fn try_from(wire: LoftSurfacePayloadWire) -> Result<Self, Self::Error> {
        Self::try_new(
            wire.sections,
            wire.parameters,
            wire.closures,
            wire.singularities,
            wire.mode,
            wire.bridge,
            wire.cache,
        )
    }
}

/// Admitted compound loft surface construction.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[cfg_attr(feature = "schema", schemars(with = "CompoundLoftSurfacePayloadWire"))]
#[serde(try_from = "CompoundLoftSurfacePayloadWire")]
pub struct CompoundLoftSurfacePayload {
    construction: Box<CompoundLoftConstruction<FiniteReal, FiniteVector3>>,
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
struct CompoundLoftSurfacePayloadWire {
    /// Compound loft construction.
    construction: Box<CompoundLoftConstruction>,
    /// Solved-cache fit contract this construction states itself.
    #[serde(default, deserialize_with = "deserialize_cache")]
    cache: Option<LegacyCache>,
}
impl CompoundLoftSurfacePayload {
    /// Admit the construction parameters.
    pub fn try_new(
        construction: CompoundLoftConstruction,
        cache: Option<LegacyCache>,
    ) -> Result<Self, ProceduralGeometryError> {
        let construction = construction
            .admit()
            .filter(|construction| match &construction.tail {
                CompoundLoftTail::Six {
                    parameter_range, ..
                } => ordered(parameter_range),
                CompoundLoftTail::Seven { .. } | CompoundLoftTail::Zero { .. } => true,
            })
            .ok_or(ProceduralGeometryError::Payload(
                "compound loft construction payload is invalid",
            ))?;
        Ok(Self {
            construction: Box::new(construction),
            cache,
        })
    }
    /// Return the construction.
    pub fn construction(&self) -> &CompoundLoftConstruction<FiniteReal, FiniteVector3> {
        &self.construction
    }
}
impl TryFrom<CompoundLoftSurfacePayloadWire> for CompoundLoftSurfacePayload {
    type Error = ProceduralGeometryError;
    fn try_from(wire: CompoundLoftSurfacePayloadWire) -> Result<Self, Self::Error> {
        Self::try_new(*wire.construction, wire.cache)
    }
}

/// Admitted scaled compound loft surface construction.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[cfg_attr(
    feature = "schema",
    schemars(with = "ScaledCompoundLoftSurfacePayloadWire")
)]
#[serde(try_from = "ScaledCompoundLoftSurfacePayloadWire")]
pub struct ScaledCompoundLoftSurfacePayload {
    construction: Box<ScaledCompoundLoftConstruction<FiniteReal, FiniteVector3>>,
    /// Solved-cache fit contract this construction states itself.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    cache: Option<LegacyCache>,
}
#[derive(Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(deny_unknown_fields)]
struct ScaledCompoundLoftSurfacePayloadWire {
    /// Scaled compound loft construction.
    construction: Box<ScaledCompoundLoftConstruction>,
    /// Solved-cache fit contract this construction states itself.
    #[serde(default, deserialize_with = "deserialize_cache")]
    cache: Option<LegacyCache>,
}
impl ScaledCompoundLoftSurfacePayload {
    /// Admit the construction parameters.
    pub fn try_new(
        construction: Box<ScaledCompoundLoftConstruction>,
        cache: Option<LegacyCache>,
    ) -> Result<Self, ProceduralGeometryError> {
        let construction = (*construction)
            .admit()
            .filter(|construction| match &construction.shape {
                ScaledCompoundLoftShape::Full {} => true,
                ScaledCompoundLoftShape::None {
                    parameter_ranges, ..
                } => parameter_ranges.iter().all(ordered),
            })
            .ok_or(ProceduralGeometryError::Payload(
                "scaled compound loft construction payload is invalid",
            ))?;
        Ok(Self {
            construction: Box::new(construction),
            cache,
        })
    }
    /// Return the construction.
    pub fn construction(&self) -> &ScaledCompoundLoftConstruction<FiniteReal, FiniteVector3> {
        &self.construction
    }
}
impl TryFrom<ScaledCompoundLoftSurfacePayloadWire> for ScaledCompoundLoftSurfacePayload {
    type Error = ProceduralGeometryError;
    fn try_from(wire: ScaledCompoundLoftSurfacePayloadWire) -> Result<Self, Self::Error> {
        Self::try_new(wire.construction, wire.cache)
    }
}

/// Admitted law surface construction.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[cfg_attr(feature = "schema", schemars(with = "LawSurfacePayloadWire"))]
#[serde(try_from = "LawSurfacePayloadWire")]
pub struct LawSurfacePayload {
    construction: Box<LawSurfaceConstruction<FiniteReal, FiniteVector3, FinitePoint3>>,
}
#[derive(Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(deny_unknown_fields)]
struct LawSurfacePayloadWire {
    /// Law surface construction.
    construction: Box<LawSurfaceConstruction>,
}
impl LawSurfacePayload {
    /// Admit the construction parameters.
    pub fn try_new(
        construction: Box<LawSurfaceConstruction>,
    ) -> Result<Self, ProceduralGeometryError> {
        let construction = (*construction)
            .admit()
            .ok_or(ProceduralGeometryError::Payload(
                "law surface construction payload is invalid",
            ))?;
        Ok(Self {
            construction: Box::new(construction),
        })
    }
    /// Return the construction.
    pub fn construction(&self) -> &LawSurfaceConstruction<FiniteReal, FiniteVector3, FinitePoint3> {
        &self.construction
    }
}
impl TryFrom<LawSurfacePayloadWire> for LawSurfacePayload {
    type Error = ProceduralGeometryError;
    fn try_from(wire: LawSurfacePayloadWire) -> Result<Self, Self::Error> {
        Self::try_new(wire.construction)
    }
}

/// Admitted skin surface construction.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[cfg_attr(feature = "schema", schemars(with = "SkinSurfacePayloadWire"))]
#[serde(try_from = "SkinSurfacePayloadWire")]
pub struct SkinSurfacePayload {
    construction: Box<SkinSurfaceConstruction<FiniteReal, FiniteVector3, FinitePoint3>>,
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
struct SkinSurfacePayloadWire {
    /// Skin surface construction.
    construction: Box<SkinSurfaceConstruction>,
    /// Solved-cache fit contract this construction states itself.
    #[serde(default, deserialize_with = "deserialize_cache")]
    cache: Option<LegacyCache>,
}
impl SkinSurfacePayload {
    /// Admit the construction parameters.
    pub fn try_new(
        construction: Box<SkinSurfaceConstruction>,
        cache: Option<LegacyCache>,
    ) -> Result<Self, ProceduralGeometryError> {
        let construction = (*construction)
            .admit()
            .ok_or(ProceduralGeometryError::Payload(
                "skin surface construction payload is invalid",
            ))?;
        Ok(Self {
            construction: Box::new(construction),
            cache,
        })
    }
    /// Return the construction.
    pub fn construction(
        &self,
    ) -> &SkinSurfaceConstruction<FiniteReal, FiniteVector3, FinitePoint3> {
        &self.construction
    }
}
impl TryFrom<SkinSurfacePayloadWire> for SkinSurfacePayload {
    type Error = ProceduralGeometryError;
    fn try_from(wire: SkinSurfacePayloadWire) -> Result<Self, Self::Error> {
        Self::try_new(wire.construction, wire.cache)
    }
}

/// Admitted net surface construction.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[cfg_attr(feature = "schema", schemars(with = "NetSurfacePayloadWire"))]
#[serde(try_from = "NetSurfacePayloadWire")]
pub struct NetSurfacePayload {
    construction: Box<NetSurfaceConstruction<FiniteReal, FiniteVector3, FinitePoint3>>,
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
struct NetSurfacePayloadWire {
    /// Net surface construction.
    construction: Box<NetSurfaceConstruction>,
    /// Solved-cache fit contract this construction states itself.
    #[serde(default, deserialize_with = "deserialize_cache")]
    cache: Option<LegacyCache>,
}
impl NetSurfacePayload {
    /// Admit the construction parameters.
    pub fn try_new(
        construction: Box<NetSurfaceConstruction>,
        cache: Option<LegacyCache>,
    ) -> Result<Self, ProceduralGeometryError> {
        let construction = (*construction)
            .admit()
            .ok_or(ProceduralGeometryError::Payload(
                "net surface construction payload is invalid",
            ))?;
        Ok(Self {
            construction: Box::new(construction),
            cache,
        })
    }
    /// Return the construction.
    pub fn construction(&self) -> &NetSurfaceConstruction<FiniteReal, FiniteVector3, FinitePoint3> {
        &self.construction
    }
}
impl TryFrom<NetSurfacePayloadWire> for NetSurfacePayload {
    type Error = ProceduralGeometryError;
    fn try_from(wire: NetSurfacePayloadWire) -> Result<Self, Self::Error> {
        Self::try_new(wire.construction, wire.cache)
    }
}

/// Admitted sweep surface construction.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[cfg_attr(feature = "schema", schemars(with = "SweepSurfacePayloadWire"))]
#[serde(try_from = "SweepSurfacePayloadWire")]
pub struct SweepSurfacePayload {
    profile: CurveId,

    spine: CurveId,

    #[serde(default, skip_serializing_if = "Option::is_none")]
    native: Option<Box<SweepSurfaceConstruction<FiniteReal, FiniteVector3, FinitePoint3>>>,
}
#[derive(Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(deny_unknown_fields)]
struct SweepSurfacePayloadWire {
    /// Curve swept along the spine.
    profile: CurveId,

    /// Curve the profile follows.
    spine: CurveId,

    /// Native sweep construction, when the source carries one.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_native"
    )]
    native: Option<Box<SweepSurfaceConstruction>>,
}
impl SweepSurfacePayload {
    /// Admit the construction parameters.
    pub fn try_new(
        profile: CurveId,
        spine: CurveId,
        native: Option<Box<SweepSurfaceConstruction>>,
    ) -> Result<Self, ProceduralGeometryError> {
        let native = match native {
            None => None,
            Some(construction) => Some(Box::new((*construction).admit().ok_or(
                ProceduralGeometryError::Payload("sweep surface construction payload is invalid"),
            )?)),
        };
        Ok(Self {
            profile,
            spine,
            native,
        })
    }
    /// Return the profile.
    pub fn profile(&self) -> &CurveId {
        &self.profile
    }
    /// Return the spine.
    pub fn spine(&self) -> &CurveId {
        &self.spine
    }
    /// Return the native.
    pub fn native(
        &self,
    ) -> &Option<Box<SweepSurfaceConstruction<FiniteReal, FiniteVector3, FinitePoint3>>> {
        &self.native
    }
}
impl TryFrom<SweepSurfacePayloadWire> for SweepSurfacePayload {
    type Error = ProceduralGeometryError;
    fn try_from(wire: SweepSurfacePayloadWire) -> Result<Self, Self::Error> {
        Self::try_new(wire.profile, wire.spine, wire.native)
    }
}

/// Admitted deformable surface construction.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[cfg_attr(feature = "schema", schemars(with = "DeformableSurfacePayloadWire"))]
#[serde(try_from = "DeformableSurfacePayloadWire")]
pub struct DeformableSurfacePayload {
    construction: Box<DeformableSurfaceConstruction<FiniteReal, FiniteVector3, FinitePoint3>>,
}
#[derive(Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(deny_unknown_fields)]
struct DeformableSurfacePayloadWire {
    /// Deformable surface construction.
    construction: Box<DeformableSurfaceConstruction>,
}
impl DeformableSurfacePayload {
    /// Admit the construction parameters.
    pub fn try_new(
        construction: Box<DeformableSurfaceConstruction>,
    ) -> Result<Self, ProceduralGeometryError> {
        let construction = (*construction)
            .admit()
            .ok_or(ProceduralGeometryError::Payload(
                "deformable surface construction payload is invalid",
            ))?;
        Ok(Self {
            construction: Box::new(construction),
        })
    }
    /// Return the construction.
    pub fn construction(
        &self,
    ) -> &DeformableSurfaceConstruction<FiniteReal, FiniteVector3, FinitePoint3> {
        &self.construction
    }
}
impl TryFrom<DeformableSurfacePayloadWire> for DeformableSurfacePayload {
    type Error = ProceduralGeometryError;
    fn try_from(wire: DeformableSurfacePayloadWire) -> Result<Self, Self::Error> {
        Self::try_new(wire.construction)
    }
}

/// Admitted G2 blend surface construction.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[cfg_attr(feature = "schema", schemars(with = "G2BlendSurfacePayloadWire"))]
#[serde(try_from = "G2BlendSurfacePayloadWire")]
pub struct G2BlendSurfacePayload {
    construction: Box<G2BlendConstruction<FiniteReal, FiniteVector3>>,
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
struct G2BlendSurfacePayloadWire {
    /// G2 blend construction.
    construction: Box<G2BlendConstruction>,
    /// Solved-cache fit contract this construction states itself.
    #[serde(default, deserialize_with = "deserialize_cache")]
    cache: Option<LegacyCache>,
}
impl G2BlendSurfacePayload {
    /// Admit the construction parameters.
    pub fn try_new(
        construction: Box<G2BlendConstruction>,
        cache: Option<LegacyCache>,
    ) -> Result<Self, ProceduralGeometryError> {
        let construction = (*construction)
            .admit()
            .filter(|construction| construction.parameter_ranges.iter().all(ordered))
            .ok_or(ProceduralGeometryError::Payload(
                "G2 blend construction payload is invalid",
            ))?;
        Ok(Self {
            construction: Box::new(construction),
            cache,
        })
    }
    /// Return the construction.
    pub fn construction(&self) -> &G2BlendConstruction<FiniteReal, FiniteVector3> {
        &self.construction
    }
}
impl TryFrom<G2BlendSurfacePayloadWire> for G2BlendSurfacePayload {
    type Error = ProceduralGeometryError;
    fn try_from(wire: G2BlendSurfacePayloadWire) -> Result<Self, Self::Error> {
        Self::try_new(wire.construction, wire.cache)
    }
}

/// Admitted variable blend surface construction.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[cfg_attr(feature = "schema", schemars(with = "VariableBlendSurfacePayloadWire"))]
#[serde(try_from = "VariableBlendSurfacePayloadWire")]
pub struct VariableBlendSurfacePayload {
    construction: Box<VariableBlendConstruction<FiniteReal, FiniteVector3, FinitePoint3>>,
    #[serde(skip)]
    slice_range: OrderedOptionalRange,
}
#[derive(Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(deny_unknown_fields)]
struct VariableBlendSurfacePayloadWire {
    /// Variable blend construction.
    construction: Box<VariableBlendConstruction>,
}
impl VariableBlendSurfacePayload {
    /// Admit the construction parameters.
    pub fn try_new(
        construction: Box<VariableBlendConstruction>,
    ) -> Result<Self, ProceduralGeometryError> {
        const INVALID: &str = "variable blend construction payload is invalid";
        let construction = (*construction)
            .admit()
            .ok_or(ProceduralGeometryError::Payload(INVALID))?;
        if !ordered(&construction.u_range) || !optional_ordered(&construction.post_range) {
            return Err(ProceduralGeometryError::Payload(INVALID));
        }
        let slice_range = OrderedOptionalRange::new(construction.slice_range)
            .ok_or(ProceduralGeometryError::Payload(INVALID))?;
        if construction
            .secondary_curve
            .as_ref()
            .is_some_and(|curve| !optional_ordered(&curve.parameter_range))
        {
            return Err(ProceduralGeometryError::Payload(INVALID));
        }
        Ok(Self {
            construction: Box::new(construction),
            slice_range,
        })
    }
    /// Return the construction.
    pub fn construction(
        &self,
    ) -> &VariableBlendConstruction<FiniteReal, FiniteVector3, FinitePoint3> {
        &self.construction
    }

    /// Return the admitted slice interval.
    pub(crate) fn slice_range(&self) -> OrderedOptionalRange {
        self.slice_range
    }
}
impl TryFrom<VariableBlendSurfacePayloadWire> for VariableBlendSurfacePayload {
    type Error = ProceduralGeometryError;
    fn try_from(wire: VariableBlendSurfacePayloadWire) -> Result<Self, Self::Error> {
        Self::try_new(wire.construction)
    }
}

/// Admitted vertex blend surface construction.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[cfg_attr(feature = "schema", schemars(with = "VertexBlendSurfacePayloadWire"))]
#[serde(try_from = "VertexBlendSurfacePayloadWire")]
pub struct VertexBlendSurfacePayload {
    construction: Box<VertexBlendConstruction<FiniteReal, FiniteVector3, FinitePoint3>>,
}
#[derive(Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(deny_unknown_fields)]
struct VertexBlendSurfacePayloadWire {
    /// Vertex blend construction.
    construction: Box<VertexBlendConstruction>,
}
impl VertexBlendSurfacePayload {
    /// Admit the construction parameters.
    pub fn try_new(construction: VertexBlendConstruction) -> Result<Self, ProceduralGeometryError> {
        let construction = construction
            .admit()
            .filter(|construction| {
                construction
                    .boundaries
                    .iter()
                    .all(|boundary| match &boundary.geometry {
                        VertexBlendBoundaryGeometry::Degenerate { normals, .. } => {
                            normals.iter().all(|normal| normal.norm() > f64::EPSILON)
                        }
                        VertexBlendBoundaryGeometry::Plane { normal, .. } => {
                            normal.norm() > f64::EPSILON
                        }
                        VertexBlendBoundaryGeometry::Circle { .. }
                        | VertexBlendBoundaryGeometry::Pcurve { .. } => true,
                    })
            })
            .ok_or(ProceduralGeometryError::Payload(
                "vertex blend construction payload is invalid",
            ))?;
        Ok(Self {
            construction: Box::new(construction),
        })
    }
    /// Return the construction.
    pub fn construction(
        &self,
    ) -> &VertexBlendConstruction<FiniteReal, FiniteVector3, FinitePoint3> {
        &self.construction
    }
}
impl TryFrom<VertexBlendSurfacePayloadWire> for VertexBlendSurfacePayload {
    type Error = ProceduralGeometryError;
    fn try_from(wire: VertexBlendSurfacePayloadWire) -> Result<Self, Self::Error> {
        Self::try_new(*wire.construction)
    }
}

/// Optional finite interval endpoints, ordered when both are present.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct OrderedOptionalRange([Option<FiniteReal>; 2]);

impl OrderedOptionalRange {
    fn new(endpoints: [Option<FiniteReal>; 2]) -> Option<Self> {
        optional_ordered(&endpoints).then_some(Self(endpoints))
    }

    pub(crate) fn endpoints(self) -> [Option<FiniteReal>; 2] {
        self.0
    }
}

/// Admitted blend surface construction.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[cfg_attr(feature = "schema", schemars(with = "BlendSurfacePayloadWire"))]
#[serde(try_from = "BlendSurfacePayloadWire")]
pub struct BlendSurfacePayload {
    supports: [Option<BlendSupport>; 2],

    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_spine"
    )]
    spine: Option<CurveId>,

    radius: BlendRadiusLaw,

    cross_section: BlendCrossSection,

    /// Cache contract: the native rolling-ball construction, which states its
    /// own cache form, or the legacy solved-cache tolerance stated instead.
    #[serde(default, skip_serializing_if = "CacheContract::is_bare_legacy")]
    cache: CacheContract<Box<RollingBallConstruction<FiniteReal, FiniteVector3, FinitePoint3>>>,

    #[serde(skip)]
    native_ranges: Option<[OrderedOptionalRange; 2]>,
}
#[derive(Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(deny_unknown_fields)]
struct BlendSurfacePayloadWire {
    /// Blend support sides.
    supports: [Option<BlendSupport>; 2],

    /// Spine curve, when the source resolves one.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_spine"
    )]
    spine: Option<CurveId>,

    /// Blend radius law.
    radius: BlendRadiusLaw,

    /// Blend cross-section form.
    cross_section: BlendCrossSection,

    /// Cache contract: the native rolling-ball construction, which states its
    /// own cache form, or the legacy solved-cache tolerance stated instead.
    #[serde(default)]
    cache: CacheContract<Box<RollingBallConstruction>>,
}
impl BlendSurfacePayload {
    /// Admit the construction parameters.
    pub fn try_new(
        supports: [Option<BlendSupport>; 2],
        spine: Option<CurveId>,
        radius: BlendRadiusLaw,
        cross_section: BlendCrossSection,
        cache: CacheContract<Box<RollingBallConstruction>>,
    ) -> Result<Self, ProceduralGeometryError> {
        let mut native_ranges = None;
        let cache = cache
            .admit_form(|construction| {
                let construction = (*construction).admit()?;
                native_ranges = Some([
                    OrderedOptionalRange::new(construction.u_range)?,
                    OrderedOptionalRange::new(construction.v_range)?,
                ]);
                Some(Box::new(construction))
            })
            .ok_or(ProceduralGeometryError::Payload(
                "rolling-ball blend construction payload is invalid",
            ))?;
        Ok(Self {
            supports,
            spine,
            radius,
            cross_section,
            cache,
            native_ranges,
        })
    }
    /// Return the supports.
    pub fn supports(&self) -> &[Option<BlendSupport>; 2] {
        &self.supports
    }
    /// Return the spine.
    pub fn spine(&self) -> &Option<CurveId> {
        &self.spine
    }
    /// Return the radius.
    pub fn radius(&self) -> &BlendRadiusLaw {
        &self.radius
    }
    /// Return the cross section.
    pub fn cross_section(&self) -> &BlendCrossSection {
        &self.cross_section
    }
    /// Return the native.
    pub fn native(
        &self,
    ) -> Option<&RollingBallConstruction<FiniteReal, FiniteVector3, FinitePoint3>> {
        self.cache.form().map(Box::as_ref)
    }

    /// Return the native construction with its admitted U and V ranges.
    pub(crate) fn native_with_ranges(
        &self,
    ) -> Option<(
        &RollingBallConstruction<FiniteReal, FiniteVector3, FinitePoint3>,
        [OrderedOptionalRange; 2],
    )> {
        self.native().zip(self.native_ranges)
    }
}
impl TryFrom<BlendSurfacePayloadWire> for BlendSurfacePayload {
    type Error = ProceduralGeometryError;
    fn try_from(wire: BlendSurfacePayloadWire) -> Result<Self, Self::Error> {
        Self::try_new(
            wire.supports,
            wire.spine,
            wire.radius,
            wire.cross_section,
            wire.cache,
        )
    }
}

impl ExactSurfacePayload {
    pub(super) fn revision_cache(
        &self,
    ) -> Option<&super::RevisionCacheForm<super::RevisionSurfaceParameterization<FiniteReal>>> {
        match &self.spline {
            ExactSpline::Revision { form, .. } => Some(&form.cache),
            ExactSpline::Legacy { .. } => None,
        }
    }
    pub(super) fn write_revision_fit_tolerance(
        &mut self,
        value: super::FitTolerance,
        write: super::ToleranceWrite,
    ) -> super::RevisionCacheWrite {
        super::write_revision_form_tolerance(
            match &mut self.spline {
                ExactSpline::Revision { form, .. } => Some(&mut form.cache),
                ExactSpline::Legacy { .. } => None,
            },
            value,
            write,
        )
    }
}

impl LoftSurfacePayload {
    pub(super) fn revision_cache(
        &self,
    ) -> Option<&super::RevisionCacheForm<super::RevisionSurfaceParameterization<FiniteReal>>> {
        self.cache.form().map(|form| &form.cache)
    }
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
}

impl SweepSurfacePayload {
    pub(super) fn revision_cache(
        &self,
    ) -> Option<&super::RevisionCacheForm<super::RevisionSurfaceParameterization<FiniteReal>>> {
        self.native
            .as_ref()
            .and_then(|construction| construction.cache.form())
            .map(|form| &form.cache)
    }
    pub(super) fn write_revision_fit_tolerance(
        &mut self,
        value: super::FitTolerance,
        write: super::ToleranceWrite,
    ) -> super::RevisionCacheWrite {
        super::write_revision_form_tolerance(
            self.native
                .as_mut()
                .and_then(|construction| construction.cache.form_mut())
                .map(|form| &mut form.cache),
            value,
            write,
        )
    }
}

impl DeformableSurfacePayload {
    pub(super) fn revision_cache(
        &self,
    ) -> Option<&super::RevisionCacheForm<super::RevisionSurfaceParameterization<FiniteReal>>> {
        self.construction.cache.form().map(|form| &form.cache)
    }
    pub(super) fn write_revision_fit_tolerance(
        &mut self,
        value: super::FitTolerance,
        write: super::ToleranceWrite,
    ) -> super::RevisionCacheWrite {
        super::write_revision_form_tolerance(
            self.construction
                .cache
                .form_mut()
                .map(|form| &mut form.cache),
            value,
            write,
        )
    }
}

impl BlendSurfacePayload {
    pub(super) fn revision_cache(
        &self,
    ) -> Option<&super::RevisionCacheForm<super::RevisionSurfaceParameterization<FiniteReal>>> {
        self.cache.form().map(|construction| &construction.cache)
    }
    pub(super) fn write_revision_fit_tolerance(
        &mut self,
        value: super::FitTolerance,
        write: super::ToleranceWrite,
    ) -> super::RevisionCacheWrite {
        super::write_revision_form_tolerance(
            self.cache
                .form_mut()
                .map(|construction| &mut construction.cache),
            value,
            write,
        )
    }
}

impl VariableBlendSurfacePayload {
    /// Change the effective fit tolerance of the approximation cache.
    ///
    /// The narrow write route: the borrow of the cache stays inside this
    /// method, so the payload lends no admitted interior.
    pub(super) fn write_cache_fit_tolerance(
        &mut self,
        value: Option<super::FitTolerance>,
    ) -> Result<(), super::CacheContractError> {
        super::set_variable_blend_cache(&mut self.construction.cache, value)
    }
}

impl BlendSurfacePayload {
    /// Set the resolved blend support sides.
    pub fn set_supports(&mut self, supports: [Option<BlendSupport>; 2]) {
        self.supports = supports;
    }
    /// Set the resolved spine curve.
    pub fn set_spine(&mut self, spine: Option<CurveId>) {
        self.spine = spine;
    }
}

/// Whether an admitted pair is ordered.
fn ordered(range: &[FiniteReal; 2]) -> bool {
    range[0] <= range[1]
}

/// Whether an admitted optional pair is ordered when both ends are present.
fn optional_ordered(range: &[Option<FiniteReal>; 2]) -> bool {
    match range {
        [Some(lower), Some(upper)] => lower <= upper,
        [None | Some(_), None] | [None, Some(_)] => true,
    }
}

impl CompoundSurfacePayload {
    /// Solved-cache fit contract this construction states.
    #[must_use]
    pub const fn legacy_cache(&self) -> Option<LegacyCache> {
        self.cache
    }

    /// Mutable legacy solved-cache slot this construction states.
    pub(super) const fn legacy_cache_slot_mut(&mut self) -> LegacyCacheSlot<'_> {
        LegacyCacheSlot::Optional(&mut self.cache)
    }
}

impl CompoundLoftSurfacePayload {
    /// Solved-cache fit contract this construction states.
    #[must_use]
    pub const fn legacy_cache(&self) -> Option<LegacyCache> {
        self.cache
    }

    /// Mutable legacy solved-cache slot this construction states.
    pub(super) const fn legacy_cache_slot_mut(&mut self) -> LegacyCacheSlot<'_> {
        LegacyCacheSlot::Optional(&mut self.cache)
    }
}

impl ScaledCompoundLoftSurfacePayload {
    /// Solved-cache fit contract this construction states.
    #[must_use]
    pub const fn legacy_cache(&self) -> Option<LegacyCache> {
        self.cache
    }

    /// Mutable legacy solved-cache slot this construction states.
    pub(super) const fn legacy_cache_slot_mut(&mut self) -> LegacyCacheSlot<'_> {
        LegacyCacheSlot::Optional(&mut self.cache)
    }
}

impl SkinSurfacePayload {
    /// Solved-cache fit contract this construction states.
    #[must_use]
    pub const fn legacy_cache(&self) -> Option<LegacyCache> {
        self.cache
    }

    /// Mutable legacy solved-cache slot this construction states.
    pub(super) const fn legacy_cache_slot_mut(&mut self) -> LegacyCacheSlot<'_> {
        LegacyCacheSlot::Optional(&mut self.cache)
    }
}

impl LawSurfacePayload {
    /// Solved-cache fit contract the full tail states.
    #[must_use]
    pub const fn legacy_cache(&self) -> Option<LegacyCache> {
        match &self.construction.tail {
            crate::geometry::LawSurfaceTail::Full { cache } => Some(*cache),
            _ => None,
        }
    }

    /// Mutable solved-cache slot of the full tail. Other tails carry no
    /// solved cache and state no slot. The full tail always states one, so
    /// the slot is required.
    pub(super) fn legacy_cache_slot_mut(&mut self) -> Option<LegacyCacheSlot<'_>> {
        match &mut self.construction.tail {
            crate::geometry::LawSurfaceTail::Full { cache } => {
                Some(LegacyCacheSlot::Required(cache))
            }
            _ => None,
        }
    }
}

impl NetSurfacePayload {
    /// Solved-cache fit contract this construction states.
    #[must_use]
    pub const fn legacy_cache(&self) -> Option<LegacyCache> {
        self.cache
    }

    /// Mutable legacy solved-cache slot this construction states.
    pub(super) const fn legacy_cache_slot_mut(&mut self) -> LegacyCacheSlot<'_> {
        LegacyCacheSlot::Optional(&mut self.cache)
    }
}

impl G2BlendSurfacePayload {
    /// Solved-cache fit contract this construction states.
    #[must_use]
    pub const fn legacy_cache(&self) -> Option<LegacyCache> {
        self.cache
    }

    /// Mutable legacy solved-cache slot this construction states.
    pub(super) const fn legacy_cache_slot_mut(&mut self) -> LegacyCacheSlot<'_> {
        LegacyCacheSlot::Optional(&mut self.cache)
    }
}

impl SubsetSurfaceConstruction {
    /// Solved-cache fit contract this construction states.
    #[must_use]
    pub const fn legacy_cache(&self) -> Option<LegacyCache> {
        self.cache
    }

    /// Mutable legacy solved-cache slot this construction states.
    pub(super) const fn legacy_cache_slot_mut(&mut self) -> LegacyCacheSlot<'_> {
        LegacyCacheSlot::Optional(&mut self.cache)
    }
}

impl TaperSurfaceConstruction {
    /// Solved-cache fit contract this construction states, absent when a
    /// revision-gated form states it instead.
    #[must_use]
    pub const fn legacy_cache(&self) -> Option<LegacyCache> {
        match self.cache.legacy_fit_tolerance() {
            Some(fit_tolerance) => Some(LegacyCache { fit_tolerance }),
            None => None,
        }
    }

    /// Mutable legacy solved-cache slot, absent when a revision-gated form
    /// states the tolerance instead.
    pub(super) fn legacy_cache_slot_mut(&mut self) -> Option<LegacyCacheSlot<'_>> {
        self.cache.legacy_cache_mut().map(LegacyCacheSlot::Optional)
    }
}

impl ExtrusionSurfaceConstruction {
    /// Solved-cache fit contract this construction states, absent when a
    /// revision-gated form states it instead.
    #[must_use]
    pub const fn legacy_cache(&self) -> Option<LegacyCache> {
        match self.cache.legacy_fit_tolerance() {
            Some(fit_tolerance) => Some(LegacyCache { fit_tolerance }),
            None => None,
        }
    }

    /// Mutable legacy solved-cache slot, absent when a revision-gated form
    /// states the tolerance instead.
    pub(super) fn legacy_cache_slot_mut(&mut self) -> Option<LegacyCacheSlot<'_>> {
        self.cache.legacy_cache_mut().map(LegacyCacheSlot::Optional)
    }
}

impl RevolutionSurfaceConstruction {
    /// Solved-cache fit contract this construction states, absent when a
    /// revision-gated form states it instead.
    #[must_use]
    pub const fn legacy_cache(&self) -> Option<LegacyCache> {
        match self.cache.legacy_fit_tolerance() {
            Some(fit_tolerance) => Some(LegacyCache { fit_tolerance }),
            None => None,
        }
    }

    /// Mutable legacy solved-cache slot, absent when a revision-gated form
    /// states the tolerance instead.
    pub(super) fn legacy_cache_slot_mut(&mut self) -> Option<LegacyCacheSlot<'_>> {
        self.cache.legacy_cache_mut().map(LegacyCacheSlot::Optional)
    }
}

impl SumSurfaceConstruction {
    /// Solved-cache fit contract this construction states, absent when a
    /// revision-gated form states it instead.
    #[must_use]
    pub const fn legacy_cache(&self) -> Option<LegacyCache> {
        match self.cache.legacy_fit_tolerance() {
            Some(fit_tolerance) => Some(LegacyCache { fit_tolerance }),
            None => None,
        }
    }

    /// Mutable legacy solved-cache slot, absent when a revision-gated form
    /// states the tolerance instead.
    pub(super) fn legacy_cache_slot_mut(&mut self) -> Option<LegacyCacheSlot<'_>> {
        self.cache.legacy_cache_mut().map(LegacyCacheSlot::Optional)
    }
}

impl LoftSurfacePayload {
    /// Solved-cache fit contract this construction states, absent when a
    /// revision-gated form states it instead.
    #[must_use]
    pub const fn legacy_cache(&self) -> Option<LegacyCache> {
        match self.cache.legacy_fit_tolerance() {
            Some(fit_tolerance) => Some(LegacyCache { fit_tolerance }),
            None => None,
        }
    }

    /// Mutable legacy solved-cache slot, absent when a revision-gated form
    /// states the tolerance instead.
    pub(super) fn legacy_cache_slot_mut(&mut self) -> Option<LegacyCacheSlot<'_>> {
        self.cache.legacy_cache_mut().map(LegacyCacheSlot::Optional)
    }
}

impl ExactSurfacePayload {
    /// Solved-cache fit contract the legacy spline layout states.
    #[must_use]
    pub const fn legacy_cache(&self) -> Option<LegacyCache> {
        match &self.spline {
            ExactSpline::Legacy { cache, .. } => *cache,
            ExactSpline::Revision { .. } => None,
        }
    }

    /// Mutable legacy solved-cache slot, absent when the revision spline
    /// layout states the tolerance instead.
    pub(super) fn legacy_cache_slot_mut(&mut self) -> Option<LegacyCacheSlot<'_>> {
        match &mut self.spline {
            ExactSpline::Legacy { cache, .. } => Some(LegacyCacheSlot::Optional(cache)),
            ExactSpline::Revision { .. } => None,
        }
    }
}

impl OffsetSurfaceConstruction {
    /// Solved-cache fit contract the legacy extension states.
    #[must_use]
    pub const fn legacy_cache(&self) -> Option<LegacyCache> {
        match &self.extension {
            OffsetExtension::Legacy { cache, .. } => *cache,
            OffsetExtension::Revision { .. } => None,
        }
    }

    /// Mutable legacy solved-cache slot, absent when the revision extension
    /// states the tolerance instead.
    pub(super) fn legacy_cache_slot_mut(&mut self) -> Option<LegacyCacheSlot<'_>> {
        match &mut self.extension {
            OffsetExtension::Legacy { cache, .. } => Some(LegacyCacheSlot::Optional(cache)),
            OffsetExtension::Revision { .. } => None,
        }
    }
}

impl DeformableSurfacePayload {
    /// Solved-cache fit contract this construction states.
    #[must_use]
    pub const fn legacy_cache(&self) -> Option<LegacyCache> {
        match self.construction.cache.legacy_fit_tolerance() {
            Some(fit_tolerance) => Some(LegacyCache { fit_tolerance }),
            None => None,
        }
    }

    /// Mutable legacy solved-cache slot, absent when a revision-gated form
    /// states the tolerance instead.
    pub(super) fn legacy_cache_slot_mut(&mut self) -> Option<LegacyCacheSlot<'_>> {
        self.construction
            .cache
            .legacy_cache_mut()
            .map(LegacyCacheSlot::Optional)
    }
}

impl SweepSurfacePayload {
    /// Solved-cache fit contract the native construction states.
    #[must_use]
    pub fn legacy_cache(&self) -> Option<LegacyCache> {
        self.native
            .as_ref()
            .and_then(|construction| construction.cache.legacy_fit_tolerance())
            .map(|fit_tolerance| LegacyCache { fit_tolerance })
    }

    /// Mutable legacy solved-cache slot of the native construction. A sweep
    /// with no native construction, or one whose cache is revision-gated,
    /// states no slot.
    pub(super) fn legacy_cache_slot_mut(&mut self) -> Option<LegacyCacheSlot<'_>> {
        self.native
            .as_mut()
            .and_then(|construction| construction.cache.legacy_cache_mut())
            .map(LegacyCacheSlot::Optional)
    }
}

impl BlendSurfacePayload {
    /// Solved-cache fit contract this construction states.
    #[must_use]
    pub const fn legacy_cache(&self) -> Option<LegacyCache> {
        match self.cache.legacy_fit_tolerance() {
            Some(fit_tolerance) => Some(LegacyCache { fit_tolerance }),
            None => None,
        }
    }

    /// Mutable legacy solved-cache slot, absent when a revision-gated form
    /// states the tolerance instead.
    pub(super) fn legacy_cache_slot_mut(&mut self) -> Option<LegacyCacheSlot<'_>> {
        self.cache.legacy_cache_mut().map(LegacyCacheSlot::Optional)
    }
}

#[cfg(test)]
mod tests;

// Each optional key below names itself in whatever it refuses.
cadmpeg_core::named_optional_field!(deserialize_pcurve, PcurveGeometry, "pcurve");
cadmpeg_core::named_optional_field!(
    deserialize_parameter_interval,
    [f64; 2],
    "parameter_interval"
);
cadmpeg_core::named_optional_field!(deserialize_native_position, Point3, "native_position");
cadmpeg_core::named_optional_field!(
    deserialize_angular_parameter_interval,
    [f64; 2],
    "angular_parameter_interval"
);
cadmpeg_core::named_optional_field!(deserialize_u_sense, i64, "u_sense");
cadmpeg_core::named_optional_field!(deserialize_v_sense, i64, "v_sense");
cadmpeg_core::named_optional_field!(
    deserialize_subset_surface_construction_u_sense,
    bool,
    "u_sense"
);
cadmpeg_core::named_optional_field!(
    deserialize_subset_surface_construction_v_sense,
    bool,
    "v_sense"
);
cadmpeg_core::named_optional_field!(deserialize_cache, LegacyCache, "cache");
cadmpeg_core::named_optional_field!(deserialize_native, Box<SweepSurfaceConstruction>, "native");
cadmpeg_core::named_optional_field!(deserialize_spine, CurveId, "spine");
