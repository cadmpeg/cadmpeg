// SPDX-License-Identifier: Apache-2.0
//! Source-less edges that store no parameter range.

use std::io::Cursor;

use cadmpeg_ir::codec::write::target::TargetRequest;
use cadmpeg_ir::codec::write::{EncodeInput, Encoder};
use cadmpeg_ir::codec::{Codec, DecodeOptions};
use cadmpeg_ir::geometry::SolvedCurveGeometry;

use crate::F3dCodec;

#[test]
fn generated_source_less_closed_circle_edge_without_range_spans_one_revolution() {
    let source_less = cadmpeg_test_support::solids::solid_cylinder();
    let mut encoded = Vec::new();
    F3dCodec
        .plan(EncodeInput::new(&source_less, None), TargetRequest::Inherit)
        .and_then(|plan| plan.write_to(&mut encoded))
        .expect("source-less cylinder encode");
    let round_trip = F3dCodec
        .decode(&mut Cursor::new(encoded), &DecodeOptions::default())
        .expect("source-less cylinder round trip");
    let model = &round_trip.ir().model;
    let circle_ranges = model
        .edges
        .iter()
        .filter(|edge| edge.start == edge.end)
        .filter(|edge| {
            edge.curve().is_some_and(|curve_id| {
                model.curves.iter().any(|curve| {
                    curve.id == *curve_id
                        && matches!(
                            curve.geometry.solved(),
                            Some(SolvedCurveGeometry::Circle(_))
                        )
                })
            })
        })
        .map(|edge| edge.param_range().map(cadmpeg_ir::units::FiniteVector::get))
        .collect::<Vec<_>>();
    assert_eq!(
        circle_ranges,
        vec![Some([0.0, std::f64::consts::TAU]); 2],
        "each closed circle edge spans one revolution"
    );
}
