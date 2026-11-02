// SPDX-License-Identifier: Apache-2.0
//! Typed BODY/SHELL/REGION/FACE ownership and face-use decode tests.
#![allow(clippy::unwrap_used)]

use std::collections::{BTreeMap, BTreeSet};
use std::io::Cursor;

use cadmpeg_ir::codec::{Codec, DecodeOptions};

use crate::test_support::container::sldprt_with_body;
use crate::test_support::parasolid::append_void_and_faceless_regions;
use crate::test_support::parasolid::bridge_owned;
use crate::test_support::parasolid::entity51;
use crate::test_support::parasolid::owned_triangle;
use crate::test_support::parasolid::owned_triangle_with_kind;
use crate::test_support::parasolid::triangle_body;
use crate::test_support::parasolid::untyped_triangle;
use crate::SldprtCodec;

#[test]
fn decode_builds_valid_topology_and_plane() {
    use cadmpeg_ir::geometry::{SolvedSurfaceGeometry, SurfaceGeometry};
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
        SurfaceGeometry::Solved(SolvedSurfaceGeometry::Plane(plane_surface)) => {
            let origin = plane_surface.origin();
            let normal = plane_surface.frame().axis().as_raw();
            let u_axis = plane_surface.frame().reference().as_raw();
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
        .map(|p| p.position().get().x)
        .collect();
    assert!(xs.contains(&1000.0));

    let report = cadmpeg_ir::validate::validate_neutral(result.ir(), Vec::new())
        .expect("validate the neutral model");
    assert!(report.is_ok(), "validation findings: {:?}", report.findings);
    assert_eq!(result.ir().model.loops[0].coedges().len(), 3);
    assert!(result
        .ir()
        .model
        .edges
        .iter()
        .all(|edge| edge.curve().is_none()));
}

#[test]
fn numeric_entity_chains_do_not_claim_faces() {
    let mut body = untyped_triangle(0.0);
    let typed_start = body
        .windows(3)
        .position(|window| window == [0x00, 0x0c, 0xff])
        .expect("typed ownership suffix");
    body.truncate(typed_start);
    body.extend(crate::test_support::parasolid::fixture_vertex_owner_class(
        900,
    ));
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
    assert_eq!(
        result.ir().model.bodies[0].id.as_str(),
        "sldprt:brep:body#0"
    );
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
    assert_eq!(
        result.ir().model.bodies[0].id.as_str(),
        "sldprt:brep:body#500"
    );
    assert_eq!(
        result.ir().model.bodies[1].id.as_str(),
        "sldprt:brep:body#501"
    );
    assert!(result
        .ir()
        .model
        .shells
        .iter()
        .all(|shell| shell.faces().len() == 1));
}

#[test]
fn typed_face_ownership_is_independent_of_attribute_chain_head() {
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
    assert_eq!(
        result.ir().model.bodies[0].id.as_str(),
        "sldprt:brep:body#900"
    );
    assert_eq!(
        result.ir().model.faces[0].shell.as_str(),
        "sldprt:brep:shell#901"
    );
}

#[test]
fn a_shared_metadata_head_does_not_supply_loop_or_body_ownership() {
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
    assert_eq!(
        result.ir().model.faces[0].id.as_str(),
        "sldprt:brep:face#10"
    );
}

#[test]
fn distinct_faces_with_shared_metadata_head_retain_typed_body_membership() {
    let mut body = triangle_body();
    let first_bridge = body
        .windows(2)
        .position(|w| w == [0x00, 0x0e])
        .expect("bridge");
    body[first_bridge + 8..first_bridge + 10].copy_from_slice(&700u16.to_be_bytes());
    let mut other_use = bridge_owned(11, 20, 200, 700);
    other_use[4..8].copy_from_slice(&400_011u32.to_be_bytes());
    other_use[24..26].copy_from_slice(&901u16.to_be_bytes());
    body.extend(other_use);
    body.extend(owned_triangle(200, 700, 2.0));
    let result = SldprtCodec
        .decode(
            &mut Cursor::new(sldprt_with_body(&body)),
            &DecodeOptions::default(),
        )
        .unwrap();

    assert_eq!(result.ir().model.faces.len(), 2);
    let identities: std::collections::HashSet<_> = result
        .ir()
        .model
        .faces
        .iter()
        .map(|face| face.id.as_str())
        .collect();
    assert_eq!(
        identities,
        std::collections::HashSet::from(["sldprt:brep:face#10", "sldprt:brep:face#210"])
    );
    assert_eq!(result.ir().model.bodies.len(), 2);
    assert!(result
        .ir()
        .model
        .shells
        .iter()
        .all(|shell| shell.faces().len() == 1));
}

#[test]
fn a_void_region_without_faces_transfers_no_region_and_a_faceless_solid_region_stays() {
    let (owner, region, void, faceless) = (500, 80, 600, 601);
    let mut body = owned_triangle(0, owner, 0.0);
    append_void_and_faceless_regions(&mut body, owner, region, void, faceless);

    let result = SldprtCodec
        .decode(
            &mut Cursor::new(sldprt_with_body(&body)),
            &DecodeOptions::default(),
        )
        .unwrap();

    let regions: BTreeMap<_, _> = result
        .ir()
        .model
        .regions
        .iter()
        .map(|region| (region.id.as_str(), region.shells.len()))
        .collect();
    assert_eq!(
        regions,
        BTreeMap::from([("sldprt:brep:region#80", 1), ("sldprt:brep:region#601", 0)])
    );
    let body_regions: BTreeSet<_> = result.ir().model.bodies[0]
        .regions
        .iter()
        .map(cadmpeg_ir::ids::RegionId::as_str)
        .collect();
    assert_eq!(
        body_regions,
        BTreeSet::from(["sldprt:brep:region#80", "sldprt:brep:region#601"])
    );
}
