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
    // Add an independently owned use of the retained child carrier. The source
    // composite still declares its constituents physically dependent.
    {
        use cadmpeg_ir::{
            ids::{EdgeId, PointId, VertexId},
            topology::{Edge, EdgeCarrier, Point, Vertex},
        };
        let mut ir = decoded.ir_mut();
        let curve = ir
            .geometry
            .curves
            .iter()
            .find(|curve| curve.id.as_str() == "iges:model:curve#D3")
            .unwrap();
        let curve_id = curve.id.clone();
        let range = curve.parameter_range.unwrap().endpoints();
        let positions = range.map(|parameter| {
            cadmpeg_ir::eval::decode::curve_point(
                cadmpeg_ir::eval::admission::EvaluationAdmission::Standard,
                &curve.geometry,
                parameter,
            )
            .unwrap()
        });
        let vertices = ["start", "end"]
            .map(|suffix| VertexId::mint(format!("test:model:vertex#child-{suffix}")).unwrap());
        for (index, position) in positions.into_iter().enumerate() {
            let point = PointId::mint(format!("test:model:point#child-{index}")).unwrap();
            ir.geometry
                .points
                .push(Point::new(point.clone(), position, None));
            ir.geometry.vertices.push(Vertex {
                id: vertices[index].clone(),
                point,
                tolerance: None,
                native_ref: None,
            });
        }
        let edge = EdgeId::mint("test:model:edge#child").unwrap();
        ir.geometry.shells[0].add_wire_edge(edge.clone());
        ir.geometry.edges.push(Edge {
            id: edge,
            carrier: EdgeCarrier::new(Some(curve_id), Some(range)).unwrap(),
            ends: Some(cadmpeg_ir::topology::EdgeEnds::new(
                vertices[0].clone(),
                vertices[1].clone(),
            )),
            tolerance: None,
            native_ref: None,
        });
    }
    let second_start = decoded
        .ir()
        .geometry
        .edges
        .iter()
        .find(|edge| {
            edge.curve()
                .is_some_and(|curve| curve.as_str() == "iges:model:curve#D3")
        })
        .expect("second Type 102 child edge")
        .ends
        .as_ref()
        .unwrap()
        .start
        .clone();
    {
        let mut ir = decoded.ir_mut();
        let composite = ir
            .geometry
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
            .geometry
            .edges
            .iter_mut()
            .find(|edge| {
                edge.curve()
                    .is_some_and(|curve| curve.as_str() == "iges:model:curve#D5")
            })
            .expect("Type 102 composite edge");
        let previous_end = composite_edge.ends.as_ref().unwrap().end.clone();
        composite_edge.ends.as_mut().unwrap().end = second_start;
        ir.geometry.shells[0].add_free_vertex(previous_end);
    }
    decoded
}

#[test]
fn encode_drops_a_reversed_composite_with_shared_wire_vertex() {
    let decoded = reversed_composite_with_shared_vertex();
    let validation =
        cadmpeg_ir::validate_neutral(decoded.ir(), Vec::new()).expect("validate the neutral model");
    assert!(validation.is_ok(), "{:#?}", validation.findings);
    let plan = plan_at(IgesVersion::V5_0, decoded.ir(), None).unwrap();
    let losses = &plan.report().losses;
    assert!(
        losses.iter().any(|loss| {
            loss.code.to_string() == "iges/writer.body-omitted"
                && loss.message.contains("shared wire vertex")
        }),
        "{losses:#?}"
    );
}

#[test]
fn encode_drops_the_wire_beside_an_orphan_vertex_and_names_the_vertex() {
    let mut decoded = reversed_composite_with_shared_vertex();
    decoded.ir_mut().geometry.shells[0]
        .members
        .retain(|member| !matches!(member, cadmpeg_ir::topology::ShellMember::FreeVertex { .. }));
    let plan = plan_at(IgesVersion::V5_0, decoded.ir(), None).unwrap();
    let losses = &plan.report().losses;
    assert!(
        losses.iter().any(|loss| {
            loss.code.to_string() == "iges/writer.body-omitted"
                && loss.message.contains("no wire or face owner")
        }),
        "{losses:#?}"
    );
}

#[test]
fn encode_reverses_a_composite_constituent_as_a_directed_type_102_child() {
    let mut decoded = reversed_composite_with_shared_vertex();
    {
        let mut ir = decoded.ir_mut();
        let source = ir
            .geometry
            .edges
            .iter()
            .find(|edge| {
                edge.curve()
                    .is_some_and(|curve| curve.as_str() == "iges:model:curve#D5")
            })
            .expect("Type 102 composite edge")
            .ends
            .as_ref()
            .unwrap()
            .end
            .clone();
        let mut detached = ir
            .geometry
            .vertices
            .iter()
            .find(|vertex| vertex.id == source)
            .expect("composite end vertex")
            .clone();
        detached.id = "cadir:model:vertex#detached-composite-end"
            .try_into()
            .expect("vertex ID");
        let detached_id = detached.id.clone();
        ir.geometry.vertices.push(detached);
        ir.geometry
            .edges
            .iter_mut()
            .find(|edge| {
                edge.curve()
                    .is_some_and(|curve| curve.as_str() == "iges:model:curve#D5")
            })
            .expect("Type 102 composite edge")
            .ends
            .as_mut()
            .unwrap()
            .end = detached_id;
    }
    let plan = plan_at(IgesVersion::V5_0, decoded.ir(), None)
        .expect("reversed Type 102 child is writable");
    let mut written = Vec::new();
    plan.write_to(&mut written).unwrap();
    let round_trip = IgesCodec
        .decode(&mut Cursor::new(written), &DecodeOptions::default())
        .unwrap();
    assert!(round_trip.ir().geometry.curves.iter().any(|curve| {
        matches!(curve.geometry.solved(), Some(SolvedCurveGeometry::Line(line_curve))
        if {
            let origin = line_curve.origin().get();
            let direction = *line_curve.direction().as_raw();
            same_float(origin.x, 2.0) && same_float(direction.x, -1.0)
        })
    }));
    let validation =
        cadmpeg_ir::validate_neutral(round_trip.ir(), round_trip.report().losses.clone())
            .expect("validate the neutral model");
    assert!(validation.is_ok(), "{:#?}", validation.findings);
}

#[test]
fn synthesized_wire_refuses_shared_vertex_identity() {
    let decoded = IgesCodec
        .decode(&mut Cursor::new(line_file(0)), &DecodeOptions::default())
        .expect("source line");
    let mut ir = decoded.ir().clone();
    let mut second = ir.geometry.edges[0].clone();
    second.id = "cadir:model:edge#parallel".try_into().expect("edge ID");
    ir.geometry.shells[0].add_wire_edge(second.id.clone());
    ir.geometry.edges.push(second);
    ir.geometry
        .edges
        .sort_by(|left, right| left.id.cmp(&right.id));
    let validation =
        cadmpeg_ir::validate_neutral(&ir, Vec::new()).expect("validate the neutral model");
    assert!(validation.is_ok(), "{:#?}", validation.findings);
    assert_eq!(
        ir.geometry.edges[0].ends.as_ref().unwrap().start,
        ir.geometry.edges[1].ends.as_ref().unwrap().start
    );

    let error = crate::writer::synthesize(
        &ir,
        IgesVersion::V5_3,
        cadmpeg_core::decode::ResourceLimits::desktop(),
    )
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
    ir.geometry.bodies[0].name = Some("Owned wire".into());
    let validation =
        cadmpeg_ir::validate_neutral(&ir, Vec::new()).expect("validate the neutral model");
    assert!(validation.is_ok(), "{:#?}", validation.findings);

    let error = crate::writer::synthesize(
        &ir,
        IgesVersion::V5_3,
        cadmpeg_core::decode::ResourceLimits::desktop(),
    )
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
    ir.geometry.edges.clear();
    let error = crate::writer::synthesize(
        &ir,
        IgesVersion::V5_3,
        cadmpeg_core::decode::ResourceLimits::desktop(),
    )
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
    ir.geometry.bodies[0].color = cadmpeg_ir::topology::Color::new(1.0, 0.0, 0.0, 1.0);
    ir.geometry.bodies[0].visible = Some(false);
    let generated = crate::writer::synthesize(
        &ir,
        IgesVersion::V5_3,
        cadmpeg_core::decode::ResourceLimits::desktop(),
    )
    .expect("synthesis");
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
    assert_eq!(round_trip.ir().geometry.bodies[0].color, None);
    assert_eq!(round_trip.ir().geometry.bodies[0].visible, None);
}
