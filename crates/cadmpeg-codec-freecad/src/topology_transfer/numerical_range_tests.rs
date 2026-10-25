// SPDX-License-Identifier: Apache-2.0

use super::*;

const SMALL_PARAMETER_DOMAIN: f64 = 1e-12;
use cadmpeg_ir::geometry::pcurve::PcurveNurbs;
use cadmpeg_ir::math::Point2;

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
    use cadmpeg_ir::math::Point3;

    let mut ir = cadmpeg_ir::examples::unit_cube().unwrap();
    let source = CurveId::mint("test:model:curve#circle").unwrap();
    let subset = CurveId::mint("test:model:curve#subset").unwrap();
    let construction = ProceduralCurveId::mint("test:model:construction#subset").unwrap();
    ir.model.curves.push(Curve {
        native_ref: None,
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
        native_ref: None,
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
    let local = occt_subset_local_range(&ir, &subset, range)
        .unwrap()
        .map(FiniteReal::get);
    assert_eq!(local, [0.0, 1.0]);
}
