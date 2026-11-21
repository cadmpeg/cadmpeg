// SPDX-License-Identifier: Apache-2.0

use super::super::INVALID_PRIMITIVE_DIMENSIONS;
use super::{PrimitiveSolid, PrimitiveSolidKind, PrimitiveSolidScaleError};
use crate::scalar::{Length, PositiveReal};

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
