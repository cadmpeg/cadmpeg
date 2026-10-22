// SPDX-License-Identifier: Apache-2.0
//! Placement applies to free-vertex and body-bound mesh output as well as Breps.

use crate::loss::RhinoLossCode;
use crate::{RhinoArchiveVersion, RhinoCodec};
use cadmpeg_ir::codec::write::target::TargetRequest;
use cadmpeg_ir::codec::write::{EncodeInput, Encoder};
use cadmpeg_ir::codec::{Codec, DecodeOptions};
use cadmpeg_ir::document::CadIr;
use cadmpeg_ir::features::FinitePoint3;
use cadmpeg_ir::ids::{PointId, RegionId, ShellId, VertexId};
use cadmpeg_ir::math::{Point3, Vector3};
use cadmpeg_ir::report::export::ExportReport;
use cadmpeg_ir::tessellation::{Tessellation, TessellationId, TessellationMesh};
use cadmpeg_ir::topology::{Body, Point, Region, Shell, Vertex};
use cadmpeg_ir::transform::Transform;
use serde_json::json;
use std::io::Cursor;

fn rigid() -> Transform {
    Transform::affine([
        [0.0, -1.0, 0.0, 10.0],
        [1.0, 0.0, 0.0, -2.0],
        [0.0, 0.0, 1.0, 3.0],
    ])
    .expect("quarter turn about z plus translation")
}

fn nonrigid() -> Transform {
    Transform::affine([
        [2.0, 0.0, 0.0, 10.0],
        [0.0, 1.0, 0.0, -2.0],
        [0.0, 0.0, 1.0, 3.0],
    ])
    .expect("finite scaling plus translation")
}

fn body(transform: Transform) -> Body {
    serde_json::from_value(json!({"id":"cadir:model:body#placed", "kind":"general", "regions":[],
        "transform":transform,"name":"placed geometry","color":{"r":1,"g":0,"b":0,"a":1},"visible":false})).expect("body")
}

fn free_vertices(transform: Transform) -> CadIr {
    let mut ir = CadIr::empty();
    let mut body = body(transform);
    let region: RegionId = "cadir:model:region#placed"
        .try_into()
        .expect("region identity");
    let shell: ShellId = "cadir:model:shell#placed"
        .try_into()
        .expect("shell identity");
    body.regions = vec![region.clone()];
    ir.model.regions.push(Region {
        id: region.clone(),
        body: body.id.clone(),
        shells: vec![shell.clone()],
    });
    let mut vertices = Vec::new();
    for (index, position) in [Point3::new(1.0, 2.0, 3.0), Point3::new(-2.0, 1.0, 4.0)]
        .into_iter()
        .enumerate()
    {
        let vertex: VertexId = format!("cadir:model:vertex#placed.{index}")
            .try_into()
            .expect("vertex identity");
        let point: PointId = format!("cadir:model:point#placed.{index}")
            .try_into()
            .expect("point identity");
        vertices.push(vertex.clone());
        ir.model.vertices.push(Vertex {
            id: vertex,
            point: point.clone(),
            tolerance: None,
        });
        ir.model.points.push(Point::new(
            point,
            FinitePoint3::new(position).expect("finite point"),
            None,
        ));
    }
    ir.model
        .shells
        .push(Shell::with_free_vertices(shell, region, vertices).expect("free vertices"));
    ir.model.bodies.push(body);
    ir
}

fn mesh(transform: Transform) -> CadIr {
    let mut ir = CadIr::empty();
    let body = body(transform);
    let mesh = Tessellation::new(
        TessellationId::mint("cadir:model:tessellation#placed").expect("mesh identity"),
        TessellationMesh::from_list_lanes(
            vec![
                Point3::new(1.0, 0.0, 0.0),
                Point3::new(0.0, 2.0, 0.0),
                Point3::new(0.0, 0.0, 1.0),
            ],
            vec![[0, 1, 2]],
            Some(vec![Vector3::new(1.0, 0.0, 0.0); 3]),
        )
        .expect("mesh lanes"),
        Vec::new(),
    )
    .expect("mesh")
    .with_body(Some(body.id.clone()));
    ir.model.bodies.push(body);
    ir.model.tessellations.push(mesh);
    ir
}

fn write_read(ir: &CadIr) -> (CadIr, ExportReport) {
    let mut bytes = Vec::new();
    let report = RhinoCodec
        .plan(
            EncodeInput::new(ir, None),
            TargetRequest::Explicit(RhinoArchiveVersion::V8.descriptor().id.as_str()),
        )
        .and_then(|plan| plan.write_to(&mut bytes))
        .expect("placed geometry writes");
    let decoded = RhinoCodec
        .decode(&mut Cursor::new(bytes), &DecodeOptions::default())
        .expect("placed geometry decodes");
    (decoded.ir().clone(), report)
}

fn assert_transform_drop(report: &ExportReport) {
    assert!(
        report
            .losses
            .iter()
            .any(|loss| loss.code == RhinoLossCode::WriterBodyTransformNotApplied.kind()),
        "nonrigid placement must report its loss"
    );
}

#[test]
fn general_free_vertices_apply_rigid_rotation_translation_once() {
    let ir = free_vertices(rigid());
    let (written, report) = write_read(&ir);
    let points: Vec<_> = written
        .model
        .points
        .iter()
        .map(|point| point.position().get())
        .collect();
    assert_eq!(
        points,
        [Point3::new(8.0, -1.0, 6.0), Point3::new(9.0, -4.0, 7.0)]
    );
    assert_eq!(written.model.bodies.len(), 1);
    assert_eq!(written.model.vertices.len(), 2);
    assert_eq!(written.model.bodies[0].name, ir.model.bodies[0].name);
    assert_eq!(written.model.bodies[0].color, ir.model.bodies[0].color);
    assert_eq!(written.model.bodies[0].visible, Some(false));
    assert!(report
        .losses
        .iter()
        .all(|loss| loss.code != RhinoLossCode::WriterBodyTransformNotApplied.kind()));
}

#[test]
fn general_free_vertices_keep_local_geometry_when_nonrigid_placement_drops() {
    let ir = free_vertices(nonrigid());
    let (written, report) = write_read(&ir);
    let points: Vec<_> = written
        .model
        .points
        .iter()
        .map(|point| point.position().get())
        .collect();
    assert_eq!(
        points,
        ir.model
            .points
            .iter()
            .map(|point| point.position().get())
            .collect::<Vec<_>>()
    );
    assert_eq!(written.model.vertices.len(), 2);
    assert_transform_drop(&report);
}

#[test]
fn body_bound_mesh_applies_rigid_rotation_translation_and_rotates_normals() {
    let ir = mesh(rigid());
    let (written, report) = write_read(&ir);
    assert_eq!(written.model.tessellations.len(), 1);
    let mesh = &written.model.tessellations[0];
    assert_eq!(
        mesh.vertices()
            .into_iter()
            .map(FinitePoint3::get)
            .collect::<Vec<_>>(),
        [
            Point3::new(10.0, -1.0, 3.0),
            Point3::new(8.0, -2.0, 3.0),
            Point3::new(10.0, -2.0, 4.0)
        ]
    );
    assert_eq!(mesh.triangles(), ir.model.tessellations[0].triangles());
    assert!(mesh
        .vertex_normals()
        .iter()
        .all(|normal| normal.get() == Vector3::new(0.0, 1.0, 0.0)));
    assert!(report
        .losses
        .iter()
        .all(|loss| loss.code != RhinoLossCode::WriterBodyTransformNotApplied.kind()));
}

#[test]
fn body_bound_mesh_keeps_local_geometry_and_normals_when_nonrigid_placement_drops() {
    let ir = mesh(nonrigid());
    let (written, report) = write_read(&ir);
    assert_eq!(written.model.tessellations.len(), 1);
    let mesh = &written.model.tessellations[0];
    assert_eq!(mesh.vertices(), ir.model.tessellations[0].vertices());
    assert_eq!(
        mesh.vertex_normals(),
        ir.model.tessellations[0].vertex_normals()
    );
    assert_eq!(mesh.triangles(), ir.model.tessellations[0].triangles());
    assert_transform_drop(&report);
}
