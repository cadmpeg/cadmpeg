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
#[test]
fn floating_sample_fields_decode_and_validate_with_exact_bounds() {
    use std::collections::BTreeSet;
    let mut body = vec![0x11, 0x1d, 0x92, 0xab, 0xcd, 0xef, 0x12, 0x34, 0x15, 0x1e];
    body.extend_from_slice(&[0x15, 0x26, 0x37, 0x48, 0x59, 0x6a, 0x7b]);
    body.extend_from_slice(&[0x13, 0x18, 0x11, 0x18, 0x11, 0x18, 0x13, 0x18]);
    let make_payload = |body: &[u8], count: u8| {
        let mut payload = b"crv_array\0\xe0\x01crv_id\0\x09\xe0\x02crv_pnt_arr\0\xf9".to_vec();
        payload.extend_from_slice(&[count, 4]);
        payload.extend_from_slice(body);
        payload.extend_from_slice(b"\xe0\x01crv_hdr_geom_ptr[0]\0\x0a\xe0\x01crv_hdr_geom_ptr[1]\0\x0b\xe0\x01next_crv_hdr_ptr[0]\0\x09\xe0\x01next_crv_hdr_ptr[1]\0\x09topol_ref_data\0");
        payload
    };
    let mut losses = Vec::new();
    let records = crate::decode::with_service_context(|ctx| {
        crate::curve::two_chart_pcurve_samples(
            ctx,
            &make_payload(&body, 3),
            Some(&BTreeSet::from([10, 11])),
            &mut losses,
        )
    })
    .expect("bounded angular samples");
    assert!(losses.iter().any(|loss| loss.code
        == crate::loss::CreoLossCode::PcurveSampleCoordinatesDropped.kind()
        && loss.message.contains("3 paired boundary samples")));
    assert_eq!(records.len(), 1);
    assert_eq!(
        records[0].samples[0][0][1],
        f64::from_bits(0x3c92_abcd_ef12_3400)
    );
    assert_eq!(
        records[0].samples[0][1][1],
        f64::from_bits(0x3d15_2637_4859_6a7b)
    );
    for (body, count) in [(&body[..7], 3), (&body[..16], 3), (&body[..], 4)] {
        let mut losses = Vec::new();
        let records = crate::decode::with_service_context(|ctx| {
            crate::curve::two_chart_pcurve_samples(
                ctx,
                &make_payload(body, count),
                Some(&BTreeSet::from([10, 11])),
                &mut losses,
            )
        })
        .expect("bounded sample refusal");
        assert!(records.is_empty());
        assert!(losses.iter().any(|loss| loss.code
            == crate::loss::CreoLossCode::TwoChartScalarFrameUnresolved.kind()
            && loss.message.contains("Curve 9")));
    }
    for (bytes, complete) in [(&body[..], true), (&body[..7], false), (&body[..16], false)] {
        let result = decode(build_prt(
            "samples",
            &[("ND:0:VisibGeom:0", make_payload(bytes, 3))],
        ));
        assert_valid(&result);
        let expected = if complete {
            crate::loss::CreoLossCode::PcurveSampleCoordinatesDropped
        } else {
            crate::loss::CreoLossCode::TwoChartScalarFrameUnresolved
        };
        assert!(result
            .report()
            .losses
            .iter()
            .any(|loss| loss.code == expected.kind()));
    }
}
#[test]
fn floating_cylinder_instances_share_bounded_frame_admission() {
    let envelope = [
        0x18, 0x18, 0x13, 0x2f, 0x1c, 0, 0x2f, 0x10, 0, 0x18, 0x48, 0x24, 0, 0x2f, 0x28, 0, 0x2f,
        0x1c, 0, 0x48, 0, 0,
    ];
    for section in ["ActDatums", "VisibGeom"] {
        for refusal in 0..7 {
            let mut envelope = envelope;
            if refusal == 1 {
                envelope[7] = 0x08;
            } else if refusal == 2 {
                envelope[17] = 0x18;
            } else if refusal == 4 {
                envelope[2] = 0x18;
            }
            let tiny = if section == "ActDatums" { 0x32 } else { 0x19 };
            let mut matrix = vec![tiny, 0x90, 0, 0, 0, 0, 0, 0, 0x0f, 0x18, 0xe5, 0x10, 0x10];
            matrix.extend_from_slice(&[tiny, 0x90, 0, 0, 0, 0, 0, 0, 0x18]);
            if refusal == 5 {
                matrix = vec![tiny, 0x90, 0, 0, 0, 0, 0, 0, 0x0f];
                matrix.extend_from_slice(&[tiny, 0x90, 0, 0, 0, 0, 0, 0, 0x18, 0x18, 0x10, 0x10]);
                matrix.extend_from_slice(&[tiny, 0x90, 0, 0, 0, 0, 0, 0, 0x18]);
            }
            if refusal == 6 {
                // An off-axis component of the rotation axis refuses the carrier.
                matrix.splice(9..10, [tiny, 0x90, 0, 0, 0, 0, 0, 0x01]);
            }
            matrix.extend_from_slice(&[0x2f, 0x20, 0, 0x2f, 0x1c, 0, 0x48, 0x18, 0]);
            let radius = [0x2f, if refusal == 3 { 0x08 } else { 0x10 }, 0];
            let mut payload = b"srf_array\0\xf8\x02".to_vec();
            payload.extend_from_slice(&[9, 0x24, 4, 0xf6, 0, 11]);
            payload.extend_from_slice(&envelope);
            payload.extend_from_slice(
                b"\xe0\x00srf_prim_ptr(cylinder)\0\xe3\xe0\x02local_sys\0\xf9\x04\x03",
            );
            payload.extend_from_slice(&matrix);
            payload.extend_from_slice(b"\xe0\x02radius\0");
            payload.extend_from_slice(&radius);
            payload.extend_from_slice(b"\xf1\xf7\x0e\xe3");
            payload.extend_from_slice(&[11, 0x24, 4, 0xf6, 0, 0]);
            payload.extend_from_slice(&envelope);
            payload.push(0xe3);
            payload.extend_from_slice(&matrix);
            payload.extend_from_slice(&radius);
            payload.push(0xe3);
            let name = format!("ND:0:{section}:0");
            let result = decode(build_prt("c", &[(name.as_str(), payload)]));
            assert_valid(&result);
            let namespace = if section == "ActDatums" {
                "actdatums"
            } else {
                "visibgeom"
            };
            for id in [9, 11] {
                let cylinder = result.ir().model.surfaces.iter().find_map(|surface| {
                    if surface.id.as_str() == format!("creo:{namespace}:surface#{id}") {
                        if let SurfaceGeometry::Solved(SolvedSurfaceGeometry::Cylinder(cylinder)) =
                            &surface.geometry
                        {
                            return Some(cylinder);
                        }
                    }
                    None
                });
                assert_eq!(cylinder.is_some(), refusal == 0);
                if let Some(cylinder) = cylinder {
                    assert_eq!(cylinder.origin().get(), [8.0, 7.0, -6.0].into());
                    assert_eq!(*cylinder.frame().axis().as_raw(), [0.0, -1.0, 0.0].into());
                    assert_eq!(cylinder.radius().get(), 4.0);
                }
            }
            if refusal != 0 {
                assert!(result.report().losses.iter().any(|loss| {
                    loss.code == crate::loss::CreoLossCode::CylinderRotationFrameUnresolved.kind()
                }));
            }
        }
    }
}

#[test]
fn subunit_rotation_pipeline_uses_the_shared_frame_gate() {
    let mut frame = vec![0x0f, 0x18, 0x19, 0x80, 0, 0, 0, 0, 0, 0, 0x18, 0x10, 0x18];
    frame.extend([0x19, 0x80, 0, 0, 0, 0, 0, 0, 0x18, 0x10]);
    frame.extend([0x18, 0x2f, 0x08, 0, 0x2f, 0x10, 0]);
    let envelope = [
        0x18, 0x2f, 0x10, 0, 0x11, 0x2f, 0x24, 0, 0x48, 0, 0, 0x0f, 0x48, 0x18, 0, 0x2f, 0, 0,
        0x2f, 0x08, 0, 0x19, 0x80, 0, 0, 0, 0, 0, 0,
    ];
    for trailing in [false, true] {
        let mut payload = b"srf_array\0\xf8\x01".to_vec();
        payload.extend([7, 0x24, 4, 0x01, 0, 0]);
        payload.extend(envelope);
        payload.extend(b"\xe0\0srf_prim_ptr(cylinder)\0\xe0\x02local_sys\0\xf9\x04\x03");
        payload.extend(&frame);
        if trailing {
            payload.push(0x18);
        }
        payload.extend(b"\xe0\x02radius\0\x2f\x00\x00\xe1crv_array\0\xf3\xf8\0");
        let mut active = b"srf_array\0\xf8\x01".to_vec();
        active.extend([9, 0x22, 4, 0xf6, 0, 7]);
        active.extend([
            0x18, 0x18, 0x2f, 0, 0, 0x2f, 0, 0, 0x18, 0x2f, 0x08, 0, 0x2f, 0x10, 0, 0x2f, 0, 0,
            0x0f, 0x2f, 0x10, 0,
        ]);
        active.push(0xe3);
        active.extend(&frame);
        if trailing {
            active.push(0x18);
        }
        active.extend([0xe3, 0xe1]);
        let result = decode(build_prt(
            "integration",
            &[("ND:0:VisibGeom:0", payload), ("ND:0:ActDatums:0", active)],
        ));
        assert_valid(&result);
        let native = result.ir().native.namespace("creo").expect("native Creo");
        let parameter = native.arenas()["surface_parameters"]
            .iter()
            .find(|record| record.id() == "creo:visibgeom:surface_parameter#7")
            .expect("named envelope");
        assert_eq!(
            crate::test_support::native_bytes(&parameter.fields().expect("fields")["body"]),
            envelope
        );
        let active_plane = result
            .ir()
            .model
            .surfaces
            .iter()
            .find(|surface| surface.id.as_str() == "creo:actdatums:surface#9");
        if trailing {
            assert!(active_plane.is_none());
            assert!(result.report().losses.iter().any(|loss| loss.code
                == crate::loss::CreoLossCode::ActiveBoundaryPlaneUnresolved.kind()
                && loss.message.contains('9')));
        } else {
            let SurfaceGeometry::Solved(SolvedSurfaceGeometry::Plane(plane)) =
                active_plane.expect("source-owned boundary plane").geometry
            else {
                panic!("plane chart");
            };
            assert_eq!(<[f64; 3]>::from(plane.origin().get()), [0.0, 3.0, 4.0]);
            assert!(plane.frame().axis().as_raw().z < -0.999_999_999);
        }
        let surface = result
            .ir()
            .model
            .surfaces
            .iter()
            .find(|s| s.id.as_str() == "creo:visibgeom:surface#7");
        if trailing {
            assert!(surface.is_none_or(|surface| matches!(
                surface.geometry,
                SurfaceGeometry::Solved(SolvedSurfaceGeometry::Unknown { .. })
            )));
            assert!(result.report().losses.iter().any(|loss| loss.code
                == crate::loss::CreoLossCode::RevolutionCarrierUnresolved.kind()
                && loss.message.contains('7')));
        } else {
            let SurfaceGeometry::Solved(SolvedSurfaceGeometry::Cylinder(cylinder)) =
                surface.expect("named cylinder").geometry
            else {
                panic!("cylinder carrier");
            };
            assert_eq!(cylinder.radius().get(), 2.0);
            assert_eq!(<[f64; 3]>::from(cylinder.origin().get()), [0.0, 3.0, 4.0]);
        }
    }
    for trailing in [false, true] {
        let mut payload = b"srf_array\0\xf8\x01".to_vec();
        payload.extend([7, 0x24, 4, 0x01, 0, 0]);
        payload.extend(envelope);
        payload.push(0xe3);
        payload.extend(&frame);
        if trailing {
            payload.push(0x18);
        }
        payload.extend([0x2f, 0, 0, 0xe3]);
        payload.extend(b"crv_array\0\xf3\xf8\0");
        let result = decode(build_prt("integration", &[("ND:0:VisibGeom:0", payload)]));
        assert_valid(&result);
        let surface = result
            .ir()
            .model
            .surfaces
            .iter()
            .find(|s| s.id.as_str() == "creo:visibgeom:surface#7");
        if trailing {
            assert!(surface.is_none_or(|surface| matches!(
                surface.geometry,
                SurfaceGeometry::Solved(SolvedSurfaceGeometry::Unknown { .. })
            )));
            assert!(result.report().losses.iter().any(|loss| loss.code
                == crate::loss::CreoLossCode::RevolutionCarrierUnresolved.kind()
                && loss.message.contains('7')));
        } else {
            let SurfaceGeometry::Solved(SolvedSurfaceGeometry::Cylinder(cylinder)) =
                surface.expect("inline cylinder").geometry
            else {
                panic!("cylinder carrier");
            };
            assert_eq!(cylinder.radius().get(), 2.0);
            assert_eq!(<[f64; 3]>::from(cylinder.origin().get()), [0.0, 3.0, 4.0]);
        }
    }
    let mut sampled = b"topol_ref_data\0".to_vec();
    sampled.extend([13, 0, 4, 1, 0xf6, 0xfc, 2]);
    sampled.extend([
        0x18, 0xe4, 0x13, 0x18, 0x19, 0x80, 0, 0, 0, 0, 0, 0, 0x2f, 0x14, 0, 0x13, 0x2f, 0x14, 0,
    ]);
    sampled.extend([11, 9, 13, 13, 0, 0, 0xe3, 0xe1, 0xe3]);
    let samples = crate::decode::with_service_context(|ctx| {
        crate::curve::two_chart_pcurve_samples(
            ctx,
            &sampled,
            Some(&std::collections::BTreeSet::from([11, 9])),
            &mut Vec::new(),
        )
    })
    .expect("counted endpoint charts");
    assert_eq!(samples.len(), 1);
    assert_eq!(
        samples[0].samples[1][0],
        [f64::from_bits(0x3c80_0000_0000_0000), 5.0]
    );
}
#[test]
fn angular_wall_chart_pipeline_requires_a_complete_rotation() {
    fn scalar(bytes: &mut Vec<u8>, value: f64) {
        let raw = value.to_be_bytes();
        if value == 0.0 {
            bytes.push(0x18);
        } else if value == 1.0 {
            bytes.push(0x0f);
        } else if value == -0.6 {
            assert_eq!(&raw[..2], &[0xbf, 0xe3]);
            bytes.push(0xb6);
            bytes.extend_from_slice(&raw[2..]);
        } else {
            bytes.push(if raw[0] == 0x3f { 0x41 } else { 0x2d });
            bytes.extend_from_slice(&raw[1..]);
        }
    }
    for complete in [true, false] {
        let mut payload = b"srf_array\0\xf8\x01".to_vec();
        payload.extend_from_slice(&[9, 0x22, 4, 1, 1, 0]);
        for value in [0.0, 0.0, 4.0, 6.0, 7.0, 11.0, 13.0, 11.0, 15.8, 16.6] {
            scalar(&mut payload, value);
        }
        payload.push(0xe3);
        for value in [
            1.0, 0.0, 0.0, 0.0, 0.8, -0.6, 0.0, 0.6, 0.8, 7.0, 11.0, 13.0,
        ] {
            scalar(&mut payload, value);
        }
        if !complete {
            payload.push(0xff);
        }
        payload.extend_from_slice(b"\xe3crv_array\0\xf3\xf8\0");
        let result = decode(build_prt("integration", &[("ND:0:VisibGeom:0", payload)]));
        let plane = result.ir().model.surfaces.iter().find_map(|surface| {
            if surface.id.as_str() != "creo:visibgeom:surface#9" {
                return None;
            }
            match &surface.geometry {
                SurfaceGeometry::Solved(SolvedSurfaceGeometry::Plane(plane)) => Some(plane),
                _ => None,
            }
        });
        if complete {
            let plane = plane.expect("complete wall rotation");
            assert_eq!(plane.origin().get(), [7.0, 11.0, 13.0].into());
            let normal = plane.frame().axis().as_raw();
            assert!(normal.x.abs() < 1.0e-12);
            assert!((normal.y + 0.6).abs() < 1.0e-12);
            assert!((normal.z - 0.8).abs() < 1.0e-12);
        } else {
            assert!(plane.is_none());
            assert!(result.report().losses.iter().any(|loss| {
                loss.code == crate::loss::CreoLossCode::PlaneAngularFrameUnresolved.kind()
                    && loss.message.contains('9')
            }));
        }
        assert_valid(&result);
    }
}

#[test]
fn positive_lower_bend_pipeline_reconciles_named_and_inline_carriers() {
    fn scalar(bytes: &mut Vec<u8>, value: f64) {
        if value == 0.0 {
            bytes.push(0x18);
            return;
        }
        let raw = value.to_be_bytes();
        bytes.push(if raw[0] == 0x3f { 0x41 } else { 0x2d });
        bytes.extend_from_slice(&raw[1..]);
    }
    let lower = 0.7_f64;
    let radius = 4.0;
    let mut envelope = Vec::new();
    scalar(&mut envelope, lower);
    scalar(&mut envelope, 0.0);
    envelope.push(0x12);
    for value in [
        5.0,
        7.0,
        11.0 + radius * lower.sin(),
        13.0,
        7.0 + radius * lower.cos(),
        11.0 + radius,
        18.0,
    ] {
        scalar(&mut envelope, value);
    }
    let mut matrix = vec![0x0f, 0x18, 0xe5, 0x0f, 0x18, 0xe5, 0x0f];
    for value in [7.0, 11.0, 13.0] {
        scalar(&mut matrix, value);
    }
    for section in ["VisibGeom", "ActDatums"] {
        for agrees in [true, false] {
            let encoded_radius = if agrees {
                [0x2f, 0x10, 0]
            } else {
                [0x2f, 0x14, 0]
            };
            let mut payload = b"srf_array\0\xf8\x02".to_vec();
            payload.extend_from_slice(&[9, 0x24, 4, 0xf6, 1, 11]);
            payload.extend_from_slice(&envelope);
            payload.extend_from_slice(
                b"\xe0\0srf_prim_ptr(cylinder)\0\xe3\xe0\x02local_sys\0\xf9\x04\x03",
            );
            payload.extend_from_slice(&matrix);
            payload.extend_from_slice(b"\xe0\x02radius\0");
            payload.extend_from_slice(&encoded_radius);
            payload.extend_from_slice(b"\xf1\xf7\x0e\xe3");
            payload.extend_from_slice(&[11, 0x24, 4, 0xf6, 1, 0]);
            payload.extend_from_slice(&envelope);
            payload.extend_from_slice(b"\xf7\x25\xe3");
            payload.extend_from_slice(&matrix);
            payload.extend_from_slice(&encoded_radius);
            payload.extend_from_slice(b"\xe3crv_array\0\xf3\xf8\0");
            let result = decode(build_prt(
                "integration",
                &[(&format!("ND:0:{section}:0"), payload)],
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
            if agrees {
                assert_eq!(cylinders.len(), 2);
                for cylinder in cylinders {
                    assert_eq!(cylinder.origin().get(), [7.0, 11.0, 13.0].into());
                    assert_eq!(cylinder.radius().get(), radius);
                    assert_eq!(*cylinder.frame().axis().as_raw(), [0.0, 0.0, 1.0].into());
                    assert_eq!(
                        *cylinder.frame().reference().as_raw(),
                        [1.0, 0.0, 0.0].into()
                    );
                    let frame = crate::surface::PositionalCylinderFrame::new(
                        [7.0, 11.0, 13.0],
                        [0.0, 0.0, 1.0],
                        [1.0, 0.0, 0.0],
                        radius,
                        None,
                    )
                    .expect("signed cylinder");
                    assert_eq!(
                        crate::surface::cylinder_frame_readers::rotation_cylinder_parameter_bounds(
                            &envelope, frame,
                        ),
                        Some([[lower, std::f64::consts::FRAC_PI_2], [0.0, 5.0]])
                    );
                }
            } else {
                assert!(cylinders.is_empty());
                assert!(result.report().losses.iter().any(|loss| {
                    loss.code
                        == crate::loss::CreoLossCode::CylinderRotationEnvelopeUnresolved.kind()
                        && loss.message.contains("11")
                }));
            }
            assert_valid(&result);
        }
    }
}
