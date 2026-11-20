// SPDX-License-Identifier: Apache-2.0
//! Source-less generation writes what it can encode and charges a loss naming
//! each item it drops or degrades.

use std::io::Cursor;

use cadmpeg_ir::codec::write::{target::TargetRequest, EncodeInput, Encoder};
use cadmpeg_ir::codec::{Codec, DecodeOptions};
use cadmpeg_ir::document::CadIr;
use cadmpeg_ir::geometry::{Curve, CurveGeometry, SolvedCurveGeometry};
use cadmpeg_ir::ids::CurveId;
use cadmpeg_ir::math::{Point3, Vector3};
use cadmpeg_ir::report::export::ExportReport;
use cadmpeg_ir::topology::{EdgeCarrier, ShellMember};

use crate::loss::F3dLossCode;
use crate::test_support::assert_written_with_loss;
use crate::test_support::native_test::{f3d_native, f3d_native_mut};
use crate::test_support::smbh_geometry_test::{
    synthetic_geometry_smbh, synthetic_two_body_smbh, synthetic_wire_body_smbh,
};
use crate::test_support::smbh_pcurves_test::synthetic_geometry_with_pcurve_smbh;
use crate::test_support::zip_test::f3d_with_smbh;
use crate::F3dCodec;

fn source_less(smbh: &[u8]) -> CadIr {
    let decoded = F3dCodec
        .decode(
            &mut Cursor::new(f3d_with_smbh(smbh)),
            &DecodeOptions::default(),
        )
        .expect("generated fixture decodes");
    let (mut ir, _, _) = decoded.into_parts();
    ir.source = None;
    ir
}

fn write(ir: &CadIr) -> (ExportReport, CadIr) {
    let mut bytes = Vec::new();
    let report = F3dCodec
        .plan(EncodeInput::new(ir, None), TargetRequest::Inherit)
        .and_then(|plan| plan.write_to(&mut bytes))
        .expect("the writer writes what it can encode");
    let decoded = F3dCodec
        .decode(&mut Cursor::new(bytes), &DecodeOptions::default())
        .expect("the written file decodes");
    crate::test_support::assert_decodes_back(&report, decoded.report());
    (report, decoded.into_parts().0)
}

fn charges(report: &ExportReport, code: &str) -> bool {
    report
        .losses
        .iter()
        .any(|loss| loss.code.to_string() == code)
}

#[test]
fn a_document_with_no_geometry_writes_an_empty_design() {
    let (report, decoded) = write(&CadIr::empty());
    assert!(
        charges(&report, "export/output.empty"),
        "{:#?}",
        report.losses
    );
    assert!(decoded.geometry.bodies.is_empty());
    assert!(decoded.geometry.faces.is_empty());
}

#[test]
fn a_body_that_cannot_be_written_is_dropped_with_the_native_records_naming_it() {
    let mut ir = source_less(&synthetic_geometry_smbh());
    assert!(ir.native.namespace("f3d").is_some());
    let body = ir.geometry.bodies[0].id.to_string();
    ir.geometry.bodies[0].kind = cadmpeg_ir::topology::BodyKind::Solid;
    let report = assert_written_with_loss(
        &ir,
        None,
        &F3dLossCode::WriterBodyOmitted.kind().to_string(),
        &body,
    );
    assert!(
        report.losses.iter().any(|loss| {
            loss.code.to_string() == "export/native.records-omitted"
                && loss
                    .message
                    .contains("name topology or geometry the write omits")
        }),
        "{:#?}",
        report.losses
    );
    assert!(
        charges(&report, "export/output.empty"),
        "{:#?}",
        report.losses
    );
}

fn wire_with_vertex_free_edge(curve: SolvedCurveGeometry) -> (CadIr, String) {
    let mut ir = source_less(&synthetic_wire_body_smbh());
    ir.native = cadmpeg_ir::native::Native::default();
    let curve_id = CurveId::mint("generated:test:vertex-free-curve#0").expect("identity grammar");
    ir.geometry.curves.push(Curve {
        parameter_range: None,
        id: curve_id.clone(),
        geometry: CurveGeometry::Solved(curve),
        source_object: None,
        native_ref: None,
    });
    let edge = &mut ir.geometry.edges[0];
    edge.carrier = EdgeCarrier::new(Some(curve_id), None).expect("a curve without a range");
    edge.ends = None;
    let id = edge.id.to_string();
    let used = ir
        .geometry
        .edges
        .iter()
        .flat_map(|edge| edge.vertices().cloned().collect::<Vec<_>>())
        .chain(
            ir.geometry
                .shells
                .iter()
                .flat_map(|shell| shell.free_vertices().cloned().collect::<Vec<_>>()),
        )
        .collect::<std::collections::HashSet<_>>();
    ir.geometry
        .vertices
        .retain(|vertex| used.contains(&vertex.id));
    let points = ir
        .geometry
        .vertices
        .iter()
        .map(|vertex| vertex.point.clone())
        .collect::<std::collections::HashSet<_>>();
    ir.geometry
        .points
        .retain(|point| points.contains(&point.id));
    let curves = ir
        .geometry
        .edges
        .iter()
        .filter_map(|edge| edge.curve().cloned())
        .collect::<std::collections::HashSet<_>>();
    ir.geometry
        .curves
        .retain(|curve| curves.contains(&curve.id));
    (ir, id)
}

#[test]
fn a_vertex_free_edge_on_a_closed_curve_is_written_with_one_vertex_at_its_start() {
    let circle = cadmpeg_ir::geometry::analytic::CircleCurve::try_new(
        Point3::new(0.0, 0.0, 0.0),
        Vector3::new(0.0, 0.0, 1.0),
        Vector3::new(1.0, 0.0, 0.0),
        5.0,
    )
    .expect("circle");
    let (ir, _) = wire_with_vertex_free_edge(SolvedCurveGeometry::Circle(circle));
    let edges = ir.geometry.edges.len();
    let (report, decoded) = write(&ir);
    assert!(report.losses.is_empty(), "{:#?}", report.losses);
    assert_eq!(decoded.geometry.edges.len(), edges);
    let closed = decoded
        .geometry
        .edges
        .iter()
        .find(|edge| edge.start().is_some() && edge.start() == edge.end())
        .expect("the closed edge has one vertex at both ends");
    let vertex = decoded
        .geometry
        .vertices
        .iter()
        .find(|vertex| Some(&vertex.id) == closed.start())
        .expect("the vertex is written");
    let position = decoded
        .geometry
        .points
        .iter()
        .find(|point| point.id == vertex.point)
        .expect("the vertex has a point")
        .position()
        .get();
    assert!(
        (position.x - 5.0).abs() < 1e-9 && position.y.abs() < 1e-9,
        "{position:?}"
    );
}

#[test]
fn a_vertex_free_edge_on_an_open_curve_is_omitted_with_a_loss() {
    let line = cadmpeg_ir::geometry::analytic::LineCurve::try_new(
        Point3::new(0.0, 0.0, 0.0),
        Vector3::new(1.0, 0.0, 0.0),
    )
    .expect("line");
    let (ir, edge) = wire_with_vertex_free_edge(SolvedCurveGeometry::Line(line));
    assert!(ir.geometry.shells.iter().any(|shell| shell
        .members
        .iter()
        .any(|member| matches!(member, ShellMember::WireEdge { id } if id.as_str() == edge))));
    let edges = ir.geometry.edges.len();
    assert_written_with_loss(
        &ir,
        None,
        &F3dLossCode::WriterGeometryOmitted.kind().to_string(),
        &edge,
    );
    let (_, decoded) = write(&ir);
    assert_eq!(decoded.geometry.edges.len(), edges - 1);
}

#[test]
fn a_pcurve_with_no_native_form_is_omitted_and_the_face_written() {
    let mut ir = source_less(&synthetic_geometry_with_pcurve_smbh());
    let coedge = ir
        .geometry
        .coedges
        .iter_mut()
        .find(|coedge| !coedge.pcurves.is_empty())
        .expect("the fixture holds a coedge pcurve");
    let duplicate = coedge.pcurves[0].clone();
    coedge.pcurves.push(duplicate);
    let pcurve = coedge.pcurves[0].pcurve.to_string();
    let faces = ir.geometry.faces.len();
    assert_written_with_loss(
        &ir,
        None,
        &F3dLossCode::WriterPcurveOmitted.kind().to_string(),
        &pcurve,
    );
    let (_, decoded) = write(&ir);
    assert_eq!(decoded.geometry.faces.len(), faces);
    assert!(decoded.geometry.pcurves.is_empty());
}

#[test]
fn configurations_without_native_design_records_are_dropped_by_name() {
    let path = concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/tests/golden/fixtures/design_configuration.f3d"
    );
    let bytes = std::fs::read(path).expect("committed fixture");
    let decoded = F3dCodec
        .decode(&mut Cursor::new(bytes), &DecodeOptions::default())
        .expect("fixture decodes");
    let (mut ir, _, _) = decoded.into_parts();
    ir.source = None;
    ir.native = cadmpeg_ir::native::Native::default();
    let configuration = ir.model.configurations[0].id.to_string();
    assert_written_with_loss(
        &ir,
        None,
        &F3dLossCode::WriterConfigurationOmitted.kind().to_string(),
        &configuration,
    );
}

#[test]
fn a_document_asset_is_dropped_by_name() {
    let mut ir = CadIr::empty();
    ir.model.assets.push(
        cadmpeg_ir::assets::Asset::try_new(
            cadmpeg_ir::assets::AssetId::mint("generated:test:asset#0").expect("identity grammar"),
            Some("decal.png".into()),
            None,
            cadmpeg_ir::assets::AssetContent::External {
                uri: cadmpeg_core::text::NonBlankString::new("decal.png").expect("nonblank"),
            },
            None,
        )
        .expect("asset"),
    );
    assert_written_with_loss(
        &ir,
        None,
        &F3dLossCode::WriterAssetOmitted.kind().to_string(),
        "generated:test:asset#0",
    );
}

#[test]
fn a_pole_loop_is_dropped_and_its_face_written() {
    use cadmpeg_ir::topology::{FaceLoops, Loop, LoopBoundary, Point, Vertex};

    let mut ir = cadmpeg_ir::examples::unit_cube().expect("unit cube fixture");
    let face = ir.geometry.faces[0].id.clone();
    let point = cadmpeg_ir::ids::PointId::mint("generated:test:pole-point#0").expect("identity");
    let vertex = cadmpeg_ir::ids::VertexId::mint("generated:test:pole#0").expect("identity");
    let pole = cadmpeg_ir::ids::LoopId::mint("generated:test:pole-loop#0").expect("identity");
    ir.geometry.points.push(Point::new(
        point.clone(),
        cadmpeg_ir::features::FinitePoint3::new(Point3::new(0.5, 0.5, 0.0)).expect("finite"),
        None,
    ));
    ir.geometry.vertices.push(Vertex {
        id: vertex.clone(),
        point,
        tolerance: None,
        native_ref: None,
    });
    ir.geometry.loops.push(Loop {
        id: pole.clone(),
        face: face.clone(),
        boundary: LoopBoundary::Vertex {
            vertex,
            pcurves: Vec::new(),
        },
        native_ref: None,
    });
    match &mut ir.geometry.faces[0].loops {
        FaceLoops::Unspecified { loops } => loops.push(pole.clone()),
        FaceLoops::Classified { inner, .. } => inner.push(pole.clone()),
    }
    let faces = ir.geometry.faces.len();
    assert_written_with_loss(
        &ir,
        None,
        &F3dLossCode::WriterGeometryOmitted.kind().to_string(),
        pole.as_str(),
    );
    let (report, decoded) = write(&ir);
    assert!(
        report
            .losses
            .iter()
            .all(|loss| loss.code != F3dLossCode::WriterBodyOmitted.kind()),
        "{:#?}",
        report.losses
    );
    assert_eq!(decoded.geometry.faces.len(), faces);
}

#[test]
fn an_open_edge_with_no_stated_range_is_written_between_its_vertices() {
    let mut ir = source_less(&synthetic_wire_body_smbh());
    ir.native = cadmpeg_ir::native::Native::default();
    let stated = ir
        .geometry
        .edges
        .iter()
        .map(|edge| {
            (
                edge.id.to_string(),
                edge.param_range().map(cadmpeg_ir::units::FiniteVector::get),
            )
        })
        .collect::<Vec<_>>();
    assert!(stated.iter().all(|(_, range)| range.is_some()));
    for edge in &mut ir.geometry.edges {
        edge.carrier =
            EdgeCarrier::new(edge.curve().cloned(), None).expect("a curve without a range");
    }
    let (report, decoded) = write(&ir);
    assert!(report.losses.is_empty(), "{:#?}", report.losses);
    for ((_, expected), edge) in stated.iter().zip(&decoded.geometry.edges) {
        let [low, high] = expected.expect("stated");
        let [written_low, written_high] = edge.param_range().expect("written").get();
        assert!(
            (low - written_low).abs() < 1e-9 && (high - written_high).abs() < 1e-9,
            "{expected:?} != {:?}",
            [written_low, written_high]
        );
    }
}

#[test]
fn an_open_edge_whose_vertex_misses_its_line_is_written_with_record_invalid() {
    let mut ir = source_less(&synthetic_wire_body_smbh());
    ir.native = cadmpeg_ir::native::Native::default();
    let edge = ir
        .geometry
        .edges
        .iter_mut()
        .find(|edge| {
            edge.ends
                .as_ref()
                .is_some_and(|ends| ends.start != ends.end)
        })
        .expect("the fixture holds an open edge");
    edge.carrier = EdgeCarrier::new(edge.curve().cloned(), None).expect("a curve without a range");
    let id = edge.id.to_string();
    let end = edge.ends.as_ref().expect("an open edge").end.clone();
    let point = ir
        .geometry
        .vertices
        .iter()
        .find(|vertex| vertex.id == end)
        .expect("the end vertex")
        .point
        .clone();
    let row = ir
        .geometry
        .points
        .iter_mut()
        .find(|row| row.id == point)
        .expect("the end point");
    let position = row.position().get();
    // The fixture's line runs along x; the vertex moves 1 mm off it.
    *row = cadmpeg_ir::topology::Point::new(
        point,
        cadmpeg_ir::features::FinitePoint3::new(Point3::new(
            position.x,
            position.y + 1.0,
            position.z,
        ))
        .expect("finite"),
        None,
    );
    assert_written_with_loss(&ir, None, "export/record.invalid", &id);
}

#[test]
fn geometry_of_a_design_with_no_body_is_named_as_not_written() {
    let mut ir = CadIr::empty();
    let line = cadmpeg_ir::geometry::analytic::LineCurve::try_new(
        Point3::new(0.0, 0.0, 0.0),
        Vector3::new(1.0, 0.0, 0.0),
    )
    .expect("line");
    ir.geometry.curves.push(Curve {
        parameter_range: None,
        id: CurveId::mint("generated:test:loose-line#0").expect("identity grammar"),
        geometry: CurveGeometry::Solved(SolvedCurveGeometry::Line(line)),
        source_object: None,
        native_ref: None,
    });
    let report = assert_written_with_loss(
        &ir,
        None,
        &F3dLossCode::WriterGeometryOmitted.kind().to_string(),
        "generated:test:loose-line#0",
    );
    assert!(
        charges(&report, "export/output.empty"),
        "{:#?}",
        report.losses
    );
}

#[test]
fn a_body_without_a_retained_key_takes_a_key_no_other_body_holds() {
    let mut ir = source_less(&synthetic_two_body_smbh([7, 1]));
    let first = ir.geometry.bodies[0].id.to_string();
    {
        let mut native = f3d_native_mut(&mut ir);
        assert_eq!(native.body_native_keys.len(), 2);
        native
            .body_native_keys
            .retain(|key| key.body.as_str() != first);
    }
    let (_, decoded) = write(&ir);
    let keys = f3d_native(&decoded)
        .body_native_keys
        .iter()
        .map(|key| key.asm_body_key)
        .collect::<Vec<_>>();
    assert_eq!(keys.len(), 2, "{keys:?}");
    assert_ne!(keys[0], keys[1], "{keys:?}");
    assert!(
        keys.contains(&Some(1)),
        "the retained key is kept: {keys:?}"
    );
}
