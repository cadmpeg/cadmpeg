// SPDX-License-Identifier: Apache-2.0
//! Exact NURBS surface isolines contained in a support plane.

use cadmpeg_core::decode::DecodeContext;
use cadmpeg_core::CodecError;
use cadmpeg_ir::geometry::analytic::PlaneSurface;
use cadmpeg_ir::geometry::nurbs::{NurbsCurve, NurbsSurface};
use cadmpeg_ir::math::Point3;

const EPS_BOUNDARY_PLANE_MM: f64 = 1.0e-9;
const EPS_ISOLINE_PLANE_MM: f64 = 1.0e-9;
const EPS_ISOLINE_WITNESS_MM: f64 = 1.0e-7;

fn reverse_in_domain(curve: &mut NurbsCurve) -> Option<()> {
    let [lower, upper] = cadmpeg_ir::eval::nurbs_curve_parameter_domain(curve)?.endpoints();
    curve.reverse_parameterization_in_range(
        cadmpeg_ir::scalar::FiniteReal::new(lower)?,
        cadmpeg_ir::scalar::FiniteReal::new(upper)?,
    )
}

pub(crate) fn interior(
    ctx: &DecodeContext<'_>,
    plane: &PlaneSurface,
    surface: &NurbsSurface,
    witnesses: [Point3; 2],
) -> Result<Option<NurbsCurve>, CodecError> {
    let Some(uv) = cadmpeg_ir::eval::nurbs_surface_parameter_within_tolerance(
        ctx,
        surface,
        witnesses[0],
        None,
        EPS_ISOLINE_WITNESS_MM,
    )?
    else {
        return Ok(None);
    };
    let uv = uv.get();
    let mut selected = None;
    for (direction, fixed, seed) in [
        (cadmpeg_ir::eval::IsolineDirection::ConstantU, uv.u, uv.v),
        (cadmpeg_ir::eval::IsolineDirection::ConstantV, uv.v, uv.u),
    ] {
        let Some(mut curve) =
            cadmpeg_ir::eval::nurbs_surface_isoline(ctx, surface, direction, fixed)?
        else {
            continue;
        };
        if curve.control_points().iter().any(|pole| {
            pole.get()
                .vector_from(plane.origin().get())
                .dot(*plane.frame().axis().as_raw())
                .abs()
                > EPS_ISOLINE_PLANE_MM
        }) {
            continue;
        }
        let Some(first) = cadmpeg_ir::eval::nurbs_curve_parameter_near_point(
            ctx,
            &curve,
            witnesses[0],
            EPS_ISOLINE_WITNESS_MM,
            seed,
        )?
        else {
            continue;
        };
        let Some(second) = cadmpeg_ir::eval::nurbs_curve_parameter_near_point(
            ctx,
            &curve,
            witnesses[1],
            EPS_ISOLINE_WITNESS_MM,
            first.get(),
        )?
        else {
            continue;
        };
        if first == second {
            continue;
        }
        if second < first && reverse_in_domain(&mut curve).is_none() {
            continue;
        }
        if selected.replace(curve).is_some() {
            return Ok(None);
        }
    }
    Ok(selected)
}

pub(crate) fn section(
    plane: &PlaneSurface,
    surface: &NurbsSurface,
    witnesses: [Point3; 2],
) -> Option<NurbsCurve> {
    let mut selected = None;
    for (fixed_u, upper) in [(true, false), (true, true), (false, false), (false, true)] {
        if (fixed_u && surface.u_periodic()) || (!fixed_u && surface.v_periodic()) {
            continue;
        }
        let (fixed_degree, fixed_knots, fixed_count, degree, knots, count, periodic) = if fixed_u {
            (
                surface.u_degree(),
                surface.u_knots(),
                surface.u_count(),
                surface.v_degree(),
                surface.v_knots(),
                surface.v_count(),
                surface.v_periodic(),
            )
        } else {
            (
                surface.v_degree(),
                surface.v_knots(),
                surface.v_count(),
                surface.u_degree(),
                surface.u_knots(),
                surface.u_count(),
                surface.u_periodic(),
            )
        };
        let multiplicity = usize::try_from(fixed_degree).ok()?.checked_add(1)?;
        let clamped = if upper {
            fixed_knots
                .iter()
                .rev()
                .take(multiplicity)
                .all(|value| Some(value) == fixed_knots.last())
        } else {
            fixed_knots
                .iter()
                .take(multiplicity)
                .all(|value| Some(value) == fixed_knots.first())
        };
        if !clamped {
            continue;
        }
        let fixed = if upper {
            fixed_count.checked_sub(1)?
        } else {
            0
        };
        let index = |free| {
            if fixed_u {
                (fixed, free)
            } else {
                (free, fixed)
            }
        };
        let poles = (0..count)
            .map(|free| {
                let (u, v) = index(free);
                surface.pole(u, v)
            })
            .collect::<Option<Vec<_>>>()?;
        if poles.iter().any(|pole| {
            pole.get()
                .vector_from(plane.origin().get())
                .dot(*plane.frame().axis().as_raw())
                .abs()
                > EPS_BOUNDARY_PLANE_MM
        }) {
            continue;
        }
        let weights = (0..count)
            .map(|free| {
                let (u, v) = index(free);
                surface.weight(u, v)
            })
            .collect::<Option<Vec<_>>>();
        let direction = poles.last()?.get().vector_from(poles.first()?.get());
        let alignment = witnesses[1].vector_from(witnesses[0]).dot(direction);
        if alignment == 0.0 || !alignment.is_finite() {
            continue;
        }
        let mut curve =
            NurbsCurve::from_checked_lanes(degree, knots.clone(), poles, weights, periodic).ok()?;
        if alignment < 0.0 {
            reverse_in_domain(&mut curve)?;
        }
        if selected.replace(curve).is_some() {
            return None;
        }
    }
    selected
}

#[cfg(test)]
mod tests {
    use super::{interior, section};
    use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy};
    use cadmpeg_ir::geometry::analytic::PlaneSurface;
    use cadmpeg_ir::geometry::nurbs::{NurbsSurface, NurbsSurfaceAxis, NurbsSurfaceLanes};
    use cadmpeg_ir::math::{Point3, Vector3};

    const EPS_ISOLINE_POLE_MM: f64 = 1.0e-9;

    #[test]
    fn interior_isoline_keeps_the_exact_rational_section_and_direction() {
        for rational in [false, true] {
            for reversed in [false, true] {
                let surface = surface(true, rational);
                let plane = PlaneSurface::try_new(
                    Point3::new(1.0, 0.0, 0.0),
                    Vector3::new(1.0, 0.0, 0.0),
                    Vector3::new(0.0, 1.0, 0.0),
                )
                .expect("interior support plane");
                let mut witnesses = [
                    Point3::new(1.0, 0.0, 0.0),
                    Point3::new(1.0, 1.0, if rational { 1.0 } else { 0.75 }),
                ];
                let mut expected = [
                    Point3::new(1.0, 0.0, 0.0),
                    Point3::new(1.0, 1.0, 1.5),
                    Point3::new(1.0, 2.0, 0.0),
                ];
                if reversed {
                    witnesses.reverse();
                    expected.reverse();
                }
                let arena = DecodeArena::new();
                let policy = DecodePolicy::default();
                let (ctx, _) =
                    DecodeContext::from_root_bytes(&[], &arena, &policy).expect("context");
                let curve = interior(&ctx, &plane, &surface, witnesses)
                    .expect("evaluation budget")
                    .expect("interior section");
                assert_eq!(curve.degree(), 2);
                assert_eq!(curve.knots(), surface.v_knots());
                for (actual, expected) in curve.control_points().iter().zip(expected) {
                    assert!(actual.get().distance(expected) < EPS_ISOLINE_POLE_MM);
                }
                assert_eq!(
                    curve.weights(),
                    surface.weights().map(|rows| rows[0].clone())
                );
                ctx.finish_session().expect("session");
            }
        }
    }

    #[test]
    fn an_off_surface_chart_cannot_select_an_isoline() {
        let arena = DecodeArena::new();
        let policy = DecodePolicy::default();
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("context");
        assert!(interior(
            &ctx,
            &plane(),
            &surface(true, false),
            [Point3::new(3.0, 0.0, 0.0), Point3::new(3.0, 1.0, 0.5)],
        )
        .expect("evaluation budget")
        .is_none());
        ctx.finish_session().expect("session");
    }

    fn surface(clamped: bool, rational: bool) -> NurbsSurface {
        NurbsSurface::from_lanes(
            NurbsSurfaceAxis::new(
                1,
                if clamped {
                    vec![0.0, 0.0, 1.0, 1.0]
                } else {
                    vec![-1.0, 0.0, 1.0, 2.0]
                },
                false,
            ),
            NurbsSurfaceAxis::new(2, vec![0.0, 0.0, 0.0, 1.0, 1.0, 1.0], false),
            NurbsSurfaceLanes::new(
                vec![
                    vec![
                        Point3::new(0.0, 0.0, 0.0),
                        Point3::new(0.0, 1.0, 1.0),
                        Point3::new(0.0, 2.0, 0.0),
                    ],
                    vec![
                        Point3::new(2.0, 0.0, 0.0),
                        Point3::new(2.0, 1.0, 2.0),
                        Point3::new(2.0, 2.0, 0.0),
                    ],
                ],
                rational.then(|| vec![vec![1.0, 2.0, 1.0], vec![1.0, 2.0, 1.0]]),
            ),
            false,
        )
        .expect("nonplanar patch")
    }

    fn plane() -> PlaneSurface {
        PlaneSurface::try_new(
            Point3::new(0.0, 0.0, 0.0),
            Vector3::new(1.0, 0.0, 0.0),
            Vector3::new(0.0, 1.0, 0.0),
        )
        .expect("support plane")
    }

    #[test]
    fn clamped_boundary_keeps_exact_poles_knots_and_weights() {
        for rational in [false, true] {
            let surface = surface(true, rational);
            let curve = section(
                &plane(),
                &surface,
                [Point3::new(0.0, 0.0, 0.0), Point3::new(0.0, 1.0, 0.5)],
            )
            .expect("boundary section");
            assert_eq!(curve.degree(), 2);
            assert_eq!(curve.knots(), surface.v_knots());
            assert_eq!(curve.control_points(), surface.control_grid()[0]);
            assert_eq!(
                curve.weights(),
                surface.weights().map(|rows| rows[0].clone())
            );
        }
    }

    #[test]
    fn chart_direction_reverses_the_boundary_parameter() {
        let surface = surface(true, false);
        let curve = section(
            &plane(),
            &surface,
            [Point3::new(0.0, 1.0, 0.5), Point3::new(0.0, 0.0, 0.0)],
        )
        .expect("reversed boundary");
        let mut expected = surface.control_grid()[0].clone();
        expected.reverse();
        assert_eq!(curve.control_points(), expected);
    }

    #[test]
    fn unclamped_control_rows_do_not_define_boundary_sections() {
        assert!(section(
            &plane(),
            &surface(false, false),
            [Point3::new(0.0, 0.0, 0.0), Point3::new(0.0, 1.0, 0.5)],
        )
        .is_none());
    }
}
