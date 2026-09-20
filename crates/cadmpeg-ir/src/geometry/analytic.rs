// SPDX-License-Identifier: Apache-2.0
//! Analytic curves and surfaces with admitted frames and dimensions.

use crate::features::FinitePoint3;
use crate::math::{Point3, Vector3};
use crate::scalar::{Angle, FiniteReal, NonNegativeLength, PositiveReal};
use crate::units::{OrthonormalFrame3, UnitVector3};
#[cfg(feature = "schema")]
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

/// Plane with a finite origin and an orthonormal frame.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(try_from = "PlaneSurfaceWire", into = "PlaneSurfaceWire")]
pub struct PlaneSurface {
    origin: FinitePoint3,
    frame: OrthonormalFrame3,
}

#[derive(Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(deny_unknown_fields)]
struct PlaneSurfaceWire {
    /// Plane origin.
    origin: Point3,
    /// Plane normal.
    normal: Vector3,
    /// First in-plane axis, perpendicular to `normal`.
    u_axis: Vector3,
}

impl PlaneSurface {
    /// Admit finite parameters that satisfy the carrier's numeric contract.
    pub fn try_new(origin: Point3, normal: Vector3, u_axis: Vector3) -> Result<Self, &'static str> {
        let frame = OrthonormalFrame3::new(normal, u_axis)
            .ok_or("PlaneSurface.normal/u_axis must form an orthonormal frame")?;
        let origin = FinitePoint3::new(origin).ok_or("PlaneSurface.origin must be finite")?;
        Ok(Self { origin, frame })
    }

    /// Return the origin.
    #[must_use]
    pub const fn origin(&self) -> &Point3 {
        self.origin.as_raw()
    }

    /// Return the normal.
    #[must_use]
    pub const fn normal(&self) -> &Vector3 {
        self.frame.axis()
    }

    /// Return the u axis.
    #[must_use]
    pub const fn u_axis(&self) -> &Vector3 {
        self.frame.reference()
    }
}

impl From<PlaneSurface> for PlaneSurfaceWire {
    fn from(value: PlaneSurface) -> Self {
        Self {
            origin: *value.origin(),
            normal: *value.normal(),
            u_axis: *value.u_axis(),
        }
    }
}

impl TryFrom<PlaneSurfaceWire> for PlaneSurface {
    type Error = &'static str;
    fn try_from(wire: PlaneSurfaceWire) -> Result<Self, Self::Error> {
        Self::try_new(wire.origin, wire.normal, wire.u_axis)
    }
}

/// Circular cylinder with a positive radius and an orthonormal frame.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(try_from = "CylinderSurfaceWire", into = "CylinderSurfaceWire")]
pub struct CylinderSurface {
    origin: FinitePoint3,
    radius: PositiveReal,
    frame: OrthonormalFrame3,
}

#[derive(Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(deny_unknown_fields)]
struct CylinderSurfaceWire {
    /// Cylinder frame origin.
    origin: Point3,
    /// Frame axis direction.
    axis: Vector3,
    /// Frame reference direction, perpendicular to `axis`.
    ref_direction: Vector3,
    /// Cylinder radius.
    radius: f64,
}

impl CylinderSurface {
    /// Admit finite parameters that satisfy the carrier's numeric contract.
    pub fn try_new(
        origin: Point3,
        axis: Vector3,
        ref_direction: Vector3,
        radius: f64,
    ) -> Result<Self, &'static str> {
        let frame = OrthonormalFrame3::new(axis, ref_direction)
            .ok_or("CylinderSurface.axis/ref_direction must form an orthonormal frame")?;
        let origin = FinitePoint3::new(origin).ok_or("CylinderSurface.origin must be finite")?;
        let radius = PositiveReal::new(radius)
            .ok_or("CylinderSurface.radius must be positive and finite")?;
        Ok(Self {
            origin,
            radius,
            frame,
        })
    }

    /// Return the origin.
    #[must_use]
    pub const fn origin(&self) -> &Point3 {
        self.origin.as_raw()
    }

    /// Return the axis.
    #[must_use]
    pub const fn axis(&self) -> &Vector3 {
        self.frame.axis()
    }

    /// Return the ref direction.
    #[must_use]
    pub const fn ref_direction(&self) -> &Vector3 {
        self.frame.reference()
    }

    /// Return the radius.
    #[must_use]
    pub const fn radius(&self) -> f64 {
        self.radius.get()
    }
}

impl From<CylinderSurface> for CylinderSurfaceWire {
    fn from(value: CylinderSurface) -> Self {
        Self {
            origin: *value.origin(),
            axis: *value.axis(),
            ref_direction: *value.ref_direction(),
            radius: value.radius(),
        }
    }
}

impl TryFrom<CylinderSurfaceWire> for CylinderSurface {
    type Error = &'static str;
    fn try_from(wire: CylinderSurfaceWire) -> Result<Self, Self::Error> {
        Self::try_new(wire.origin, wire.axis, wire.ref_direction, wire.radius)
    }
}

/// Elliptical cone with finite parameters and an orthonormal frame.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(try_from = "ConeSurfaceWire", into = "ConeSurfaceWire")]
pub struct ConeSurface {
    origin: FinitePoint3,
    radius: NonNegativeLength,
    ratio: PositiveReal,
    half_angle: Angle,
    frame: OrthonormalFrame3,
}

#[derive(Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(deny_unknown_fields)]
struct ConeSurfaceWire {
    /// Cone frame origin.
    origin: Point3,
    /// Frame axis direction.
    axis: Vector3,
    /// Frame reference direction, perpendicular to `axis`.
    ref_direction: Vector3,
    /// Cross-section radius at the origin.
    radius: f64,
    /// Ratio of the minor to the major cross-section radius.
    ratio: f64,
    /// Half angle of the cone, in radians.
    half_angle: f64,
}

impl ConeSurface {
    /// Admit finite parameters that satisfy the carrier's numeric contract.
    pub fn try_new(
        origin: Point3,
        axis: Vector3,
        ref_direction: Vector3,
        radius: f64,
        ratio: f64,
        half_angle: f64,
    ) -> Result<Self, &'static str> {
        let frame = OrthonormalFrame3::new(axis, ref_direction)
            .ok_or("ConeSurface.axis/ref_direction must form an orthonormal frame")?;
        let origin = FinitePoint3::new(origin).ok_or("ConeSurface.origin must be finite")?;
        let radius = NonNegativeLength::new(radius)
            .ok_or("ConeSurface.radius must be nonnegative and finite")?;
        let ratio =
            PositiveReal::new(ratio).ok_or("ConeSurface.ratio must be positive and finite")?;
        let half_angle = Angle::new(half_angle).ok_or("ConeSurface.half_angle must be finite")?;
        Ok(Self {
            origin,
            radius,
            ratio,
            half_angle,
            frame,
        })
    }

    /// Return the origin.
    #[must_use]
    pub const fn origin(&self) -> &Point3 {
        self.origin.as_raw()
    }

    /// Return the axis.
    #[must_use]
    pub const fn axis(&self) -> &Vector3 {
        self.frame.axis()
    }

    /// Return the ref direction.
    #[must_use]
    pub const fn ref_direction(&self) -> &Vector3 {
        self.frame.reference()
    }

    /// Return the cross-section radius at the origin. Zero is a valid cone
    /// radius, so the type admits it.
    #[must_use]
    pub const fn radius(&self) -> NonNegativeLength {
        self.radius
    }

    /// Return the ratio of the minor to the major cross-section radius.
    #[must_use]
    pub const fn ratio(&self) -> PositiveReal {
        self.ratio
    }

    /// Return the half angle. Any finite signed angle is a valid cone half
    /// angle, so the type admits it.
    #[must_use]
    pub const fn half_angle(&self) -> Angle {
        self.half_angle
    }
}

impl From<ConeSurface> for ConeSurfaceWire {
    fn from(value: ConeSurface) -> Self {
        Self {
            origin: *value.origin(),
            axis: *value.axis(),
            ref_direction: *value.ref_direction(),
            radius: value.radius().get(),
            ratio: value.ratio().get(),
            half_angle: value.half_angle().get(),
        }
    }
}

impl TryFrom<ConeSurfaceWire> for ConeSurface {
    type Error = &'static str;
    fn try_from(wire: ConeSurfaceWire) -> Result<Self, Self::Error> {
        Self::try_new(
            wire.origin,
            wire.axis,
            wire.ref_direction,
            wire.radius,
            wire.ratio,
            wire.half_angle,
        )
    }
}

/// Sphere with a signed nonzero radius and an orthonormal frame.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(try_from = "SphereSurfaceWire", into = "SphereSurfaceWire")]
pub struct SphereSurface {
    center: FinitePoint3,
    radius: FiniteReal,
    frame: OrthonormalFrame3,
}

#[derive(Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(deny_unknown_fields)]
struct SphereSurfaceWire {
    /// Sphere center.
    center: Point3,
    /// Frame axis direction.
    axis: Vector3,
    /// Frame reference direction, perpendicular to `axis`.
    ref_direction: Vector3,
    /// Signed sphere radius.
    radius: f64,
}

impl SphereSurface {
    /// Admit finite parameters that satisfy the carrier's numeric contract.
    pub fn try_new(
        center: Point3,
        axis: Vector3,
        ref_direction: Vector3,
        radius: f64,
    ) -> Result<Self, &'static str> {
        let frame = OrthonormalFrame3::new(axis, ref_direction)
            .ok_or("SphereSurface.axis/ref_direction must form an orthonormal frame")?;
        if radius == 0.0 {
            return Err("SphereSurface.radius must be nonzero");
        }
        let center = FinitePoint3::new(center).ok_or("SphereSurface.center must be finite")?;
        let radius = FiniteReal::new(radius).ok_or("SphereSurface.radius must be finite")?;
        Ok(Self {
            center,
            radius,
            frame,
        })
    }

    /// Return the center.
    #[must_use]
    pub const fn center(&self) -> &Point3 {
        self.center.as_raw()
    }

    /// Return the axis.
    #[must_use]
    pub const fn axis(&self) -> &Vector3 {
        self.frame.axis()
    }

    /// Return the ref direction.
    #[must_use]
    pub const fn ref_direction(&self) -> &Vector3 {
        self.frame.reference()
    }

    /// Return the radius.
    #[must_use]
    pub const fn radius(&self) -> f64 {
        self.radius.get()
    }
}

impl From<SphereSurface> for SphereSurfaceWire {
    fn from(value: SphereSurface) -> Self {
        Self {
            center: *value.center(),
            axis: *value.axis(),
            ref_direction: *value.ref_direction(),
            radius: value.radius(),
        }
    }
}

impl TryFrom<SphereSurfaceWire> for SphereSurface {
    type Error = &'static str;
    fn try_from(wire: SphereSurfaceWire) -> Result<Self, Self::Error> {
        Self::try_new(wire.center, wire.axis, wire.ref_direction, wire.radius)
    }
}

/// Torus with a positive major radius, signed tube radius, and orthonormal frame.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(try_from = "TorusSurfaceWire", into = "TorusSurfaceWire")]
pub struct TorusSurface {
    center: FinitePoint3,
    major_radius: PositiveReal,
    minor_radius: FiniteReal,
    frame: OrthonormalFrame3,
}

#[derive(Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(deny_unknown_fields)]
struct TorusSurfaceWire {
    /// Torus center.
    center: Point3,
    /// Frame axis direction.
    axis: Vector3,
    /// Frame reference direction, perpendicular to `axis`.
    ref_direction: Vector3,
    /// Distance from the center to the tube center.
    major_radius: f64,
    /// Signed tube radius.
    minor_radius: f64,
}

impl TorusSurface {
    /// Admit finite parameters that satisfy the carrier's numeric contract.
    pub fn try_new(
        center: Point3,
        axis: Vector3,
        ref_direction: Vector3,
        major_radius: f64,
        minor_radius: f64,
    ) -> Result<Self, &'static str> {
        let frame = OrthonormalFrame3::new(axis, ref_direction)
            .ok_or("TorusSurface.axis/ref_direction must form an orthonormal frame")?;
        if minor_radius == 0.0 {
            return Err("TorusSurface.minor_radius must be nonzero");
        }
        let center = FinitePoint3::new(center).ok_or("TorusSurface.center must be finite")?;
        let major_radius = PositiveReal::new(major_radius)
            .ok_or("TorusSurface.major_radius must be positive and finite")?;
        let minor_radius =
            FiniteReal::new(minor_radius).ok_or("TorusSurface.minor_radius must be finite")?;
        Ok(Self {
            center,
            major_radius,
            minor_radius,
            frame,
        })
    }

    /// Return the center.
    #[must_use]
    pub const fn center(&self) -> &Point3 {
        self.center.as_raw()
    }

    /// Return the axis.
    #[must_use]
    pub const fn axis(&self) -> &Vector3 {
        self.frame.axis()
    }

    /// Return the ref direction.
    #[must_use]
    pub const fn ref_direction(&self) -> &Vector3 {
        self.frame.reference()
    }

    /// Return the major radius.
    #[must_use]
    pub const fn major_radius(&self) -> f64 {
        self.major_radius.get()
    }

    /// Return the minor radius.
    #[must_use]
    pub const fn minor_radius(&self) -> f64 {
        self.minor_radius.get()
    }
}

impl From<TorusSurface> for TorusSurfaceWire {
    fn from(value: TorusSurface) -> Self {
        Self {
            center: *value.center(),
            axis: *value.axis(),
            ref_direction: *value.ref_direction(),
            major_radius: value.major_radius(),
            minor_radius: value.minor_radius(),
        }
    }
}

impl TryFrom<TorusSurfaceWire> for TorusSurface {
    type Error = &'static str;
    fn try_from(wire: TorusSurfaceWire) -> Result<Self, Self::Error> {
        Self::try_new(
            wire.center,
            wire.axis,
            wire.ref_direction,
            wire.major_radius,
            wire.minor_radius,
        )
    }
}

/// Line with a finite origin and a unit direction.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(try_from = "LineCurveWire")]
pub struct LineCurve {
    origin: FinitePoint3,
    direction: UnitVector3,
}

#[derive(Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(deny_unknown_fields)]
struct LineCurveWire {
    /// Line origin.
    origin: Point3,
    /// Line direction.
    direction: Vector3,
}

impl LineCurve {
    /// Reverse the curve parameter direction.
    pub fn reverse_parameterization(&mut self) {
        self.direction = self.direction.reversed();
    }

    /// Admit finite parameters that satisfy the carrier's numeric contract.
    pub fn try_new(origin: Point3, direction: Vector3) -> Result<Self, &'static str> {
        let origin = FinitePoint3::new(origin).ok_or("LineCurve.origin must be finite")?;
        let direction =
            UnitVector3::new(direction).ok_or("LineCurve.direction must have unit length")?;
        Ok(Self { origin, direction })
    }

    /// Return the origin.
    #[must_use]
    pub const fn origin(&self) -> &Point3 {
        self.origin.as_raw()
    }

    /// Return the direction.
    #[must_use]
    pub const fn direction(&self) -> &Vector3 {
        self.direction.as_raw()
    }
}

impl TryFrom<LineCurveWire> for LineCurve {
    type Error = &'static str;
    fn try_from(wire: LineCurveWire) -> Result<Self, Self::Error> {
        Self::try_new(wire.origin, wire.direction)
    }
}

/// Circle with a positive radius and an orthonormal frame.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(try_from = "CircleCurveWire", into = "CircleCurveWire")]
pub struct CircleCurve {
    center: FinitePoint3,
    radius: PositiveReal,
    frame: OrthonormalFrame3,
}

#[derive(Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(deny_unknown_fields)]
struct CircleCurveWire {
    /// Circle center.
    center: Point3,
    /// Frame axis direction.
    axis: Vector3,
    /// Frame reference direction, perpendicular to `axis`.
    ref_direction: Vector3,
    /// Circle radius.
    radius: f64,
}

impl CircleCurve {
    /// Reverse the curve parameter direction.
    pub fn reverse_parameterization(&mut self) {
        self.frame.reverse_axis();
    }

    /// Admit finite parameters that satisfy the carrier's numeric contract.
    pub fn try_new(
        center: Point3,
        axis: Vector3,
        ref_direction: Vector3,
        radius: f64,
    ) -> Result<Self, &'static str> {
        let frame = OrthonormalFrame3::new(axis, ref_direction)
            .ok_or("CircleCurve.axis/ref_direction must form an orthonormal frame")?;
        let center = FinitePoint3::new(center).ok_or("CircleCurve.center must be finite")?;
        let radius =
            PositiveReal::new(radius).ok_or("CircleCurve.radius must be positive and finite")?;
        Ok(Self {
            center,
            radius,
            frame,
        })
    }

    /// Return the center.
    #[must_use]
    pub const fn center(&self) -> &Point3 {
        self.center.as_raw()
    }

    /// Return the axis.
    #[must_use]
    pub const fn axis(&self) -> &Vector3 {
        self.frame.axis()
    }

    /// Return the ref direction.
    #[must_use]
    pub const fn ref_direction(&self) -> &Vector3 {
        self.frame.reference()
    }

    /// Return the radius.
    #[must_use]
    pub const fn radius(&self) -> f64 {
        self.radius.get()
    }
}

impl From<CircleCurve> for CircleCurveWire {
    fn from(value: CircleCurve) -> Self {
        Self {
            center: *value.center(),
            axis: *value.axis(),
            ref_direction: *value.ref_direction(),
            radius: value.radius(),
        }
    }
}

impl TryFrom<CircleCurveWire> for CircleCurve {
    type Error = &'static str;
    fn try_from(wire: CircleCurveWire) -> Result<Self, Self::Error> {
        Self::try_new(wire.center, wire.axis, wire.ref_direction, wire.radius)
    }
}

/// Ellipse with ordered positive radii and an orthonormal frame.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(try_from = "EllipseCurveWire", into = "EllipseCurveWire")]
pub struct EllipseCurve {
    center: FinitePoint3,
    major_radius: PositiveReal,
    minor_radius: PositiveReal,
    frame: OrthonormalFrame3,
}

#[derive(Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(deny_unknown_fields)]
struct EllipseCurveWire {
    /// Ellipse center.
    center: Point3,
    /// Frame axis direction.
    axis: Vector3,
    /// Major-axis direction, perpendicular to `axis`.
    major_direction: Vector3,
    /// Major semiaxis radius.
    major_radius: f64,
    /// Minor semiaxis radius.
    minor_radius: f64,
}

impl EllipseCurve {
    /// Reverse the curve parameter direction.
    pub fn reverse_parameterization(&mut self) {
        self.frame.reverse_axis();
    }

    /// Admit finite parameters that satisfy the carrier's numeric contract.
    pub fn try_new(
        center: Point3,
        axis: Vector3,
        major_direction: Vector3,
        major_radius: f64,
        minor_radius: f64,
    ) -> Result<Self, &'static str> {
        let frame = OrthonormalFrame3::new(axis, major_direction)
            .ok_or("EllipseCurve.axis/major_direction must form an orthonormal frame")?;
        if major_radius < minor_radius {
            return Err("EllipseCurve.major_radius must be at least minor_radius");
        }
        let center = FinitePoint3::new(center).ok_or("EllipseCurve.center must be finite")?;
        let major_radius = PositiveReal::new(major_radius)
            .ok_or("EllipseCurve.major_radius must be positive and finite")?;
        let minor_radius = PositiveReal::new(minor_radius)
            .ok_or("EllipseCurve.minor_radius must be positive and finite")?;
        Ok(Self {
            center,
            major_radius,
            minor_radius,
            frame,
        })
    }

    /// Return the center.
    #[must_use]
    pub const fn center(&self) -> &Point3 {
        self.center.as_raw()
    }

    /// Return the axis.
    #[must_use]
    pub const fn axis(&self) -> &Vector3 {
        self.frame.axis()
    }

    /// Return the major direction.
    #[must_use]
    pub const fn major_direction(&self) -> &Vector3 {
        self.frame.reference()
    }

    /// Return the major radius.
    #[must_use]
    pub const fn major_radius(&self) -> f64 {
        self.major_radius.get()
    }

    /// Return the minor radius.
    #[must_use]
    pub const fn minor_radius(&self) -> f64 {
        self.minor_radius.get()
    }
}

impl From<EllipseCurve> for EllipseCurveWire {
    fn from(value: EllipseCurve) -> Self {
        Self {
            center: *value.center(),
            axis: *value.axis(),
            major_direction: *value.major_direction(),
            major_radius: value.major_radius(),
            minor_radius: value.minor_radius(),
        }
    }
}

impl TryFrom<EllipseCurveWire> for EllipseCurve {
    type Error = &'static str;
    fn try_from(wire: EllipseCurveWire) -> Result<Self, Self::Error> {
        Self::try_new(
            wire.center,
            wire.axis,
            wire.major_direction,
            wire.major_radius,
            wire.minor_radius,
        )
    }
}

/// Parabola with a positive focal distance and an orthonormal frame.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(try_from = "ParabolaCurveWire", into = "ParabolaCurveWire")]
pub struct ParabolaCurve {
    vertex: FinitePoint3,
    focal_distance: PositiveReal,
    frame: OrthonormalFrame3,
}

#[derive(Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(deny_unknown_fields)]
struct ParabolaCurveWire {
    /// Parabola vertex.
    vertex: Point3,
    /// Frame axis direction.
    axis: Vector3,
    /// Major-axis direction, perpendicular to `axis`.
    major_direction: Vector3,
    /// Distance from the vertex to the focus.
    focal_distance: f64,
}

impl ParabolaCurve {
    /// Admit finite parameters that satisfy the carrier's numeric contract.
    pub fn try_new(
        vertex: Point3,
        axis: Vector3,
        major_direction: Vector3,
        focal_distance: f64,
    ) -> Result<Self, &'static str> {
        let frame = OrthonormalFrame3::new(axis, major_direction)
            .ok_or("ParabolaCurve.axis/major_direction must form an orthonormal frame")?;
        let vertex = FinitePoint3::new(vertex).ok_or("ParabolaCurve.vertex must be finite")?;
        let focal_distance = PositiveReal::new(focal_distance)
            .ok_or("ParabolaCurve.focal_distance must be positive and finite")?;
        Ok(Self {
            vertex,
            focal_distance,
            frame,
        })
    }

    /// Return the vertex.
    #[must_use]
    pub const fn vertex(&self) -> &Point3 {
        self.vertex.as_raw()
    }

    /// Return the axis.
    #[must_use]
    pub const fn axis(&self) -> &Vector3 {
        self.frame.axis()
    }

    /// Return the major direction.
    #[must_use]
    pub const fn major_direction(&self) -> &Vector3 {
        self.frame.reference()
    }

    /// Return the focal distance.
    #[must_use]
    pub const fn focal_distance(&self) -> f64 {
        self.focal_distance.get()
    }
}

impl From<ParabolaCurve> for ParabolaCurveWire {
    fn from(value: ParabolaCurve) -> Self {
        Self {
            vertex: *value.vertex(),
            axis: *value.axis(),
            major_direction: *value.major_direction(),
            focal_distance: value.focal_distance(),
        }
    }
}

impl TryFrom<ParabolaCurveWire> for ParabolaCurve {
    type Error = &'static str;
    fn try_from(wire: ParabolaCurveWire) -> Result<Self, Self::Error> {
        Self::try_new(
            wire.vertex,
            wire.axis,
            wire.major_direction,
            wire.focal_distance,
        )
    }
}

/// Hyperbola with positive radii and an orthonormal frame.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(try_from = "HyperbolaCurveWire", into = "HyperbolaCurveWire")]
pub struct HyperbolaCurve {
    center: FinitePoint3,
    major_radius: PositiveReal,
    minor_radius: PositiveReal,
    frame: OrthonormalFrame3,
}

#[derive(Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(deny_unknown_fields)]
struct HyperbolaCurveWire {
    /// Hyperbola center.
    center: Point3,
    /// Frame axis direction.
    axis: Vector3,
    /// Major-axis direction, perpendicular to `axis`.
    major_direction: Vector3,
    /// Transverse semiaxis radius.
    major_radius: f64,
    /// Conjugate semiaxis radius.
    minor_radius: f64,
}

impl HyperbolaCurve {
    /// Return the opposite branch with unchanged radii.
    #[must_use]
    pub fn opposite_branch(mut self) -> Self {
        self.frame.reverse_reference();
        self
    }

    /// Admit finite parameters that satisfy the carrier's numeric contract.
    pub fn try_new(
        center: Point3,
        axis: Vector3,
        major_direction: Vector3,
        major_radius: f64,
        minor_radius: f64,
    ) -> Result<Self, &'static str> {
        let frame = OrthonormalFrame3::new(axis, major_direction)
            .ok_or("HyperbolaCurve.axis/major_direction must form an orthonormal frame")?;
        let center = FinitePoint3::new(center).ok_or("HyperbolaCurve.center must be finite")?;
        let major_radius = PositiveReal::new(major_radius)
            .ok_or("HyperbolaCurve.major_radius must be positive and finite")?;
        let minor_radius = PositiveReal::new(minor_radius)
            .ok_or("HyperbolaCurve.minor_radius must be positive and finite")?;
        Ok(Self {
            center,
            major_radius,
            minor_radius,
            frame,
        })
    }

    /// Return the center.
    #[must_use]
    pub const fn center(&self) -> &Point3 {
        self.center.as_raw()
    }

    /// Return the axis.
    #[must_use]
    pub const fn axis(&self) -> &Vector3 {
        self.frame.axis()
    }

    /// Return the major direction.
    #[must_use]
    pub const fn major_direction(&self) -> &Vector3 {
        self.frame.reference()
    }

    /// Return the major radius.
    #[must_use]
    pub const fn major_radius(&self) -> f64 {
        self.major_radius.get()
    }

    /// Return the minor radius.
    #[must_use]
    pub const fn minor_radius(&self) -> f64 {
        self.minor_radius.get()
    }
}

impl From<HyperbolaCurve> for HyperbolaCurveWire {
    fn from(value: HyperbolaCurve) -> Self {
        Self {
            center: *value.center(),
            axis: *value.axis(),
            major_direction: *value.major_direction(),
            major_radius: value.major_radius(),
            minor_radius: value.minor_radius(),
        }
    }
}

impl TryFrom<HyperbolaCurveWire> for HyperbolaCurve {
    type Error = &'static str;
    fn try_from(wire: HyperbolaCurveWire) -> Result<Self, Self::Error> {
        Self::try_new(
            wire.center,
            wire.axis,
            wire.major_direction,
            wire.major_radius,
            wire.minor_radius,
        )
    }
}

/// Degenerate curve at a finite point.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(try_from = "DegenerateCurveWire")]
pub struct DegenerateCurve {
    point: FinitePoint3,
}

#[derive(Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(deny_unknown_fields)]
struct DegenerateCurveWire {
    /// Point the curve degenerates to.
    point: Point3,
}

impl DegenerateCurve {
    /// Admit finite parameters that satisfy the carrier's numeric contract.
    pub fn try_new(point: Point3) -> Result<Self, &'static str> {
        let point = FinitePoint3::new(point).ok_or("DegenerateCurve.point must be finite")?;
        Ok(Self { point })
    }

    /// Return the point.
    #[must_use]
    pub const fn point(&self) -> &Point3 {
        self.point.as_raw()
    }
}

impl TryFrom<DegenerateCurveWire> for DegenerateCurve {
    type Error = &'static str;
    fn try_from(wire: DegenerateCurveWire) -> Result<Self, Self::Error> {
        Self::try_new(wire.point)
    }
}

#[cfg(test)]
mod tests;
