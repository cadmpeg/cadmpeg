// SPDX-License-Identifier: Apache-2.0
//! Writer unit tests.

use std::collections::BTreeMap;

use cadmpeg_ir::geometry::{CurveGeometry, SurfaceGeometry};
use cadmpeg_ir::math::{Point3, Vector3};
use cadmpeg_ir::topology::Sense;

use crate::sab;
use crate::writer::patch::geometry::{
    patch_framed_geometry, patch_tagged_integer_at, GeometryEdits,
};
use crate::writer::primitives::normalized_face_sense_to_native;

#[test]
fn generated_face_sense_edit_preserves_native_normalization_relation() {
    assert_eq!(
        normalized_face_sense_to_native(Sense::Reversed, Sense::Forward, Sense::Forward,),
        Sense::Reversed
    );
    assert_eq!(
        normalized_face_sense_to_native(Sense::Reversed, Sense::Reversed, Sense::Forward,),
        Sense::Forward
    );
}

#[test]
fn generated_straight_record_patches_by_token_boundaries() {
    let mut bytes = Vec::new();
    bytes.extend_from_slice(b"\x0d\x08straight");
    bytes.push(0x13);
    for value in [1.0f64, 2.0, 3.0] {
        bytes.extend_from_slice(&value.to_le_bytes());
    }
    bytes.push(0x14);
    for value in [1.0f64, 0.0, 0.0] {
        bytes.extend_from_slice(&value.to_le_bytes());
    }
    bytes.push(0x11);
    let records = sab::frame(&bytes, 0, bytes.len(), 8).expect("generated straight record");
    let lines = BTreeMap::from([(
        "f3d:brep:entity#0".to_string(),
        (Point3::new(40.0, 50.0, 60.0), Vector3::new(0.0, 1.0, 0.0)),
    )]);
    patch_framed_geometry(
        &mut bytes,
        &records,
        &GeometryEdits {
            positions: &BTreeMap::new(),
            lines: &lines,
            conics: &BTreeMap::new(),
            degenerate_curves: &BTreeMap::new(),
            planes: &BTreeMap::new(),
            spheres: &BTreeMap::new(),
            tori: &BTreeMap::new(),
            cones: &BTreeMap::new(),
            body_transforms: &BTreeMap::new(),
            entity_colors: &BTreeMap::new(),
            edge_ranges: &BTreeMap::new(),
            face_senses: &BTreeMap::new(),
            coedge_senses: &BTreeMap::new(),
            procedural_surface_edits: &BTreeMap::new(),
            nurbs_surfaces: &BTreeMap::new(),
            nurbs_curves: &BTreeMap::new(),
            pcurves: &BTreeMap::new(),
            procedural_curve_edits: &BTreeMap::new(),
            procedural_surface_fits: &BTreeMap::new(),
            creation_timestamps: &BTreeMap::new(),
            edge_continuities: &BTreeMap::new(),
            vertex_ownerships: &BTreeMap::new(),
            face_sidedness: &BTreeMap::new(),
            tolerant_edges: &BTreeMap::new(),
            tolerant_vertices: &BTreeMap::new(),
        },
        1.0,
    )
    .expect("generated line edit");
    let decoded = sab::frame(&bytes, 0, bytes.len(), 8).expect("patched generated straight record");
    assert!(matches!(
        crate::brep::geometry::decode_curve(&decoded[0]),
        Some(CurveGeometry::Line { origin, direction })
            if origin == Point3::new(40.0, 50.0, 60.0)
                && direction == Vector3::new(0.0, 1.0, 0.0)
    ));
}

#[test]
fn generated_signed_sphere_patches_exact_frame_and_radius() {
    let mut bytes = Vec::new();
    bytes.extend_from_slice(b"\x0d\x06sphere");
    bytes.push(0x13);
    for value in [0.0f64, 0.0, 0.0] {
        bytes.extend_from_slice(&value.to_le_bytes());
    }
    bytes.push(0x06);
    bytes.extend_from_slice(&1.0f64.to_le_bytes());
    for vector in [[1.0f64, 0.0, 0.0], [0.0, 0.0, 1.0]] {
        bytes.push(0x14);
        for value in vector {
            bytes.extend_from_slice(&value.to_le_bytes());
        }
    }
    bytes.push(0x11);
    let records = sab::frame(&bytes, 0, bytes.len(), 8).expect("generated sphere record");
    let spheres = BTreeMap::from([(
        "f3d:brep:entity#0".to_string(),
        (
            Point3::new(10.0, 20.0, 30.0),
            Vector3::new(0.0, 1.0, 0.0),
            Vector3::new(1.0, 0.0, 0.0),
            -25.0,
        ),
    )]);
    patch_framed_geometry(
        &mut bytes,
        &records,
        &GeometryEdits {
            positions: &BTreeMap::new(),
            lines: &BTreeMap::new(),
            conics: &BTreeMap::new(),
            degenerate_curves: &BTreeMap::new(),
            planes: &BTreeMap::new(),
            spheres: &spheres,
            tori: &BTreeMap::new(),
            cones: &BTreeMap::new(),
            body_transforms: &BTreeMap::new(),
            entity_colors: &BTreeMap::new(),
            edge_ranges: &BTreeMap::new(),
            face_senses: &BTreeMap::new(),
            coedge_senses: &BTreeMap::new(),
            procedural_surface_edits: &BTreeMap::new(),
            nurbs_surfaces: &BTreeMap::new(),
            nurbs_curves: &BTreeMap::new(),
            pcurves: &BTreeMap::new(),
            procedural_curve_edits: &BTreeMap::new(),
            procedural_surface_fits: &BTreeMap::new(),
            creation_timestamps: &BTreeMap::new(),
            edge_continuities: &BTreeMap::new(),
            vertex_ownerships: &BTreeMap::new(),
            face_sidedness: &BTreeMap::new(),
            tolerant_edges: &BTreeMap::new(),
            tolerant_vertices: &BTreeMap::new(),
        },
        1.0,
    )
    .expect("generated sphere edit");
    let decoded = sab::frame(&bytes, 0, bytes.len(), 8).expect("patched sphere record");
    assert!(matches!(
        crate::brep::geometry::decode_surface(&decoded[0]),
        Some((SurfaceGeometry::Sphere { center, axis, ref_direction, radius }, false))
            if center == Point3::new(10.0, 20.0, 30.0)
                && axis == Vector3::new(0.0, 1.0, 0.0)
                && ref_direction == Vector3::new(1.0, 0.0, 0.0)
                && radius == -25.0
    ));
}

#[test]
fn generated_torus_preserves_signed_self_intersecting_radii() {
    let mut bytes = Vec::new();
    bytes.extend_from_slice(b"\x0d\x05torus");
    bytes.push(0x13);
    for value in [0.0f64, 0.0, 0.0] {
        bytes.extend_from_slice(&value.to_le_bytes());
    }
    bytes.push(0x14);
    for value in [0.0f64, 0.0, 1.0] {
        bytes.extend_from_slice(&value.to_le_bytes());
    }
    for value in [1.0f64, 0.25] {
        bytes.push(0x06);
        bytes.extend_from_slice(&value.to_le_bytes());
    }
    bytes.push(0x14);
    for value in [1.0f64, 0.0, 0.0] {
        bytes.extend_from_slice(&value.to_le_bytes());
    }
    bytes.push(0x11);
    let records = sab::frame(&bytes, 0, bytes.len(), 8).expect("generated torus record");
    let tori = BTreeMap::from([(
        "f3d:brep:entity#0".to_string(),
        (
            Point3::new(10.0, 20.0, 30.0),
            Vector3::new(0.0, 1.0, 0.0),
            Vector3::new(1.0, 0.0, 0.0),
            20.0,
            -35.0,
        ),
    )]);
    patch_framed_geometry(
        &mut bytes,
        &records,
        &GeometryEdits {
            positions: &BTreeMap::new(),
            lines: &BTreeMap::new(),
            conics: &BTreeMap::new(),
            degenerate_curves: &BTreeMap::new(),
            planes: &BTreeMap::new(),
            spheres: &BTreeMap::new(),
            tori: &tori,
            cones: &BTreeMap::new(),
            body_transforms: &BTreeMap::new(),
            entity_colors: &BTreeMap::new(),
            edge_ranges: &BTreeMap::new(),
            face_senses: &BTreeMap::new(),
            coedge_senses: &BTreeMap::new(),
            procedural_surface_edits: &BTreeMap::new(),
            nurbs_surfaces: &BTreeMap::new(),
            nurbs_curves: &BTreeMap::new(),
            pcurves: &BTreeMap::new(),
            procedural_curve_edits: &BTreeMap::new(),
            procedural_surface_fits: &BTreeMap::new(),
            creation_timestamps: &BTreeMap::new(),
            edge_continuities: &BTreeMap::new(),
            vertex_ownerships: &BTreeMap::new(),
            face_sidedness: &BTreeMap::new(),
            tolerant_edges: &BTreeMap::new(),
            tolerant_vertices: &BTreeMap::new(),
        },
        1.0,
    )
    .expect("generated torus edit");
    let decoded = sab::frame(&bytes, 0, bytes.len(), 8).expect("patched torus record");
    assert!(matches!(
        crate::brep::geometry::decode_surface(&decoded[0]),
        Some((SurfaceGeometry::Torus {
            center,
            axis,
            ref_direction,
            major_radius,
            minor_radius,
        }, false))
            if center == Point3::new(10.0, 20.0, 30.0)
                && axis == Vector3::new(0.0, 1.0, 0.0)
                && ref_direction == Vector3::new(1.0, 0.0, 0.0)
                && major_radius == 20.0
                && minor_radius == -35.0
    ));
}

#[test]
fn generated_cylinder_preserves_native_angle_branch() {
    let mut bytes = Vec::new();
    bytes.extend_from_slice(b"\x0d\x04cone");
    bytes.push(0x13);
    for value in [0.0f64, 0.0, 0.0] {
        bytes.extend_from_slice(&value.to_le_bytes());
    }
    for vector in [[0.0f64, 0.0, 1.0], [1.0, 0.0, 0.0]] {
        bytes.push(0x14);
        for value in vector {
            bytes.extend_from_slice(&value.to_le_bytes());
        }
    }
    for value in [1.0f64, 0.0, -1.0, 1.0] {
        bytes.push(0x06);
        bytes.extend_from_slice(&value.to_le_bytes());
    }
    bytes.push(0x11);
    let records = sab::frame(&bytes, 0, bytes.len(), 8).expect("generated cylinder record");
    let cones = BTreeMap::from([(
        "f3d:brep:entity#0".to_string(),
        (
            Point3::new(10.0, 20.0, 30.0),
            Vector3::new(0.0, 1.0, 0.0),
            Vector3::new(1.0, 0.0, 0.0),
            40.0,
            1.0,
            0.0,
        ),
    )]);
    patch_framed_geometry(
        &mut bytes,
        &records,
        &GeometryEdits {
            positions: &BTreeMap::new(),
            lines: &BTreeMap::new(),
            conics: &BTreeMap::new(),
            degenerate_curves: &BTreeMap::new(),
            planes: &BTreeMap::new(),
            spheres: &BTreeMap::new(),
            tori: &BTreeMap::new(),
            cones: &cones,
            body_transforms: &BTreeMap::new(),
            entity_colors: &BTreeMap::new(),
            edge_ranges: &BTreeMap::new(),
            face_senses: &BTreeMap::new(),
            coedge_senses: &BTreeMap::new(),
            procedural_surface_edits: &BTreeMap::new(),
            nurbs_surfaces: &BTreeMap::new(),
            nurbs_curves: &BTreeMap::new(),
            pcurves: &BTreeMap::new(),
            procedural_curve_edits: &BTreeMap::new(),
            procedural_surface_fits: &BTreeMap::new(),
            creation_timestamps: &BTreeMap::new(),
            edge_continuities: &BTreeMap::new(),
            vertex_ownerships: &BTreeMap::new(),
            face_sidedness: &BTreeMap::new(),
            tolerant_edges: &BTreeMap::new(),
            tolerant_vertices: &BTreeMap::new(),
        },
        1.0,
    )
    .expect("generated cylinder edit");
    let decoded = sab::frame(&bytes, 0, bytes.len(), 8).expect("patched cylinder record");
    // The patch preserves the record's native negative-cosine angle
    // branch, so decode reports the inward-normal flag.
    assert!(matches!(
        crate::brep::geometry::decode_surface(&decoded[0]),
        Some((SurfaceGeometry::Cylinder {
            origin,
            axis,
            ref_direction,
            radius,
        }, true))
            if origin == Point3::new(10.0, 20.0, 30.0)
                && axis == Vector3::new(0.0, 1.0, 0.0)
                && ref_direction == Vector3::new(1.0, 0.0, 0.0)
                && radius == 40.0
    ));
}

#[test]
fn generated_ellipse_preserves_negative_ratio_phase() {
    let mut bytes = Vec::new();
    bytes.extend_from_slice(b"\x0d\x07ellipse");
    bytes.push(0x13);
    for value in [0.0f64, 0.0, 0.0] {
        bytes.extend_from_slice(&value.to_le_bytes());
    }
    for vector in [[0.0f64, 0.0, 1.0], [1.0, 0.0, 0.0]] {
        bytes.push(0x14);
        for value in vector {
            bytes.extend_from_slice(&value.to_le_bytes());
        }
    }
    bytes.push(0x06);
    bytes.extend_from_slice(&(-0.5f64).to_le_bytes());
    bytes.push(0x11);
    let records = sab::frame(&bytes, 0, bytes.len(), 8).expect("generated ellipse record");
    let conics = BTreeMap::from([(
        "f3d:brep:entity#0".to_string(),
        (
            Point3::new(10.0, 20.0, 30.0),
            Vector3::new(0.0, 1.0, 0.0),
            Vector3::new(1.0, 0.0, 0.0),
            40.0,
            10.0,
        ),
    )]);
    patch_framed_geometry(
        &mut bytes,
        &records,
        &GeometryEdits {
            positions: &BTreeMap::new(),
            lines: &BTreeMap::new(),
            conics: &conics,
            degenerate_curves: &BTreeMap::new(),
            planes: &BTreeMap::new(),
            spheres: &BTreeMap::new(),
            tori: &BTreeMap::new(),
            cones: &BTreeMap::new(),
            body_transforms: &BTreeMap::new(),
            entity_colors: &BTreeMap::new(),
            edge_ranges: &BTreeMap::new(),
            face_senses: &BTreeMap::new(),
            coedge_senses: &BTreeMap::new(),
            procedural_surface_edits: &BTreeMap::new(),
            nurbs_surfaces: &BTreeMap::new(),
            nurbs_curves: &BTreeMap::new(),
            pcurves: &BTreeMap::new(),
            procedural_curve_edits: &BTreeMap::new(),
            procedural_surface_fits: &BTreeMap::new(),
            creation_timestamps: &BTreeMap::new(),
            edge_continuities: &BTreeMap::new(),
            vertex_ownerships: &BTreeMap::new(),
            face_sidedness: &BTreeMap::new(),
            tolerant_edges: &BTreeMap::new(),
            tolerant_vertices: &BTreeMap::new(),
        },
        1.0,
    )
    .expect("generated ellipse edit");
    let ratio_offset =
        sab::payload_token_offsets(&bytes, &records[0], 8, 0x06).expect("ellipse tokens")[0];
    assert_eq!(
        f64::from_le_bytes(
            bytes[ratio_offset + 1..ratio_offset + 9]
                .try_into()
                .expect("ratio payload"),
        ),
        -0.25
    );
    let decoded = sab::frame(&bytes, 0, bytes.len(), 8).expect("patched ellipse record");
    assert!(matches!(
        crate::brep::geometry::decode_curve(&decoded[0]),
        Some(CurveGeometry::Ellipse {
            center,
            axis,
            major_direction,
            major_radius,
            minor_radius,
        })
            if center == Point3::new(10.0, 20.0, 30.0)
                && axis == Vector3::new(0.0, 1.0, 0.0)
                && major_direction == Vector3::new(1.0, 0.0, 0.0)
                && major_radius == 40.0
                && minor_radius == 10.0
    ));
}

#[test]
fn generated_binaryfile4_integer_patch_preserves_following_token() {
    let mut bytes = vec![0x15];
    bytes.extend_from_slice(&(-3i32).to_le_bytes());
    bytes.extend_from_slice(&[0x0d, 0x03, b'n', b'e', b'x']);

    patch_tagged_integer_at(&mut bytes, 0, 4, 7).expect("width-4 enum patch");

    assert_eq!(&bytes[1..5], &7i32.to_le_bytes());
    assert_eq!(&bytes[5..], &[0x0d, 0x03, b'n', b'e', b'x']);
}
