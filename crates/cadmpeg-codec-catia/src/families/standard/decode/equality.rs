//! Borrowed geometry lane comparisons under decode admission.

use cadmpeg_core::decode::DecodeContext;
use cadmpeg_core::CodecError;
use cadmpeg_ir::geometry::nurbs::{NurbsCurve, NurbsPoleGrid, NurbsPoles3};
use cadmpeg_ir::geometry::pcurve::{PcurveGeometry, PcurveNurbsPoles, PolarNurbsPoles};
use cadmpeg_ir::geometry::{SolvedSurfaceGeometry, SurfaceGeometry};

fn lanes_equal<T: PartialEq>(
    ctx: &DecodeContext<'_>,
    left: &[T],
    right: &[T],
    operation: &'static str,
) -> Result<bool, CodecError> {
    if left.len() != right.len() {
        return Ok(false);
    }
    ctx.all_by(
        left.iter().zip(right),
        |(left, right)| Ok(left == right),
        operation,
    )
}

pub(super) fn nurbs_curves_equal(
    ctx: &DecodeContext<'_>,
    left: &NurbsCurve,
    right: &NurbsCurve,
    operation: &'static str,
) -> Result<bool, CodecError> {
    if left.degree() != right.degree()
        || left.periodic() != right.periodic()
        || !lanes_equal(
            ctx,
            left.knots().as_slice(),
            right.knots().as_slice(),
            operation,
        )?
    {
        return Ok(false);
    }
    match (left.pole_rows(), right.pole_rows()) {
        (NurbsPoles3::Polynomial { points: left }, NurbsPoles3::Polynomial { points: right }) => {
            lanes_equal(ctx, left, right, operation)
        }
        (NurbsPoles3::Rational { points: left }, NurbsPoles3::Rational { points: right }) => {
            lanes_equal(ctx, left, right, operation)
        }
        _ => Ok(false),
    }
}

fn grids_equal<T: PartialEq>(
    ctx: &DecodeContext<'_>,
    left: &[Vec<T>],
    right: &[Vec<T>],
    operation: &'static str,
) -> Result<bool, CodecError> {
    if left.len() != right.len() {
        return Ok(false);
    }
    ctx.all_by(
        left.iter().zip(right),
        |(left, right)| lanes_equal(ctx, left, right, operation),
        operation,
    )
}

pub(super) fn surfaces_equal(
    ctx: &DecodeContext<'_>,
    left: &SurfaceGeometry,
    right: &SurfaceGeometry,
    operation: &'static str,
) -> Result<bool, CodecError> {
    match (left, right) {
        (SurfaceGeometry::Solved(left), SurfaceGeometry::Solved(right)) => {
            solved_surfaces_equal(ctx, left, right, operation)
        }
        (
            SurfaceGeometry::Procedural {
                construction: left,
                cache: left_cache,
            },
            SurfaceGeometry::Procedural {
                construction: right,
                cache: right_cache,
            },
        ) => {
            if !ctx.equal(left, right, operation)? {
                return Ok(false);
            }
            match (left_cache, right_cache) {
                (Some(left), Some(right)) => solved_surfaces_equal(ctx, left, right, operation),
                (None, None) => Ok(true),
                _ => Ok(false),
            }
        }
        _ => Ok(false),
    }
}

fn solved_surfaces_equal(
    ctx: &DecodeContext<'_>,
    left: &SolvedSurfaceGeometry,
    right: &SolvedSurfaceGeometry,
    operation: &'static str,
) -> Result<bool, CodecError> {
    match (left, right) {
        (SolvedSurfaceGeometry::Nurbs(left), SolvedSurfaceGeometry::Nurbs(right)) => {
            if left.u_degree() != right.u_degree()
                || left.v_degree() != right.v_degree()
                || left.normal_reversed() != right.normal_reversed()
                || left.u_periodic() != right.u_periodic()
                || left.v_periodic() != right.v_periodic()
                || !lanes_equal(
                    ctx,
                    left.u_knots().as_slice(),
                    right.u_knots().as_slice(),
                    operation,
                )?
                || !lanes_equal(
                    ctx,
                    left.v_knots().as_slice(),
                    right.v_knots().as_slice(),
                    operation,
                )?
            {
                return Ok(false);
            }
            match (left.pole_grid(), right.pole_grid()) {
                (
                    NurbsPoleGrid::Polynomial { rows: left },
                    NurbsPoleGrid::Polynomial { rows: right },
                ) => grids_equal(ctx, left, right, operation),
                (
                    NurbsPoleGrid::Rational { rows: left },
                    NurbsPoleGrid::Rational { rows: right },
                ) => grids_equal(ctx, left, right, operation),
                _ => Ok(false),
            }
        }
        (SolvedSurfaceGeometry::Polygonal(left), SolvedSurfaceGeometry::Polygonal(right)) => {
            Ok(left.chordal_deflection() == right.chordal_deflection()
                && lanes_equal(ctx, left.vertices(), right.vertices(), operation)?
                && lanes_equal(ctx, left.triangles(), right.triangles(), operation)?)
        }
        (SolvedSurfaceGeometry::Transformed(left), SolvedSurfaceGeometry::Transformed(right)) => {
            if left.transform() != right.transform() {
                return Ok(false);
            }
            let _depth = ctx.enter_nested(operation)?;
            solved_surfaces_equal(ctx, left.basis(), right.basis(), operation)
        }
        (
            SolvedSurfaceGeometry::Unknown { record: left },
            SolvedSurfaceGeometry::Unknown { record: right },
        ) => ctx.equal(left, right, operation),
        (SolvedSurfaceGeometry::Plane(left), SolvedSurfaceGeometry::Plane(right)) => {
            Ok(left == right)
        }
        (SolvedSurfaceGeometry::Cylinder(left), SolvedSurfaceGeometry::Cylinder(right)) => {
            Ok(left == right)
        }
        (SolvedSurfaceGeometry::Cone(left), SolvedSurfaceGeometry::Cone(right)) => {
            Ok(left == right)
        }
        (SolvedSurfaceGeometry::Sphere(left), SolvedSurfaceGeometry::Sphere(right)) => {
            Ok(left == right)
        }
        (SolvedSurfaceGeometry::Torus(left), SolvedSurfaceGeometry::Torus(right)) => {
            Ok(left == right)
        }
        _ => Ok(false),
    }
}

pub(super) fn pcurves_equal(
    ctx: &DecodeContext<'_>,
    left: &PcurveGeometry,
    right: &PcurveGeometry,
    operation: &'static str,
) -> Result<bool, CodecError> {
    match (left, right) {
        (PcurveGeometry::Nurbs { nurbs: left }, PcurveGeometry::Nurbs { nurbs: right }) => {
            if left.degree() != right.degree()
                || left.periodic() != right.periodic()
                || !lanes_equal(
                    ctx,
                    left.knots().as_slice(),
                    right.knots().as_slice(),
                    operation,
                )?
            {
                return Ok(false);
            }
            match (left.pole_rows(), right.pole_rows()) {
                (
                    PcurveNurbsPoles::Polynomial { points: left },
                    PcurveNurbsPoles::Polynomial { points: right },
                ) => lanes_equal(ctx, left, right, operation),
                (
                    PcurveNurbsPoles::Rational { points: left },
                    PcurveNurbsPoles::Rational { points: right },
                ) => lanes_equal(ctx, left, right, operation),
                _ => Ok(false),
            }
        }
        (
            PcurveGeometry::PolarNurbs { nurbs: left },
            PcurveGeometry::PolarNurbs { nurbs: right },
        ) => {
            if left.degree() != right.degree()
                || left.periodic() != right.periodic()
                || !lanes_equal(
                    ctx,
                    left.knots().as_slice(),
                    right.knots().as_slice(),
                    operation,
                )?
            {
                return Ok(false);
            }
            match (left.pole_rows(), right.pole_rows()) {
                (
                    PolarNurbsPoles::Polynomial { poles: left },
                    PolarNurbsPoles::Polynomial { poles: right },
                ) => lanes_equal(ctx, left, right, operation),
                (
                    PolarNurbsPoles::Rational { poles: left },
                    PolarNurbsPoles::Rational { poles: right },
                ) => lanes_equal(ctx, left, right, operation),
                _ => Ok(false),
            }
        }
        (PcurveGeometry::Transformed(left), PcurveGeometry::Transformed(right)) => {
            if left.transform() != right.transform() {
                return Ok(false);
            }
            let _depth = ctx.enter_nested(operation)?;
            pcurves_equal(ctx, left.basis(), right.basis(), operation)
        }
        (PcurveGeometry::Trimmed(left), PcurveGeometry::Trimmed(right)) => {
            if left.parameter_range() != right.parameter_range()
                || left.same_sense() != right.same_sense()
            {
                return Ok(false);
            }
            let _depth = ctx.enter_nested(operation)?;
            pcurves_equal(ctx, left.basis(), right.basis(), operation)
        }
        (PcurveGeometry::Offset(left), PcurveGeometry::Offset(right)) => {
            if left.distance() != right.distance() {
                return Ok(false);
            }
            let _depth = ctx.enter_nested(operation)?;
            pcurves_equal(ctx, left.basis(), right.basis(), operation)
        }
        (PcurveGeometry::Line(_), PcurveGeometry::Line(_))
        | (PcurveGeometry::PolarHarmonic(_), PcurveGeometry::PolarHarmonic(_))
        | (PcurveGeometry::SphericalGreatCircle(_), PcurveGeometry::SphericalGreatCircle(_))
        | (PcurveGeometry::Circle(_), PcurveGeometry::Circle(_))
        | (PcurveGeometry::Ellipse(_), PcurveGeometry::Ellipse(_))
        | (PcurveGeometry::Harmonic(_), PcurveGeometry::Harmonic(_))
        | (PcurveGeometry::Parabola(_), PcurveGeometry::Parabola(_))
        | (PcurveGeometry::Hyperbola(_), PcurveGeometry::Hyperbola(_))
        | (PcurveGeometry::Hyperbolic(_), PcurveGeometry::Hyperbolic(_)) => Ok(left == right),
        _ => Ok(false),
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn geometry_lane_comparison_stops_at_first_difference() {
        let left = vec![0.0; 4096];
        let mut right = left.clone();
        right[0] = 1.0;
        crate::test_support::with_work_limit(1, |ctx| {
            assert!(!super::lanes_equal(ctx, &left, &right, "geometry lanes")
                .expect("one visited pair"));
            assert!(ctx.resource_refusal().is_none());
        });
        crate::test_support::with_work_limit(0, |ctx| {
            assert!(
                !super::lanes_equal(ctx, &left, &right[..4095], "geometry lanes")
                    .expect("different lengths visit no pairs")
            );
        });
        let result = crate::test_support::with_work_refusal("geometry lanes", |ctx| {
            super::lanes_equal(ctx, &left, &left, "geometry lanes")
        });
        assert!(
            matches!(result, Err(cadmpeg_core::CodecError::ResourceLimit(limit)) if limit.operation == "geometry lanes")
        );
    }
}
