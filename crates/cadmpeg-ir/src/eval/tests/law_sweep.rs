// SPDX-License-Identifier: Apache-2.0

use crate::eval::model_surface_partials_by_id;
use crate::eval::model_surface_point;
use crate::eval::model_surface_point_by_id;
use crate::eval::scalar_sweep_law_differential;
use crate::eval::sweep_profile_differential;
use crate::eval::sweep_profile_reversed;
use crate::eval::tests::bilinear_surface;
use crate::features::FinitePoint3;
use crate::features::FiniteVector3;
use crate::geometry::nurbs::NurbsCurve;
use crate::geometry::Curve;
use crate::geometry::CurveGeometry;
use crate::geometry::LawExpression;
use crate::geometry::LawFormula;
use crate::geometry::ProceduralSurface;
use crate::geometry::ProceduralSurfaceDefinition;
use crate::geometry::RevisionCacheForm;
use crate::geometry::RevisionSurfaceParameterization;
use crate::geometry::SolvedCurveGeometry;
use crate::geometry::SolvedSurfaceGeometry;
use crate::geometry::Surface;
use crate::geometry::SurfaceGeometry;
use crate::geometry::SweepRevisionForm;
use crate::geometry::SweepSurfaceConstruction;
use crate::geometry::SweepSurfaceLayout;
use crate::ids::CurveId;
use crate::ids::ProceduralSurfaceId;
use crate::ids::SurfaceId;
use crate::math::Point3;
use crate::math::Vector3;
use crate::CadIr;

#[test]
fn sweep_profile_frame_with_overflowing_norm_keeps_its_direction() {
    let frame = (
        FinitePoint3::ZERO,
        FiniteVector3::new(Vector3::new(f64::MAX, 0.0, 0.0)).unwrap(),
    );
    assert_eq!(
        sweep_profile_reversed(Some(frame), Vector3::new(-1.0, 0.0, 0.0)),
        Ok(true)
    );
}

#[test]
fn sweep_spine_tangent_with_overflowing_norm_keeps_its_direction() {
    let frame = (
        FinitePoint3::ZERO,
        FiniteVector3::new(Vector3::new(1.0, 0.0, 0.0)).unwrap(),
    );
    assert_eq!(
        sweep_profile_reversed(Some(frame), Vector3::new(-f64::MAX, 0.0, 0.0)),
        Ok(true)
    );
}

#[test]
fn law_sweep_maps_wide_profile_interval_into_finite_nurbs_domain() {
    let profile = CurveId::mint("test:model:curve#wide-sweep-profile").unwrap();
    let mut ir = CadIr::empty();
    ir.model.curves.push(Curve {
        id: profile.clone(),
        geometry: CurveGeometry::Solved(SolvedCurveGeometry::Nurbs(
            NurbsCurve::from_lanes(
                1,
                vec![0.0, 0.0, 1.0, 1.0],
                vec![Point3::new(1.0, 0.0, 0.0), Point3::new(2.0, 0.0, 0.0)],
                None,
                false,
            )
            .unwrap(),
        )),
        source_object: None,
    });
    let index = crate::index::ModelIndex::new(&ir);
    let range = [finite(-f64::MAX), finite(f64::MAX)];
    let parameter = finite(-f64::MAX * 0.5);
    let forward = sweep_profile_differential(&index, &profile, range, false, parameter)
        .expect("forward interior profile point");
    let reversed = sweep_profile_differential(&index, &profile, range, true, parameter)
        .expect("reversed interior profile point");
    assert_eq!(forward.point.get(), Point3::new(1.25, 0.0, 0.0));
    assert_eq!(reversed.point.get(), Point3::new(1.75, 0.0, 0.0));
}

#[test]
fn cacheless_law_differential_applies_algebraic_product_rule() {
    let law = LawExpression::Algebraic {
        operator: "MUL".into(),
        operands: vec![
            LawExpression::Double { value: 2.0 },
            LawExpression::Text {
                value: cadmpeg_core::nonblank_literal!("X"),
            },
        ],
    };
    let differential =
        scalar_sweep_law_differential(&law.admit().expect("finite law"), finite(3.0))
            .expect("law differential");
    assert_eq!(differential.value.get(), 6.0);
    assert_eq!(differential.derivative.unwrap().get(), 2.0);
}

#[test]
fn cacheless_law_differential_applies_elementary_functions_and_composition() {
    let inner = LawExpression::Algebraic {
        operator: "MUL".into(),
        operands: vec![
            LawExpression::Double { value: 2.0 },
            LawExpression::Text {
                value: cadmpeg_core::nonblank_literal!("X"),
            },
        ],
    };
    let law = LawExpression::Algebraic {
        operator: "SIN".into(),
        operands: vec![inner.clone()],
    };
    let differential =
        scalar_sweep_law_differential(&law.admit().expect("finite law"), finite(0.75))
            .expect("sine law");
    assert!((differential.value.get() - 1.5f64.sin()).abs() <= f64::EPSILON * 64.0);
    assert!(
        (differential.derivative.unwrap().get() - 2.0 * 1.5f64.cos()).abs() <= f64::EPSILON * 64.0
    );

    let composition = LawExpression::Algebraic {
        operator: "O".into(),
        operands: vec![
            LawExpression::Algebraic {
                operator: "COS".into(),
                operands: vec![LawExpression::Text {
                    value: cadmpeg_core::nonblank_literal!("X"),
                }],
            },
            inner,
        ],
    };
    let differential =
        scalar_sweep_law_differential(&composition.admit().expect("finite law"), finite(0.75))
            .expect("composed cosine law");
    assert!((differential.value.get() - 1.5f64.cos()).abs() <= f64::EPSILON * 64.0);
    assert!(
        (differential.derivative.unwrap().get() + 2.0 * 1.5f64.sin()).abs() <= f64::EPSILON * 64.0
    );
}

#[test]
fn a_law_whose_derivative_has_no_value_keeps_its_value() {
    // |x| at 0 is 0 and asin(x) at 1 is pi/2; neither has a derivative there.
    let absolute = LawExpression::Algebraic {
        operator: "ABS".into(),
        operands: vec![LawExpression::Text {
            value: cadmpeg_core::nonblank_literal!("X"),
        }],
    };
    let law = scalar_sweep_law_differential(&absolute, finite(0.0)).expect("absolute value");
    assert_eq!(law.value.get(), 0.0);
    assert_eq!(law.derivative, Err(crate::eval::EvaluationFailure::NoValue));

    let inverse = LawExpression::Algebraic {
        operator: "ARCSIN".into(),
        operands: vec![LawExpression::Text {
            value: cadmpeg_core::nonblank_literal!("X"),
        }],
    };
    let law = scalar_sweep_law_differential(&inverse, finite(1.0)).expect("inverse sine");
    assert_eq!(law.value.get(), std::f64::consts::FRAC_PI_2);
    assert_eq!(law.derivative, Err(crate::eval::EvaluationFailure::NoValue));
}

/// A finite test parameter.
fn finite(value: f64) -> crate::scalar::FiniteReal {
    crate::scalar::FiniteReal::new(value).unwrap()
}

#[test]
fn law_sweep_evaluation_applies_profile_scale_and_current_cache() {
    let profile_id =
        CurveId::mint("test:model:entity#profile-frame-profile").expect("valid identity");
    let spine_id = CurveId::mint("test:model:entity#profile-frame-spine").expect("valid identity");
    let surface_id =
        SurfaceId::mint("test:model:entity#profile-frame-sweep").expect("valid identity");
    let construction_id = ProceduralSurfaceId::mint("test:model:entity#profile-frame-construction")
        .expect("valid identity");
    let mut ir = CadIr::empty();
    ir.model.curves = vec![
        Curve {
            id: profile_id.clone(),
            geometry: CurveGeometry::Solved(SolvedCurveGeometry::Nurbs(
                NurbsCurve::from_lanes(
                    1,
                    vec![0.0, 0.0, 1.0, 1.0],
                    vec![Point3::new(1.0, 0.0, 0.0), Point3::new(2.0, 0.0, 0.0)],
                    None,
                    false,
                )
                .unwrap(),
            )),
            source_object: None,
        },
        Curve {
            id: spine_id.clone(),
            geometry: CurveGeometry::Solved(SolvedCurveGeometry::Nurbs(
                NurbsCurve::from_lanes(
                    1,
                    vec![0.0, 0.0, 1.0, 1.0],
                    vec![Point3::new(4.0, 5.0, 6.0), Point3::new(4.0, 5.0, 7.0)],
                    None,
                    false,
                )
                .unwrap(),
            )),
            source_object: None,
        },
    ];
    ir.model.surfaces.push(Surface {
        id: surface_id.clone(),
        geometry: SurfaceGeometry::Procedural {
            construction: construction_id.clone(),
            cache: None,
        },
        source_object: None,
    });
    ir.model.procedural_surfaces.push(procedural_surface! {
        id: construction_id,
        definition: ProceduralSurfaceDefinition::Sweep(crate::geometry::surface_payloads::SweepSurfacePayload::try_new(profile_id, spine_id, Some(Box::new(SweepSurfaceConstruction {
                primary_kind: 0,
                cache: crate::geometry::CacheContract::Revision { form: SweepRevisionForm {
                    revision: crate::scalar::PositiveI64::new(22601).expect("positive revision"),
                    primary_flag: true,
                    profile_endpoints: [None, None],
                    path_endpoints: [None, None],
                    cache: crate::geometry::RevisionCacheForm::Parameterization(
                        RevisionSurfaceParameterization::default(),
                    ),
                } },
                layout: SweepSurfaceLayout::LawDriven {
                    mode: -2,
                    profile_range: [-1.0, 0.0],
                    profile_frame: Some((Point3::new(2.0, 0.0, 0.0), Vector3::new(0.0, 0.0, -1.0))),
                    origin: Point3::new(0.0, 0.0, 0.0),
                    directions: [
                        Vector3::new(1.0, 0.0, 0.0),
                        Vector3::new(0.0, 1.0, 0.0),
                        Vector3::new(0.0, 0.0, 1.0),
                    ],
                    first_law: Box::new(LawExpression::Text {
                        value: cadmpeg_core::nonblank_literal!("2.0*X"),
                    }),
                    first_mode: 0,
                    first_range: [0.0, 1.0],
                    law_direction: Vector3::new(0.0, 0.0, 1.0),
                    path_mode: 1,
                    path_flag: true,
                    path_range: [0.0, 1.0],
                    path_parameter: 0.0,
                    second_law_flag: true,
                    second_law: Box::new(LawExpression::Text {
                        value: cadmpeg_core::nonblank_literal!("VEC(2,1,1)"),
                    }),
                    formula_mode: 0,
                    formula: LawFormula::Named {
                        name: cadmpeg_core::nonblank_literal!(
                            "ROTATE(DOMAIN(VEC(1,0,0),0,1),TRANS1)"
                        ),
                        variables: vec![LawExpression::TransformVec {
                            vectors: [
                                Vector3::new(0.0, 1.0, 0.0),
                                Vector3::new(-1.0, 0.0, 0.0),
                                Vector3::new(0.0, 0.0, 1.0),
                                Vector3::new(0.0, 0.0, 0.0),
                            ],
                            scale: 1.0,
                            flags: [true, false, false],
                        }],
                    },
                    trailing_flag: false,
                },
                discontinuities: std::array::from_fn(|_| Vec::new()),
                discontinuity_flag: false,
            }))).unwrap()),
        cache_fit_tolerance: None,
        record_bounds: None,
    });

    let index = crate::index::ModelIndex::new(&ir);
    let expected = Point3::new(-0.5, 0.5, 0.25);
    let point = model_surface_point_by_id(&index, &surface_id, -0.25, 0.25)
        .expect("profile-frame sweep point")
        .get();
    assert!((point.x - expected.x).abs() <= f64::EPSILON * 64.0);
    assert!((point.y - expected.y).abs() <= f64::EPSILON * 64.0);
    assert!((point.z - expected.z).abs() <= f64::EPSILON * 64.0);

    let partials = model_surface_partials_by_id(&index, &surface_id, -0.25, 0.25)
        .expect("profile-frame sweep partials");
    assert_eq!(partials.point, point);
    assert_eq!(partials.du, Vector3::new(0.0, -2.0, 0.0));
    assert_eq!(partials.dv, Vector3::new(-2.0, 0.0, 1.0));

    let SurfaceGeometry::Procedural { cache, .. } = &mut ir.model.surfaces[0].geometry else {
        panic!("fixture surface must retain its construction");
    };
    *cache = Some(SolvedSurfaceGeometry::Nurbs(bilinear_surface()));
    ir.model.procedural_surfaces[0].edit_definition(|definition| {
        let ProceduralSurfaceDefinition::Sweep(definition_payload) = definition else {
            unreachable!()
        };
        let (Some(mut native),) = (definition_payload
            .native()
            .as_deref()
            .map(|native| Box::new(native.to_raw())),)
        else {
            unreachable!()
        };

        let form = native.cache.form_mut().expect("revision sweep form");
        form.cache = crate::geometry::RevisionCacheForm::SolvedCache {
            fit_tolerance: crate::geometry::FitTolerance::try_new(0.0).unwrap(),
        };
        *definition_payload = crate::geometry::surface_payloads::SweepSurfacePayload::try_new(
            definition_payload.profile().clone(),
            definition_payload.spine().clone(),
            Some(native),
        )
        .unwrap();
    });

    let index = crate::index::ModelIndex::new(&ir);
    assert_eq!(
        model_surface_point_by_id(&index, &surface_id, 0.25, 0.5)
            .map(crate::features::FinitePoint3::get),
        Ok(Point3::new(0.25, 0.5, 0.0))
    );
    let cached_partials = model_surface_partials_by_id(&index, &surface_id, 0.25, 0.5)
        .expect("current sweep cache partials");
    assert_eq!(cached_partials.point, Point3::new(0.25, 0.5, 0.0));
    assert_eq!(cached_partials.du, Vector3::new(1.0, 0.0, 0.0));
    assert_eq!(cached_partials.dv, Vector3::new(0.0, 1.0, 0.0));

    ir.model.procedural_surfaces[0].edit_definition(|definition| {
        let ProceduralSurfaceDefinition::Sweep(definition_payload) = definition else {
            unreachable!()
        };
        let (Some(mut native),) = (definition_payload
            .native()
            .as_deref()
            .map(|native| Box::new(native.to_raw())),)
        else {
            unreachable!()
        };

        let form = native.cache.form_mut().expect("revision sweep form");
        form.cache = crate::geometry::RevisionCacheForm::Parameterization(
            RevisionSurfaceParameterization::default(),
        );
        if let SweepSurfaceLayout::LawDriven { first_law, .. } = &mut native.layout {
            **first_law = LawExpression::Text {
                value: cadmpeg_core::nonblank_literal!("unsupported-law"),
            };
        } else {
            unreachable!()
        }
        *definition_payload = crate::geometry::surface_payloads::SweepSurfacePayload::try_new(
            definition_payload.profile().clone(),
            definition_payload.spine().clone(),
            Some(native),
        )
        .unwrap();
    });
    let index = crate::index::ModelIndex::new(&ir);
    assert_eq!(
        model_surface_point_by_id(&index, &surface_id, 0.25, 0.5),
        Err(crate::eval::EvaluationFailure::NoValue)
    );
    assert!(model_surface_partials_by_id(&index, &surface_id, 0.25, 0.5).is_err());
}

#[test]
fn numerical_seventh_sweep_rail_keeps_finite_rotated_coordinates() {
    let off_diagonal = 2.0 / 3.0;
    let diagonal = -1.0 / 3.0;
    let formula = LawFormula::Named {
        name: cadmpeg_core::nonblank_literal!("ROTATE(DOMAIN(VEC(1,0,0),0,1),TRANS1)"),
        variables: vec![LawExpression::TransformVec {
            vectors: [
                Vector3::new(diagonal, off_diagonal, off_diagonal),
                Vector3::new(off_diagonal, diagonal, off_diagonal),
                Vector3::new(off_diagonal, off_diagonal, diagonal),
                Vector3::new(0.0, 0.0, 0.0),
            ],
            scale: 1.0,
            flags: [true, false, false],
        }],
    };
    let transform =
        crate::eval::sweep_rail_transform(&formula.admit().expect("finite formula")).unwrap();
    let point = transform
        .apply_point(Point3::new(f64::MAX, f64::MAX, f64::MAX))
        .unwrap();
    let vector = transform
        .apply_vector(Vector3::new(f64::MAX, f64::MAX, f64::MAX))
        .unwrap();
    for value in [point.x, point.y, point.z, vector.x, vector.y, vector.z] {
        assert!((value / f64::MAX - 1.0).abs() <= 4.0 * f64::EPSILON);
    }
}

#[test]
fn numerical_seventh_normalized_derivative_keeps_finite_results() {
    use crate::eval::unit_vector_with_derivative;
    let vector = |x, y, z| crate::features::FiniteVector3::new(Vector3::new(x, y, z)).unwrap();
    for magnitude in [1.0, 1.0e200, f64::MAX] {
        let (_, derivative) = unit_vector_with_derivative(
            vector(1.0, 1.0, 1.0),
            Ok(vector(magnitude, magnitude, magnitude)),
        )
        .unwrap();
        assert_eq!(derivative.unwrap(), Vector3::new(0.0, 0.0, 0.0));
        let (unit, derivative) = unit_vector_with_derivative(
            vector(magnitude, magnitude, 0.0),
            Ok(vector(0.0, 0.0, 0.0)),
        )
        .unwrap();
        assert!((unit.norm() - 1.0).abs() <= 4.0 * f64::EPSILON);
        assert_eq!(derivative.unwrap(), Vector3::new(0.0, 0.0, 0.0));
    }
    let (_, derivative) =
        unit_vector_with_derivative(vector(3.0, 0.0, 0.0), Ok(vector(7.0, 6.0, 0.0))).unwrap();
    assert_eq!(derivative.unwrap(), Vector3::new(0.0, 2.0, 0.0));
}

/// A cacheless law-driven sweep of the x-axis profile along the vertical
/// spine through (7, 11, 13), displaced by `first_law` along the section
/// normal.
pub(super) fn law_sweep_model(first_law: LawExpression) -> (CadIr, SurfaceId) {
    let profile_id = CurveId::mint("test:model:entity#profile").expect("valid identity");
    let spine_id = CurveId::mint("test:model:entity#spine").expect("valid identity");
    let surface_id = SurfaceId::mint("test:model:entity#cacheless-sweep").expect("valid identity");
    let mut ir = CadIr::empty();
    ir.model.curves = vec![
        Curve {
            id: profile_id.clone(),
            geometry: CurveGeometry::Solved(SolvedCurveGeometry::Line(
                crate::geometry::analytic::LineCurve::try_new(
                    Point3::new(0.0, 0.0, 0.0),
                    Vector3::new(1.0, 0.0, 0.0),
                )
                .unwrap(),
            )),
            source_object: None,
        },
        Curve {
            id: spine_id.clone(),
            geometry: CurveGeometry::Solved(SolvedCurveGeometry::Line(
                crate::geometry::analytic::LineCurve::try_new(
                    Point3::new(7.0, 11.0, 13.0),
                    Vector3::new(0.0, 0.0, 1.0),
                )
                .unwrap(),
            )),
            source_object: None,
        },
    ];
    ir.model.surfaces.push(Surface {
        id: surface_id.clone(),
        geometry: SurfaceGeometry::Procedural {
            construction: ProceduralSurfaceId::mint(
                "test:model:entity#cacheless-sweep-construction",
            )
            .expect("valid identity"),
            cache: None,
        },
        source_object: None,
    });
    ir.model.procedural_surfaces.push(procedural_surface! {
        id: ProceduralSurfaceId::mint("test:model:entity#cacheless-sweep-construction").expect("valid identity"),
        definition: ProceduralSurfaceDefinition::Sweep(crate::geometry::surface_payloads::SweepSurfacePayload::try_new(profile_id, spine_id, Some(Box::new(SweepSurfaceConstruction {
                primary_kind: 0,
                cache: crate::geometry::CacheContract::Revision { form: SweepRevisionForm {
                    revision: crate::scalar::PositiveI64::new(23100).expect("positive revision"),
                    primary_flag: false,
                    profile_endpoints: [Some(0.0), Some(1.0)],
                    path_endpoints: [Some(0.0), Some(1.0)],
                    cache: RevisionCacheForm::Parameterization(
                        RevisionSurfaceParameterization::default(),
                    ),
                } },
                layout: SweepSurfaceLayout::LawDriven {
                    mode: 10,
                    profile_range: [0.0, 1.0],
                    profile_frame: None,
                    origin: Point3::new(0.0, 0.0, 0.0),
                    directions: [
                        Vector3::new(1.0, 0.0, 0.0),
                        Vector3::new(0.0, 1.0, 0.0),
                        Vector3::new(0.0, 0.0, 1.0),
                    ],
                    first_law: Box::new(first_law),
                    first_mode: 21,
                    first_range: [0.0, 1.0],
                    law_direction: Vector3::new(0.0, 0.0, 1.0),
                    path_mode: 1,
                    path_flag: false,
                    path_range: [0.0, 1.0],
                    path_parameter: 0.0,
                    second_law_flag: false,
                    second_law: Box::new(LawExpression::Text {
                        value: cadmpeg_core::nonblank_literal!("VEC(1,1,1)"),
                    }),
                    formula_mode: 0,
                    formula: LawFormula::Null {},
                    trailing_flag: false,
                },
                discontinuities: std::array::from_fn(|_| Vec::new()),
                discontinuity_flag: false,
            }))).unwrap()),
        cache_fit_tolerance: None,
        record_bounds: None,
    });
    (ir, surface_id)
}

#[test]
fn a_law_sweep_whose_law_overflows_reaches_no_coordinate() {
    // exp(1000 v) at v = 1 leaves the finite range, so the section offset
    // reaches no coordinate.
    let (ir, surface_id) = law_sweep_model(LawExpression::Algebraic {
        operator: "EXP".into(),
        operands: vec![LawExpression::Text {
            value: cadmpeg_core::nonblank_literal!("1000.0*X"),
        }],
    });
    let index = crate::index::ModelIndex::new(&ir);
    for route in [
        model_surface_point_by_id(&index, &surface_id, 0.5, 1.0),
        model_surface_point(&ir, &ir.model.surfaces[0].geometry, 0.5, 1.0),
    ] {
        assert!(
            matches!(
                route,
                Err(crate::eval::EvaluationFailure::NonFinite(point))
                    if point.x.is_nan() && point.y.is_nan() && point.z.is_nan()
            ),
            "{route:?}"
        );
    }
}

#[test]
fn a_law_sweep_whose_law_derivative_has_no_value_keeps_its_point() {
    // ABS(X) at X = 0 has the value 0 and no derivative; the section point
    // reads the value alone and matches the sweep of the law X there.
    let (ir, surface_id) = law_sweep_model(LawExpression::Algebraic {
        operator: "ABS".into(),
        operands: vec![LawExpression::Text {
            value: cadmpeg_core::nonblank_literal!("X"),
        }],
    });
    let (plain, plain_id) = law_sweep_model(LawExpression::Text {
        value: cadmpeg_core::nonblank_literal!("X"),
    });
    let expected =
        model_surface_point_by_id(&crate::index::ModelIndex::new(&plain), &plain_id, 0.5, 0.0)
            .expect("sweep of the law X");
    let index = crate::index::ModelIndex::new(&ir);
    assert_eq!(
        model_surface_point_by_id(&index, &surface_id, 0.5, 0.0),
        Ok(expected)
    );
    assert_eq!(
        model_surface_point(&ir, &ir.model.surfaces[0].geometry, 0.5, 0.0),
        Ok(expected)
    );
    assert_eq!(
        model_surface_partials_by_id(&index, &surface_id, 0.5, 0.0)
            .map(crate::eval::SurfacePartials::into_raw),
        Err(crate::eval::EvaluationFailure::NoValue)
    );
}
