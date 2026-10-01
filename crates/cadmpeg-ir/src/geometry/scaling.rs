// SPDX-License-Identifier: Apache-2.0
//! Unit scaling of solved carriers.
//!
//! A positive scale multiplies every length of a carrier and keeps its
//! directions, angles, ratios and parameterization. A scaled value is tested
//! only for what scaling can break: a coordinate or length that overflows, and
//! a positive or nonzero length that rounds to zero. Rounding is monotone, so
//! signs and non-strict orders are kept, and the inline basis chain of a
//! placement is kept, so none of them is tested again.

use super::analytic::{
    CircleCurve, ConeSurface, CylinderSurface, DegenerateCurve, HyperbolaCurve, LineCurve,
    ParabolaCurve, PlaneSurface, SphereSurface, TorusSurface,
};
use super::nurbs::NurbsError;
use super::sampled::GeometryLayoutError;
use super::{SolvedCurveGeometry, SolvedSurfaceGeometry};
use crate::features::FinitePoint3;
use crate::scalar::{NonZeroLength, PositiveLength, PositiveReal};

/// A unit scaling that a solved carrier refuses.
#[derive(Debug, Clone, PartialEq)]
pub enum ScaleRefusal {
    /// Storage or work exceeded the operation limit.
    Resource(cadmpeg_core::decode::ResourceLimit),
    /// A scaled analytic field is refused; the text names the field and its
    /// condition.
    Field(&'static str),
    /// A scaled NURBS control point is refused.
    ControlPoints(NurbsError),
    /// A scaled vertex, sample point or chordal deviation of a sampled
    /// carrier is refused.
    Samples(GeometryLayoutError),
    /// A scaled placement translation is not finite.
    Translation,
}

enum ScalingError {
    Geometry(ScaleRefusal),
    Resource(cadmpeg_core::CodecError),
}
impl From<ScaleRefusal> for ScalingError {
    fn from(error: ScaleRefusal) -> Self { Self::Geometry(error) }
}
impl From<cadmpeg_core::CodecError> for ScalingError {
    fn from(error: cadmpeg_core::CodecError) -> Self { Self::Resource(error) }
}
impl ScaleRefusal {
    fn from_codec(error: cadmpeg_core::CodecError) -> Self {
        match error {
            cadmpeg_core::CodecError::ResourceLimit(limit) => Self::Resource(limit),
            error => Self::ControlPoints(NurbsError::Structure(error.to_string())),
        }
    }
}

/// `point` times `scale`, refused with `field` when a coordinate overflows.
fn scaled_point(
    point: FinitePoint3,
    scale: PositiveReal,
    field: &'static str,
) -> Result<FinitePoint3, ScaleRefusal> {
    point.scaled(scale).ok_or(ScaleRefusal::Field(field))
}

/// `length` times `scale`, refused with `field` when the product overflows or
/// rounds to zero.
fn scaled_positive(
    length: PositiveLength,
    scale: PositiveReal,
    field: &'static str,
) -> Result<PositiveLength, ScaleRefusal> {
    PositiveLength::new(length.get() * scale.get()).ok_or(ScaleRefusal::Field(field))
}

/// `length` times `scale`, refused with `field` when the product overflows or
/// rounds to zero.
fn scaled_nonzero(
    length: NonZeroLength,
    scale: PositiveReal,
    field: &'static str,
) -> Result<NonZeroLength, ScaleRefusal> {
    NonZeroLength::new(length.get() * scale.get()).ok_or(ScaleRefusal::Field(field))
}

impl SolvedSurfaceGeometry {
    /// The carrier with every length times `scale`.
    ///
    /// An origin or center is refused before a radius, and a major radius
    /// before a minor one. A placement scales its basis before its
    /// translation and keeps its nesting depth, because the scaled basis has
    /// the variant chain of the basis.
    pub fn scaled(&self, scale: PositiveReal) -> Result<Self, ScaleRefusal> {
        let arena = cadmpeg_core::decode::DecodeArena::new();
        let policy = cadmpeg_core::decode::DecodePolicy::default();
        let (ctx, _) = cadmpeg_core::decode::DecodeContext::from_root_bytes(&[], &arena, &policy).map_err(ScaleRefusal::from_codec)?;
        Self::scaled_for_decode(self, &ctx, scale).map_err(ScaleRefusal::from_codec)?
    }

    /// Copy and scale a borrowed carrier through the caller's context.
    pub fn scaled_for_decode(&self, ctx: &cadmpeg_core::decode::DecodeContext<'_>, scale: PositiveReal) -> Result<Result<Self, ScaleRefusal>, cadmpeg_core::CodecError> {
        self.try_clone_for_decode(ctx, "IR scaled carrier copy")?.scaled_owned(ctx, scale)
    }
}

impl SolvedCurveGeometry {
    /// The carrier with every length times `scale`.
    ///
    /// A center, origin or vertex is refused before a radius or focal
    /// distance, and a major radius before a minor one. A placement scales
    /// its basis before its translation and keeps its nesting depth, because
    /// the scaled basis has the variant chain of the basis. A composite curve
    /// holds references to other curves and no length of its own.
    pub fn scaled(&self, scale: PositiveReal) -> Result<Self, ScaleRefusal> {
        let arena = cadmpeg_core::decode::DecodeArena::new();
        let policy = cadmpeg_core::decode::DecodePolicy::default();
        let (ctx, _) = cadmpeg_core::decode::DecodeContext::from_root_bytes(&[], &arena, &policy).map_err(ScaleRefusal::from_codec)?;
        Self::scaled_for_decode(self, &ctx, scale).map_err(ScaleRefusal::from_codec)?
    }

    /// Copy and scale a borrowed carrier through the caller's context.
    pub fn scaled_for_decode(&self, ctx: &cadmpeg_core::decode::DecodeContext<'_>, scale: PositiveReal) -> Result<Result<Self, ScaleRefusal>, cadmpeg_core::CodecError> {
        self.try_clone_for_decode(ctx, "IR scaled carrier copy")?.scaled_owned(ctx, scale)
    }
}

impl SolvedCurveGeometry {
    /// Scale an owned carrier without copying its retained text, rows or placement boxes.
    /// Resource refusal is separate from the geometric refusal. Refused candidates are consumed.
    pub fn scaled_owned(
        mut self,
        ctx: &cadmpeg_core::decode::DecodeContext<'_>,
        scale: PositiveReal,
    ) -> Result<Result<Self, ScaleRefusal>, cadmpeg_core::CodecError> {
        match self.scale_in_place(ctx, scale) {
            Ok(()) => Ok(Ok(self)),
            Err(ScalingError::Geometry(error)) => Ok(Err(error)),
            Err(ScalingError::Resource(error)) => Err(error),
        }
    }

    fn scale_in_place(&mut self, ctx: &cadmpeg_core::decode::DecodeContext<'_>, scale: PositiveReal) -> Result<(), ScalingError> {
        ctx.charge_work(1, "IR geometry unit scaling work")?;
        let scaled = match self {
            Self::Line(line) => Self::Line(LineCurve::new(
                scaled_point(
                    line.origin(),
                    scale,
                    "scaled LineCurve.origin must be finite",
                )?,
                line.direction(),
            )),
            Self::Circle(circle) => {
                let center =
                    scaled_point(circle.center(), scale, "CircleCurve.center must be finite")?;
                let radius = scaled_positive(
                    circle.radius(),
                    scale,
                    "CircleCurve.radius must be positive and finite",
                )?;
                Self::Circle(CircleCurve::new(center, *circle.frame(), radius))
            }
            Self::Ellipse(ellipse) => {
                Self::Ellipse(ellipse.scaled(scale).map_err(ScaleRefusal::Field)?)
            }
            Self::Parabola(parabola) => {
                let vertex = scaled_point(
                    parabola.vertex(),
                    scale,
                    "ParabolaCurve.vertex must be finite",
                )?;
                let focal_distance = scaled_positive(
                    parabola.focal_distance(),
                    scale,
                    "ParabolaCurve.focal_distance must be positive and finite",
                )?;
                Self::Parabola(ParabolaCurve::new(
                    vertex,
                    *parabola.frame(),
                    focal_distance,
                ))
            }
            Self::Hyperbola(hyperbola) => {
                let center = scaled_point(
                    hyperbola.center(),
                    scale,
                    "HyperbolaCurve.center must be finite",
                )?;
                let major_radius = scaled_positive(
                    hyperbola.major_radius(),
                    scale,
                    "HyperbolaCurve.major_radius must be positive and finite",
                )?;
                let minor_radius = scaled_positive(
                    hyperbola.minor_radius(),
                    scale,
                    "HyperbolaCurve.minor_radius must be positive and finite",
                )?;
                Self::Hyperbola(HyperbolaCurve::new(
                    center,
                    *hyperbola.frame(),
                    major_radius,
                    minor_radius,
                ))
            }
            Self::Degenerate(degenerate) => Self::Degenerate(DegenerateCurve::new(scaled_point(
                degenerate.point(),
                scale,
                "DegenerateCurve.point must be finite",
            )?)),

            Self::Nurbs(value) => {
                value.scale_points(ctx, scale)?.map_err(ScaleRefusal::ControlPoints)?;
                return Ok(());
            }
            Self::Polyline(value) => {
                value.scale_points(ctx, scale)?.map_err(ScaleRefusal::Samples)?;
                return Ok(());
            }
            Self::Transformed(placed) => {
                let _depth = ctx.enter_nested("IR geometry unit scaling nesting")?;
                placed.basis.scale_in_place(ctx, scale)?;
                placed.transform = placed.transform.scaled_translation(scale).ok_or(ScaleRefusal::Translation)?;
                return Ok(());
            }
            Self::Composite { .. } | Self::Unknown { .. } => return Ok(()),
        };
        *self = scaled;
        Ok(())
    }
}

impl SolvedSurfaceGeometry {
    /// Scale an owned carrier without copying its retained text, rows or placement boxes.
    /// Resource refusal is separate from the geometric refusal. Refused candidates are consumed.
    pub fn scaled_owned(
        mut self,
        ctx: &cadmpeg_core::decode::DecodeContext<'_>,
        scale: PositiveReal,
    ) -> Result<Result<Self, ScaleRefusal>, cadmpeg_core::CodecError> {
        match self.scale_in_place(ctx, scale) {
            Ok(()) => Ok(Ok(self)),
            Err(ScalingError::Geometry(error)) => Ok(Err(error)),
            Err(ScalingError::Resource(error)) => Err(error),
        }
    }

    fn scale_in_place(&mut self, ctx: &cadmpeg_core::decode::DecodeContext<'_>, scale: PositiveReal) -> Result<(), ScalingError> {
        ctx.charge_work(1, "IR geometry unit scaling work")?;
        let scaled = match self {
            Self::Plane(plane) => Self::Plane(PlaneSurface::new(
                scaled_point(plane.origin(), scale, "PlaneSurface.origin must be finite")?,
                *plane.frame(),
            )),
            Self::Cylinder(cylinder) => {
                let origin = scaled_point(
                    cylinder.origin(),
                    scale,
                    "CylinderSurface.origin must be finite",
                )?;
                let radius = scaled_positive(
                    cylinder.radius(),
                    scale,
                    "CylinderSurface.radius must be positive and finite",
                )?;
                Self::Cylinder(CylinderSurface::new(origin, *cylinder.frame(), radius))
            }
            Self::Cone(cone) => {
                let origin =
                    scaled_point(cone.origin(), scale, "ConeSurface.origin must be finite")?;
                let radius = cone.radius().scaled(scale).ok_or(ScaleRefusal::Field(
                    "ConeSurface.radius must be nonnegative and finite",
                ))?;
                Self::Cone(ConeSurface::new(
                    origin,
                    *cone.frame(),
                    radius,
                    cone.ratio(),
                    cone.half_angle(),
                ))
            }
            Self::Sphere(sphere) => {
                let center = scaled_point(
                    sphere.center(),
                    scale,
                    "SphereSurface.center must be finite",
                )?;
                let radius = scaled_nonzero(
                    sphere.radius(),
                    scale,
                    "SphereSurface.radius must be finite and nonzero",
                )?;
                Self::Sphere(SphereSurface::new(center, *sphere.frame(), radius))
            }
            Self::Torus(torus) => {
                let center =
                    scaled_point(torus.center(), scale, "TorusSurface.center must be finite")?;
                let major_radius = scaled_positive(
                    torus.major_radius(),
                    scale,
                    "TorusSurface.major_radius must be positive and finite",
                )?;
                let minor_radius = scaled_nonzero(
                    torus.minor_radius(),
                    scale,
                    "TorusSurface.minor_radius must be finite and nonzero",
                )?;
                Self::Torus(TorusSurface::new(
                    center,
                    *torus.frame(),
                    major_radius,
                    minor_radius,
                ))
            }

            Self::Nurbs(value) => {
                value.scale_points(ctx, scale)?.map_err(ScaleRefusal::ControlPoints)?;
                return Ok(());
            }
            Self::Polygonal(value) => {
                value.scale_points(ctx, scale)?.map_err(ScaleRefusal::Samples)?;
                return Ok(());
            }
            Self::Transformed(placed) => {
                let _depth = ctx.enter_nested("IR geometry unit scaling nesting")?;
                placed.basis.scale_in_place(ctx, scale)?;
                placed.transform = placed.transform.scaled_translation(scale).ok_or(ScaleRefusal::Translation)?;
                return Ok(());
            }
            Self::Unknown { .. } => return Ok(()),
        };
        *self = scaled;
        Ok(())
    }
}

#[cfg(test)]
mod tests;
