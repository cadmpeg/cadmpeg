// SPDX-License-Identifier: Apache-2.0
//! Body membership, sheet classification, and face-use decode tests.
#![allow(clippy::unwrap_used)]
#![allow(unused_imports)]

use std::io::Cursor;

use cadmpeg_ir::codec::{Codec, DecodeOptions, Encoder};
use cadmpeg_ir::LossTaxonomy;

use crate::container;
use crate::test_support::*;
use crate::SldprtCodec;

#[test]
fn decode_builds_valid_topology_and_plane() {
    use cadmpeg_ir::geometry::SurfaceGeometry;
    use cadmpeg_ir::math::Point3;

    let f = sldprt_with_body(&triangle_body());
    let mut cur = Cursor::new(f);
    let result = SldprtCodec
        .decode(&mut cur, &DecodeOptions::default())
        .unwrap();

    assert!(result.report().geometry_transferred);
    assert_eq!(result.ir().model.bodies.len(), 1);
    assert_eq!(result.ir().model.faces.len(), 1);
    assert_eq!(result.ir().model.loops.len(), 1);
    assert_eq!(result.ir().model.coedges.len(), 3);
    assert_eq!(result.ir().model.edges.len(), 3);
    assert_eq!(result.ir().model.vertices.len(), 3);
    assert_eq!(result.ir().model.points.len(), 3);
    assert_eq!(result.ir().model.surfaces.len(), 1);

    match &result.ir().model.surfaces[0].geometry {
        SurfaceGeometry::Plane {
            origin,
            normal,
            u_axis,
        } => {
            assert_eq!(*origin, Point3::new(0.0, 0.0, 0.0));
            assert_eq!(normal.z, 1.0);
            assert_eq!(u_axis.x, 1.0);
        }
        other => panic!("expected plane, got {other:?}"),
    }

    let xs: Vec<f64> = result
        .ir()
        .model
        .points
        .iter()
        .map(|p| p.position.x)
        .collect();
    assert!(xs.contains(&1000.0));

    let report = cadmpeg_ir::validate::validate_neutral(result.ir(), Vec::new());
    assert!(report.is_ok(), "validation findings: {:?}", report.findings);
    assert_eq!(result.ir().model.loops[0].coedges.len(), 3);
    // Edges carry no analytic curve (their carriers were null), which is legal.
    assert!(result.ir().model.edges.iter().all(|e| e.curve.is_none()));
}

#[test]
fn decode_reports_and_withholds_faces_without_body_membership() {
    let mut body = owned_triangle(0, 700, 0.0);
    body.extend(owned_triangle(200, 701, 2.0));
    body.extend(entity51(2, 500, 0x0017, &[10, 0, 0, 0, 0, 0, 1]));
    let result = SldprtCodec
        .decode(
            &mut Cursor::new(sldprt_with_body(&body)),
            &DecodeOptions::default(),
        )
        .unwrap();

    assert_eq!(result.ir().model.faces.len(), 1);
    assert_eq!(result.ir().model.faces[0].id.0, "sldprt:brep:face#10");
    assert!(result.report().losses.iter().any(|loss| {
        loss.code.taxonomy() == LossTaxonomy::TopologyNotTransferred
            && loss
                .message
                .contains("not claimed by an explicit body relation")
    }));
}

#[test]
fn class_root_index_selects_complete_cluster_body_relation() {
    let mut body = class_root_index(&[5, 32, 36, 500, 510, 520, 700, 701]);
    body.extend(entity51(2, 5, 0x0004, &[3, 32, 1, 1, 1, 1, 1]));
    body.extend(entity51(2, 32, 0x000f, &[3, 36, 5, 1, 1, 1, 1]));
    body.extend(entity51(2, 36, 0x0011, &[3, 1, 32, 1, 1, 1, 1]));
    body.extend(entity51(2, 500, 0x001a, &[510, 1, 1, 1, 1, 1, 1]));
    body.extend(entity51(2, 510, 0x0016, &[520, 1, 1, 1, 1, 1, 1]));
    body.extend(entity51(2, 520, 0x0020, &[1, 1, 700, 520, 1, 1, 1]));
    body.extend(entity51(1, 700, 0x0014, &[10, 1, 1, 1, 1, 1]));
    body.extend(entity51(1, 701, 0x0014, &[210, 1, 1, 1, 1, 1]));
    body.extend(owned_triangle(0, 700, 0.0));
    body.extend(owned_triangle(200, 701, 2.0));

    let result = SldprtCodec
        .decode(
            &mut Cursor::new(sldprt_with_body(&body)),
            &DecodeOptions::default(),
        )
        .unwrap();

    assert_eq!(result.ir().model.bodies.len(), 1);
    assert_eq!(result.ir().model.bodies[0].id.0, "sldprt:brep:body#32");
    assert_eq!(result.ir().model.faces.len(), 2);
    assert!(result
        .report()
        .losses
        .iter()
        .all(|loss| loss.code.taxonomy() != LossTaxonomy::TopologyNotTransferred));
    assert!(cadmpeg_ir::validate_neutral(result.ir(), result.report().losses.clone()).is_ok());
}

#[test]
fn class_root_body_relation_selects_missing_deltas_face() {
    let mut partition = class_root_index(&[5, 32, 36, 700]);
    partition.extend(entity51(2, 5, 0x0004, &[3, 32, 1, 1, 1, 1, 1]));
    partition.extend(entity51(2, 32, 0x000f, &[3, 36, 5, 1, 1, 1, 1]));
    partition.extend(entity51(2, 36, 0x0011, &[3, 1, 32, 1, 1, 1, 1]));
    partition.extend(entity51(1, 700, 0x0014, &[10, 1, 1, 1, 1, 1]));
    partition.extend(owned_triangle(0, 700, 0.0));

    let mut deltas = vec![0x00, 0x51];
    be32(&mut deltas, 2);
    be16(&mut deltas, 500);
    be32(&mut deltas, 1);
    be16(&mut deltas, 0x0017);
    for reference in [700, 701, 1, 1, 1, 1, 1] {
        deltas.push(1);
        be16(&mut deltas, reference);
    }
    deltas.push(0);
    deltas.extend(entity51(1, 701, 0x0014, &[210, 1, 1, 1, 1, 1]));
    deltas.extend(owned_triangle(200, 701, 2.0));

    let result = SldprtCodec
        .decode(
            &mut Cursor::new(sldprt_with_partition_and_deltas(&partition, &deltas)),
            &DecodeOptions::default(),
        )
        .unwrap();

    assert_eq!(result.ir().model.bodies.len(), 1);
    assert_eq!(result.ir().model.bodies[0].id.0, "sldprt:brep:body#32");
    assert_eq!(result.ir().model.faces.len(), 2);
    assert!(result
        .report()
        .losses
        .iter()
        .all(|loss| loss.code.taxonomy() != LossTaxonomy::TopologyNotTransferred));
    assert!(cadmpeg_ir::validate_neutral(result.ir(), result.report().losses.clone()).is_ok());
}

#[test]
fn explicit_delta_body_selector_binds_new_face_through_partition_anchor() {
    let mut partition = entity51(2, 500, 0x0017, &[700, 1, 1, 1, 1, 1, 1]);
    partition.extend(entity51(1, 700, 0x0014, &[10, 1, 1, 1, 1, 1]));
    partition.extend(owned_triangle(0, 700, 0.0));

    let mut deltas = vec![0x00, 0x51];
    be32(&mut deltas, 2);
    be16(&mut deltas, 501);
    be32(&mut deltas, 1);
    be16(&mut deltas, 0x0017);
    for reference in [700, 701, 1, 1, 1, 1, 1] {
        deltas.push(1);
        be16(&mut deltas, reference);
    }
    deltas.push(0);
    deltas.extend(entity51(1, 701, 0x0014, &[210, 1, 1, 1, 1, 1]));
    deltas.extend(owned_triangle(200, 701, 2.0));

    let result = SldprtCodec
        .decode(
            &mut Cursor::new(sldprt_with_partition_and_deltas(&partition, &deltas)),
            &DecodeOptions::default(),
        )
        .unwrap();

    assert_eq!(result.ir().model.bodies.len(), 1);
    assert_eq!(result.ir().model.faces.len(), 2);
    assert!(result
        .report()
        .losses
        .iter()
        .all(|loss| loss.code.taxonomy() != LossTaxonomy::TopologyNotTransferred));
    assert!(cadmpeg_ir::validate_neutral(result.ir(), result.report().losses.clone()).is_ok());
}

#[test]
fn duplicate_face_uses_emit_one_face() {
    let mut body = triangle_body();
    let first_bridge = body
        .windows(2)
        .position(|w| w == [0x00, 0x0e])
        .expect("bridge");
    body[first_bridge + 8..first_bridge + 10].copy_from_slice(&700u16.to_be_bytes());
    body.extend(bridge_owned(11, 20, 100, 700));
    let f = sldprt_with_body(&body);
    let mut cur = Cursor::new(f);

    let result = SldprtCodec
        .decode(&mut cur, &DecodeOptions::default())
        .unwrap();

    assert_eq!(result.ir().model.faces.len(), 1);
    assert_eq!(result.ir().model.faces[0].id.0, "sldprt:brep:face#10");
}

#[test]
fn decode_withholds_non_equivalent_face_uses_with_same_owner() {
    let mut body = triangle_body();
    let first_bridge = body
        .windows(2)
        .position(|w| w == [0x00, 0x0e])
        .expect("bridge");
    body[first_bridge + 8..first_bridge + 10].copy_from_slice(&700u16.to_be_bytes());
    body.extend(bridge_owned(11, 20, 200, 700));
    body.extend(owned_triangle(200, 701, 2.0));
    let result = SldprtCodec
        .decode(
            &mut Cursor::new(sldprt_with_body(&body)),
            &DecodeOptions::default(),
        )
        .unwrap();

    assert_eq!(result.ir().model.faces.len(), 1);
    assert_eq!(result.ir().model.faces[0].id.0, "sldprt:brep:face#210");
    assert!(
        result
            .report()
            .losses
            .iter()
            .any(
                |loss| loss.code.taxonomy() == LossTaxonomy::TopologyGaugeSubstituted
                    && loss.message.contains("non-equivalent bridge uses")
            ),
        "losses: {:?}",
        result.report().losses
    );
}

#[test]
fn sheet_body_faces_are_retained_and_classified() {
    let mut body = Vec::new();
    body.extend(entity51(2, 500, 0x0017, &[510, 700, 0, 0, 0, 0]));
    body.extend(entity51(2, 501, 0x0017, &[511, 701, 0, 0, 0, 0]));
    body.extend(entity51(1, 510, 0x001b, &[700, 0, 0, 0, 0, 0]));
    body.extend(entity51(1, 511, 0x001d, &[701, 0, 0, 0, 0, 0]));
    body.extend(owned_triangle(0, 700, 0.0));
    body.extend(owned_triangle(200, 701, 10.0));
    let f = sldprt_with_body(&body);
    let mut cur = Cursor::new(f);

    let result = SldprtCodec
        .decode(&mut cur, &DecodeOptions::default())
        .unwrap();

    assert_eq!(result.ir().model.bodies.len(), 2);
    assert_eq!(result.ir().model.faces.len(), 2);
    assert_eq!(
        result
            .ir()
            .model
            .bodies
            .iter()
            .filter(|body| body.kind == cadmpeg_ir::topology::BodyKind::Solid)
            .count(),
        1
    );
    assert_eq!(
        result
            .ir()
            .model
            .bodies
            .iter()
            .filter(|body| body.kind == cadmpeg_ir::topology::BodyKind::Sheet)
            .count(),
        1
    );
}

#[test]
fn schema_33103_disc1d_flo2_is_not_a_sheet_region() {
    let mut body = Vec::new();
    body.extend(entity51(2, 501, 0x0017, &[511, 701, 0, 0, 0, 0]));
    body.extend(entity51(2, 511, 0x001d, &[701, 0, 0, 0, 0, 0]));
    body.extend(owned_triangle(0, 701, 0.0));
    let decoded = SldprtCodec
        .decode(
            &mut Cursor::new(sldprt_with_body(&body)),
            &DecodeOptions::default(),
        )
        .unwrap();

    assert_eq!(decoded.ir().model.bodies.len(), 1);
    assert_eq!(
        decoded.ir().model.bodies[0].kind,
        cadmpeg_ir::topology::BodyKind::Solid
    );
}

#[test]
fn decode_preserves_explicit_body_membership() {
    let mut body = Vec::new();
    body.extend(entity51(2, 500, 0x0017, &[700, 0, 0, 0, 0, 0]));
    body.extend(entity51(2, 501, 0x0017, &[701, 0, 0, 0, 0, 0]));
    body.extend(owned_triangle(0, 700, 0.0));
    body.extend(owned_triangle(200, 701, 10.0));
    let f = sldprt_with_body(&body);
    let mut cur = Cursor::new(f);

    let result = SldprtCodec
        .decode(&mut cur, &DecodeOptions::default())
        .unwrap();

    assert_eq!(result.ir().model.bodies.len(), 2);
    assert_eq!(result.ir().model.shells.len(), 2);
    assert_eq!(result.ir().model.faces.len(), 2);
    assert_eq!(result.ir().model.bodies[0].id.0, "sldprt:brep:body#500");
    assert_eq!(result.ir().model.bodies[1].id.0, "sldprt:brep:body#501");
}

#[test]
fn decode_preserves_multiple_regions_and_shells_per_body() {
    let mut body = Vec::new();
    body.extend(entity51(2, 500, 0x0017, &[510, 511, 0, 0, 0, 0]));
    body.extend(entity51(1, 510, 0x001b, &[520, 0, 0, 0, 0, 0]));
    body.extend(entity51(1, 511, 0x001b, &[521, 0, 0, 0, 0, 0]));
    body.extend(entity51(1, 520, 0x001f, &[530, 0, 0, 0, 0, 0]));
    body.extend(entity51(1, 521, 0x001f, &[531, 0, 0, 0, 0, 0]));
    body.extend(entity51(1, 530, 0x0021, &[700, 0, 0, 0, 0, 0]));
    body.extend(entity51(1, 531, 0x0021, &[701, 0, 0, 0, 0, 0]));
    body.extend(owned_triangle(0, 700, 0.0));
    body.extend(owned_triangle(200, 701, 10.0));

    let mut result = SldprtCodec
        .decode(
            &mut Cursor::new(sldprt_with_body(&body)),
            &DecodeOptions::default(),
        )
        .unwrap();

    assert_eq!(result.ir().model.bodies.len(), 1);
    assert_eq!(result.ir().model.regions.len(), 2);
    assert_eq!(result.ir().model.shells.len(), 2);
    assert_eq!(result.ir().model.bodies[0].regions.len(), 2);
    assert!(result
        .ir()
        .model
        .regions
        .iter()
        .all(|region| region.shells.len() == 1));
    assert!(result
        .ir()
        .model
        .shells
        .iter()
        .all(|shell| shell.faces.len() == 1));
    let report = cadmpeg_ir::validate::validate_neutral(result.ir(), Vec::new());
    assert!(report.is_ok(), "validation findings: {:?}", report.findings);

    result.ir_mut().model.points[0].position.z += 1.0;
    let mut encoded = Vec::new();
    SldprtCodec
        .write_preserved_with_source_fidelity(result.ir(), result.source_fidelity(), &mut encoded)
        .unwrap();
    let regenerated = SldprtCodec
        .decode(&mut Cursor::new(encoded), &DecodeOptions::default())
        .unwrap();
    assert_eq!(regenerated.ir().model.bodies.len(), 1);
    assert_eq!(regenerated.ir().model.regions.len(), 2);
    assert_eq!(regenerated.ir().model.shells.len(), 2);
    assert!(regenerated
        .ir()
        .model
        .shells
        .iter()
        .all(|shell| shell.faces.len() == 1));
}

#[test]
fn decode_follows_connector_region_lump_and_shell_chain() {
    let mut body = Vec::new();
    body.extend(entity51(2, 500, 0x0017, &[510, 0, 0, 0, 0, 0]));
    body.extend(entity51(2, 510, 0x0019, &[0, 520, 0, 0, 0, 0]));
    body.extend(entity51(1, 520, 0x001b, &[530, 0, 0, 0, 0, 0]));
    body.extend(entity51(2, 530, 0x001f, &[540, 0, 0, 0, 0, 0]));
    body.extend(entity51(2, 540, 0x0021, &[550, 0, 0, 0, 0, 0]));
    body.extend(entity51(2, 550, 0x0023, &[700, 0, 0, 0, 0, 0]));
    body.extend(owned_triangle(0, 700, 0.0));

    let decoded = SldprtCodec
        .decode(
            &mut Cursor::new(sldprt_with_body(&body)),
            &DecodeOptions::default(),
        )
        .unwrap();

    assert_eq!(decoded.ir().model.bodies.len(), 1);
    assert_eq!(decoded.ir().model.regions[0].id.0, "sldprt:brep:region#520");
    assert_eq!(decoded.ir().model.shells[0].id.0, "sldprt:brep:shell#550");
    assert_eq!(decoded.ir().model.shells[0].faces.len(), 1);
    assert_eq!(
        decoded.ir().model.bodies[0].kind,
        cadmpeg_ir::topology::BodyKind::Solid
    );
}
