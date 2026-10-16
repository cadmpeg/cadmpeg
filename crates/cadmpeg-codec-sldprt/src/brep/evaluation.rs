// SPDX-License-Identifier: Apache-2.0
//! Caller admission for stored carrier evaluation scratch.

use cadmpeg_core::decode::{DecodeContext, WorkBudget};
use cadmpeg_core::CodecError;
use cadmpeg_ir::features::FinitePoint3;
use cadmpeg_ir::geometry::nurbs::NurbsCurve;
use cadmpeg_ir::geometry::CurveGeometry;
use cadmpeg_ir::report::loss::LossNote;

pub(super) fn curve_point(
    ctx: &DecodeContext<'_>,
    curve: &CurveGeometry,
    parameter: f64,
) -> Result<Option<FinitePoint3>, CodecError> {
    Ok(cadmpeg_ir::eval::finite_or_refusal(
        cadmpeg_ir::eval::decode::outer_refusal(cadmpeg_ir::eval::decode::curve_point(
            ctx, curve, parameter,
        ))?,
    )?)
}

pub(super) fn nurbs_curve_point(
    ctx: &DecodeContext<'_>,
    curve: &NurbsCurve,
    parameter: f64,
) -> Result<Option<FinitePoint3>, CodecError> {
    Ok(cadmpeg_ir::eval::finite_or_refusal(
        cadmpeg_ir::eval::decode::outer_refusal(cadmpeg_ir::eval::decode::nurbs_curve_point_at(
            ctx, curve, parameter,
        ))?,
    )?)
}

pub(crate) fn nurbs_surface_point(
    ctx: &DecodeContext<'_>,
    surface: &cadmpeg_ir::geometry::nurbs::NurbsSurface,
    u: f64,
    v: f64,
) -> Result<Option<FinitePoint3>, CodecError> {
    Ok(cadmpeg_ir::eval::finite_or_refusal(
        cadmpeg_ir::eval::decode::outer_refusal(cadmpeg_ir::eval::decode::nurbs_surface_point(
            ctx, surface, u, v,
        ))?,
    )?)
}

pub(crate) fn nurbs_surface_parameter_near_point(
    ctx: &DecodeContext<'_>,
    surface: &cadmpeg_ir::geometry::nurbs::NurbsSurface,
    point: cadmpeg_ir::math::Point3,
    seed: Option<cadmpeg_ir::math::Point2>,
) -> Result<Option<cadmpeg_ir::units::FinitePoint2>, CodecError> {
    Ok(cadmpeg_ir::eval::nurbs_surface_parameter_near_point(
        ctx, surface, point, seed,
    )?)
}

pub(crate) fn nurbs_surface_partials(
    ctx: &DecodeContext<'_>,
    surface: &cadmpeg_ir::geometry::nurbs::NurbsSurface,
    u: f64,
    v: f64,
) -> Result<
    Option<cadmpeg_ir::eval::SurfacePartials<FinitePoint3, cadmpeg_ir::features::FiniteVector3>>,
    CodecError,
> {
    Ok(cadmpeg_ir::eval::finite_or_refusal(
        cadmpeg_ir::eval::nurbs_surface_partials(ctx, surface, u, v),
    )?)
}

pub(crate) fn surface_point(
    ctx: &DecodeContext<'_>,
    surface: &cadmpeg_ir::geometry::SurfaceGeometry,
    u: f64,
    v: f64,
) -> Result<Option<FinitePoint3>, CodecError> {
    Ok(cadmpeg_ir::eval::finite_or_refusal(
        cadmpeg_ir::eval::decode::outer_refusal(cadmpeg_ir::eval::decode::surface_point(
            ctx, surface, u, v,
        ))?,
    )?)
}

const SURFACE_SOLVER_LOCAL_WORK: usize = 1_000_000;

fn surface_solver_work_capped_loss() -> LossNote {
    crate::loss::SldprtLossCode::GeometrySurfaceSolverWorkCapped.note(format!(
        "NURBS surface solver exceeded the local work cap SURFACE_SOLVER_LOCAL_WORK \
         ({SURFACE_SOLVER_LOCAL_WORK}); the derived pcurve is skipped."
    ))
}

fn with_surface_solver_work<T>(
    work: usize,
    losses: &mut Vec<LossNote>,
    run: impl FnOnce(&WorkBudget) -> Result<Option<T>, CodecError>,
) -> Result<Option<T>, CodecError> {
    let budget = WorkBudget::new(work);
    let result = run(&budget);
    if budget.exhausted() {
        losses.push(surface_solver_work_capped_loss());
        return Ok(None);
    }
    result
}

pub(super) fn nurbs_surface_parameter_within_tolerance(
    ctx: &DecodeContext<'_>,
    surface: &cadmpeg_ir::geometry::nurbs::NurbsSurface,
    point: cadmpeg_ir::math::Point3,
    seed: Option<cadmpeg_ir::math::Point2>,
    tolerance: f64,
    losses: &mut Vec<LossNote>,
) -> Result<Option<cadmpeg_ir::units::FinitePoint2>, CodecError> {
    with_surface_solver_work(SURFACE_SOLVER_LOCAL_WORK, losses, |budget| {
        Ok(
            cadmpeg_ir::eval::nurbs_surface_parameter_within_tolerance_with_budget(
                ctx, surface, point, seed, tolerance, budget,
            )?,
        )
    })
}

pub(super) fn nurbs_surface_parameter_segment_chord_bound(
    ctx: &DecodeContext<'_>,
    surface: &cadmpeg_ir::geometry::nurbs::NurbsSurface,
    parameters: [cadmpeg_ir::math::Point2; 2],
    chord: [cadmpeg_ir::math::Point3; 2],
    losses: &mut Vec<LossNote>,
) -> Result<Option<f64>, CodecError> {
    with_surface_solver_work(SURFACE_SOLVER_LOCAL_WORK, losses, |budget| {
        cadmpeg_ir::eval::nurbs_surface_parameter_segment_chord_bound_with_budget(
            ctx, surface, parameters, chord, budget,
        )
    })
}

#[cfg(test)]
mod tests {
    use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, WorkBudget};
    use cadmpeg_ir::geometry::nurbs::{NurbsSurface, NurbsSurfaceAxis, NurbsSurfaceLanes};
    use cadmpeg_ir::math::Point3;

    /// A dense wavy cubic patch grid whose global inversion of an off-surface
    /// point needs more than the local solver slice.
    fn dense_wavy_surface() -> NurbsSurface {
        const COUNT: u32 = 80;
        let mut knots = vec![0.0; 4];
        knots.extend((1..COUNT - 3).map(f64::from));
        knots.extend([f64::from(COUNT - 3); 4]);
        NurbsSurface::from_lanes(
            NurbsSurfaceAxis::new(3, knots.clone(), false),
            NurbsSurfaceAxis::new(3, knots, false),
            NurbsSurfaceLanes::new(
                (0..COUNT)
                    .map(|u| {
                        (0..COUNT)
                            .map(|v| {
                                let (u, v) = (f64::from(u), f64::from(v));
                                Point3::new(u, v, (u * 0.7).sin() * (v * 0.9).cos())
                            })
                            .collect()
                    })
                    .collect(),
                None,
            ),
            false,
        )
        .expect("dense cubic surface")
    }

    #[test]
    fn surface_inversion_skips_when_the_local_solver_slice_is_exhausted() {
        let surface = dense_wavy_surface();
        let point = Point3::new(40.5, 40.5, 50.0);
        let arena = DecodeArena::new();
        let policy = DecodePolicy::service();
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
        let budget = WorkBudget::new(super::SURFACE_SOLVER_LOCAL_WORK);
        let solved = cadmpeg_ir::eval::nurbs_surface_parameter_within_tolerance_with_budget(
            &ctx, &surface, point, None, 1.0e-3, &budget,
        );
        assert!(
            solved.is_ok(),
            "the session admits the local slice: {solved:?}"
        );
        assert!(
            budget.exhausted(),
            "the fixture must exhaust the local slice"
        );

        let arena = DecodeArena::new();
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
        let mut losses = Vec::new();
        assert!(super::nurbs_surface_parameter_within_tolerance(
            &ctx,
            &surface,
            point,
            None,
            1.0e-3,
            &mut losses
        )
        .unwrap()
        .is_none());
        assert_eq!(losses.len(), 1);
        assert_eq!(
            losses[0].code.local_code(),
            "geometry.surface-solver-work-capped"
        );
        assert!(losses[0].message.contains("SURFACE_SOLVER_LOCAL_WORK"));
        assert!(ctx.finish_session().is_ok());
    }

    #[test]
    fn surface_chord_bound_skips_when_the_local_solver_slice_is_exhausted() {
        let surface = dense_wavy_surface();
        let parameters = [
            cadmpeg_ir::math::Point2::new(0.0, 0.0),
            cadmpeg_ir::math::Point2::new(76.0, 76.0),
        ];
        let chord = [
            cadmpeg_ir::math::Point3::new(0.0, 0.0, 0.0),
            cadmpeg_ir::math::Point3::new(76.0, 76.0, 50.0),
        ];
        let arena = DecodeArena::new();
        let policy = DecodePolicy::service();
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
        let mut losses = Vec::new();
        assert!(super::with_surface_solver_work(0, &mut losses, |budget| {
            cadmpeg_ir::eval::nurbs_surface_parameter_segment_chord_bound_with_budget(
                &ctx, &surface, parameters, chord, budget,
            )
        })
        .unwrap()
        .is_none());
        assert_eq!(losses.len(), 1);
        assert_eq!(
            losses[0].code.local_code(),
            "geometry.surface-solver-work-capped"
        );
        assert!(losses[0].message.contains("SURFACE_SOLVER_LOCAL_WORK"));
        assert!(ctx.finish_session().is_ok());
    }
}
