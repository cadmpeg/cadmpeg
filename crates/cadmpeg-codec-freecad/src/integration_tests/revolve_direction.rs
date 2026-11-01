// SPDX-License-Identifier: Apache-2.0
//! Angular sense and stored end faces of CC0 recipe revolutions.

use std::io::Cursor;
use std::path::Path;

use cadmpeg_ir::features::{AngularTermination, BooleanOp, FeatureOperation, RevolveExtent};
use cadmpeg_ir::{Codec, DecodeOptions};
use cadmpeg_measure::{MeasureInput, MeasureOptions, Measurer, Selection};

use crate::FcstdCodec;

const EPS_RECIPE_ANGLE: f64 = 1.0e-12;
const EPS_RECIPE_END_FACE_MM: f64 = 1.0e-8;
const EPS_RECIPE_END_FACE_AREA_MM2: f64 = 1.0e-7;

#[test]
fn revolve_recipe_sides_agree_with_stored_solid_end_faces() {
    for (file, first, second, symmetric) in [
        ("0.21.2-F06b", 270.0_f64, 0.0_f64, false),
        ("1.0.2-F06b", 270.0, 0.0, false),
        ("1.1.4-F06b", 270.0, 0.0, false),
        ("1.1.4-F15b", 90.0, 45.0, false),
        ("1.1.4-F15c", 180.0, 0.0, true),
    ] {
        let path = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../corpus/freecad_fcstd/fixtures/recipes")
            .join(format!("{file}.FCStd"));
        let bytes = std::fs::read(path).expect("CC0 recipe");
        let decoded = FcstdCodec
            .decode(&mut Cursor::new(bytes), &DecodeOptions::default())
            .expect("recipe decode");
        let ir = decoded.ir();
        let operation = ir
            .model
            .features
            .iter()
            .find_map(|feature| {
                let operation = feature.evaluation.definition().operation();
                matches!(operation, FeatureOperation::Revolve { .. }).then_some(operation)
            })
            .expect("recipe revolution");
        let FeatureOperation::Revolve { construction, op } = operation else {
            unreachable!()
        };
        assert_eq!(*op, BooleanOp::Join);
        let axis = construction.axis().expect("axis");
        assert_eq!(axis.origin, cadmpeg_ir::features::FinitePoint3::ZERO);
        assert!((axis.direction.z - 1.0).abs() < EPS_RECIPE_ANGLE);
        assert!(axis.reference.is_some());
        let extent = construction.extent().expect("extent");
        let angle = |termination: &AngularTermination| {
            let AngularTermination::Angle { angle } = termination else {
                panic!("angle")
            };
            ir.dimension_value(angle)
                .expect("resolved revolution angle")
                .get()
        };
        match extent {
            RevolveExtent::OneSided { termination } => {
                assert!(!symmetric && second == 0.0);
                assert!((angle(termination) - first.to_radians()).abs() < EPS_RECIPE_ANGLE);
            }
            RevolveExtent::TwoSided {
                first: a,
                second: b,
            } => {
                assert!(!symmetric && second > 0.0);
                assert!((angle(a) - first.to_radians()).abs() < EPS_RECIPE_ANGLE);
                assert!((angle(b) - second.to_radians()).abs() < EPS_RECIPE_ANGLE);
            }
            RevolveExtent::Symmetric { termination } => {
                assert!(symmetric);
                assert!((angle(termination) - first.to_radians()).abs() < EPS_RECIPE_ANGLE);
            }
        }
        let ctx = cadmpeg_test_support::service_decode_context();
        let measured = Measurer::new(
            MeasureInput {
                ir,
                fidelity: None,
                geometry_transferred: true,
            },
            MeasureOptions::default(),
        )
        .measure(
            &ctx,
            &Selection {
                all_faces: true,
                ..Selection::default()
            },
        )
        .expect("stored shape measurement");
        let report = serde_json::to_value(measured).expect("measurement JSON");
        let (start, end) = if symmetric {
            (-first / 2.0, first / 2.0)
        } else {
            (-second, first)
        };
        for degrees in [start, end] {
            let radians = degrees.to_radians();
            let inner = [20.0 * radians.cos(), 20.0 * radians.sin(), 0.0];
            let outer = [30.0 * radians.cos(), 30.0 * radians.sin(), 40.0];
            let low = [inner[0].min(outer[0]), inner[1].min(outer[1]), 0.0];
            let high = [inner[0].max(outer[0]), inner[1].max(outer[1]), 40.0];
            assert!(
                report["faces"]
                    .as_array()
                    .expect("faces")
                    .iter()
                    .any(|face| {
                        if !face["face"]
                            .as_str()
                            .is_some_and(|id| id.contains("Revolution:Shape:"))
                        {
                            return false;
                        }
                        if face["carrier"]["kind"] != "plane"
                            || !face["measures"]["area"]["value"]
                                .as_f64()
                                .is_some_and(|area| {
                                    (area - 400.0).abs() < EPS_RECIPE_END_FACE_AREA_MM2
                                })
                        {
                            return false;
                        }
                        let bbox = &face["measures"]["bbox"];
                        [("min", low), ("max", high)]
                            .into_iter()
                            .all(|(bound, expected)| {
                                ["x", "y", "z"]
                                    .into_iter()
                                    .zip(expected)
                                    .all(|(key, value)| {
                                        bbox[bound][key].as_f64().is_some_and(|actual| {
                                            (actual - value).abs() < EPS_RECIPE_END_FACE_MM
                                        })
                                    })
                            })
                    }),
                "{file}: no stored radial end face at {degrees} degrees"
            );
        }
    }
}
