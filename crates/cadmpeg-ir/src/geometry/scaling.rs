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
use super::nurbs::{NurbsError, NurbsSurface};
use super::sampled::GeometryLayoutError;
use super::{
    InlineCurve, InlineSurface, PlacedCurve, PlacedSurface, SolvedCurveGeometry,
    SolvedSurfaceGeometry,
};
use crate::features::geometry::FinitePoint3;
use crate::scalar::{NonZeroLength, PositiveLength, PositiveReal};

/// A unit scaling that a solved carrier refuses.
#[derive(Debug, Clone, PartialEq)]
pub enum ScaleRefusal {
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
    fn from(error: ScaleRefusal) -> Self {
        Self::Geometry(error)
    }
}
impl From<cadmpeg_core::CodecError> for ScalingError {
    fn from(error: cadmpeg_core::CodecError) -> Self {
        Self::Resource(error)
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
    pub fn scaled(
        &self,
        ctx: &cadmpeg_core::decode::DecodeContext<'_>,
        scale: PositiveReal,
    ) -> Result<Result<Self, ScaleRefusal>, cadmpeg_core::CodecError> {
        self.clone().scaled_owned(ctx, scale)
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
    pub fn scaled(
        &self,
        ctx: &cadmpeg_core::decode::DecodeContext<'_>,
        scale: PositiveReal,
    ) -> Result<Result<Self, ScaleRefusal>, cadmpeg_core::CodecError> {
        self.clone().scaled_owned(ctx, scale)
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

    fn scale_in_place(
        &mut self,
        ctx: &cadmpeg_core::decode::DecodeContext<'_>,
        scale: PositiveReal,
    ) -> Result<(), ScalingError> {
        match self {
            Self::Line(line) => *line = scaled_line(line, scale)?,
            Self::Circle(circle) => *circle = scaled_circle(circle, scale)?,
            Self::Ellipse(ellipse) => {
                *ellipse = ellipse.scaled(scale).map_err(ScaleRefusal::Field)?;
            }
            Self::Parabola(parabola) => *parabola = scaled_parabola(parabola, scale)?,
            Self::Hyperbola(hyperbola) => *hyperbola = scaled_hyperbola(hyperbola, scale)?,
            Self::Degenerate(degenerate) => {
                *degenerate = DegenerateCurve::new(scaled_point(
                    degenerate.point(),
                    scale,
                    "DegenerateCurve.point must be finite",
                )?);
            }
            Self::Nurbs(value) => value
                .scale_points(scale)
                .map_err(ScaleRefusal::ControlPoints)?,
            Self::Polyline(value) => value.scale_points(scale).map_err(ScaleRefusal::Samples)?,
            Self::Transformed(placed) => placed.scale_in_place(ctx, scale)?,
            Self::Composite { .. } | Self::Unknown { .. } => {}
        }
        Ok(())
    }
}

impl InlineCurve {
    /// The carrier with every length times `scale`, by the rules of
    /// [`SolvedCurveGeometry::scaled`].
    pub fn scaled(
        &self,
        ctx: &cadmpeg_core::decode::DecodeContext<'_>,
        scale: PositiveReal,
    ) -> Result<Result<Self, ScaleRefusal>, cadmpeg_core::CodecError> {
        let mut scaled = self.clone();
        match scaled.scale_in_place(ctx, scale) {
            Ok(()) => Ok(Ok(scaled)),
            Err(ScalingError::Geometry(error)) => Ok(Err(error)),
            Err(ScalingError::Resource(error)) => Err(error),
        }
    }

    fn scale_in_place(
        &mut self,
        ctx: &cadmpeg_core::decode::DecodeContext<'_>,
        scale: PositiveReal,
    ) -> Result<(), ScalingError> {
        let Self::Transformed { basis, transform } = self else {
            return self.scale_leaf(scale).map_err(ScalingError::from);
        };
        let _depth = ctx.enter_nested("IR geometry unit scaling nesting")?;
        basis.scale_in_place(ctx, scale)?;
        *transform = transform
            .scaled_translation(scale)
            .ok_or(ScaleRefusal::Translation)?;
        Ok(())
    }

    fn scale_leaf(&mut self, scale: PositiveReal) -> Result<(), ScaleRefusal> {
        match self {
            Self::Line(line) => *line = scaled_line(line, scale)?,
            Self::Circle(circle) => *circle = scaled_circle(circle, scale)?,
            Self::Ellipse(ellipse) => {
                *ellipse = ellipse.scaled(scale).map_err(ScaleRefusal::Field)?;
            }
            Self::Parabola(parabola) => *parabola = scaled_parabola(parabola, scale)?,
            Self::Hyperbola(hyperbola) => *hyperbola = scaled_hyperbola(hyperbola, scale)?,
            Self::Nurbs(value) => value
                .scale_points(scale)
                .map_err(ScaleRefusal::ControlPoints)?,
            Self::Polyline(value) => value.scale_points(scale).map_err(ScaleRefusal::Samples)?,
            Self::Transformed { .. } => {}
        }
        Ok(())
    }
}

fn scaled_line(line: &LineCurve, scale: PositiveReal) -> Result<LineCurve, ScaleRefusal> {
    Ok(LineCurve::new(
        scaled_point(
            line.origin(),
            scale,
            "scaled LineCurve.origin must be finite",
        )?,
        line.direction(),
    ))
}

fn scaled_circle(circle: &CircleCurve, scale: PositiveReal) -> Result<CircleCurve, ScaleRefusal> {
    let center = scaled_point(circle.center(), scale, "CircleCurve.center must be finite")?;
    let radius = scaled_positive(
        circle.radius(),
        scale,
        "CircleCurve.radius must be positive and finite",
    )?;
    Ok(CircleCurve::new(center, *circle.frame(), radius))
}

fn scaled_parabola(
    parabola: &ParabolaCurve,
    scale: PositiveReal,
) -> Result<ParabolaCurve, ScaleRefusal> {
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
    Ok(ParabolaCurve::new(
        vertex,
        *parabola.frame(),
        focal_distance,
    ))
}

fn scaled_hyperbola(
    hyperbola: &HyperbolaCurve,
    scale: PositiveReal,
) -> Result<HyperbolaCurve, ScaleRefusal> {
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
    Ok(HyperbolaCurve::new(
        center,
        *hyperbola.frame(),
        major_radius,
        minor_radius,
    ))
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

    fn scale_in_place(
        &mut self,
        ctx: &cadmpeg_core::decode::DecodeContext<'_>,
        scale: PositiveReal,
    ) -> Result<(), ScalingError> {
        match self {
            Self::Plane(plane) => *plane = scaled_plane(plane, scale)?,
            Self::Cylinder(cylinder) => *cylinder = scaled_cylinder(cylinder, scale)?,
            Self::Cone(cone) => *cone = scaled_cone(cone, scale)?,
            Self::Sphere(sphere) => *sphere = scaled_sphere(sphere, scale)?,
            Self::Torus(torus) => *torus = scaled_torus(torus, scale)?,
            Self::Nurbs(value) => scale_nurbs_surface(value, scale)?,
            Self::Polygonal(value) => value.scale_points(scale).map_err(ScaleRefusal::Samples)?,
            Self::Transformed(placed) => placed.scale_in_place(ctx, scale)?,
            Self::Unknown { .. } => {}
        }
        Ok(())
    }
}

impl InlineSurface {
    /// The carrier with every length times `scale`, by the rules of
    /// [`SolvedSurfaceGeometry::scaled`].
    pub fn scaled(
        &self,
        ctx: &cadmpeg_core::decode::DecodeContext<'_>,
        scale: PositiveReal,
    ) -> Result<Result<Self, ScaleRefusal>, cadmpeg_core::CodecError> {
        let mut scaled = self.clone();
        match scaled.scale_in_place(ctx, scale) {
            Ok(()) => Ok(Ok(scaled)),
            Err(ScalingError::Geometry(error)) => Ok(Err(error)),
            Err(ScalingError::Resource(error)) => Err(error),
        }
    }

    fn scale_in_place(
        &mut self,
        ctx: &cadmpeg_core::decode::DecodeContext<'_>,
        scale: PositiveReal,
    ) -> Result<(), ScalingError> {
        let Self::Transformed { basis, transform } = self else {
            return self.scale_leaf(scale).map_err(ScalingError::from);
        };
        let _depth = ctx.enter_nested("IR geometry unit scaling nesting")?;
        basis.scale_in_place(ctx, scale)?;
        *transform = transform
            .scaled_translation(scale)
            .ok_or(ScaleRefusal::Translation)?;
        Ok(())
    }

    fn scale_leaf(&mut self, scale: PositiveReal) -> Result<(), ScaleRefusal> {
        match self {
            Self::Plane(plane) => *plane = scaled_plane(plane, scale)?,
            Self::Cylinder(cylinder) => *cylinder = scaled_cylinder(cylinder, scale)?,
            Self::Cone(cone) => *cone = scaled_cone(cone, scale)?,
            Self::Sphere(sphere) => *sphere = scaled_sphere(sphere, scale)?,
            Self::Torus(torus) => *torus = scaled_torus(torus, scale)?,
            Self::Nurbs(value) => scale_nurbs_surface(value, scale)?,
            Self::Polygonal(value) => value.scale_points(scale).map_err(ScaleRefusal::Samples)?,
            Self::Transformed { .. } => {}
        }
        Ok(())
    }
}

fn scaled_plane(plane: &PlaneSurface, scale: PositiveReal) -> Result<PlaneSurface, ScaleRefusal> {
    Ok(PlaneSurface::new(
        scaled_point(plane.origin(), scale, "PlaneSurface.origin must be finite")?,
        *plane.frame(),
    ))
}

fn scaled_cylinder(
    cylinder: &CylinderSurface,
    scale: PositiveReal,
) -> Result<CylinderSurface, ScaleRefusal> {
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
    Ok(CylinderSurface::new(origin, *cylinder.frame(), radius))
}

fn scaled_cone(cone: &ConeSurface, scale: PositiveReal) -> Result<ConeSurface, ScaleRefusal> {
    let origin = scaled_point(cone.origin(), scale, "ConeSurface.origin must be finite")?;
    let radius = cone.radius().scaled(scale).ok_or(ScaleRefusal::Field(
        "ConeSurface.radius must be nonnegative and finite",
    ))?;
    Ok(ConeSurface::new(
        origin,
        *cone.frame(),
        radius,
        cone.ratio(),
        cone.half_angle(),
    ))
}

fn scaled_sphere(
    sphere: &SphereSurface,
    scale: PositiveReal,
) -> Result<SphereSurface, ScaleRefusal> {
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
    Ok(SphereSurface::new(center, *sphere.frame(), radius))
}

fn scaled_torus(torus: &TorusSurface, scale: PositiveReal) -> Result<TorusSurface, ScaleRefusal> {
    let center = scaled_point(torus.center(), scale, "TorusSurface.center must be finite")?;
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
    Ok(TorusSurface::new(
        center,
        *torus.frame(),
        major_radius,
        minor_radius,
    ))
}

fn scale_nurbs_surface(value: &mut NurbsSurface, scale: PositiveReal) -> Result<(), ScaleRefusal> {
    let fit = value
        .cache_fit_tolerance()
        .map(|fit| {
            fit.scaled(scale)
                .ok_or(ScaleRefusal::Field("cache_fit_tolerance must be finite"))
        })
        .transpose()?;
    value
        .scale_points(scale)
        .map_err(ScaleRefusal::ControlPoints)?;
    value.set_cache_fit_tolerance(fit);
    Ok(())
}

// Keep the recursive placement walk separate from the carrier match. Its
// analytic construction temporaries otherwise occupy every recursive frame.
impl PlacedCurve {
    fn scale_in_place(
        &mut self,
        ctx: &cadmpeg_core::decode::DecodeContext<'_>,
        scale: PositiveReal,
    ) -> Result<(), ScalingError> {
        let _depth = ctx.enter_nested("IR geometry unit scaling nesting")?;
        if let SolvedCurveGeometry::Transformed(placed) = self.basis.as_mut() {
            placed.scale_in_place(ctx, scale)?;
        } else {
            self.basis.scale_in_place(ctx, scale)?;
        }
        self.transform = self
            .transform
            .scaled_translation(scale)
            .ok_or(ScaleRefusal::Translation)?;
        Ok(())
    }
}

impl PlacedSurface {
    fn scale_in_place(
        &mut self,
        ctx: &cadmpeg_core::decode::DecodeContext<'_>,
        scale: PositiveReal,
    ) -> Result<(), ScalingError> {
        let _depth = ctx.enter_nested("IR geometry unit scaling nesting")?;
        if let SolvedSurfaceGeometry::Transformed(placed) = self.basis.as_mut() {
            placed.scale_in_place(ctx, scale)?;
        } else {
            self.basis.scale_in_place(ctx, scale)?;
        }
        self.transform = self
            .transform
            .scaled_translation(scale)
            .ok_or(ScaleRefusal::Translation)?;
        Ok(())
    }
}

#[cfg(test)]
mod tests;
