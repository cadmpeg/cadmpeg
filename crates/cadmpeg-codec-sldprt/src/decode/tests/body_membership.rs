// SPDX-License-Identifier: Apache-2.0
//! Typed BODY/SHELL/REGION/FACE ownership and face-use decode tests.
#![allow(clippy::unwrap_used)]

use std::io::Cursor;

use cadmpeg_ir::codec::{Codec, DecodeOptions};
use cadmpeg_ir::LossTaxonomy;

use crate::test_support::*;
use crate::SldprtCodec;

#[test]
fn decode_builds_valid_topology_and_plane() {
    use cadmpeg_ir::geometry::SurfaceGeometry;
    use cadmpeg_ir::math::Point3;

    let result = SldprtCodec
        .decode(
            &mut Cursor::new(sldprt_with_body(&triangle_body())),
            &DecodeOptions::default(),
        )
        .unwrap();

    assert!(result.report().geometry_transferred());
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
    assert_eq!(result.ir().model.loops[0].coedges().len(), 3);
    assert!(result
        .ir()
        .model
        .edges
        .iter()
        .all(|edge| edge.curve.is_none()));
}

#[test]
fn numeric_entity_chains_do_not_claim_faces() {
    let mut body = untyped_triangle(0.0);
    let typed_start = body
        .windows(3)
        .position(|window| window == [0x00, 0x0c, 0xff])
        .expect("typed ownership suffix");
    body.truncate(typed_start);
    body.extend(entity51(2, 500, 0x0017, &[510, 700, 0, 0, 0, 0, 0]));
    body.extend(entity51(1, 510, 0x001b, &[520, 0, 0, 0, 0, 0]));
    body.extend(entity51(1, 520, 0x001f, &[530, 0, 0, 0, 0, 0]));
    body.extend(entity51(1, 530, 0x0021, &[10, 0, 0, 0, 0, 0]));

    let result = SldprtCodec
        .decode(
            &mut Cursor::new(sldprt_with_body(&body)),
            &DecodeOptions::default(),
        )
        .unwrap();

    assert_eq!(result.ir().model.faces.len(), 1);
    assert_eq!(result.ir().model.bodies.len(), 1);
    assert_eq!(result.ir().model.bodies[0].id.0, "sldprt:brep:body#0");
    assert!(result
        .report()
        .losses
        .iter()
        .any(|loss| loss.message.contains("No body record was available")));
}

#[test]
fn typed_body_kind_is_preserved() {
    let result = SldprtCodec
        .decode(
            &mut Cursor::new(sldprt_with_body(&owned_triangle_with_kind(0, 700, 0.0, 3))),
            &DecodeOptions::default(),
        )
        .unwrap();

    assert_eq!(result.ir().model.bodies.len(), 1);
    assert_eq!(result.ir().model.faces.len(), 1);
    assert_eq!(
        result.ir().model.bodies[0].kind,
        cadmpeg_ir::topology::BodyKind::Sheet
    );
}

#[test]
fn typed_ownership_keeps_distinct_bodies_separate() {
    let mut body = owned_triangle(0, 500, 0.0);
    body.extend(owned_triangle(200, 501, 10.0));
    let result = SldprtCodec
        .decode(
            &mut Cursor::new(sldprt_with_body(&body)),
            &DecodeOptions::default(),
        )
        .unwrap();

    assert_eq!(result.ir().model.bodies.len(), 2);
    assert_eq!(result.ir().model.regions.len(), 2);
    assert_eq!(result.ir().model.shells.len(), 2);
    assert_eq!(result.ir().model.faces.len(), 2);
    assert_eq!(result.ir().model.bodies[0].id.0, "sldprt:brep:body#500");
    assert_eq!(result.ir().model.bodies[1].id.0, "sldprt:brep:body#501");
    assert!(result
        .ir()
        .model
        .shells
        .iter()
        .all(|shell| shell.faces.len() == 1));
}

#[test]
fn typed_face_ownership_overrides_compact_bridge_owner() {
    let mut body = triangle_body();
    let bridge = body
        .windows(2)
        .position(|window| window == [0x00, 0x0e])
        .expect("face bridge");
    body[bridge + 8..bridge + 10].copy_from_slice(&999u16.to_be_bytes());

    let result = SldprtCodec
        .decode(
            &mut Cursor::new(sldprt_with_body(&body)),
            &DecodeOptions::default(),
        )
        .unwrap();

    assert_eq!(result.ir().model.faces.len(), 1);
    assert_eq!(result.ir().model.bodies.len(), 1);
    assert_eq!(result.ir().model.bodies[0].id.0, "sldprt:brep:body#900");
    assert_eq!(result.ir().model.faces[0].shell.0, "sldprt:brep:shell#901");
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
    let result = SldprtCodec
        .decode(
            &mut Cursor::new(sldprt_with_body(&body)),
            &DecodeOptions::default(),
        )
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
    assert!(result.report().losses.iter().any(|loss| {
        loss.code.taxonomy() == LossTaxonomy::TopologyGaugeSubstituted
            && loss.message.contains("non-equivalent bridge uses")
    }));
}
