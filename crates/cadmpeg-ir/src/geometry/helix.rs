// SPDX-License-Identifier: Apache-2.0
//! Helix path, curve, and surface constructions.

use super::legacy_cache::LegacyCache;
use crate::features::geometry::{FinitePoint3, FiniteVector3};
use crate::math::{Point3, Vector3};
use crate::scalar::FiniteReal;
use crate::units::FiniteVector;
#[cfg(feature = "schema")]
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

pub(super) const EPS_HELIX_SURFACE_RADIUS_RELATIVE: f64 = 1.0e-9;
pub(super) const EPS_HELIX_CURVE_RADIUS: f64 = 1.0e-9;

/// Finite circular path of a helix surface.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize, cadmpeg_derive::StructuralOrd)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(try_from = "HelixPathConstructionWire")]
pub struct HelixPathConstruction {
    pub(super) angle_range: FiniteVector<2>,
    pub(super) center: FinitePoint3,
    pub(super) major: FiniteVector3,
    pub(super) minor: FiniteVector3,
    pub(super) pitch: FiniteVector3,
    pub(super) apex_factor: FiniteReal,
    pub(super) axis: FiniteVector3,
}

#[derive(Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(deny_unknown_fields)]
pub(super) struct HelixPathConstructionWire {
    /// Native helix angle interval.
    pub(super) angle_range: [f64; 2],
    /// Helix center in model space.
    pub(super) center: Point3,
    /// Major radial axis vector.
    pub(super) major: Vector3,
    /// Minor radial axis vector.
    pub(super) minor: Vector3,
    /// Axial advance of one turn, as a vector.
    pub(super) pitch: Vector3,
    /// Native apex taper factor.
    pub(super) apex_factor: f64,
    /// Helix axis direction.
    pub(super) axis: Vector3,
}

/// The positioned frame a helix construction states.
///
/// The two helix constructions state the same five values, so they take them
/// as one argument and each admits them against its own contract.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct HelixFrame<Axis = Vector3> {
    /// Helix centre in model space.
    pub center: Point3,
    /// Major radial vector.
    pub major: Vector3,
    /// Minor radial vector.
    pub minor: Vector3,
    /// Pitch vector along the axis.
    pub pitch: Vector3,
    /// Helix axis direction.
    pub axis: Axis,
}

impl HelixPathConstruction {
    /// Admit parameters that satisfy the helix payload contract.
    pub fn try_new(
        angle_range: [f64; 2],
        frame: HelixFrame,
        apex_factor: f64,
    ) -> Result<Self, &'static str> {
        Self::try_from_axis(angle_range, frame, apex_factor, FiniteVector3::new)
    }

    /// Build a helix path from an already admitted unit axis.
    pub fn try_with_unit_axis(
        angle_range: [f64; 2],
        frame: HelixFrame<crate::units::UnitVector3>,
        apex_factor: f64,
    ) -> Result<Self, &'static str> {
        Self::try_from_axis(angle_range, frame, apex_factor, |axis| Some(axis.into()))
    }

    pub(super) fn try_from_axis<Axis>(
        angle_range: [f64; 2],
        frame: HelixFrame<Axis>,
        apex_factor: f64,
        admit_axis: impl FnOnce(Axis) -> Option<FiniteVector3>,
    ) -> Result<Self, &'static str> {
        let HelixFrame {
            center,
            major,
            minor,
            pitch,
            axis,
        } = frame;
        let major_length = major.norm();
        let minor_length = minor.norm();
        if !(major_length.is_finite()
            && minor_length.is_finite()
            && major_length > 0.0
            && minor_length > 0.0
            && (major_length - minor_length).abs()
                <= EPS_HELIX_SURFACE_RADIUS_RELATIVE * major_length.max(minor_length))
        {
            return Err("helix surface path major and minor must define a circular path");
        }

        let angle_range = FiniteVector::new(angle_range)
            .ok_or("HelixPathConstruction.angle_range must be finite")?;
        let center =
            FinitePoint3::new(center).ok_or("HelixPathConstruction.center must be finite")?;
        let major =
            FiniteVector3::new(major).ok_or("HelixPathConstruction.major must be finite")?;
        let minor =
            FiniteVector3::new(minor).ok_or("HelixPathConstruction.minor must be finite")?;
        let pitch =
            FiniteVector3::new(pitch).ok_or("HelixPathConstruction.pitch must be finite")?;
        let apex_factor = FiniteReal::new(apex_factor)
            .ok_or("HelixPathConstruction.apex_factor must be finite")?;
        let axis = admit_axis(axis).ok_or("HelixPathConstruction.axis must be finite")?;
        Ok(Self {
            angle_range,
            center,
            major,
            minor,
            pitch,
            apex_factor,
            axis,
        })
    }
    /// Return the angle range.
    #[must_use]
    pub const fn angle_range(&self) -> &FiniteVector<2> {
        &self.angle_range
    }

    /// Return the center.
    #[must_use]
    pub const fn center(&self) -> &FinitePoint3 {
        &self.center
    }

    /// Return the major.
    #[must_use]
    pub const fn major(&self) -> &FiniteVector3 {
        &self.major
    }

    /// Return the minor.
    #[must_use]
    pub const fn minor(&self) -> &FiniteVector3 {
        &self.minor
    }

    /// Return the pitch.
    #[must_use]
    pub const fn pitch(&self) -> &FiniteVector3 {
        &self.pitch
    }

    /// Return the apex factor.
    #[must_use]
    pub const fn apex_factor(&self) -> FiniteReal {
        self.apex_factor
    }

    /// Return the axis.
    #[must_use]
    pub const fn axis(&self) -> &FiniteVector3 {
        &self.axis
    }
}

impl TryFrom<HelixPathConstructionWire> for HelixPathConstruction {
    type Error = &'static str;
    fn try_from(wire: HelixPathConstructionWire) -> Result<Self, Self::Error> {
        Self::try_new(
            wire.angle_range,
            HelixFrame {
                center: wire.center,
                major: wire.major,
                minor: wire.minor,
                pitch: wire.pitch,
                axis: wire.axis,
            },
            wire.apex_factor,
        )
    }
}

/// Finite ordered helix curve with a non-degenerate radial frame.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize, cadmpeg_derive::StructuralOrd)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(try_from = "HelixCurveConstructionWire")]
pub struct HelixCurveConstruction {
    pub(super) angle_range: FiniteVector<2>,
    pub(super) center: FinitePoint3,
    pub(super) major: FiniteVector3,
    pub(super) minor: FiniteVector3,
    pub(super) pitch: FiniteVector3,
    pub(super) apex_factor: FiniteReal,
    pub(super) axis: FiniteVector3,
    /// Solved-cache fit contract this construction states itself.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_cache"
    )]
    pub(super) cache: Option<LegacyCache>,
}

#[derive(Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(deny_unknown_fields)]
pub(super) struct HelixCurveConstructionWire {
    /// Native helix angle interval.
    pub(super) angle_range: [f64; 2],
    /// Helix center in model space.
    pub(super) center: Point3,
    /// Major radial axis vector.
    pub(super) major: Vector3,
    /// Minor radial axis vector.
    pub(super) minor: Vector3,
    /// Axial advance of one turn, as a vector.
    pub(super) pitch: Vector3,
    /// Native apex taper factor.
    pub(super) apex_factor: f64,
    /// Helix axis direction.
    pub(super) axis: Vector3,
    /// Solved-cache fit contract this construction states itself.
    #[serde(default, deserialize_with = "deserialize_cache")]
    pub(super) cache: Option<LegacyCache>,
}

impl HelixCurveConstruction {
    /// Admit parameters that satisfy the helix payload contract.
    pub fn try_new(
        angle_range: [f64; 2],
        frame: HelixFrame,
        apex_factor: f64,
        cache: Option<LegacyCache>,
    ) -> Result<Self, &'static str> {
        let HelixFrame {
            center,
            major,
            minor,
            pitch,
            axis,
        } = frame;
        if angle_range[0] > angle_range[1] {
            return Err("helix curve angle_range must be ordered");
        }
        let major_radius = major.norm();
        let minor_radius = minor.norm();
        if major_radius <= f64::EPSILON
            || minor_radius <= f64::EPSILON
            || axis.norm() <= f64::EPSILON
        {
            return Err("helix curve major, minor, and axis must be non-degenerate");
        }
        if major.is_finite()
            && minor.is_finite()
            && (!major_radius.is_finite()
                || !minor_radius.is_finite()
                || (major_radius - minor_radius).abs() > EPS_HELIX_CURVE_RADIUS)
        {
            return Err("helix curve major and minor radii must agree");
        }

        let angle_range = FiniteVector::new(angle_range)
            .ok_or("HelixCurveConstruction.angle_range must be finite")?;
        let center =
            FinitePoint3::new(center).ok_or("HelixCurveConstruction.center must be finite")?;
        let major =
            FiniteVector3::new(major).ok_or("HelixCurveConstruction.major must be finite")?;
        let minor =
            FiniteVector3::new(minor).ok_or("HelixCurveConstruction.minor must be finite")?;
        let pitch =
            FiniteVector3::new(pitch).ok_or("HelixCurveConstruction.pitch must be finite")?;
        let apex_factor = FiniteReal::new(apex_factor)
            .ok_or("HelixCurveConstruction.apex_factor must be finite")?;
        let axis = FiniteVector3::new(axis).ok_or("HelixCurveConstruction.axis must be finite")?;
        Ok(Self {
            angle_range,
            center,
            major,
            minor,
            pitch,
            apex_factor,
            axis,
            cache,
        })
    }
    /// Return the angle range.
    #[must_use]
    pub const fn angle_range(&self) -> &FiniteVector<2> {
        &self.angle_range
    }

    /// Borrow the admitted center.
    #[must_use]
    pub const fn center(&self) -> &FinitePoint3 {
        &self.center
    }

    /// Return the major.
    #[must_use]
    pub const fn major(&self) -> &FiniteVector3 {
        &self.major
    }

    /// Return the minor.
    #[must_use]
    pub const fn minor(&self) -> &FiniteVector3 {
        &self.minor
    }

    /// Return the pitch.
    #[must_use]
    pub const fn pitch(&self) -> &FiniteVector3 {
        &self.pitch
    }

    /// Return the apex factor.
    #[must_use]
    pub const fn apex_factor(&self) -> FiniteReal {
        self.apex_factor
    }

    /// Return the axis.
    #[must_use]
    pub const fn axis(&self) -> &FiniteVector3 {
        &self.axis
    }
}

impl TryFrom<HelixCurveConstructionWire> for HelixCurveConstruction {
    type Error = &'static str;
    fn try_from(wire: HelixCurveConstructionWire) -> Result<Self, Self::Error> {
        Self::try_new(
            wire.angle_range,
            HelixFrame {
                center: wire.center,
                major: wire.major,
                minor: wire.minor,
                pitch: wire.pitch,
                axis: wire.axis,
            },
            wire.apex_factor,
            wire.cache,
        )
    }
}

impl HelixCurveConstruction {
    /// Reverse the parameter while preserving points and derivatives.
    pub fn try_reverse_parameterization(&mut self) -> Result<(), &'static str> {
        let candidate = Self::try_new(
            self.angle_range.reversed_negated().get(),
            HelixFrame {
                center: self.center.get(),
                major: self.major.get(),
                minor: self.minor.negated().get(),
                pitch: self.pitch.negated().get(),
                axis: self.axis.get(),
            },
            -self.apex_factor.get(),
            self.cache,
        )?;
        *self = candidate;
        Ok(())
    }

    /// Scale lengths atomically and retain the old path when admission fails.
    pub fn try_scale_lengths(&mut self, scale: f64) -> Result<(), &'static str> {
        let vector =
            |value: Vector3| Vector3::new(value.x * scale, value.y * scale, value.z * scale);
        let center = Point3::new(
            self.center.x * scale,
            self.center.y * scale,
            self.center.z * scale,
        );
        let major = vector(self.major.get());
        let minor = vector(self.minor.get());
        let pitch = vector(self.pitch.get());
        let major_radius = major.norm();
        let minor_radius = minor.norm();
        if major_radius <= f64::EPSILON || minor_radius <= f64::EPSILON {
            return Err("helix curve major, minor, and axis must be non-degenerate");
        }
        if major.is_finite()
            && minor.is_finite()
            && (!major_radius.is_finite()
                || !minor_radius.is_finite()
                || (major_radius - minor_radius).abs() > EPS_HELIX_CURVE_RADIUS)
        {
            return Err("helix curve major and minor radii must agree");
        }
        let center =
            FinitePoint3::new(center).ok_or("HelixCurveConstruction.center must be finite")?;
        let major =
            FiniteVector3::new(major).ok_or("HelixCurveConstruction.major must be finite")?;
        let minor =
            FiniteVector3::new(minor).ok_or("HelixCurveConstruction.minor must be finite")?;
        let pitch =
            FiniteVector3::new(pitch).ok_or("HelixCurveConstruction.pitch must be finite")?;
        self.center = center;
        self.major = major;
        self.minor = minor;
        self.pitch = pitch;
        Ok(())
    }
}

/// Finite circular helix profile with a nonzero signed radius.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize, cadmpeg_derive::StructuralOrd)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(try_from = "HelixCircleProfileWire")]
pub struct HelixCircleProfile {
    pub(super) angle: FiniteReal,
    pub(super) radius: FiniteReal,
}

#[derive(Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(deny_unknown_fields)]
pub(super) struct HelixCircleProfileWire {
    /// Native angle stored before the path, in radians.
    pub(super) angle: f64,
    /// Signed circular profile radius.
    pub(super) radius: f64,
}

impl HelixCircleProfile {
    /// Admit parameters that satisfy the helix payload contract.
    pub fn try_new(angle: f64, radius: f64) -> Result<Self, &'static str> {
        if radius == 0.0 {
            return Err("helix circle profile radius must be finite and nonzero");
        }
        let angle = FiniteReal::new(angle).ok_or("helix circle profile angle must be finite")?;
        let radius = FiniteReal::new(radius).ok_or("helix circle profile radius must be finite")?;
        Ok(Self { angle, radius })
    }
    /// Native angle stored before the path, in radians.
    #[must_use]
    pub const fn angle(&self) -> FiniteReal {
        self.angle
    }

    /// Signed circular profile radius.
    #[must_use]
    pub const fn radius(&self) -> FiniteReal {
        self.radius
    }
}

impl TryFrom<HelixCircleProfileWire> for HelixCircleProfile {
    type Error = &'static str;
    fn try_from(wire: HelixCircleProfileWire) -> Result<Self, Self::Error> {
        Self::try_new(wire.angle, wire.radius)
    }
}

/// Finite non-degenerate linear helix profile.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize, cadmpeg_derive::StructuralOrd)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(try_from = "HelixLineProfileWire")]
pub struct HelixLineProfile {
    pub(super) direction: FiniteVector3,
}

#[derive(Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(deny_unknown_fields)]
pub(super) struct HelixLineProfileWire {
    /// Profile direction coefficients: `x` and `y` scale the path's rotated
    /// major and minor vectors, and `z` is a length along the path axis.
    pub(super) direction: Vector3,
}

impl HelixLineProfile {
    /// Admit parameters that satisfy the helix payload contract.
    pub fn try_new(direction: Vector3) -> Result<Self, &'static str> {
        const INVALID: &str = "helix line profile direction must be finite and non-degenerate";
        let direction = FiniteVector3::new(direction).ok_or(INVALID)?;
        if direction.x == 0.0 && direction.y == 0.0 && direction.z == 0.0 {
            return Err(INVALID);
        }
        Ok(Self { direction })
    }
    /// Profile direction coefficients: `x` and `y` scale the path's rotated
    /// major and minor vectors, and `z` is a length along the path axis.
    #[must_use]
    pub const fn direction(&self) -> FiniteVector3 {
        self.direction
    }
}

impl TryFrom<HelixLineProfileWire> for HelixLineProfile {
    type Error = &'static str;
    fn try_from(wire: HelixLineProfileWire) -> Result<Self, Self::Error> {
        Self::try_new(wire.direction)
    }
}

/// Profile-specific tail of a helix surface.
#[derive(
    Debug,
    Clone,
    Copy,
    PartialEq,
    Serialize,
    cadmpeg_derive::DeserializeTagged,
    cadmpeg_derive::StructuralOrd,
)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(tag = "kind", rename_all = "snake_case")]
#[serde(deny_unknown_fields)]
pub enum HelixSurfaceProfile {
    /// Circular profile swept along the helix.
    Circle(HelixCircleProfile),
    /// Linear profile swept along the helix.
    Line(HelixLineProfile),
}

/// Complete helix-surface construction with finite native intervals.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize, cadmpeg_derive::StructuralOrd)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(try_from = "HelixSurfaceConstructionWire")]
pub struct HelixSurfaceConstruction {
    pub(super) angle_range: FiniteVector<2>,
    pub(super) dimension_range: FiniteVector<2>,
    pub(super) path: HelixPathConstruction,
    pub(super) profile: HelixSurfaceProfile,
}

#[derive(Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(deny_unknown_fields)]
pub(super) struct HelixSurfaceConstructionWire {
    /// Native profile parameter interval.
    pub(super) angle_range: [f64; 2],
    /// Native path angle interval.
    pub(super) dimension_range: [f64; 2],
    /// Circular path of the helix surface.
    pub(super) path: HelixPathConstruction,
    /// Profile swept along the path.
    pub(super) profile: HelixSurfaceProfile,
}

impl HelixSurfaceConstruction {
    /// Admit parameters that satisfy the helix payload contract.
    pub fn try_new(
        angle_range: [f64; 2],
        dimension_range: [f64; 2],
        path: HelixPathConstruction,
        profile: HelixSurfaceProfile,
    ) -> Result<Self, &'static str> {
        let angle_range =
            FiniteVector::new(angle_range).ok_or("helix surface angle_range must be finite")?;
        let dimension_range = FiniteVector::new(dimension_range)
            .ok_or("helix surface dimension_range must be finite")?;
        Ok(Self {
            angle_range,
            dimension_range,
            path,
            profile,
        })
    }
    /// Return the native profile parameter interval.
    #[must_use]
    pub const fn angle_range(&self) -> &FiniteVector<2> {
        &self.angle_range
    }

    /// Return the native path angle interval.
    #[must_use]
    pub const fn dimension_range(&self) -> &FiniteVector<2> {
        &self.dimension_range
    }

    /// Return the path.
    #[must_use]
    pub const fn path(&self) -> &HelixPathConstruction {
        &self.path
    }

    /// Return the profile.
    #[must_use]
    pub const fn profile(&self) -> &HelixSurfaceProfile {
        &self.profile
    }
}

impl TryFrom<HelixSurfaceConstructionWire> for HelixSurfaceConstruction {
    type Error = &'static str;
    fn try_from(wire: HelixSurfaceConstructionWire) -> Result<Self, Self::Error> {
        Self::try_new(
            wire.angle_range,
            wire.dimension_range,
            wire.path,
            wire.profile,
        )
    }
}

impl HelixCurveConstruction {
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

cadmpeg_core::named_optional_field!(deserialize_cache, LegacyCache, "cache");
