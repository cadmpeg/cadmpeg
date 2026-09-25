// SPDX-License-Identifier: Apache-2.0

use crate::loss::IgesLossCode;
use crate::test_support::plan_at;
use crate::test_support::test_curves_and_surfaces::{composite_curve_with_join_gap, line_file};
use crate::writer::same_float;
use crate::{IgesCodec, IgesVersion};
use cadmpeg_ir::codec::{Codec, DecodeOptions};
use cadmpeg_ir::geometry::{CurveGeometry, SolvedCurveGeometry};
use cadmpeg_test_support::EditableDecodeResult;
use std::io::Cursor;

fn reversed_composite_with_shared_vertex() -> EditableDecodeResult {
    let decoded = IgesCodec
        .decode(
            &mut Cursor::new(composite_curve_with_join_gap(0.001_001)),
            &DecodeOptions::default(),
        )
        .unwrap();
    let mut decoded = EditableDecodeResult::from(decoded);
    let second_start = decoded
        .ir()
        .model
        .edges
        .iter()
        .find(|edge| {
            edge.curve()
                .is_some_and(|curve| curve.as_str() == "iges:model:curve#D3")
        })
        .expect("second Type 102 child edge")
        .start
        .clone();
    {
        let mut ir = decoded.ir_mut();
        let composite = ir
            .model
            .curves
            .iter_mut()
            .find(|curve| curve.id.as_str() == "iges:model:curve#D5")
            .expect("Type 102 composite curve");
        let CurveGeometry::Solved(SolvedCurveGeometry::Composite { segments, .. }) =
            &mut composite.geometry
        else {
            panic!("expected retained Type 102 composite geometry");
        };
        segments[1].same_sense = false;
        let composite_edge = ir
            .model
            .edges
            .iter_mut()
            .find(|edge| {
                edge.curve()
                    .is_some_and(|curve| curve.as_str() == "iges:model:curve#D5")
            })
            .expect("Type 102 composite edge");
        let previous_end = composite_edge.end.clone();
        composite_edge.end = second_start;
        ir.model.shells[0].add_free_vertex(previous_end);
    }
    decoded
}

#[test]
fn encode_refuses_reversed_composite_with_shared_wire_vertex() {
    let decoded = reversed_composite_with_shared_vertex();
    let validation = cadmpeg_ir::validate_neutral(decoded.ir(), Vec::new());
    assert!(validation.is_ok(), "{:#?}", validation.findings);
    let Err(error) = plan_at(IgesVersion::V5_0, decoded.ir(), None) else {
        panic!("shared wire vertex was accepted");
    };
    assert!(matches!(error, cadmpeg_core::CodecError::NotImplemented(_)));
    assert!(error.to_string().contains("shared wire vertex"), "{error}");
}

#[test]
fn encode_reports_orphan_wire_vertex_as_malformed() {
    let mut decoded = reversed_composite_with_shared_vertex();
    decoded.ir_mut().model.shells[0]
        .edit_topology(|_, _, free_vertices| free_vertices.clear())
        .expect("wire edge keeps shell nonempty");
    let Err(error) = plan_at(IgesVersion::V5_0, decoded.ir(), None) else {
        panic!("orphan vertex was accepted");
    };
    assert!(matches!(error, cadmpeg_core::CodecError::Malformed(_)));
    assert!(
        error.to_string().contains("no wire or face owner"),
        "{error}"
    );
}

#[test]
fn encode_reverses_a_composite_constituent_as_a_directed_type_102_child() {
    let mut decoded = reversed_composite_with_shared_vertex();
    {
        let mut ir = decoded.ir_mut();
        let source = ir
            .model
            .edges
            .iter()
            .find(|edge| {
                edge.curve()
                    .is_some_and(|curve| curve.as_str() == "iges:model:curve#D5")
            })
            .expect("Type 102 composite edge")
            .end
            .clone();
        let mut detached = ir
            .model
            .vertices
            .iter()
            .find(|vertex| vertex.id == source)
            .expect("composite end vertex")
            .clone();
        detached.id = "cadir:model:vertex#detached-composite-end"
            .try_into()
            .expect("vertex ID");
        let detached_id = detached.id.clone();
        ir.model.vertices.push(detached);
        ir.model
            .edges
            .iter_mut()
            .find(|edge| {
                edge.curve()
                    .is_some_and(|curve| curve.as_str() == "iges:model:curve#D5")
            })
            .expect("Type 102 composite edge")
            .end = detached_id;
    }
    let plan = plan_at(IgesVersion::V5_0, decoded.ir(), None)
        .expect("reversed Type 102 child is writable");
    let mut written = Vec::new();
    plan.write_to(&mut written).unwrap();
    let round_trip = IgesCodec
        .decode(&mut Cursor::new(written), &DecodeOptions::default())
        .unwrap();
    assert!(round_trip.ir().model.curves.iter().any(|curve| {
        matches!(curve.geometry.solved(), Some(SolvedCurveGeometry::Line(line_curve))
        if {
            let origin = line_curve.origin().get();
            let direction = *line_curve.direction().as_raw();
            same_float(origin.x, 2.0) && same_float(direction.x, -1.0)
        })
    }));
    let validation =
        cadmpeg_ir::validate_neutral(round_trip.ir(), round_trip.report().losses.clone());
    assert!(validation.is_ok(), "{:#?}", validation.findings);
}

#[test]
fn synthesized_wire_refuses_shared_vertex_identity() {
    let decoded = IgesCodec
        .decode(&mut Cursor::new(line_file(0)), &DecodeOptions::default())
        .expect("source line");
    let mut ir = decoded.ir().clone();
    let mut second = ir.model.edges[0].clone();
    second.id = "cadir:model:edge#parallel".try_into().expect("edge ID");
    ir.model.shells[0].add_wire_edge(second.id.clone());
    ir.model.edges.push(second);
    ir.model.edges.sort_by(|left, right| left.id.cmp(&right.id));
    let validation = cadmpeg_ir::validate_neutral(&ir, Vec::new());
    assert!(validation.is_ok(), "{:#?}", validation.findings);
    assert_eq!(ir.model.edges[0].start, ir.model.edges[1].start);

    let error = crate::writer::synthesize(&ir, IgesVersion::V5_3)
        .err()
        .expect("shared wire vertex must be refused");
    assert!(matches!(error, cadmpeg_core::CodecError::NotImplemented(_)));
    assert!(error.to_string().contains("shared wire vertex"), "{error}");
}

#[test]
fn synthesized_owned_wire_refuses_unrepresented_body() {
    let decoded = IgesCodec
        .decode(&mut Cursor::new(line_file(0)), &DecodeOptions::default())
        .expect("source line");
    let mut ir = decoded.ir().clone();
    ir.model.bodies[0].name = Some("Owned wire".into());
    let validation = cadmpeg_ir::validate_neutral(&ir, Vec::new());
    assert!(validation.is_ok(), "{:#?}", validation.findings);

    let error = crate::writer::synthesize(&ir, IgesVersion::V5_3)
        .err()
        .expect("owned wire body must be refused");
    assert!(matches!(error, cadmpeg_core::CodecError::NotImplemented(_)));
    assert!(error.to_string().contains("owned wire body"), "{error}");
}

#[test]
fn synthesized_wire_reports_missing_edge_as_malformed() {
    let decoded = IgesCodec
        .decode(&mut Cursor::new(line_file(0)), &DecodeOptions::default())
        .expect("source line");
    let mut ir = decoded.ir().clone();
    ir.model.edges.clear();
    let error = crate::writer::synthesize(&ir, IgesVersion::V5_3)
        .err()
        .expect("missing edge must be refused");
    assert!(matches!(error, cadmpeg_core::CodecError::Malformed(_)));
    assert!(error.to_string().contains("missing edge"), "{error}");
}

#[test]
fn synthesized_free_geometry_reports_unowned_body_presentation() {
    let decoded = IgesCodec
        .decode(&mut Cursor::new(line_file(0)), &DecodeOptions::default())
        .expect("source line");
    let mut ir = decoded.ir().clone();
    ir.model.bodies[0].color = cadmpeg_ir::topology::Color::new(1.0, 0.0, 0.0, 1.0);
    ir.model.bodies[0].visible = Some(false);
    let generated = crate::writer::synthesize(&ir, IgesVersion::V5_3).expect("synthesis");
    assert!(generated.losses.iter().any(|loss| {
        loss.code == IgesLossCode::WriterBodyColorNotRepresented.kind()
            && loss.message.contains("no owning Directory Entry")
    }));
    assert!(generated.losses.iter().any(|loss| {
        loss.code == IgesLossCode::WriterBodyVisibilityNotRepresented.kind()
            && loss.message.contains("no owning Directory Entry")
    }));
    let round_trip = IgesCodec
        .decode(&mut Cursor::new(generated.bytes), &DecodeOptions::default())
        .expect("generated IGES decodes");
    assert_eq!(round_trip.ir().model.bodies[0].color, None);
    assert_eq!(round_trip.ir().model.bodies[0].visible, None);
}
