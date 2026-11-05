// SPDX-License-Identifier: Apache-2.0
//! Explicit edge parameter ranges.
#![allow(clippy::unwrap_used)]

use cadmpeg_ir::codec::{
    write::{target::TargetRequest, EncodeInput, Encoder},
    Codec, DecodeOptions,
};

const PARAMETER_TOLERANCE: f64 = 1.0e-9;
use std::io::Cursor;

#[test]
fn linear_edge_parameter_range_is_authored_as_a_bounded_curve() {
    let mut ir = cadmpeg_ir::examples::unit_cube().unwrap();
    for edge in &mut ir.model.edges[1..] {
        edge.set_param_range(None);
    }
    let id = ir.model.edges[0].id.clone();
    let plan = crate::SldprtCodec
        .plan(EncodeInput::new(&ir, None), TargetRequest::Inherit)
        .unwrap();
    assert!(!plan
        .report()
        .losses
        .iter()
        .any(|loss| loss.message.contains(id.as_str()) && loss.message.contains("param_range")));
    let mut bytes = Vec::new();
    plan.write_to(&mut bytes).unwrap();
    let scan = crate::test_support::container::scan(&bytes);
    let partition = crate::container::select_active_parasolid_site(&scan)
        .unwrap()
        .section
        .payload();
    assert!(partition.windows(2).any(|pair| pair == [0x00, 0x85]));
    let written = crate::SldprtCodec
        .decode(&mut Cursor::new(bytes), &DecodeOptions::default())
        .unwrap();
    assert!(written
        .ir()
        .model
        .edges
        .iter()
        .any(|edge| edge.param_range().map(cadmpeg_ir::units::FiniteVector::get) == Some([0.0, 10.0])));
}

#[test]
fn circular_edge_parameter_range_survives_synthesis() {
    let source = include_bytes!("../../../tests/golden/fixtures/authored_complete_cylinder.sldprt");
    let input = crate::SldprtCodec
        .decode(&mut Cursor::new(source), &DecodeOptions::default())
        .unwrap();
    let (edge, range) = input
        .ir()
        .model
        .edges
        .iter()
        .find_map(|edge| {
            let curve = input
                .ir()
                .model
                .curves
                .iter()
                .find(|curve| Some(&curve.id) == edge.curve())?;
            if matches!(
                curve.geometry,
                cadmpeg_ir::geometry::CurveGeometry::Solved(
                    cadmpeg_ir::geometry::SolvedCurveGeometry::Circle(_)
                )
            ) {
                Some((edge.id.clone(), edge.param_range()?.get()))
            } else {
                None
            }
        })
        .unwrap();
    let plan = crate::SldprtCodec
        .plan(EncodeInput::new(input.ir(), None), TargetRequest::Inherit)
        .unwrap();
    assert!(!plan
        .report()
        .losses
        .iter()
        .any(|loss| loss.message.contains(edge.as_str()) && loss.message.contains("param_range")));
    let mut bytes = Vec::new();
    plan.write_to(&mut bytes).unwrap();
    let written = crate::SldprtCodec
        .decode(&mut Cursor::new(bytes), &DecodeOptions::default())
        .unwrap();
    assert!(written
        .ir()
        .model
        .edges
        .iter()
        .filter_map(cadmpeg_ir::topology::Edge::param_range)
        .any(|actual| {
            actual
                .get()
                .into_iter()
                .zip(range)
                .all(|(actual, expected)| (actual - expected).abs() < PARAMETER_TOLERANCE)
        }));
}

#[test]
fn nurbs_edge_parameter_range_survives_synthesis() {
    use cadmpeg_ir::geometry::{
        nurbs::{NurbsCurve, NurbsPoles3},
        CurveGeometry, SolvedCurveGeometry,
    };
    use cadmpeg_ir::math::Point3;
    let mut ir = cadmpeg_ir::examples::unit_cube().unwrap();
    for edge in &mut ir.model.edges[1..] {
        edge.set_param_range(None);
    }
    let poles = NurbsPoles3::from_lanes(
        vec![Point3::new(0.0, 0.0, 0.0), Point3::new(10.0, 0.0, 0.0)],
        None,
    )
    .unwrap();
    ir.model.curves[0].geometry = CurveGeometry::Solved(SolvedCurveGeometry::Nurbs(
        NurbsCurve::new(1, vec![0.0, 0.0, 10.0, 10.0], poles, false).unwrap(),
    ));
    let id = ir.model.edges[0].id.clone();
    let plan = crate::SldprtCodec
        .plan(EncodeInput::new(&ir, None), TargetRequest::Inherit)
        .unwrap();
    assert!(!plan
        .report()
        .losses
        .iter()
        .any(|loss| loss.message.contains(id.as_str()) && loss.message.contains("param_range")));
    let mut bytes = Vec::new();
    plan.write_to(&mut bytes).unwrap();
    let written = crate::SldprtCodec
        .decode(&mut Cursor::new(bytes), &DecodeOptions::default())
        .unwrap();
    assert!(written.ir().model.edges.iter().any(|edge| edge
        .param_range()
        .map(cadmpeg_ir::units::FiniteVector::get)
        == Some([0.0, 10.0])
        && edge
            .curve()
            .is_some_and(
                |id| written.ir().model.curves.iter().any(|curve| &curve.id == id
                    && matches!(
                        curve.geometry,
                        CurveGeometry::Solved(SolvedCurveGeometry::Nurbs(_))
                    ))
            )));
}
