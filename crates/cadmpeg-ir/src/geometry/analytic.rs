// SPDX-License-Identifier: Apache-2.0
//! Analytic curves and surfaces with admitted frames and dimensions.
//!
//! Each type states what its caller supplies through three constructor forms:
//!
//! | Inputs | Constructor |
//! |---|---|
//! | Raw values requiring admission | `try_new(...) -> Result<Self, &'static str>` |
//! | Checked parts that establish every invariant | `new(...) -> Self` |
//! | Checked parts with a remaining relationship | `try_from_parts(...) -> Result<Self, &'static str>` |
//!
//! `try_new` admits each raw component and delegates, so wire deserialization
//! and model code reach the same stored state through one admission path. A
//! getter returns the checked type of the stored value, so an unchanged
//! component moves into another model object without a second admission.
//!
//! Raw construction reports component admission failures before relationships
//! between components. For example, an ellipse with radii `-1` and `2` reports
//! the invalid major radius before checking radius ordering. Sphere radii and
//! torus minor radii use a single field-specific finite/nonzero error for both
//! restrictions. This replaces the separate zero check used before checked
//! constructor composition; callers must not depend on the old error precedence.

use crate::features::FinitePoint3;
use crate::math::{Point3, Vector3};
use crate::scalar::{Angle, NonNegativeLength, NonZeroLength, PositiveLength, PositiveReal};
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
    /// Build a plane from checked parts. The argument types state the whole
    /// invariant, so nothing is checked again.
    ///
    /// Checked parts taken from one plane rebuild another without readmission:
    ///
    /// ```
    /// use cadmpeg_ir::geometry::analytic::PlaneSurface;
    /// use cadmpeg_ir::math::{Point3, Vector3};
    ///
    /// let plane = PlaneSurface::try_new(
    ///     Point3::new(1.0, 2.0, 3.0),
    ///     Vector3::new(0.0, 0.0, 1.0),
    ///     Vector3::new(1.0, 0.0, 0.0),
    /// )
    /// .expect("orthonormal frame and finite origin");
    /// let moved = PlaneSurface::new(plane.origin(), plane.frame());
    /// assert_eq!(moved, plane);
    /// ```
    ///
    /// Raw coordinates and directions do not reach it:
    ///
    /// ```compile_fail
    /// use cadmpeg_ir::geometry::analytic::PlaneSurface;
    /// use cadmpeg_ir::math::{Point3, Vector3};
    ///
    /// let plane = PlaneSurface::new(
    ///     Point3::new(1.0, 2.0, 3.0),
    ///     Vector3::new(0.0, 0.0, 1.0),
    /// );
    /// ```
    #[must_use]
    pub const fn new(origin: FinitePoint3, frame: OrthonormalFrame3) -> Self {
        Self { origin, frame }
    }

    /// Admit finite parameters that satisfy the carrier's numeric contract.
    pub fn try_new(origin: Point3, normal: Vector3, u_axis: Vector3) -> Result<Self, &'static str> {
        let frame = OrthonormalFrame3::new(normal, u_axis)
            .ok_or("PlaneSurface.normal/u_axis must form an orthonormal frame")?;
        let origin = FinitePoint3::new(origin).ok_or("PlaneSurface.origin must be finite")?;
        Ok(Self::new(origin, frame))
    }

    /// Return the origin.
    #[must_use]
    pub const fn origin(&self) -> FinitePoint3 {
        self.origin
    }

    /// Return the frame.
    #[must_use]
    pub const fn frame(&self) -> OrthonormalFrame3 {
        self.frame
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
            origin: value.origin().get(),
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
    radius: PositiveLength,
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
    /// Build a cylinder from checked parts. The argument types state the whole
    /// invariant, so nothing is checked again.
    ///
    /// A raw radius has no way in:
    ///
    /// ```compile_fail
    /// use cadmpeg_ir::geometry::analytic::CylinderSurface;
    /// use cadmpeg_ir::math::{Point3, Vector3};
    ///
    /// let cylinder = CylinderSurface::try_new(
    ///     Point3::new(0.0, 0.0, 0.0),
    ///     Vector3::new(0.0, 0.0, 1.0),
    ///     Vector3::new(1.0, 0.0, 0.0),
    ///     2.0,
    /// )
    /// .expect("orthonormal frame, finite origin and positive radius");
    /// let wider = CylinderSurface::new(cylinder.origin(), cylinder.frame(), 4.0);
    /// ```
    ///
    /// An admitted one does:
    ///
    /// ```
    /// use cadmpeg_ir::geometry::analytic::CylinderSurface;
    /// use cadmpeg_ir::math::{Point3, Vector3};
    /// use cadmpeg_ir::scalar::PositiveLength;
    ///
    /// let cylinder = CylinderSurface::try_new(
    ///     Point3::new(0.0, 0.0, 0.0),
    ///     Vector3::new(0.0, 0.0, 1.0),
    ///     Vector3::new(1.0, 0.0, 0.0),
    ///     2.0,
    /// )
    /// .expect("orthonormal frame, finite origin and positive radius");
    /// let wider = CylinderSurface::new(
    ///     cylinder.origin(),
    ///     cylinder.frame(),
    ///     PositiveLength::new(4.0).expect("positive finite"),
    /// );
    /// assert_eq!(wider.radius().get(), 4.0);
    /// ```
    #[must_use]
    pub const fn new(
        origin: FinitePoint3,
        frame: OrthonormalFrame3,
        radius: PositiveLength,
    ) -> Self {
        Self {
            origin,
            radius,
            frame,
        }
    }

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
        let radius = PositiveLength::new(radius)
            .ok_or("CylinderSurface.radius must be positive and finite")?;
        Ok(Self::new(origin, frame, radius))
    }

    /// Return the origin.
    #[must_use]
    pub const fn origin(&self) -> FinitePoint3 {
        self.origin
    }

    /// Return the frame.
    #[must_use]
    pub const fn frame(&self) -> OrthonormalFrame3 {
        self.frame
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
    pub const fn radius(&self) -> PositiveLength {
        self.radius
    }
}

impl From<CylinderSurface> for CylinderSurfaceWire {
    fn from(value: CylinderSurface) -> Self {
        Self {
            origin: value.origin().get(),
            axis: *value.axis(),
            ref_direction: *value.ref_direction(),
            radius: value.radius().get(),
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
    /// Build a cone from checked parts. The argument types state the whole
    /// invariant, so nothing is checked again.
    ///
    /// One component is replaced and the rest move across unchanged:
    ///
    /// ```
    /// use cadmpeg_ir::geometry::analytic::ConeSurface;
    /// use cadmpeg_ir::math::{Point3, Vector3};
    /// use cadmpeg_ir::scalar::NonNegativeLength;
    ///
    /// let cone = ConeSurface::try_new(
    ///     Point3::new(0.0, 0.0, 0.0),
    ///     Vector3::new(0.0, 0.0, 1.0),
    ///     Vector3::new(1.0, 0.0, 0.0),
    ///     3.0,
    ///     1.0,
    ///     0.5,
    /// )
    /// .expect("orthonormal frame, finite origin and admitted dimensions");
    /// let sharpened = ConeSurface::new(
    ///     cone.origin(),
    ///     cone.frame(),
    ///     NonNegativeLength::new(0.0).expect("zero is a valid cone radius"),
    ///     cone.ratio(),
    ///     cone.half_angle(),
    /// );
    /// assert_eq!(sharpened.radius().get(), 0.0);
    /// assert_eq!(sharpened.half_angle(), cone.half_angle());
    /// ```
    #[must_use]
    pub const fn new(
        origin: FinitePoint3,
        frame: OrthonormalFrame3,
        radius: NonNegativeLength,
        ratio: PositiveReal,
        half_angle: Angle,
    ) -> Self {
        Self {
            origin,
            radius,
            ratio,
            half_angle,
            frame,
        }
    }

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
        Ok(Self::new(origin, frame, radius, ratio, half_angle))
    }

    /// Return the origin.
    #[must_use]
    pub const fn origin(&self) -> FinitePoint3 {
        self.origin
    }

    /// Return the frame.
    #[must_use]
    pub const fn frame(&self) -> OrthonormalFrame3 {
        self.frame
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
            origin: value.origin().get(),
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
    radius: NonZeroLength,
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
    /// Build a sphere from checked parts. The argument types state the whole
    /// invariant, so nothing is checked again.
    ///
    /// A sphere rebuilds from its own parts, and a negative radius stays
    /// negative:
    ///
    /// ```
    /// use cadmpeg_ir::geometry::analytic::SphereSurface;
    /// use cadmpeg_ir::math::{Point3, Vector3};
    ///
    /// let sphere = SphereSurface::try_new(
    ///     Point3::new(0.0, 0.0, 0.0),
    ///     Vector3::new(0.0, 0.0, 1.0),
    ///     Vector3::new(1.0, 0.0, 0.0),
    ///     -2.0,
    /// )
    /// .expect("orthonormal frame, finite center and nonzero radius");
    /// let moved = SphereSurface::new(sphere.center(), sphere.frame(), sphere.radius());
    /// assert_eq!(moved, sphere);
    /// assert_eq!(moved.radius().get(), -2.0);
    /// ```
    #[must_use]
    pub const fn new(
        center: FinitePoint3,
        frame: OrthonormalFrame3,
        radius: NonZeroLength,
    ) -> Self {
        Self {
            center,
            radius,
            frame,
        }
    }

    /// Admit finite parameters that satisfy the carrier's numeric contract.
    pub fn try_new(
        center: Point3,
        axis: Vector3,
        ref_direction: Vector3,
        radius: f64,
    ) -> Result<Self, &'static str> {
        let frame = OrthonormalFrame3::new(axis, ref_direction)
            .ok_or("SphereSurface.axis/ref_direction must form an orthonormal frame")?;
        let center = FinitePoint3::new(center).ok_or("SphereSurface.center must be finite")?;
        let radius =
            NonZeroLength::new(radius).ok_or("SphereSurface.radius must be finite and nonzero")?;
        Ok(Self::new(center, frame, radius))
    }

    /// Return the center.
    #[must_use]
    pub const fn center(&self) -> FinitePoint3 {
        self.center
    }

    /// Return the frame.
    #[must_use]
    pub const fn frame(&self) -> OrthonormalFrame3 {
        self.frame
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

    /// Return the radius. A sphere radius is signed and nonzero, so the type
    /// admits both signs.
    #[must_use]
    pub const fn radius(&self) -> NonZeroLength {
        self.radius
    }
}

impl From<SphereSurface> for SphereSurfaceWire {
    fn from(value: SphereSurface) -> Self {
        Self {
            center: value.center().get(),
            axis: *value.axis(),
            ref_direction: *value.ref_direction(),
            radius: value.radius().get(),
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
    major_radius: PositiveLength,
    minor_radius: NonZeroLength,
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
    /// Build a torus from checked parts. The argument types state the whole
    /// invariant, so nothing is checked again.
    ///
    /// The two radii carry different restrictions, so exchanging them does not
    /// compile:
    ///
    /// ```compile_fail
    /// use cadmpeg_ir::geometry::analytic::TorusSurface;
    /// use cadmpeg_ir::math::{Point3, Vector3};
    ///
    /// let torus = TorusSurface::try_new(
    ///     Point3::new(0.0, 0.0, 0.0),
    ///     Vector3::new(0.0, 0.0, 1.0),
    ///     Vector3::new(1.0, 0.0, 0.0),
    ///     5.0,
    ///     1.0,
    /// )
    /// .expect("orthonormal frame, finite center and admitted radii");
    /// let swapped = TorusSurface::new(
    ///     torus.center(),
    ///     torus.frame(),
    ///     torus.minor_radius(),
    ///     torus.major_radius(),
    /// );
    /// ```
    ///
    /// Each radius in its own place does:
    ///
    /// ```
    /// use cadmpeg_ir::geometry::analytic::TorusSurface;
    /// use cadmpeg_ir::math::{Point3, Vector3};
    ///
    /// let torus = TorusSurface::try_new(
    ///     Point3::new(0.0, 0.0, 0.0),
    ///     Vector3::new(0.0, 0.0, 1.0),
    ///     Vector3::new(1.0, 0.0, 0.0),
    ///     5.0,
    ///     1.0,
    /// )
    /// .expect("orthonormal frame, finite center and admitted radii");
    /// let moved = TorusSurface::new(
    ///     torus.center(),
    ///     torus.frame(),
    ///     torus.major_radius(),
    ///     torus.minor_radius(),
    /// );
    /// assert_eq!(moved, torus);
    /// ```
    #[must_use]
    pub const fn new(
        center: FinitePoint3,
        frame: OrthonormalFrame3,
        major_radius: PositiveLength,
        minor_radius: NonZeroLength,
    ) -> Self {
        Self {
            center,
            major_radius,
            minor_radius,
            frame,
        }
    }

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
        let center = FinitePoint3::new(center).ok_or("TorusSurface.center must be finite")?;
        let major_radius = PositiveLength::new(major_radius)
            .ok_or("TorusSurface.major_radius must be positive and finite")?;
        let minor_radius = NonZeroLength::new(minor_radius)
            .ok_or("TorusSurface.minor_radius must be finite and nonzero")?;
        Ok(Self::new(center, frame, major_radius, minor_radius))
    }

    /// Return the center.
    #[must_use]
    pub const fn center(&self) -> FinitePoint3 {
        self.center
    }

    /// Return the frame.
    #[must_use]
    pub const fn frame(&self) -> OrthonormalFrame3 {
        self.frame
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
    pub const fn major_radius(&self) -> PositiveLength {
        self.major_radius
    }

    /// Return the minor radius. A torus tube radius is signed and nonzero, so
    /// the type admits both signs.
    #[must_use]
    pub const fn minor_radius(&self) -> NonZeroLength {
        self.minor_radius
    }
}

impl From<TorusSurface> for TorusSurfaceWire {
    fn from(value: TorusSurface) -> Self {
        Self {
            center: value.center().get(),
            axis: *value.axis(),
            ref_direction: *value.ref_direction(),
            major_radius: value.major_radius().get(),
            minor_radius: value.minor_radius().get(),
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

    /// Build a line from checked parts. The argument types state the whole
    /// invariant, so nothing is checked again.
    ///
    /// A frame hands back an admitted direction, which builds a line along the
    /// axis without a second admission:
    ///
    /// ```
    /// use cadmpeg_ir::geometry::analytic::{LineCurve, PlaneSurface};
    /// use cadmpeg_ir::math::{Point3, Vector3};
    ///
    /// let plane = PlaneSurface::try_new(
    ///     Point3::new(1.0, 2.0, 3.0),
    ///     Vector3::new(0.0, 0.0, 1.0),
    ///     Vector3::new(1.0, 0.0, 0.0),
    /// )
    /// .expect("orthonormal frame and finite origin");
    /// let axis_line = LineCurve::new(plane.origin(), plane.frame().unit_axis());
    /// assert_eq!(axis_line.direction().as_raw(), plane.normal());
    /// ```
    ///
    /// A raw direction has no way in:
    ///
    /// ```compile_fail
    /// use cadmpeg_ir::geometry::analytic::LineCurve;
    /// use cadmpeg_ir::math::{Point3, Vector3};
    ///
    /// let line = LineCurve::try_new(
    ///     Point3::new(0.0, 0.0, 0.0),
    ///     Vector3::new(0.0, 0.0, 1.0),
    /// )
    /// .expect("finite origin and unit direction");
    /// let tilted = LineCurve::new(line.origin(), Vector3::new(1.0, 0.0, 0.0));
    /// ```
    #[must_use]
    pub const fn new(origin: FinitePoint3, direction: UnitVector3) -> Self {
        Self { origin, direction }
    }

    /// Admit finite parameters that satisfy the carrier's numeric contract.
    pub fn try_new(origin: Point3, direction: Vector3) -> Result<Self, &'static str> {
        let origin = FinitePoint3::new(origin).ok_or("LineCurve.origin must be finite")?;
        let direction =
            UnitVector3::new(direction).ok_or("LineCurve.direction must have unit length")?;
        Ok(Self::new(origin, direction))
    }

    /// Return the origin.
    #[must_use]
    pub const fn origin(&self) -> FinitePoint3 {
        self.origin
    }

    /// Return the direction.
    #[must_use]
    pub const fn direction(&self) -> UnitVector3 {
        self.direction
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
    radius: PositiveLength,
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

    /// Build a circle from checked parts. The argument types state the whole
    /// invariant, so nothing is checked again.
    ///
    /// A circle widens on an admitted radius and keeps the rest of its parts:
    ///
    /// ```
    /// use cadmpeg_ir::geometry::analytic::CircleCurve;
    /// use cadmpeg_ir::math::{Point3, Vector3};
    /// use cadmpeg_ir::scalar::PositiveLength;
    ///
    /// let circle = CircleCurve::try_new(
    ///     Point3::new(0.0, 0.0, 0.0),
    ///     Vector3::new(0.0, 0.0, 1.0),
    ///     Vector3::new(1.0, 0.0, 0.0),
    ///     2.0,
    /// )
    /// .expect("orthonormal frame, finite center and positive radius");
    /// let wider = CircleCurve::new(
    ///     circle.center(),
    ///     circle.frame(),
    ///     PositiveLength::new(4.0).expect("positive finite"),
    /// );
    /// assert_eq!(wider.radius().get(), 4.0);
    /// assert_eq!(wider.center(), circle.center());
    /// ```
    #[must_use]
    pub const fn new(
        center: FinitePoint3,
        frame: OrthonormalFrame3,
        radius: PositiveLength,
    ) -> Self {
        Self {
            center,
            radius,
            frame,
        }
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
            PositiveLength::new(radius).ok_or("CircleCurve.radius must be positive and finite")?;
        Ok(Self::new(center, frame, radius))
    }

    /// Return the center.
    #[must_use]
    pub const fn center(&self) -> FinitePoint3 {
        self.center
    }

    /// Return the frame.
    #[must_use]
    pub const fn frame(&self) -> OrthonormalFrame3 {
        self.frame
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
    pub const fn radius(&self) -> PositiveLength {
        self.radius
    }
}

impl From<CircleCurve> for CircleCurveWire {
    fn from(value: CircleCurve) -> Self {
        Self {
            center: value.center().get(),
            axis: *value.axis(),
            ref_direction: *value.ref_direction(),
            radius: value.radius().get(),
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
    major_radius: PositiveLength,
    minor_radius: PositiveLength,
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

    /// Build an ellipse from checked parts. Two positive radii do not state
    /// their order, so the remaining relationship is checked here.
    ///
    /// An ellipse rebuilds from its own parts:
    ///
    /// ```
    /// use cadmpeg_ir::geometry::analytic::EllipseCurve;
    /// use cadmpeg_ir::math::{Point3, Vector3};
    ///
    /// let ellipse = EllipseCurve::try_new(
    ///     Point3::new(0.0, 0.0, 0.0),
    ///     Vector3::new(0.0, 0.0, 1.0),
    ///     Vector3::new(1.0, 0.0, 0.0),
    ///     4.0,
    ///     2.0,
    /// )
    /// .expect("orthonormal frame, finite center and ordered radii");
    /// let moved = EllipseCurve::try_from_parts(
    ///     ellipse.center(),
    ///     ellipse.frame(),
    ///     ellipse.major_radius(),
    ///     ellipse.minor_radius(),
    /// )
    /// .expect("the stored radii keep their order");
    /// assert_eq!(moved, ellipse);
    /// ```
    ///
    /// Exchanging them is refused, because the argument types state positivity
    /// and not the order:
    ///
    /// ```
    /// use cadmpeg_ir::geometry::analytic::EllipseCurve;
    /// use cadmpeg_ir::math::{Point3, Vector3};
    ///
    /// let ellipse = EllipseCurve::try_new(
    ///     Point3::new(0.0, 0.0, 0.0),
    ///     Vector3::new(0.0, 0.0, 1.0),
    ///     Vector3::new(1.0, 0.0, 0.0),
    ///     4.0,
    ///     2.0,
    /// )
    /// .expect("orthonormal frame, finite center and ordered radii");
    /// assert!(EllipseCurve::try_from_parts(
    ///     ellipse.center(),
    ///     ellipse.frame(),
    ///     ellipse.minor_radius(),
    ///     ellipse.major_radius(),
    /// )
    /// .is_err());
    /// ```
    pub fn try_from_parts(
        center: FinitePoint3,
        frame: OrthonormalFrame3,
        major_radius: PositiveLength,
        minor_radius: PositiveLength,
    ) -> Result<Self, &'static str> {
        if major_radius.get() < minor_radius.get() {
            return Err("EllipseCurve.major_radius must be at least minor_radius");
        }
        Ok(Self {
            center,
            major_radius,
            minor_radius,
            frame,
        })
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
        let center = FinitePoint3::new(center).ok_or("EllipseCurve.center must be finite")?;
        let major_radius = PositiveLength::new(major_radius)
            .ok_or("EllipseCurve.major_radius must be positive and finite")?;
        let minor_radius = PositiveLength::new(minor_radius)
            .ok_or("EllipseCurve.minor_radius must be positive and finite")?;
        Self::try_from_parts(center, frame, major_radius, minor_radius)
    }

    /// Return the center.
    #[must_use]
    pub const fn center(&self) -> FinitePoint3 {
        self.center
    }

    /// Return the frame.
    #[must_use]
    pub const fn frame(&self) -> OrthonormalFrame3 {
        self.frame
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
    pub const fn major_radius(&self) -> PositiveLength {
        self.major_radius
    }

    /// Return the minor radius.
    #[must_use]
    pub const fn minor_radius(&self) -> PositiveLength {
        self.minor_radius
    }
}

impl From<EllipseCurve> for EllipseCurveWire {
    fn from(value: EllipseCurve) -> Self {
        Self {
            center: value.center().get(),
            axis: *value.axis(),
            major_direction: *value.major_direction(),
            major_radius: value.major_radius().get(),
            minor_radius: value.minor_radius().get(),
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
    focal_distance: PositiveLength,
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
    /// Build a parabola from checked parts. The argument types state the whole
    /// invariant, so nothing is checked again.
    ///
    /// A parabola rebuilds from its own parts:
    ///
    /// ```
    /// use cadmpeg_ir::geometry::analytic::ParabolaCurve;
    /// use cadmpeg_ir::math::{Point3, Vector3};
    ///
    /// let parabola = ParabolaCurve::try_new(
    ///     Point3::new(1.0, 2.0, 3.0),
    ///     Vector3::new(0.0, 0.0, 1.0),
    ///     Vector3::new(1.0, 0.0, 0.0),
    ///     2.0,
    /// )
    /// .expect("orthonormal frame, finite vertex and positive focal distance");
    /// let moved = ParabolaCurve::new(
    ///     parabola.vertex(),
    ///     parabola.frame(),
    ///     parabola.focal_distance(),
    /// );
    /// assert_eq!(moved, parabola);
    /// ```
    #[must_use]
    pub const fn new(
        vertex: FinitePoint3,
        frame: OrthonormalFrame3,
        focal_distance: PositiveLength,
    ) -> Self {
        Self {
            vertex,
            focal_distance,
            frame,
        }
    }

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
        let focal_distance = PositiveLength::new(focal_distance)
            .ok_or("ParabolaCurve.focal_distance must be positive and finite")?;
        Ok(Self::new(vertex, frame, focal_distance))
    }

    /// Return the vertex.
    #[must_use]
    pub const fn vertex(&self) -> FinitePoint3 {
        self.vertex
    }

    /// Return the frame.
    #[must_use]
    pub const fn frame(&self) -> OrthonormalFrame3 {
        self.frame
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
    pub const fn focal_distance(&self) -> PositiveLength {
        self.focal_distance
    }
}

impl From<ParabolaCurve> for ParabolaCurveWire {
    fn from(value: ParabolaCurve) -> Self {
        Self {
            vertex: value.vertex().get(),
            axis: *value.axis(),
            major_direction: *value.major_direction(),
            focal_distance: value.focal_distance().get(),
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
    major_radius: PositiveLength,
    minor_radius: PositiveLength,
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

    /// Build a hyperbola from checked parts. The argument types state the whole
    /// invariant, so nothing is checked again. A hyperbola carries no radius
    /// ordering, so two positive radii state everything its fields require.
    ///
    /// A hyperbola rebuilds from its own parts, in either order:
    ///
    /// ```
    /// use cadmpeg_ir::geometry::analytic::HyperbolaCurve;
    /// use cadmpeg_ir::math::{Point3, Vector3};
    ///
    /// let hyperbola = HyperbolaCurve::try_new(
    ///     Point3::new(0.0, 0.0, 0.0),
    ///     Vector3::new(0.0, 0.0, 1.0),
    ///     Vector3::new(1.0, 0.0, 0.0),
    ///     2.0,
    ///     5.0,
    /// )
    /// .expect("orthonormal frame, finite center and positive radii");
    /// let moved = HyperbolaCurve::new(
    ///     hyperbola.center(),
    ///     hyperbola.frame(),
    ///     hyperbola.major_radius(),
    ///     hyperbola.minor_radius(),
    /// );
    /// assert_eq!(moved, hyperbola);
    /// assert_eq!(moved.major_radius().get(), 2.0);
    /// ```
    #[must_use]
    pub const fn new(
        center: FinitePoint3,
        frame: OrthonormalFrame3,
        major_radius: PositiveLength,
        minor_radius: PositiveLength,
    ) -> Self {
        Self {
            center,
            major_radius,
            minor_radius,
            frame,
        }
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
        let major_radius = PositiveLength::new(major_radius)
            .ok_or("HyperbolaCurve.major_radius must be positive and finite")?;
        let minor_radius = PositiveLength::new(minor_radius)
            .ok_or("HyperbolaCurve.minor_radius must be positive and finite")?;
        Ok(Self::new(center, frame, major_radius, minor_radius))
    }

    /// Return the center.
    #[must_use]
    pub const fn center(&self) -> FinitePoint3 {
        self.center
    }

    /// Return the frame.
    #[must_use]
    pub const fn frame(&self) -> OrthonormalFrame3 {
        self.frame
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
    pub const fn major_radius(&self) -> PositiveLength {
        self.major_radius
    }

    /// Return the minor radius.
    #[must_use]
    pub const fn minor_radius(&self) -> PositiveLength {
        self.minor_radius
    }
}

impl From<HyperbolaCurve> for HyperbolaCurveWire {
    fn from(value: HyperbolaCurve) -> Self {
        Self {
            center: value.center().get(),
            axis: *value.axis(),
            major_direction: *value.major_direction(),
            major_radius: value.major_radius().get(),
            minor_radius: value.minor_radius().get(),
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
    /// Build a degenerate curve from a checked point. The argument type states
    /// the whole invariant, so nothing is checked again.
    ///
    /// A line's admitted origin degenerates without readmission:
    ///
    /// ```
    /// use cadmpeg_ir::geometry::analytic::{DegenerateCurve, LineCurve};
    /// use cadmpeg_ir::math::{Point3, Vector3};
    ///
    /// let line = LineCurve::try_new(
    ///     Point3::new(1.0, 2.0, 3.0),
    ///     Vector3::new(0.0, 0.0, 1.0),
    /// )
    /// .expect("finite origin and unit direction");
    /// let degenerate = DegenerateCurve::new(line.origin());
    /// assert_eq!(degenerate.point(), line.origin());
    /// ```
    ///
    /// A raw point has no way in:
    ///
    /// ```compile_fail
    /// use cadmpeg_ir::geometry::analytic::DegenerateCurve;
    /// use cadmpeg_ir::math::Point3;
    ///
    /// let degenerate = DegenerateCurve::new(Point3::new(1.0, 2.0, 3.0));
    /// ```
    #[must_use]
    pub const fn new(point: FinitePoint3) -> Self {
        Self { point }
    }

    /// Admit finite parameters that satisfy the carrier's numeric contract.
    pub fn try_new(point: Point3) -> Result<Self, &'static str> {
        let point = FinitePoint3::new(point).ok_or("DegenerateCurve.point must be finite")?;
        Ok(Self::new(point))
    }

    /// Return the point.
    #[must_use]
    pub const fn point(&self) -> FinitePoint3 {
        self.point
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
