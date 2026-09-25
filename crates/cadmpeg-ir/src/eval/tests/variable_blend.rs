// SPDX-License-Identifier: Apache-2.0

use crate::eval::constant_rolling_ball_first_order;
use crate::eval::model_surface_partials_by_id;
use crate::eval::model_surface_point;
use crate::eval::model_surface_point_by_id;
use crate::eval::tests::bilinear_surface;
use crate::eval::tests::contact_track;
use crate::eval::variable_blend_is_zero_radius;
use crate::eval::variable_blend_radius;
use crate::eval::ConstantRollingBallSection;
use crate::geometry::pcurve::PcurveGeometry;
use crate::geometry::sampled::PolylineSamples;
use crate::geometry::sampled::PolylineVertex;
use crate::geometry::BlendCrossSection;
use crate::geometry::BlendRadiusLaw;
use crate::geometry::BlendSupport;
use crate::geometry::Curve;
use crate::geometry::CurveGeometry;
use crate::geometry::ProceduralSurface;
use crate::geometry::ProceduralSurfaceDefinition;
use crate::geometry::RevisionSurfaceParameterization;
use crate::geometry::RollingBallConstruction;
use crate::geometry::RollingBallRadiusSelector;
use crate::geometry::RollingBallSide;
use crate::geometry::SolvedCurveGeometry;
use crate::geometry::SolvedSurfaceGeometry;
use crate::geometry::Surface;
use crate::geometry::SurfaceGeometry;
use crate::geometry::VariableBlendConstruction;
use crate::geometry::VariableBlendConvexity;
use crate::geometry::VariableBlendCrossSection;
use crate::geometry::VariableBlendRadii;
use crate::geometry::VariableBlendRenderMode;
use crate::geometry::VariableBlendSupportKind;
use crate::geometry::VariableBlendSurfaceSubtype;
use crate::geometry::VariableBlendValue;
use crate::geometry::VariableBlendValuePayload;
use crate::ids::CurveId;
use crate::ids::ProceduralSurfaceId;
use crate::ids::SurfaceId;
use crate::math::Point2;
use crate::math::Point3;
use crate::math::Vector3;
use crate::scalar::FiniteReal;
use crate::transform::Transform;
use crate::CadIr;

fn variable_blend_eval_fixture(
    second_origin: Point3,
    pcurves: [(Point2, Point2); 2],
    radii: [f64; 2],
    cross_section: Option<VariableBlendCrossSection>,
) -> (CadIr, SurfaceId) {
    let first_surface = SurfaceId::mint("test:model:entity#first-support").expect("valid identity");
    let second_surface =
        SurfaceId::mint("test:model:entity#second-support").expect("valid identity");
    let blend_surface =
        SurfaceId::mint("test:model:entity#cacheless-variable-blend").expect("valid identity");
    let slice = CurveId::mint("test:model:entity#blend-slice").expect("valid identity");
    let mut ir = CadIr::empty();
    ir.model.curves.push(Curve {
        id: slice.clone(),
        geometry: CurveGeometry::Solved(SolvedCurveGeometry::Line(
            crate::geometry::analytic::LineCurve::try_new(
                Point3::new(0.0, 0.0, 0.0),
                Vector3::new(0.0, 1.0, 0.0),
            )
            .unwrap(),
        )),
        source_object: None,
    });
    ir.model.surfaces.extend([
        Surface {
            id: first_surface.clone(),
            geometry: SurfaceGeometry::Solved(SolvedSurfaceGeometry::Plane(
                crate::geometry::analytic::PlaneSurface::try_new(
                    Point3::new(0.0, 0.0, 0.0),
                    Vector3::new(0.0, 0.0, 1.0),
                    Vector3::new(1.0, 0.0, 0.0),
                )
                .unwrap(),
            )),
            source_object: None,
        },
        Surface {
            id: second_surface.clone(),
            geometry: SurfaceGeometry::Solved(SolvedSurfaceGeometry::Plane(
                crate::geometry::analytic::PlaneSurface::try_new(
                    second_origin,
                    Vector3::new(1.0, 0.0, 0.0),
                    Vector3::new(0.0, 1.0, 0.0),
                )
                .unwrap(),
            )),
            source_object: None,
        },
        Surface {
            id: blend_surface.clone(),
            geometry: SurfaceGeometry::Procedural {
                construction: ProceduralSurfaceId::mint(
                    "test:model:entity#variable-blend-construction",
                )
                .expect("valid identity"),
                cache: None,
            },
            source_object: None,
        },
    ]);
    let side = |surface, origin: Point2, direction: Point2| RollingBallSide {
        support_kind: VariableBlendSupportKind::Surface,
        surface: Some(crate::geometry::RollingBallSupportSurface {
            surface,
            parameter_ranges: [[None, None], [None, None]],
        }),
        curve: None,
        pcurve: Some(PcurveGeometry::Line(
            crate::geometry::pcurve::LinePcurve::try_new(origin, direction).unwrap(),
        )),
        location: Point3::new(0.0, 0.0, 0.0),
        secondary_pcurve: None,
        extension: None,
    };
    let radius = VariableBlendValue {
        modern_flag: false,
        calibrated: 0,
        payload: VariableBlendValuePayload::TwoEnds {
            discriminator: 0,
            parameters: [0.0, 1.0],
            radii,
        },
    };
    ir.model.procedural_surfaces.push(procedural_surface! {
        id: ProceduralSurfaceId::mint("test:model:entity#variable-blend-construction").expect("valid identity"),
        definition: ProceduralSurfaceDefinition::VariableBlend(crate::geometry::surface_payloads::VariableBlendSurfacePayload::try_new(Box::new(VariableBlendConstruction {
                subtype: VariableBlendSurfaceSubtype::VariableBlend,
                revision: crate::scalar::PositiveI64::new(23100).expect("positive revision"),
                sides: [
                    side(first_surface, pcurves[0].0, pcurves[0].1),
                    side(second_surface, pcurves[1].0, pcurves[1].1),
                ],
                slice,
                slice_range: [Some(0.0), Some(1.0)],
                offsets: [0.0, 0.0],
                radii: VariableBlendRadii::Single { value: radius },
                cross_section,
                u_range: [0.0, 1.0],
                v_lower: Some(0.0),
                shape_parameter: 0.0,
                shape_length: 0.0,
                shape_tail: 0,
                cache: crate::geometry::VariableBlendCache::Parameterization {
                    shape_prefix: 1,
                    parameterization: RevisionSurfaceParameterization {
                        u_interval: [Some(0.0), Some(1.0)],
                        v_interval: [Some(0.0), Some(1.0)],
                        ..Default::default()
                    },
                },
                discontinuities: std::array::from_fn(|_| Vec::new()),
                tail_flag: false,
                tail_extensions: [0; 3],
                secondary_curve: None,
                convexity: VariableBlendConvexity::Convex,
                render_mode: VariableBlendRenderMode::RollingBallEnvelope,
                post_range: [None, None],
                post_curve: None,
                post_pcurve: None,
            })).unwrap()),
        cache_fit_tolerance: None,
        record_bounds: None,
    });
    (ir, blend_surface)
}

#[test]
fn cacheless_zero_radius_rounded_chamfer_is_ruled_between_contact_tracks() {
    let (mut ir, blend_surface) = variable_blend_eval_fixture(
        Point3::new(10.0, 0.0, 0.0),
        [
            (Point2::new(1.0, 2.0), Point2::new(2.0, 3.0)),
            (Point2::new(4.0, 5.0), Point2::new(6.0, 7.0)),
        ],
        [2.0, 2.0],
        Some(VariableBlendCrossSection::RoundedChamfer { radius: None }),
    );

    let index = crate::index::ModelIndex::new(&ir);
    assert_eq!(
        model_surface_point_by_id(&index, &blend_surface, 0.0, 0.5)
            .map(crate::features::FinitePoint3::get),
        Ok(Point3::new(2.0, 3.5, 0.0))
    );
    assert_eq!(
        model_surface_point_by_id(&index, &blend_surface, 1.0, 0.5)
            .map(crate::features::FinitePoint3::get),
        Ok(Point3::new(10.0, 7.0, 8.5))
    );
    assert_eq!(
        model_surface_point(&ir, &ir.model.surfaces[2].geometry, 0.25, 0.5)
            .map(crate::features::FinitePoint3::get),
        Ok(Point3::new(4.0, 4.375, 2.125))
    );
    let partials = model_surface_partials_by_id(&index, &blend_surface, 0.25, 0.5)
        .expect("cacheless ruled variable blend");
    assert_eq!(partials.point, Point3::new(4.0, 4.375, 2.125));
    assert_eq!(partials.du, Vector3::new(8.0, 3.5, 8.5));
    assert_eq!(partials.dv, Vector3::new(1.5, 3.75, 1.75));

    ir.model.procedural_surfaces[0].edit_definition(|definition| {
        let ProceduralSurfaceDefinition::VariableBlend(definition_payload) = definition else {
            unreachable!()
        };
        let mut construction = definition_payload.construction().to_raw();

        let first = construction.radii.first().clone();
        construction.radii = VariableBlendRadii::Two {
            first: first.clone(),
            second: first,
        };
        *definition_payload =
            crate::geometry::surface_payloads::VariableBlendSurfacePayload::try_new(Box::new(
                construction,
            ))
            .unwrap();
    });
    let index = crate::index::ModelIndex::new(&ir);
    assert_eq!(
        model_surface_point_by_id(&index, &blend_surface, 0.25, 0.5)
            .map(crate::features::FinitePoint3::get),
        Ok(Point3::new(4.0, 4.375, 2.125))
    );

    let zero_radius = VariableBlendValue {
        modern_flag: false,
        calibrated: 0,
        payload: VariableBlendValuePayload::TwoEnds {
            discriminator: 0,
            parameters: [0.0, 1.0],
            radii: [0.0, 0.0],
        },
    };
    ir.model.procedural_surfaces[0].edit_definition(|definition| {
        let ProceduralSurfaceDefinition::VariableBlend(definition_payload) = definition else {
            unreachable!()
        };
        let mut construction = definition_payload.construction().to_raw();

        construction.cross_section = Some(VariableBlendCrossSection::RoundedChamfer {
            radius: Some(Box::new(zero_radius.clone())),
        });
        *definition_payload =
            crate::geometry::surface_payloads::VariableBlendSurfacePayload::try_new(Box::new(
                construction,
            ))
            .unwrap();
    });
    let index = crate::index::ModelIndex::new(&ir);
    assert_eq!(
        model_surface_point_by_id(&index, &blend_surface, 0.25, 0.5)
            .map(crate::features::FinitePoint3::get),
        Ok(Point3::new(4.0, 4.375, 2.125))
    );
    assert!(variable_blend_is_zero_radius(
        &VariableBlendValue {
            modern_flag: false,
            calibrated: 0,
            payload: VariableBlendValuePayload::Constant {
                discriminator: 0,
                parameters: [0.0, 0.0],
                radius: 0.0,
                variable_chamfer: 0,
                chamfer_type: 0,
                nested: Box::new(zero_radius),
            },
        }
        .admit()
        .expect("finite value")
    ));
}

#[test]
fn current_variable_blend_uses_the_solved_cache_for_points_and_partials() {
    let (mut ir, blend_surface) = variable_blend_eval_fixture(
        Point3::new(10.0, 0.0, 0.0),
        [
            (Point2::new(1.0, 2.0), Point2::new(2.0, 3.0)),
            (Point2::new(4.0, 5.0), Point2::new(6.0, 7.0)),
        ],
        [2.0, 2.0],
        Some(VariableBlendCrossSection::G2Round {
            parameters: [1.0, 1.0],
        }),
    );
    let SurfaceGeometry::Procedural { cache, .. } = &mut ir.model.surfaces[2].geometry else {
        panic!("fixture surface must retain its construction");
    };
    *cache = Some(SolvedSurfaceGeometry::Nurbs(bilinear_surface()));
    ir.model.procedural_surfaces[0].edit_definition(|definition| {
        let ProceduralSurfaceDefinition::VariableBlend(definition_payload) = definition else {
            unreachable!()
        };
        let mut construction = definition_payload.construction().to_raw();

        construction.cache = crate::geometry::VariableBlendCache::Current {
            shape_prefix: std::num::NonZeroI64::new(1).unwrap(),
            fit_tolerance: crate::geometry::FitTolerance::try_new(0.0).unwrap(),
        };
        *definition_payload =
            crate::geometry::surface_payloads::VariableBlendSurfacePayload::try_new(Box::new(
                construction,
            ))
            .unwrap();
    });

    let index = crate::index::ModelIndex::new(&ir);
    assert_eq!(
        model_surface_point_by_id(&index, &blend_surface, 0.25, 0.5)
            .map(crate::features::FinitePoint3::get),
        Ok(Point3::new(0.25, 0.5, 0.0))
    );
    let partials = model_surface_partials_by_id(&index, &blend_surface, 0.25, 0.5)
        .expect("current variable blend cache partials");
    assert_eq!(partials.point, Point3::new(0.25, 0.5, 0.0));
    assert_eq!(partials.du, Vector3::new(1.0, 0.0, 0.0));
    assert_eq!(partials.dv, Vector3::new(0.0, 1.0, 0.0));

    ir.model.procedural_surfaces[0].edit_definition(|definition| {
        let ProceduralSurfaceDefinition::VariableBlend(definition_payload) = definition else {
            unreachable!()
        };
        let mut construction = definition_payload.construction().to_raw();

        construction.cache = crate::geometry::VariableBlendCache::Stale {};
        *definition_payload =
            crate::geometry::surface_payloads::VariableBlendSurfacePayload::try_new(Box::new(
                construction,
            ))
            .unwrap();
    });
    let index = crate::index::ModelIndex::new(&ir);
    assert_eq!(
        model_surface_point_by_id(&index, &blend_surface, 0.25, 0.5),
        Err(crate::eval::EvaluationFailure::NoValue)
    );
    assert!(model_surface_partials_by_id(&index, &blend_surface, 0.25, 0.5).is_err());
}

#[test]
fn cacheless_circular_variable_blend_uses_the_common_contact_center() {
    let (ir, blend_surface) = variable_blend_eval_fixture(
        Point3::new(0.0, 0.0, 0.0),
        [
            (Point2::new(2.0, 0.0), Point2::new(2.0, 1.0)),
            (Point2::new(0.0, 2.0), Point2::new(1.0, 2.0)),
        ],
        [2.0, 4.0],
        Some(VariableBlendCrossSection::Circular {}),
    );
    let index = crate::index::ModelIndex::new(&ir);
    assert_eq!(
        model_surface_point_by_id(&index, &blend_surface, 0.0, 0.5)
            .map(crate::features::FinitePoint3::get),
        Ok(Point3::new(3.0, 0.5, 0.0))
    );
    assert_eq!(
        model_surface_point_by_id(&index, &blend_surface, 1.0, 0.5)
            .map(crate::features::FinitePoint3::get),
        Ok(Point3::new(0.0, 0.5, 3.0))
    );
    let point = model_surface_point_by_id(&index, &blend_surface, 0.5, 0.5)
        .expect("cacheless circular variable blend");
    let expected = 3.0 - 3.0 / 2.0_f64.sqrt();
    let tolerance = 64.0 * f64::EPSILON;
    assert!((point.x - expected).abs() <= tolerance);
    assert!((point.y - 0.5).abs() <= tolerance);
    assert!((point.z - expected).abs() <= tolerance);

    let partials = model_surface_partials_by_id(&index, &blend_surface, 0.5, 0.5)
        .expect("cacheless circular variable-blend partials");
    let derivative = 3.0 * std::f64::consts::FRAC_PI_2 / 2.0_f64.sqrt();
    let derivative_tolerance = 128.0 * f64::EPSILON;
    assert!((partials.point.x - expected).abs() <= derivative_tolerance);
    assert!((partials.point.y - 0.5).abs() <= derivative_tolerance);
    assert!((partials.point.z - expected).abs() <= derivative_tolerance);
    assert!((partials.du.x + derivative).abs() <= derivative_tolerance);
    assert!(partials.du.y.abs() <= derivative_tolerance);
    assert!((partials.du.z - derivative).abs() <= derivative_tolerance);
    let transverse = 2.0_f64.sqrt();
    assert!((partials.dv.x - (2.0 - transverse)).abs() <= derivative_tolerance);
    assert!((partials.dv.y - 1.0).abs() <= derivative_tolerance);
    assert!((partials.dv.z - (2.0 - transverse)).abs() <= derivative_tolerance);
}

#[test]
fn cacheless_circular_variable_blend_rejects_an_undetermined_center_tangent() {
    let (mut ir, blend_surface) = variable_blend_eval_fixture(
        Point3::new(0.0, 0.0, 0.0),
        [
            (Point2::new(2.0, 0.0), Point2::new(2.0, 1.0)),
            (Point2::new(0.0, 2.0), Point2::new(1.0, 2.0)),
        ],
        [2.0, 4.0],
        Some(VariableBlendCrossSection::Circular {}),
    );
    ir.model.procedural_surfaces[0].edit_definition(|definition| {
        let ProceduralSurfaceDefinition::VariableBlend(definition_payload) = definition else {
            unreachable!()
        };
        let mut construction = definition_payload.construction().to_raw();

        construction.sides[0].pcurve = Some(crate::geometry::pcurve::PcurveGeometry::Line(
            crate::geometry::pcurve::LinePcurve::try_new(
                Point2::new(3.0, 0.0),
                Point2::new(0.0, 1.0),
            )
            .unwrap(),
        ));
        construction.sides[1].pcurve = Some(crate::geometry::pcurve::PcurveGeometry::Line(
            crate::geometry::pcurve::LinePcurve::try_new(
                Point2::new(0.5, 2.0),
                Point2::new(0.0, 2.0),
            )
            .unwrap(),
        ));
        *definition_payload =
            crate::geometry::surface_payloads::VariableBlendSurfacePayload::try_new(Box::new(
                construction,
            ))
            .unwrap();
    });

    let index = crate::index::ModelIndex::new(&ir);
    let point = model_surface_point_by_id(&index, &blend_surface, 0.5, 0.5)
        .expect("contact centers still coincide at the sample");
    let expected = 3.0 - 3.0 / 2.0_f64.sqrt();
    let tolerance = 64.0 * f64::EPSILON;
    assert!((point.x - expected).abs() <= tolerance);
    assert!((point.y - 0.5).abs() <= tolerance);
    assert!((point.z - expected).abs() <= tolerance);
    assert!(model_surface_partials_by_id(&index, &blend_surface, 0.5, 0.5).is_err());
}

/// The constant rolling ball of radius 3 between the planes `z = 0` and
/// `x = 0`, with `slice` as its section-center curve.
fn constant_rolling_ball_fixture(slice: CurveGeometry) -> (CadIr, SurfaceId) {
    let (mut ir, blend_surface) = variable_blend_eval_fixture(
        Point3::new(0.0, 0.0, 0.0),
        [
            (Point2::new(3.0, 0.0), Point2::new(0.0, 1.0)),
            (Point2::new(0.0, 3.0), Point2::new(1.0, 0.0)),
        ],
        [3.0, 3.0],
        Some(VariableBlendCrossSection::Circular {}),
    );
    ir.model.curves[0].geometry = slice;
    let ProceduralSurfaceDefinition::VariableBlend(definition_payload) =
        ir.model.procedural_surfaces[0].definition()
    else {
        unreachable!()
    };
    let construction = definition_payload.construction().to_raw();

    let sides = construction.sides.clone();
    let slice = construction.slice.clone();
    let supports = sides.each_ref().map(|side| {
        side.surface.as_ref().map(|surface| BlendSupport {
            surface: surface.surface.clone(),
            reversed: false,
        })
    });
    ir.model.procedural_surfaces[0].edit_definition(|definition| {
        *definition = ProceduralSurfaceDefinition::Blend(
            crate::geometry::surface_payloads::BlendSurfacePayload::try_new(
                supports,
                Some(slice.clone()),
                BlendRadiusLaw::constant(3.0).unwrap(),
                BlendCrossSection::Circular,
                crate::geometry::CacheContract::from_form(Some(Box::new(
                    RollingBallConstruction {
                        revision: crate::scalar::PositiveI64::new(23100)
                            .expect("positive revision"),
                        sides,
                        slice,
                        slice_range: [Some(0.0), Some(1.0)],
                        offsets: [3.0, 3.0],
                        radius_selector: RollingBallRadiusSelector::None {},
                        u_range: [Some(0.0), Some(1.0)],
                        v_range: [Some(0.0), Some(1.0)],
                        shape_prefix: 0,
                        parameters: [0.0, 0.0],
                        tail: 0,
                        cache: crate::geometry::RevisionCacheForm::Parameterization(
                            RevisionSurfaceParameterization {
                                u_interval: [Some(0.0), Some(1.0)],
                                v_interval: [Some(0.0), Some(1.0)],
                                ..Default::default()
                            },
                        ),
                        discontinuities: std::array::from_fn(|_| Vec::new()),
                        tail_flag: false,
                        third: None,
                        tail_extensions: [0; 3],
                    },
                ))),
            )
            .unwrap(),
        );
    });
    (ir, blend_surface)
}

#[test]
fn cacheless_constant_rolling_ball_uses_its_spine_as_section_center() {
    let (mut ir, blend_surface) =
        constant_rolling_ball_fixture(CurveGeometry::Solved(SolvedCurveGeometry::Line(
            crate::geometry::analytic::LineCurve::try_new(
                Point3::new(3.0, 0.0, 3.0),
                Vector3::new(0.0, 1.0, 0.0),
            )
            .unwrap(),
        )));

    let index = crate::index::ModelIndex::new(&ir);
    assert_eq!(
        model_surface_point_by_id(&index, &blend_surface, 0.0, 0.5)
            .map(crate::features::FinitePoint3::get),
        Ok(Point3::new(3.0, 0.5, 0.0))
    );
    assert_eq!(
        model_surface_point_by_id(&index, &blend_surface, 1.0, 0.5)
            .map(crate::features::FinitePoint3::get),
        Ok(Point3::new(0.0, 0.5, 3.0))
    );
    let point = model_surface_point_by_id(&index, &blend_surface, 0.5, 0.5)
        .expect("cacheless constant rolling-ball blend");
    let expected = 3.0 - 3.0 / 2.0_f64.sqrt();
    let tolerance = 64.0 * f64::EPSILON;
    assert!((point.x - expected).abs() <= tolerance);
    assert!((point.y - 0.5).abs() <= tolerance);
    assert!((point.z - expected).abs() <= tolerance);

    let partials = model_surface_partials_by_id(&index, &blend_surface, 0.5, 0.5)
        .expect("cacheless rolling-ball partials");
    let derivative = 3.0 * std::f64::consts::FRAC_PI_2 / 2.0_f64.sqrt();
    assert!((partials.point.x - expected).abs() <= tolerance);
    assert!((partials.point.y - 0.5).abs() <= tolerance);
    assert!((partials.point.z - expected).abs() <= tolerance);
    assert!((partials.du.x + derivative).abs() <= tolerance);
    assert!(partials.du.y.abs() <= tolerance);
    assert!((partials.du.z - derivative).abs() <= tolerance);
    assert!(partials.dv.x.abs() <= tolerance);
    assert!((partials.dv.y - 1.0).abs() <= tolerance);
    assert!(partials.dv.z.abs() <= tolerance);

    let replica_surface = SurfaceId::mint("test:model:entity#cacheless-rolling-ball-replica")
        .expect("valid identity");
    let replica_construction =
        ProceduralSurfaceId::mint("test:model:entity#cacheless-rolling-ball-replica-def")
            .expect("valid identity");
    ir.model.surfaces.push(Surface {
        id: replica_surface.clone(),
        geometry: SurfaceGeometry::Procedural {
            construction: replica_construction.clone(),
            cache: None,
        },
        source_object: None,
    });
    ir.model.procedural_surfaces.push(procedural_surface! {
        id: replica_construction,
        definition: ProceduralSurfaceDefinition::Replica {
            source: blend_surface.clone(),
            transform: Transform::affine([
                    [1.0, 0.0, 0.0, 10.0],
                    [0.0, 1.0, 0.0, 20.0],
                    [0.0, 0.0, 1.0, 30.0]
                ]).expect("affine transform"),
        },
        cache_fit_tolerance: None,
        record_bounds: None,
    });
    let index = crate::index::ModelIndex::new(&ir);
    let replica = model_surface_point_by_id(&index, &replica_surface, 0.5, 0.5)
        .expect("rolling-ball replica");
    assert!((replica.x - (expected + 10.0)).abs() <= tolerance);
    assert!((replica.y - 20.5).abs() <= tolerance);
    assert!((replica.z - (expected + 30.0)).abs() <= tolerance);

    ir.model.curves[0].geometry = CurveGeometry::Solved(SolvedCurveGeometry::Polyline(
        crate::geometry::sampled::PolylineCurve::new(
            PolylineSamples::Parameterized {
                vertices: vec![
                    Point3::new(2.0, 0.0, 3.0),
                    Point3::new(3.0, 0.5, 3.0),
                    Point3::new(3.0, 1.0, 3.0),
                ]
                .into_iter()
                .zip(vec![0.0, 0.5, 1.0])
                .map(|(point, parameter)| PolylineVertex { parameter, point })
                .collect::<Vec<_>>()
                .try_into()
                .expect("nonempty polyline fixture"),
            },
            0.0,
        )
        .unwrap(),
    ));
    let index = crate::index::ModelIndex::new(&ir);
    assert!(model_surface_point_by_id(&index, &blend_surface, 0.5, 0.5).is_ok());
    assert!(model_surface_partials_by_id(&index, &blend_surface, 0.5, 0.5).is_err());

    ir.model.procedural_surfaces[0].edit_definition(|definition| {
        let ProceduralSurfaceDefinition::Blend(definition_payload) = definition else {
            unreachable!()
        };
        let (Some(mut native),) = (definition_payload
            .native()
            .map(RollingBallConstruction::to_raw),)
        else {
            unreachable!()
        };

        native.offsets = [4.0, 4.0];
        let restored_cache = definition_payload.legacy_cache();
        *definition_payload = crate::geometry::surface_payloads::BlendSurfacePayload::try_new(
            definition_payload.supports().clone(),
            definition_payload.spine().clone(),
            definition_payload.radius().clone(),
            definition_payload.cross_section().clone(),
            crate::geometry::CacheContract::from_form(Some(Box::new(native))),
        )
        .unwrap();
        definition
            .set_legacy_cache(restored_cache)
            .expect("a blend payload states a legacy cache slot");
    });
    let index = crate::index::ModelIndex::new(&ir);
    assert_eq!(
        model_surface_point_by_id(&index, &blend_surface, 0.5, 0.5),
        Err(crate::eval::EvaluationFailure::NoValue)
    );

    let SurfaceGeometry::Procedural { cache, .. } = &mut ir.model.surfaces[2].geometry else {
        panic!("fixture surface must retain its construction");
    };
    *cache = Some(SolvedSurfaceGeometry::Nurbs(bilinear_surface()));
    ir.model.procedural_surfaces[0].edit_definition(|definition| {
        let ProceduralSurfaceDefinition::Blend(definition_payload) = definition else {
            unreachable!()
        };
        let (Some(mut native),) = (definition_payload
            .native()
            .map(RollingBallConstruction::to_raw),)
        else {
            unreachable!()
        };

        native.offsets = [3.0, 3.0];
        native.cache = crate::geometry::RevisionCacheForm::SolvedCache {
            fit_tolerance: crate::geometry::FitTolerance::try_new(0.0).unwrap(),
        };
        let restored_cache = definition_payload.legacy_cache();
        *definition_payload = crate::geometry::surface_payloads::BlendSurfacePayload::try_new(
            definition_payload.supports().clone(),
            definition_payload.spine().clone(),
            definition_payload.radius().clone(),
            definition_payload.cross_section().clone(),
            crate::geometry::CacheContract::from_form(Some(Box::new(native))),
        )
        .unwrap();
        definition
            .set_legacy_cache(restored_cache)
            .expect("a blend payload states a legacy cache slot");
    });
    let index = crate::index::ModelIndex::new(&ir);
    assert_eq!(
        model_surface_point_by_id(&index, &blend_surface, 0.25, 0.5)
            .map(crate::features::FinitePoint3::get),
        Ok(Point3::new(0.25, 0.5, 0.0))
    );
    let cached_partials = model_surface_partials_by_id(&index, &blend_surface, 0.25, 0.5)
        .expect("current rolling-ball cache partials");
    assert_eq!(cached_partials.point, Point3::new(0.25, 0.5, 0.0));
    assert_eq!(cached_partials.du, Vector3::new(1.0, 0.0, 0.0));
    assert_eq!(cached_partials.dv, Vector3::new(0.0, 1.0, 0.0));

    ir.model.procedural_surfaces[0].edit_definition(|definition| {
        let ProceduralSurfaceDefinition::Blend(definition_payload) = definition else {
            unreachable!()
        };
        let (Some(mut native),) = (definition_payload
            .native()
            .map(RollingBallConstruction::to_raw),)
        else {
            unreachable!()
        };

        native.offsets = [4.0, 4.0];
        native.cache = crate::geometry::RevisionCacheForm::Parameterization(
            RevisionSurfaceParameterization::default(),
        );
        let restored_cache = definition_payload.legacy_cache();
        *definition_payload = crate::geometry::surface_payloads::BlendSurfacePayload::try_new(
            definition_payload.supports().clone(),
            definition_payload.spine().clone(),
            definition_payload.radius().clone(),
            definition_payload.cross_section().clone(),
            crate::geometry::CacheContract::from_form(Some(Box::new(native))),
        )
        .unwrap();
        definition
            .set_legacy_cache(restored_cache)
            .expect("a blend payload states a legacy cache slot");
    });
    let index = crate::index::ModelIndex::new(&ir);
    assert_eq!(
        model_surface_point_by_id(&index, &blend_surface, 0.25, 0.5),
        Err(crate::eval::EvaluationFailure::NoValue)
    );
    assert!(model_surface_partials_by_id(&index, &blend_surface, 0.25, 0.5).is_err());
}

/// Both surface routes of `surface` at `(u, v)` left the finite range at a
/// point with no coordinate.
fn both_routes_reach_no_coordinate(ir: &CadIr, surface: &SurfaceId, u: f64, v: f64) -> bool {
    let index = crate::index::ModelIndex::new(ir);
    let geometry = &ir
        .model
        .surfaces
        .iter()
        .find(|candidate| candidate.id == *surface)
        .expect("blend surface")
        .geometry;
    [
        model_surface_point_by_id(&index, surface, u, v),
        model_surface_point(ir, geometry, u, v),
    ]
    .into_iter()
    .all(|route| {
        matches!(
            route,
            Err(crate::eval::EvaluationFailure::NonFinite(point))
                if point.x.is_nan() && point.y.is_nan() && point.z.is_nan()
        )
    })
}

#[test]
fn a_constant_rolling_ball_whose_section_center_overflows_reaches_no_coordinate() {
    // The slice line along y through (MAX, 0, 0), placed by a transform that
    // adds MAX to x, reaches x = +inf: the section has no finite center.
    let slice = CurveGeometry::Solved(SolvedCurveGeometry::Transformed(
        crate::geometry::PlacedCurve::try_new(
            Box::new(SolvedCurveGeometry::Line(
                crate::geometry::analytic::LineCurve::try_new(
                    Point3::new(f64::MAX, 0.0, 0.0),
                    Vector3::new(0.0, 1.0, 0.0),
                )
                .unwrap(),
            )),
            Transform::affine([
                [1.0, 0.0, 0.0, f64::MAX],
                [0.0, 1.0, 0.0, 0.0],
                [0.0, 0.0, 1.0, 0.0],
            ])
            .unwrap(),
        )
        .unwrap(),
    ));
    let (ir, blend_surface) = constant_rolling_ball_fixture(slice);
    assert!(both_routes_reach_no_coordinate(
        &ir,
        &blend_surface,
        0.5,
        0.5
    ));
}

#[test]
fn a_circular_variable_blend_whose_radius_overflows_reaches_no_coordinate() {
    // The radius runs from MAX to -MAX over [0, 1]; at v = 0.5 the
    // interpolation reaches -inf.
    let (ir, blend_surface) = variable_blend_eval_fixture(
        Point3::new(0.0, 0.0, 0.0),
        [
            (Point2::new(2.0, 0.0), Point2::new(2.0, 1.0)),
            (Point2::new(0.0, 2.0), Point2::new(1.0, 2.0)),
        ],
        [f64::MAX, -f64::MAX],
        Some(VariableBlendCrossSection::Circular {}),
    );
    assert!(both_routes_reach_no_coordinate(
        &ir,
        &blend_surface,
        0.5,
        0.5
    ));
}

#[test]
fn rolling_ball_partials_follow_a_changing_section_angle() {
    let section_angle = std::f64::consts::FRAC_PI_3;
    let radius = 3.0;
    let zero = Vector3::new(0.0, 0.0, 0.0);
    let section = ConstantRollingBallSection {
        center: Point3::new(0.0, 0.0, 0.0),
        center_tangent: Ok(zero),
        first: contact_track(Point3::new(radius, 0.0, 0.0), zero),
        second: contact_track(
            Point3::new(
                radius * section_angle.cos(),
                0.0,
                radius * section_angle.sin(),
            ),
            Vector3::new(
                -radius * section_angle.sin(),
                0.0,
                radius * section_angle.cos(),
            ),
        ),
        radius,
    };
    let u = 0.4;
    let angle = u * section_angle;
    let partials = constant_rolling_ball_first_order(&section, u)
        .and_then(crate::eval::SurfaceFirstOrder::partials)
        .expect("rolling-ball partials")
        .into_raw();
    let tolerance = 128.0 * f64::EPSILON;
    assert!((partials.point.x - radius * angle.cos()).abs() <= tolerance);
    assert!(partials.point.y.abs() <= tolerance);
    assert!((partials.point.z - radius * angle.sin()).abs() <= tolerance);
    assert!((partials.du.x + radius * section_angle * angle.sin()).abs() <= tolerance);
    assert!(partials.du.y.abs() <= tolerance);
    assert!((partials.du.z - radius * section_angle * angle.cos()).abs() <= tolerance);
    assert!((partials.dv.x + radius * u * angle.sin()).abs() <= tolerance);
    assert!(partials.dv.y.abs() <= tolerance);
    assert!((partials.dv.z - radius * u * angle.cos()).abs() <= tolerance);
}

#[test]
fn variable_blend_two_ends_radius_extrapolates_its_calibration_line() {
    let value = VariableBlendValue {
        modern_flag: false,
        calibrated: 0,
        payload: VariableBlendValuePayload::TwoEnds {
            discriminator: 0,
            parameters: [2.0, 4.0],
            radii: [5.0, 9.0],
        },
    }
    .admit()
    .expect("finite value");
    assert_eq!(
        variable_blend_radius(&value, 2.0).map(FiniteReal::get),
        Ok(5.0)
    );
    assert_eq!(
        variable_blend_radius(&value, 3.0).map(FiniteReal::get),
        Ok(7.0)
    );
    assert_eq!(
        variable_blend_radius(&value, 5.0).map(FiniteReal::get),
        Ok(11.0)
    );
}

#[test]
fn variable_blend_function_uses_its_first_coordinate_as_radius() {
    let value = VariableBlendValue {
        modern_flag: false,
        calibrated: 0,
        payload: VariableBlendValuePayload::Functional {
            discriminator: 0,
            parameter: 0.0,
            radius: 0.0,
            function: PcurveGeometry::Line(
                crate::geometry::pcurve::LinePcurve::try_new(
                    Point2::new(2.0, 100.0),
                    Point2::new(3.0, 200.0),
                )
                .unwrap(),
            ),
            terminal: crate::geometry::VariableBlendTerminal::Double(0.0),
        },
    }
    .admit()
    .expect("finite value");
    assert_eq!(
        variable_blend_radius(&value, 0.5).map(FiniteReal::get),
        Ok(3.5)
    );
}

/// The plane `z = 0` over `u` in `[0, 3]` with `x = u`, `y = v`, followed by
/// a ramp of `1e293` along `x` over the next representable `u` after 3. At
/// `u = 3` the point is `(3, v, 0)` and the `u` partial overflows.
fn steep_plane_support() -> SurfaceGeometry {
    let ramp_end = f64::from_bits(3.0_f64.to_bits() + 1);
    SurfaceGeometry::Solved(SolvedSurfaceGeometry::Nurbs(
        crate::geometry::nurbs::NurbsSurface::from_lanes(
            crate::geometry::nurbs::NurbsSurfaceAxis::new(
                1,
                vec![0.0, 0.0, 3.0, ramp_end, ramp_end],
                false,
            ),
            crate::geometry::nurbs::NurbsSurfaceAxis::new(1, vec![0.0, 0.0, 1.0, 1.0], false),
            crate::geometry::nurbs::NurbsSurfaceLanes::new(
                [0.0, 3.0, 3.0 + 1.0e293]
                    .map(|x| vec![Point3::new(x, 0.0, 0.0), Point3::new(x, 1.0, 0.0)])
                    .to_vec(),
                None,
            ),
            false,
        )
        .expect("steep plane fixture"),
    ))
}

/// The contacts `(3, v, 0)` and `(0, v, 3)` of a blend between the planes
/// `z = 0` and `x = 0`, with `cross_section`, whose first support is
/// [`steep_plane_support`].
fn steep_support_blend_fixture(cross_section: VariableBlendCrossSection) -> (CadIr, SurfaceId) {
    let (mut ir, blend_surface) = variable_blend_eval_fixture(
        Point3::new(0.0, 0.0, 0.0),
        [
            (Point2::new(3.0, 0.0), Point2::new(0.0, 1.0)),
            (Point2::new(0.0, 3.0), Point2::new(1.0, 0.0)),
        ],
        [3.0, 3.0],
        Some(cross_section),
    );
    ir.model.surfaces[0].geometry = steep_plane_support();
    (ir, blend_surface)
}

/// The first support's partials at the contact `(3, 0.5, 0)` overflow.
fn assert_first_support_partial_overflows(ir: &CadIr) {
    let index = crate::index::ModelIndex::new(ir);
    assert_eq!(
        model_surface_partials_by_id(&index, &ir.model.surfaces[0].id, 3.0, 0.5)
            .map(crate::eval::SurfacePartials::into_raw),
        Err(crate::eval::EvaluationFailure::NonFinite(Point3::new(
            3.0, 0.5, 0.0
        )))
    );
}

#[test]
fn a_ruled_variable_blend_whose_support_partial_overflows_keeps_its_point() {
    // The zero-radius chamfer rules from (3, v, 0) to (0, v, 3).
    let (ir, blend_surface) =
        steep_support_blend_fixture(VariableBlendCrossSection::RoundedChamfer { radius: None });
    assert_first_support_partial_overflows(&ir);
    let index = crate::index::ModelIndex::new(&ir);
    let point = Point3::new(1.5, 0.5, 1.5);
    assert_eq!(
        model_surface_point_by_id(&index, &blend_surface, 0.5, 0.5)
            .map(crate::features::FinitePoint3::get),
        Ok(point)
    );
    assert_eq!(
        model_surface_point(&ir, &ir.model.surfaces[2].geometry, 0.5, 0.5)
            .map(crate::features::FinitePoint3::get),
        Ok(point)
    );
    assert_eq!(
        model_surface_partials_by_id(&index, &blend_surface, 0.5, 0.5)
            .map(crate::eval::SurfacePartials::into_raw),
        Err(crate::eval::EvaluationFailure::NonFinite(point))
    );
}

#[test]
fn a_circular_variable_blend_whose_support_partial_overflows_reaches_no_coordinate() {
    // The circular section reads the support normals, which the
    // overflowing partial leaves without a value.
    let (ir, blend_surface) = steep_support_blend_fixture(VariableBlendCrossSection::Circular {});
    assert_first_support_partial_overflows(&ir);
    assert!(both_routes_reach_no_coordinate(
        &ir,
        &blend_surface,
        0.5,
        0.5
    ));
}

#[test]
fn a_constant_rolling_ball_whose_support_partial_overflows_keeps_its_point() {
    // The section centered on the spine reads the contact points alone.
    let (mut ir, blend_surface) =
        constant_rolling_ball_fixture(CurveGeometry::Solved(SolvedCurveGeometry::Line(
            crate::geometry::analytic::LineCurve::try_new(
                Point3::new(3.0, 0.0, 3.0),
                Vector3::new(0.0, 1.0, 0.0),
            )
            .unwrap(),
        )));
    ir.model.surfaces[0].geometry = steep_plane_support();
    assert_first_support_partial_overflows(&ir);
    let index = crate::index::ModelIndex::new(&ir);
    assert_eq!(
        model_surface_point_by_id(&index, &blend_surface, 0.0, 0.5)
            .map(crate::features::FinitePoint3::get),
        Ok(Point3::new(3.0, 0.5, 0.0))
    );
    let point = model_surface_point_by_id(&index, &blend_surface, 0.5, 0.5)
        .expect("cacheless constant rolling-ball blend");
    let expected = 3.0 - 3.0 / 2.0_f64.sqrt();
    let tolerance = 64.0 * f64::EPSILON;
    assert!((point.x - expected).abs() <= tolerance);
    assert!((point.y - 0.5).abs() <= tolerance);
    assert!((point.z - expected).abs() <= tolerance);
    assert!(matches!(
        model_surface_partials_by_id(&index, &blend_surface, 0.5, 0.5),
        Err(crate::eval::EvaluationFailure::NonFinite(reached)) if reached == point.get()
    ));
}

#[test]
fn a_circular_variable_blend_whose_contact_pcurve_has_no_tangent_keeps_its_point() {
    // The first contact pcurve offsets the line u = 5 twice by 1 toward
    // smaller u: its point is (3, t) and an offset over an offset states no
    // tangent.
    let (mut ir, blend_surface) = variable_blend_eval_fixture(
        Point3::new(0.0, 0.0, 0.0),
        [
            (Point2::new(3.0, 0.0), Point2::new(0.0, 1.0)),
            (Point2::new(0.0, 3.0), Point2::new(1.0, 0.0)),
        ],
        [3.0, 3.0],
        Some(VariableBlendCrossSection::Circular {}),
    );
    let offset = |basis| {
        PcurveGeometry::Offset(
            crate::geometry::pcurve::OffsetPcurve::try_new(1.0, Box::new(basis))
                .expect("offset pcurve fixture"),
        )
    };
    let pcurve = offset(offset(PcurveGeometry::Line(
        crate::geometry::pcurve::LinePcurve::try_new(Point2::new(5.0, 0.0), Point2::new(0.0, 1.0))
            .expect("line pcurve fixture"),
    )));
    assert_eq!(
        crate::eval::pcurve_uv(&pcurve, 0.5).map(crate::units::FinitePoint2::get),
        Ok(Point2::new(3.0, 0.5))
    );
    assert_eq!(
        crate::eval::pcurve_tangent(&pcurve, 0.5),
        Err(crate::eval::EvaluationFailure::NoValue)
    );
    ir.model.procedural_surfaces[0].edit_definition(|definition| {
        let ProceduralSurfaceDefinition::VariableBlend(definition_payload) = definition else {
            unreachable!()
        };
        let mut construction = definition_payload.construction().to_raw();
        construction.sides[0].pcurve = Some(pcurve);
        *definition_payload =
            crate::geometry::surface_payloads::VariableBlendSurfacePayload::try_new(Box::new(
                construction,
            ))
            .unwrap();
    });
    let index = crate::index::ModelIndex::new(&ir);
    let point = model_surface_point_by_id(&index, &blend_surface, 0.5, 0.5)
        .expect("cacheless circular variable blend");
    let expected = 3.0 - 3.0 / 2.0_f64.sqrt();
    let tolerance = 64.0 * f64::EPSILON;
    assert!((point.x - expected).abs() <= tolerance);
    assert!((point.y - 0.5).abs() <= tolerance);
    assert!((point.z - expected).abs() <= tolerance);
    assert!(model_surface_partials_by_id(&index, &blend_surface, 0.5, 0.5).is_err());
}

#[test]
fn a_cacheless_blend_has_the_same_partials_within_a_work_budget() {
    let (ir, blend_surface) =
        constant_rolling_ball_fixture(CurveGeometry::Solved(SolvedCurveGeometry::Line(
            crate::geometry::analytic::LineCurve::try_new(
                Point3::new(3.0, 0.0, 3.0),
                Vector3::new(0.0, 1.0, 0.0),
            )
            .unwrap(),
        )));
    let index = crate::index::ModelIndex::new(&ir);
    let budget = cadmpeg_core::decode::WorkBudget::new(1_000_000);
    let partials = model_surface_partials_by_id(&index, &blend_surface, 0.5, 0.5)
        .expect("cacheless rolling-ball partials");
    assert_eq!(
        crate::eval::model_surface_partials_by_id_with_budget(
            &index,
            &blend_surface,
            0.5,
            0.5,
            &budget
        ),
        Ok(partials)
    );
}
