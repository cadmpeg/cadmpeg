// SPDX-License-Identifier: Apache-2.0
//! Native partition patch tests.
#![allow(clippy::unwrap_used)]

use std::io::Cursor;

use cadmpeg_ir::codec::{Codec, DecodeOptions};

use crate::container;
use crate::test_support::*;
use crate::SldprtCodec;

#[test]
fn native_patch_edits_compact_counted_nurbs_surface_arrays() {
    use cadmpeg_ir::geometry::SurfaceGeometry;

    let mut bytes = compact_counted_nurbs_surface_carrier(180, 181, 10);
    let carrier = crate::brep::spline::scan_surface_carriers(&bytes)
        .remove(&180)
        .expect("compact NURBS carrier");
    let crate::brep::CarrierGeometry::Surface(SurfaceGeometry::Nurbs(old)) = carrier.geometry
    else {
        panic!("compact NURBS surface");
    };
    let mut new = old.clone();
    new.control_points[3].z = 750.0;
    new.u_knots[2..].fill(2.0);
    new.v_knots[2..].fill(3.0);
    let dirty_slots = [
        f64::from_bits(0x7ff8_0000_0000_0001).to_be_bytes(),
        f64::from_bits(0x7ff8_0000_0000_0002).to_be_bytes(),
    ];

    crate::brep::patch_nurbs_surface(&mut bytes, 0, &old, &new, 0.001)
        .expect("compact NURBS patch");

    let patched = crate::brep::spline::scan_surface_carriers(&bytes)
        .remove(&180)
        .expect("patched compact NURBS carrier");
    let crate::brep::CarrierGeometry::Surface(SurfaceGeometry::Nurbs(patched)) = patched.geometry
    else {
        panic!("patched compact NURBS surface");
    };
    assert_eq!(patched, new);
    for dirty in dirty_slots {
        assert_eq!(
            bytes
                .windows(dirty.len())
                .filter(|window| *window == dirty)
                .count(),
            2
        );
    }
}

#[test]
fn native_patch_edits_nurbs_carriers_beside_untyped_surfaces() {
    use cadmpeg_ir::geometry::{CurveGeometry, SurfaceGeometry};

    let mut body = triangle_body();
    let bridge_offset = body.windows(2).position(|w| w == [0x00, 0x0e]).unwrap();
    body[bridge_offset + 26..bridge_offset + 28].copy_from_slice(&180u16.to_be_bytes());
    let edge = body.windows(2).position(|w| w == [0x00, 0x10]).unwrap();
    body[edge + 24..edge + 26].copy_from_slice(&170u16.to_be_bytes());
    body.extend(nurbs_curve_carrier(170, 171));
    body.extend(nurbs_surface_carrier_with_terminal_knot_slot(180, 181, 10));
    body.extend(bridge(210, 220, 999));
    body.extend(loop_head(220, 230, 210));
    body.extend(coedge(230, 220, 231, 250, 0, 240, false));
    body.extend(coedge(231, 220, 232, 251, 0, 241, false));
    body.extend(coedge(232, 220, 230, 252, 0, 242, false));
    body.extend(edge_use(240, 0));
    body.extend(edge_use(241, 0));
    body.extend(edge_use(242, 0));
    body.extend(vertex_use(250, 260));
    body.extend(vertex_use(251, 261));
    body.extend(vertex_use(252, 262));
    body.extend(world_point(260, [10.0, 0.0, 0.0]));
    body.extend(world_point(261, [11.0, 0.0, 0.0]));
    body.extend(world_point(262, [10.0, 1.0, 0.0]));

    let decoded = SldprtCodec
        .decode(
            &mut Cursor::new(sldprt_with_body(&body)),
            &DecodeOptions::default(),
        )
        .unwrap();
    let mut decoded = cadmpeg_test_support::EditableDecodeResult::from(decoded);
    let (expected_curve, expected_surface) = {
        let mut ir_edit = decoded.ir_mut();
        let curve = ir_edit
            .model
            .curves
            .iter_mut()
            .find_map(|curve| match &mut curve.geometry {
                CurveGeometry::Nurbs(nurbs) => Some(nurbs),
                _ => None,
            })
            .unwrap();
        curve.control_points[1].y = 1_500.0;
        curve.knots[3..].fill(2.0);
        let expected_curve = curve.clone();
        let surface = ir_edit
            .model
            .surfaces
            .iter_mut()
            .find_map(|surface| match &mut surface.geometry {
                SurfaceGeometry::Nurbs(nurbs) => Some(nurbs),
                _ => None,
            })
            .unwrap();
        surface.control_points[3].z = 750.0;
        surface.u_knots[2..].fill(2.0);
        surface.v_knots[2..].fill(3.0);
        let expected_surface = surface.clone();
        (expected_curve, expected_surface)
    };

    let mut encoded = Vec::new();
    crate::test_support::plan_inherited_write(
        decoded.ir(),
        decoded.source_fidelity(),
        &mut encoded,
    )
    .unwrap();
    assert!(crate::container::scan_bytes(&encoded)
        .blocks
        .iter()
        .flat_map(|block| block.ps_streams.iter())
        .any(|stream| stream
            .payload
            .windows(DIRTY_TERMINAL_KNOT.len())
            .any(|window| { window == DIRTY_TERMINAL_KNOT })));
    let regenerated = SldprtCodec
        .decode(&mut Cursor::new(encoded), &DecodeOptions::default())
        .unwrap();

    assert!(regenerated.ir().model.curves.iter().any(
        |curve| matches!(&curve.geometry, CurveGeometry::Nurbs(value) if value == &expected_curve)
    ));
    assert!(regenerated.ir().model.surfaces.iter().any(
        |surface| matches!(&surface.geometry, SurfaceGeometry::Nurbs(value) if value == &expected_surface)
    ));
    assert!(regenerated
        .ir()
        .model
        .surfaces
        .iter()
        .any(|surface| matches!(surface.geometry, SurfaceGeometry::Unknown { .. })));
}

#[test]
fn native_patch_edits_points_without_dropping_untyped_surfaces() {
    use cadmpeg_ir::geometry::SurfaceGeometry;

    let mut body = Vec::new();
    body.extend(bridge(10, 20, 999));
    body.extend(loop_head(20, 30, 10));
    body.extend(coedge(30, 20, 31, 50, 0, 40, false));
    body.extend(coedge(31, 20, 32, 51, 0, 41, false));
    body.extend(coedge(32, 20, 30, 52, 0, 42, false));
    body.extend(edge_use(40, 0));
    body.extend(edge_use(41, 0));
    body.extend(edge_use(42, 0));
    body.extend(vertex_use(50, 60));
    body.extend(vertex_use(51, 61));
    body.extend(vertex_use(52, 62));
    body.extend(world_point(60, [0.0, 0.0, 0.0]));
    body.extend(world_point(61, [1.0, 0.0, 0.0]));
    body.extend(world_point(62, [0.0, 1.0, 0.0]));

    let deltas = parasolid_with_body(
        "deltas body",
        "SCH_SW_33103_11000",
        &line_carrier(800, [0.0, 0.0, 0.0], [1.0, 0.0, 0.0]),
    );
    let mut source = sldprt_with_body(&body);
    source.extend(make_block(0x21, "Contents/Config-0-Deltas", &deltas));
    let decoded = SldprtCodec
        .decode(&mut Cursor::new(source), &DecodeOptions::default())
        .unwrap();
    let mut decoded = cadmpeg_test_support::EditableDecodeResult::from(decoded);
    decoded.ir_mut().model.points[1].position.x = 1_250.0;

    let mut encoded = Vec::new();
    crate::test_support::plan_inherited_write(
        decoded.ir(),
        decoded.source_fidelity(),
        &mut encoded,
    )
    .unwrap();
    let regenerated = SldprtCodec
        .decode(&mut Cursor::new(encoded), &DecodeOptions::default())
        .unwrap();

    assert_eq!(regenerated.ir().model.points[1].position.x, 1_250.0);
    assert!(matches!(
        regenerated.ir().model.surfaces[0].geometry,
        SurfaceGeometry::Unknown { .. }
    ));
    assert_eq!(regenerated.ir().model.faces.len(), 1);
    let written = regenerated
        .source_fidelity()
        .retained_record("sldprt:file:source-image#0")
        .and_then(|record| record.data())
        .unwrap();
    let scan = container::scan_bytes(written);
    assert!(scan.blocks.iter().any(|block| {
        block.section.as_deref() == Some("Contents/Config-0-Deltas") && block.payload == deltas
    }));
}

#[test]
fn native_patch_requires_point_provenance_annotation() {
    let mut body = Vec::new();
    body.extend(bridge(10, 20, 999));
    body.extend(loop_head(20, 30, 10));
    body.extend(coedge(30, 20, 31, 50, 0, 40, false));
    body.extend(coedge(31, 20, 32, 51, 0, 41, false));
    body.extend(coedge(32, 20, 30, 52, 0, 42, false));
    body.extend(edge_use(40, 0));
    body.extend(edge_use(41, 0));
    body.extend(edge_use(42, 0));
    body.extend(vertex_use(50, 60));
    body.extend(vertex_use(51, 61));
    body.extend(vertex_use(52, 62));
    body.extend(world_point(60, [0.0, 0.0, 0.0]));
    body.extend(world_point(61, [1.0, 0.0, 0.0]));
    body.extend(world_point(62, [0.0, 1.0, 0.0]));

    let decoded = SldprtCodec
        .decode(
            &mut Cursor::new(sldprt_with_body(&body)),
            &DecodeOptions::default(),
        )
        .unwrap();
    let mut decoded = cadmpeg_test_support::EditableDecodeResult::from(decoded);
    let point_id = decoded.ir().model.points[1].id.0.clone();
    assert!(decoded
        .source_fidelity()
        .annotations
        .provenance
        .contains_key(&point_id));
    decoded.ir_mut().model.points[1].position.x = 1_250.0;
    decoded
        .source_fidelity_mut()
        .annotations
        .provenance
        .remove(&point_id);

    let error = crate::test_support::plan_inherited_write(
        decoded.ir(),
        decoded.source_fidelity(),
        &mut Vec::new(),
    )
    .unwrap_err();
    assert!(matches!(
        error,
        cadmpeg_core::CodecError::Malformed(message)
            if message.contains("requires provenance annotation") && message.contains(&point_id)
    ));
}

#[test]
fn native_patch_edits_analytic_carriers_beside_untyped_surfaces() {
    use cadmpeg_ir::geometry::{CurveGeometry, SurfaceGeometry};

    let mut body = triangle_body();
    body.extend(line_carrier(70, [0.0, 0.0, 0.0], [1.0, 0.0, 0.0]));
    body.extend(edge_use(40, 70));
    body.extend(bridge(210, 220, 999));
    body.extend(loop_head(220, 230, 210));
    body.extend(coedge(230, 220, 231, 250, 0, 240, false));
    body.extend(coedge(231, 220, 232, 251, 0, 241, false));
    body.extend(coedge(232, 220, 230, 252, 0, 242, false));
    body.extend(edge_use(240, 0));
    body.extend(edge_use(241, 0));
    body.extend(edge_use(242, 0));
    body.extend(vertex_use(250, 260));
    body.extend(vertex_use(251, 261));
    body.extend(vertex_use(252, 262));
    body.extend(world_point(260, [10.0, 0.0, 0.0]));
    body.extend(world_point(261, [11.0, 0.0, 0.0]));
    body.extend(world_point(262, [10.0, 1.0, 0.0]));

    let decoded = SldprtCodec
        .decode(
            &mut Cursor::new(sldprt_with_body(&body)),
            &DecodeOptions::default(),
        )
        .unwrap();
    let mut decoded = cadmpeg_test_support::EditableDecodeResult::from(decoded);
    {
        let mut ir_edit = decoded.ir_mut();
        let plane = ir_edit
            .model
            .surfaces
            .iter_mut()
            .find(|surface| matches!(surface.geometry, SurfaceGeometry::Plane { .. }))
            .unwrap();
        let SurfaceGeometry::Plane { origin, .. } = &mut plane.geometry else {
            unreachable!()
        };
        origin.x = 25.0;
        let line = ir_edit
            .model
            .curves
            .iter_mut()
            .find(|curve| matches!(curve.geometry, CurveGeometry::Line { .. }))
            .unwrap();
        let CurveGeometry::Line { origin, .. } = &mut line.geometry else {
            unreachable!()
        };
        origin.y = 12.0;
    }

    let mut encoded = Vec::new();
    crate::test_support::plan_inherited_write(
        decoded.ir(),
        decoded.source_fidelity(),
        &mut encoded,
    )
    .unwrap();
    let regenerated = SldprtCodec
        .decode(&mut Cursor::new(encoded), &DecodeOptions::default())
        .unwrap();

    assert!(regenerated
        .ir()
        .model
        .surfaces
        .iter()
        .any(|surface| matches!(
            surface.geometry,
            SurfaceGeometry::Plane { origin, .. } if origin.x == 25.0
        )));
    assert!(regenerated
        .ir()
        .model
        .surfaces
        .iter()
        .any(|surface| matches!(surface.geometry, SurfaceGeometry::Unknown { .. })));
    assert!(regenerated.ir().model.curves.iter().any(|curve| matches!(
        curve.geometry,
        CurveGeometry::Line { origin, .. } if origin.y == 12.0
    )));
}

#[test]
fn auxiliary_edit_retains_opaque_partition_payload() {
    use cadmpeg_ir::geometry::SurfaceGeometry;

    let mut body = Vec::new();
    body.extend(bridge(10, 20, 999));
    body.extend(loop_head(20, 30, 10));
    body.extend(coedge(30, 20, 31, 50, 0, 40, false));
    body.extend(coedge(31, 20, 32, 51, 0, 41, false));
    body.extend(coedge(32, 20, 30, 52, 0, 42, false));
    body.extend(edge_use(40, 0));
    body.extend(edge_use(41, 0));
    body.extend(edge_use(42, 0));
    body.extend(vertex_use(50, 60));
    body.extend(vertex_use(51, 61));
    body.extend(vertex_use(52, 62));
    body.extend(world_point(60, [0.0, 0.0, 0.0]));
    body.extend(world_point(61, [1.0, 0.0, 0.0]));
    body.extend(world_point(62, [0.0, 1.0, 0.0]));
    let mut source = sldprt_with_body_and_history(&body);
    source.extend(make_block(
        0x66,
        "Contents/Config-0-Deltas",
        b"opaque-deltas",
    ));
    source.extend(make_block(
        0x67,
        "Contents/Config-0-GhostPartition",
        b"opaque-ghost",
    ));
    source.extend(make_cache_cell(90, "Contents/Config-0-Partition"));
    source.extend(make_cache_cell(100, "Contents/Keywords"));
    let indexed = container::scan_bytes(&source);
    let partition = indexed
        .blocks
        .iter()
        .find(|block| block.section.as_deref() == Some("Contents/Config-0-Partition"))
        .unwrap();
    let keywords = indexed
        .blocks
        .iter()
        .find(|block| block.section.as_deref() == Some("Contents/Keywords"))
        .unwrap();
    let mut directory = make_directory_entry(
        partition.type_id,
        partition.uncomp_sz,
        "Contents/Config-0-Partition",
    );
    directory[26] = 0xab;
    let trailer = directory.len() - 6;
    directory[trailer..trailer + 4].copy_from_slice(&[0x11, 0x22, 0x33, 0x44]);
    source.extend(directory);
    let mut directory =
        make_directory_entry(keywords.type_id, keywords.uncomp_sz, "Contents/Keywords");
    directory[26] = 0xcd;
    let trailer = directory.len() - 6;
    directory[trailer..trailer + 4].copy_from_slice(&[0x11, 0x22, 0x33, 0x44]);
    source.extend(directory);
    let source_scan = container::scan_bytes(&source);
    let source_partition = source_scan
        .blocks
        .iter()
        .find(|block| block.section.as_deref() == Some("Contents/Config-0-Partition"))
        .unwrap()
        .payload
        .clone();
    let decoded = SldprtCodec
        .decode(&mut Cursor::new(source), &DecodeOptions::default())
        .unwrap();
    let mut decoded = cadmpeg_test_support::EditableDecodeResult::from(decoded);
    let brep_hash = crate::decode::brep_local_sha256(decoded.ir());
    let document_hash = crate::decode::document_local_sha256(decoded.ir());
    update_sldprt_native(&mut decoded.ir_mut(), |native| {
        native.feature_histories[0].features[0]
            .parameters
            .insert("Depth".into(), "30000mm".into());
    });
    decoded.ir_mut().model.configurations[0]
        .parameter_values
        .insert(
            cadmpeg_ir::features::ParameterId("configuration-only".into()),
            cadmpeg_ir::features::ParameterValue::Integer(3),
        );
    decoded.source_fidelity_mut().annotations.exactness.clear();
    assert_eq!(crate::decode::brep_local_sha256(decoded.ir()), brep_hash);
    assert_ne!(
        crate::decode::document_local_sha256(decoded.ir()),
        document_hash
    );

    let mut encoded = Vec::new();
    crate::test_support::plan_inherited_write(
        decoded.ir(),
        decoded.source_fidelity(),
        &mut encoded,
    )
    .unwrap();
    let written_scan = container::scan_bytes(&encoded);
    let written_partition = written_scan
        .blocks
        .iter()
        .find(|block| block.section.as_deref() == Some("Contents/Config-0-Partition"))
        .unwrap();
    assert_eq!(written_partition.payload, source_partition);
    assert!(written_scan.blocks.iter().any(|block| {
        block.section.as_deref() == Some("Contents/Config-0-Deltas")
            && block.payload == b"opaque-deltas"
    }));
    assert_eq!(written_scan.cache_cells.len(), 1);
    assert_eq!(
        written_scan.cache_cells[0].name,
        "Contents/Config-0-Partition"
    );
    assert_eq!(written_scan.cache_cells[0].logical_len, 90);
    let partition_directory = written_scan
        .directory
        .iter()
        .find(|entry| entry.name == "Contents/Config-0-Partition")
        .unwrap();
    assert_eq!(encoded[partition_directory.offset + 26], 0xab);
    let trailer = partition_directory.offset + 40 + partition_directory.name.len();
    assert_eq!(&encoded[trailer..trailer + 4], &[0x11, 0x22, 0x33, 0x44]);
    assert!(written_scan
        .directory
        .iter()
        .all(|entry| entry.trailer == [0x11, 0x22, 0x33, 0x44, 0, 0]));
    let keywords_directory = written_scan
        .directory
        .iter()
        .find(|entry| entry.name == "Contents/Keywords")
        .unwrap();
    assert_eq!(
        keywords_directory.descriptor,
        [0xcd, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0]
    );
    assert!(written_scan.blocks.iter().any(|block| {
        block.section.as_deref() == Some("Contents/Config-0-GhostPartition")
            && block.payload == b"opaque-ghost"
    }));
    let regenerated = SldprtCodec
        .decode(&mut Cursor::new(encoded), &DecodeOptions::default())
        .unwrap();
    assert!(matches!(
        regenerated.ir().model.surfaces[0].geometry,
        SurfaceGeometry::Unknown { .. }
    ));
    assert_eq!(
        sldprt_native(regenerated.ir()).feature_histories[0].features[0].parameters["Depth"],
        "30000mm"
    );
}

#[test]
fn opaque_curve_is_retained_and_does_not_block_point_edits() {
    use cadmpeg_ir::geometry::CurveGeometry;

    let mut body = triangle_body();
    body.extend(edge_use(40, 999));
    let decoded = SldprtCodec
        .decode(
            &mut Cursor::new(sldprt_with_body(&body)),
            &DecodeOptions::default(),
        )
        .unwrap();
    let mut decoded = cadmpeg_test_support::EditableDecodeResult::from(decoded);

    let curve_id = decoded.ir().model.edges[0]
        .curve
        .as_ref()
        .expect("opaque edge curve");
    let curve = decoded
        .ir()
        .model
        .curves
        .iter()
        .find(|curve| curve.id == *curve_id)
        .expect("opaque curve carrier");
    let CurveGeometry::Unknown {
        record: Some(record),
    } = &curve.geometry
    else {
        panic!("opaque curve has no replay record");
    };
    let unknowns = decoded.ir().native_unknowns("sldprt").unwrap();
    let retained = unknowns
        .iter()
        .find(|unknown| unknown.id == *record)
        .expect("opaque curve record");
    assert!(retained.links.contains(&curve.id.0));

    decoded.ir_mut().model.points[1].position.x = 1_500.0;
    let mut encoded = Vec::new();
    crate::test_support::plan_inherited_write(
        decoded.ir(),
        decoded.source_fidelity(),
        &mut encoded,
    )
    .unwrap();
    let regenerated = SldprtCodec
        .decode(&mut Cursor::new(encoded), &DecodeOptions::default())
        .unwrap();

    assert_eq!(regenerated.ir().model.points[1].position.x, 1_500.0);
    assert!(regenerated
        .ir()
        .model
        .curves
        .iter()
        .any(|curve| matches!(curve.geometry, CurveGeometry::Unknown { .. })));
}
