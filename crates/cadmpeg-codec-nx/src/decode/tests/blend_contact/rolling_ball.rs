// SPDX-License-Identifier: Apache-2.0
//! Decode-owner unit tests.

use super::EPS_TOPOLOGY_TOLERANCE;
use crate::decode::blend::{
    blend_surface_parameters, blend_surface_parameters_for_fit, blend_surface_point,
    blend_surface_u_derivative, coarse_blend_surface_parameters, refine_blend_surface_parameters,
    BlendParameterGrid,
};
use crate::decode::pcurves::blend_boundary_parameter_from_support_spine;
use cadmpeg_ir::geometry::{
    pcurve::PcurveGeometry, BlendCrossSection, BlendRadiusLaw, CurveGeometry,
    ProceduralSurfaceDefinition, SolvedCurveGeometry, SolvedSurfaceGeometry, SurfaceGeometry,
};
use cadmpeg_ir::math::{Point2, Point3, Vector3};
use cadmpeg_test_support::edit;

#[test]
fn rolling_ball_blend_parameters_invert_the_canal_surface_law() {
    use cadmpeg_ir::geometry::{
        BlendSupport, Curve, IntcurveSupportContext, IntcurveSupportSide, ProceduralCurve,
        ProceduralCurveDefinition, ProceduralSurface, Surface,
    };
    use cadmpeg_ir::ids::{
        CurveId, EdgeId, ProceduralCurveId, ProceduralSurfaceId, SurfaceId, VertexId,
    };
    use cadmpeg_ir::topology::Edge;

    const OUTSIDE_BLEND_SECTION_DELTA: f64 = 1.0e-6;
    const DIRECT_INVERSE_TOLERANCE: f64 = 1.0e-8;

    crate::test_support::with_decode_context(|geometry_ctx| {
        let mut ir = cadmpeg_ir::document::CadIr::empty();
        let first =
            SurfaceId::mint("test:model:entity#synthetic:first-plane").expect("identity grammar");
        let second =
            SurfaceId::mint("test:model:entity#synthetic:second-plane").expect("identity grammar");
        ir.model.surfaces.extend([
            Surface {
                id: first.clone(),
                geometry: SurfaceGeometry::Solved(SolvedSurfaceGeometry::Plane(
                    cadmpeg_ir::geometry::analytic::PlaneSurface::try_new(
                        cadmpeg_ir::math::Point3::new(0.0, 0.0, 0.0),
                        Vector3::new(1.0, 0.0, 0.0),
                        Vector3::new(0.0, 0.0, 1.0),
                    )
                    .unwrap(),
                )),
                source_object: None,
            },
            Surface {
                id: second.clone(),
                geometry: SurfaceGeometry::Solved(SolvedSurfaceGeometry::Plane(
                    cadmpeg_ir::geometry::analytic::PlaneSurface::try_new(
                        cadmpeg_ir::math::Point3::new(0.0, 0.0, 0.0),
                        Vector3::new(0.0, 1.0, 0.0),
                        Vector3::new(0.0, 0.0, 1.0),
                    )
                    .unwrap(),
                )),
                source_object: None,
            },
        ]);
        let first_spine_side = SurfaceId::mint("test:model:entity#synthetic:first-spine-side")
            .expect("identity grammar");
        let second_spine_side = SurfaceId::mint("test:model:entity#synthetic:second-spine-side")
            .expect("identity grammar");
        ir.model.surfaces.extend([
            Surface {
                id: first_spine_side.clone(),
                geometry: SurfaceGeometry::Solved(SolvedSurfaceGeometry::Plane(
                    cadmpeg_ir::geometry::analytic::PlaneSurface::try_new(
                        cadmpeg_ir::math::Point3::new(2.0, 0.0, 0.0),
                        Vector3::new(1.0, 0.0, 0.0),
                        Vector3::new(0.0, 0.0, 1.0),
                    )
                    .unwrap(),
                )),
                source_object: None,
            },
            Surface {
                id: second_spine_side.clone(),
                geometry: SurfaceGeometry::Solved(SolvedSurfaceGeometry::Plane(
                    cadmpeg_ir::geometry::analytic::PlaneSurface::try_new(
                        cadmpeg_ir::math::Point3::new(0.0, 2.0, 0.0),
                        Vector3::new(0.0, 1.0, 0.0),
                        Vector3::new(0.0, 0.0, 1.0),
                    )
                    .unwrap(),
                )),
                source_object: None,
            },
        ]);
        let spine = CurveId::mint("test:model:entity#synthetic:spine").expect("identity grammar");
        ir.model.curves.push(Curve {
            id: spine.clone(),
            geometry: CurveGeometry::Solved(SolvedCurveGeometry::Line(
                cadmpeg_ir::geometry::analytic::LineCurve::try_new(
                    cadmpeg_ir::math::Point3::new(2.0, 2.0, 0.0),
                    Vector3::new(0.0, 0.0, 1.0),
                )
                .unwrap(),
            )),
            source_object: None,
        });
        let surface =
            SurfaceId::mint("test:model:entity#synthetic:blend").expect("identity grammar");
        let construction =
            ProceduralSurfaceId::mint("test:model:entity#synthetic:blend-construction")
                .expect("identity grammar");
        ir.model.surfaces.push(Surface {
            id: surface.clone(),
            geometry: SurfaceGeometry::Procedural {
                construction: construction.clone(),
                cache: None,
            },
            source_object: None,
        });
        ir.model.procedural_surfaces.push(ProceduralSurface::new(
            construction,
            ProceduralSurfaceDefinition::Blend(
                cadmpeg_ir::geometry::surface_payloads::BlendSurfacePayload::try_new(
                    [
                        Some(BlendSupport {
                            surface: first.clone(),
                            reversed: false,
                        }),
                        Some(BlendSupport {
                            surface: second.clone(),
                            reversed: false,
                        }),
                    ],
                    Some(spine.clone()),
                    BlendRadiusLaw::constant(2.0).unwrap(),
                    BlendCrossSection::Circular,
                    cadmpeg_ir::geometry::CacheContract::from_form(None),
                )
                .unwrap(),
            ),
            None,
        ));
        let expected = Point2::new(8.0, 0.35);
        let point = blend_surface_point(geometry_ctx, &ir, &surface, expected.u, expected.v)
            .expect("evaluator allocation succeeds")
            .unwrap();
        let boundary_without_contact_chart =
            blend_surface_point(geometry_ctx, &ir, &surface, expected.u, 1.0)
                .expect("evaluator allocation succeeds")
                .expect("analytic supports provide a blend boundary without a spine pcurve");
        let boundary_without_contact_parameters = blend_surface_parameters(
            geometry_ctx,
            &ir,
            &surface,
            boundary_without_contact_chart,
            None,
        )
        .expect("evaluator allocation succeeds")
        .expect("blend inverse evaluates an analytic-support boundary");
        assert!((0.0..=1.0).contains(&boundary_without_contact_parameters.v));

        assert_eq!(
            crate::decode::support_uv::blend_spine_cache_fit_tolerance(&ir, &surface, 0.25),
            0.25
        );
        let procedural = ProceduralCurve::new(
            ProceduralCurveId::mint("test:model:entity#synthetic:spine-construction")
                .expect("identity grammar"),
            ProceduralCurveDefinition::Intersection {
                context: IntcurveSupportContext::try_new(
                    [
                        IntcurveSupportSide {
                            surface: Some(first_spine_side),
                            pcurve: Some(
                                PcurveGeometry::Line(
                                    cadmpeg_ir::geometry::pcurve::LinePcurve::try_new(
                                        Point2::new(0.0, -2.0),
                                        Point2::new(1.0, 0.0),
                                    )
                                    .unwrap(),
                                )
                                .into(),
                            ),
                        },
                        IntcurveSupportSide {
                            surface: Some(second_spine_side),
                            pcurve: Some(
                                PcurveGeometry::Line(
                                    cadmpeg_ir::geometry::pcurve::LinePcurve::try_new(
                                        Point2::new(0.0, 2.0),
                                        Point2::new(1.0, 0.0),
                                    )
                                    .unwrap(),
                                )
                                .into(),
                            ),
                        },
                    ],
                    [0.0, 10.0],
                    [Vec::new(), Vec::new(), Vec::new()],
                )
                .unwrap(),
                discontinuity_flag: false,
                cache: Some(
                    cadmpeg_ir::geometry::LegacyCache::try_new(0.75).expect("fit tolerance"),
                ),
            },
        );
        ir.model
            .add_procedural_curve(
                &cadmpeg_ir::document::admission::StandardAdmission,
                &spine,
                procedural,
            )
            .unwrap()
            .unwrap();
        assert_eq!(
            crate::decode::support_uv::blend_spine_cache_fit_tolerance(&ir, &surface, 0.25),
            1.0
        );

        let actual = blend_surface_parameters(geometry_ctx, &ir, &surface, point, None)
            .expect("evaluator allocation succeeds")
            .unwrap();

        assert!((actual.u - expected.u).abs() < 1.0e-8);
        assert!((actual.v - expected.v).abs() < 1.0e-8);

        let boundary_point = blend_surface_point(geometry_ctx, &ir, &surface, expected.u, 1.0)
            .expect("evaluator allocation succeeds")
            .unwrap();
        let boundary_parameters =
            blend_surface_parameters(geometry_ctx, &ir, &surface, boundary_point, None)
                .expect("evaluator allocation succeeds")
                .expect("blend inverse returns the section boundary");
        assert!((0.0..=1.0).contains(&boundary_parameters.v));

        let outside_boundary_point = blend_surface_point(
            geometry_ctx,
            &ir,
            &surface,
            expected.u,
            1.0 + OUTSIDE_BLEND_SECTION_DELTA,
        )
        .expect("evaluator allocation succeeds")
        .unwrap();
        let outside_parameters =
            blend_surface_parameters(geometry_ctx, &ir, &surface, outside_boundary_point, None)
                .expect("evaluator allocation succeeds");
        assert!(outside_parameters.is_none());
        let geometry_budget = crate::decode::geometry_work::GeometryWorkBudget::from_context(
            geometry_ctx,
            cadmpeg_core::decode::u64_from_index(
                crate::decode::geometry_work::MAX_ADAPTIVE_GEOMETRY_WORK,
            ),
        );
        let continuation_parameters =
        crate::decode::blend::blend_surface_parameters_for_fit_with_source_continuation_and_budget(
            &cadmpeg_ir::index::ModelIndex::build(&ir, cadmpeg_ir::index::StandardIndex),
            &surface,
            outside_boundary_point,
            None,
            1.0e-8,
            BlendParameterGrid::Disabled,
            &geometry_budget,
        )
        .expect("evaluator allocation succeeds")
        .expect("bounded source continuation admits the certified section point");
        assert!((continuation_parameters.u - expected.u).abs() < 1.0e-8);
        assert!((continuation_parameters.v - (1.0 + OUTSIDE_BLEND_SECTION_DELTA)).abs() < 1.0e-8);

        let direct_geometry_budget = crate::decode::geometry_work::GeometryWorkBudget::from_context(
            geometry_ctx,
            cadmpeg_core::decode::u64_from_index(
                crate::decode::geometry_work::MAX_ADAPTIVE_GEOMETRY_WORK,
            ),
        );
        let mut direct_contact_seeds = crate::decode::blend::BlendContactSeedCache::default();
        let direct_parameters =
            crate::decode::blend::blend_surface_parameters_from_point_with_index_and_budget(
                &cadmpeg_ir::index::ModelIndex::build(&ir, cadmpeg_ir::index::StandardIndex),
                &surface,
                outside_boundary_point,
                None,
                DIRECT_INVERSE_TOLERANCE,
                &mut direct_contact_seeds,
                &direct_geometry_budget,
            )
            .expect("evaluator allocation succeeds")
            .expect("direct blend inverse admits a certified continuation point");
        assert!((direct_parameters.u - expected.u).abs() < DIRECT_INVERSE_TOLERANCE);
        assert!(
            (direct_parameters.v - (1.0 + OUTSIDE_BLEND_SECTION_DELTA)).abs()
                < DIRECT_INVERSE_TOLERANCE
        );

        let continued = blend_surface_parameters_for_fit(
            geometry_ctx,
            &ir,
            &surface,
            point,
            Some(Point2::new(expected.u + 0.1, expected.v - 0.05)),
            1.0e-8,
        )
        .expect("evaluator allocation succeeds")
        .unwrap();
        assert!((continued.u - expected.u).abs() < 1.0e-8);
        assert!((continued.v - expected.v).abs() < 1.0e-8);

        let mut varying_frame = ir.clone();
        let carrier = varying_frame
            .model
            .curves
            .iter_mut()
            .find(|curve| curve.id == spine)
            .unwrap();
        let CurveGeometry::Procedural { cache, .. } = &mut carrier.geometry else {
            panic!("procedural spine carrier");
        };
        *cache = Some(
            cadmpeg_ir::geometry::CurveGeometry::Solved(SolvedCurveGeometry::Parabola(
                cadmpeg_ir::geometry::analytic::ParabolaCurve::try_new(
                    cadmpeg_ir::math::Point3::new(2.0, 2.0, 0.0),
                    Vector3::new(0.0, 1.0, 0.0),
                    Vector3::new(1.0, 0.0, 0.0),
                    0.5,
                )
                .unwrap(),
            ))
            .solved()
            .expect("solved carrier")
            .clone(),
        );
        varying_frame
            .model
            .procedural_curves
            .iter_mut()
            .find(|curve| {
                curve.id
                    == ProceduralCurveId::mint("test:model:entity#synthetic:spine-construction")
                        .expect("identity grammar")
            })
            .unwrap()
            .edit_definition(|definition| {
                let ProceduralCurveDefinition::Intersection { context, .. } = definition else {
                    unreachable!()
                };
                edit::replace(context, |previous| {
                    let mut sides = previous.sides().clone();
                    let range = previous.parameter_range().endpoints();
                    let discontinuities =
                        cadmpeg_ir::scalar::FiniteReal::raw_lanes(previous.discontinuities());
                    {
                        let context_sides: &mut [cadmpeg_ir::geometry::IntcurveSupportSide; 2] =
                            &mut sides;

                        (*context_sides)[0].pcurve = Some(
                            PcurveGeometry::Offset(
                                cadmpeg_ir::geometry::pcurve::OffsetPcurve::try_new(
                                    0.1,
                                    Box::new((*context_sides)[0].pcurve.take().unwrap().geometry),
                                )
                                .unwrap(),
                            )
                            .into(),
                        );
                    };
                    cadmpeg_ir::geometry::IntcurveSupportContext::try_new(
                        sides,
                        range,
                        discontinuities,
                    )
                })
                .unwrap();
            });
        let parameters = Point2::new(0.4, 0.35);
        let exact = blend_surface_u_derivative(
            geometry_ctx,
            &varying_frame,
            &surface,
            parameters.u,
            parameters.v,
            0,
        )
        .expect("evaluator allocation succeeds")
        .expect("complete rolling-ball frame has an exact derivative");
        let step = 1.0e-6;
        let before = blend_surface_point(
            geometry_ctx,
            &varying_frame,
            &surface,
            parameters.u - step,
            parameters.v,
        )
        .expect("evaluator allocation succeeds")
        .unwrap();
        let after = blend_surface_point(
            geometry_ctx,
            &varying_frame,
            &surface,
            parameters.u + step,
            parameters.v,
        )
        .expect("evaluator allocation succeeds")
        .unwrap();
        let numerical = Vector3::new(
            (after.x - before.x) / (2.0 * step),
            (after.y - before.y) / (2.0 * step),
            (after.z - before.z) / (2.0 * step),
        );
        assert!((exact.x - numerical.x).abs() < 1.0e-7);
        assert!((exact.y - numerical.y).abs() < 1.0e-7);
        assert!((exact.z - numerical.z).abs() < 1.0e-7);

        let mut translated = ir.clone();
        for carrier in &mut translated.model.surfaces {
            if let SurfaceGeometry::Solved(SolvedSurfaceGeometry::Plane(plane_surface)) =
                &mut carrier.geometry
            {
                let origin = plane_surface.origin();
                let normal = plane_surface.frame().axis().as_raw();
                let u_axis = plane_surface.frame().reference().as_raw();
                let mut origin = *origin;
                origin.x += 1.0e12;
                origin.y += 1.0e12;
                origin.z += 1.0e12;
                *plane_surface =
                    cadmpeg_ir::geometry::analytic::PlaneSurface::try_new(origin, *normal, *u_axis)
                        .unwrap();
            }
        }
        let carrier = translated
            .model
            .curves
            .iter_mut()
            .find(|curve| curve.id == spine)
            .expect("translated spine");
        let CurveGeometry::Procedural {
            cache: Some(cache), ..
        } = &mut carrier.geometry
        else {
            panic!("procedural spine cache");
        };
        let mut geometry = cache.clone();
        let SolvedCurveGeometry::Line(line_curve) = &mut geometry else {
            panic!("line spine cache");
        };
        let direction = line_curve.direction();
        let mut origin = line_curve.origin().get();
        origin.x += 1.0e12;
        origin.y += 1.0e12;
        origin.z += 1.0e12;
        let origin = cadmpeg_ir::features::FinitePoint3::new(origin).unwrap();
        *line_curve = cadmpeg_ir::geometry::analytic::LineCurve::new(origin, direction);
        *cache = geometry;
        let translated_point =
            blend_surface_point(geometry_ctx, &translated, &surface, expected.u, expected.v)
                .expect("evaluator allocation succeeds")
                .unwrap();
        let translated_parameters = blend_surface_parameters_for_fit(
            geometry_ctx,
            &translated,
            &surface,
            translated_point,
            Some(Point2::new(expected.u + 0.1, expected.v - 0.05)),
            1.0e-3,
        )
        .expect("evaluator allocation succeeds")
        .expect("exact section tangent is independent of model-space magnitude");
        assert!((translated_parameters.u - expected.u).abs() < 1.0e-3);
        assert!((translated_parameters.v - expected.v).abs() < 1.0e-3);

        let boundary_curve = CurveId::mint("test:model:entity#synthetic:blend-boundary-curve")
            .expect("identity grammar");
        ir.model.curves.push(Curve {
            id: boundary_curve.clone(),
            geometry: CurveGeometry::Solved(SolvedCurveGeometry::Unknown { record: None }),
            source_object: None,
        });
        let _attached = ir.model.add_procedural_curve(
            &cadmpeg_ir::document::admission::StandardAdmission,
            &boundary_curve,
            ProceduralCurve::new(
                ProceduralCurveId::mint("test:model:entity#synthetic:blend-boundary")
                    .expect("identity grammar"),
                ProceduralCurveDefinition::Intersection {
                    context: IntcurveSupportContext::try_new(
                        [
                            IntcurveSupportSide {
                                surface: Some(first.clone()),
                                pcurve: Some(
                                    PcurveGeometry::Line(
                                        cadmpeg_ir::geometry::pcurve::LinePcurve::try_new(
                                            Point2::new(0.0, -2.0),
                                            Point2::new(1.0, 0.0),
                                        )
                                        .unwrap(),
                                    )
                                    .into(),
                                ),
                            },
                            IntcurveSupportSide {
                                surface: Some(surface.clone()),
                                pcurve: None,
                            },
                        ],
                        [0.0, 1.0],
                        [Vec::new(), Vec::new(), Vec::new()],
                    )
                    .unwrap(),
                    discontinuity_flag: false,
                    cache: None,
                },
            ),
        );
        ir.model.edges.push(Edge {
            id: EdgeId::mint("test:model:entity#synthetic:blend-boundary-edge")
                .expect("identity grammar"),
            carrier: cadmpeg_ir::topology::EdgeCarrier::new(Some(boundary_curve), Some([0.0, 1.0]))
                .unwrap(),
            start: VertexId::mint("test:model:entity#synthetic:blend-boundary-start")
                .expect("identity grammar"),
            end: VertexId::mint("test:model:entity#synthetic:blend-boundary-end")
                .expect("identity grammar"),
            tolerance: Some(
                cadmpeg_ir::scalar::PositiveReal::new(EPS_TOPOLOGY_TOLERANCE)
                    .expect("positive finite tolerance"),
            ),
        });
        crate::decode::pcurves::complete_intersection_pcurves_from_opposite_charts(
            geometry_ctx,
            &mut ir,
        )
        .unwrap();
        let ProceduralCurveDefinition::Intersection { context, .. } =
            ir.model.procedural_curves.last().unwrap().definition()
        else {
            unreachable!()
        };
        let PcurveGeometry::Nurbs { nurbs } = &context.sides()[1].pcurve.as_ref().unwrap().geometry
        else {
            unreachable!()
        };
        assert_eq!(
            nurbs.pole_rows().raw_points().first(),
            Some(&Point2::new(0.0, 0.0))
        );
        assert_eq!(
            nurbs.pole_rows().raw_points().last(),
            Some(&Point2::new(1.0, 0.0))
        );
        assert_eq!(
            blend_boundary_parameter_from_support_spine(
                geometry_ctx,
                &ir,
                &surface,
                &first,
                cadmpeg_ir::math::Point3::new(0.0, 2.0, 0.0),
                None,
                1.0e-8,
            ),
            Ok(Some(Point2::new(0.0, 0.0)))
        );
        ir.model
            .procedural_curves
            .iter_mut()
            .find(|procedural| {
                procedural.id
                    == ProceduralCurveId::mint("test:model:entity#synthetic:spine-construction")
                        .expect("identity grammar")
            })
            .unwrap()
            .replace_definition(ProceduralCurveDefinition::Unknown {
                native_kind: None,
                record: None,
                cache: None,
            });
        assert_eq!(
            blend_boundary_parameter_from_support_spine(
                geometry_ctx,
                &ir,
                &surface,
                &first,
                cadmpeg_ir::math::Point3::new(0.0, 2.0, 0.0),
                None,
                1.0e-8,
            ),
            Ok(Some(Point2::new(0.0, 0.0)))
        );

        let carrier = ir
            .model
            .curves
            .iter_mut()
            .find(|curve| curve.id == spine)
            .unwrap();
        let CurveGeometry::Procedural { cache, .. } = &mut carrier.geometry else {
            panic!("procedural spine carrier");
        };
        *cache = Some(
            cadmpeg_ir::geometry::CurveGeometry::Solved(SolvedCurveGeometry::Nurbs(
                cadmpeg_ir::geometry::nurbs::NurbsCurve::from_lanes(
                    &cadmpeg_test_support::service_decode_context(),
                    1,
                    vec![0.0, 0.0, 10.0, 10.0],
                    vec![
                        cadmpeg_ir::math::Point3::new(2.0, 2.0, 0.0),
                        cadmpeg_ir::math::Point3::new(2.0, 2.0, 10.0),
                    ],
                    None,
                    false,
                )
                .expect("fixture constructor admission")
                .unwrap(),
            ))
            .solved()
            .expect("solved carrier")
            .clone(),
        );
        let coarse = coarse_blend_surface_parameters(geometry_ctx, &ir, &surface, point, 0)
            .expect("evaluator allocation succeeds")
            .unwrap();
        let coarse_point = blend_surface_point(geometry_ctx, &ir, &surface, coarse.u, coarse.v)
            .expect("evaluator allocation succeeds")
            .unwrap();
        assert!(
            ((coarse_point.x - point.x).powi(2)
                + (coarse_point.y - point.y).powi(2)
                + (coarse_point.z - point.z).powi(2))
            .sqrt()
                < 1.0
        );

        let refined = refine_blend_surface_parameters(
            geometry_ctx,
            &ir,
            &surface,
            point,
            Point2::new(expected.u + 0.5, expected.v + 0.1),
            0,
        )
        .expect("evaluator allocation succeeds")
        .unwrap();
        let refined_point = blend_surface_point(geometry_ctx, &ir, &surface, refined.u, refined.v)
            .expect("evaluator allocation succeeds")
            .unwrap();
        let refined_error = ((refined_point.x - point.x).powi(2)
            + (refined_point.y - point.y).powi(2)
            + (refined_point.z - point.z).powi(2))
        .sqrt();
        assert!(refined_error < 1.0e-9);

        let third =
            SurfaceId::mint("test:model:entity#synthetic:third-plane").expect("identity grammar");
        ir.model.surfaces.push(Surface {
            id: third.clone(),
            geometry: SurfaceGeometry::Solved(SolvedSurfaceGeometry::Plane(
                cadmpeg_ir::geometry::analytic::PlaneSurface::try_new(
                    cadmpeg_ir::math::Point3::new(0.0, 8.0, 0.0),
                    Vector3::new(0.0, 1.0, 0.0),
                    Vector3::new(0.0, 0.0, 1.0),
                )
                .unwrap(),
            )),
            source_object: None,
        });
        let outer_spine =
            CurveId::mint("test:model:entity#synthetic:outer-spine").expect("identity grammar");
        ir.model.curves.push(Curve {
            id: outer_spine.clone(),
            geometry: CurveGeometry::Solved(SolvedCurveGeometry::Line(
                cadmpeg_ir::geometry::analytic::LineCurve::try_new(
                    cadmpeg_ir::math::Point3::new(4.0, 6.0, 0.0),
                    Vector3::new(0.0, 0.0, 1.0),
                )
                .unwrap(),
            )),
            source_object: None,
        });
        let outer =
            SurfaceId::mint("test:model:entity#synthetic:outer-blend").expect("identity grammar");
        let outer_construction =
            ProceduralSurfaceId::mint("test:model:entity#synthetic:outer-blend-construction")
                .expect("identity grammar");
        ir.model.surfaces.push(Surface {
            id: outer.clone(),
            geometry: SurfaceGeometry::Procedural {
                construction: outer_construction.clone(),
                cache: None,
            },
            source_object: None,
        });
        ir.model.procedural_surfaces.push(ProceduralSurface::new(
            outer_construction,
            ProceduralSurfaceDefinition::Blend(
                cadmpeg_ir::geometry::surface_payloads::BlendSurfacePayload::try_new(
                    [
                        Some(BlendSupport {
                            surface,
                            reversed: false,
                        }),
                        Some(BlendSupport {
                            surface: third,
                            reversed: false,
                        }),
                    ],
                    Some(outer_spine),
                    BlendRadiusLaw::constant(1.5).unwrap(),
                    BlendCrossSection::Circular,
                    cadmpeg_ir::geometry::CacheContract::from_form(None),
                )
                .unwrap(),
            ),
            None,
        ));
        let expected = Point2::new(4.0, 0.2);
        let point = blend_surface_point(geometry_ctx, &ir, &outer, expected.u, expected.v)
            .expect("evaluator allocation succeeds")
            .unwrap();
        let outer_geometry = ir
            .model
            .surfaces
            .iter()
            .find(|candidate| candidate.id == outer)
            .map(|surface| &surface.geometry)
            .unwrap();
        let index = cadmpeg_ir::index::ModelIndex::build(&ir, cadmpeg_ir::index::StandardIndex);
        let geometry_budget = crate::decode::geometry_work::GeometryWorkBudget::from_context(
            geometry_ctx,
            cadmpeg_core::decode::u64_from_index(
                crate::decode::geometry_work::MAX_ADAPTIVE_GEOMETRY_WORK,
            ),
        );
        let evaluated = crate::decode::blend::decoded_surface_point_with_geometry_and_budget(
            &index,
            &outer,
            outer_geometry,
            expected.u,
            expected.v,
            0,
            &geometry_budget,
        )
        .expect("evaluator allocation succeeds")
        .expect("budgeted evaluation handles a nested blend support");
        assert!(Point3::distance(evaluated, point) <= 64.0 * f64::EPSILON);
        let actual = blend_surface_parameters(geometry_ctx, &ir, &outer, point, None)
            .expect("evaluator allocation succeeds")
            .unwrap();
        assert!((actual.u - expected.u).abs() < 1.0e-8);
        assert!((actual.v - expected.v).abs() < 1.0e-8);

        let outer_definition = ir
            .model
            .procedural_surfaces
            .iter_mut()
            .find(|candidate| {
                candidate.id
                    == ProceduralSurfaceId::mint(
                        "test:model:entity#synthetic:outer-blend-construction",
                    )
                    .expect("identity grammar")
            })
            .unwrap();
        outer_definition.edit_definition(|definition| {
            let ProceduralSurfaceDefinition::Blend(definition_payload) = definition else {
                panic!("blend definition");
            };
            let mut edited_supports = definition_payload.supports().clone();
            let supports = &mut edited_supports;

            supports[0].as_mut().unwrap().surface = outer.clone();
            *definition_payload =
                cadmpeg_ir::geometry::surface_payloads::BlendSurfacePayload::try_new(
                    edited_supports,
                    definition_payload.spine().clone(),
                    definition_payload.radius().clone(),
                    definition_payload.cross_section().clone(),
                    cadmpeg_ir::geometry::CacheContract::from_form(
                        definition_payload
                            .native()
                            .map(cadmpeg_ir::geometry::RollingBallConstruction::to_raw)
                            .map(Box::new),
                    ),
                )
                .unwrap();
        });
        assert!(
            blend_surface_point(geometry_ctx, &ir, &outer, expected.u, expected.v)
                .expect("evaluator allocation succeeds")
                .is_none()
        );
    });
}
