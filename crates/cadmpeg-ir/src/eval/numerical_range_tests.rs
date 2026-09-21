// SPDX-License-Identifier: Apache-2.0

use super::*;

const CHORD_BOUND_TOLERANCE: f64 = 1e-10;
use crate::geometry::nurbs::{NurbsSurfaceAxis, NurbsSurfaceLanes};
use crate::geometry::{
    Curve, ProceduralSurface, RevisionSurfaceParameterization, Surface, SweepRevisionForm,
    SweepSurfaceConstruction,
};
use crate::ids::{CurveId, ProceduralSurfaceId, SurfaceId};

fn bilinear(domain: [f64; 2], scale: f64) -> NurbsSurface {
    NurbsSurface::from_lanes(
        NurbsSurfaceAxis::new(1, vec![domain[0], domain[0], domain[1], domain[1]], false),
        NurbsSurfaceAxis::new(1, vec![0., 0., 1., 1.], false),
        NurbsSurfaceLanes::new(
            vec![
                vec![Point3::new(0., 0., 0.), Point3::new(0., scale, 0.)],
                vec![Point3::new(scale, 0., 0.), Point3::new(scale, scale, 0.)],
            ],
            None,
        ),
        false,
    )
    .unwrap()
}
fn fixture(scale: f64) -> (crate::CadIr, SurfaceId) {
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
    ir.model.procedural_surfaces.push(ProceduralSurface::new(
        construction_id,
        ProceduralSurfaceDefinition::Sweep(
            crate::geometry::surface_payloads::SweepSurfacePayload::try_new(
                profile_id,
                spine_id,
                Some(Box::new(SweepSurfaceConstruction {
                    primary_kind: 0,
                    cache: crate::geometry::CacheContract::Revision {
                        form: SweepRevisionForm {
                            revision: 22601,
                            primary_flag: true,
                            profile_endpoints: [None, None],
                            path_endpoints: [None, None],
                            cache: crate::geometry::RevisionCacheForm::Parameterization(
                                RevisionSurfaceParameterization::default(),
                            ),
                        },
                    },
                    layout: SweepSurfaceLayout::LawDriven {
                        mode: -2,
                        profile_range: [-scale, 0.0],
                        profile_frame: Some((
                            Point3::new(2.0, 0.0, 0.0),
                            Vector3::new(0.0, 0.0, -1.0),
                        )),
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
                })),
            )
            .unwrap(),
        ),
        None,
    ));

    (ir, surface_id)
}
#[test]
fn numerical_0922_wide_surface_chart_keeps_inverse_and_bound() {
    for d in [[0., 1.], [-1e308, 1e308]] {
        let s = bilinear(d, 1.);
        let endpoints = [Point2::new(d[0], 0.), Point2::new(d[1], 1.)];
        let bound = crate::eval::nurbs_surface_parameter_segment_chord_bound(
            &s,
            endpoints,
            [Point3::new(0., 0., 0.), Point3::new(1., 1., 0.)],
        );
        let inverse =
            crate::eval::nurbs_surface_parameter_near_point(&s, Point3::new(0.5, 0.5, 0.), None);
        println!("IR plane domain {d:?}: chord bound={bound:?}, inverse={inverse:?}");
        assert!(bound.unwrap() < CHORD_BOUND_TOLERANCE);
        let expected_u = d[0].midpoint(d[1]);
        assert_eq!(inverse, Some(Point2::new(expected_u, 0.5)));
    }
}
#[test]
fn numerical_0922_far_surface_query_keeps_inverse() {
    let s = bilinear([0., 1.], 1.);
    for z in [1., 1e200] {
        let budget = cadmpeg_core::decode::WorkBudget::new(100_000);
        let result = crate::eval::nurbs_surface_closest_parameter_with_budget(
            &s,
            Point3::new(0.5, 0.5, z),
            None,
            &budget,
        );
        println!(
            "IR unit plane, query z={z:e}: {result:?}, budget {}",
            budget.consumed()
        );
        assert_eq!(result, Some(Point2::new(0.5, 0.5)));
    }
}
#[test]
fn numerical_0922_reflection_rejects_unrelated_points() {
    for scale in [1., 1e200] {
        let a = Point3::new(0., scale, 0.);
        let b = Point3::new(scale, scale, 0.);
        let result = crate::eval::spatial_points_are_reflections(
            a,
            b,
            Point3::new(0., 0., 0.),
            Point3::new(scale, 0., 0.),
        );
        println!("IR non-reflections scale{scale:e}: {result}");
        assert!(!result);
        assert!(spatial_points_are_reflections(
            a,
            Point3::new(0., -scale, 0.),
            Point3::new(0., 0., 0.),
            Point3::new(scale, 0., 0.)
        ));
    }
}
#[test]
fn numerical_0922_sweep_evaluation_ignores_profile_units() {
    for scale in [1., 1e18] {
        let (ir, id) = fixture(scale);
        let index = crate::index::ModelIndex::new(&ir);
        let r = crate::eval::model_surface_point_by_id(&index, &id, -0.25 * scale, 0.25);
        println!("IR same law sweep profile chart scale{scale:e}: {r:?}");
        assert!(r.unwrap().distance(Point3::new(-0.5, 0.5, 0.25)) < 1e-14);
    }
}
