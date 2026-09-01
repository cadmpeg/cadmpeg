// SPDX-License-Identifier: Apache-2.0
//! End-to-end contracts over synthesized Creo PSB byte images.
#![allow(clippy::unwrap_used)]

use std::io::Cursor;

use cadmpeg_core::decode::InspectOptions;
use cadmpeg_ir::codec::{Codec, Confidence, DecodeOptions};
use cadmpeg_ir::features::FeatureDefinition;
use cadmpeg_ir::geometry::SurfaceGeometry;
use cadmpeg_ir::sketches::SketchConstraintDefinition;

use crate::container::role;
use crate::test_support::*;
use crate::CreoCodec;

fn decode(bytes: Vec<u8>) -> cadmpeg_ir::codec::DecodeResult {
    CreoCodec
        .decode(&mut Cursor::new(bytes), &DecodeOptions::default())
        .expect("synthesized Creo part should decode")
}

fn assert_valid(result: &cadmpeg_ir::codec::DecodeResult) {
    let validation = cadmpeg_ir::validate_neutral(result.ir(), result.report().losses.clone());
    assert!(validation.is_ok(), "{validation:#?}");
    assert!(result.ir().native.namespace("creo").is_some());
}

fn closed_plane_brep() -> Vec<u8> {
    let mut payload = b"srf_array\0\xf8\x04".to_vec();
    for (surface, reversed, u, v, origin) in [
        (1, true, [0.0, 1.0, 0.0], [0.0, 0.0, 1.0], [0.0, 0.0, 0.0]),
        (2, false, [0.0, 0.0, 1.0], [1.0, 0.0, 0.0], [0.0, 0.0, 0.0]),
        (3, false, [1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 0.0]),
        (
            4,
            false,
            [-2.0, -1.0, 2.0],
            [2.0, -2.0, 1.0],
            [1.0, 0.0, 0.0],
        ),
    ] {
        push_generated_plane_row(&mut payload, surface, reversed, u, v, origin);
    }
    payload.extend_from_slice(b"crv_array\0\xf3\xf8\x06topol_ref_data\0");
    for (curve, faces, next) in [
        (10, [1, 2], [12, 13]),
        (11, [1, 3], [10, 15]),
        (12, [1, 4], [11, 14]),
        (13, [2, 3], [14, 11]),
        (14, [2, 4], [10, 15]),
        (15, [3, 4], [13, 12]),
    ] {
        push_generated_topology_row(&mut payload, curve, faces, next);
    }
    build_prt("integration", &[("VisibGeom", payload)])
}

#[test]
fn psb_pipeline_aligns_detection_inspection_layout_and_section_roles() {
    let bytes = build_prt(
        "integration",
        &[
            ("ND:0:VisibGeom:1", visibgeom_payload(7, 9)),
            ("AllFeatur", b"feature bytes".to_vec()),
            ("THMB_IMG_MAIN", jpeg_payload()),
        ],
    );
    assert_eq!(CreoCodec.detect(&bytes), Confidence::High);
    let summary = CreoCodec
        .inspect(&mut Cursor::new(bytes), &InspectOptions::default())
        .expect("Creo inspection");
    assert_eq!(summary.format(), "creo");
    assert_eq!(summary.container_kind, "psb");
    assert_eq!(summary.entries.len(), 3);
    assert!(summary.notes.iter().any(|note| note.contains("layout: ND")));
    assert!(summary
        .notes
        .iter()
        .any(|note| note.contains("srf_array=7")));
    assert!(summary
        .entries
        .iter()
        .any(|entry| entry.role == role::THUMBNAIL));
}

#[test]
fn visible_geometry_pipeline_places_a_complete_analytic_prototype() {
    let mut payload = b"srf_array\0\xf8\x01".to_vec();
    payload.extend_from_slice(&[7, 0x24, 4, 0x01, 0, 0]);
    push_named_analytic_prototype(&mut payload, "cylinder", &[("radius", 1.0)]);
    payload.extend_from_slice(b"crv_array\0\xf3\xf8\0");

    let result = decode(build_prt("integration", &[("ND:0:VisibGeom:0", payload)]));
    assert!(result.report().geometry_transferred());
    assert!(result.ir().model.surfaces.iter().any(|surface| {
        matches!(surface.geometry, SurfaceGeometry::Cylinder { radius, .. } if radius == 1.0)
    }));
    assert_valid(&result);
}

#[test]
fn topology_pipeline_reconstructs_a_closed_plane_intersection_solid() {
    let result = decode(closed_plane_brep());
    assert_eq!(result.ir().model.points.len(), 4);
    assert_eq!(result.ir().model.vertices.len(), 4);
    assert_eq!(result.ir().model.edges.len(), 6);
    assert_eq!(result.ir().model.faces.len(), 4);
    assert_eq!(result.ir().model.coedges.len(), 12);
    assert_eq!(result.ir().model.pcurves.len(), 12);
    assert_eq!(result.ir().model.bodies.len(), 1);
    assert_eq!(
        result.ir().model.bodies[0].kind,
        cadmpeg_ir::topology::BodyKind::Solid
    );
    assert_valid(&result);
}

#[test]
fn datum_pipeline_merges_placed_geometry_with_ordered_feature_history() {
    let mut datum = b"srf_array\0\xf8\x01".to_vec();
    datum.extend([4, 0x22, 4, 1, 1, 0]);
    datum.extend([0x0f; 4]);
    for value in [2.0_f64, 0.0, 3.0, -2.0, 0.0, -3.0] {
        if value == 0.0 {
            datum.push(0x0f);
        } else {
            let mut bytes = value.to_be_bytes();
            bytes[0] = if value.is_sign_negative() { 0x2d } else { 0x46 };
            datum.extend(bytes);
        }
    }
    let result = decode(build_prt(
        "integration",
        &[
            ("ActDatums", datum),
            ("MdlStatus", b"Round id 3\0Datum Plane id 4\0".to_vec()),
        ],
    ));
    let datum_feature = result
        .ir()
        .model
        .features
        .iter()
        .find(|feature| feature.id.as_str() == "creo:model:feature#4")
        .expect("datum feature");
    assert!(matches!(
        datum_feature.definition,
        FeatureDefinition::DatumPlane { .. }
    ));
    assert!(result
        .ir()
        .model
        .surfaces
        .iter()
        .any(|surface| { matches!(surface.geometry, SurfaceGeometry::Plane { .. }) }));
    assert_valid(&result);
}

#[test]
fn featdefs_pipeline_projects_mixed_sketch_entities_and_native_constraints() {
    let mut payload =
        b"feat_defs_40\0segtab_ptr\0\xf8\x05\xf7\x01\xfb\xe2schema\xf2\xf7\x01\xe2".to_vec();
    payload.extend_from_slice(&[2, 0, 0, 0, 7, 8, 0xf6, 0, 0, 0xf6, 0xf6, 42, 0xe2, 0xe3]);
    payload.extend_from_slice(&[3, 0, 0, 0, 8, 9, 10, 1, 0, 11, 12, 43, 0xe2, 0xe3]);
    payload.extend_from_slice(&[2, 0, 0, 0, 9, 10, 0xf6, 0, 0, 0xf6, 0xf6, 0x80, 0xe3, 0xe2]);
    payload.extend_from_slice(&[0xe3, 0xe2, 0, 0xf6, 0xe2, 0xc0, 0x80]);
    payload.extend_from_slice(&[2, 0, 0, 0, 11, 12, 0xf6, 0, 0, 0xf6, 0xf6, 0, 0xe2]);
    payload.extend_from_slice(&[0xe3, 0xe2, 0, 0xf6, 0xe2]);
    payload.extend_from_slice(&[5, 1, 0, 0xe4, 13, 0xe4, 0xf6, 0, 2, 0xf6, 0xf6, 4, 0xe2]);
    payload.extend_from_slice(b"dimtab_ptr\0");

    let result = decode(build_prt("integration", &[("FeatDefs", payload)]));
    assert_eq!(result.ir().model.sketches.len(), 1);
    assert_eq!(result.ir().model.sketch_entities.len(), 5);
    assert_eq!(result.ir().model.sketch_constraints.len(), 7);
    assert!(result
        .ir()
        .model
        .sketch_constraints
        .iter()
        .any(|constraint| {
            matches!(
                constraint.definition,
                SketchConstraintDefinition::Native { .. }
            )
        }));
    assert_valid(&result);
}

#[test]
fn featdefs_pipeline_retains_solver_relations_and_resolved_dimension_inputs() {
    let mut payload = b"feat_defs_40\0relat_ptr\0\xf4\x04\xf8\x04\xf7\x6a\xfb\xe2\
        \xe0\x01id\0\xe0\x01used\0\xe0\x01type\0\xf1\xf7\x6a\xe2\
        \x34\x00\x05\x01\xf6\xe4\x00\xe6\x0f\x10\x0f\xe4\x00\x00\x00\xe2\
        \x35\x01\x07\x29\x32\xf6\x00\xe6\x0f\x10\x0f\xe4\x01\x2a\x03\xe2"
        .to_vec();
    payload.extend_from_slice(
        b"skamp_ptr\0\xf3\xf8\x01\xf7\x6b\xfb\xe2\
          \xe0\x01id\0\x05\xe0\x01type\0\x02\xe0\x01flags\0\x03\
          \xe0\x01status\0\x04\xe0\x01items\0\xf8\x01\xf7\x6c\xfb\xe2\
          \xe0\x01ent_id\0\x2a\xe0\x01sense\0\x01\xf1\xf7\x6c\xe2\
          \xf3\xf7\x6b\xe2\
          triples_ptr\0\xf4\x04\xf8\x02\xf7\x6d\xfb\xe2\
          \xe0\x01rel_id\0\x07\xe0\x01eqn_id\0\x08\xe0\x01skamp_id\0\x05\
          \xf1\xf7\x6d\xe2\xf6\x09\x05\xe2",
    );
    let result = decode(build_prt("integration", &[("FeatDefs", payload)]));
    let sketches = &result.ir().native.namespace("creo").unwrap().arenas["sketches"];
    assert_eq!(sketches.len(), 1);
    let fields = sketches[0].fields();
    let headers = fields["table_headers"].as_array().unwrap();
    assert!(headers
        .iter()
        .any(|header| header["kind"] == "solver_incidences"));
    assert!(headers
        .iter()
        .any(|header| header["kind"] == "relation_triples"));
    assert_valid(&result);
}

#[test]
fn container_only_pipeline_preserves_geometry_thumbnail_and_design_sections() {
    let mut geometry = visibgeom_payload(1, 0);
    geometry.extend_from_slice(&[7, 0x22, 4, 0x01, 0, 0]);
    let bytes = build_prt(
        "integration",
        &[
            ("VisibGeom", geometry),
            ("FeatDefs", b"feat_defs_40\0".to_vec()),
            ("THMB_IMG_MAIN", jpeg_payload()),
        ],
    );
    let result = CreoCodec
        .decode(
            &mut Cursor::new(bytes),
            &DecodeOptions {
                container_only: true,
                ..DecodeOptions::default()
            },
        )
        .expect("container-only Creo decode");
    assert!(result.report().container_only());
    assert!(!result.report().geometry_transferred());
    assert!(result.ir().model.surfaces.is_empty());
    assert!(result.ir().model.features.is_empty());
    assert_eq!(result.ir().native_unknowns("creo").unwrap().len(), 2);
    assert!(!result.source_fidelity().retained_records.is_empty());
    assert_valid(&result);
}
