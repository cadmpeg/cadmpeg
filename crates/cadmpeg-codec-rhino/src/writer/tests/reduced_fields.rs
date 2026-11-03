// SPDX-License-Identifier: Apache-2.0
//! Every [`super::super::target::REDUCED_FIELDS`] row stores its reduced
//! value: a value the storage cannot represent re-decodes as the stored
//! value, different from the input, within one quantum.

use std::io::Cursor;

use cadmpeg_ir::codec::write::{target::TargetRequest, EncodeInput, Encoder};
use cadmpeg_ir::codec::{Codec, DecodeOptions};
use cadmpeg_ir::document::CadIr;
use cadmpeg_ir::math::{Point3, Vector3};
use cadmpeg_ir::report::export::ExportReport;

use super::super::target::{ReducedStorage, REDUCED_FIELDS};
use crate::{RhinoArchiveVersion, RhinoCodec};

fn f32_stored(value: f64) -> f64 {
    f64::from(value as f32)
}

fn byte_stored(value: f32) -> f32 {
    ((f64::from(value) * 255.0).round() / 255.0) as f32
}

/// One triangle whose first vertex and first normal the storages cannot hold,
// plus a free-vertex point body carrying an irreducible display color.
fn quantized_document() -> CadIr {
    use cadmpeg_ir::tessellation::{Tessellation, TessellationMesh};
    let mut ir = CadIr::empty();
    ir.model.tessellations.push(
        Tessellation::new(
            cadmpeg_ir::tessellation::TessellationId::mint("cadir:model:tessellation#reduced")
                .expect("valid identity"),
            TessellationMesh::from_list_lanes(
                vec![
                    Point3::new(0.1, 0.0, 0.0),
                    Point3::new(2.0, 0.0, 0.0),
                    Point3::new(0.0, 3.0, 0.0),
                ],
                vec![[0, 1, 2]],
                Some(vec![
                    Vector3::new(0.1, 0.0, 1.0),
                    Vector3::new(0.0, 0.0, 1.0),
                    Vector3::new(0.0, 0.0, 1.0),
                ]),
            )
            .expect("normals cover the mesh"),
            Vec::new(),
        )
        .expect("valid tessellation"),
    );
    let body_id: cadmpeg_ir::ids::BodyId = "cadir:model:body#reduced".try_into().expect("identity");
    let region_id: cadmpeg_ir::ids::RegionId =
        "cadir:model:region#reduced".try_into().expect("identity");
    let shell_id: cadmpeg_ir::ids::ShellId =
        "cadir:model:shell#reduced".try_into().expect("identity");
    let vertex_id =
        cadmpeg_ir::ids::VertexId::mint("cadir:model:vertex#reduced").expect("identity grammar");
    let point_id =
        cadmpeg_ir::ids::PointId::mint("cadir:model:point#reduced").expect("identity grammar");
    let color = cadmpeg_ir::topology::Color::new(0.5001, 0.0, 1.0, 1.0).expect("valid color");
    ir.model.bodies.push(cadmpeg_ir::topology::Body {
        native_ref: None,
        id: body_id.clone(),
        kind: cadmpeg_ir::topology::BodyKind::General,
        regions: vec![region_id.clone()],
        transform: None,
        name: None,
        color: Some(color),
        visible: None,
    });
    ir.model.regions.push(cadmpeg_ir::topology::Region {
        native_ref: None,
        id: region_id.clone(),
        body: body_id,
        shells: vec![shell_id.clone()],
    });
    ir.model.shells.push(
        cadmpeg_ir::topology::Shell::new(
            shell_id,
            region_id,
            Vec::new(),
            Vec::new(),
            vec![vertex_id.clone()],
        )
        .expect("free-vertex shell"),
    );
    ir.model.vertices.push(cadmpeg_ir::topology::Vertex {
        native_ref: None,
        id: vertex_id,
        point: point_id.clone(),
        tolerance: None,
    });
    ir.model.points.push(cadmpeg_ir::topology::Point::new(
        point_id,
        cadmpeg_ir::features::FinitePoint3::new(Point3::new(0.0, 0.0, 0.0))
            .expect("finite position"),
        None,
    ));
    ir.model
        .appearances
        .push(cadmpeg_ir::appearance::Appearance {
            id: "cadir:model:appearance#reduced"
                .try_into()
                .expect("identity"),
            native_ref: None,
            name: None,
            asset_guid: None,
            library_id: None,
            visual_guid: None,
            physical_token: None,
            schema: None,
            category: None,
            base_color: Some(color),
            properties: std::collections::BTreeMap::new(),
            textures: Vec::new(),
        });
    ir.finalize();
    ir
}

fn write(ir: &CadIr, version: RhinoArchiveVersion) -> (Vec<u8>, ExportReport) {
    let mut bytes = Vec::new();
    let report = RhinoCodec
        .plan(
            EncodeInput::new(ir, None),
            TargetRequest::Explicit(version.descriptor().id.as_str()),
        )
        .and_then(|plan| plan.write_to(&mut bytes))
        .expect("the document is writable");
    (bytes, report)
}

fn charged(report: &ExportReport, local: &str) -> bool {
    report
        .losses
        .iter()
        .any(|loss| loss.code.local_code() == local)
}

#[test]
fn declared_rows_cover_normals_vertices_and_colors() {
    let rows = REDUCED_FIELDS
        .iter()
        .map(|(path, storage)| (*path, *storage))
        .collect::<Vec<_>>();
    assert!(rows.contains(&(
        "model.tessellations[].vertex_normals[]",
        ReducedStorage::F32
    )));
    assert!(rows.contains(&(
        "model.tessellations[].vertices[]",
        ReducedStorage::F32BelowArchive60
    )));
    assert!(rows.contains(&("model.bodies[].color", ReducedStorage::U8Channel)));
    assert!(rows.contains(&("model.appearances[].base_color", ReducedStorage::U8Channel)));
}

#[test]
fn capacity_row_names_the_chunk_length_field_and_its_helper() {
    use super::super::target::CAPACITY_FIELDS;
    assert_eq!(CAPACITY_FIELDS.len(), 1);
    let row = &CAPACITY_FIELDS[0];
    assert_eq!(row.field, "chunk.declared_length");
    assert_eq!(row.limit, 9_223_372_036_854_775_807);
    assert_eq!(row.helper, "archive_body_len");
}

#[test]
fn mesh_normals_store_binary32_on_every_target() {
    let ir = quantized_document();
    let input = ir.model.tessellations[0].vertex_normals()[0].x;
    assert_ne!(
        f32_stored(input),
        input,
        "the row needs an irreducible value"
    );
    for version in [RhinoArchiveVersion::V5, RhinoArchiveVersion::V8] {
        let (bytes, report) = write(&ir, version);
        assert!(
            charged(&report, "mesh.normal-precision-reduced"),
            "{version:?}: the normal reduction is charged"
        );
        let decoded = RhinoCodec
            .decode(&mut Cursor::new(bytes), &DecodeOptions::default())
            .expect("the mesh decodes");
        let stored = decoded.ir().model.tessellations[0].vertex_normals()[0].x;
        assert_eq!(stored, f32_stored(input), "{version:?}");
        assert_ne!(stored, input, "{version:?}: the storage reduces the row");
    }
}

#[test]
fn mesh_vertices_store_binary32_below_archive_60_only() {
    let ir = quantized_document();
    let input = ir.model.tessellations[0].vertices()[0].x;
    assert_ne!(
        f32_stored(input),
        input,
        "the row needs an irreducible value"
    );

    let (v5, v5_report) = write(&ir, RhinoArchiveVersion::V5);
    assert!(
        charged(&v5_report, "mesh.vertex-precision-reduced"),
        "the archive-50 vertex reduction is charged"
    );
    let decoded = RhinoCodec
        .decode(&mut Cursor::new(v5), &DecodeOptions::default())
        .expect("the archive-50 mesh decodes");
    let stored = decoded.ir().model.tessellations[0].vertices()[0].x;
    assert_eq!(stored, f32_stored(input));
    assert_ne!(stored, input, "archive 50 reduces the row");

    let (v8, v8_report) = write(&ir, RhinoArchiveVersion::V8);
    assert!(
        !charged(&v8_report, "mesh.vertex-precision-reduced"),
        "archive 80 stores f64 vertices and charges nothing"
    );
    let decoded = RhinoCodec
        .decode(&mut Cursor::new(v8), &DecodeOptions::default())
        .expect("the archive-80 mesh decodes");
    assert_eq!(
        decoded.ir().model.tessellations[0].vertices()[0].x,
        input,
        "archive 80 keeps the exact vertex"
    );
}

#[test]
fn body_and_appearance_colors_store_eight_bit_channels() {
    let ir = quantized_document();
    let input = ir.model.bodies[0].color.expect("body color").r();
    assert_ne!(
        byte_stored(input),
        input,
        "the row needs an irreducible value"
    );
    for version in [RhinoArchiveVersion::V5, RhinoArchiveVersion::V8] {
        let (bytes, report) = write(&ir, version);
        assert!(
            charged(&report, "appearance.color-precision-reduced"),
            "{version:?}: the color reduction is charged"
        );
        let decoded = RhinoCodec
            .decode(&mut Cursor::new(bytes), &DecodeOptions::default())
            .expect("the document decodes");
        let body = decoded
            .ir()
            .model
            .bodies
            .iter()
            .find(|body| body.color.is_some())
            .expect("the colored body reappears");
        assert_eq!(
            body.color.expect("body color").r(),
            byte_stored(input),
            "{version:?}"
        );
        assert_ne!(body.color.expect("body color").r(), input, "{version:?}");
        let appearance = decoded
            .ir()
            .model
            .appearances
            .iter()
            .find(|appearance| {
                appearance
                    .base_color
                    .is_some_and(|color| color.r() == byte_stored(input))
            })
            .expect("the quantized appearance color reappears");
        assert_eq!(
            appearance.base_color.expect("appearance color").r(),
            byte_stored(input),
            "{version:?}"
        );
    }
}
