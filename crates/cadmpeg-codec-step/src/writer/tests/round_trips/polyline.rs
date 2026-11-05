// SPDX-License-Identifier: Apache-2.0

use std::io::Cursor;

use cadmpeg_ir::codec::{Codec, DecodeOptions};
use cadmpeg_ir::eval::admission::EvaluationAdmission;
use cadmpeg_ir::eval::decode::curve_point_solved;
use cadmpeg_ir::geometry::sampled::{PolylineCurve, PolylineSamples, PolylineVertex};
use cadmpeg_ir::geometry::{Curve, CurveGeometry, SolvedCurveGeometry};
use cadmpeg_ir::ids::CurveId;
use cadmpeg_ir::math::Point3;
use cadmpeg_ir::CadIr;

use crate::{StepCodec, StepSchema, StepWriteOptions};

const EPS_POLYLINE_POINT: f64 = 1.0e-10;

#[test]
fn polyline_parameters_survive_conversion_to_degree_one_bspline() {
    for parameters in [[2.0, 5.0, 9.0], [9.0, 5.0, 2.0]] {
        let samples = [
            Point3::new(1.0, 0.0, 0.0),
            Point3::new(2.0, 3.0, 0.0),
            Point3::new(7.0, 3.0, 2.0),
        ]
        .into_iter()
        .zip(parameters)
        .map(|(point, parameter)| PolylineVertex { point, parameter })
        .collect::<Vec<_>>()
        .try_into()
        .expect("nonempty samples");
        let source = SolvedCurveGeometry::Polyline(
            PolylineCurve::new(PolylineSamples::Parameterized { vertices: samples }, 0.0)
                .expect("monotone parameters"),
        );
        let mut ir = CadIr::empty();
        ir.model.curves.push(Curve {
            id: CurveId::mint("test:model:curve#polyline").expect("curve id"),
            native_ref: None,
            parameter_range: None,
            geometry: CurveGeometry::Solved(source.clone()),
            source_object: None,
        });
        let mut bytes = Vec::new();
        crate::export::write_step(
            &ir,
            &mut bytes,
            StepSchema::Ap242Edition3,
            &StepWriteOptions::default(),
        )
        .expect("write polyline");
        assert!(String::from_utf8_lossy(&bytes).contains("B_SPLINE_CURVE_WITH_KNOTS("));
        let decoded = StepCodec::default()
            .decode(&mut Cursor::new(bytes), &DecodeOptions::default())
            .expect("decode spline")
            .into_parts()
            .0;
        let written = decoded
            .model
            .curves
            .iter()
            .find_map(|curve| match curve.geometry.solved() {
                Some(spline @ SolvedCurveGeometry::Nurbs(_)) => Some(spline),
                _ => None,
            })
            .expect("written spline");
        for parameter in [2.0, 3.5, 5.0, 7.0, 9.0] {
            let expected = curve_point_solved(EvaluationAdmission::Standard, &source, parameter)
                .expect("source point")
                .get();
            let actual = curve_point_solved(EvaluationAdmission::Standard, written, parameter)
                .expect("written point")
                .get();
            assert!(
                expected.distance(actual) < EPS_POLYLINE_POINT,
                "{parameter}: {expected:?} != {actual:?}"
            );
        }
    }
}
