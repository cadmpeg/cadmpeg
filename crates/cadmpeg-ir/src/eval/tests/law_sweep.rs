// SPDX-License-Identifier: Apache-2.0

use super::*;

#[test]
fn cacheless_law_differential_applies_algebraic_product_rule() {
    let law = LawExpression::Algebraic {
        operator: "MUL".into(),
        operands: vec![
            LawExpression::Double { value: 2.0 },
            LawExpression::Text { value: "X".into() },
        ],
    };
    let differential = scalar_sweep_law_differential(&law, 3.0).expect("law differential");
    assert_eq!(differential.value, 6.0);
    assert_eq!(differential.derivative, 2.0);
}

#[test]
fn cacheless_law_differential_applies_elementary_functions_and_composition() {
    let inner = LawExpression::Algebraic {
        operator: "MUL".into(),
        operands: vec![
            LawExpression::Double { value: 2.0 },
            LawExpression::Text { value: "X".into() },
        ],
    };
    let law = LawExpression::Algebraic {
        operator: "SIN".into(),
        operands: vec![inner.clone()],
    };
    let differential = scalar_sweep_law_differential(&law, 0.75).expect("sine law");
    assert!((differential.value - 1.5f64.sin()).abs() <= f64::EPSILON * 64.0);
    assert!((differential.derivative - 2.0 * 1.5f64.cos()).abs() <= f64::EPSILON * 64.0);

    let composition = LawExpression::Algebraic {
        operator: "O".into(),
        operands: vec![
            LawExpression::Algebraic {
                operator: "COS".into(),
                operands: vec![LawExpression::Text { value: "X".into() }],
            },
            inner,
        ],
    };
    let differential =
        scalar_sweep_law_differential(&composition, 0.75).expect("composed cosine law");
    assert!((differential.value - 1.5f64.cos()).abs() <= f64::EPSILON * 64.0);
    assert!((differential.derivative + 2.0 * 1.5f64.sin()).abs() <= f64::EPSILON * 64.0);
}

#[test]
fn cacheless_law_differential_rejects_undefined_domains() {
    let absolute = LawExpression::Algebraic {
        operator: "ABS".into(),
        operands: vec![LawExpression::Text { value: "X".into() }],
    };
    assert!(scalar_sweep_law_differential(&absolute, 0.0).is_none());

    let inverse = LawExpression::Algebraic {
        operator: "ARCSIN".into(),
        operands: vec![LawExpression::Text { value: "X".into() }],
    };
    assert!(scalar_sweep_law_differential(&inverse, 1.0).is_none());
}

#[test]
fn law_sweep_evaluation_applies_profile_scale_and_current_cache() {
    let profile_id = CurveId("profile-frame-profile".into());
    let spine_id = CurveId("profile-frame-spine".into());
    let surface_id = SurfaceId("profile-frame-sweep".into());
    let construction_id = ProceduralSurfaceId("profile-frame-construction".into());
    let mut ir = CadIr::empty();
    ir.model.curves = vec![
        Curve {
            id: profile_id.clone(),
            geometry: CurveGeometry::Nurbs(
                NurbsCurve::new(
                    1,
                    vec![0.0, 0.0, 1.0, 1.0],
                    vec![Point3::new(1.0, 0.0, 0.0), Point3::new(2.0, 0.0, 0.0)],
                    None,
                    false,
                )
                .unwrap(),
            ),
            source_object: None,
        },
        Curve {
            id: spine_id.clone(),
            geometry: CurveGeometry::Nurbs(
                NurbsCurve::new(
                    1,
                    vec![0.0, 0.0, 1.0, 1.0],
                    vec![Point3::new(4.0, 5.0, 6.0), Point3::new(4.0, 5.0, 7.0)],
                    None,
                    false,
                )
                .unwrap(),
            ),
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
        definition: ProceduralSurfaceDefinition::Sweep {
            profile: profile_id,
            spine: spine_id,
            native: Some(Box::new(SweepSurfaceConstruction {
                primary_kind: 0,
                revision_form: Some(SweepRevisionForm {
                    revision: 22601,
                    primary_flag: true,
                    profile_endpoints: [None, None],
                    path_endpoints: [None, None],
                    cache: crate::geometry::RevisionCacheForm::Parameterization(
                        RevisionSurfaceParameterization::default(),
                    ),
                }),
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
                        value: "2.0*X".into(),
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
                        value: "VEC(2,1,1)".into(),
                    }),
                    formula_mode: 0,
                    formula: LawFormula::Named {
                        name: crate::geometry::LawFormulaName::new(
                            "ROTATE(DOMAIN(VEC(1,0,0),0,1),TRANS1)",
                        )
                        .unwrap(),
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
            })),
        },
        cache_fit_tolerance: None,
        record_bounds: None,
    });

    let index = crate::index::ModelIndex::new(&ir);
    let expected = Point3::new(-0.5, 0.5, 0.25);
    let point = model_surface_point_by_id(&index, &surface_id, -0.25, 0.25)
        .expect("profile-frame sweep point");
    assert!((point.x - expected.x).abs() <= f64::EPSILON * 64.0);
    assert!((point.y - expected.y).abs() <= f64::EPSILON * 64.0);
    assert!((point.z - expected.z).abs() <= f64::EPSILON * 64.0);

    let partials = model_surface_partials_by_id(&index, &surface_id, -0.25, 0.25)
        .expect("profile-frame sweep partials");
    assert_eq!(partials.point, point);
    assert_eq!(partials.du, Vector3::new(0.0, -2.0, 0.0));
    assert_eq!(partials.dv, Vector3::new(-2.0, 0.0, 1.0));

    ir.model.surfaces[0].geometry = SurfaceGeometry::Nurbs(bilinear_surface());
    ir.model.procedural_surfaces[0].edit_definition(|definition| {
        let ProceduralSurfaceDefinition::Sweep {
            native: Some(native),
            ..
        } = definition
        else {
            unreachable!()
        };
        let form = native.revision_form.as_mut().expect("revision sweep form");
        form.cache = crate::geometry::RevisionCacheForm::SolvedCache { fit_tolerance: 0.0 };
    });

    let index = crate::index::ModelIndex::new(&ir);
    assert_eq!(
        model_surface_point_by_id(&index, &surface_id, 0.25, 0.5),
        Some(Point3::new(0.25, 0.5, 0.0))
    );
    let cached_partials = model_surface_partials_by_id(&index, &surface_id, 0.25, 0.5)
        .expect("current sweep cache partials");
    assert_eq!(cached_partials.point, Point3::new(0.25, 0.5, 0.0));
    assert_eq!(cached_partials.du, Vector3::new(1.0, 0.0, 0.0));
    assert_eq!(cached_partials.dv, Vector3::new(0.0, 1.0, 0.0));

    ir.model.procedural_surfaces[0].edit_definition(|definition| {
        let ProceduralSurfaceDefinition::Sweep {
            native: Some(native),
            ..
        } = definition
        else {
            unreachable!()
        };
        let form = native.revision_form.as_mut().expect("revision sweep form");
        form.cache = crate::geometry::RevisionCacheForm::Parameterization(
            RevisionSurfaceParameterization::default(),
        );
        if let SweepSurfaceLayout::LawDriven { first_law, .. } = &mut native.layout {
            **first_law = LawExpression::Text {
                value: "unsupported-law".into(),
            };
        } else {
            unreachable!()
        }
    });
    let index = crate::index::ModelIndex::new(&ir);
    assert!(model_surface_point_by_id(&index, &surface_id, 0.25, 0.5).is_none());
    assert!(model_surface_partials_by_id(&index, &surface_id, 0.25, 0.5).is_none());
}
