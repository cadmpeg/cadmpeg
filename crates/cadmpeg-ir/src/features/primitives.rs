// SPDX-License-Identifier: Apache-2.0
//! Primitive solids.

use crate::scalar::{Angle, Length, PositiveReal};
#[cfg(feature = "schema")]
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

/// The refusal of primitive dimensions that define no solid.
const INVALID_PRIMITIVE_DIMENSIONS: &str = "primitive dimensions are invalid";

/// An analytic solid primitive with valid dimensions.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(try_from = "PrimitiveSolidKind", into = "PrimitiveSolidKind")]
pub struct PrimitiveSolid(pub(super) PrimitiveSolidKind);

/// Which condition refuses a scaled primitive.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PrimitiveSolidScaleError {
    /// A scaled dimension overflows.
    NonFinite,
    /// A positive dimension or strict extent collapses under rounding.
    Admission(&'static str),
}

impl PrimitiveSolid {
    /// Admits dimensions that define a solid primitive.
    pub fn new(kind: PrimitiveSolidKind) -> Result<Self, &'static str> {
        let positive = |value: Length| value.get() > 0.0;
        let valid = match &kind {
            PrimitiveSolidKind::Box {
                length,
                width,
                height,
            } => positive(*length) && positive(*width) && positive(*height),
            PrimitiveSolidKind::Cylinder { radius, height, .. } => {
                positive(*radius) && positive(*height)
            }
            PrimitiveSolidKind::Cone {
                radius1,
                radius2,
                height,
                ..
            } => {
                radius1.get() >= 0.0
                    && radius2.get() >= 0.0
                    && (positive(*radius1) || positive(*radius2))
                    && positive(*height)
            }
            PrimitiveSolidKind::Sphere {
                radius,
                latitude1,
                latitude2,
                ..
            } => positive(*radius) && latitude1.get() < latitude2.get(),
            PrimitiveSolidKind::Ellipsoid {
                x_radius,
                y_radius,
                z_radius,
                latitude1,
                latitude2,
                ..
            } => {
                positive(*x_radius)
                    && positive(*y_radius)
                    && positive(*z_radius)
                    && latitude1.get() < latitude2.get()
            }
            PrimitiveSolidKind::Torus {
                major_radius,
                minor_radius,
                latitude1,
                latitude2,
                ..
            } => {
                positive(*major_radius)
                    && positive(*minor_radius)
                    && latitude1.get() < latitude2.get()
            }
            PrimitiveSolidKind::Prism {
                sides,
                circumradius,
                height,
            } => *sides >= 3 && positive(*circumradius) && positive(*height),
            PrimitiveSolidKind::Wedge {
                xmin,
                ymin,
                zmin,
                x2min,
                z2min,
                xmax,
                ymax,
                zmax,
                x2max,
                z2max,
            } => {
                xmax.get() > xmin.get()
                    && ymax.get() > ymin.get()
                    && zmax.get() > zmin.get()
                    && x2max.get() >= x2min.get()
                    && z2max.get() >= z2min.get()
            }
        };
        valid
            .then_some(Self(kind))
            .ok_or(INVALID_PRIMITIVE_DIMENSIONS)
    }

    /// The primitive with every length times `scale`, keeping its angles and
    /// side count.
    ///
    /// A non-finite result is distinguished from a collapsed dimension. Rounding is monotone, so a
    /// positive scale keeps every sign and every non-strict order of the
    /// lengths, and the angles and the side count are kept. The scaled
    /// dimensions are tested only for what scaling can break: a positive
    /// length that rounds to zero, and a wedge extent whose strictly ordered
    /// bounds round to one value. That refusal is the admission's own text.
    pub fn scaled(&self, scale: PositiveReal) -> Result<Self, PrimitiveSolidScaleError> {
        let scaled = |value: Length| {
            Length::new(value.get() * scale.get()).ok_or(PrimitiveSolidScaleError::NonFinite)
        };
        let positive = |value: Length| value.get() > 0.0;
        let (kind, valid) = match &self.0 {
            PrimitiveSolidKind::Box {
                length,
                width,
                height,
            } => {
                let [length, width, height] = [scaled(*length)?, scaled(*width)?, scaled(*height)?];
                (
                    PrimitiveSolidKind::Box {
                        length,
                        width,
                        height,
                    },
                    positive(length) && positive(width) && positive(height),
                )
            }
            PrimitiveSolidKind::Cylinder {
                radius,
                height,
                angle,
            } => {
                let [radius, height] = [scaled(*radius)?, scaled(*height)?];
                (
                    PrimitiveSolidKind::Cylinder {
                        radius,
                        height,
                        angle: *angle,
                    },
                    positive(radius) && positive(height),
                )
            }
            PrimitiveSolidKind::Cone {
                radius1,
                radius2,
                height,
                angle,
            } => {
                let [radius1, radius2, height] =
                    [scaled(*radius1)?, scaled(*radius2)?, scaled(*height)?];
                (
                    PrimitiveSolidKind::Cone {
                        radius1,
                        radius2,
                        height,
                        angle: *angle,
                    },
                    (positive(radius1) || positive(radius2)) && positive(height),
                )
            }
            PrimitiveSolidKind::Sphere {
                radius,
                latitude1,
                latitude2,
                longitude,
            } => {
                let radius = scaled(*radius)?;
                (
                    PrimitiveSolidKind::Sphere {
                        radius,
                        latitude1: *latitude1,
                        latitude2: *latitude2,
                        longitude: *longitude,
                    },
                    positive(radius),
                )
            }
            PrimitiveSolidKind::Ellipsoid {
                x_radius,
                y_radius,
                z_radius,
                latitude1,
                latitude2,
                longitude,
            } => {
                let [x_radius, y_radius, z_radius] =
                    [scaled(*x_radius)?, scaled(*y_radius)?, scaled(*z_radius)?];
                (
                    PrimitiveSolidKind::Ellipsoid {
                        x_radius,
                        y_radius,
                        z_radius,
                        latitude1: *latitude1,
                        latitude2: *latitude2,
                        longitude: *longitude,
                    },
                    positive(x_radius) && positive(y_radius) && positive(z_radius),
                )
            }
            PrimitiveSolidKind::Torus {
                major_radius,
                minor_radius,
                latitude1,
                latitude2,
                longitude,
            } => {
                let [major_radius, minor_radius] = [scaled(*major_radius)?, scaled(*minor_radius)?];
                (
                    PrimitiveSolidKind::Torus {
                        major_radius,
                        minor_radius,
                        latitude1: *latitude1,
                        latitude2: *latitude2,
                        longitude: *longitude,
                    },
                    positive(major_radius) && positive(minor_radius),
                )
            }
            PrimitiveSolidKind::Prism {
                sides,
                circumradius,
                height,
            } => {
                let [circumradius, height] = [scaled(*circumradius)?, scaled(*height)?];
                (
                    PrimitiveSolidKind::Prism {
                        sides: *sides,
                        circumradius,
                        height,
                    },
                    positive(circumradius) && positive(height),
                )
            }
            PrimitiveSolidKind::Wedge {
                xmin,
                ymin,
                zmin,
                x2min,
                z2min,
                xmax,
                ymax,
                zmax,
                x2max,
                z2max,
            } => {
                let [xmin, ymin, zmin, x2min, z2min, xmax, ymax, zmax, x2max, z2max] = [
                    scaled(*xmin)?,
                    scaled(*ymin)?,
                    scaled(*zmin)?,
                    scaled(*x2min)?,
                    scaled(*z2min)?,
                    scaled(*xmax)?,
                    scaled(*ymax)?,
                    scaled(*zmax)?,
                    scaled(*x2max)?,
                    scaled(*z2max)?,
                ];
                (
                    PrimitiveSolidKind::Wedge {
                        xmin,
                        ymin,
                        zmin,
                        x2min,
                        z2min,
                        xmax,
                        ymax,
                        zmax,
                        x2max,
                        z2max,
                    },
                    xmax.get() > xmin.get() && ymax.get() > ymin.get() && zmax.get() > zmin.get(),
                )
            }
        };
        valid
            .then_some(Self(kind))
            .ok_or(PrimitiveSolidScaleError::Admission(
                INVALID_PRIMITIVE_DIMENSIONS,
            ))
    }

    /// Returns the primitive form and dimensions.
    pub fn kind(&self) -> &PrimitiveSolidKind {
        &self.0
    }
}

impl TryFrom<PrimitiveSolidKind> for PrimitiveSolid {
    type Error = &'static str;

    fn try_from(kind: PrimitiveSolidKind) -> Result<Self, Self::Error> {
        Self::new(kind)
    }
}

impl From<PrimitiveSolid> for PrimitiveSolidKind {
    fn from(solid: PrimitiveSolid) -> Self {
        solid.0
    }
}

/// Canonical dimensions of an analytic solid primitive.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum PrimitiveSolidKind {
    /// Rectangular solid aligned to its feature frame.
    Box {
        /// Size along the local x-axis.
        length: Length,
        /// Size along the local y-axis.
        width: Length,
        /// Size along the local z-axis.
        height: Length,
    },
    /// Circular cylinder aligned to its feature-frame z-axis.
    Cylinder {
        /// Circular base radius.
        radius: Length,
        /// Axial height.
        height: Length,
        /// Angular sweep around the axis.
        angle: Angle,
    },
    /// Circular cone or frustum aligned to its feature-frame z-axis.
    Cone {
        /// Radius at the local-frame origin.
        radius1: Length,
        /// Radius at the opposite end.
        radius2: Length,
        /// Axial height.
        height: Length,
        /// Angular sweep around the axis.
        angle: Angle,
    },
    /// Spherical segment.
    Sphere {
        /// Sphere radius.
        radius: Length,
        /// Lower latitude bound.
        latitude1: Angle,
        /// Upper latitude bound.
        latitude2: Angle,
        /// Longitudinal sweep.
        longitude: Angle,
    },
    /// Ellipsoidal segment aligned to its feature frame.
    Ellipsoid {
        /// Radius along local x.
        x_radius: Length,
        /// Radius along local y.
        y_radius: Length,
        /// Radius along local z.
        z_radius: Length,
        /// Lower latitude bound.
        latitude1: Angle,
        /// Upper latitude bound.
        latitude2: Angle,
        /// Longitudinal sweep.
        longitude: Angle,
    },
    /// Toroidal segment aligned to its feature frame.
    Torus {
        /// Distance from the axis to the tube center.
        major_radius: Length,
        /// Tube radius.
        minor_radius: Length,
        /// Lower tube-angle bound.
        latitude1: Angle,
        /// Upper tube-angle bound.
        latitude2: Angle,
        /// Sweep around the torus axis.
        longitude: Angle,
    },
    /// Regular polygonal prism aligned to its feature frame.
    Prism {
        /// Number of polygon sides.
        sides: u32,
        /// Distance from polygon center to each vertex.
        circumradius: Length,
        /// Axial height.
        height: Length,
    },
    /// General wedge defined by two x-z profiles across a y interval.
    Wedge {
        /// Lower x bound.
        xmin: Length,
        /// Lower y bound.
        ymin: Length,
        /// Lower z bound.
        zmin: Length,
        /// Inner x coordinate on the lower-y profile.
        x2min: Length,
        /// Inner z coordinate on the lower-y profile.
        z2min: Length,
        /// Upper x bound.
        xmax: Length,
        /// Upper y bound.
        ymax: Length,
        /// Upper z bound.
        zmax: Length,
        /// Inner x coordinate on the upper-y profile.
        x2max: Length,
        /// Inner z coordinate on the upper-y profile.
        z2max: Length,
    },
}
