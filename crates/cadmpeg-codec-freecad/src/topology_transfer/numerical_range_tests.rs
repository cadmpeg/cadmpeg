// SPDX-License-Identifier: Apache-2.0

use super::*;

const SMALL_PARAMETER_DOMAIN: f64 = 1e-12;
use cadmpeg_ir::geometry::pcurve::PcurveNurbs;
use cadmpeg_ir::math::{Point2, Point3};

#[test]
fn numerical_0922_interior_trim_is_preserved() {
    for domain in [1., SMALL_PARAMETER_DOMAIN] {
        let g = PcurveGeometry::Nurbs {
            nurbs: PcurveNurbs::from_lanes(
                1,
                vec![0., 0., domain, domain],
                vec![Point2::new(0., 0.), Point2::new(10., 0.)],
                None,
                false,
            )
            .unwrap(),
        };
        let range = [0.1 * domain, 0.9 * domain];
        let result = normalize_pcurve_parameter_range(
            &g,
            Some(range.map(|value| FiniteReal::new(value).expect("finite test endpoint"))),
        )
        .unwrap()
        .map(FiniteReal::get);
        println!("FreeCAD domain {domain:e}, input {range:?} => {result:?}");
        assert_eq!(result, range);
    }
}

#[test]
fn an_occt_endpoint_a_whole_period_off_a_circle_subset_start_maps_to_local_zero() {
    use cadmpeg_ir::geometry::analytic::CircleCurve;
    use cadmpeg_ir::geometry::curve_payloads::SubsetCurveConstruction;
    use cadmpeg_ir::geometry::{Curve, ProceduralCurve, ProceduralCurveDefinition};
    use cadmpeg_ir::ids::{CurveId, ProceduralCurveId};

    let mut ir = cadmpeg_ir::examples::unit_cube().unwrap();
    let source = CurveId::mint("test:model:curve#circle").unwrap();
    let subset = CurveId::mint("test:model:curve#subset").unwrap();
    let construction = ProceduralCurveId::mint("test:model:construction#subset").unwrap();
    ir.model.curves.push(Curve {
        id: source.clone(),
        geometry: CurveGeometry::Solved(SolvedCurveGeometry::Circle(
            CircleCurve::try_new(
                Point3::new(0.0, 0.0, 0.0),
                Vector3::new(0.0, 0.0, 1.0),
                Vector3::new(1.0, 0.0, 0.0),
                1.0,
            )
            .unwrap(),
        )),
        parameter_range: None,
        source_object: None,
    });
    ir.model.curves.push(Curve {
        id: subset.clone(),
        geometry: CurveGeometry::Procedural {
            construction: construction.clone(),
            cache: None,
        },
        parameter_range: None,
        source_object: None,
    });
    ir.model
        .add_procedural_curve(
            &subset,
            ProceduralCurve::new(
                construction,
                ProceduralCurveDefinition::Subset(
                    SubsetCurveConstruction::try_new(source, [1.0, 2.0], true, None).unwrap(),
                ),
            ),
        )
        .unwrap();
    let below_period = 1.0 - std::f64::consts::TAU - 4.0 * f64::EPSILON;
    let range = [below_period, 2.0].map(|value| FiniteReal::new(value).unwrap());
    let local = occt_subset_local_range(&ir, &mut ArenaPositions::default(), &subset, range)
        .unwrap()
        .map(FiniteReal::get);
    assert_eq!(local, [0.0, 1.0]);
}

#[test]
fn native_nonperiodic_cubic_edge_ranges_snap_only_exterior_rounding_noise() {
    use cadmpeg_ir::geometry::nurbs::NurbsCurve;
    for (domain, native_range) in [
        (
            [0.014_823_792_750_146_8, 0.014_967_317_186_835_7],
            [0.014_823_792_750_147, 0.014_967_317_186_836],
        ),
        (
            [7.357_766_737_4e-7, 0.000_144_260_213_362_61],
            [7.357_766_74e-7, 0.000_144_260_213_363],
        ),
        (
            [0.029_795_255_066_579_2, 0.029_938_808_475_002_6],
            [0.029_795_255_066_579, 0.029_938_808_475_003],
        ),
    ] {
        let geometry = SolvedCurveGeometry::Nurbs(
            NurbsCurve::from_lanes(
                3,
                [vec![domain[0]; 4], vec![domain[1]; 4]].concat(),
                vec![
                    Point3::new(0.0, 0.0, 0.0),
                    Point3::new(1.0, 0.0, 0.0),
                    Point3::new(2.0, 0.0, 0.0),
                    Point3::new(3.0, 0.0, 0.0),
                ],
                None,
                false,
            )
            .unwrap(),
        );
        let range = native_range.map(|value| FiniteReal::new(value).unwrap());
        let normalized = normalize_occt_curve_range(&geometry, Some(range))
            .unwrap()
            .map(FiniteReal::get);
        assert_eq!(normalized[0], native_range[0].max(domain[0]));
        assert_eq!(normalized[1], domain[1]);
        assert_eq!(range.map(FiniteReal::get), native_range);
        let wrapped = CurveGeometry::Solved(geometry.clone());
        for parameter in normalized {
            assert!(cadmpeg_ir::eval::decode::curve_point(
                cadmpeg_ir::eval::admission::EvaluationAdmission::Standard,
                &wrapped,
                parameter
            )
            .is_ok());
        }
        let outside =
            [domain[0] - 1.0e-8, domain[1] + 1.0e-8].map(|value| FiniteReal::new(value).unwrap());
        assert_eq!(
            normalize_occt_curve_range(&geometry, Some(outside)),
            Some(outside)
        );
        let inside = [
            domain[0] + (domain[1] - domain[0]) * 0.2,
            domain[0] + (domain[1] - domain[0]) * 0.8,
        ]
        .map(|value| FiniteReal::new(value).unwrap());
        assert_eq!(
            normalize_occt_curve_range(&geometry, Some(inside)),
            Some(inside)
        );
    }
}

#[test]
fn native_periodic_cubic_pcurve_keeps_a_seam_crossing_range() {
    for (period, range) in [
        (
            0.005_123_231_207_432_89,
            [0.002_564_023_778_11, 0.007_687_254_985_543],
        ),
        (
            0.005_118_414_858_650_36,
            [0.002_559_207_429_328, 0.007_677_622_287_978],
        ),
    ] {
        let geometry = PcurveGeometry::Nurbs {
            nurbs: PcurveNurbs::from_lanes(
                3,
                (-3..=7)
                    .map(|index| f64::from(index) * period / 4.0)
                    .collect::<Vec<_>>(),
                vec![
                    Point2::new(0.0, 0.0),
                    Point2::new(1.0, 0.0),
                    Point2::new(1.0, 1.0),
                    Point2::new(0.0, 1.0),
                    Point2::new(0.0, 0.0),
                    Point2::new(1.0, 0.0),
                    Point2::new(1.0, 1.0),
                ],
                None,
                true,
            )
            .unwrap(),
        };
        let range = range.map(|value| FiniteReal::new(value).unwrap());
        assert_eq!(
            normalize_pcurve_parameter_range(&geometry, Some(range)),
            Some(range)
        );
        for parameter in range {
            assert!(cadmpeg_ir::eval::decode::pcurve_uv(
                cadmpeg_ir::eval::admission::EvaluationAdmission::Standard,
                &geometry,
                parameter.get()
            )
            .is_ok());
        }
    }
}

#[test]
fn native_edge_range_on_a_zero_width_nurbs_domain_remains_available_to_validation() {
    let geometry = SolvedCurveGeometry::Nurbs(
        cadmpeg_ir::geometry::nurbs::NurbsCurve::from_lanes(
            1,
            vec![0.0; 4],
            vec![Point3::new(0.0, 0.0, 0.0), Point3::new(1.0, 0.0, 0.0)],
            None,
            false,
        )
        .unwrap(),
    );
    let range = [FiniteReal::ZERO, FiniteReal::ONE];
    assert_eq!(
        normalize_occt_curve_range(&geometry, Some(range)),
        Some(range)
    );
}
