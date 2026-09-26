// SPDX-License-Identifier: Apache-2.0
use crate::geometry::curve_payloads::{
    OffsetCurveConstruction, ProjectionCurvePayload, SpringCurvePayload,
};
use crate::geometry::pcurve::{LinePcurve, PcurveGeometry};
use crate::geometry::surface_payloads::{
    CompoundSurfacePayload, ExactSurfacePayload, TaperSurfaceConstruction,
};
use crate::geometry::{
    CacheContract, CompoundComponent, CurveOffsetDistanceLaw, CurveOffsetLawBasis,
    CurveOffsetRange, DirectedParameterRange, ExactSpline, IntcurveSupportContext,
    IntcurveSupportSide, OffsetSide, ProceduralGeometryError, ProjectionRole, ProjectionTail,
    SpringLayout, SpringPcurve, SpringSupport, SupportPcurve, TaperSurfaceKind,
};
use crate::ids::{CurveId, SurfaceId};
use crate::math::{Point2, Vector3};
use crate::scalar::FiniteReal;
use crate::topology::ParameterInterval;

fn surface() -> SurfaceId {
    SurfaceId::mint("synthetic:test:surface#admitted").unwrap()
}

fn curve() -> CurveId {
    CurveId::mint("synthetic:test:curve#admitted").unwrap()
}

fn refusal(message: &'static str) -> ProceduralGeometryError {
    ProceduralGeometryError::Payload(message)
}

#[test]
fn procedural_surface_stores_hold_their_admitted_construction_records() {
    let taper = TaperSurfaceKind::Ruled {
        draft: Vector3::new(0.0, 0.0, 1.0),
        sine: 0.6,
        cosine: 0.8,
        factor: 1.25,
    };
    let admitted = TaperSurfaceConstruction::try_new(
        surface(),
        curve(),
        None,
        2.0,
        taper.clone(),
        CacheContract::legacy(),
    )
    .unwrap();
    let TaperSurfaceKind::Ruled { factor, .. } = admitted.taper() else {
        panic!("ruled taper tail")
    };
    assert_eq!(*factor, FiniteReal::new(1.25).unwrap());
    assert_eq!(admitted.taper().to_raw(), taper);
    let TaperSurfaceKind::Ruled { draft, .. } = &taper else {
        unreachable!()
    };
    // The tail refusal precedes the parameter refusal, as before.
    assert_eq!(
        TaperSurfaceConstruction::try_new(
            surface(),
            curve(),
            None,
            f64::NAN,
            TaperSurfaceKind::Ruled {
                draft: *draft,
                sine: 0.6,
                cosine: 0.8,
                factor: f64::INFINITY,
            },
            CacheContract::legacy(),
        )
        .unwrap_err(),
        refusal("taper surface parameter or subtype tail is invalid")
    );
    assert_eq!(
        TaperSurfaceConstruction::try_new(
            surface(),
            curve(),
            None,
            f64::NAN,
            taper,
            CacheContract::legacy(),
        )
        .unwrap_err(),
        refusal("Taper.parameter is not finite")
    );

    // Admission tests finiteness; the store tests the range order.
    let spline = ExactSpline::Legacy {
        ranges: [[0.0, 1.0], [-2.0, 2.0]],
        extension: 0,
        cache: None,
    };
    let exact = ExactSurfacePayload::try_new(spline.clone()).unwrap();
    assert_eq!(exact.spline().to_raw(), spline);
    for ranges in [[[1.0, 0.0], [-2.0, 2.0]], [[0.0, f64::NAN], [-2.0, 2.0]]] {
        assert_eq!(
            ExactSurfacePayload::try_new(ExactSpline::Legacy {
                ranges,
                extension: 0,
                cache: None,
            })
            .unwrap_err(),
            refusal("exact spline surface parameter fields are invalid")
        );
    }

    let components = vec![CompoundComponent {
        parameter: 0.5,
        component: surface(),
    }];
    let compound = CompoundSurfacePayload::try_new(components.clone(), None).unwrap();
    assert_eq!(
        compound.components()[0].parameter,
        FiniteReal::new(0.5).unwrap()
    );
    assert_eq!(compound.components()[0].to_raw(), components[0]);
    assert_eq!(
        CompoundSurfacePayload::try_new(
            vec![CompoundComponent {
                parameter: f64::NEG_INFINITY,
                component: surface(),
            }],
            None,
        )
        .unwrap_err(),
        refusal("compound surface parameters and components are inconsistent")
    );
}

#[test]
fn procedural_curve_stores_hold_their_admitted_construction_records() {
    let side = OffsetSide::PlaneNormal {
        normal: Vector3::new(0.0, 0.0, 1.0),
    };
    let range = CurveOffsetRange::Variable {
        parameter_range: crate::topology::IncreasingParameterInterval::new([0.0, 1.0]).unwrap(),
        distance_law: CurveOffsetDistanceLaw::Coordinate {
            function: curve(),
            coordinate: crate::geometry::CurveOffsetCoordinate::try_new(1).unwrap(),
            basis: CurveOffsetLawBasis::Parameter,
            function_parameter_offset: 0.0,
            function_parameter_scale: 0.5,
        },
    };
    let offset =
        OffsetCurveConstruction::try_new(curve(), 1.0, side.clone(), Some(range.clone())).unwrap();
    assert_eq!(offset.side().to_raw(), side);
    assert_eq!(
        offset.range().as_ref().map(CurveOffsetRange::to_raw),
        Some(range.clone())
    );
    // A non-unit normal, a zero law scale and a non-finite law value share
    // one refusal, which precedes the distance refusal.
    let CurveOffsetRange::Variable {
        parameter_range, ..
    } = &range
    else {
        unreachable!()
    };
    for (side, scale) in [
        (
            OffsetSide::PlaneNormal {
                normal: Vector3::new(0.0, 0.0, 2.0),
            },
            0.5,
        ),
        (side.clone(), 0.0),
        (side.clone(), f64::NAN),
    ] {
        assert_eq!(
            OffsetCurveConstruction::try_new(
                curve(),
                f64::NAN,
                side,
                Some(CurveOffsetRange::Variable {
                    parameter_range: *parameter_range,
                    distance_law: CurveOffsetDistanceLaw::Coordinate {
                        function: curve(),
                        coordinate: crate::geometry::CurveOffsetCoordinate::try_new(1).unwrap(),
                        basis: CurveOffsetLawBasis::Parameter,
                        function_parameter_offset: 0.0,
                        function_parameter_scale: scale,
                    },
                }),
            )
            .unwrap_err(),
            refusal(crate::geometry::INVALID_CURVE_OFFSET)
        );
    }

    let sides = || {
        std::array::from_fn(|_| IntcurveSupportSide {
            surface: None,
            pcurve: None,
        })
    };
    let context = IntcurveSupportContext::try_new(sides(), [0.0, 1.0], Default::default()).unwrap();
    let tail = ProjectionTail::Ranged {
        flag: true,
        parameter_range: [0.0, 2.0],
        role: ProjectionRole::Surf2,
    };
    let projection =
        ProjectionCurvePayload::try_new(context.clone(), false, curve(), tail.clone()).unwrap();
    assert_eq!(projection.tail().to_raw(), tail);
    for parameter_range in [[2.0, 0.0], [0.0, f64::INFINITY]] {
        assert_eq!(
            ProjectionCurvePayload::try_new(
                context.clone(),
                false,
                curve(),
                ProjectionTail::Ranged {
                    flag: true,
                    parameter_range,
                    role: ProjectionRole::Surf2,
                },
            )
            .unwrap_err(),
            refusal("projection fields are not finite and ordered")
        );
    }

    let spring = |parameter_range: [f64; 2], discontinuity: f64, range: [f64; 2]| {
        SpringLayout::ContextFirst {
            supports: [
                SpringSupport::Surface(surface()),
                SpringSupport::Ranges([[0.0, 1.0], [0.0, 1.0]]),
            ],
            first_pcurve: SpringPcurve::Range(range),
            second_pcurve: None,
            parameter_range,
            discontinuities: [vec![discontinuity], Vec::new(), Vec::new()],
            discontinuity_flag: false,
            cache: None,
        }
    };
    let layout = spring([0.0, 1.0], 0.5, [0.0, 1.0]);
    let payload = SpringCurvePayload::try_new(layout.clone(), 4).unwrap();
    assert_eq!(payload.layout().to_raw(), layout);
    let SpringLayout::ContextFirst {
        parameter_range,
        discontinuities,
        ..
    } = payload.layout()
    else {
        panic!("context-first spring")
    };
    assert_eq!(parameter_range.endpoints(), [0.0, 1.0]);
    assert_eq!(discontinuities[0], [FiniteReal::new(0.5).unwrap()]);
    assert_eq!(
        payload.support_context().clone(),
        IntcurveSupportContext::try_new(
            [
                IntcurveSupportSide {
                    surface: Some(surface()),
                    pcurve: None,
                },
                IntcurveSupportSide {
                    surface: None,
                    pcurve: None,
                },
            ],
            [0.0, 1.0],
            [vec![0.5], Vec::new(), Vec::new()],
        )
        .unwrap()
    );
    for layout in [
        spring([1.0, 0.0], 0.5, [0.0, 1.0]),
        spring([0.0, 1.0], f64::NAN, [0.0, 1.0]),
        spring([0.0, 1.0], 0.5, [1.0, 0.0]),
    ] {
        assert_eq!(
            SpringCurvePayload::try_new(layout, 4).unwrap_err(),
            refusal("spring context, null-support ranges, or cache-first form are invalid")
        );
    }
}

#[test]
fn a_support_context_from_admitted_parts_tests_only_the_mapped_interval_width() {
    let mapped = || {
        let line = PcurveGeometry::Line(
            LinePcurve::try_new(Point2::new(0.0, 0.0), Point2::new(1.0, 0.0)).unwrap(),
        );
        [
            IntcurveSupportSide {
                surface: Some(surface()),
                pcurve: Some(SupportPcurve::new(
                    line,
                    Some(DirectedParameterRange::new([0.0, 1.0]).unwrap()),
                )),
            },
            IntcurveSupportSide {
                surface: None,
                pcurve: None,
            },
        ]
    };
    let discontinuities = [vec![FiniteReal::new(0.25).unwrap()], Vec::new(), Vec::new()];
    let context = IntcurveSupportContext::from_parts(
        mapped(),
        ParameterInterval::new([0.0, 1.0]).unwrap(),
        discontinuities.clone(),
    )
    .unwrap();
    assert_eq!(
        context,
        IntcurveSupportContext::try_new(mapped(), [0.0, 1.0], [vec![0.25], Vec::new(), Vec::new()])
            .unwrap()
    );
    assert_eq!(
        IntcurveSupportContext::from_parts(
            mapped(),
            ParameterInterval::new([1.0, 1.0]).unwrap(),
            discontinuities,
        )
        .unwrap_err(),
        "support context parameter_range must be nonzero for an explicit pcurve mapping"
    );
}
