// SPDX-License-Identifier: Apache-2.0
//! Copies of admitted geometry preserve their admitted invariants.

use super::{
    CompositeCurveSegment, CompositeCurveSegments, CurveGeometry, PlacedCurve, PlacedSurface,
    SolvedCurveGeometry, SolvedSurfaceGeometry, SurfaceGeometry,
};
use cadmpeg_core::{decode::DecodeContext, CodecError};

pub(super) fn lane<T: Copy>(ctx: &DecodeContext<'_>, source: &[T]) -> Result<Vec<T>, CodecError> {
    let mut values = Vec::new();
    ctx.reserve_working(&mut values, source.len(), "copy admitted geometry lane")?;
    values.extend_from_slice(source);
    Ok(values)
}

impl CurveGeometry {
    /// Copy this admitted carrier through fallible caller working storage.
    pub fn try_clone_working(&self, ctx: &DecodeContext<'_>) -> Result<Self, CodecError> {
        Ok(match self {
            Self::Solved(value) => Self::Solved(value.try_clone_working(ctx)?),
            Self::Procedural {
                construction,
                cache,
            } => Self::Procedural {
                construction: construction.try_clone_working(ctx)?,
                cache: cache
                    .as_ref()
                    .map(|value| value.try_clone_working(ctx))
                    .transpose()?,
            },
        })
    }
}

impl SurfaceGeometry {
    /// Copy this admitted carrier through fallible caller working storage.
    pub fn try_clone_working(&self, ctx: &DecodeContext<'_>) -> Result<Self, CodecError> {
        Ok(match self {
            Self::Solved(value) => Self::Solved(value.try_clone_working(ctx)?),
            Self::Procedural {
                construction,
                cache,
            } => Self::Procedural {
                construction: construction.try_clone_working(ctx)?,
                cache: cache
                    .as_ref()
                    .map(|value| value.try_clone_working(ctx))
                    .transpose()?,
            },
        })
    }
}

impl SolvedCurveGeometry {
    /// Copy this admitted geometry without repeating an invariant check.
    pub fn try_clone_working(&self, ctx: &DecodeContext<'_>) -> Result<Self, CodecError> {
        if let Some(refusal) = ctx.resource_refusal() {
            return Err(refusal.into());
        }
        Ok(match self {
            Self::Nurbs(value) => Self::Nurbs(value.try_clone_working(ctx)?),
            Self::Polyline(value) => Self::Polyline(value.try_clone_working(ctx)?),
            Self::Composite {
                segments,
                self_intersect,
            } => {
                let mut values = Vec::new();
                ctx.reserve_working(
                    &mut values,
                    segments.len(),
                    "copy admitted composite segments",
                )?;
                for segment in segments {
                    values.push(CompositeCurveSegment {
                        curve: segment.curve.try_clone_working(ctx)?,
                        same_sense: segment.same_sense,
                        transition: segment.transition,
                    });
                }
                Self::Composite {
                    segments: CompositeCurveSegments(values),
                    self_intersect: *self_intersect,
                }
            }
            Self::Transformed(value) => Self::Transformed(value.try_clone_working(ctx)?),
            Self::Unknown { record } => Self::Unknown {
                record: record
                    .as_ref()
                    .map(|id| id.try_clone_working(ctx))
                    .transpose()?,
            },
            Self::Line(value) => Self::Line(*value),
            Self::Circle(value) => Self::Circle(*value),
            Self::Ellipse(value) => Self::Ellipse(*value),
            Self::Parabola(value) => Self::Parabola(*value),
            Self::Hyperbola(value) => Self::Hyperbola(*value),
            Self::Degenerate(value) => Self::Degenerate(*value),
        })
    }
}

impl SolvedSurfaceGeometry {
    /// Copy this admitted geometry without repeating an invariant check.
    pub fn try_clone_working(&self, ctx: &DecodeContext<'_>) -> Result<Self, CodecError> {
        if let Some(refusal) = ctx.resource_refusal() {
            return Err(refusal.into());
        }
        Ok(match self {
            Self::Nurbs(value) => Self::Nurbs(value.try_clone_working(ctx)?),
            Self::Polygonal(value) => Self::Polygonal(value.try_clone_working(ctx)?),
            Self::Transformed(value) => Self::Transformed(value.try_clone_working(ctx)?),
            Self::Unknown { record } => Self::Unknown {
                record: record
                    .as_ref()
                    .map(|id| id.try_clone_working(ctx))
                    .transpose()?,
            },
            Self::Plane(value) => Self::Plane(*value),
            Self::Cylinder(value) => Self::Cylinder(*value),
            Self::Cone(value) => Self::Cone(*value),
            Self::Sphere(value) => Self::Sphere(*value),
            Self::Torus(value) => Self::Torus(*value),
        })
    }
}

impl PlacedCurve {
    /// Copy the admitted basis, placement and recorded depth unchanged.
    pub fn try_clone_working(&self, ctx: &DecodeContext<'_>) -> Result<Self, CodecError> {
        Ok(Self {
            basis: ctx.alloc_box_working(
                self.basis.try_clone_working(ctx)?,
                "copy admitted curve placement basis",
            )?,
            transform: self.transform,
            depth: self.depth,
        })
    }
}

impl PlacedSurface {
    /// Copy the admitted basis, placement and recorded depth unchanged.
    pub fn try_clone_working(&self, ctx: &DecodeContext<'_>) -> Result<Self, CodecError> {
        Ok(Self {
            basis: ctx.alloc_box_working(
                self.basis.try_clone_working(ctx)?,
                "copy admitted surface placement basis",
            )?,
            transform: self.transform,
            depth: self.depth,
        })
    }
}

#[cfg(test)]
mod tests;
