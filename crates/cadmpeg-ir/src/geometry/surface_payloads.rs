// SPDX-License-Identifier: Apache-2.0
//! Checked procedural surface payloads.

#[cfg(feature = "schema")]
#[cfg(feature = "schema")]
use super::ExactSplineSchemaWire;
#[cfg(feature = "schema")]
use super::OffsetExtensionSchemaWire;
use super::{
    BlendCrossSection, BlendRadiusLaw, BlendSupport, CompoundComponent, CompoundLoftConstruction,
    DeformableSurfaceConstruction, ExactSpline, G2BlendConstruction, LawSurfaceConstruction,
    LoftBridgeToken, LoftRevisionForm, LoftSection, NetSurfaceConstruction,
    RollingBallConstruction, ScaledCompoundLoftConstruction, SkinSurfaceConstruction,
    SplineSurfaceParameters, SweepSurfaceConstruction, VariableBlendConstruction,
    VertexBlendConstruction,
};
use super::{
    DirectedParameterRange, OffsetExtension, PcurveGeometry, ProceduralGeometryError,
    RevisionSurfaceForm, TaperSurfaceKind,
};
use crate::features::{FinitePoint3, FiniteVector3};
use crate::ids::{CurveId, SurfaceId};
use crate::math::{Point3, Vector3};
use crate::scalar::FiniteReal;
use crate::topology::ParameterInterval;
use crate::units::{FiniteVector, UnitVector3};
#[cfg(feature = "schema")]
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

/// Admitted support surface restriction parameters.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[cfg_attr(feature = "schema", schemars(with = "SubSurfaceConstructionWire"))]
#[serde(try_from = "SubSurfaceConstructionWire")]
pub struct SubSurfaceConstruction {
    /// Embedded support surface whose parameterization is retained.
    support: SurfaceId,
    /// Ordered U and V parameter intervals.
    parameter_ranges: [FiniteVector<2>; 2],
}

#[derive(Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
struct SubSurfaceConstructionWire {
    /// Embedded support surface whose parameterization is retained.
    support: SurfaceId,
    /// Ordered U and V parameter intervals.
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
    pub fn parameter_ranges(&self) -> [[f64; 2]; 2] {
        self.parameter_ranges.map(FiniteVector::get)
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
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pcurve: Option<PcurveGeometry>,
    /// Native taper parameter or draft magnitude.
    parameter: FiniteReal,
    /// Subtype-specific taper tail.
    taper: TaperSurfaceKind,
    /// Revision-gated form fields; absent from the pre-revision layout.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    revision_form: Option<RevisionSurfaceForm>,
}

#[derive(Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
struct TaperSurfaceConstructionWire {
    /// Base surface being tapered.
    support: SurfaceId,
    /// Reference curve on the support.
    reference: CurveId,
    /// UV curve on the support, absent for `nullbs`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pcurve: Option<PcurveGeometry>,
    /// Native taper parameter or draft magnitude.
    parameter: f64,
    /// Subtype-specific taper tail.
    taper: TaperSurfaceKind,
    /// Revision-gated form fields; absent from the pre-revision layout.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    revision_form: Option<RevisionSurfaceForm>,
}

impl TaperSurfaceConstruction {
    pub(super) fn revision_cache_mut(&mut self) -> Option<&mut super::RevisionCacheForm> {
        self.revision_form.as_mut().map(|form| &mut form.cache)
    }
    /// Admit the construction parameters.
    pub fn try_new(
        support: SurfaceId,
        reference: CurveId,
        pcurve: Option<PcurveGeometry>,
        parameter: f64,
        taper: TaperSurfaceKind,
        revision_form: Option<RevisionSurfaceForm>,
    ) -> Result<Self, ProceduralGeometryError> {
        let vector_finite =
            |vector: &Vector3| vector.x.is_finite() && vector.y.is_finite() && vector.z.is_finite();
        let tail_finite = match &taper {
            crate::geometry::TaperSurfaceKind::Standard
            | crate::geometry::TaperSurfaceKind::Orthogonal { .. } => true,
            crate::geometry::TaperSurfaceKind::Edge { draft } => vector_finite(draft),
            crate::geometry::TaperSurfaceKind::Shadow {
                draft,
                sine,
                cosine,
            }
            | crate::geometry::TaperSurfaceKind::Swept {
                draft,
                sine,
                cosine,
            } => vector_finite(draft) && sine.is_finite() && cosine.is_finite(),
            crate::geometry::TaperSurfaceKind::Ruled {
                draft,
                sine,
                cosine,
                factor,
            } => {
                vector_finite(draft) && sine.is_finite() && cosine.is_finite() && factor.is_finite()
            }
        };
        if !tail_finite {
            return Err(ProceduralGeometryError::Payload(
                "taper surface parameter or subtype tail is not finite",
            ));
        }

        Ok(Self {
            support,
            reference,
            pcurve,
            parameter: FiniteReal::new(parameter).ok_or(ProceduralGeometryError::Payload(
                "Taper.parameter is not finite",
            ))?,
            taper,
            revision_form,
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
    pub fn parameter(&self) -> &f64 {
        self.parameter.as_raw()
    }
    /// Return the taper.
    pub fn taper(&self) -> &TaperSurfaceKind {
        &self.taper
    }
    /// Return the revision form.
    pub fn revision_form(&self) -> &Option<RevisionSurfaceForm> {
        &self.revision_form
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
            wire.revision_form,
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
    /// Revision-gated form fields; absent from the pre-revision layout.
    /// The directrix parameter interval is `parameter_interval`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    revision_form: Option<RevisionSurfaceForm>,
}

#[derive(Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
struct ExtrusionSurfaceConstructionWire {
    /// Curve swept along `direction` to form the surface.
    directrix: CurveId,
    /// Native source directrix parameter interval, when carried by the
    /// source. The neutral surface-carrier interval is in
    /// `ProceduralSurface::record_bounds`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    parameter_interval: Option<[f64; 2]>,
    /// Length-bearing sweep direction, in document length units.
    direction: Vector3,
    /// Native model-space position following the sweep direction, when carried.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    native_position: Option<Point3>,
    /// Revision-gated form fields; absent from the pre-revision layout.
    /// The directrix parameter interval is `parameter_interval`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    revision_form: Option<RevisionSurfaceForm>,
}

impl ExtrusionSurfaceConstruction {
    pub(super) fn revision_cache_mut(&mut self) -> Option<&mut super::RevisionCacheForm> {
        self.revision_form.as_mut().map(|form| &mut form.cache)
    }
    /// Admit the construction parameters.
    pub fn try_new(
        directrix: CurveId,
        parameter_interval: Option<[f64; 2]>,
        direction: Vector3,
        native_position: Option<Point3>,
        revision_form: Option<RevisionSurfaceForm>,
    ) -> Result<Self, ProceduralGeometryError> {
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
            revision_form,
        })
    }
    /// Return the directrix.
    pub fn directrix(&self) -> &CurveId {
        &self.directrix
    }
    /// Return the parameter interval.
    pub fn parameter_interval(&self) -> Option<[f64; 2]> {
        self.parameter_interval.map(FiniteVector::get)
    }
    /// Return the direction.
    pub fn direction(&self) -> &Vector3 {
        self.direction.as_raw()
    }
    /// Return the native position.
    pub fn native_position(&self) -> Option<Point3> {
        self.native_position.map(FinitePoint3::get)
    }
    /// Return the revision form.
    pub fn revision_form(&self) -> &Option<RevisionSurfaceForm> {
        &self.revision_form
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
            wire.revision_form,
        )
    }
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
    /// A point on the revolution axis.
    axis_origin: Point3,
    /// Unit direction of the revolution axis.
    axis_direction: Vector3,
    /// Angular start and end parameters, in radians.
    angular_interval: ParameterInterval,
    /// Surface-parameter interval that maps affinely to
    /// `angular_interval`. Absence means the surface parameter is already
    /// the revolution angle in radians.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    angular_parameter_interval: Option<ParameterInterval>,
    /// Native source directrix parameter start and end values, when
    /// carried by the source representation. The neutral surface-carrier
    /// interval is in `ProceduralSurface::record_bounds`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    parameter_interval: Option<ParameterInterval>,
    /// Whether the source parameter directions are transposed.
    transposed: bool,
    /// Revision-gated form fields; absent from the pre-revision layout.
    /// The profile curve's optional endpoints are `reference_endpoints`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    revision_form: Option<RevisionSurfaceForm>,
}

#[derive(Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
struct RevolutionSurfaceConstructionWire {
    /// Curve revolved about the axis to form the surface.
    directrix: CurveId,
    /// A point on the revolution axis.
    axis_origin: Point3,
    /// Unit direction of the revolution axis.
    axis_direction: Vector3,
    /// Angular start and end parameters, in radians.
    angular_interval: [f64; 2],
    /// Surface-parameter interval that maps affinely to
    /// `angular_interval`. Absence means the surface parameter is already
    /// the revolution angle in radians.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    angular_parameter_interval: Option<[f64; 2]>,
    /// Native source directrix parameter start and end values, when
    /// carried by the source representation. The neutral surface-carrier
    /// interval is in `ProceduralSurface::record_bounds`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    parameter_interval: Option<[f64; 2]>,
    /// Whether the source parameter directions are transposed.
    transposed: bool,
    /// Revision-gated form fields; absent from the pre-revision layout.
    /// The profile curve's optional endpoints are `reference_endpoints`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    revision_form: Option<RevisionSurfaceForm>,
}

impl RevolutionSurfaceConstruction {
    pub(super) fn revision_cache_mut(&mut self) -> Option<&mut super::RevisionCacheForm> {
        self.revision_form.as_mut().map(|form| &mut form.cache)
    }
    /// Admit the construction parameters.
    pub fn try_new(
        directrix: CurveId,
        (axis_origin, axis_direction): (Point3, Vector3),
        angular_interval: [f64; 2],
        angular_parameter_interval: Option<[f64; 2]>,
        parameter_interval: Option<[f64; 2]>,
        transposed: bool,
        revision_form: Option<RevisionSurfaceForm>,
    ) -> Result<Self, ProceduralGeometryError> {
        let admit_interval = |range: [f64; 2], message| {
            let interval = ParameterInterval::new(range)
                .map_err(|_| ProceduralGeometryError::Payload(message))?;
            if range[0] == range[1] {
                return Err(ProceduralGeometryError::Payload(message));
            }
            Ok(interval)
        };
        let angular_interval = admit_interval(
            angular_interval,
            "revolution angular_interval must be finite and strictly increasing",
        )?;
        let angular_parameter_interval = angular_parameter_interval
            .map(|range| {
                admit_interval(
                    range,
                    "revolution angular_parameter_interval must be finite and strictly increasing",
                )
            })
            .transpose()?;
        let parameter_interval = parameter_interval
            .map(|range| {
                admit_interval(
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
            revision_form,
        })
    }
    /// Return the directrix.
    pub fn directrix(&self) -> &CurveId {
        &self.directrix
    }
    /// Return the axis origin.
    pub fn axis_origin(&self) -> &Point3 {
        &self.axis_origin
    }
    /// Return the axis direction.
    pub fn axis_direction(&self) -> &Vector3 {
        &self.axis_direction
    }
    /// Return the angular interval.
    pub fn angular_interval(&self) -> &[f64; 2] {
        self.angular_interval.as_raw()
    }
    /// Return the angular parameter interval.
    pub fn angular_parameter_interval(&self) -> Option<[f64; 2]> {
        self.angular_parameter_interval
            .map(ParameterInterval::endpoints)
    }
    /// Return the parameter interval.
    pub fn parameter_interval(&self) -> Option<[f64; 2]> {
        self.parameter_interval.map(ParameterInterval::endpoints)
    }
    /// Return the transposed.
    pub fn transposed(&self) -> &bool {
        &self.transposed
    }
    /// Return the revision form.
    pub fn revision_form(&self) -> &Option<RevisionSurfaceForm> {
        &self.revision_form
    }
}

impl TryFrom<RevolutionSurfaceConstructionWire> for RevolutionSurfaceConstruction {
    type Error = ProceduralGeometryError;
    fn try_from(wire: RevolutionSurfaceConstructionWire) -> Result<Self, Self::Error> {
        Self::try_new(
            wire.directrix,
            (wire.axis_origin, wire.axis_direction),
            wire.angular_interval,
            wire.angular_parameter_interval,
            wire.parameter_interval,
            wire.transposed,
            wire.revision_form,
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
    #[serde(default, skip_serializing_if = "Option::is_none")]
    u_sense: Option<i64>,
    /// Native V parameter-direction sense enum, when carried.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    v_sense: Option<i64>,
    /// Whether the support continues as ruled linear strips outside its
    /// active NURBS rectangle.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    linear_support_extension: bool,
    /// Legacy conditional extension flags or the revision-gated form.
    #[serde(flatten)]
    #[cfg_attr(feature = "schema", schemars(with = "OffsetExtensionSchemaWire"))]
    extension: OffsetExtension,
}

#[derive(Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
struct OffsetSurfaceConstructionWire {
    /// Surface this surface is offset from.
    support: SurfaceId,
    /// Signed offset distance, in document length units.
    distance: f64,
    /// Native U parameter-direction sense enum, when carried.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    u_sense: Option<i64>,
    /// Native V parameter-direction sense enum, when carried.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    v_sense: Option<i64>,
    /// Whether the support continues as ruled linear strips outside its
    /// active NURBS rectangle.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    linear_support_extension: bool,
    /// Legacy conditional extension flags or the revision-gated form.
    #[serde(flatten)]
    #[cfg_attr(feature = "schema", schemars(with = "OffsetExtensionSchemaWire"))]
    extension: OffsetExtension,
}

impl OffsetSurfaceConstruction {
    /// Replace the support identity.
    pub fn set_support(&mut self, support: SurfaceId) {
        self.support = support;
    }
    /// Replace the linear support extension flag.
    pub fn set_linear_support_extension(&mut self, linear_support_extension: bool) {
        self.linear_support_extension = linear_support_extension;
    }
    /// Replace the finite offset distance.
    pub fn try_set_distance(&mut self, distance: f64) -> Result<(), ProceduralGeometryError> {
        self.distance = FiniteReal::new(distance).ok_or(ProceduralGeometryError::Payload(
            "offset spline surface distance is invalid",
        ))?;
        Ok(())
    }

    pub(super) fn revision_cache_mut(&mut self) -> Option<&mut super::RevisionCacheForm> {
        match &mut self.extension {
            OffsetExtension::Revision(form) => Some(&mut form.cache),
            OffsetExtension::Legacy(_) => None,
        }
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
    /// Return the support.
    pub fn support(&self) -> &SurfaceId {
        &self.support
    }
    /// Return the distance.
    pub fn distance(&self) -> &f64 {
        self.distance.as_raw()
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
    pub fn extension(&self) -> &OffsetExtension {
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
    #[serde(default, skip_serializing_if = "Option::is_none")]
    u_sense: Option<bool>,
    /// Whether the trimmed surface V direction agrees with the support.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    v_sense: Option<bool>,
}

#[derive(Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
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
    #[serde(default, skip_serializing_if = "Option::is_none")]
    u_sense: Option<bool>,
    /// Whether the trimmed surface V direction agrees with the support.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    v_sense: Option<bool>,
}

impl SubsetSurfaceConstruction {
    /// Admit the construction parameters.
    pub fn try_new(
        support: SurfaceId,
        parameter_ranges: [[f64; 2]; 2],
        u_sense: Option<bool>,
        v_sense: Option<bool>,
    ) -> Result<Self, ProceduralGeometryError> {
        Ok(Self {
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
    pub fn parameter_ranges(&self) -> [[f64; 2]; 2] {
        self.parameter_ranges.map(DirectedParameterRange::endpoints)
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
struct ParallelOffsetSurfaceConstructionWire {
    /// Surface being offset.
    support: SurfaceId,
    /// Signed offset distance.
    distance: f64,
    /// Whether the source classifies the result as self-intersecting.
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
    pub fn distance(&self) -> &f64 {
        self.distance.as_raw()
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
    direction: FiniteVector3,
}

#[derive(Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
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
        let direction = FiniteVector3::new(direction).ok_or(ProceduralGeometryError::Payload(
            "invalid linear-sweep direction",
        ))?;
        if direction.as_raw().norm() <= f64::EPSILON {
            return Err(ProceduralGeometryError::Payload(
                "invalid linear-sweep direction",
            ));
        }
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
    pub fn direction(&self) -> &Vector3 {
        self.direction.as_raw()
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
        const INVALID_AXIS: ProceduralGeometryError = ProceduralGeometryError::Payload(
            "revolution axis_origin and axis_direction must be finite, with unit axis_direction",
        );
        Ok(Self {
            directrix,
            axis_origin: FinitePoint3::new(axis_origin).ok_or(INVALID_AXIS)?,
            axis_direction: UnitVector3::new(axis_direction).ok_or(INVALID_AXIS)?,
        })
    }
    /// Return the directrix.
    pub fn directrix(&self) -> &CurveId {
        &self.directrix
    }
    /// Return the axis origin.
    pub fn axis_origin(&self) -> &Point3 {
        self.axis_origin.as_raw()
    }
    /// Return the axis direction.
    pub fn axis_direction(&self) -> &Vector3 {
        self.axis_direction.as_raw()
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
    /// Revision-gated form fields; absent from the pre-revision layout.
    /// The first curve's optional endpoints are `reference_endpoints`
    /// and the second curve's are `second_endpoints`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    revision_form: Option<RevisionSurfaceForm>,
}

#[derive(Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
struct SumSurfaceConstructionWire {
    /// First curve, varying in the first surface parameter.
    first: CurveId,
    /// Second curve, varying in the second surface parameter.
    second: CurveId,
    /// Surface base point.
    basepoint: Vector3,
    /// Revision-gated form fields; absent from the pre-revision layout.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    revision_form: Option<RevisionSurfaceForm>,
}

impl SumSurfaceConstruction {
    pub(super) fn revision_cache_mut(&mut self) -> Option<&mut super::RevisionCacheForm> {
        self.revision_form.as_mut().map(|form| &mut form.cache)
    }
    /// Admit the construction parameters.
    pub fn try_new(
        first: CurveId,
        second: CurveId,
        basepoint: Vector3,
        revision_form: Option<RevisionSurfaceForm>,
    ) -> Result<Self, ProceduralGeometryError> {
        Ok(Self {
            first,
            second,
            basepoint: FiniteVector3::new(basepoint).ok_or(ProceduralGeometryError::Payload(
                "sum basepoint must be finite",
            ))?,
            revision_form,
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
    pub fn basepoint(&self) -> &Vector3 {
        self.basepoint.as_raw()
    }
    /// Return the revision form.
    pub fn revision_form(&self) -> &Option<RevisionSurfaceForm> {
        &self.revision_form
    }
}

impl TryFrom<SumSurfaceConstructionWire> for SumSurfaceConstruction {
    type Error = ProceduralGeometryError;
    fn try_from(wire: SumSurfaceConstructionWire) -> Result<Self, Self::Error> {
        Self::try_new(wire.first, wire.second, wire.basepoint, wire.revision_form)
    }
}

/// Admitted exact surface construction.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[cfg_attr(feature = "schema", schemars(with = "ExactSurfacePayloadWire"))]
#[serde(try_from = "ExactSurfacePayloadWire")]
pub struct ExactSurfacePayload {
    #[serde(flatten)]
    #[cfg_attr(feature = "schema", schemars(with = "ExactSplineSchemaWire"))]
    spline: ExactSpline,
}
#[derive(Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
struct ExactSurfacePayloadWire {
    #[serde(flatten)]
    #[cfg_attr(feature = "schema", schemars(with = "ExactSplineSchemaWire"))]
    spline: ExactSpline,
}
impl ExactSurfacePayload {
    /// Admit the construction parameters.
    pub fn try_new(spline: ExactSpline) -> Result<Self, ProceduralGeometryError> {
        let valid = match &spline {
            crate::geometry::ExactSpline::Legacy { ranges, .. } => ranges
                .iter()
                .all(|range| range.iter().all(|value| value.is_finite()) && range[0] <= range[1]),
            crate::geometry::ExactSpline::Revision { intervals, .. } => intervals
                .iter()
                .flatten()
                .flatten()
                .all(|value| value.is_finite()),
        };
        if !valid {
            return Err(ProceduralGeometryError::Payload(
                "exact spline surface parameter fields are invalid",
            ));
        }
        Ok(Self { spline })
    }
    /// Return the spline.
    pub fn spline(&self) -> &ExactSpline {
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
    components: Vec<CompoundComponent<SurfaceId>>,
}
#[derive(Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(deny_unknown_fields)]
struct CompoundSurfacePayloadWire {
    components: Vec<CompoundComponent<SurfaceId>>,
}
impl CompoundSurfacePayload {
    /// Admit the construction parameters.
    pub fn try_new(
        components: Vec<CompoundComponent<SurfaceId>>,
    ) -> Result<Self, ProceduralGeometryError> {
        if components.iter().any(|item| !item.parameter.is_finite()) {
            return Err(ProceduralGeometryError::Payload(
                "compound surface parameters and components are inconsistent",
            ));
        }
        Ok(Self { components })
    }
    /// Return the components.
    pub fn components(&self) -> &Vec<CompoundComponent<SurfaceId>> {
        &self.components
    }
}
impl TryFrom<CompoundSurfacePayloadWire> for CompoundSurfacePayload {
    type Error = ProceduralGeometryError;
    fn try_from(wire: CompoundSurfacePayloadWire) -> Result<Self, Self::Error> {
        Self::try_new(wire.components)
    }
}

/// Admitted loft surface construction.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[cfg_attr(feature = "schema", schemars(with = "LoftSurfacePayloadWire"))]
#[serde(try_from = "LoftSurfacePayloadWire")]
pub struct LoftSurfacePayload {
    sections: [LoftSection; 2],

    parameters: SplineSurfaceParameters,

    closures: [i64; 2],

    singularities: [i64; 2],

    mode: i64,

    bridge: Vec<LoftBridgeToken>,

    #[serde(default, skip_serializing_if = "Option::is_none")]
    revision_form: Option<LoftRevisionForm>,
}
#[derive(Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
struct LoftSurfacePayloadWire {
    sections: [LoftSection; 2],

    parameters: SplineSurfaceParameters,

    closures: [i64; 2],

    singularities: [i64; 2],

    mode: i64,

    bridge: Vec<LoftBridgeToken>,

    #[serde(default, skip_serializing_if = "Option::is_none")]
    revision_form: Option<LoftRevisionForm>,
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
        revision_form: Option<LoftRevisionForm>,
    ) -> Result<Self, ProceduralGeometryError> {
        let parameters_valid = match &parameters {
            crate::geometry::SplineSurfaceParameters::OrderedRanges { ranges } => ranges
                .iter()
                .all(|range| range[0].is_finite() && range[1].is_finite() && range[0] <= range[1]),
            crate::geometry::SplineSurfaceParameters::RevisionRanges { intervals } => intervals
                .iter()
                .flatten()
                .flatten()
                .all(|value| value.is_finite()),
        };
        let sections_valid = sections
            .iter()
            .flat_map(|section| &section.entries)
            .all(|entry| {
                entry.parameter.is_finite()
                    && entry.profile.iter().all(|member| {
                        let table = member.form.subdata();
                        table.row_values_are_finite()
                    })
            });
        let bridge_valid = bridge.iter().all(|token| match token {
            crate::geometry::LoftBridgeToken::Double(value) => value.is_finite(),
            crate::geometry::LoftBridgeToken::Boolean(_)
            | crate::geometry::LoftBridgeToken::Integer(_)
            | crate::geometry::LoftBridgeToken::Text(_)
            | crate::geometry::LoftBridgeToken::Enum(_) => true,
        });
        if !parameters_valid || !sections_valid || !bridge_valid {
            return Err(ProceduralGeometryError::Payload(
                "loft construction payload is invalid",
            ));
        }
        Ok(Self {
            sections,
            parameters,
            closures,
            singularities,
            mode,
            bridge,
            revision_form,
        })
    }
    /// Return the sections.
    pub fn sections(&self) -> &[LoftSection; 2] {
        &self.sections
    }
    /// Return the parameters.
    pub fn parameters(&self) -> &SplineSurfaceParameters {
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
    pub fn bridge(&self) -> &Vec<LoftBridgeToken> {
        &self.bridge
    }
    /// Return the revision form.
    pub fn revision_form(&self) -> &Option<LoftRevisionForm> {
        &self.revision_form
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
            wire.revision_form,
        )
    }
}

/// Admitted compound loft surface construction.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[cfg_attr(feature = "schema", schemars(with = "CompoundLoftSurfacePayloadWire"))]
#[serde(try_from = "CompoundLoftSurfacePayloadWire")]
pub struct CompoundLoftSurfacePayload {
    construction: Box<CompoundLoftConstruction>,
}
#[derive(Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
struct CompoundLoftSurfacePayloadWire {
    construction: Box<CompoundLoftConstruction>,
}
impl CompoundLoftSurfacePayload {
    /// Admit the construction parameters.
    pub fn try_new(
        construction: Box<CompoundLoftConstruction>,
    ) -> Result<Self, ProceduralGeometryError> {
        let vector_finite =
            |vector: &Vector3| vector.x.is_finite() && vector.y.is_finite() && vector.z.is_finite();
        let mut scales = construction.scales.as_slice().iter().collect::<Vec<_>>();
        let tail_valid = match &construction.tail {
            crate::geometry::CompoundLoftTail::Six {
                scale,
                direction,
                parameter_range,
                ..
            } => {
                scales.push(scale.as_ref());
                vector_finite(direction)
                    && parameter_range.iter().all(|value| value.is_finite())
                    && parameter_range[0] <= parameter_range[1]
            }
            crate::geometry::CompoundLoftTail::Seven {
                first_scale,
                second_scale,
                direction,
                ..
            } => {
                scales.extend(first_scale.iter().map(Box::as_ref));
                scales.push(second_scale.as_ref());
                vector_finite(direction)
            }
            crate::geometry::CompoundLoftTail::Zero { direction, .. } => match direction {
                crate::geometry::CompoundLoftDirection::Vector { value } => vector_finite(value),
                crate::geometry::CompoundLoftDirection::Curve { .. } => true,
            },
        };
        let scales_valid = scales.iter().all(|scale| {
            scale.members.iter().all(|member| {
                let data = &member.data;
                let table = &data.subdata;
                table.row_values_are_finite() && data.direction.as_ref().is_none_or(&vector_finite)
            })
        });
        if !tail_valid || !scales_valid {
            return Err(ProceduralGeometryError::Payload(
                "compound loft construction payload is invalid",
            ));
        }
        Ok(Self { construction })
    }
    /// Return the construction.
    pub fn construction(&self) -> &CompoundLoftConstruction {
        &self.construction
    }
}
impl TryFrom<CompoundLoftSurfacePayloadWire> for CompoundLoftSurfacePayload {
    type Error = ProceduralGeometryError;
    fn try_from(wire: CompoundLoftSurfacePayloadWire) -> Result<Self, Self::Error> {
        Self::try_new(wire.construction)
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
    construction: Box<ScaledCompoundLoftConstruction>,
}
#[derive(Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
struct ScaledCompoundLoftSurfacePayloadWire {
    construction: Box<ScaledCompoundLoftConstruction>,
}
impl ScaledCompoundLoftSurfacePayload {
    /// Admit the construction parameters.
    pub fn try_new(
        construction: Box<ScaledCompoundLoftConstruction>,
    ) -> Result<Self, ProceduralGeometryError> {
        let vector_finite =
            |vector: &Vector3| vector.x.is_finite() && vector.y.is_finite() && vector.z.is_finite();
        let shape_valid = match &construction.shape {
            crate::geometry::ScaledCompoundLoftShape::Full => true,
            crate::geometry::ScaledCompoundLoftShape::None {
                parameter_ranges,
                parameters,
            } => {
                parameter_ranges
                    .iter()
                    .flatten()
                    .chain(parameters.iter().flatten())
                    .all(|value| value.is_finite())
                    && parameter_ranges.iter().all(|range| range[0] <= range[1])
            }
        };
        let mut scales = construction.scales.as_slice().iter().collect::<Vec<_>>();
        let branch_valid = match &construction.branch {
            crate::geometry::ScaledCompoundLoftBranch::ExtendedVector {
                first_scale,
                second_scale,
                direction,
                ..
            } => {
                scales.extend(first_scale.iter().map(Box::as_ref));
                scales.push(second_scale.as_ref());
                vector_finite(direction)
            }
            crate::geometry::ScaledCompoundLoftBranch::ExtendedCurve { scale, .. } => {
                scales.extend(scale.iter().map(Box::as_ref));
                true
            }
            crate::geometry::ScaledCompoundLoftBranch::Direct { direction, .. } => {
                match direction {
                    crate::geometry::CompoundLoftDirection::Vector { value } => {
                        vector_finite(value)
                    }
                    crate::geometry::CompoundLoftDirection::Curve { .. } => true,
                }
            }
        };
        let scales_valid = scales.iter().all(|scale| {
            scale.members.iter().all(|member| {
                let data = &member.data;
                let table = &data.subdata;
                table.row_values_are_finite() && data.direction.as_ref().is_none_or(&vector_finite)
            })
        });
        let scalars_valid = construction
            .discontinuities
            .iter()
            .flatten()
            .all(|value| value.is_finite())
            && construction.tail_directions.iter().all(vector_finite);
        if !shape_valid || !branch_valid || !scales_valid || !scalars_valid {
            return Err(ProceduralGeometryError::Payload(
                "scaled compound loft construction payload is invalid",
            ));
        }
        Ok(Self { construction })
    }
    /// Return the construction.
    pub fn construction(&self) -> &ScaledCompoundLoftConstruction {
        &self.construction
    }
}
impl TryFrom<ScaledCompoundLoftSurfacePayloadWire> for ScaledCompoundLoftSurfacePayload {
    type Error = ProceduralGeometryError;
    fn try_from(wire: ScaledCompoundLoftSurfacePayloadWire) -> Result<Self, Self::Error> {
        Self::try_new(wire.construction)
    }
}

/// Admitted law surface construction.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[cfg_attr(feature = "schema", schemars(with = "LawSurfacePayloadWire"))]
#[serde(try_from = "LawSurfacePayloadWire")]
pub struct LawSurfacePayload {
    construction: Box<LawSurfaceConstruction>,
}
#[derive(Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
struct LawSurfacePayloadWire {
    construction: Box<LawSurfaceConstruction>,
}
impl LawSurfacePayload {
    /// Admit the construction parameters.
    pub fn try_new(
        construction: Box<LawSurfaceConstruction>,
    ) -> Result<Self, ProceduralGeometryError> {
        let formula_valid = |formula: &crate::geometry::LawFormula| {
            formula.variables().iter().all(|value| law_valid(value, 0))
        };
        let tail_valid = match &construction.tail {
            crate::geometry::LawSurfaceTail::Summary { parameters, .. } => {
                parameters.iter().flatten().all(|value| value.is_finite())
            }
            crate::geometry::LawSurfaceTail::None {
                parameter_ranges, ..
            } => parameter_ranges
                .iter()
                .flatten()
                .all(|value| value.is_finite()),
            crate::geometry::LawSurfaceTail::Full {}
            | crate::geometry::LawSurfaceTail::Historical {}
            | crate::geometry::LawSurfaceTail::Optimal {} => true,
        };
        let valid = construction
            .parameter_ranges
            .iter()
            .flatten()
            .flatten()
            .chain(construction.discontinuities.iter().flatten())
            .all(|value| value.is_finite())
            && tail_valid
            && formula_valid(&construction.primary)
            && construction.additional.iter().all(formula_valid);
        if !valid {
            return Err(ProceduralGeometryError::Payload(
                "law surface construction payload is invalid",
            ));
        }
        Ok(Self { construction })
    }
    /// Return the construction.
    pub fn construction(&self) -> &LawSurfaceConstruction {
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
    construction: Box<SkinSurfaceConstruction>,
}
#[derive(Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
struct SkinSurfacePayloadWire {
    construction: Box<SkinSurfaceConstruction>,
}
impl SkinSurfacePayload {
    /// Admit the construction parameters.
    pub fn try_new(
        construction: Box<SkinSurfaceConstruction>,
    ) -> Result<Self, ProceduralGeometryError> {
        let vector_finite =
            |vector: &Vector3| vector.x.is_finite() && vector.y.is_finite() && vector.z.is_finite();
        let layout_valid = match &construction.layout {
            crate::geometry::SkinSurfaceLayout::Profiles { profiles, .. } => {
                profiles.iter().all(|profile| {
                    let table = &profile.data.subdata;
                    table.row_values_are_finite()
                        && profile.data.direction.as_ref().is_none_or(&vector_finite)
                })
            }
            crate::geometry::SkinSurfaceLayout::Compact { subdata, .. } => {
                subdata.row_values_are_finite()
            }
        };
        let formula_valid = construction
            .formula
            .variables()
            .iter()
            .all(|variable| law_valid(variable, 0));
        let scalars_valid = construction.parameter.is_finite()
            && construction.trailing_parameter.is_finite()
            && vector_finite(&construction.direction)
            && construction
                .discontinuities
                .iter()
                .flatten()
                .all(|value| value.is_finite());
        if !layout_valid || !formula_valid || !scalars_valid {
            return Err(ProceduralGeometryError::Payload(
                "skin surface construction payload is invalid",
            ));
        }
        Ok(Self { construction })
    }
    /// Return the construction.
    pub fn construction(&self) -> &SkinSurfaceConstruction {
        &self.construction
    }
}
impl TryFrom<SkinSurfacePayloadWire> for SkinSurfacePayload {
    type Error = ProceduralGeometryError;
    fn try_from(wire: SkinSurfacePayloadWire) -> Result<Self, Self::Error> {
        Self::try_new(wire.construction)
    }
}

/// Admitted net surface construction.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[cfg_attr(feature = "schema", schemars(with = "NetSurfacePayloadWire"))]
#[serde(try_from = "NetSurfacePayloadWire")]
pub struct NetSurfacePayload {
    construction: Box<NetSurfaceConstruction>,
}
#[derive(Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
struct NetSurfacePayloadWire {
    construction: Box<NetSurfaceConstruction>,
}
impl NetSurfacePayload {
    /// Admit the construction parameters.
    pub fn try_new(
        construction: Box<NetSurfaceConstruction>,
    ) -> Result<Self, ProceduralGeometryError> {
        let sections_valid = construction.sections.iter().all(|section| {
            section.entries.iter().all(|entry| {
                entry.parameter.is_finite()
                    && entry.profile.iter().all(|member| {
                        let table = member.form.subdata();
                        table.row_values_are_finite()
                    })
            })
        });
        let formulas_valid = construction.formulas.iter().all(|formula| {
            formula
                .variables()
                .iter()
                .all(|variable| law_valid(variable, 0))
        });
        let scalars_valid = construction
            .frame_parameters
            .iter()
            .chain(construction.discontinuities.iter().flatten())
            .all(|value| value.is_finite())
            && construction.directions.iter().all(|direction| {
                direction.x.is_finite() && direction.y.is_finite() && direction.z.is_finite()
            });
        if !sections_valid || !formulas_valid || !scalars_valid {
            return Err(ProceduralGeometryError::Payload(
                "net surface construction payload is invalid",
            ));
        }
        Ok(Self { construction })
    }
    /// Return the construction.
    pub fn construction(&self) -> &NetSurfaceConstruction {
        &self.construction
    }
}
impl TryFrom<NetSurfacePayloadWire> for NetSurfacePayload {
    type Error = ProceduralGeometryError;
    fn try_from(wire: NetSurfacePayloadWire) -> Result<Self, Self::Error> {
        Self::try_new(wire.construction)
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
    native: Option<Box<SweepSurfaceConstruction>>,
}
#[derive(Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
struct SweepSurfacePayloadWire {
    profile: CurveId,

    spine: CurveId,

    #[serde(default, skip_serializing_if = "Option::is_none")]
    native: Option<Box<SweepSurfaceConstruction>>,
}
impl SweepSurfacePayload {
    /// Admit the construction parameters.
    pub fn try_new(
        profile: CurveId,
        spine: CurveId,
        native: Option<Box<SweepSurfaceConstruction>>,
    ) -> Result<Self, ProceduralGeometryError> {
        if let Some(construction) = &native {
            let vector_finite = |vector: &Vector3| {
                vector.x.is_finite() && vector.y.is_finite() && vector.z.is_finite()
            };
            let point_finite = |point: &crate::math::Point3| {
                point.x.is_finite() && point.y.is_finite() && point.z.is_finite()
            };
            let formula_valid = |formula: &crate::geometry::LawFormula| {
                formula
                    .variables()
                    .iter()
                    .all(|variable| law_valid(variable, 0))
            };
            let layout_valid = match &construction.layout {
                crate::geometry::SweepSurfaceLayout::ProfileFirst {
                    directions,
                    origin,
                    parameters,
                    formulas,
                    ..
                } => {
                    directions.iter().all(vector_finite)
                        && point_finite(origin)
                        && parameters.iter().all(|value| value.is_finite())
                        && formulas.iter().all(formula_valid)
                }
                crate::geometry::SweepSurfaceLayout::ExplicitFormula {
                    profile_range,
                    profile_frame,
                    origin,
                    directions,
                    path_range,
                    path_parameter,
                    formula,
                    ..
                } => {
                    profile_range
                        .iter()
                        .chain(path_range)
                        .all(|value| value.is_finite())
                        && profile_frame.as_ref().is_none_or(|(point, vector)| {
                            point_finite(point) && vector_finite(vector)
                        })
                        && point_finite(origin)
                        && directions.iter().all(vector_finite)
                        && path_parameter.is_finite()
                        && formula_valid(formula)
                }
                crate::geometry::SweepSurfaceLayout::ExplicitGuide {
                    profile_range,
                    profile_frame,
                    origin,
                    directions,
                    path_range,
                    path_parameter,
                    guide_range,
                    guide_parameters,
                    ..
                } => {
                    profile_range
                        .iter()
                        .chain(path_range)
                        .chain(guide_range)
                        .chain(guide_parameters)
                        .all(|value| value.is_finite())
                        && profile_frame.as_ref().is_none_or(|(point, vector)| {
                            point_finite(point) && vector_finite(vector)
                        })
                        && point_finite(origin)
                        && directions.iter().all(vector_finite)
                        && path_parameter.is_finite()
                }
                crate::geometry::SweepSurfaceLayout::ExplicitSurface {
                    profile_range,
                    profile_frame,
                    origin,
                    directions,
                    path_range,
                    path_parameter,
                    ..
                } => {
                    profile_range
                        .iter()
                        .chain(path_range)
                        .all(|value| value.is_finite())
                        && profile_frame.as_ref().is_none_or(|(point, vector)| {
                            point_finite(point) && vector_finite(vector)
                        })
                        && point_finite(origin)
                        && directions.iter().all(vector_finite)
                        && path_parameter.is_finite()
                }
                crate::geometry::SweepSurfaceLayout::LawDriven {
                    profile_range,
                    profile_frame,
                    origin,
                    directions,
                    first_law,
                    first_range,
                    law_direction,
                    path_range,
                    path_parameter,
                    second_law,
                    formula,
                    ..
                } => {
                    profile_range
                        .iter()
                        .chain(first_range)
                        .chain(path_range)
                        .all(|value| value.is_finite())
                        && profile_frame.as_ref().is_none_or(|(point, vector)| {
                            point_finite(point) && vector_finite(vector)
                        })
                        && point_finite(origin)
                        && directions.iter().all(vector_finite)
                        && vector_finite(law_direction)
                        && path_parameter.is_finite()
                        && law_valid(first_law, 0)
                        && law_valid(second_law, 0)
                        && formula_valid(formula)
                }
            };
            let scalars_valid = layout_valid
                && construction
                    .discontinuities
                    .iter()
                    .flatten()
                    .all(|value| value.is_finite());
            if !scalars_valid {
                return Err(ProceduralGeometryError::Payload(
                    "sweep surface construction payload is invalid",
                ));
            }
        }
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
    pub fn native(&self) -> &Option<Box<SweepSurfaceConstruction>> {
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
    construction: Box<DeformableSurfaceConstruction>,
}
#[derive(Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
struct DeformableSurfacePayloadWire {
    construction: Box<DeformableSurfaceConstruction>,
}
impl DeformableSurfacePayload {
    /// Admit the construction parameters.
    pub fn try_new(
        construction: Box<DeformableSurfaceConstruction>,
    ) -> Result<Self, ProceduralGeometryError> {
        let vector_finite =
            |vector: &Vector3| vector.x.is_finite() && vector.y.is_finite() && vector.z.is_finite();
        let frame_valid = |frame: &crate::geometry::DeformableSurfaceFrame| {
            frame.leading_vectors.iter().all(vector_finite)
                && frame.secondary_vectors.iter().all(vector_finite)
                && frame.leading_parameter.is_finite()
                && frame.secondary_parameter.is_finite()
                && frame.point.x.is_finite()
                && frame.point.y.is_finite()
                && frame.point.z.is_finite()
        };
        let data_valid = match &construction.data {
            crate::geometry::DeformableSurfaceData::Full {
                leading_vectors,
                leading_parameter,
                first_parameter,
                second_parameter,
                frames,
                ..
            } => {
                leading_vectors.iter().all(vector_finite)
                    && leading_parameter.is_finite()
                    && first_parameter.is_finite()
                    && second_parameter.is_finite()
                    && frames.iter().all(|frame| {
                        frame.vectors.iter().all(vector_finite) && frame.parameter.is_finite()
                    })
            }
            crate::geometry::DeformableSurfaceData::SurfaceCurve {
                first_parameter,
                second_parameter,
                vectors,
                frame_parameter,
                parameter_triples,
                ..
            } => {
                first_parameter.is_finite()
                    && second_parameter.is_finite()
                    && vectors.iter().all(vector_finite)
                    && frame_parameter.is_finite()
                    && parameter_triples
                        .iter()
                        .flatten()
                        .all(|value| value.is_finite())
            }
            crate::geometry::DeformableSurfaceData::Plain {
                frame,
                parameter_triples,
            } => {
                frame_valid(frame)
                    && parameter_triples
                        .iter()
                        .flatten()
                        .all(|value| value.is_finite())
            }
            crate::geometry::DeformableSurfaceData::Guided {
                frame,
                guide_parameter,
                ..
            } => frame_valid(frame) && guide_parameter.is_finite(),
            crate::geometry::DeformableSurfaceData::Minimal { vectors, .. } => {
                vectors.iter().all(vector_finite)
            }
            crate::geometry::DeformableSurfaceData::RevisionMode3 {
                leading_vectors,
                leading_parameter,
                trailing_point,
                trailing_vectors,
                frame_parameter,
                parameters,
                trailing_parameter,
                ..
            } => {
                leading_vectors.iter().all(vector_finite)
                    && leading_parameter.is_finite()
                    && trailing_point.x.is_finite()
                    && trailing_point.y.is_finite()
                    && trailing_point.z.is_finite()
                    && trailing_vectors.iter().all(vector_finite)
                    && frame_parameter.is_finite()
                    && parameters.iter().all(|value| value.is_finite())
                    && trailing_parameter.is_finite()
            }
        };
        if !data_valid
            || !construction
                .discontinuities
                .iter()
                .flatten()
                .all(|value| value.is_finite())
        {
            return Err(ProceduralGeometryError::Payload(
                "deformable surface construction payload is invalid",
            ));
        }
        Ok(Self { construction })
    }
    /// Return the construction.
    pub fn construction(&self) -> &DeformableSurfaceConstruction {
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
    construction: Box<G2BlendConstruction>,
}
#[derive(Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
struct G2BlendSurfacePayloadWire {
    construction: Box<G2BlendConstruction>,
}
impl G2BlendSurfacePayload {
    /// Admit the construction parameters.
    pub fn try_new(
        construction: Box<G2BlendConstruction>,
    ) -> Result<Self, ProceduralGeometryError> {
        let direction_finite = |direction: &Vector3| {
            direction.x.is_finite() && direction.y.is_finite() && direction.z.is_finite()
        };
        let first_shape_valid = match &construction.first_shape {
            crate::geometry::G2BlendFirstShape::Full { .. } => true,
            crate::geometry::G2BlendFirstShape::None {
                coefficients,
                extension,
                ..
            } => {
                coefficients.iter().all(|value| value.is_finite())
                    && extension.as_ref().is_none_or(|token| match token {
                        crate::geometry::LoftBridgeToken::Double(value) => value.is_finite(),
                        crate::geometry::LoftBridgeToken::Boolean(_)
                        | crate::geometry::LoftBridgeToken::Integer(_)
                        | crate::geometry::LoftBridgeToken::Text(_)
                        | crate::geometry::LoftBridgeToken::Enum(_) => true,
                    })
            }
        };
        let ranges_valid = construction
            .parameter_ranges
            .iter()
            .all(|range| range[0].is_finite() && range[1].is_finite() && range[0] <= range[1]);
        let scalars_valid = construction
            .center_parameters
            .iter()
            .chain(construction.trailing_parameters.iter())
            .chain(construction.discontinuities.iter().flatten())
            .all(|value| value.is_finite());
        if !direction_finite(&construction.first.direction)
            || !direction_finite(&construction.second.direction)
            || !first_shape_valid
            || !ranges_valid
            || !scalars_valid
        {
            return Err(ProceduralGeometryError::Payload(
                "G2 blend construction payload is invalid",
            ));
        }
        Ok(Self { construction })
    }
    /// Return the construction.
    pub fn construction(&self) -> &G2BlendConstruction {
        &self.construction
    }
}
impl TryFrom<G2BlendSurfacePayloadWire> for G2BlendSurfacePayload {
    type Error = ProceduralGeometryError;
    fn try_from(wire: G2BlendSurfacePayloadWire) -> Result<Self, Self::Error> {
        Self::try_new(wire.construction)
    }
}

/// Admitted variable blend surface construction.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[cfg_attr(feature = "schema", schemars(with = "VariableBlendSurfacePayloadWire"))]
#[serde(try_from = "VariableBlendSurfacePayloadWire")]
pub struct VariableBlendSurfacePayload {
    construction: Box<VariableBlendConstruction>,
}
#[derive(Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
struct VariableBlendSurfacePayloadWire {
    construction: Box<VariableBlendConstruction>,
}
impl VariableBlendSurfacePayload {
    /// Admit the construction parameters.
    pub fn try_new(
        construction: Box<VariableBlendConstruction>,
    ) -> Result<Self, ProceduralGeometryError> {
        let ranges_valid = construction.u_range.iter().all(|value| value.is_finite())
            && construction.u_range[0] <= construction.u_range[1]
            && construction.v_lower.is_none_or(f64::is_finite)
            && [&construction.post_range, &construction.slice_range]
                .into_iter()
                .chain(
                    construction
                        .secondary_curve
                        .as_ref()
                        .map(|curve| &curve.parameter_range),
                )
                .all(|range| {
                    range.iter().flatten().all(|value| value.is_finite())
                        && match (range[0], range[1]) {
                            (Some(lower), Some(upper)) => lower <= upper,
                            (None | Some(_), None) | (None, Some(_)) => true,
                        }
                });
        let sides_valid = construction.sides.iter().all(|side| {
            side.location.x.is_finite()
                && side.location.y.is_finite()
                && side.location.z.is_finite()
        });
        let values_valid = match &construction.radii {
            crate::geometry::VariableBlendRadii::Single { value } => {
                variable_blend_value_valid(value)
            }
            crate::geometry::VariableBlendRadii::Two { first, second } => {
                variable_blend_value_valid(first) && variable_blend_value_valid(second)
            }
        } && construction
            .cross_section
            .as_ref()
            .is_none_or(|cross_section| match cross_section {
                crate::geometry::VariableBlendCrossSection::Circular => true,
                crate::geometry::VariableBlendCrossSection::Thumbweights { parameters }
                | crate::geometry::VariableBlendCrossSection::G2Round { parameters } => {
                    parameters.iter().all(|value| value.is_finite())
                }
                crate::geometry::VariableBlendCrossSection::RoundedChamfer { radius } => {
                    radius.as_deref().is_none_or(variable_blend_value_valid)
                }
                crate::geometry::VariableBlendCrossSection::UnclassifiedBare { .. } => true,
            });
        let scalar_tail_valid = construction.offsets.iter().all(|value| value.is_finite())
            && construction.shape_parameter.is_finite()
            && construction.shape_length.is_finite();
        if !ranges_valid || !sides_valid || !values_valid || !scalar_tail_valid {
            return Err(ProceduralGeometryError::Payload(
                "variable blend construction payload is invalid",
            ));
        }
        Ok(Self { construction })
    }
    /// Return the construction.
    pub fn construction(&self) -> &VariableBlendConstruction {
        &self.construction
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
    construction: Box<VertexBlendConstruction>,
}
#[derive(Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
struct VertexBlendSurfacePayloadWire {
    construction: Box<VertexBlendConstruction>,
}
impl VertexBlendSurfacePayload {
    /// Admit the construction parameters.
    pub fn try_new(
        construction: Box<VertexBlendConstruction>,
    ) -> Result<Self, ProceduralGeometryError> {
        let point_finite = |point: &crate::math::Point3| {
            point.x.is_finite() && point.y.is_finite() && point.z.is_finite()
        };
        let vector_finite =
            |vector: &Vector3| vector.x.is_finite() && vector.y.is_finite() && vector.z.is_finite();
        let boundaries_valid = construction.boundaries.iter().all(|boundary| {
            vector_finite(&boundary.magic)
                && boundary.fullness.is_finite()
                && match &boundary.geometry {
                    crate::geometry::VertexBlendBoundaryGeometry::Circle {
                        twists,
                        parameters,
                        ..
                    } => {
                        twists.entries().iter().all(&point_finite)
                            && parameters.iter().all(|value| value.is_finite())
                    }
                    crate::geometry::VertexBlendBoundaryGeometry::Degenerate {
                        location,
                        normals,
                    } => {
                        point_finite(location)
                            && normals.iter().all(|normal| {
                                vector_finite(normal) && (normal.norm() > f64::EPSILON)
                            })
                    }
                    crate::geometry::VertexBlendBoundaryGeometry::Pcurve { .. } => true,
                    crate::geometry::VertexBlendBoundaryGeometry::Plane {
                        normal,
                        parameters,
                        ..
                    } => {
                        vector_finite(normal)
                            && (normal.norm() > f64::EPSILON)
                            && parameters.iter().all(|value| value.is_finite())
                    }
                }
        });
        if !boundaries_valid {
            return Err(ProceduralGeometryError::Payload(
                "vertex blend construction payload is invalid",
            ));
        }
        Ok(Self { construction })
    }
    /// Return the construction.
    pub fn construction(&self) -> &VertexBlendConstruction {
        &self.construction
    }
}
impl TryFrom<VertexBlendSurfacePayloadWire> for VertexBlendSurfacePayload {
    type Error = ProceduralGeometryError;
    fn try_from(wire: VertexBlendSurfacePayloadWire) -> Result<Self, Self::Error> {
        Self::try_new(wire.construction)
    }
}

/// Admitted blend surface construction.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[cfg_attr(feature = "schema", schemars(with = "BlendSurfacePayloadWire"))]
#[serde(try_from = "BlendSurfacePayloadWire")]
pub struct BlendSurfacePayload {
    supports: [Option<BlendSupport>; 2],

    #[serde(default, skip_serializing_if = "Option::is_none")]
    spine: Option<CurveId>,

    radius: BlendRadiusLaw,

    cross_section: BlendCrossSection,

    #[serde(default, skip_serializing_if = "Option::is_none")]
    native: Option<Box<RollingBallConstruction>>,
}
#[derive(Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
struct BlendSurfacePayloadWire {
    supports: [Option<BlendSupport>; 2],

    #[serde(default, skip_serializing_if = "Option::is_none")]
    spine: Option<CurveId>,

    radius: BlendRadiusLaw,

    cross_section: BlendCrossSection,

    #[serde(default, skip_serializing_if = "Option::is_none")]
    native: Option<Box<RollingBallConstruction>>,
}
impl BlendSurfacePayload {
    /// Admit the construction parameters.
    pub fn try_new(
        supports: [Option<BlendSupport>; 2],
        spine: Option<CurveId>,
        radius: BlendRadiusLaw,
        cross_section: BlendCrossSection,
        native: Option<Box<RollingBallConstruction>>,
    ) -> Result<Self, ProceduralGeometryError> {
        if let Some(construction) = &native {
            let point_finite = |point: &crate::math::Point3| {
                point.x.is_finite() && point.y.is_finite() && point.z.is_finite()
            };
            let vector_finite = |vector: &Vector3| {
                vector.x.is_finite() && vector.y.is_finite() && vector.z.is_finite()
            };
            let ranges_valid = [&construction.u_range, &construction.v_range]
                .iter()
                .all(|range| {
                    range.iter().flatten().all(|value| value.is_finite())
                        && match range {
                            [Some(lower), Some(upper)] => lower <= upper,
                            [None | Some(_), None] | [None, Some(_)] => true,
                        }
                });
            let selector_valid = match construction.radius_selector {
                crate::geometry::RollingBallRadiusSelector::None {} => true,
                crate::geometry::RollingBallRadiusSelector::Value { value } => value.is_finite(),
            };
            let scalars_valid = construction
                .offsets
                .iter()
                .chain(construction.parameters.iter())
                .chain(construction.discontinuities.iter().flatten())
                .all(|value| value.is_finite());
            let sides_valid = construction
                .sides
                .iter()
                .all(|side| point_finite(&side.location));
            let third_valid = construction
                .third
                .as_ref()
                .is_none_or(|side| vector_finite(&side.direction));
            if !ranges_valid || !selector_valid || !scalars_valid || !sides_valid || !third_valid {
                return Err(ProceduralGeometryError::Payload(
                    "rolling-ball blend construction payload is invalid",
                ));
            }
        }
        Ok(Self {
            supports,
            spine,
            radius,
            cross_section,
            native,
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
    pub fn native(&self) -> &Option<Box<RollingBallConstruction>> {
        &self.native
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
            wire.native,
        )
    }
}

impl ExactSurfacePayload {
    pub(super) fn revision_cache(&self) -> Option<&super::RevisionCacheForm> {
        match &self.spline {
            ExactSpline::Revision { form, .. } => Some(&form.cache),
            ExactSpline::Legacy { .. } => None,
        }
    }
    pub(super) fn revision_cache_mut(&mut self) -> Option<&mut super::RevisionCacheForm> {
        match &mut self.spline {
            ExactSpline::Revision { form, .. } => Some(&mut form.cache),
            ExactSpline::Legacy { .. } => None,
        }
    }
}

impl LoftSurfacePayload {
    pub(super) fn revision_cache(&self) -> Option<&super::RevisionCacheForm> {
        self.revision_form.as_ref().map(|form| &form.cache)
    }
    pub(super) fn revision_cache_mut(&mut self) -> Option<&mut super::RevisionCacheForm> {
        self.revision_form.as_mut().map(|form| &mut form.cache)
    }
}

impl SweepSurfacePayload {
    pub(super) fn revision_cache(&self) -> Option<&super::RevisionCacheForm> {
        self.native
            .as_ref()
            .and_then(|construction| construction.revision_form.as_ref())
            .map(|form| &form.cache)
    }
    pub(super) fn revision_cache_mut(&mut self) -> Option<&mut super::RevisionCacheForm> {
        self.native
            .as_mut()
            .and_then(|construction| construction.revision_form.as_mut())
            .map(|form| &mut form.cache)
    }
}

impl DeformableSurfacePayload {
    pub(super) fn revision_cache(&self) -> Option<&super::RevisionCacheForm> {
        self.construction
            .revision_form
            .as_ref()
            .map(|form| &form.cache)
    }
    pub(super) fn revision_cache_mut(&mut self) -> Option<&mut super::RevisionCacheForm> {
        self.construction
            .revision_form
            .as_mut()
            .map(|form| &mut form.cache)
    }
}

impl BlendSurfacePayload {
    pub(super) fn revision_cache(&self) -> Option<&super::RevisionCacheForm> {
        self.native.as_ref().map(|construction| &construction.cache)
    }
    pub(super) fn revision_cache_mut(&mut self) -> Option<&mut super::RevisionCacheForm> {
        self.native
            .as_mut()
            .map(|construction| &mut construction.cache)
    }
}

impl VariableBlendSurfacePayload {
    pub(super) fn cache_mut(&mut self) -> &mut super::VariableBlendCache {
        &mut self.construction.cache
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

fn variable_blend_value_valid(value: &crate::geometry::VariableBlendValue) -> bool {
    use crate::geometry::VariableBlendValuePayload;
    let finite = |values: &[f64]| values.iter().all(|value| value.is_finite());
    match &value.payload {
        VariableBlendValuePayload::TwoEnds {
            parameters, radii, ..
        } => finite(parameters) && finite(radii),
        VariableBlendValuePayload::FixedWidth {
            parameters, width, ..
        } => finite(parameters) && width.is_finite(),
        VariableBlendValuePayload::EdgeOffset {
            scalars, lengths, ..
        } => finite(scalars) && finite(lengths),
        VariableBlendValuePayload::Functional {
            parameter,
            radius,
            terminal,
            ..
        } => {
            parameter.is_finite()
                && radius.is_finite()
                && !matches!(terminal, crate::geometry::VariableBlendTerminal::Double(v) if !v.is_finite())
        }
        VariableBlendValuePayload::Constant {
            parameters,
            radius,
            nested,
            ..
        } => finite(parameters) && radius.is_finite() && variable_blend_value_valid(nested),
        VariableBlendValuePayload::Interpolated {
            parameter,
            radius,
            points,
            ..
        } => {
            parameter.is_finite()
                && radius.is_finite()
                && points.iter().all(|point| {
                    point.parameter.is_finite()
                        && point.radius.is_finite()
                        && point
                            .tangents
                            .iter()
                            .flatten()
                            .all(|value| value.is_finite())
                        && point.location.x.is_finite()
                        && point.location.y.is_finite()
                        && point.location.z.is_finite()
                        && point.normal.x.is_finite()
                        && point.normal.y.is_finite()
                        && point.normal.z.is_finite()
                })
        }
    }
}

fn law_valid(expression: &crate::geometry::LawExpression, depth: usize) -> bool {
    if depth > 64 {
        return false;
    }
    match expression {
        crate::geometry::LawExpression::Null | crate::geometry::LawExpression::Integer { .. } => {
            true
        }
        crate::geometry::LawExpression::Text { value } => !value.is_empty(),
        crate::geometry::LawExpression::Double { value } => value.is_finite(),
        crate::geometry::LawExpression::Point { value } => {
            value.x.is_finite() && value.y.is_finite() && value.z.is_finite()
        }
        crate::geometry::LawExpression::Vector { value } => {
            value.x.is_finite() && value.y.is_finite() && value.z.is_finite()
        }
        crate::geometry::LawExpression::Transform { scalars, .. } => {
            scalars.iter().all(|value| value.is_finite())
        }
        crate::geometry::LawExpression::TransformVec { vectors, scale, .. } => {
            scale.is_finite()
                && vectors
                    .iter()
                    .all(|value| value.x.is_finite() && value.y.is_finite() && value.z.is_finite())
        }
        crate::geometry::LawExpression::Edge { parameters, .. } => {
            parameters.iter().all(|value| value.is_finite())
        }
        crate::geometry::LawExpression::Spline {
            knots,
            controls,
            point,
            ..
        } => {
            knots.iter().chain(controls).all(|value| value.is_finite())
                && point.x.is_finite()
                && point.y.is_finite()
                && point.z.is_finite()
        }
        crate::geometry::LawExpression::Algebraic { operands, .. } => {
            operands.iter().all(|operand| law_valid(operand, depth + 1))
        }
    }
}
#[cfg(test)]
mod tests;
