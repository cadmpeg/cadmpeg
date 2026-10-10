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
    mut left: &SolvedSurfaceGeometry,
    mut right: &SolvedSurfaceGeometry,
    operation: &'static str,
) -> Result<bool, CodecError> {
    // Admitted geometry bounds every wrapper chain by MAX_GEOMETRY_NESTING.
    while let (SolvedSurfaceGeometry::Transformed(a), SolvedSurfaceGeometry::Transformed(b)) =
        (left, right)
    {
        if a.transform() != b.transform() {
            return Ok(false);
        }
        left = a.basis();
        right = b.basis();
    }
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
        (
            SolvedSurfaceGeometry::Unknown { record: left },
            SolvedSurfaceGeometry::Unknown { record: right },
        ) => match (left, right) {
            (None, None) => Ok(true),
            (Some(left), Some(right)) => ctx.equal_bytes(
                left.as_str().as_bytes(),
                right.as_str().as_bytes(),
                operation,
            ),
            _ => Ok(false),
        },
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
    mut left: &PcurveGeometry,
    mut right: &PcurveGeometry,
    operation: &'static str,
) -> Result<bool, CodecError> {
    // Admitted geometry bounds every wrapper chain by MAX_GEOMETRY_NESTING.
    loop {
        match (left, right) {
            (PcurveGeometry::Transformed(a), PcurveGeometry::Transformed(b)) => {
                if a.transform() != b.transform() {
                    return Ok(false);
                }
                left = a.basis();
                right = b.basis();
            }
            (PcurveGeometry::Trimmed(a), PcurveGeometry::Trimmed(b)) => {
                if a.parameter_range() != b.parameter_range() || a.same_sense() != b.same_sense() {
                    return Ok(false);
                }
                left = a.basis();
                right = b.basis();
            }
            (PcurveGeometry::Offset(a), PcurveGeometry::Offset(b)) => {
                if a.distance() != b.distance() {
                    return Ok(false);
                }
                left = a.basis();
                right = b.basis();
            }
            _ => break,
        }
    }
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

pub(super) fn rolling_ball_jets_equal(
    ctx: &DecodeContext<'_>,
    left: &cadmpeg_ir::geometry::ProceduralSurfaceDefinition,
    right: &cadmpeg_ir::geometry::ProceduralSurfaceDefinition,
    operation: &'static str,
) -> Result<bool, CodecError> {
    use cadmpeg_ir::geometry::ProceduralSurfaceDefinition;
    match (left, right) {
        (
            ProceduralSurfaceDefinition::RollingBallJet(left),
            ProceduralSurfaceDefinition::RollingBallJet(right),
        ) => Ok(left.degree() == right.degree()
            && lanes_equal(ctx, left.stations(), right.stations(), operation)?),
        // The rolling-ball evidence route admits jet constructions only.
        _ => Ok(false),
    }
}

fn extrusion_supports_equal(
    ctx: &DecodeContext<'_>,
    left: &crate::families::b5::transfer::ResolvedExtrusionSupport,
    right: &crate::families::b5::transfer::ResolvedExtrusionSupport,
    operation: &'static str,
) -> Result<bool, CodecError> {
    Ok(left.surface_object_id == right.surface_object_id
        && left.pcurve_parameter_range == right.pcurve_parameter_range
        && surfaces_equal(ctx, &left.surface, &right.surface, operation)?
        && pcurves_equal(ctx, &left.pcurve, &right.pcurve, operation)?)
}

/// Lifted extrusion curves contain only lines, circles, or NURBS.
fn extrusion_curves_equal(
    ctx: &DecodeContext<'_>,
    left: &cadmpeg_ir::geometry::CurveGeometry,
    right: &cadmpeg_ir::geometry::CurveGeometry,
    operation: &'static str,
) -> Result<bool, CodecError> {
    use cadmpeg_ir::geometry::{CurveGeometry, SolvedCurveGeometry};
    match (left, right) {
        (
            CurveGeometry::Solved(SolvedCurveGeometry::Nurbs(left)),
            CurveGeometry::Solved(SolvedCurveGeometry::Nurbs(right)),
        ) => nurbs_curves_equal(ctx, left, right, operation),
        (
            CurveGeometry::Solved(SolvedCurveGeometry::Line(left)),
            CurveGeometry::Solved(SolvedCurveGeometry::Line(right)),
        ) => Ok(left == right),
        (
            CurveGeometry::Solved(SolvedCurveGeometry::Circle(left)),
            CurveGeometry::Solved(SolvedCurveGeometry::Circle(right)),
        ) => Ok(left == right),
        _ => Ok(false),
    }
}

pub(super) fn extrusions_equal(
    ctx: &DecodeContext<'_>,
    left: &crate::families::b5::transfer::ResolvedExtrusionSurface,
    right: &crate::families::b5::transfer::ResolvedExtrusionSurface,
    operation: &'static str,
) -> Result<bool, CodecError> {
    use crate::families::b5::transfer::ResolvedExtrusionDirectrix;
    if left.surface_object_id != right.surface_object_id
        || left.directrix_object_id != right.directrix_object_id
        || left.directrix_parameter_range != right.directrix_parameter_range
        || left.direction != right.direction
        || left.parameter_bounds != right.parameter_bounds
    {
        return Ok(false);
    }
    match (&left.directrix, &right.directrix) {
        (
            ResolvedExtrusionDirectrix::Intersection {
                supports: left,
                cache_fit_tolerance: left_tolerance,
            },
            ResolvedExtrusionDirectrix::Intersection {
                supports: right,
                cache_fit_tolerance: right_tolerance,
            },
        ) => Ok(left_tolerance == right_tolerance
            && extrusion_supports_equal(ctx, &left[0], &right[0], operation)?
            && extrusion_supports_equal(ctx, &left[1], &right[1], operation)?),
        (
            ResolvedExtrusionDirectrix::SurfaceCurve {
                support: left,
                curve: left_curve,
            },
            ResolvedExtrusionDirectrix::SurfaceCurve {
                support: right,
                curve: right_curve,
            },
        ) => Ok(extrusion_supports_equal(ctx, left, right, operation)?
            && extrusion_curves_equal(ctx, left_curve, right_curve, operation)?),
        (
            ResolvedExtrusionDirectrix::Offset {
                source_object_id: left_id,
                support: left,
                source_curve: left_curve,
                source_parameter_range: left_range,
                distance: left_distance,
                direction: left_direction,
            },
            ResolvedExtrusionDirectrix::Offset {
                source_object_id: right_id,
                support: right,
                source_curve: right_curve,
                source_parameter_range: right_range,
                distance: right_distance,
                direction: right_direction,
            },
        ) => Ok(left_id == right_id
            && left_range == right_range
            && left_distance == right_distance
            && left_direction == right_direction
            && extrusion_supports_equal(ctx, left, right, operation)?
            && extrusion_curves_equal(ctx, left_curve, right_curve, operation)?),
        _ => Ok(false),
    }
}

pub(super) fn procedures_equal(
    ctx: &DecodeContext<'_>,
    left: &super::StandardSurfaceProcedure,
    right: &super::StandardSurfaceProcedure,
    operation: &'static str,
) -> Result<bool, CodecError> {
    use super::StandardSurfaceProcedure;
    use crate::families::b5::transfer::ResolvedOffsetSupport;
    match (left, right) {
        (
            StandardSurfaceProcedure::RollingBall {
                carrier_object_id: left_id,
                definition: left,
                source: left_source,
            },
            StandardSurfaceProcedure::RollingBall {
                carrier_object_id: right_id,
                definition: right,
                source: right_source,
            },
        ) => Ok(left_id == right_id
            && left_source == right_source
            && rolling_ball_jets_equal(ctx, left, right, operation)?),
        (
            StandardSurfaceProcedure::Revolution(left),
            StandardSurfaceProcedure::Revolution(right),
        ) => Ok(left.axis_origin == right.axis_origin
            && left.axis_direction == right.axis_direction
            && left.angular_interval == right.angular_interval
            && left.angular_parameter_interval == right.angular_parameter_interval
            && left.parameter_interval == right.parameter_interval
            && nurbs_curves_equal(ctx, &left.directrix, &right.directrix, operation)?),
        (
            StandardSurfaceProcedure::Offset {
                carrier_object_id: left_id,
                support_object_id: left_support_id,
                support: left,
                distance: left_distance,
                parameter_bounds: left_bounds,
            },
            StandardSurfaceProcedure::Offset {
                carrier_object_id: right_id,
                support_object_id: right_support_id,
                support: right,
                distance: right_distance,
                parameter_bounds: right_bounds,
            },
        ) => {
            if left_id != right_id
                || left_support_id != right_support_id
                || left_distance != right_distance
                || left_bounds != right_bounds
            {
                return Ok(false);
            }
            match (left, right) {
                (ResolvedOffsetSupport::Geometry(left), ResolvedOffsetSupport::Geometry(right)) => {
                    surfaces_equal(ctx, left, right, operation)
                }
                (
                    ResolvedOffsetSupport::Extrusion(left),
                    ResolvedOffsetSupport::Extrusion(right),
                ) => extrusions_equal(ctx, left, right, operation),
                _ => Ok(false),
            }
        }
        (StandardSurfaceProcedure::Extrusion(left), StandardSurfaceProcedure::Extrusion(right)) => {
            extrusions_equal(ctx, left, right, operation)
        }
        _ => Ok(false),
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn unknown_surface_equality_admits_only_present_id_pairs() {
        use cadmpeg_ir::geometry::{SolvedSurfaceGeometry, SurfaceGeometry};
        use cadmpeg_ir::ids::UnknownId;
        let absent = SurfaceGeometry::Solved(SolvedSurfaceGeometry::Unknown { record: None });
        let present = SurfaceGeometry::Solved(SolvedSurfaceGeometry::Unknown {
            record: Some(
                UnknownId::mint(format!("catia:test:unknown#{}", "x".repeat(4096)))
                    .expect("identity"),
            ),
        });
        crate::test_support::with_work_limit(0, |ctx| {
            assert!(
                super::surfaces_equal(ctx, &absent, &absent, "unknown equality")
                    .expect("fixed option match")
            );
            assert!(
                !super::surfaces_equal(ctx, &absent, &present, "unknown equality")
                    .expect("fixed option mismatch")
            );
            assert!(
                !super::surfaces_equal(ctx, &present, &absent, "unknown equality")
                    .expect("fixed option mismatch")
            );
        });
        let result = crate::test_support::with_work_refusal("unknown equality", |ctx| {
            super::surfaces_equal(ctx, &present, &present, "unknown equality")
        });
        assert!(
            matches!(result, Err(cadmpeg_core::CodecError::ResourceLimit(limit)) if limit.operation == "unknown equality")
        );
    }

    #[test]
    fn extrusion_equality_admits_each_directrix_geometry_lane() {
        use super::super::{StandardSupportRef, StandardSurfaceProcedure};
        use crate::families::b5::transfer::{
            ResolvedExtrusionDirectrix, ResolvedExtrusionSupport, ResolvedExtrusionSurface,
            ResolvedOffsetSupport,
        };
        use cadmpeg_ir::geometry::pcurve::{LinePcurve, PcurveGeometry, PcurveNurbs};
        use cadmpeg_ir::geometry::{
            CurveGeometry, SolvedCurveGeometry, SolvedSurfaceGeometry, SurfaceGeometry,
        };
        use cadmpeg_ir::math::{Point2, Point3, Vector3};
        use cadmpeg_ir::scalar::{FiniteReal, PositiveReal};
        use cadmpeg_ir::topology::IncreasingParameterInterval;
        let interval = IncreasingParameterInterval::new([0.0, 1.0]).expect("interval");
        let direction =
            cadmpeg_ir::units::UnitVector3::new(Vector3::new(0.0, 0.0, 1.0)).expect("direction");
        let support = ResolvedExtrusionSupport {
            surface_object_id: 1,
            surface: SurfaceGeometry::Solved(SolvedSurfaceGeometry::Unknown { record: None }),
            pcurve: PcurveGeometry::Line(LinePcurve::U_AXIS),
            pcurve_parameter_range: [0.0, 1.0],
        };
        let curve = CurveGeometry::Solved(SolvedCurveGeometry::Nurbs(
            super::NurbsCurve::from_lanes(
                &cadmpeg_test_support::service_decode_context(),
                1,
                vec![0.0, 0.0, 1.0, 1.0],
                vec![Point3::new(0.0, 0.0, 0.0), Point3::new(1.0, 0.0, 0.0)],
                None,
                false,
            )
            .expect("admission")
            .expect("curve"),
        ));
        let pcurve = PcurveGeometry::Nurbs {
            nurbs: PcurveNurbs::from_lanes(
                &cadmpeg_test_support::service_decode_context(),
                1,
                vec![0.0, 0.0, 1.0, 1.0],
                vec![Point2::new(0.0, 0.0), Point2::new(1.0, 0.0)],
                None,
                false,
            )
            .expect("admission")
            .expect("pcurve"),
        };
        let mut pcurve_support = support.clone();
        pcurve_support.pcurve = pcurve;
        let mut surface_support = support.clone();
        surface_support.surface = SurfaceGeometry::Solved(SolvedSurfaceGeometry::Unknown {
            record: Some(
                cadmpeg_ir::ids::UnknownId::mint(format!(
                    "catia:test:unknown#{}",
                    "x".repeat(4096)
                ))
                .expect("identity"),
            ),
        });
        let line = CurveGeometry::Solved(SolvedCurveGeometry::Line(
            cadmpeg_ir::geometry::analytic::LineCurve::try_new(
                Point3::new(0.0, 0.0, 0.0),
                Vector3::from(direction),
            )
            .expect("line"),
        ));
        for directrix in [
            ResolvedExtrusionDirectrix::Intersection {
                supports: Box::new([support.clone(), surface_support]),
                cache_fit_tolerance: PositiveReal::new(0.001).expect("tolerance"),
            },
            ResolvedExtrusionDirectrix::Intersection {
                supports: Box::new([support.clone(), pcurve_support.clone()]),
                cache_fit_tolerance: PositiveReal::new(0.001).expect("tolerance"),
            },
            ResolvedExtrusionDirectrix::SurfaceCurve {
                support: pcurve_support,
                curve: line,
            },
            ResolvedExtrusionDirectrix::SurfaceCurve {
                support: support.clone(),
                curve: curve.clone(),
            },
            ResolvedExtrusionDirectrix::Offset {
                source_object_id: 3,
                support,
                source_curve: curve,
                source_parameter_range: interval,
                distance: FiniteReal::new(1.0).expect("distance"),
                direction,
            },
        ] {
            let extrusion = ResolvedExtrusionSurface {
                surface_object_id: 4,
                directrix_object_id: 5,
                directrix_parameter_range: interval,
                direction,
                parameter_bounds: [interval; 2],
                directrix,
            };
            let procedure = StandardSurfaceProcedure::Extrusion(Box::new(extrusion.clone()));
            let offset = ResolvedOffsetSupport::Extrusion(Box::new(extrusion.clone()));
            let offset_procedure = StandardSurfaceProcedure::Offset {
                carrier_object_id: 6,
                support_object_id: 4,
                support: offset.clone(),
                distance: FiniteReal::new(1.0).expect("distance"),
                parameter_bounds: [interval; 2],
            };
            crate::test_support::with_service_context(|ctx| {
                assert!(
                    super::procedures_equal(ctx, &procedure, &procedure, "extrusion equality")
                        .expect("equal geometry")
                );
                assert!(super::procedures_equal(
                    ctx,
                    &offset_procedure,
                    &offset_procedure,
                    "extrusion equality"
                )
                .expect("equal offset geometry"));
                assert!(StandardSupportRef::Offset(&offset)
                    .equivalent(ctx, &StandardSupportRef::Offset(&offset))
                    .expect("equal support"));
            });
            for procedure in [&procedure, &offset_procedure] {
                let result = crate::test_support::with_work_refusal("extrusion equality", |ctx| {
                    super::procedures_equal(ctx, procedure, procedure, "extrusion equality")
                });
                assert!(
                    matches!(result, Err(cadmpeg_core::CodecError::ResourceLimit(limit)) if limit.operation == "extrusion equality")
                );
            }
            let result = crate::test_support::with_work_refusal(
                "catia_standard_support_candidates",
                |ctx| {
                    StandardSupportRef::Offset(&offset)
                        .equivalent(ctx, &StandardSupportRef::Offset(&offset))
                },
            );
            assert!(
                matches!(result, Err(cadmpeg_core::CodecError::ResourceLimit(limit)) if limit.operation == "catia_standard_support_candidates")
            );
            let mut different = extrusion.clone();
            different.surface_object_id += 1;
            crate::test_support::with_work_limit(0, |ctx| {
                assert!(
                    !super::extrusions_equal(ctx, &extrusion, &different, "extrusion equality")
                        .expect("fixed metadata mismatch")
                )
            });
        }
    }

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
    #[test]
    fn geometry_wrappers_use_the_fixed_ir_depth_bound_without_refusal() {
        use cadmpeg_ir::geometry::pcurve::{LinePcurve, OffsetPcurve, PcurveGeometry};
        let mut pcurve = PcurveGeometry::Line(LinePcurve::U_AXIS);
        for _ in 0..cadmpeg_ir::geometry::MAX_GEOMETRY_NESTING {
            pcurve = PcurveGeometry::Offset(
                OffsetPcurve::try_new(1.0, Box::new(pcurve)).expect("admitted depth"),
            );
        }
        crate::test_support::with_work_limit(0, |ctx| {
            assert!(
                super::pcurves_equal(ctx, &pcurve, &pcurve, "fixed geometry wrappers")
                    .expect("bounded analytic chain")
            );
            assert!(ctx.resource_refusal().is_none());
        });
    }
}
