// SPDX-License-Identifier: Apache-2.0
use super::*;
use crate::ids::ProceduralCurveId;

#[test]
fn cached_subset_retains_local_parameters_for_points_derivatives_and_inversion() {
    let source = CurveId::mint("test:model:curve#source").unwrap();
    let subset = CurveId::mint("test:model:curve#subset").unwrap();
    let line = CurveGeometry::Line(
        crate::geometry::LineCurve::try_new(
            Point3::new(0.0, 0.0, 0.0),
            Vector3::new(1.0, 0.0, 0.0),
        )
        .unwrap(),
    );
    for sense in [true, false] {
        let mut ir = CadIr::empty();
        for id in [&source, &subset] {
            ir.model.curves.push(Curve {
                id: id.clone(),
                geometry: line.clone(),
                source_object: None,
            });
        }
        ir.model
            .add_procedural_curve(
                subset.clone(),
                ProceduralCurve::try_new(
                    ProceduralCurveId::mint("test:model:procedural-curve#subset").unwrap(),
                    ProceduralCurveDefinition::Subset {
                        source: source.clone(),
                        parameter_range: [2.0, 5.0],
                        sense,
                    },
                    None,
                )
                .unwrap(),
            )
            .unwrap();
        let index = crate::index::ModelIndex::new(&ir);
        let expected = Point3::new(if sense { 3.0 } else { 4.0 }, 0.0, 0.0);
        assert_eq!(
            model_curve_point_by_id(&index, &subset, 1.0),
            Some(expected)
        );
        let differential = model_curve_differential_by_id(&index, &subset, 1.0).unwrap();
        assert_eq!(differential.point, expected);
        assert_eq!(
            differential.tangent,
            Vector3::new(if sense { 1.0 } else { -1.0 }, 0.0, 0.0)
        );
        assert_eq!(differential.acceleration, Vector3::new(0.0, 0.0, 0.0));
        assert_eq!(
            model_curve_parameter_near_point_with_tolerance(&index, &subset, expected, 1.0, 0.0, 0,),
            Some(1.0),
        );
        assert_eq!(model_curve_point_by_id(&index, &subset, 4.0), None);
    }
}
