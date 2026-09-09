// SPDX-License-Identifier: Apache-2.0
//! Checked procedural surface payloads.

#[cfg(feature = "schema")]
use super::OffsetExtensionSchemaWire;
use super::{
    DirectedParameterRange, OffsetExtension, OffsetSupportExtension, PcurveGeometry,
    ProceduralGeometryError, RevisionSurfaceForm, TaperSurfaceKind,
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
    /// Support continuation law outside its active NURBS rectangle.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    support_extension: Option<OffsetSupportExtension>,
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
    /// Support continuation law outside its active NURBS rectangle.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    support_extension: Option<OffsetSupportExtension>,
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
    /// Replace the support extension law.
    pub fn set_support_extension(&mut self, extension: Option<OffsetSupportExtension>) {
        self.support_extension = extension;
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
        support_extension: Option<OffsetSupportExtension>,
        extension: OffsetExtension,
    ) -> Result<Self, ProceduralGeometryError> {
        Ok(Self {
            support,
            distance: FiniteReal::new(distance).ok_or(ProceduralGeometryError::Payload(
                "Offset.distance is not finite",
            ))?,
            u_sense,
            v_sense,
            support_extension,
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
    /// Return the support extension.
    pub fn support_extension(&self) -> &Option<OffsetSupportExtension> {
        &self.support_extension
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
            wire.support_extension,
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

#[cfg(test)]
mod tests;
