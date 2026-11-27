// SPDX-License-Identifier: Apache-2.0
#![allow(clippy::unwrap_used)]

use std::io::Cursor;

use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, InspectOptions};
use cadmpeg_ir::codec::{Codec, Confidence, DecodeOptions};

use crate::{test_support, SolidEdgeCodec};

const EPS_SPHERE_RADIUS_MM: f64 = 1.0e-12;
const EPS_RESOLUTION_MM: f64 = 1.0e-15;
/// The Parasolid modeller default linear precision (`parasolid.md` §4).
const MODELLER_DEFAULT_LINEAR_MM: f64 = 1.0e-5;
/// The Parasolid modeller angular precision (`parasolid.md` §4).
const MODELLER_ANGULAR_RADIANS: f64 = 1.0e-11;
const EPS_RING_VERTEX_MM: f64 = 1.0e-9;
/// The null real of the kernel's binary encoding.
const NULL_REAL: f64 = -3.14158e13;

#[test]
fn directory_signature_detects_solid_edge() {
    let bytes = test_support::compound();
    let arena = DecodeArena::new();
    let policy = DecodePolicy::default();
    let (ctx, root) = DecodeContext::from_root_bytes(&bytes, &arena, &policy).unwrap();
    assert_eq!(SolidEdgeCodec.detect(&ctx, root).unwrap(), Confidence::High);
    ctx.finish_session().unwrap();
}

fn detect(bytes: &[u8]) -> Confidence {
    let arena = DecodeArena::new();
    let policy = DecodePolicy::default();
    let (ctx, root) = DecodeContext::from_root_bytes(bytes, &arena, &policy).unwrap();
    let confidence = SolidEdgeCodec.detect(&ctx, root).unwrap();
    ctx.finish_session().unwrap();
    confidence
}

#[test]
fn family_directory_signature_detects_solid_edge() {
    let bytes = test_support::compound_with("FamilyInfo", "FOAPublishData", test_support::FAMILY);
    assert_eq!(detect(&bytes), Confidence::High);
}

#[test]
fn every_document_class_with_its_entries_is_detected() {
    for class in [
        test_support::PART,
        test_support::SHEET_METAL,
        test_support::ASSEMBLY,
    ] {
        let bytes = test_support::compound_with("PSMroots", "JSitesList", class);
        assert_eq!(detect(&bytes), Confidence::High, "{class:?}");
    }
}

#[test]
fn unlisted_root_class_is_rejected_with_solid_edge_entries() {
    for (first, second) in [("PSMroots", "JSitesList"), ("FamilyInfo", "FOAPublishData")] {
        let bytes = test_support::compound_with(first, second, test_support::OTHER);
        assert_eq!(detect(&bytes), Confidence::No, "{first}");
        assert!(SolidEdgeCodec
            .decode(&mut Cursor::new(bytes.clone()), &DecodeOptions::default())
            .is_err());
        assert!(SolidEdgeCodec
            .inspect(&mut Cursor::new(bytes), &InspectOptions::default())
            .is_err());
    }
}

#[test]
fn root_class_of_the_other_form_is_rejected() {
    let document = test_support::compound_with("PSMroots", "JSitesList", test_support::FAMILY);
    let family = test_support::compound_with("FamilyInfo", "FOAPublishData", test_support::PART);
    assert_eq!(detect(&document), Confidence::No);
    assert_eq!(detect(&family), Confidence::No);
}

#[test]
fn entry_names_compare_without_regard_to_case_through_decode_and_inspect() {
    let bytes = test_support::compound_with("psmROOTS", "jsitesLIST", test_support::ASSEMBLY);
    assert_eq!(detect(&bytes), Confidence::High);
    let summary = SolidEdgeCodec
        .inspect(&mut Cursor::new(bytes.clone()), &InspectOptions::default())
        .unwrap();
    assert_eq!(summary.entries.len(), 2);
    SolidEdgeCodec
        .decode(&mut Cursor::new(bytes), &DecodeOptions::default())
        .unwrap();
}

#[test]
fn family_documents_decode_and_inspect() {
    let bytes = test_support::compound_with("FAMILYinfo", "foaPUBLISHdata", test_support::FAMILY);
    let summary = SolidEdgeCodec
        .inspect(&mut Cursor::new(bytes.clone()), &InspectOptions::default())
        .unwrap();
    assert_eq!(summary.entries.len(), 2);
    let result = SolidEdgeCodec
        .decode(&mut Cursor::new(bytes), &DecodeOptions::default())
        .unwrap();
    assert!(result.ir().geometry.bodies.is_empty());
}

#[test]
fn incomplete_directory_signatures_are_rejected() {
    for pair in [
        ("JSitesList", "Payload"),
        ("PSMroots", "Payload"),
        ("JGlobal", "JSitesList"),
        ("PSMroots", "FamilyInfo"),
    ] {
        let bytes = test_support::compound_with(pair.0, pair.1, test_support::PART);
        assert_eq!(detect(&bytes), Confidence::No, "{pair:?}");
    }
}

#[test]
fn generic_compound_file_has_no_solid_edge_identity() {
    let bytes = cadmpeg_test_support::compound::compound_fixture();
    let arena = DecodeArena::new();
    let policy = DecodePolicy::default();
    let (ctx, root) = DecodeContext::from_root_bytes(&bytes, &arena, &policy).unwrap();
    assert_eq!(SolidEdgeCodec.detect(&ctx, root).unwrap(), Confidence::No);
    ctx.finish_session().unwrap();
    assert!(SolidEdgeCodec
        .decode(&mut Cursor::new(bytes), &DecodeOptions::default())
        .is_err());
}

#[test]
fn inspection_lists_compound_streams() {
    let bytes = test_support::compound();
    let summary = SolidEdgeCodec
        .inspect(&mut Cursor::new(bytes), &InspectOptions::default())
        .unwrap();
    assert_eq!(summary.entries.len(), 2);
}

#[test]
fn full_decode_reports_pending_bodies() {
    let bytes = test_support::compound();
    let result = SolidEdgeCodec
        .decode(&mut Cursor::new(bytes), &DecodeOptions::default())
        .unwrap();
    assert!(result.ir().geometry.bodies.is_empty());
    for code in [
        crate::loss::SolidEdgeLossCode::BodiesPending,
        crate::loss::SolidEdgeLossCode::FeaturesPending,
        crate::loss::SolidEdgeLossCode::AttributesPending,
    ] {
        assert!(result
            .report()
            .losses
            .iter()
            .any(|loss| loss.code == code.note("").code));
    }
}

#[test]
fn primary_partition_projects_an_exact_sphere_in_millimetres() {
    let bytes = test_support::with_partitions(&[test_support::sphere_partition()]);
    let result = SolidEdgeCodec
        .decode(&mut Cursor::new(bytes), &DecodeOptions::default())
        .unwrap();
    assert!(result.report().geometry_transferred());
    let ir = result.ir();
    assert_eq!(ir.geometry.bodies.len(), 1);
    assert_eq!(ir.geometry.faces.len(), 1);
    assert_eq!(ir.geometry.regions.len(), 1);
    assert_eq!(ir.geometry.shells.len(), 1);
    assert_eq!(
        ir.geometry.bodies[0].kind,
        cadmpeg_ir::topology::BodyKind::Solid
    );
    let cadmpeg_ir::geometry::SurfaceGeometry::Solved(
        cadmpeg_ir::geometry::SolvedSurfaceGeometry::Sphere(sphere),
    ) = &ir.geometry.surfaces[0].geometry
    else {
        panic!("sphere carrier");
    };
    assert!((sphere.radius().get() - 7.0).abs() < EPS_SPHERE_RADIUS_MM);
    assert_geometry_valid(ir);
}

#[test]
fn the_document_root_lists_the_bodies_of_every_partition() {
    let bytes = test_support::with_partitions(&[
        test_support::sphere_partition(),
        test_support::two_solid_regions_partition(),
    ]);
    let result = SolidEdgeCodec
        .decode(&mut Cursor::new(bytes), &DecodeOptions::default())
        .unwrap();
    let ir = result.ir();
    assert_eq!(ir.geometry.bodies.len(), 2);
    let [root] = ir.model.product_definitions.as_slice() else {
        panic!("one product definition");
    };
    assert!(root.is_root());
    let bodies: std::collections::BTreeSet<_> =
        ir.geometry.bodies.iter().map(|body| &body.id).collect();
    assert_eq!(root.bodies.len(), bodies.len());
    assert_eq!(
        root.bodies
            .iter()
            .collect::<std::collections::BTreeSet<_>>(),
        bodies
    );
    assert_geometry_valid(ir);
}

#[test]
fn one_body_with_two_solid_regions_stays_one_body() {
    let bytes = test_support::with_partitions(&[test_support::two_solid_regions_partition()]);
    let result = SolidEdgeCodec
        .decode(&mut Cursor::new(bytes), &DecodeOptions::default())
        .unwrap();
    let ir = result.ir();
    assert_eq!(ir.geometry.bodies.len(), 1);
    assert_eq!(ir.geometry.regions.len(), 2);
    assert_eq!(ir.geometry.shells.len(), 2);
    assert_eq!(ir.geometry.faces.len(), 2);
    for region in &ir.geometry.regions {
        assert_eq!(region.body, ir.geometry.bodies[0].id);
        assert_eq!(region.stated_shells().count(), 1);
    }
    assert_ne!(ir.geometry.regions[0].id, ir.geometry.regions[1].id);
    assert_geometry_valid(ir);
}

fn decode_partition(partition: Vec<u8>) -> cadmpeg_ir::codec::DecodeResult {
    let bytes = test_support::with_partitions(&[partition]);
    SolidEdgeCodec
        .decode(&mut Cursor::new(bytes), &DecodeOptions::default())
        .unwrap()
}

fn charged(result: &cadmpeg_ir::codec::DecodeResult, code: crate::loss::SolidEdgeLossCode) -> bool {
    result
        .report()
        .losses
        .iter()
        .any(|loss| loss.code == code.note("").code)
}

#[test]
fn void_region_that_owns_a_face_transfers_as_its_own_region() {
    let result = decode_partition(test_support::solid_with_cavity_partition());
    let ir = result.ir();
    assert_eq!(ir.geometry.bodies.len(), 1);
    // The solid region and the cavity region; the exterior void region owns
    // no face and transfers no region.
    assert_eq!(ir.geometry.regions.len(), 2);
    assert_eq!(ir.geometry.shells.len(), 2);
    assert_eq!(ir.geometry.faces.len(), 2);
    for region in &ir.geometry.regions {
        assert_eq!(region.body, ir.geometry.bodies[0].id);
        assert_eq!(region.stated_shells().count(), 1);
    }
    for shell in &ir.geometry.shells {
        assert!(ir
            .geometry
            .regions
            .iter()
            .any(|region| region.id == shell.region
                && region.stated_shells().any(|stated| stated == &shell.id)));
    }
    for face in &ir.geometry.faces {
        assert!(ir
            .geometry
            .shells
            .iter()
            .any(|shell| shell.id == face.shell));
    }
    assert!(!charged(
        &result,
        crate::loss::SolidEdgeLossCode::RegionFacesUntransferred
    ));
    assert_geometry_valid(ir);
}

#[test]
fn exterior_void_region_with_a_faceless_shell_transfers_no_region() {
    let result = decode_partition(test_support::solid_regions_with_void_shell_partition());
    let ir = result.ir();
    assert_eq!(ir.geometry.regions.len(), 2);
    assert_eq!(ir.geometry.shells.len(), 2);
    assert_eq!(ir.geometry.faces.len(), 2);
    assert!(!charged(
        &result,
        crate::loss::SolidEdgeLossCode::RegionFacesUntransferred
    ));
    assert_geometry_valid(ir);
}

#[test]
fn region_whose_faces_cannot_be_transferred_charges_a_loss() {
    let mut partition = test_support::solid_with_cavity_partition();
    let records = cadmpeg_parasolid::records::PrimaryRecords::read(&partition)
        .unwrap()
        .unwrap();
    let range = records
        .nodes()
        .find(|node| {
            node.kind() == cadmpeg_parasolid::records::node_type::FACE && node.index() == 7
        })
        .unwrap()
        .field_range("sense")
        .unwrap();
    partition[range].copy_from_slice(b"?");
    let result = decode_partition(partition);
    let ir = result.ir();
    assert_eq!(ir.geometry.faces.len(), 1);
    assert!(charged(
        &result,
        crate::loss::SolidEdgeLossCode::FaceSenseUnresolved
    ));
    assert!(charged(
        &result,
        crate::loss::SolidEdgeLossCode::RegionFacesUntransferred
    ));
    assert_geometry_valid(ir);
}

#[test]
fn a_pinned_decode_refuses_a_face_that_states_no_orientation() {
    let mut partition = test_support::solid_with_cavity_partition();
    let records = cadmpeg_parasolid::records::PrimaryRecords::read(&partition)
        .unwrap()
        .unwrap();
    let range = records
        .nodes()
        .find(|node| {
            node.kind() == cadmpeg_parasolid::records::node_type::FACE && node.index() == 7
        })
        .unwrap()
        .field_range("sense")
        .unwrap();
    partition[range].copy_from_slice(b"?");
    let bytes = test_support::with_partitions(&[partition]);

    let admitted = SolidEdgeCodec
        .decode(&mut Cursor::new(bytes.clone()), &DecodeOptions::default())
        .unwrap();
    assert!(charged(
        &admitted,
        crate::loss::SolidEdgeLossCode::FaceSenseUnresolved
    ));
    let given_up = admitted
        .report()
        .losses
        .iter()
        .filter(|loss| loss.code == cadmpeg_ir::unknown::part::record_unreadable_kind())
        .flat_map(|loss| loss.message.split_whitespace())
        .find(|token| token.starts_with("solidedge:brep:face#"))
        .expect("record.unreadable names the face")
        .to_owned();
    assert!(admitted
        .ir()
        .geometry
        .faces
        .iter()
        .all(|face| face.id.as_str() != given_up));

    let mut options = DecodeOptions::default();
    options.strictness.pinned = true;
    let failure = SolidEdgeCodec
        .decode(&mut Cursor::new(bytes), &options)
        .expect_err("a pinned decode refuses a given-up face");
    let cadmpeg_ir::codec::DecodeFailure::StrictRejected { rejection } = &failure else {
        panic!("expected a strict rejection, got {failure:?}");
    };
    assert_eq!(
        rejection.loss().code,
        cadmpeg_ir::unknown::part::record_unreadable_kind()
    );
    assert!(rejection
        .loss()
        .message
        .split_whitespace()
        .any(|token| token == given_up));
}

#[test]
fn a_face_that_states_no_orientation_drops_its_loops_and_coedges() {
    let mut partition = test_support::cube_partition();
    let records = cadmpeg_parasolid::records::PrimaryRecords::read(&partition)
        .unwrap()
        .unwrap();
    let face = records
        .nodes()
        .find(|node| node.kind() == cadmpeg_parasolid::records::node_type::FACE)
        .unwrap();
    let span = format!(
        " stream=\"solidedge:PARASOLID/STREAM0.P_B\" offset={} length={} given up: ",
        face.range().start,
        face.range().len()
    );
    let range = face.field_range("sense").unwrap();
    partition[range].copy_from_slice(b"?");
    let bytes = test_support::with_partitions(&[partition]);
    let result = SolidEdgeCodec
        .decode(&mut Cursor::new(bytes), &DecodeOptions::default())
        .unwrap();
    let charges: Vec<&str> = result
        .report()
        .losses
        .iter()
        .filter(|loss| loss.code == cadmpeg_ir::unknown::part::record_unreadable_kind())
        .map(|loss| loss.message.as_str())
        .collect();
    let [charge] = charges.as_slice() else {
        panic!("one record.unreadable: {charges:?}");
    };
    assert!(
        charge.starts_with("record solidedge:brep:face#"),
        "{charge}"
    );
    assert!(charge.contains(&span), "{charge}");
    let dropped = charge
        .split_once("; dropped=[")
        .and_then(|(_, tail)| tail.split_once(']'))
        .map(|(list, _)| list.split(", ").collect::<Vec<_>>())
        .expect("the face's charge names the records dropped with it");
    for kind in ["solidedge:brep:loop#", "solidedge:brep:coedge#"] {
        assert!(dropped.iter().any(|id| id.starts_with(kind)), "{charge}");
    }
    let geometry = &result.ir().geometry;
    assert_eq!(geometry.faces.len(), 5);
    assert!(dropped.iter().all(|id| {
        geometry.loops.iter().all(|loop_| loop_.id.as_str() != *id)
            && geometry
                .coedges
                .iter()
                .all(|coedge| coedge.id.as_str() != *id)
    }));
}

fn surfaceless_face(sense: u8) -> cadmpeg_ir::codec::DecodeResult {
    let mut partition = test_support::sphere_partition();
    let records = cadmpeg_parasolid::records::PrimaryRecords::read(&partition)
        .unwrap()
        .unwrap();
    let face = records
        .nodes()
        .find(|node| node.kind() == cadmpeg_parasolid::records::node_type::FACE)
        .unwrap();
    let surface = face.field_range("surface").unwrap();
    let sense_range = face.field_range("sense").unwrap();
    partition[surface].copy_from_slice(&1u16.to_be_bytes());
    partition[sense_range].copy_from_slice(&[sense]);
    decode_partition(partition)
}

#[test]
fn face_without_a_surface_keeps_its_stored_orientation() {
    for (sense, expected) in [
        (b'-', cadmpeg_ir::topology::Sense::Reversed),
        (b'+', cadmpeg_ir::topology::Sense::Forward),
    ] {
        let result = surfaceless_face(sense);
        let ir = result.ir();
        assert_eq!(ir.geometry.faces.len(), 1);
        assert_eq!(ir.geometry.faces[0].sense, expected);
        assert!(charged(
            &result,
            crate::loss::SolidEdgeLossCode::SurfaceUnresolved
        ));
        assert_geometry_valid(ir);
    }
}

#[test]
fn body_that_names_an_instance_keeps_its_coordinates_with_a_loss() {
    let plain = decode_partition(test_support::sphere_partition());
    assert!(!charged(
        &plain,
        crate::loss::SolidEdgeLossCode::BodyPlacementUnread
    ));
    assert!(plain.ir().geometry.bodies[0].transform.is_none());
    let mut partition = test_support::sphere_partition();
    let records = cadmpeg_parasolid::records::PrimaryRecords::read(&partition)
        .unwrap()
        .unwrap();
    let range = records
        .nodes()
        .find(|node| node.kind() == cadmpeg_parasolid::records::node_type::BODY)
        .unwrap()
        .field_range("ref_instance")
        .unwrap();
    partition[range].copy_from_slice(&9u16.to_be_bytes());
    let result = decode_partition(partition);
    assert!(charged(
        &result,
        crate::loss::SolidEdgeLossCode::BodyPlacementUnread
    ));
    assert!(result.ir().geometry.bodies[0].transform.is_none());
    assert_eq!(result.ir().geometry.faces.len(), 1);
    assert_geometry_valid(result.ir());
}

#[test]
fn unreadable_partition_preserves_another_partition() {
    let bytes =
        test_support::with_partitions(&[b"broken".to_vec(), test_support::sphere_partition()]);
    let result = SolidEdgeCodec
        .decode(&mut Cursor::new(bytes), &DecodeOptions::default())
        .unwrap();
    assert_eq!(result.ir().geometry.bodies.len(), 1);
    assert!(result.report().losses.iter().any(|loss| loss.code
        == crate::loss::SolidEdgeLossCode::PartitionUnreadable
            .note("")
            .code));
    let given_up: Vec<_> = result
        .report()
        .losses
        .iter()
        .filter(|loss| loss.code == cadmpeg_ir::unknown::part::record_unreadable_kind())
        .collect();
    let [given_up] = given_up.as_slice() else {
        panic!("one given-up partition: {given_up:?}");
    };
    assert!(
        given_up
            .message
            .starts_with("part partition stream=\"solidedge:"),
        "{given_up:?}"
    );
    let stubs = result.ir().native_unknowns("solidedge").unwrap();
    let [stub] = stubs.as_slice() else {
        panic!("one stub: {stubs:?}");
    };
    assert!(given_up.message.contains(&format!("record={}", stub.id)));
    let (_, _, fidelity) = result.into_parts();
    let retained = fidelity.retained_record(stub.id.as_str()).unwrap();
    let mut stream = b"broken".to_vec();
    stream.resize(4096, 0);
    assert_eq!(retained.data(), Some(stream.as_slice()));
}

#[test]
fn partition_indices_have_separate_identity_spaces() {
    let partition = test_support::sphere_partition();
    let bytes = test_support::with_partitions(&[partition.clone(), partition]);
    let result = SolidEdgeCodec
        .decode(&mut Cursor::new(bytes), &DecodeOptions::default())
        .unwrap();
    let ir = result.ir();
    assert_eq!(ir.geometry.bodies.len(), 2);
    assert_ne!(ir.geometry.bodies[0].id, ir.geometry.bodies[1].id);
    assert_geometry_valid(ir);
}

#[test]
fn missing_terminator_keeps_framed_body_nodes() {
    let mut partition = test_support::sphere_partition();
    partition.truncate(partition.len() - 4);
    let bytes = test_support::with_partitions(&[partition]);
    let result = SolidEdgeCodec
        .decode(&mut Cursor::new(bytes), &DecodeOptions::default())
        .unwrap();
    assert_eq!(result.ir().geometry.bodies.len(), 1);
    assert!(result.report().losses.iter().any(|loss| loss.code
        == crate::loss::SolidEdgeLossCode::PartitionIncomplete
            .note("")
            .code));
}

#[test]
fn carrier_reference_order_preserves_a_closed_cube() {
    let bytes = test_support::with_partitions(&[test_support::cube_partition()]);
    let result = SolidEdgeCodec
        .decode(&mut Cursor::new(bytes), &DecodeOptions::default())
        .unwrap();
    let ir = result.ir();
    assert_eq!(ir.geometry.bodies.len(), 1);
    assert_eq!(ir.geometry.faces.len(), 6);
    assert_eq!(ir.geometry.edges.len(), 12);
    assert_eq!(ir.geometry.vertices.len(), 8);
    assert_eq!(ir.geometry.coedges.len(), 24);
    assert_geometry_valid(ir);
    assert!(!result.report().losses.iter().any(|loss| loss.code
        == crate::loss::SolidEdgeLossCode::PartitionUnreadable
            .note("")
            .code));
}

#[test]
fn invalid_tolerance_keeps_body_geometry_with_a_loss() {
    let mut partition = test_support::cube_partition();
    let records = cadmpeg_parasolid::records::PrimaryRecords::read(&partition)
        .unwrap()
        .unwrap();
    let range = records
        .nodes()
        .find(|node| node.kind() == cadmpeg_parasolid::records::node_type::FACE)
        .unwrap()
        .field_range("tolerance")
        .unwrap();
    partition[range].copy_from_slice(&(-1.0f64).to_be_bytes());
    let bytes = test_support::with_partitions(&[partition]);
    let result = SolidEdgeCodec
        .decode(&mut Cursor::new(bytes), &DecodeOptions::default())
        .unwrap();
    assert_eq!(result.ir().geometry.faces.len(), 6);
    assert!(result.report().losses.iter().any(|loss| loss.code
        == crate::loss::SolidEdgeLossCode::TopologyToleranceInvalid
            .note("")
            .code));
    assert_geometry_valid(result.ir());
}

#[test]
fn dangling_face_surface_keeps_the_other_faces() {
    let mut partition = test_support::cube_partition();
    let records = cadmpeg_parasolid::records::PrimaryRecords::read(&partition)
        .unwrap()
        .unwrap();
    let range = records
        .nodes()
        .find(|node| node.kind() == cadmpeg_parasolid::records::node_type::FACE)
        .unwrap()
        .field_range("surface")
        .unwrap();
    partition[range].copy_from_slice(&4000u16.to_be_bytes());
    let bytes = test_support::with_partitions(&[partition]);
    let result = SolidEdgeCodec
        .decode(&mut Cursor::new(bytes), &DecodeOptions::default())
        .unwrap();
    let ir = result.ir();
    assert_eq!(ir.geometry.bodies.len(), 1);
    assert_eq!(ir.geometry.faces.len(), 6);
    assert_eq!(
        ir.geometry
            .surfaces
            .iter()
            .filter(|surface| matches!(
                surface.geometry,
                cadmpeg_ir::geometry::SurfaceGeometry::Solved(
                    cadmpeg_ir::geometry::SolvedSurfaceGeometry::Unknown { .. }
                )
            ))
            .count(),
        1
    );
    let codes: Vec<_> = result
        .report()
        .losses
        .iter()
        .map(|loss| loss.code.clone())
        .collect();
    assert!(codes.contains(
        &crate::loss::SolidEdgeLossCode::SurfaceUnresolved
            .note("")
            .code
    ));
    assert!(!codes.contains(
        &crate::loss::SolidEdgeLossCode::PartitionUnreadable
            .note("")
            .code
    ));
    assert_geometry_valid(ir);
}

#[test]
fn edge_naming_a_fin_outside_any_loop_reads_through_its_radial_fin() {
    use cadmpeg_parasolid::records::node_type;
    let mut partition = test_support::cube_partition();
    let records = cadmpeg_parasolid::records::PrimaryRecords::read(&partition)
        .unwrap()
        .unwrap();
    let edge = records
        .nodes()
        .find(|node| node.kind() == node_type::EDGE)
        .unwrap();
    let named = edge.pointer("halfedge").unwrap();
    let range = records
        .nodes()
        .find(|node| node.kind() == node_type::FIN && node.index() == named)
        .unwrap()
        .field_range("backward")
        .unwrap();
    partition[range].copy_from_slice(&1u16.to_be_bytes());
    let bytes = test_support::with_partitions(&[partition]);
    let result = SolidEdgeCodec
        .decode(&mut Cursor::new(bytes), &DecodeOptions::default())
        .unwrap();
    let ir = result.ir();
    assert_eq!(ir.geometry.faces.len(), 6);
    assert_eq!(ir.geometry.edges.len(), 12);
    assert_eq!(ir.geometry.coedges.len(), 24);
    assert!(!result
        .report()
        .losses
        .iter()
        .any(|loss| loss.code == crate::loss::SolidEdgeLossCode::LoopUnreadable.note("").code));
    assert_geometry_valid(ir);
}

#[test]
fn face_with_no_loop_on_a_plane_is_stated_partial() {
    let mut partition = test_support::disc_partition();
    let records = cadmpeg_parasolid::records::PrimaryRecords::read(&partition)
        .unwrap()
        .unwrap();
    let range = records
        .nodes()
        .find(|node| node.kind() == cadmpeg_parasolid::records::node_type::FACE)
        .unwrap()
        .field_range("loop")
        .unwrap();
    partition[range].copy_from_slice(&1u16.to_be_bytes());
    let result = decode_partition(partition);
    let ir = result.ir();
    assert_eq!(ir.geometry.faces.len(), 1);
    let face = &ir.geometry.faces[0];
    assert!(face.loops.is_partial());
    assert_eq!(face.loops.stated().count(), 0);
    assert!(result.report().losses.iter().any(|loss| loss.code
        == crate::loss::SolidEdgeLossCode::TopologyFaceBoundaryUnstated
            .note("")
            .code
        && loss.message.contains(face.id.as_str())));
    assert_geometry_valid(ir);
}

fn assert_geometry_valid(ir: &cadmpeg_ir::CadIr) {
    let mut geometry = ir.clone();
    geometry.source = None;
    assert!(
        cadmpeg_ir::validate::validate_neutral(&geometry, Vec::new())
            .unwrap()
            .is_ok()
    );
}

#[test]
fn body_resolution_sets_document_linear_tolerance_in_millimetres() {
    let partitions: Vec<_> = [2.0e-8_f64, 4.0e-8]
        .into_iter()
        .map(|resolution| {
            let mut partition = test_support::sphere_partition();
            let records = cadmpeg_parasolid::records::PrimaryRecords::read(&partition)
                .unwrap()
                .unwrap();
            let range = records
                .nodes()
                .find(|node| node.kind() == cadmpeg_parasolid::records::node_type::BODY)
                .unwrap()
                .field_range("res_linear")
                .unwrap();
            partition[range].copy_from_slice(&resolution.to_be_bytes());
            partition
        })
        .collect();
    let result = SolidEdgeCodec
        .decode(
            &mut Cursor::new(test_support::with_partitions(&partitions)),
            &DecodeOptions::default(),
        )
        .unwrap();
    let expected_mm = 4.0e-5;
    assert!((result.ir().tolerances.linear.get() - expected_mm).abs() < EPS_RESOLUTION_MM);
    assert_eq!(
        result.ir().tolerances.angular.get(),
        MODELLER_ANGULAR_RADIANS
    );
    assert_geometry_valid(result.ir());
}

/// A sphere partition whose BODY stores `resolution` metres as `res_linear`.
fn sphere_partition_with_resolution(resolution: f64) -> Vec<u8> {
    let mut partition = test_support::sphere_partition();
    let range = cadmpeg_parasolid::records::PrimaryRecords::read(&partition)
        .unwrap()
        .unwrap()
        .nodes()
        .find(|node| node.kind() == cadmpeg_parasolid::records::node_type::BODY)
        .unwrap()
        .field_range("res_linear")
        .unwrap();
    partition[range].copy_from_slice(&resolution.to_be_bytes());
    partition
}

#[test]
fn an_invalid_body_resolution_leaves_the_largest_stored_resolution() {
    let partitions = [
        sphere_partition_with_resolution(1.0e-12),
        sphere_partition_with_resolution(0.0),
    ];
    let result = SolidEdgeCodec
        .decode(
            &mut Cursor::new(test_support::with_partitions(&partitions)),
            &DecodeOptions::default(),
        )
        .unwrap();
    assert_eq!(result.ir().geometry.bodies.len(), 2);
    assert!((result.ir().tolerances.linear.get() - 1.0e-9).abs() < EPS_RESOLUTION_MM);
    assert_geometry_valid(result.ir());
}

#[test]
fn a_container_decode_states_the_modeller_precision() {
    let result = SolidEdgeCodec
        .decode(
            &mut Cursor::new(test_support::with_partitions(&[
                test_support::sphere_partition(),
            ])),
            &DecodeOptions {
                depth: cadmpeg_core::decode::DecodeDepth::Container,
                ..DecodeOptions::default()
            },
        )
        .unwrap();
    assert!(result.ir().geometry.bodies.is_empty());
    assert_eq!(
        result.ir().tolerances.linear.get(),
        MODELLER_DEFAULT_LINEAR_MM
    );
    assert_eq!(
        result.ir().tolerances.angular.get(),
        MODELLER_ANGULAR_RADIANS
    );
}

#[test]
fn invalid_body_resolution_keeps_geometry_and_reports_a_loss() {
    let mut partition = test_support::sphere_partition();
    let records = cadmpeg_parasolid::records::PrimaryRecords::read(&partition)
        .unwrap()
        .unwrap();
    let range = records
        .nodes()
        .find(|node| node.kind() == cadmpeg_parasolid::records::node_type::BODY)
        .unwrap()
        .field_range("res_linear")
        .unwrap();
    partition[range].copy_from_slice(&0.0f64.to_be_bytes());
    let result = SolidEdgeCodec
        .decode(
            &mut Cursor::new(test_support::with_partitions(&[partition])),
            &DecodeOptions::default(),
        )
        .unwrap();
    assert_eq!(result.ir().geometry.bodies.len(), 1);
    assert_eq!(
        result.ir().tolerances.linear.get(),
        MODELLER_DEFAULT_LINEAR_MM
    );
    let body = result.ir().geometry.bodies[0].id.as_str();
    assert!(result.report().losses.iter().any(|loss| loss.code
        == crate::loss::SolidEdgeLossCode::BodyResolutionInvalid
            .note("")
            .code
        && loss.message.contains(body)));
    assert_geometry_valid(result.ir());
}

/// The sphere partition under an embedded schema whose BODY declaration
/// deletes `res_linear`.
fn sphere_partition_without_resolution() -> Vec<u8> {
    let base = test_support::sphere_partition();
    let records = cadmpeg_parasolid::records::PrimaryRecords::read(&base)
        .unwrap()
        .unwrap();
    let description = b": TRANSMIT FILE (partition)";
    let schema = b"SCH_999_999_13006";
    let mut bytes = b"PS".to_vec();
    bytes.extend_from_slice(&u32::try_from(description.len()).unwrap().to_be_bytes());
    bytes.extend_from_slice(description);
    bytes.extend_from_slice(&u32::try_from(schema.len()).unwrap().to_be_bytes());
    bytes.extend_from_slice(schema);
    bytes.extend_from_slice(&250u16.to_be_bytes());
    bytes.extend_from_slice(&0u32.to_be_bytes());
    let mut declared = std::collections::BTreeSet::new();
    let mut end = 0;
    for node in records.nodes() {
        let range = node.range();
        bytes.extend_from_slice(&base[range.start..range.start + 2]);
        let is_body = node.kind() == cadmpeg_parasolid::records::node_type::BODY;
        if declared.insert(node.kind()) {
            if is_body {
                bytes.push(22);
                bytes.extend_from_slice(&[b'C'; 8]);
                bytes.push(b'D');
                bytes.extend_from_slice(&[b'C'; 14]);
                bytes.push(b'Z');
            } else {
                bytes.push(255);
            }
        }
        if is_body {
            let resolution = node.field_range("res_linear").unwrap();
            bytes.extend_from_slice(&base[range.start + 2..resolution.start]);
            bytes.extend_from_slice(&base[resolution.end..range.end]);
        } else {
            bytes.extend_from_slice(&base[range.start + 2..range.end]);
        }
        end = range.end;
    }
    bytes.extend_from_slice(&base[end..]);
    bytes
}

#[test]
fn a_body_without_a_stored_resolution_keeps_geometry_and_reports_a_loss() {
    let result = SolidEdgeCodec
        .decode(
            &mut Cursor::new(test_support::with_partitions(&[
                sphere_partition_without_resolution(),
            ])),
            &DecodeOptions::default(),
        )
        .unwrap();
    assert_eq!(result.ir().geometry.bodies.len(), 1);
    assert_eq!(
        result.ir().tolerances.linear.get(),
        MODELLER_DEFAULT_LINEAR_MM
    );
    let body = result.ir().geometry.bodies[0].id.as_str();
    let unread = crate::loss::SolidEdgeLossCode::BodyResolutionUnread
        .note("")
        .code;
    assert!(
        result
            .report()
            .losses
            .iter()
            .any(|loss| loss.code == unread && loss.message.contains(body)),
        "{:?}",
        result.report().losses
    );
    assert_geometry_valid(result.ir());
}

#[test]
fn empty_partition_preserves_stated_body_resolution() {
    let mut partition = test_support::sphere_partition();
    let (resolution, first_node) = {
        let records = cadmpeg_parasolid::records::PrimaryRecords::read(&partition)
            .unwrap()
            .unwrap();
        let body = records
            .nodes()
            .find(|node| node.kind() == cadmpeg_parasolid::records::node_type::BODY)
            .unwrap();
        (body.field_range("res_linear").unwrap(), body.range().start)
    };
    let mut empty = partition[..first_node].to_vec();
    empty.extend_from_slice(&[0, 1, 0, 1]);
    partition[resolution].copy_from_slice(&1.0e-12_f64.to_be_bytes());
    for partitions in [[partition.clone(), empty.clone()], [empty, partition]] {
        let result = SolidEdgeCodec
            .decode(
                &mut Cursor::new(test_support::with_partitions(&partitions)),
                &DecodeOptions::default(),
            )
            .unwrap();
        assert_eq!(result.ir().geometry.bodies.len(), 1);
        assert!((result.ir().tolerances.linear.get() - 1.0e-9).abs() < EPS_RESOLUTION_MM);
        assert_geometry_valid(result.ir());
    }
}

#[test]
fn vertexless_ring_edge_is_bounded_by_one_vertex_on_its_carrier() {
    let bytes = test_support::with_partitions(&[test_support::disc_partition()]);
    let result = SolidEdgeCodec
        .decode(&mut Cursor::new(bytes), &DecodeOptions::default())
        .unwrap();
    let ir = result.ir();
    assert_eq!(ir.geometry.faces.len(), 1);
    assert_eq!(ir.geometry.edges.len(), 1);
    assert_eq!(ir.geometry.vertices.len(), 1);
    assert_eq!(ir.geometry.points.len(), 1);
    let ends = ir.geometry.edges[0].ends.as_ref().expect("bounded ring");
    assert_eq!(ends.start, ends.end);
    assert_eq!(ends.start, ir.geometry.vertices[0].id);
    let position = ir.geometry.points[0].position().get();
    let radius_mm = 5.0;
    let distance = (position.x * position.x + position.y * position.y).sqrt();
    assert!((distance - radius_mm).abs() < EPS_RING_VERTEX_MM);
    assert!(position.z.abs() < EPS_RING_VERTEX_MM);
    let range = ir.geometry.edges[0]
        .carrier
        .param_range()
        .expect("ring range")
        .get();
    assert!((range[1] - range[0] - std::f64::consts::TAU).abs() < EPS_RING_VERTEX_MM);
    assert_geometry_valid(ir);
}

/// A vertexless edge with no carrier is given up with its node's span in the
/// partition stream, from the start of the node sequence after any text
/// header, and drops its loop, coedge, and face.
#[test]
fn a_vertexless_edge_with_no_carrier_is_given_up_at_its_node() {
    let partition = test_support::uncarried_ring_partition();
    let records = cadmpeg_parasolid::records::PrimaryRecords::read(&partition)
        .unwrap()
        .unwrap();
    let edge = records
        .nodes()
        .find(|node| node.kind() == cadmpeg_parasolid::records::node_type::EDGE)
        .unwrap()
        .range();
    let header = b"**END_OF_HEADER\n".to_vec();
    for prefix in [Vec::new(), header] {
        let mut stream = prefix.clone();
        stream.extend_from_slice(&partition);
        let result = decode_partition(stream);
        let geometry = &result.ir().geometry;
        assert!(geometry.edges.is_empty());
        assert!(geometry.faces.is_empty());
        assert!(geometry.loops.is_empty());
        assert!(geometry.coedges.is_empty());
        let charges: Vec<_> = result
            .report()
            .losses
            .iter()
            .filter(|loss| loss.code == cadmpeg_ir::unknown::part::record_unreadable_kind())
            .collect();
        let [charge] = charges.as_slice() else {
            panic!("one record.unreadable: {charges:?}");
        };
        let offset = prefix.len() + edge.start;
        let span = format!(
            " stream=\"solidedge:PARASOLID/STREAM0.P_B\" offset={offset} length={} given up: \
             a vertexless edge has no carrier; dropped=[",
            edge.len()
        );
        assert!(
            charge.message.starts_with("record solidedge:brep:edge#"),
            "{charge:?}"
        );
        assert!(charge.message.contains(&span), "{charge:?}");
        assert_eq!(
            charge
                .provenance
                .as_ref()
                .map(|provenance| provenance.offset),
            Some(u64::try_from(offset).unwrap())
        );
        let dropped = charge
            .message
            .split_once("; dropped=[")
            .and_then(|(_, tail)| tail.split_once(']'))
            .map(|(list, _)| list.split(", ").collect::<Vec<_>>())
            .unwrap();
        for kind in [
            "solidedge:brep:coedge#",
            "solidedge:brep:face#",
            "solidedge:brep:loop#",
        ] {
            assert!(dropped.iter().any(|id| id.starts_with(kind)), "{charge:?}");
        }
    }
}

/// A swept ring face whose edge has neither a vertex nor a carrier is
/// dropped with its loop; its surface leaves with it, and the charge names
/// the surface's procedural construction among the records dropped.
#[test]
fn a_dropped_swept_face_names_the_construction_of_its_surface() {
    let kept = decode_partition(test_support::swept_ring_partition(NULL_REAL));
    let [construction] = kept.ir().geometry.procedural_surfaces.as_slice() else {
        panic!("the swept surface has one construction");
    };
    let result = decode_partition(test_support::uncarried_swept_ring_partition(NULL_REAL));
    let geometry = &result.ir().geometry;
    assert!(geometry.faces.is_empty());
    assert!(geometry.procedural_surfaces.is_empty());
    let id = construction.id.as_str();
    assert!(
        result.report().losses.iter().any(|loss| {
            loss.code == cadmpeg_ir::unknown::part::record_unreadable_kind()
                && loss
                    .message
                    .split_once("; dropped=[")
                    .and_then(|(_, tail)| tail.split_once(']'))
                    .is_some_and(|(list, _)| list.split(", ").any(|named| named == id))
        }),
        "{:#?}",
        result.report().losses
    );
}

fn swept_ring_direction(scale: f64) -> f64 {
    let bytes = test_support::with_partitions(&[test_support::swept_ring_partition(scale)]);
    let result = SolidEdgeCodec
        .decode(&mut Cursor::new(bytes), &DecodeOptions::default())
        .unwrap();
    let geometry = &result.ir().geometry;
    let surface = geometry
        .surfaces
        .iter()
        .find(|candidate| candidate.id == geometry.faces[0].surface)
        .unwrap();
    let cadmpeg_ir::geometry::SurfaceGeometry::Procedural { construction, .. } = &surface.geometry
    else {
        panic!("a swept surface is a procedural surface");
    };
    let procedural = geometry
        .procedural_surfaces
        .iter()
        .find(|candidate| &candidate.id == construction)
        .unwrap();
    let cadmpeg_ir::geometry::procedural_surfaces::ProceduralSurfaceDefinition::LinearSweep(sweep) =
        procedural.definition()
    else {
        panic!("a swept surface is a linear sweep");
    };
    let direction = sweep.direction().get();
    assert!(direction.x.abs() < EPS_RING_VERTEX_MM && direction.y.abs() < EPS_RING_VERTEX_MM);
    assert_eq!(Some(sweep.directrix()), geometry.edges[0].carrier.curve());
    assert_eq!(geometry.curves.len(), 1);
    assert_geometry_valid(result.ir());
    direction.z
}

#[test]
fn swept_surface_with_a_stated_scale_keeps_an_unsupported_carrier() {
    let bytes = test_support::with_partitions(&[test_support::swept_ring_partition(0.01)]);
    let result = SolidEdgeCodec
        .decode(&mut Cursor::new(bytes), &DecodeOptions::default())
        .unwrap();
    assert!(result.ir().geometry.procedural_surfaces.is_empty());
    assert!(result.report().losses.iter().any(|loss| loss.code
        == crate::loss::SolidEdgeLossCode::SurfaceUnsupported
            .note("")
            .code));
}

#[test]
fn swept_surface_with_a_null_scale_sweeps_at_unit_parameter_speed() {
    assert!((swept_ring_direction(NULL_REAL) - 1.0).abs() < EPS_RING_VERTEX_MM);
}

#[test]
fn full_decode_states_the_document_unit_system() {
    use cadmpeg_ir::design::units::UnitSymbol;
    use test_support::RecordValue;

    let cluster = test_support::property_set(
        2,
        &[
            ("DefaultDistancePrimary", RecordValue::Word(2, 64)),
            ("DefaultAnglePrimary", RecordValue::Word(2, 357)),
        ],
    );
    let result = SolidEdgeCodec
        .decode(
            &mut Cursor::new(test_support::with_cluster(&cluster)),
            &DecodeOptions::default(),
        )
        .unwrap();
    let definitions = &result.ir().model.product_definitions;
    assert_eq!(definitions.len(), 1);
    let units = definitions[0].units.as_ref().unwrap();
    assert_eq!(
        (units.length, units.angle),
        (UnitSymbol::In, UnitSymbol::Deg)
    );
}

#[test]
fn full_decode_without_cluster_reports_unread_units() {
    let result = SolidEdgeCodec
        .decode(
            &mut Cursor::new(test_support::compound()),
            &DecodeOptions::default(),
        )
        .unwrap();
    assert!(result.ir().model.product_definitions[0].units.is_none());
    assert!(result
        .report()
        .losses
        .iter()
        .any(|loss| loss.code.local_code() == "units.unread"));
}
