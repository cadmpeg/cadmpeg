// SPDX-License-Identifier: Apache-2.0
//! End-to-end contracts over synthesized Creo PSB byte images.
#![allow(clippy::unwrap_used)]

use cadmpeg_test_support::EditableDecodeResult;

use crate::test_support::build_prt;
use crate::test_support::jpeg_payload;
use crate::test_support::push_generated_plane_row;
use crate::test_support::push_generated_topology_row;
use crate::test_support::push_named_analytic_prototype;
use crate::test_support::visibgeom_payload;
use cadmpeg_core::container::ContainerRole;

use std::io::Cursor;

use cadmpeg_core::decode::InspectOptions;
use cadmpeg_ir::codec::{Codec, Confidence, DecodeOptions};
use cadmpeg_ir::features::{FeatureDefinition, FeatureOperation};
use cadmpeg_ir::geometry::{SolvedSurfaceGeometry, SurfaceGeometry};
use cadmpeg_ir::sketches::SketchConstraintDefinitionInput;

use crate::CreoCodec;

fn decode(bytes: Vec<u8>) -> EditableDecodeResult {
    EditableDecodeResult::from(
        CreoCodec
            .decode(&mut Cursor::new(bytes), &DecodeOptions::default())
            .expect("synthesized Creo part should decode"),
    )
}

fn assert_valid(result: &EditableDecodeResult) {
    let validation = cadmpeg_ir::validate_neutral(result.ir(), result.report().losses.clone())
        .expect("validate the neutral model");
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
    assert_eq!(
        cadmpeg_test_support::detection::confidence(&CreoCodec, &bytes),
        Confidence::High
    );
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
        .any(|entry| entry.role == ContainerRole::Thumbnail));
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
        matches!(surface.geometry, SurfaceGeometry::Solved(SolvedSurfaceGeometry::Cylinder(cylinder_surface))
        if {
            let radius = cylinder_surface.radius().get();
            radius == 1.0
        })
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
fn rotation_plane_pipeline_shares_boundary_and_origin_admission() {
    for trailing_scalar in [false, true] {
        let mut payload = b"srf_array\0\xf8\x01".to_vec();
        payload.extend_from_slice(&[7, 0x22, 4, 1, 0, 0]);
        payload.extend_from_slice(&[0x0f; 10]);
        payload.push(0xe3);
        for (index, value) in [0.6_f64, 0.0, 0.8, 0.0, 0.0, 0.0, 0.8, 0.0, -0.6]
            .into_iter()
            .enumerate()
        {
            if value == 0.0 {
                payload.push(if index == 4 { 0x10 } else { 0x18 });
            } else {
                let bytes = value.to_be_bytes();
                payload.push(if value > 0.0 { 0x41 } else { 0x45 });
                payload.extend_from_slice(&bytes[1..if value > 0.0 { 8 } else { 7 }]);
            }
        }
        payload.extend_from_slice(&[0x2d, 0x10, 0, 0, 0, 0, 0, 0]);
        payload.extend_from_slice(&[0x8b, 0, 0, 0, 0, 0, 0]);
        payload.extend_from_slice(&[0x46, 0x20, 0, 0, 0, 0, 0, 0]);
        if trailing_scalar {
            payload.push(0x0f);
        }
        payload.push(0xe3);
        payload.extend_from_slice(b"crv_array\0\xf3\xf8\0");
        let result = decode(build_prt("integration", &[("VisibGeom", payload)]));
        let plane = result
            .ir()
            .model
            .surfaces
            .iter()
            .find_map(|surface| match &surface.geometry {
                SurfaceGeometry::Solved(SolvedSurfaceGeometry::Plane(plane)) => Some(plane),
                _ => None,
            });
        if trailing_scalar {
            assert!(plane.is_none());
            assert!(result.report().losses.iter().any(|loss| {
                loss.code == crate::loss::CreoLossCode::PlaneLocalSystemUnresolved.kind()
            }));
            assert!(result.report().losses.iter().any(|loss| {
                loss.code == crate::loss::CreoLossCode::VisibGeomSurfaceUntransferred.kind()
            }));
        } else {
            assert_eq!(
                plane.expect("placed plane").origin().get(),
                cadmpeg_ir::math::Point3::new(4.0, 2.0, -8.0)
            );
        }
        assert_valid(&result);
    }
}

#[test]
fn plane_carrier_report_excludes_untransferred_rows() {
    let fixture = include_bytes!("../tests/golden/fixtures/axis_aligned_plane.prt");
    for compatible in [true, false] {
        let mut input = fixture.to_vec();
        if !compatible {
            let prefix = [0x0f, 0x0f, 0x0f, 0x0f, 0x43];
            let offset = input
                .windows(prefix.len())
                .position(|bytes| bytes == prefix)
                .expect("literal envelope prefix");
            input[offset + 12] = 0x43;
            input[offset + 13] = 0xf0;
            input[offset + 15] = 0x0f;
        }
        let result = decode(input);
        let transferred = result
            .ir()
            .model
            .surfaces
            .iter()
            .filter(|surface| {
                matches!(
                    surface.geometry.solved(),
                    Some(SolvedSurfaceGeometry::Plane(_))
                )
            })
            .count();
        assert_eq!(transferred, usize::from(compatible));
        let losses = &result.report().losses;
        assert_eq!(
            losses
                .iter()
                .any(|loss| loss.code == crate::loss::CreoLossCode::CarrierVisibGeomPlanes.kind()),
            compatible
        );
        assert_eq!(
            losses
                .iter()
                .any(|loss| loss.code == crate::loss::CreoLossCode::PlaneCarrierUnresolved.kind()),
            !compatible
        );
        assert_eq!(result.report().geometry_transferred(), compatible);
        assert_valid(&result);
    }
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
        datum_feature.evaluation.definition(),
        FeatureDefinition::Operation(FeatureOperation::DatumPlane { .. })
    ));
    assert!(result.ir().model.surfaces.iter().any(|surface| {
        matches!(
            surface.geometry,
            SurfaceGeometry::Solved(SolvedSurfaceGeometry::Plane(_))
        )
    }));
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
                constraint.definition.kind(),
                SketchConstraintDefinitionInput::Native { .. }
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
    let sketches = &result.ir().native.namespace("creo").unwrap().arenas()["sketches"];
    assert_eq!(sketches.len(), 1);
    let fields = sketches[0].fields().unwrap();
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
    let result = EditableDecodeResult::from(
        CreoCodec
            .decode(
                &mut Cursor::new(bytes),
                &DecodeOptions {
                    depth: cadmpeg_core::decode::DecodeDepth::Container,
                    ..DecodeOptions::default()
                },
            )
            .expect("container-only Creo decode"),
    );
    assert!(result.report().container_only());
    assert!(!result.report().geometry_transferred());
    assert!(result.ir().model.surfaces.is_empty());
    assert!(result.ir().model.features.is_empty());
    assert_eq!(result.ir().native_unknowns("creo").unwrap().len(), 2);
    assert!(!result.source_fidelity().retained_records().is_empty());
    assert_valid(&result);
}

mod carrier_rejection;

mod recovery;

#[test]
fn rotation_cylinder_pipeline_requires_the_stored_envelope_and_radius() {
    fn payload(radius: f64) -> Vec<u8> {
        fn scalar(bytes: &mut Vec<u8>, value: f64) {
            let raw = value.to_be_bytes();
            assert_eq!(raw[0], 0x40);
            bytes.push(0x2d);
            bytes.extend_from_slice(&raw[1..]);
        }
        let mut bytes = b"srf_array\0\xf8\x02".to_vec();
        bytes.extend_from_slice(&[7, 0x24, 4, 1, 0, 0, 0x18, 0xe4, 0x12]);
        for value in [4.0, 4.0, 6.0, 8.0, 6.0, 8.0, 12.0] {
            scalar(&mut bytes, value);
        }
        bytes.extend_from_slice(&[0xf7, 0x25, 0xe3]);
        bytes.extend_from_slice(&[0x0f, 0x18, 0xe5, 0x0f, 0x18, 0xe5, 0x0f]);
        for value in [4.0, 6.0, 8.0] {
            scalar(&mut bytes, value);
        }
        bytes.extend_from_slice(if radius == 2.0 {
            &[0x2f, 0x00, 0x00]
        } else {
            &[0x2f, 0x08, 0x00]
        });
        bytes.push(0xe3);
        bytes.extend_from_slice(&[8, 0x22, 4, 1, 0, 0]);
        bytes.extend_from_slice(&[0x18; 10]);
        bytes.extend_from_slice(&[
            0xe3, 0x0f, 0x18, 0xe5, 0x0f, 0x18, 0xe5, 0x0f, 0x18, 0xe5, 0xe1, 0xf7, 0x25, 0xe3,
        ]);
        bytes.extend_from_slice(b"crv_array\0\xf3\xf8\0");
        bytes
    }
    for radius in [2.0, 3.0] {
        let result = decode(build_prt(
            "integration",
            &[("ND:0:VisibGeom:0", payload(radius))],
        ));
        let cylinders = result
            .ir()
            .model
            .surfaces
            .iter()
            .filter_map(|surface| match &surface.geometry {
                SurfaceGeometry::Solved(SolvedSurfaceGeometry::Cylinder(cylinder)) => {
                    Some(cylinder)
                }
                _ => None,
            })
            .collect::<Vec<_>>();
        assert!(result.ir().model.surfaces.iter().any(|surface| {
            surface.id.as_str() == "creo:visibgeom:surface#8"
                && matches!(&surface.geometry, SurfaceGeometry::Solved(SolvedSurfaceGeometry::Plane(plane))
                    if plane.origin().get() == cadmpeg_ir::math::Point3::new(0.0, 0.0, 0.0))
        }));
        if radius == 2.0 {
            assert_eq!(cylinders.len(), 1);
            assert_eq!(cylinders[0].radius().get(), 2.0);
            assert_eq!(cylinders[0].origin().get(), [4.0, 6.0, 8.0].into());
            assert_eq!(
                *cylinders[0].frame().axis().as_raw(),
                [0.0, 0.0, 1.0].into()
            );
            assert_eq!(
                *cylinders[0].frame().reference().as_raw(),
                [1.0, 0.0, 0.0].into()
            );
        } else {
            assert!(cylinders.is_empty());
            assert!(result.report().losses.iter().any(|loss| {
                loss.code == crate::loss::CreoLossCode::CylinderRotationEnvelopeUnresolved.kind()
            }));
        }
        assert_valid(&result);
    }
}

#[test]
fn compact_rotation_cylinder_decode_and_validation_share_admission() {
    let body = [
        0x11, 0x18, 0x13, 0x2f, 0x10, 0, 0x2f, 0, 0, 0x18, 0x48, 0x10, 0, 0x2f, 0x18, 0, 0x2f,
        0x10, 0, 0x18, 0xe3, 0x0f, 0x18, 0xe6, 0x10, 0x18, 0x0f, 0x18, 0x2f, 0x10, 0, 0xe4, 0x48,
        0, 0, 0x2f, 0, 0, 0xe3,
    ];
    for section in ["ActDatums", "VisibGeom"] {
        for agrees in [true, false] {
            let mut body = body;
            if !agrees {
                body[10] = 0x2f;
            }
            let mut payload = b"srf_array\0\xf8\x02".to_vec();
            payload.extend_from_slice(&[9, 0x24, 4, 0xf6, 0, 11]);
            payload.extend_from_slice(&body[..20]);
            payload.extend_from_slice(
                b"\xe0\x00srf_prim_ptr(cylinder)\0\xe3\xe0\x02local_sys\0\xf9\x04\x03",
            );
            payload.extend_from_slice(&body[21..35]);
            payload.extend_from_slice(b"\xe0\x02radius\0");
            payload.extend_from_slice(&body[35..38]);
            payload.extend_from_slice(b"\xf1\xf7\x0e\xe3");
            payload.extend_from_slice(&[11, 0x24, 4, 0xf6, 0, 0]);
            payload.extend_from_slice(&body);
            let section_name = format!("ND:0:{section}:0");
            let mut curves = b"crv_array\0\xf3\xf8\x02".to_vec();
            curves.extend_from_slice(
                b"crv_id\0\x15 type\0\x08 feat_id\0\x04 crv_pnt_dir\0\xf8\x02\x01\xf6",
            );
            curves.extend_from_slice(b"crv_hdr_geom_ptr[0]\0\x09 crv_hdr_geom_ptr[1]\0\x0b next_crv_hdr_ptr[0]\0\x16 next_crv_hdr_ptr[1]\0\x16 topol_ref_data\0\xe3");
            push_generated_topology_row(&mut curves, 22, [9, 11], [21, 21]);
            let result = decode(build_prt(
                "c",
                &[
                    (section_name.as_str(), payload),
                    ("ND:0:VisibGeom:1", curves),
                ],
            ));
            let namespace = if section == "ActDatums" {
                "actdatums"
            } else {
                "visibgeom"
            };
            assert_valid(&result);
            let native = result
                .ir()
                .native
                .namespace("creo")
                .expect("retained native topology");
            let promoted = native
                .arenas()
                .get("curve_topology_rows")
                .into_iter()
                .flatten()
                .any(|row| {
                    row.fields()
                        .is_ok_and(|fields| fields.get("curve_id") == Some(&serde_json::json!(21)))
                });
            assert_eq!(promoted, agrees || section == "VisibGeom");
            if agrees {
                for id in [9, 11] {
                    let cylinder = result
                        .ir()
                        .model
                        .surfaces
                        .iter()
                        .find_map(|surface| {
                            if surface.id.as_str() != format!("creo:{namespace}:surface#{id}") {
                                return None;
                            }
                            match &surface.geometry {
                                SurfaceGeometry::Solved(SolvedSurfaceGeometry::Cylinder(
                                    cylinder,
                                )) => Some(cylinder),
                                _ => None,
                            }
                        })
                        .expect("admitted named or positional cylinder");
                    assert_eq!(cylinder.origin().get(), [4.0, 4.0, -2.0].into());
                    assert_eq!(*cylinder.frame().axis().as_raw(), [0.0, -1.0, 0.0].into());
                }
            } else {
                assert!(result.report().losses.iter().any(|loss| {
                    loss.code == crate::loss::CreoLossCode::CylinderRotationFrameUnresolved.kind()
                }));
                for id in [9, 11] {
                    assert!(!result.ir().model.surfaces.iter().any(|surface| {
                        surface.id.as_str() == format!("creo:{namespace}:surface#{id}")
                            && matches!(
                                surface.geometry,
                                SurfaceGeometry::Solved(SolvedSurfaceGeometry::Cylinder(_))
                            )
                    }));
                }
            }
        }
    }
    let mut payload = b"srf_array\0\xf8\x01".to_vec();
    payload.extend_from_slice(&[9, 0x24, 4, 0x01, 0, 0]);
    payload.extend_from_slice(&[
        0x11, 0x18, 0x13, 0x2f, 0x10, 0, 0x48, 0, 0, 0x18, 0x19, 0, 0, 0, 0, 0, 0, 0, 0x2f, 0, 0,
        0x2f, 0x10, 0, 0x2f, 0, 0,
    ]);
    payload.extend_from_slice(b"\xe0\x00srf_prim_ptr(cylinder)\0\xe3\xe0\x02local_sys\0\xf9\x04\x03\x0f\x18\xe6\x0f\x18\x10\x18\xe6\xe0\x02radius\0\x2f\x00\x00\xf1\xf7\x0e");
    let result = decode(build_prt("c", &[("ND:0:VisibGeom:0", payload)]));
    assert_valid(&result);
    let cylinder = result
        .ir()
        .model
        .surfaces
        .iter()
        .find_map(|surface| {
            if surface.id.as_str() == "creo:visibgeom:surface#9" {
                if let SurfaceGeometry::Solved(SolvedSurfaceGeometry::Cylinder(cylinder)) =
                    &surface.geometry
                {
                    return Some(cylinder);
                }
            }
            None
        })
        .expect("named first-instance cylinder");
    assert_eq!(cylinder.origin().get(), [0.0, 0.0, 0.0].into());
}
