// SPDX-License-Identifier: Apache-2.0
//! Crate-level tests over synthetic byte fixtures. No real CAD file exists in
//! this repo and none may be added, so every fixture is a hand-built `.prt`
//! byte image whose bytes exercise the real SPLMSSTR container parse, the
//! Parasolid zlib extraction/classification, and the analytic geometry decode,
//! and fail if the code regresses.
//!
//! JT codec primitives live in `jt` / `jt_topology`. OM wire parsers live in
//! `om`. Feature-completeness predicates live in `decode`.
#![allow(clippy::unwrap_used)]
#![allow(
    clippy::default_trait_access,
    reason = "Test fixtures use type-inferred defaults for compact record construction."
)]

use std::io::Cursor;

use cadmpeg_ir::codec::{Codec, CodecEntry, Confidence, DecodeOptions};

use cadmpeg_core::decode::{DecodeMode, InspectOptions};
use cadmpeg_ir::geometry::{
    BlendCrossSection, BlendRadiusLaw, CurveGeometry, PcurveGeometry, ProceduralCurveDefinition,
    ProceduralSurfaceDefinition, SurfaceGeometry,
};
use cadmpeg_ir::math::{Point2, Vector3};
use cadmpeg_ir::report::{LossCategory, LossKind};
use cadmpeg_ir::Exactness;

use crate::container;
use crate::parasolid::{self, StreamKind};
use crate::test_support::*;
use crate::NxCodec;

fn extract_streams(bytes: &[u8]) -> Vec<crate::parasolid::Stream> {
    let arena = cadmpeg_core::decode::DecodeArena::new();
    let policy = cadmpeg_core::decode::DecodePolicy::default();
    let (ctx, root) = cadmpeg_core::decode::DecodeContext::from_root_bytes(bytes, &arena, &policy)
        .expect("bounded test input");
    let container = container::scan_bytes(bytes.to_vec()).expect("test SPLMSSTR container");
    parasolid::extract_streams(&ctx, root, &container).expect("test Parasolid streams")
}

fn options_in(mode: DecodeMode, container_only: bool) -> DecodeOptions {
    DecodeOptions {
        container_only,
        policy: cadmpeg_core::decode::DecodePolicy {
            mode,
            ..Default::default()
        },
    }
}

#[test]
fn ug_part_segment_index_uses_row_one_self_boundary() {
    let file = prt_with_named_payloads(&[("/Root/UG_PART/UG_PART", segment_index_payload())]);
    let container = container::scan_bytes(file).unwrap();
    let (_, index) = container.segment_index().expect("segment index");
    assert_eq!(index.byte_len, 28);
    assert_eq!(index.rows.len(), 2);
    assert_eq!(index.rows[0].type_code, 7);
    assert_eq!(index.rows[0].subtype_code, 9);
    assert_eq!(index.rows[0].value, 11);
    assert_eq!(index.rows[1].type_code, 1);
    assert_eq!(index.rows[1].subtype_code, 1);
    assert_eq!(index.rows[1].value, 28);
    assert_eq!(index.padding, &[0xaa, 0xbb, 0xcc, 0xdd]);
}

#[test]
fn nx_circular_cone_offsets_resolve_across_equivalent_axis_origins() {
    use cadmpeg_ir::geometry::SurfaceGeometry;
    use cadmpeg_ir::math::{Point3, Vector3};

    let angle = std::f64::consts::FRAC_PI_6;
    let support = SurfaceGeometry::Cone {
        origin: Point3::new(0.0, 0.0, 0.0),
        axis: Vector3::new(0.0, 0.0, 1.0),
        ref_direction: Vector3::new(1.0, 0.0, 0.0),
        radius: 4.0,
        ratio: 1.0,
        half_angle: angle,
    };
    let expected = 2.0;
    let axial_shift = -expected * angle.sin();
    let offset = SurfaceGeometry::Cone {
        origin: Point3::new(0.0, 0.0, axial_shift),
        axis: Vector3::new(0.0, 0.0, 1.0),
        ref_direction: Vector3::new(1.0, 0.0, 0.0),
        radius: 4.0 + expected * angle.cos(),
        ratio: 1.0,
        half_angle: angle,
    };

    let distance = crate::decode::analytic_surface_offset(&support, &offset).expect("offset");
    assert!((distance - expected).abs() <= 1e-12);
    let reverse = crate::decode::analytic_surface_offset(&offset, &support).expect("reverse");
    assert!((reverse + expected).abs() <= 1e-12);

    let mut lateral = offset.clone();
    let SurfaceGeometry::Cone { origin, .. } = &mut lateral else {
        unreachable!()
    };
    origin.x = 0.1;
    assert!(crate::decode::analytic_surface_offset(&support, &lateral).is_none());

    let mut shifted_parameterization = offset.clone();
    let SurfaceGeometry::Cone { origin, .. } = &mut shifted_parameterization else {
        unreachable!()
    };
    origin.z += 0.1;
    assert!(crate::decode::analytic_surface_offset(&support, &shifted_parameterization).is_none());

    let mut elliptical = offset;
    let SurfaceGeometry::Cone { ratio, .. } = &mut elliptical else {
        unreachable!()
    };
    *ratio = 0.5;
    assert!(crate::decode::analytic_surface_offset(&support, &elliptical).is_none());
}

#[test]
fn nx_sphere_offset_lineage_follows_signed_radius_orientation() {
    use cadmpeg_ir::geometry::SurfaceGeometry;
    use cadmpeg_ir::math::{Point3, Vector3};

    let sphere = |radius| SurfaceGeometry::Sphere {
        center: Point3::new(1.0, 2.0, 3.0),
        axis: Vector3::new(0.0, 0.0, 1.0),
        ref_direction: Vector3::new(1.0, 0.0, 0.0),
        radius,
    };
    assert_eq!(
        crate::decode::analytic_surface_offset(&sphere(4.0), &sphere(6.5)),
        Some(2.5)
    );
    assert_eq!(
        crate::decode::analytic_surface_offset(&sphere(-4.0), &sphere(-6.5)),
        Some(2.5)
    );
    assert_eq!(
        crate::decode::analytic_surface_offset(&sphere(-6.5), &sphere(-4.0)),
        Some(-2.5)
    );
    assert!(crate::decode::analytic_surface_offset(&sphere(4.0), &sphere(-6.5)).is_none());
}

#[test]
fn nx_torus_offset_lineage_requires_one_ring_orientation() {
    use cadmpeg_ir::geometry::SurfaceGeometry;
    use cadmpeg_ir::math::{Point3, Vector3};

    let torus = |minor_radius| SurfaceGeometry::Torus {
        center: Point3::new(1.0, 2.0, 3.0),
        axis: Vector3::new(0.0, 0.0, 1.0),
        ref_direction: Vector3::new(1.0, 0.0, 0.0),
        major_radius: 10.0,
        minor_radius,
    };
    assert_eq!(
        crate::decode::analytic_surface_offset(&torus(2.0), &torus(3.5)),
        Some(1.5)
    );
    assert_eq!(
        crate::decode::analytic_surface_offset(&torus(-2.0), &torus(-3.5)),
        Some(1.5)
    );
    assert_eq!(
        crate::decode::analytic_surface_offset(&torus(-3.5), &torus(-2.0)),
        Some(-1.5)
    );
    assert!(crate::decode::analytic_surface_offset(&torus(2.0), &torus(-3.5)).is_none());
    assert!(crate::decode::analytic_surface_offset(&torus(2.0), &torus(10.0)).is_none());
}

#[test]
fn decode_reports_unclassified_bounded_offset_store_controls() {
    let file = prt_with_named_payloads(&[(
        "/Root/UG_PART/UG_PART",
        offset_only_indexed_om_section_with_control(&[1, 2, 3, 4]),
    )]);
    let result = NxCodec
        .decode(&mut Cursor::new(file), &DecodeOptions::default())
        .unwrap();
    let attributes = &result.ir.source.as_ref().unwrap().attributes;
    assert_eq!(attributes["offset_store_control_count"], "1");
    assert_eq!(attributes["classified_offset_store_control_count"], "0");
    assert_eq!(attributes["unclassified_offset_store_control_count"], "1");
    assert!(result.report.losses.iter().any(|loss| {
        loss.code.category() == LossCategory::Other
            && loss
                .message
                .contains("1 of 1 bounded offset-store control block(s)")
    }));
}

#[test]
fn parasolid_entity_51_records_retain_layout_selected_references() {
    let mut bytes = vec![0, 0x51];
    bytes.extend_from_slice(&1u32.to_be_bytes());
    bytes.extend_from_slice(&10u16.to_be_bytes());
    bytes.extend_from_slice(&2u32.to_be_bytes());
    bytes.extend_from_slice(&0x21u16.to_be_bytes());
    for reference in 3..=8u16 {
        bytes.extend_from_slice(&reference.to_be_bytes());
    }
    bytes.extend_from_slice(&[0xaa, 0xbb]);

    let records = crate::parasolid::entity_51_records(&bytes);
    assert_eq!(records.len(), 1);
    assert_eq!(records[0].offset, 0);
    assert_eq!(records[0].byte_len, 26);
    assert_eq!(records[0].xmt, 10);
    assert_eq!(records[0].sequence, 2);
    assert_eq!(records[0].definition_xmt, 0x21);
    assert_eq!(records[0].leading_references, [3, 4, 5, 6, 7]);
    assert_eq!(records[0].trailing_references, [8]);
    assert_eq!(
        crate::parasolid::entity_51_record_at(&bytes, 0),
        Some(records[0].clone())
    );
    assert!(crate::parasolid::entity_51_record_at(&bytes[..25], 0).is_none());
}

#[test]
fn parasolid_entity_51_definition_uses_extended_xmt_framing() {
    let mut bytes = vec![0, 0x51];
    bytes.extend_from_slice(&1u32.to_be_bytes());
    bytes.extend_from_slice(&10u16.to_be_bytes());
    bytes.extend_from_slice(&2u32.to_be_bytes());
    bytes.extend_from_slice(&(-7_233i16).to_be_bytes());
    bytes.extend_from_slice(&1u16.to_be_bytes());
    for reference in 3..=8u16 {
        bytes.extend_from_slice(&reference.to_be_bytes());
    }

    let record = crate::parasolid::entity_51_record_at(&bytes, 0).unwrap();
    assert_eq!(record.definition_xmt, 40_000);
    assert_eq!(record.byte_len, 28);
    assert!(crate::parasolid::entity_51_record_at(&bytes[..27], 0).is_none());
}

#[test]
fn parasolid_entity_51_reference_count_is_five_plus_flags() {
    for flags in 1..=0x20u32 {
        let mut direct = vec![0, 0x51];
        direct.extend_from_slice(&flags.to_be_bytes());
        direct.extend_from_slice(&10u16.to_be_bytes());
        direct.extend_from_slice(&2u32.to_be_bytes());
        direct.extend_from_slice(&0x21u16.to_be_bytes());
        for reference in 0..flags + 5 {
            direct.extend_from_slice(&(reference as u16 + 3).to_be_bytes());
        }
        direct.extend_from_slice(&[0xaa, 0xbb]);

        let record = crate::parasolid::entity_51_record_at(&direct, 0).unwrap();
        assert_eq!(record.leading_references.len(), 5);
        assert_eq!(record.trailing_references.len(), flags as usize);
        assert_eq!(record.byte_len, direct.len() - 2);
        assert!(crate::parasolid::entity_51_record_at(&direct[..direct.len() - 3], 0).is_none());

        let mut prefixed = vec![0, 0x51];
        prefixed.extend_from_slice(&flags.to_be_bytes());
        prefixed.extend_from_slice(&10u16.to_be_bytes());
        prefixed.extend_from_slice(&2u32.to_be_bytes());
        prefixed.extend_from_slice(&0x21u16.to_be_bytes());
        for reference in 0..flags + 5 {
            prefixed.push(u8::from(reference % 2 == 0));
            prefixed.extend_from_slice(&(reference as u16 + 3).to_be_bytes());
        }
        prefixed.push(0);
        prefixed.extend_from_slice(&[0xaa, 0xbb]);

        let record = crate::parasolid::entity_51_record_at(&prefixed, 0).unwrap();
        assert_eq!(record.leading_references.len(), 5);
        assert_eq!(record.trailing_references.len(), flags as usize);
        assert_eq!(record.byte_len, prefixed.len() - 2);
        assert!(
            crate::parasolid::entity_51_record_at(&prefixed[..prefixed.len() - 3], 0).is_none()
        );
    }
}

#[test]
fn parasolid_entity_51_rejects_nonzero_upper_flag_bytes() {
    let mut bytes = vec![0, 0x51];
    bytes.extend_from_slice(&0x0100_0001u32.to_be_bytes());
    bytes.extend_from_slice(&10u16.to_be_bytes());
    bytes.extend_from_slice(&2u32.to_be_bytes());
    bytes.extend_from_slice(&0x21u16.to_be_bytes());
    for reference in 3..=8u16 {
        bytes.extend_from_slice(&reference.to_be_bytes());
    }

    assert!(crate::parasolid::entity_51_record_at(&bytes, 0).is_none());
}

#[test]
fn parasolid_entity_54_strings_require_exact_length_and_terminator() {
    let mut bytes = vec![0xaa, 0x00, 0x54];
    bytes.extend_from_slice(&8u32.to_be_bytes());
    bytes.extend_from_slice(&17u16.to_be_bytes());
    bytes.extend_from_slice(b"deadbeef\0");
    bytes.extend_from_slice(&[0xbb, 0x00, 0x54, 0, 0, 0, 3, 0, 18, b'a', b'b', b'c', 1]);

    let records = crate::parasolid::entity_54_string_records(&bytes);
    assert_eq!(records.len(), 1);
    assert_eq!(records[0].offset, 1);
    assert_eq!(records[0].byte_len, 17);
    assert_eq!(records[0].xmt, 17);
    assert_eq!(records[0].value, "deadbeef");
    assert_eq!(
        crate::parasolid::entity_54_string_record_at(&bytes, 1),
        Some(records[0].clone())
    );
    assert!(crate::parasolid::entity_54_string_record_at(&bytes, bytes.len() - 12).is_none());

    let minimum = [0, 0x54, 0, 0, 0, 1, 0, 2, b'a', 0];
    assert_eq!(
        crate::parasolid::entity_54_string_records(&minimum)[0].value,
        "a"
    );
}

#[test]
fn parasolid_entity_52_integers_require_complete_counted_values() {
    let mut bytes = vec![0xaa, 0x00, 0x52];
    bytes.extend_from_slice(&2u32.to_be_bytes());
    bytes.extend_from_slice(&17u16.to_be_bytes());
    bytes.extend_from_slice(&3u32.to_be_bytes());
    bytes.extend_from_slice(&u32::MAX.to_be_bytes());

    let records = crate::parasolid::entity_52_integer_records(&bytes);
    assert_eq!(records.len(), 1);
    assert_eq!(records[0].offset, 1);
    assert_eq!(records[0].xmt, 17);
    assert_eq!(records[0].values, [3, u32::MAX]);
    assert_eq!(records[0].byte_len, 16);
    assert_eq!(
        crate::parasolid::entity_52_integer_record_at(&bytes, 1),
        Some(records[0].clone())
    );
    assert!(crate::parasolid::entity_52_integer_records(&bytes[..bytes.len() - 1]).is_empty());
    assert!(crate::parasolid::entity_52_integer_record_at(&bytes[..bytes.len() - 1], 1).is_none());
}

#[test]
fn parasolid_field_names_require_a_complete_nonempty_reference_lane() {
    let bytes = [
        0xaa, 0x00, 0x63, 0x00, 0x00, 0x00, 0x03, 0x00, 0x19, 0x00, 0x1c, 0x00, 0x1d, 0x00, 0x1e,
        0xbb,
    ];
    let records = crate::parasolid::field_names_records(&bytes);
    assert_eq!(records.len(), 1);
    assert_eq!(records[0].offset, 1);
    assert_eq!(records[0].byte_len, 14);
    assert_eq!(records[0].xmt, 25);
    assert_eq!(records[0].name_xmts, [28, 29, 30]);
    assert!(crate::parasolid::field_names_record_at(&bytes[..14], 1).is_none());

    let empty = [0x00, 0x63, 0, 0, 0, 0, 0, 25];
    assert!(crate::parasolid::field_names_records(&empty).is_empty());
}

#[test]
fn parasolid_entity_53_doubles_require_complete_finite_values() {
    let mut bytes = vec![0xaa, 0x00, 0x53, 0xff];
    bytes.extend_from_slice(&2u32.to_be_bytes());
    bytes.extend_from_slice(&18u16.to_be_bytes());
    bytes.extend_from_slice(&0.001f64.to_be_bytes());
    bytes.extend_from_slice(&0.25f64.to_be_bytes());

    let records = crate::parasolid::entity_53_double_records(&bytes);
    assert_eq!(records.len(), 1);
    assert_eq!(records[0].offset, 1);
    assert_eq!(records[0].xmt, 18);
    assert_eq!(records[0].values, [0.001, 0.25]);
    assert_eq!(records[0].byte_len, 25);
    assert_eq!(
        crate::parasolid::entity_53_double_record_at(&bytes, 1),
        Some(records[0].clone())
    );

    let last = bytes.len() - 8;
    bytes[last..].copy_from_slice(&f64::NAN.to_be_bytes());
    assert!(crate::parasolid::entity_53_double_records(&bytes).is_empty());
    assert!(crate::parasolid::entity_53_double_record_at(&bytes, 1).is_none());
}

#[test]
fn parasolid_transformable_attribute_values_preserve_vector_and_axis_grouping() {
    let vector_record = |tag: u8, xmt: u16, vectors: &[[f64; 3]]| {
        let mut bytes = vec![0x00, tag];
        bytes.extend_from_slice(
            &u32::try_from(vectors.len())
                .expect("test vector count fits u32")
                .to_be_bytes(),
        );
        bytes.extend_from_slice(&xmt.to_be_bytes());
        for vector in vectors {
            for component in vector {
                bytes.extend_from_slice(&component.to_be_bytes());
            }
        }
        bytes
    };
    let vectors = [[1.0, 2.0, 3.0], [-4.0, 5.0, 6.0]];

    let points = crate::parasolid::entity_55_point_records(&vector_record(0x55, 20, &vectors));
    assert_eq!(points.len(), 1);
    assert_eq!(points[0].values, vectors);
    let vector_values =
        crate::parasolid::entity_56_vector_records(&vector_record(0x56, 21, &vectors));
    assert_eq!(vector_values.len(), 1);
    assert_eq!(vector_values[0].values, vectors);
    let directions =
        crate::parasolid::entity_59_direction_records(&vector_record(0x59, 22, &vectors));
    assert_eq!(directions.len(), 1);
    assert_eq!(directions[0].values, vectors);

    let four_vectors = [vectors[0], vectors[1], [7.0, 8.0, 9.0], [0.0, 1.0, 0.0]];
    let axes = crate::parasolid::entity_57_axis_records(&vector_record(0x57, 23, &four_vectors));
    assert_eq!(axes.len(), 1);
    assert_eq!(
        axes[0].values,
        [
            [four_vectors[0], four_vectors[1]],
            [four_vectors[2], four_vectors[3]],
        ]
    );
    assert!(crate::parasolid::entity_57_axis_records(
        &vector_record(0x57, 23, &four_vectors[..3],)
    )
    .is_empty());

    let mut nonfinite = vectors;
    nonfinite[1][2] = f64::INFINITY;
    assert!(
        crate::parasolid::entity_55_point_records(&vector_record(0x55, 20, &nonfinite)).is_empty()
    );
}

#[test]
fn parasolid_tag_and_unicode_attribute_values_require_complete_counted_lanes() {
    let tags = [
        0x00, 0x58, 0, 0, 0, 2, 0, 24, 0, 0, 0, 7, 0xff, 0xff, 0xff, 0xff,
    ];
    let records = crate::parasolid::entity_58_tag_records(&tags);
    assert_eq!(records.len(), 1);
    assert_eq!(records[0].xmt, 24);
    assert_eq!(records[0].values, [7, u32::MAX]);
    assert!(crate::parasolid::entity_58_tag_records(&tags[..tags.len() - 1]).is_empty());

    let code_units = [b'N' as u16, b'X' as u16, 0xd83d, 0xde80];
    let mut unicode = vec![0x00, 0x62, 0xff];
    unicode.extend_from_slice(&4u32.to_be_bytes());
    unicode.extend_from_slice(&[0xff, 0xff, 0x00, 0x01]);
    for code_unit in code_units {
        unicode.extend_from_slice(&code_unit.to_be_bytes());
    }
    let records = crate::parasolid::entity_62_unicode_records(&unicode);
    assert_eq!(records.len(), 1);
    assert_eq!(records[0].xmt, 32_768);
    assert_eq!(records[0].code_units, code_units);
    assert_eq!(records[0].value, "NX🚀");
    assert!(crate::parasolid::entity_62_unicode_records(&unicode[..unicode.len() - 1]).is_empty());

    let mut invalid = unicode;
    invalid[15..17].copy_from_slice(&0xd800u16.to_be_bytes());
    invalid[17..19].copy_from_slice(&0x0041u16.to_be_bytes());
    assert!(crate::parasolid::entity_62_unicode_records(&invalid).is_empty());
}

#[test]
fn topology_rejects_shell_with_broken_face_ownership_chain() {
    let valid = topology_partition_stream();
    let graph = crate::topology::Graph::parse(&valid);
    assert_eq!(graph.body_shape_shells().len(), 1);

    let mut broken = valid;
    let face = broken
        .windows(2)
        .position(|window| window == [0, 14])
        .expect("face record");
    put_ref(&mut broken, face + 24, 99);
    assert!(crate::topology::Graph::parse(&broken)
        .body_shape_shells()
        .is_empty());

    let mut independent_previous = topology_partition_stream();
    let face = independent_previous
        .windows(2)
        .position(|window| window == [0, 14])
        .expect("face record");
    put_ref(&mut independent_previous, face + 20, 99);
    assert_eq!(
        crate::topology::Graph::parse(&independent_previous)
            .body_shape_shells()
            .len(),
        1
    );
}

#[test]
fn topology_retains_shell_body_identity_without_body_record() {
    let mut stream = topology_partition_stream();
    let body = stream
        .windows(4)
        .position(|window| window == [0, 12, 0, 2])
        .expect("body record");
    stream[body..body + 24].fill(0xff);

    let graph = crate::topology::Graph::parse(&stream);
    assert!(graph.get(12, 2).is_none());
    assert_eq!(graph.body_shape_shells().len(), 1);

    let mut input = Cursor::new(prt_with_partition(&stream));
    let result = NxCodec
        .decode(&mut input, &DecodeOptions::default())
        .unwrap();
    assert_eq!(result.ir.model.bodies.len(), 1);
    assert_eq!(result.ir.model.bodies[0].id.0, "nx:s0:body#2");
    assert_eq!(result.ir.model.faces.len(), 1);
    let validation = cadmpeg_ir::validate::validate(&result.ir, Vec::new());
    assert!(validation.is_ok(), "findings: {:?}", validation.findings);
}

#[test]
fn topology_accepts_cached_last_face_and_implicit_region_identity() {
    let mut stream = topology_partition_stream();
    let shell = stream
        .windows(4)
        .position(|window| window == [0, 13, 0, 3])
        .expect("shell record");
    put_ref(&mut stream, shell + 22, 4);
    let region = stream
        .windows(4)
        .position(|window| window == [0, 19, 0, 12])
        .expect("region record");
    stream[region..region + 16].fill(0xff);
    let mut second_face = record(14, 39);
    put_ref(&mut second_face, 2, 20);
    put_f64(&mut second_face, 10, 0.000_2);
    put_ref(&mut second_face, 18, 1);
    put_ref(&mut second_face, 20, 1);
    put_ref(&mut second_face, 22, 1);
    put_ref(&mut second_face, 24, 3);
    put_ref(&mut second_face, 26, 6);
    second_face[28] = b'+';
    stream.extend(second_face);

    let graph = crate::topology::Graph::parse(&stream);
    assert!(graph.get(19, 12).is_none());
    assert_eq!(graph.body_shape_shells().len(), 1);
    assert_eq!(graph.body_shape_face_count(), 2);

    let mut input = Cursor::new(prt_with_partition(&stream));
    let result = NxCodec
        .decode(&mut input, &DecodeOptions::default())
        .unwrap();
    assert_eq!(result.ir.model.regions.len(), 1);
    assert_eq!(result.ir.model.regions[0].id.0, "nx:s0:region#12");
    assert_eq!(result.ir.model.faces.len(), 2);
    let validation = cadmpeg_ir::validate::validate(&result.ir, Vec::new());
    assert!(validation.is_ok(), "findings: {:?}", validation.findings);
}

#[test]
fn topology_rejects_nonreciprocal_fin_ring() {
    let mut stream = topology_partition_stream();
    let fin = stream
        .windows(4)
        .position(|window| window == [0, 17, 0, 7])
        .expect("fin record");
    put_ref(&mut stream, fin + 8, 99);
    let graph = crate::topology::Graph::parse(&stream);
    assert!(graph.face_loop_rings(4).is_none());

    let mut input = Cursor::new(prt_with_partition(&stream));
    let result = NxCodec
        .decode(&mut input, &DecodeOptions::default())
        .unwrap();
    assert!(result.ir.model.loops.is_empty());
    assert!(result.ir.model.coedges.is_empty());
    assert!(result.ir.model.edges.is_empty());

    let mut broken_partner = topology_partition_stream();
    let fin = broken_partner
        .windows(4)
        .position(|window| window == [0, 17, 0, 7])
        .expect("fin record");
    put_ref(&mut broken_partner, fin + 14, 99);
    assert!(crate::topology::Graph::parse(&broken_partner)
        .face_loop_rings(4)
        .is_none());
}

#[test]
fn topology_accepts_fixed_record_envelope_escape() {
    let mut stream = topology_partition_stream();
    let fin = stream
        .windows(4)
        .position(|window| window == [0, 17, 0, 7])
        .expect("fin record");
    stream.insert(fin + 2, 0xff);
    let graph = crate::topology::Graph::parse(&stream);
    assert_eq!(
        graph.get(17, 7).unwrap().attribute_field_offset(),
        Some(fin + 5)
    );
    assert_eq!(graph.face_loop_rings(4).unwrap().len(), 1);
}

#[test]
fn topology_prefers_escaped_body_shape_over_direct_extended_xmt() {
    let mut stream = topology_partition_stream();
    let shell = stream
        .windows(4)
        .position(|window| window == [0, 13, 0, 3])
        .expect("shell record");
    stream.insert(shell + 2, 0xff);

    let graph = crate::topology::Graph::parse(&stream);
    assert_eq!(graph.get(13, 3).map(|node| node.pos), Some(shell));
    assert_eq!(graph.body_shape_shells().len(), 1);
    assert_eq!(graph.body_shape_face_count(), 1);
}

#[test]
fn topology_iterates_each_record_family_in_physical_order() {
    let mut stream = Vec::new();
    for (xmt, x) in [(77, 0.01), (3, 0.02)] {
        let mut point = record(29, 40);
        put_ref(&mut point, 2, xmt);
        put_vec3(&mut point, 16, [x, 0.0, 0.0]);
        stream.extend(point);
    }

    let graph = crate::topology::Graph::parse(&stream);
    assert_eq!(
        graph.of_kind(29).map(|node| node.xmt).collect::<Vec<_>>(),
        vec![77, 3]
    );
}

#[test]
fn decode_synthesizes_vertex_for_closed_null_vertex_fin() {
    let mut stream = topology_partition_stream();
    let fin = stream
        .windows(4)
        .position(|window| window == [0, 17, 0, 7])
        .expect("fin record");
    put_ref(&mut stream, fin + 12, 1);
    let mut input = Cursor::new(prt_with_partition(&stream));
    let result = NxCodec
        .decode(&mut input, &DecodeOptions::default())
        .unwrap();

    let edge = result.ir.model.edges.first().expect("closed edge");
    assert_eq!(edge.start, edge.end);
    assert!(edge.start.0.contains("closed-edge"));
    assert_eq!(result.ir.model.loops.len(), 1);
    assert_eq!(result.ir.model.coedges.len(), 1);
    assert!(cadmpeg_ir::validate::validate(&result.ir, Vec::new()).is_ok());
}

#[test]
fn decode_aliases_partner_closed_null_vertex_fin_to_edge_start() {
    let mut stream = topology_partition_stream();
    let fin = stream
        .windows(4)
        .position(|window| window == [0, 17, 0, 7])
        .expect("fin record");
    put_ref(&mut stream, fin + 12, 1);
    put_ref(&mut stream, fin + 14, 20);

    let mut partner = record(17, 23);
    put_ref(&mut partner, 2, 20);
    put_ref(&mut partner, 6, 1); // radial partner is not a loop member
    put_ref(&mut partner, 8, 20); // self-forward closed endpoint
    put_ref(&mut partner, 10, 20); // self-backward closed endpoint
    put_ref(&mut partner, 12, 1); // null vertex
    put_ref(&mut partner, 14, 7); // partner fin
    put_ref(&mut partner, 16, 8); // same edge
    put_ref(&mut partner, 18, 9); // same curve
    partner[22] = b'+';
    stream.extend(partner);

    let mut input = Cursor::new(prt_with_partition(&stream));
    let result = NxCodec
        .decode(&mut input, &DecodeOptions::default())
        .unwrap();

    let edge = result.ir.model.edges.first().expect("closed edge");
    assert_eq!(edge.start, edge.end);
    assert!(edge.start.0.contains("closed-edge"));
    assert_eq!(result.ir.model.loops.len(), 1);
    assert_eq!(result.ir.model.coedges.len(), 1);
    assert!(cadmpeg_ir::validate::validate(&result.ir, Vec::new()).is_ok());
}

#[test]
fn decode_does_not_alias_unresolved_edge_end_to_start_vertex() {
    let mut stream = topology_partition_stream();
    let first_fin = stream
        .windows(4)
        .position(|window| window == [0, 17, 0, 7])
        .expect("first fin record");
    put_ref(&mut stream, first_fin + 8, 20);
    put_ref(&mut stream, first_fin + 10, 20);
    put_ref(&mut stream, first_fin + 14, 20);

    let mut second_fin = record(17, 23);
    put_ref(&mut second_fin, 2, 20);
    put_ref(&mut second_fin, 6, 5);
    put_ref(&mut second_fin, 8, 7);
    put_ref(&mut second_fin, 10, 7);
    put_ref(&mut second_fin, 12, 21);
    put_ref(&mut second_fin, 14, 7);
    put_ref(&mut second_fin, 16, 8);
    put_ref(&mut second_fin, 18, 9);
    second_fin[22] = b'+';
    stream.extend(second_fin);

    let mut unresolved_vertex = record(18, 28);
    put_ref(&mut unresolved_vertex, 2, 21);
    put_ref(&mut unresolved_vertex, 16, 99);
    put_f64(&mut unresolved_vertex, 18, 0.000_1);
    stream.extend(unresolved_vertex);

    let mut input = Cursor::new(prt_with_partition(&stream));
    let result = NxCodec
        .decode(&mut input, &DecodeOptions::default())
        .unwrap();

    assert!(result.ir.model.edges.is_empty());
    assert!(result.ir.model.coedges.is_empty());
    assert!(result.ir.model.loops.is_empty());
}

#[test]
fn topology_invalid_candidate_cannot_shadow_later_valid_record() {
    let mut stream = record(14, 39);
    put_ref(&mut stream, 2, 4);
    stream.extend(topology_partition_stream());

    let graph = crate::topology::Graph::parse(&stream);
    let face = graph.get(14, 4).expect("valid later FACE");
    assert!(face.pos >= 39);
    assert!(face.face_fields().is_some());
}

#[test]
fn decode_retains_topology_owned_point_at_origin() {
    let mut stream = topology_partition_stream();
    let point = stream
        .windows(4)
        .position(|window| window == [0, 29, 0, 11])
        .expect("point record");
    put_vec3(&mut stream, point + 16, [0.0, 0.0, 0.0]);

    assert_eq!(crate::geometry::points(&stream).len(), 1);
    let graph = crate::topology::Graph::parse(&stream);
    assert_eq!(
        graph
            .get(29, 11)
            .and_then(crate::topology::Node::point_position),
        Some(cadmpeg_ir::math::Point3::new(0.0, 0.0, 0.0))
    );
    let mut input = Cursor::new(prt_with_partition(&stream));
    let result = NxCodec
        .decode(&mut input, &DecodeOptions::default())
        .unwrap();
    assert_eq!(result.ir.model.vertices.len(), 1);
    assert_eq!(result.ir.model.bodies[0].transform, None);
    assert_eq!(result.ir.model.edges.len(), 1);
    assert_eq!(
        result.ir.model.points[0].position,
        cadmpeg_ir::math::Point3::new(0.0, 0.0, 0.0)
    );
}

#[test]
fn decode_orders_graph_only_origin_before_later_nonzero_point() {
    let mut stream = topology_partition_stream();
    let first = stream
        .windows(4)
        .position(|window| window == [0, 29, 0, 11])
        .expect("point record");
    put_vec3(&mut stream, first + 16, [0.0, 0.0, 0.0]);
    let mut second = record(29, 40);
    put_ref(&mut second, 2, 77);
    put_vec3(&mut second, 16, [0.04, 0.05, 0.06]);
    stream.extend(second);

    let graph = crate::topology::Graph::parse(&stream);
    let points = crate::decode::ordered_point_candidates(&stream, &graph);
    assert_eq!(points.len(), 2);
    assert_eq!(points[0].0, first);
    assert_eq!(points[0].1, cadmpeg_ir::math::Point3::new(0.0, 0.0, 0.0));
    assert_eq!(points[0].2.map(|node| node.xmt), Some(11));
    assert_eq!(points[1].0, stream.len() - 40);
    assert_eq!(points[1].1, cadmpeg_ir::math::Point3::new(40.0, 50.0, 60.0));
    assert_eq!(points[1].2.map(|node| node.xmt), Some(77));
}

#[test]
fn decode_orders_graph_only_escaped_analytics_before_later_records() {
    let mut stream = topology_with_escaped_geometry_envelopes();
    let first_surface = stream
        .windows(3)
        .position(|window| window == [0, 50, 0xff])
        .expect("escaped plane record");
    let first_curve = stream
        .windows(3)
        .position(|window| window == [0, 30, 0xff])
        .expect("escaped line record");

    let second_surface_offset = stream.len();
    let mut plane = record(50, 91);
    put_ref(&mut plane, 2, 77);
    plane[18] = b'+';
    put_vec3(&mut plane, 19, [0.01, 0.02, 0.03]);
    put_vec3(&mut plane, 43, [0.0, 0.0, 1.0]);
    put_vec3(&mut plane, 67, [1.0, 0.0, 0.0]);
    stream.extend(plane);

    let second_curve_offset = stream.len();
    let mut line = record(30, 67);
    put_ref(&mut line, 2, 78);
    line[18] = b'+';
    put_vec3(&mut line, 19, [0.04, 0.05, 0.06]);
    put_vec3(&mut line, 43, [0.0, 1.0, 0.0]);
    stream.extend(line);

    let graph = crate::topology::Graph::parse(&stream);
    let surfaces = crate::decode::ordered_surface_candidates(&stream, &graph);
    assert_eq!(surfaces.len(), 2);
    assert_eq!(surfaces[0].0, first_surface);
    assert_eq!(surfaces[0].2.map(|node| node.xmt), Some(6));
    assert_eq!(surfaces[1].0, second_surface_offset);
    assert_eq!(surfaces[1].2.map(|node| node.xmt), Some(77));

    let curves = crate::decode::ordered_curve_candidates(&stream, &graph);
    assert_eq!(curves.len(), 2);
    assert_eq!(curves[0].0, first_curve);
    assert_eq!(curves[0].2.map(|node| node.xmt), Some(9));
    assert_eq!(curves[1].0, second_curve_offset);
    assert_eq!(curves[1].2.map(|node| node.xmt), Some(78));
}

#[test]
fn decode_does_not_attach_unreferenced_point_to_solid_topology() {
    let mut stream = topology_partition_stream();
    let mut point = record(29, 40);
    put_ref(&mut point, 2, 77);
    put_vec3(&mut point, 16, [0.04, 0.05, 0.06]);
    stream.extend_from_slice(&point);

    let mut input = Cursor::new(prt_with_partition(&stream));
    let result = NxCodec
        .decode(&mut input, &DecodeOptions::default())
        .unwrap();

    assert_eq!(result.ir.model.points.len(), 1);
    assert_eq!(result.ir.model.vertices.len(), 1);
    assert_eq!(result.ir.model.shells[0].free_vertices.len(), 0);
    assert_eq!(result.ir.model.bodies.len(), 1);
    assert!(cadmpeg_ir::validate::validate(&result.ir, Vec::new()).is_ok());
}

#[test]
fn decode_retains_connected_topology_with_unknown_surface_carrier() {
    let mut stream = topology_partition_stream();
    let face = stream
        .windows(2)
        .position(|window| window == [0, 14])
        .expect("face record");
    put_ref(&mut stream, face + 26, 99);
    let mut input = Cursor::new(prt_with_partition(&stream));
    let result = NxCodec
        .decode(&mut input, &DecodeOptions::default())
        .unwrap();

    assert_eq!(result.ir.model.faces.len(), 1);
    let surface = result
        .ir
        .model
        .surfaces
        .iter()
        .find(|surface| surface.id == result.ir.model.faces[0].surface)
        .expect("unknown face carrier");
    assert!(matches!(surface.geometry, SurfaceGeometry::Unknown { .. }));
    let validation = cadmpeg_ir::validate::validate(&result.ir, Vec::new());
    assert!(validation.is_ok(), "findings: {:?}", validation.findings);
}

#[test]
fn decode_retains_unknown_non_null_edge_curve_carrier() {
    let mut stream = topology_partition_stream();
    let edge = stream
        .windows(2)
        .position(|window| window == [0, 16])
        .expect("edge record");
    put_ref(&mut stream, edge + 24, 99);
    let mut input = Cursor::new(prt_with_partition(&stream));
    let result = NxCodec
        .decode(&mut input, &DecodeOptions::default())
        .unwrap();

    let curve = result.ir.model.edges[0]
        .curve
        .as_ref()
        .and_then(|id| result.ir.model.curves.iter().find(|curve| &curve.id == id))
        .expect("unknown edge carrier");
    assert!(matches!(curve.geometry, CurveGeometry::Unknown { .. }));
    assert!(cadmpeg_ir::validate::validate(&result.ir, Vec::new()).is_ok());
}

#[test]
fn decode_drops_unknown_carrier_outside_emitted_topology() {
    let mut stream = topology_partition_stream();
    let mut orphan = record(16, 32);
    put_ref(&mut orphan, 2, 88);
    put_f64(&mut orphan, 10, 0.000_3);
    put_ref(&mut orphan, 18, 1);
    put_ref(&mut orphan, 24, 99);
    stream.extend(orphan);

    let mut input = Cursor::new(prt_with_partition(&stream));
    let result = NxCodec
        .decode(&mut input, &DecodeOptions::default())
        .unwrap();

    assert!(result
        .ir
        .model
        .curves
        .iter()
        .all(|curve| !matches!(curve.geometry, CurveGeometry::Unknown { .. })));
    assert_eq!(result.ir.model.edges.len(), 1);
}

#[test]
fn decode_retains_native_carrierless_edge() {
    let mut stream = topology_partition_stream();
    let edge = stream
        .windows(2)
        .position(|window| window == [0, 16])
        .expect("edge record");
    put_ref(&mut stream, edge + 24, 1);
    let fin = stream
        .windows(2)
        .position(|window| window == [0, 17])
        .expect("fin record");
    put_ref(&mut stream, fin + 18, 1);
    let mut input = Cursor::new(prt_with_partition(&stream));
    let result = NxCodec
        .decode(&mut input, &DecodeOptions::default())
        .unwrap();

    let edge = &result.ir.model.edges[0];
    assert_eq!(edge.curve, None);
    assert_eq!(edge.param_range, None);
    assert!(cadmpeg_ir::validate::validate(&result.ir, Vec::new()).is_ok());
}

#[test]
fn tolerant_edge_becomes_a_two_support_procedural_intersection() {
    let mut ir = cadmpeg_ir::examples::unit_cube();
    let edge_id = ir.model.edges[0].id.clone();
    let expected_endpoints = [&ir.model.edges[0].start, &ir.model.edges[0].end].map(|vertex_id| {
        let point_id = &ir
            .model
            .vertices
            .iter()
            .find(|vertex| &vertex.id == vertex_id)
            .expect("edge vertex")
            .point;
        ir.model
            .points
            .iter()
            .find(|point| &point.id == point_id)
            .expect("vertex point")
            .position
    });
    ir.model.edges[0].curve = None;
    ir.model.edges[0].param_range = None;
    ir.model.edges[0].tolerance = Some(0.01);
    let mut edges = std::collections::BTreeMap::new();
    edges.insert(8, edge_id.clone());
    let mut incident_coedges = ir
        .model
        .coedges
        .iter_mut()
        .filter(|coedge| coedge.edge == edge_id)
        .collect::<Vec<_>>();
    assert_eq!(incident_coedges.len(), 2);
    incident_coedges[0].id = cadmpeg_ir::ids::CoedgeId("nx:test:fin#7".into());
    incident_coedges[1].id = cadmpeg_ir::ids::CoedgeId("nx:test:fin#22".into());
    let mut stream = partnered_trimmed_topology_partition_stream();
    let edge = stream
        .windows(2)
        .position(|window| window == [0, 16])
        .expect("edge record");
    put_ref(&mut stream, edge + 24, 1);
    let fin = stream
        .windows(2)
        .position(|window| window == [0, 17])
        .expect("fin record");
    put_ref(&mut stream, fin + 18, 1);
    let graph = crate::topology::Graph::parse(&stream);
    let mut off_support_ir = ir.clone();
    let mut annotations = cadmpeg_ir::annotations::AnnotationBuilder::new();
    let stream = annotations.stream("nx:test");

    crate::decode::attach_tolerant_edge_intersections(
        &mut ir,
        &graph,
        &edges,
        "nx:test",
        stream,
        &mut annotations,
    );

    let edge = ir
        .model
        .edges
        .iter()
        .find(|edge| edge.id == edge_id)
        .expect("tolerant edge");
    assert_eq!(edge.param_range, None);
    let curve = ir
        .model
        .curves
        .iter()
        .find(|curve| Some(&curve.id) == edge.curve.as_ref())
        .expect("procedural carrier");
    assert!(matches!(curve.geometry, CurveGeometry::Procedural { .. }));
    let procedural = ir
        .model
        .procedural_curves
        .iter()
        .find(|procedural| procedural.curve == curve.id)
        .expect("intersection construction");
    let cadmpeg_ir::geometry::ProceduralCurveDefinition::TolerantIntersection {
        supports,
        endpoints,
        tolerance,
        parameterization,
    } = &procedural.definition
    else {
        panic!("tolerant intersection definition");
    };
    assert_ne!(supports[0], supports[1]);
    assert_eq!(*endpoints, expected_endpoints);
    assert_eq!(*tolerance, 0.01);
    assert_eq!(*parameterization, None);

    let start = off_support_ir.model.edges[0].start.clone();
    let point_id = off_support_ir
        .model
        .vertices
        .iter()
        .find(|vertex| vertex.id == start)
        .expect("edge vertex")
        .point
        .clone();
    let point = off_support_ir
        .model
        .points
        .iter_mut()
        .find(|point| point.id == point_id)
        .expect("vertex point");
    point.position.x += 0.5;
    point.position.y += 0.5;
    point.position.z += 0.5;
    let mut annotations = cadmpeg_ir::annotations::AnnotationBuilder::new();
    let stream = annotations.stream("nx:test");
    crate::decode::attach_tolerant_edge_intersections(
        &mut off_support_ir,
        &graph,
        &edges,
        "nx:test",
        stream,
        &mut annotations,
    );
    assert_eq!(off_support_ir.model.edges[0].curve, None);
}

#[test]
fn tolerant_edge_does_not_replace_a_serialized_fin_curve() {
    let mut ir = cadmpeg_ir::examples::unit_cube();
    let edge_id = ir.model.edges[0].id.clone();
    ir.model.edges[0].curve = None;
    ir.model.edges[0].param_range = None;
    ir.model.edges[0].tolerance = Some(0.01);
    let edges = std::collections::BTreeMap::from([(8, edge_id.clone())]);
    let mut stream = partnered_trimmed_topology_partition_stream();
    let edge = stream
        .windows(2)
        .position(|window| window == [0, 16])
        .expect("edge record");
    put_ref(&mut stream, edge + 24, 1);
    let graph = crate::topology::Graph::parse(&stream);
    let mut annotations = cadmpeg_ir::annotations::AnnotationBuilder::new();
    let source_stream = annotations.stream("nx:test");

    crate::decode::attach_tolerant_edge_intersections(
        &mut ir,
        &graph,
        &edges,
        "nx:test",
        source_stream,
        &mut annotations,
    );

    let edge = ir
        .model
        .edges
        .iter()
        .find(|edge| edge.id == edge_id)
        .expect("tolerant edge");
    assert_eq!(edge.curve, None);
    assert_eq!(edge.param_range, None);
    assert!(ir.model.procedural_curves.is_empty());
}

#[test]
fn intersection_support_completion_requires_one_unique_incident_complement() {
    use cadmpeg_ir::geometry::{
        IntcurveSupportContext, IntcurveSupportSide, Pcurve, ProceduralCurve,
    };
    use cadmpeg_ir::ids::{PcurveId, ProceduralCurveId};

    let mut ir = cadmpeg_ir::examples::unit_cube();
    let edge = ir.model.edges[0].clone();
    let incident = ir
        .model
        .coedges
        .iter()
        .filter(|coedge| coedge.edge == edge.id)
        .filter_map(|coedge| {
            let face = ir
                .model
                .loops
                .iter()
                .find(|loop_| loop_.id == coedge.owner_loop)?
                .face
                .clone();
            ir.model
                .faces
                .iter()
                .find(|candidate| candidate.id == face)
                .map(|face| face.surface.clone())
        })
        .collect::<Vec<_>>();
    assert_eq!(incident.len(), 2);
    let curve = edge.curve.expect("cube edge curve");
    ir.model.procedural_curves.push(ProceduralCurve {
        id: ProceduralCurveId("nx:test:intersection#0".into()),
        curve,
        definition: ProceduralCurveDefinition::Intersection {
            context: IntcurveSupportContext {
                sides: [
                    IntcurveSupportSide {
                        surface: Some(incident[0].clone()),
                        pcurve_parameter_range: None,
                        pcurve: None,
                    },
                    IntcurveSupportSide {
                        surface: None,
                        pcurve_parameter_range: None,
                        pcurve: None,
                    },
                ],
                parameter_range: [0.0, 1.0],
                discontinuities: [Vec::new(), Vec::new(), Vec::new()],
            },
            discontinuity_flag: false,
        },
        cache_fit_tolerance: None,
    });

    crate::decode::complete_intersection_supports_from_edge_incidence(&mut ir);
    let ProceduralCurveDefinition::Intersection { context, .. } =
        &ir.model.procedural_curves[0].definition
    else {
        panic!("intersection");
    };
    assert_eq!(context.sides[1].surface.as_ref(), Some(&incident[1]));

    let pcurve_id = PcurveId("nx:test:pcurve#0".into());
    let pcurve_geometry = PcurveGeometry::Line {
        origin: Point2::new(0.0, 0.0),
        direction: Point2::new(1.0, 0.0),
    };
    ir.model.pcurves.push(Pcurve {
        id: pcurve_id.clone(),
        geometry: pcurve_geometry.clone(),
        wrapper_reversed: None,
        native_tail_flags: None,
        parameter_range: Some([0.0, 1.0]),
        fit_tolerance: None,
    });
    let second_face = ir
        .model
        .faces
        .iter()
        .find(|face| face.surface == incident[1])
        .expect("second incident face")
        .id
        .clone();
    let second_loop = ir
        .model
        .loops
        .iter()
        .find(|loop_| loop_.face == second_face)
        .expect("second incident loop")
        .id
        .clone();
    ir.model
        .coedges
        .iter_mut()
        .find(|coedge| coedge.edge == edge.id && coedge.owner_loop == second_loop)
        .expect("second incident coedge")
        .pcurves = vec![cadmpeg_ir::topology::PcurveUse {
        pcurve: pcurve_id,
        isoparametric: None,
        parameter_range: None,
    }];

    crate::decode::complete_intersection_pcurves_from_coedge_incidence(&mut ir);
    let ProceduralCurveDefinition::Intersection { context, .. } =
        &ir.model.procedural_curves[0].definition
    else {
        panic!("intersection");
    };
    assert_eq!(context.sides[1].pcurve.as_ref(), Some(&pcurve_geometry));
}

#[test]
fn opposite_intersection_chart_transfers_adaptively_within_edge_tolerance() {
    use cadmpeg_ir::geometry::{
        Curve, IntcurveSupportContext, IntcurveSupportSide, ProceduralCurve, Surface,
    };
    use cadmpeg_ir::ids::{CurveId, EdgeId, ProceduralCurveId, SurfaceId, VertexId};
    use cadmpeg_ir::math::Point3;
    use cadmpeg_ir::topology::Edge;

    let mut ir = cadmpeg_ir::document::CadIr::empty(cadmpeg_ir::units::Units::default());
    let source = SurfaceId("synthetic:source-cylinder".into());
    let target = SurfaceId("synthetic:target-plane".into());
    ir.model.surfaces.extend([
        Surface {
            id: source.clone(),
            geometry: SurfaceGeometry::Cylinder {
                origin: Point3::new(0.0, 0.0, 0.0),
                axis: Vector3::new(0.0, 0.0, 1.0),
                ref_direction: Vector3::new(1.0, 0.0, 0.0),
                radius: 10.0,
            },
            source_object: None,
        },
        Surface {
            id: target.clone(),
            geometry: SurfaceGeometry::Plane {
                origin: Point3::new(0.0, 0.0, 0.0),
                normal: Vector3::new(0.0, 0.0, 1.0),
                u_axis: Vector3::new(1.0, 0.0, 0.0),
            },
            source_object: None,
        },
    ]);
    let curve = CurveId("synthetic:intersection-curve".into());
    let construction = ProceduralCurveId("synthetic:intersection".into());
    ir.model.curves.push(Curve {
        id: curve.clone(),
        geometry: CurveGeometry::Procedural {
            construction: construction.clone(),
        },
        source_object: None,
    });
    ir.model.procedural_curves.push(ProceduralCurve {
        id: construction,
        curve: curve.clone(),
        definition: ProceduralCurveDefinition::Intersection {
            context: IntcurveSupportContext {
                sides: [
                    IntcurveSupportSide {
                        surface: Some(source),
                        pcurve_parameter_range: None,
                        pcurve: Some(PcurveGeometry::Line {
                            origin: Point2::new(0.0, 0.0),
                            direction: Point2::new(std::f64::consts::TAU, 0.0),
                        }),
                    },
                    IntcurveSupportSide {
                        surface: Some(target.clone()),
                        pcurve_parameter_range: None,
                        pcurve: None,
                    },
                ],
                parameter_range: [0.0, 1.0],
                discontinuities: [Vec::new(), Vec::new(), Vec::new()],
            },
            discontinuity_flag: false,
        },
        cache_fit_tolerance: None,
    });
    ir.model.edges.push(Edge {
        id: EdgeId("synthetic:edge".into()),
        curve: Some(curve),
        start: VertexId("synthetic:start".into()),
        end: VertexId("synthetic:end".into()),
        param_range: Some([0.0, 1.0]),
        tolerance: Some(0.01),
    });

    crate::decode::complete_intersection_pcurves_from_opposite_charts(&mut ir);

    let ProceduralCurveDefinition::Intersection { context, .. } =
        &ir.model.procedural_curves[0].definition
    else {
        unreachable!()
    };
    let pcurve = context.sides[1].pcurve.as_ref().unwrap();
    let PcurveGeometry::Nurbs { control_points, .. } = pcurve else {
        unreachable!()
    };
    assert!(control_points.len() > 2);
    for parameter in [0.0, 0.25, 0.5, 0.75, 1.0] {
        let uv = cadmpeg_ir::eval::pcurve_uv(pcurve, parameter).unwrap();
        let point =
            cadmpeg_ir::eval::surface_point(&ir.model.surfaces[1].geometry, uv.u, uv.v).unwrap();
        let angle = std::f64::consts::TAU * parameter;
        assert!((point.x - 10.0 * angle.cos()).abs() < 0.01);
        assert!((point.y - 10.0 * angle.sin()).abs() < 0.01);
        assert!(point.z.abs() < 0.01);
    }
}

#[test]
fn blend_boundary_chart_uses_the_solved_curve_when_the_source_blend_is_unevaluable() {
    use cadmpeg_ir::geometry::{
        BlendSupport, Curve, IntcurveSupportContext, IntcurveSupportSide, ProceduralCurve,
        ProceduralSurface, Surface,
    };
    use cadmpeg_ir::ids::{
        CurveId, EdgeId, ProceduralCurveId, ProceduralSurfaceId, SurfaceId, VertexId,
    };
    use cadmpeg_ir::math::Point3;
    use cadmpeg_ir::topology::Edge;

    let mut ir = cadmpeg_ir::document::CadIr::empty(cadmpeg_ir::units::Units::default());
    let source = SurfaceId("synthetic:unevaluable-source-blend".into());
    let other_support = SurfaceId("synthetic:other-support".into());
    let target = SurfaceId("synthetic:target-blend".into());
    let target_construction = ProceduralSurfaceId("synthetic:target-blend-construction".into());
    ir.model.surfaces.extend([
        Surface {
            id: source.clone(),
            geometry: SurfaceGeometry::Unknown { record: None },
            source_object: None,
        },
        Surface {
            id: other_support.clone(),
            geometry: SurfaceGeometry::Plane {
                origin: Point3::new(0.0, 0.0, 0.0),
                normal: Vector3::new(0.0, 1.0, 0.0),
                u_axis: Vector3::new(0.0, 0.0, 1.0),
            },
            source_object: None,
        },
        Surface {
            id: target.clone(),
            geometry: SurfaceGeometry::Procedural {
                construction: target_construction.clone(),
            },
            source_object: None,
        },
    ]);
    let spine = CurveId("synthetic:target-spine".into());
    ir.model.curves.push(Curve {
        id: spine.clone(),
        geometry: CurveGeometry::Line {
            origin: Point3::new(0.0, 0.0, 0.0),
            direction: Vector3::new(0.0, 0.0, 1.0),
        },
        source_object: None,
    });
    ir.model.procedural_surfaces.push(ProceduralSurface {
        id: target_construction,
        surface: target.clone(),
        definition: ProceduralSurfaceDefinition::Blend {
            supports: [
                Some(BlendSupport {
                    surface: source.clone(),
                    reversed: false,
                }),
                Some(BlendSupport {
                    surface: other_support,
                    reversed: false,
                }),
            ],
            spine: Some(spine),
            radius: BlendRadiusLaw::Constant { signed_radius: 2.0 },
            cross_section: BlendCrossSection::Circular,
            native: None,
        },
        cache_fit_tolerance: None,
        record_bounds: None,
    });

    let curve = CurveId("synthetic:solved-boundary".into());
    let construction = ProceduralCurveId("synthetic:boundary-intersection".into());
    ir.model.curves.push(Curve {
        id: curve.clone(),
        geometry: CurveGeometry::Line {
            origin: Point3::new(2.0, 0.0, 0.0),
            direction: Vector3::new(0.0, 0.0, 1.0),
        },
        source_object: None,
    });
    ir.model.procedural_curves.push(ProceduralCurve {
        id: construction,
        curve: curve.clone(),
        definition: ProceduralCurveDefinition::Intersection {
            context: IntcurveSupportContext {
                sides: [
                    IntcurveSupportSide {
                        surface: Some(source),
                        pcurve_parameter_range: None,
                        pcurve: Some(PcurveGeometry::Line {
                            origin: Point2::new(0.0, 0.0),
                            direction: Point2::new(1.0, 0.0),
                        }),
                    },
                    IntcurveSupportSide {
                        surface: Some(target),
                        pcurve_parameter_range: None,
                        pcurve: None,
                    },
                ],
                parameter_range: [0.0, 1.0],
                discontinuities: [Vec::new(), Vec::new(), Vec::new()],
            },
            discontinuity_flag: false,
        },
        cache_fit_tolerance: None,
    });
    ir.model.edges.push(Edge {
        id: EdgeId("synthetic:boundary-edge".into()),
        curve: Some(curve),
        start: VertexId("synthetic:boundary-start".into()),
        end: VertexId("synthetic:boundary-end".into()),
        param_range: Some([0.0, 1.0]),
        tolerance: Some(1.0e-8),
    });

    crate::decode::complete_intersection_pcurves_from_opposite_charts(&mut ir);

    let ProceduralCurveDefinition::Intersection { context, .. } =
        &ir.model.procedural_curves[0].definition
    else {
        unreachable!()
    };
    let PcurveGeometry::Nurbs { control_points, .. } = context.sides[1].pcurve.as_ref().unwrap()
    else {
        unreachable!()
    };
    assert_eq!(control_points.first(), Some(&Point2::new(0.0, 0.0)));
    assert_eq!(control_points.last(), Some(&Point2::new(1.0, 0.0)));
}

#[test]
fn tolerant_nurbs_boundary_establishes_both_intersection_charts() {
    use cadmpeg_ir::geometry::{Curve, NurbsSurface, ProceduralCurve, Surface};
    use cadmpeg_ir::ids::{CurveId, EdgeId, PointId, ProceduralCurveId, SurfaceId, VertexId};
    use cadmpeg_ir::math::Point3;
    use cadmpeg_ir::topology::{Edge, Point, Vertex};

    let mut ir = cadmpeg_ir::document::CadIr::empty(cadmpeg_ir::units::Units::default());
    let nurbs = SurfaceId("synthetic:nurbs-boundary".into());
    let plane = SurfaceId("synthetic:boundary-plane".into());
    ir.model.surfaces.extend([
        Surface {
            id: nurbs.clone(),
            geometry: SurfaceGeometry::Nurbs(NurbsSurface {
                u_degree: 1,
                v_degree: 1,
                u_knots: vec![0.0, 0.0, 1.0, 1.0],
                v_knots: vec![0.0, 0.0, 1.0, 1.0],
                u_count: 2,
                v_count: 2,
                control_points: vec![
                    Point3::new(0.0, 0.0, 0.0),
                    Point3::new(0.0, 5.0, 0.0),
                    Point3::new(10.0, 0.0, 0.0),
                    Point3::new(10.0, 5.0, 0.0),
                ],
                weights: None,
                u_periodic: false,
                v_periodic: false,
            }),
            source_object: None,
        },
        Surface {
            id: plane.clone(),
            geometry: SurfaceGeometry::Plane {
                origin: Point3::new(0.0, 0.0, 0.0),
                normal: Vector3::new(0.0, 1.0, 0.0),
                u_axis: Vector3::new(1.0, 0.0, 0.0),
            },
            source_object: None,
        },
    ]);
    let curve = CurveId("synthetic:boundary-curve".into());
    let construction = ProceduralCurveId("synthetic:boundary-intersection".into());
    ir.model.curves.push(Curve {
        id: curve.clone(),
        geometry: CurveGeometry::Line {
            origin: Point3::new(0.0, 0.0, 0.0),
            direction: Vector3::new(10.0, 0.0, 0.0),
        },
        source_object: None,
    });
    ir.model.procedural_curves.push(ProceduralCurve {
        id: construction,
        curve: curve.clone(),
        definition: ProceduralCurveDefinition::TolerantIntersection {
            supports: [nurbs, plane],
            endpoints: [Point3::new(0.0, 0.0, 0.0), Point3::new(10.0, 0.0, 0.0)],
            tolerance: 1.0e-8,
            parameterization: None,
        },
        cache_fit_tolerance: None,
    });
    let point_ids = [
        PointId("synthetic:p0".into()),
        PointId("synthetic:p1".into()),
    ];
    let vertex_ids = [
        VertexId("synthetic:v0".into()),
        VertexId("synthetic:v1".into()),
    ];
    ir.model.points.extend([
        Point {
            id: point_ids[0].clone(),
            position: Point3::new(0.0, 0.0, 0.0),
            source_object: None,
        },
        Point {
            id: point_ids[1].clone(),
            position: Point3::new(10.0, 0.0, 0.0),
            source_object: None,
        },
    ]);
    ir.model.vertices.extend([
        Vertex {
            id: vertex_ids[0].clone(),
            point: point_ids[0].clone(),
            tolerance: Some(1.0e-8),
        },
        Vertex {
            id: vertex_ids[1].clone(),
            point: point_ids[1].clone(),
            tolerance: Some(1.0e-8),
        },
    ]);
    ir.model.edges.push(Edge {
        id: EdgeId("synthetic:boundary-edge".into()),
        curve: Some(curve),
        start: vertex_ids[0].clone(),
        end: vertex_ids[1].clone(),
        param_range: None,
        tolerance: Some(1.0e-8),
    });

    let mut annotations = cadmpeg_ir::AnnotationBuilder::new();
    crate::decode::complete_exact_boundary_intersection_pcurves(&mut ir, &mut annotations);

    let ProceduralCurveDefinition::TolerantIntersection {
        supports,
        parameterization: Some(parameterization),
        ..
    } = &ir.model.procedural_curves[0].definition
    else {
        unreachable!()
    };
    assert_eq!(
        ir.model.procedural_curves[0].cache_fit_tolerance,
        Some(1.0e-8)
    );
    assert_eq!(parameterization.parameter_range, [0.0, 1.0]);
    assert_eq!(ir.model.edges[0].param_range, Some([0.0, 1.0]));
    for parameter in [0.0, 0.25, 0.5, 0.75, 1.0] {
        let evaluated = cadmpeg_ir::eval::model_curve_point_by_id(
            &cadmpeg_ir::index::ModelIndex::new(&ir),
            &ir.model.procedural_curves[0].curve,
            parameter,
        )
        .expect("charted tolerant intersection evaluates");
        let inverted = cadmpeg_ir::eval::model_curve_parameter_near_point(
            &ir,
            &ir.model.procedural_curves[0].curve,
            evaluated,
            parameter,
        )
        .expect("charted tolerant intersection inverts");
        assert!((inverted - parameter).abs() < 1.0e-8);
        let points: [Point3; 2] = std::array::from_fn(|side| {
            let uv =
                cadmpeg_ir::eval::pcurve_uv(&parameterization.pcurves[side], parameter).unwrap();
            let surface = ir
                .model
                .surfaces
                .iter()
                .find(|surface| surface.id == supports[side])
                .unwrap();
            cadmpeg_ir::eval::surface_point(&surface.geometry, uv.u, uv.v).unwrap()
        });
        assert!((points[0].x - 10.0 * parameter).abs() < 1.0e-8);
        assert_eq!(evaluated, points[0]);
        assert!(
            (points[0].x - points[1].x)
                .hypot(points[0].y - points[1].y)
                .hypot(points[0].z - points[1].z)
                < 1.0e-8
        );
    }
}

#[test]
fn exact_boundary_completion_preserves_existing_cache_fit_tolerance() {
    use cadmpeg_ir::geometry::{
        Curve, IntcurveSupportContext, IntcurveSupportSide, ProceduralCurve, Surface,
    };
    use cadmpeg_ir::ids::{CurveId, EdgeId, PointId, ProceduralCurveId, SurfaceId, VertexId};
    use cadmpeg_ir::math::Point3;
    use cadmpeg_ir::topology::{Edge, Point, Vertex};

    let mut ir = cadmpeg_ir::document::CadIr::empty(cadmpeg_ir::units::Units::default());
    let first_support = SurfaceId("nx:test:boundary-plane-a".into());
    let second_support = SurfaceId("nx:test:boundary-plane-b".into());
    ir.model.surfaces.extend([
        Surface {
            id: first_support.clone(),
            geometry: SurfaceGeometry::Plane {
                origin: Point3::new(0.0, 0.0, 0.0),
                normal: Vector3::new(0.0, 1.0, 0.0),
                u_axis: Vector3::new(1.0, 0.0, 0.0),
            },
            source_object: None,
        },
        Surface {
            id: second_support.clone(),
            geometry: SurfaceGeometry::Plane {
                origin: Point3::new(0.0, 0.0, 0.0),
                normal: Vector3::new(0.0, 0.0, 1.0),
                u_axis: Vector3::new(1.0, 0.0, 0.0),
            },
            source_object: None,
        },
    ]);
    let curve = CurveId("nx:test:boundary-line".into());
    ir.model.curves.push(Curve {
        id: curve.clone(),
        geometry: CurveGeometry::Line {
            origin: Point3::new(0.0, 0.0, 0.0),
            direction: Vector3::new(10.0, 0.0, 0.0),
        },
        source_object: None,
    });
    let points = [
        (
            PointId("nx:test:boundary-point-0".into()),
            Point3::new(0.0, 0.0, 0.0),
        ),
        (
            PointId("nx:test:boundary-point-1".into()),
            Point3::new(10.0, 0.0, 0.0),
        ),
    ];
    ir.model
        .points
        .extend(points.iter().map(|(id, position)| Point {
            id: id.clone(),
            position: *position,
            source_object: None,
        }));
    let vertices = [
        VertexId("nx:test:boundary-vertex-0".into()),
        VertexId("nx:test:boundary-vertex-1".into()),
    ];
    ir.model.vertices.extend([
        Vertex {
            id: vertices[0].clone(),
            point: points[0].0.clone(),
            tolerance: None,
        },
        Vertex {
            id: vertices[1].clone(),
            point: points[1].0.clone(),
            tolerance: None,
        },
    ]);
    ir.model.edges.push(Edge {
        id: EdgeId("nx:test:boundary-edge".into()),
        curve: Some(curve.clone()),
        start: vertices[0].clone(),
        end: vertices[1].clone(),
        param_range: None,
        tolerance: Some(1.0e-8),
    });
    ir.model.procedural_curves.push(ProceduralCurve {
        id: ProceduralCurveId("nx:test:serialized-boundary".into()),
        curve,
        definition: ProceduralCurveDefinition::Intersection {
            context: IntcurveSupportContext {
                sides: [
                    IntcurveSupportSide {
                        surface: Some(first_support.clone()),
                        pcurve: None,
                        pcurve_parameter_range: None,
                    },
                    IntcurveSupportSide {
                        surface: Some(second_support.clone()),
                        pcurve: None,
                        pcurve_parameter_range: None,
                    },
                ],
                parameter_range: [0.0, 1.0],
                discontinuities: [Vec::new(), Vec::new(), Vec::new()],
            },
            discontinuity_flag: false,
        },
        cache_fit_tolerance: Some(0.25),
    });

    crate::decode::complete_exact_boundary_intersection_pcurves(
        &mut ir,
        &mut cadmpeg_ir::AnnotationBuilder::new(),
    );

    let procedural = ir
        .model
        .procedural_curves
        .last()
        .expect("boundary construction");
    assert_eq!(procedural.cache_fit_tolerance, Some(0.25));
    let ProceduralCurveDefinition::Intersection { context, .. } = &procedural.definition else {
        panic!("intersection construction");
    };
    assert!(context.sides.iter().all(|side| side.pcurve.is_some()));
}

#[test]
fn decode_attaches_dimension_two_bcurve_through_surface_curve() {
    let stream = pcurve_topology_partition_stream();
    let mut input = Cursor::new(prt_with_partition(&stream));
    let result = NxCodec
        .decode(&mut input, &DecodeOptions::default())
        .unwrap();

    assert_eq!(result.ir.model.pcurves.len(), 1);
    assert_eq!(
        result.ir.model.coedges[0]
            .pcurves
            .first()
            .map(|pcurve| &pcurve.pcurve),
        Some(&result.ir.model.pcurves[0].id)
    );
    let PcurveGeometry::Nurbs {
        degree,
        knots,
        control_points,
        weights,
        periodic,
    } = &result.ir.model.pcurves[0].geometry
    else {
        panic!("expected NURBS pcurve");
    };
    assert_eq!(*degree, 1);
    assert_eq!(knots, &[0.0, 0.0, 1.0, 1.0]);
    assert_eq!(
        control_points,
        &[Point2::new(10.0, 20.0), Point2::new(10.0, 20.0)]
    );
    assert!(weights.is_none());
    assert!(!periodic);
    assert_eq!(result.ir.model.pcurves[0].fit_tolerance, Some(0.01));
    assert_eq!(
        result.ir.model.points[0].position,
        cadmpeg_ir::math::Point3::new(10.0, 20.0, 0.0)
    );
    let validation = cadmpeg_ir::validate::validate(&result.ir, Vec::new());
    assert!(
        validation.findings.is_empty(),
        "findings: {:?}",
        validation.findings
    );
}

#[test]
fn decode_assigns_descending_pcurve_trim_to_the_coedge_use() {
    let mut stream = pcurve_topology_partition_stream();
    let fin = stream
        .windows(4)
        .position(|window| window == [0, 17, 0, 7])
        .expect("fin record");
    put_ref(&mut stream, fin + 18, 26);
    let mut trim = record(133, 85);
    put_ref(&mut trim, 2, 26);
    trim[18] = b'+';
    put_ref(&mut trim, 19, 25);
    put_f64(&mut trim, 69, 1.0);
    put_f64(&mut trim, 77, 0.0);
    stream.extend(trim);

    let mut input = Cursor::new(prt_with_partition(&stream));
    let result = NxCodec
        .decode(&mut input, &DecodeOptions::default())
        .unwrap();

    assert_eq!(result.ir.model.pcurves[0].parameter_range, None);
    assert_eq!(
        result.ir.model.coedges[0].pcurves[0].parameter_range,
        Some([0.0, 1.0])
    );
    assert!(cadmpeg_ir::validate::validate(&result.ir, Vec::new()).is_ok());
}

#[test]
fn decode_omits_surface_curve_missing_tolerance_sentinel() {
    let mut stream = pcurve_topology_partition_stream();
    let surface_curve = stream
        .windows(2)
        .position(|window| window == [0, 137])
        .expect("surface curve");
    put_f64(
        &mut stream,
        surface_curve + 25,
        crate::decode::MISSING_TOLERANCE,
    );
    let mut input = Cursor::new(prt_with_partition(&stream));
    let result = NxCodec
        .decode(&mut input, &DecodeOptions::default())
        .unwrap();

    assert_eq!(result.ir.model.pcurves[0].fit_tolerance, None);
    assert!(cadmpeg_ir::validate::validate(&result.ir, Vec::new()).is_ok());
}

#[test]
fn decode_rejects_overflowing_pcurve_parameter_conversion() {
    let mut stream = pcurve_topology_partition_stream();
    let payload = stream
        .windows(4)
        .position(|window| window == [0, 135, 0, 22])
        .expect("pcurve payload");
    put_f64(&mut stream, payload + 15, f64::MAX);

    let mut input = Cursor::new(prt_with_partition(&stream));
    let result = NxCodec
        .decode(&mut input, &DecodeOptions::default())
        .unwrap();
    assert!(result.ir.model.pcurves.is_empty());
    assert!(result.ir.model.coedges[0].pcurves.is_empty());
    assert!(cadmpeg_ir::validate::validate(&result.ir, Vec::new()).is_ok());
}

#[test]
fn decode_preserves_multiple_shells_in_one_region() {
    let stream = shared_region_shells_partition_stream();
    let mut input = Cursor::new(prt_with_partition(&stream));
    let result = NxCodec
        .decode(&mut input, &DecodeOptions::default())
        .unwrap();

    assert_eq!(result.ir.model.bodies.len(), 1);
    assert_eq!(result.ir.model.regions.len(), 1);
    assert_eq!(result.ir.model.shells.len(), 2);
    assert_eq!(result.ir.model.regions[0].shells.len(), 2);
    assert_eq!(result.ir.model.bodies[0].regions.len(), 1);
    let validation = cadmpeg_ir::validate::validate(&result.ir, Vec::new());
    assert!(validation.is_ok(), "findings: {:?}", validation.findings);
}

#[test]
fn nx_offset_surface_accepts_unbounded_representable_distance() {
    let mut stream = offset_surface_topology_partition_stream();
    let offset = stream
        .windows(4)
        .position(|window| window == [0, 60, 0, 12])
        .expect("offset record");
    put_f64(&mut stream, offset + 23, 1_001.0);
    let surfaces = crate::topology::offset_surfaces(&stream);
    let [surface] = surfaces.as_slice() else {
        panic!("offset surface")
    };
    assert_eq!(surface.distance, 1_001_000.0);

    put_f64(&mut stream, offset + 23, f64::INFINITY);
    assert!(crate::topology::offset_surfaces(&stream).is_empty());

    put_f64(&mut stream, offset + 23, f64::MAX);
    assert!(crate::topology::offset_surfaces(&stream).is_empty());
}

#[test]
fn offset_surface_envelope_does_not_consume_the_following_record() {
    let mut stream = offset_surface_topology_partition_stream();
    let offset_end = stream.len();
    let mut point = record(29, 40);
    put_ref(&mut point, 2, 20);
    put_vec3(&mut point, 16, [0.001, 0.002, 0.003]);
    stream.extend(point);

    let graph = crate::topology::Graph::parse(&stream);
    assert_eq!(
        graph.get(60, 12).map(crate::topology::Node::end),
        Some(offset_end)
    );
    assert!(graph.get(29, 20).is_some());
}

#[test]
fn nx_blend_surface_requires_a_nonzero_rolling_ball_radius() {
    let mut stream = blend_surface_topology_partition_stream();
    let blend = stream
        .windows(4)
        .position(|window| window == [0, 56, 0, 12])
        .expect("blend record");
    put_f64(&mut stream, blend + 26, 0.0);
    put_f64(&mut stream, blend + 34, 0.0);
    assert!(crate::topology::blend_surfaces(&stream).is_empty());

    put_f64(&mut stream, blend + 26, 0.5e-9);
    assert!(crate::topology::blend_surfaces(&stream).is_empty());

    put_f64(&mut stream, blend + 26, f64::MAX);
    put_f64(&mut stream, blend + 34, f64::MAX);
    assert!(crate::topology::blend_surfaces(&stream).is_empty());
}

#[test]
fn detect_high_on_magic() {
    assert_eq!(NxCodec.detect(MAGIC), Confidence::High);
    assert_eq!(NxCodec.detect(&single_part_prt()), Confidence::High);
    assert_eq!(NxCodec.detect(b"PK\x03\x04 not nx"), Confidence::No);
    // A Creo/Granite .prt shares the extension but not the magic.
    assert_eq!(NxCodec.detect(b"\xe0\x02\xff\xfeGRANITE"), Confidence::No);
}

#[test]
fn container_parses_header_and_directory() {
    let c = container::scan_bytes(single_part_prt()).unwrap();
    assert_eq!(c.version, 0x06);
    assert_eq!(c.file_tag, 0x33_22_11);
    assert_eq!(c.header_entry_count, 1);
    assert_eq!(c.footer_entry_count, 0);
    assert_eq!(c.footer_fingerprint, [0; 4]);
    assert!(c
        .entries
        .iter()
        .any(|e| e.name == "/Root/UG_PART/UG_PART" && e.file_span.is_some()));
}

#[test]
fn container_rejects_incomplete_counted_directories() {
    let mut header = single_part_prt();
    header[0x1f..0x23].copy_from_slice(&2_u32.to_le_bytes());
    assert!(container::scan_bytes(header).is_err());

    let mut footer = single_part_prt();
    let footer_offset = usize::try_from(u64::from_le_bytes([
        footer[0x11],
        footer[0x12],
        footer[0x13],
        footer[0x14],
        footer[0x15],
        footer[0x16],
        0,
        0,
    ]))
    .expect("synthetic footer offset");
    footer[footer_offset + 6..footer_offset + 10].copy_from_slice(&1_u32.to_le_bytes());
    assert!(container::scan_bytes(footer).is_err());
}

#[test]
fn container_rejects_trailing_or_overlapping_footer_data() {
    let mut trailing = single_part_prt();
    trailing.push(0);
    assert!(container::scan_bytes(trailing).is_err());

    let mut overlap = single_part_prt();
    let name_len = usize::try_from(u32::from_le_bytes(
        overlap[0x23..0x27]
            .try_into()
            .expect("synthetic name length"),
    ))
    .expect("synthetic name length fits usize");
    let span = 0x27 + name_len;
    let offset = u64::from_le_bytes(
        overlap[span..span + 8]
            .try_into()
            .expect("synthetic file offset"),
    );
    let footer_offset = u64::from_le_bytes([
        overlap[0x11],
        overlap[0x12],
        overlap[0x13],
        overlap[0x14],
        overlap[0x15],
        overlap[0x16],
        0,
        0,
    ]);
    overlap[span + 8..span + 16].copy_from_slice(&(footer_offset - offset + 1).to_le_bytes());
    assert!(container::scan_bytes(overlap).is_err());
}

#[test]
fn inspect_reports_bounded_nx_object_model_entities() {
    let mut cur = Cursor::new(prt_with_indexed_om_section());
    let summary = NxCodec
        .inspect(&mut cur, &InspectOptions::default())
        .unwrap();
    assert!(summary.notes.iter().any(|note| {
        note == "NX object model: 1 indexed section(s), 2 bounded entity record(s)"
    }));
}

#[test]
fn decode_projects_part_attributes_to_document_attributes() {
    let xml = br#"<?xml version="1.0" encoding="UTF-8"?>
<UgAttributes version="4" xmlns:xsi="http://www.w3.org/2001/XMLSchema-instance">
  <Attribute owner="part" pdmBased="false" utf8title="Material"
    utf8value="Steel" version="3" xsi:type="StringAttributeType"/>
</UgAttributes>"#;
    let file = prt_with_named_payloads(&[
        ("/Root/UG_PART/UG_PART", zlib_compress(&partition_stream())),
        ("/Root/part/attrs", xml.to_vec()),
    ]);
    let mut cur = Cursor::new(file);
    let result = NxCodec.decode(&mut cur, &DecodeOptions::default()).unwrap();

    assert_eq!(result.ir.model.attributes.len(), 1);
    let attribute = &result.ir.model.attributes[0];
    assert_eq!(attribute.name, "Material");
    assert_eq!(
        attribute.target,
        cadmpeg_ir::attributes::AttributeTarget::Document
    );
    assert_eq!(
        attribute.values,
        vec![cadmpeg_ir::attributes::AttributeValue::String(
            "Steel".to_string()
        )]
    );
    assert!(cadmpeg_ir::validate::validate(&result.ir, Vec::new()).is_ok());
}

#[test]
fn decode_exposes_strict_nx_jpeg_preview_metadata() {
    let preview = [
        0xff, 0xd8, 0xff, 0xe0, 0x00, 0x04, 0x00, 0x00, 0xff, 0xc0, 0x00, 0x11, 0x08, 0x00, 0xb9,
        0x00, 0xf7, 0x03, 0x01, 0x11, 0x00, 0x02, 0x11, 0x00, 0x03, 0x11, 0x00, 0xff, 0xd9,
    ];
    let file = prt_with_named_payloads(&[
        ("/Root/UG_PART/UG_PART", zlib_compress(&partition_stream())),
        ("/Root/images/preview", preview.to_vec()),
    ]);
    let container_only_file = file.clone();
    let result = NxCodec
        .decode(&mut Cursor::new(file), &DecodeOptions::default())
        .unwrap();
    let attributes = &result.ir.source.unwrap().attributes;
    assert_eq!(attributes["jpeg_preview_count"], "1");
    assert_eq!(attributes["jpeg_preview_0_width"], "247");
    assert_eq!(attributes["jpeg_preview_0_height"], "185");
    assert_eq!(attributes["jpeg_preview_0_precision"], "8");
    assert_eq!(attributes["jpeg_preview_0_components"], "3");
    assert_eq!(
        attributes["jpeg_preview_0_byte_len"],
        preview.len().to_string()
    );
    assert_eq!(result.ir.model.assets.len(), 1);
    let asset = &result.ir.model.assets[0];
    assert_eq!(asset.name.as_deref(), Some("preview.jpg"));
    assert_eq!(asset.media_type.as_deref(), Some("image/jpeg"));
    assert_eq!(
        asset.native_ref.as_deref(),
        Some("nx:container:jpeg-preview#0")
    );
    assert!(matches!(
        &asset.content,
        cadmpeg_ir::assets::AssetContent::Embedded { data } if data == &preview
    ));
    let container_only_result = NxCodec
        .decode(
            &mut Cursor::new(container_only_file),
            &DecodeOptions {
                container_only: true,
                ..DecodeOptions::default()
            },
        )
        .unwrap();
    assert_eq!(
        container_only_result.ir.model.assets,
        result.ir.model.assets
    );

    let mut malformed = preview;
    malformed[10..12].copy_from_slice(&16u16.to_be_bytes());
    assert!(crate::decode::jpeg_dimensions(&malformed).is_none());
    let malformed_file = prt_with_named_payloads(&[
        ("/Root/UG_PART/UG_PART", zlib_compress(&partition_stream())),
        ("/Root/images/preview", malformed.to_vec()),
    ]);
    let malformed_result = NxCodec
        .decode(&mut Cursor::new(malformed_file), &DecodeOptions::default())
        .unwrap();
    assert!(malformed_result.ir.model.assets.is_empty());
    let malformed_unknowns = malformed_result.ir.native_unknowns("nx").unwrap();
    assert!(malformed_unknowns
        .iter()
        .any(|unknown| unknown.id.0 == "nx:container:jpeg-preview#0"));
}

#[test]
fn decode_rejects_repeated_nx_arrangement_terminators_atomically() {
    let mut arrangements =
        br#"<Arrangements><Arrangement Default="YES" Name="Model"/></Arrangements>"#.to_vec();
    arrangements.extend_from_slice(&[0, 0]);
    let file = prt_with_named_payloads(&[
        ("/Root/UG_PART/UG_PART", zlib_compress(&partition_stream())),
        ("/Root/part/arrangements", arrangements),
    ]);
    let result = NxCodec
        .decode(&mut Cursor::new(file), &DecodeOptions::default())
        .unwrap();
    assert!(result.ir.model.configurations.is_empty());
}

#[test]
fn parasolid_extraction_classifies_partition_and_schema() {
    let f = single_part_prt();
    let streams = extract_streams(&f);
    let part = streams
        .iter()
        .find(|s| s.kind == StreamKind::Partition)
        .expect("a partition stream");
    assert_eq!(part.schema.as_deref(), Some("SCH_TEST_1_9999"));
    assert!(part.inflated.starts_with(b"PS\x00\x00"));
}

#[test]
fn decode_transfers_point_plane_cylinder_line() {
    let mut cur = Cursor::new(single_part_prt());
    let result = NxCodec.decode(&mut cur, &DecodeOptions::default()).unwrap();

    assert!(result.report.geometry_transferred);
    assert_eq!(result.ir.model.points.len(), 1);
    assert_eq!(result.ir.model.vertices.len(), 1);
    // Point coordinate is scaled metres → millimetres, byte-exact.
    let p = &result.ir.model.points[0].position;
    assert!((p.x - 62.5).abs() < 1e-6 && (p.z - 12.7).abs() < 1e-6);

    // One plane, one cylinder decoded.
    let planes = result
        .ir
        .model
        .surfaces
        .iter()
        .filter(|s| matches!(s.geometry, SurfaceGeometry::Plane { .. }))
        .count();
    let cyls: Vec<_> = result
        .ir
        .model
        .surfaces
        .iter()
        .filter_map(|s| match &s.geometry {
            SurfaceGeometry::Cylinder { radius, .. } => Some(*radius),
            _ => None,
        })
        .collect();
    assert_eq!(planes, 1);
    assert_eq!(cyls.len(), 1);
    assert!((cyls[0] - 4.05).abs() < 1e-6);
    assert!(result.ir.model.surfaces.iter().any(|surface| matches!(
        surface.geometry,
        SurfaceGeometry::Plane {
            u_axis: axis,
            ..
        } if axis == Vector3::new(1.0, 0.0, 0.0)
    )));
    assert!(result.ir.model.surfaces.iter().any(|surface| matches!(
        surface.geometry,
        SurfaceGeometry::Cylinder {
            ref_direction: direction,
            ..
        } if direction == Vector3::new(1.0, 0.0, 0.0)
    )));

    // One line decoded, with a unit direction.
    let lines: Vec<_> = result
        .ir
        .model
        .curves
        .iter()
        .filter(|c| matches!(c.geometry, CurveGeometry::Line { .. }))
        .collect();
    assert_eq!(lines.len(), 1);

    // No topology graph is fabricated; the loss is reported as blocking.
    assert!(result.ir.model.faces.is_empty() && result.ir.model.edges.is_empty());
    assert!(result.report.losses.iter().any(|l| l.code.category()
        == cadmpeg_ir::report::LossCategory::Topology
        && l.severity == cadmpeg_ir::report::Severity::Blocking));

    // The Parasolid stream is preserved verbatim.
    let unknowns = result.ir.native_unknowns("nx").unwrap();
    assert_eq!(unknowns.len(), 1);
    assert_eq!(result.source_fidelity.retained_records[0].sha256.len(), 64);
    assert_eq!(
        unknowns[0].links,
        ["nx:s0:surf#0", "nx:s0:surf#1", "nx:s0:crv#0",]
    );
    assert_eq!(
        result.source_fidelity.annotations.exactness[&unknowns[0].id.to_string()].fields["links"],
        Exactness::Derived
    );

    // The preserved stream owns partial-decode carriers without fabricating topology.
    let report = cadmpeg_ir::validate::validate(&result.ir, Vec::new());
    assert!(report.is_ok(), "findings: {:?}", report.findings);
}

#[test]
fn decode_emits_connected_primitive_brep() {
    let mut cur = Cursor::new(topology_part_prt());
    let result = NxCodec.decode(&mut cur, &DecodeOptions::default()).unwrap();

    assert_eq!(result.ir.model.bodies.len(), 1);
    assert_eq!(result.ir.model.regions.len(), 1);
    assert_eq!(result.ir.model.shells.len(), 1);
    assert_eq!(result.ir.model.faces.len(), 1);
    assert_eq!(result.ir.model.loops.len(), 1);
    assert_eq!(result.ir.model.coedges.len(), 1);
    assert_eq!(result.ir.model.edges.len(), 1);
    assert_eq!(result.ir.model.vertices.len(), 1);
    assert_eq!(
        result.ir.model.bodies[0].kind,
        cadmpeg_ir::topology::BodyKind::Sheet
    );
    assert_eq!(
        result.ir.model.faces[0].loops,
        vec![result.ir.model.loops[0].id.clone()]
    );
    assert_eq!(
        result.ir.model.edges[0].curve.as_ref(),
        Some(&result.ir.model.curves[0].id)
    );
    assert_eq!(result.ir.model.vertices[0].tolerance, Some(0.1));
    assert_eq!(result.ir.model.edges[0].tolerance, Some(0.3));
    assert_eq!(result.ir.model.faces[0].tolerance, Some(0.2));
    assert_eq!(
        result.ir.model.coedges[0].radial_next,
        result.ir.model.coedges[0].id
    );
    assert!(result
        .report
        .losses
        .iter()
        .all(|loss| loss.code.category() != cadmpeg_ir::report::LossCategory::Topology));
    assert!(result
        .report
        .losses
        .iter()
        .all(|loss| loss.code != LossKind::MaterialNotTransferred));
    assert!(result
        .report
        .losses
        .iter()
        .all(|loss| loss.code != LossKind::AttributesNotTransferred));
    assert!(!result.report.losses.iter().any(|loss| {
        loss.code == LossKind::AssemblyPlacementsNotTransferred
            && loss.message.contains("Assembly occurrence placements")
    }));
    let validation = cadmpeg_ir::validate::validate(&result.ir, Vec::new());
    assert!(validation.is_ok(), "findings: {:?}", validation.findings);
}

#[test]
fn decode_does_not_report_assembly_placements_for_inline_external_metadata() {
    let file = prt_with_named_payloads(&[
        (
            "/Root/UG_PART/UG_PART",
            zlib_compress(&topology_partition_stream()),
        ),
        (
            "/Root/UG_PART/ExternalReferences",
            external_reference_stream(),
        ),
    ]);
    let result = NxCodec
        .decode(&mut Cursor::new(file), &DecodeOptions::default())
        .unwrap();

    assert!(!result
        .report
        .losses
        .iter()
        .any(|loss| loss.code == LossKind::AssemblyPlacementsNotTransferred));
}

#[test]
fn decode_reports_external_assembly_boundary_without_inline_geometry() {
    let file = prt_with_named_payloads(&[(
        "/Root/UG_PART/ExternalReferences",
        external_reference_stream(),
    )]);
    let result = NxCodec
        .decode(&mut Cursor::new(file), &DecodeOptions::default())
        .unwrap();

    assert!(result.report.losses.iter().any(|loss| {
        loss.code == LossKind::AssemblyComponentsExternal
            && loss.message.contains("No inline Parasolid geometry")
    }));
    assert!(!result
        .report
        .losses
        .iter()
        .any(|loss| loss.code == LossKind::AssemblyPlacementsNotTransferred));
}

#[test]
fn retained_material_library_assets_do_not_imply_an_assignment_loss() {
    let file = prt_with_named_payloads(&[
        (
            "/Root/UG_PART/UG_PART",
            zlib_compress(&topology_partition_stream()),
        ),
        (
            "/Root/materialsTif/Steel",
            vec![b'M', b'M', 0, 42, 0, 0, 0, 8, 0, 0],
        ),
    ]);
    let result = NxCodec
        .decode(&mut Cursor::new(file), &DecodeOptions::default())
        .unwrap();

    assert_eq!(result.ir.model.assets.len(), 1);
    let asset = &result.ir.model.assets[0];
    assert_eq!(asset.name.as_deref(), Some("Steel"));
    assert_eq!(asset.media_type.as_deref(), Some("image/tiff"));
    assert!(matches!(
        &asset.content,
        cadmpeg_ir::assets::AssetContent::Embedded { data }
            if data == &[b'M', b'M', 0, 42, 0, 0, 0, 8, 0, 0]
    ));
    assert_eq!(
        asset.native_ref.as_deref(),
        Some("nx:container:material-texture#0")
    );
    assert!(result
        .report
        .losses
        .iter()
        .all(|loss| loss.code != LossKind::MaterialNotTransferred));
}

#[test]
fn offset_surface_parameter_solver_preserves_support_parameters() {
    let stream = offset_surface_topology_partition_stream();
    let mut cur = Cursor::new(prt_with_partition(&stream));
    let result = NxCodec.decode(&mut cur, &DecodeOptions::default()).unwrap();
    let surface = result.ir.model.procedural_surfaces[0].surface.clone();
    let expected = Point2::new(12.0, 7.0);
    let point = cadmpeg_ir::eval::model_surface_point_by_id(
        &cadmpeg_ir::index::ModelIndex::new(&result.ir),
        &surface,
        expected.u,
        expected.v,
    )
    .unwrap();

    let actual =
        crate::decode::offset_surface_parameters(&result.ir, &surface, point, None).unwrap();

    assert!((actual.u - expected.u).abs() < 1.0e-8);
    assert!((actual.v - expected.v).abs() < 1.0e-8);

    let mut translated = result.ir.clone();
    for carrier in &mut translated.model.surfaces {
        if let SurfaceGeometry::Plane { origin, .. } = &mut carrier.geometry {
            origin.x += 1.0e12;
            origin.y += 1.0e12;
            origin.z += 1.0e12;
        }
    }
    let translated_point = cadmpeg_ir::eval::model_surface_point_by_id(
        &cadmpeg_ir::index::ModelIndex::new(&translated),
        &surface,
        expected.u,
        expected.v,
    )
    .unwrap();
    let translated_parameters = crate::decode::offset_surface_parameters_with_tolerance(
        &translated,
        &surface,
        translated_point,
        Some(Point2::new(expected.u + 0.1, expected.v - 0.1)),
        Some(1.0e-3),
    )
    .expect("exact offset tangents are independent of model-space magnitude");
    assert!((translated_parameters.u - expected.u).abs() < 1.0e-3);
    assert!((translated_parameters.v - expected.v).abs() < 1.0e-3);

    let nested_surface = cadmpeg_ir::ids::SurfaceId("synthetic:nested-offset".into());
    let nested_construction =
        cadmpeg_ir::ids::ProceduralSurfaceId("synthetic:nested-offset-construction".into());
    translated
        .model
        .surfaces
        .push(cadmpeg_ir::geometry::Surface {
            id: nested_surface.clone(),
            geometry: SurfaceGeometry::Procedural {
                construction: nested_construction.clone(),
            },
            source_object: None,
        });
    translated
        .model
        .procedural_surfaces
        .push(cadmpeg_ir::geometry::ProceduralSurface {
            id: nested_construction,
            surface: nested_surface.clone(),
            definition: ProceduralSurfaceDefinition::Offset {
                support: surface,
                distance: -0.75,
                u_sense: None,
                v_sense: None,
                extension_flags: Vec::new(),
                revision_form: None,
            },
            cache_fit_tolerance: None,
            record_bounds: None,
        });
    let nested_point = cadmpeg_ir::eval::model_surface_point_by_id(
        &cadmpeg_ir::index::ModelIndex::new(&translated),
        &nested_surface,
        expected.u,
        expected.v,
    )
    .unwrap();
    let nested_parameters = crate::decode::offset_surface_parameters_with_tolerance(
        &translated,
        &nested_surface,
        nested_point,
        Some(Point2::new(expected.u - 0.1, expected.v + 0.1)),
        Some(1.0e-3),
    )
    .expect("nested offsets share the exact base-surface normal derivative");
    assert!((nested_parameters.u - expected.u).abs() < 1.0e-3);
    assert!((nested_parameters.v - expected.v).abs() < 1.0e-3);
}

#[test]
fn offset_surface_parameter_solver_accepts_a_seed_within_fit_tolerance() {
    let stream = offset_surface_topology_partition_stream();
    let mut cur = Cursor::new(prt_with_partition(&stream));
    let result = NxCodec.decode(&mut cur, &DecodeOptions::default()).unwrap();
    let surface = result.ir.model.procedural_surfaces[0].surface.clone();
    let seed = Point2::new(12.0, 7.0);
    let mut point = cadmpeg_ir::eval::model_surface_point_by_id(
        &cadmpeg_ir::index::ModelIndex::new(&result.ir),
        &surface,
        seed.u,
        seed.v,
    )
    .unwrap();
    point.x += 0.01;

    let actual = crate::decode::offset_surface_parameters_with_tolerance(
        &result.ir,
        &surface,
        point,
        Some(seed),
        Some(0.02),
    )
    .unwrap();

    assert_eq!(actual, seed);
}

#[test]
fn decode_tracks_fully_extended_offset_common_header() {
    let stream = offset_surface_with_fully_extended_common_header();
    assert_eq!(crate::topology::offset_surfaces(&stream).len(), 1);
    let mut cur = Cursor::new(prt_with_partition(&stream));
    let result = NxCodec.decode(&mut cur, &DecodeOptions::default()).unwrap();

    let procedural = result
        .ir
        .model
        .procedural_surfaces
        .first()
        .expect("offset surface");
    let ProceduralSurfaceDefinition::Offset {
        support, distance, ..
    } = &procedural.definition
    else {
        panic!("offset definition");
    };
    assert_eq!(*distance, 2.5);
    assert_ne!(procedural.surface, *support);
    assert_eq!(result.ir.model.faces[0].surface, procedural.surface);
}

#[test]
fn decode_tracks_fully_extended_compact_geometry_headers() {
    let mut blend = blend_surface_topology_partition_stream();
    fully_extend_common_header(&mut blend, [0, 56, 0, 12]);
    assert_eq!(crate::topology::blend_surfaces(&blend).len(), 1);

    let mut intersection = intersection_curve_topology_partition_stream();
    fully_extend_common_header(&mut intersection, [0, 38, 0, 12]);
    assert_eq!(crate::topology::composite_curves(&intersection).len(), 1);

    let mut surface_curve = surface_curve_topology_partition_stream();
    fully_extend_common_header(&mut surface_curve, [0, 137, 0, 12]);
    let surface_curves = crate::topology::surface_curves(&surface_curve);
    assert_eq!(surface_curves.len(), 1);
    assert_eq!(surface_curves[0].xmt, 12);
    assert_eq!(surface_curves[0].pcurve, 9);

    let mut trimmed = trimmed_topology_partition_stream();
    fully_extend_common_header(&mut trimmed, [0, 133, 0, 12]);
    let trims = crate::topology::trimmed_curves(&trimmed);
    assert_eq!(trims.len(), 1);
    assert_eq!(trims[0].parameters, [0.000_25, 0.000_75]);

    let mut bspline = bspline_partition_stream();
    fully_extend_common_header(&mut bspline, [0, 124, 0, 10]);
    fully_extend_common_header(&mut bspline, [0, 134, 0, 50]);
    let mut cur = Cursor::new(prt_with_partition(&bspline));
    let result = NxCodec.decode(&mut cur, &DecodeOptions::default()).unwrap();
    assert!(result
        .ir
        .model
        .surfaces
        .iter()
        .any(|surface| matches!(surface.geometry, SurfaceGeometry::Nurbs(_))));
    assert!(result
        .ir
        .model
        .curves
        .iter()
        .any(|curve| matches!(curve.geometry, CurveGeometry::Nurbs(_))));
}

#[test]
fn intersection_construction_recovers_one_missing_term_from_unique_edge_endpoints() {
    let mut stream = charted_intersection_with_edge_endpoint_witnesses_stream();
    let intersection = stream
        .windows(4)
        .position(|window| window == [0, 38, 0, 12])
        .expect("intersection record");
    put_ref(&mut stream, intersection + 25, 1);
    let scan = crate::intersection::scan(&stream, crate::intersection::ChartPointLayout::Xyz3);
    assert_eq!(scan.constructions.len(), 1);
    assert_eq!(scan.curves.len(), 1);
    assert_eq!(
        scan.rejected,
        crate::intersection::RejectionCounts::default()
    );
}

#[test]
fn intersection_construction_rejects_missing_term_without_topology_endpoint_match() {
    let mut stream = charted_intersection_with_edge_endpoint_witnesses_stream();
    let intersection = stream
        .windows(4)
        .position(|window| window == [0, 38, 0, 12])
        .expect("intersection record");
    put_ref(&mut stream, intersection + 25, 1);
    let chart = stream
        .windows(8)
        .position(|window| window == [0, 40, 0, 0, 0, 2, 0, 20])
        .expect("chart record");
    put_f64(&mut stream, chart + 60, 0.005);

    let scan = crate::intersection::scan(&stream, crate::intersection::ChartPointLayout::Xyz3);
    assert_eq!(scan.constructions.len(), 1);
    assert!(scan.curves.is_empty());
    assert_eq!(scan.rejected.missing_start_term, 1);
}

#[test]
fn intersection_auxiliaries_reject_duplicate_identities() {
    fn append_record(stream: &mut Vec<u8>, marker: &[u8], len: usize) {
        let start = stream
            .windows(marker.len())
            .position(|window| window == marker)
            .expect("auxiliary record");
        let duplicate = stream[start..start + len].to_vec();
        stream.extend(duplicate);
    }

    let mut chart = charted_intersection_curve_topology_partition_stream();
    append_record(&mut chart, &[0, 40, 0, 0, 0, 2, 0, 20], 108);
    let scan = crate::intersection::scan(&chart, crate::intersection::ChartPointLayout::Xyz3);
    assert!(scan.curves.is_empty());
    assert_eq!(scan.rejected.missing_chart, 1);
    assert_eq!(
        crate::intersection::scan_with_auxiliary_replacements(
            &chart,
            &chart[..chart.len() - 108],
            &[&chart[chart.len() - 108..]],
        )
        .curves
        .len(),
        1
    );

    let base_term = charted_intersection_curve_topology_partition_stream();
    let mut term = base_term.clone();
    append_record(&mut term, &[0, 41, 0, 0, 0, 1, 0, 21], 34);
    assert_eq!(crate::intersection::term_use_records(&term).len(), 1);
    let scan = crate::intersection::scan(&term, crate::intersection::ChartPointLayout::Xyz3);
    assert!(scan.curves.is_empty());
    assert_eq!(scan.rejected.missing_start_term, 1);
    assert_eq!(
        crate::intersection::scan_with_auxiliary_replacements(
            &term,
            &base_term,
            &[&term[base_term.len()..]],
        )
        .curves
        .len(),
        1
    );

    let mut uv = charted_intersection_curve_topology_partition_stream();
    append_record(&mut uv, &[0, 204, 0, 0, 0, 4, 0, 23], 41);
    assert!(crate::intersection::support_uv_records(&uv).is_empty());
    let [curve] = crate::intersection::scan(&uv, crate::intersection::ChartPointLayout::Xyz3)
        .curves
        .try_into()
        .unwrap();
    assert_eq!(curve.support_uv, [None, None]);

    let mut blend_bound = blend_bound_charted_intersection_curve_stream();
    append_record(&mut blend_bound, &[0, 59, 0, 14], 24);
    assert!(crate::intersection::blend_bounds(&blend_bound).is_empty());
}

#[test]
fn intersection_rejection_census_requires_resolved_supports() {
    let mut stream = charted_intersection_curve_topology_partition_stream();
    let intersection = stream
        .windows(4)
        .position(|window| window == [0, 38, 0, 12])
        .expect("intersection record");
    put_ref(&mut stream, intersection + 19, 998);
    put_ref(&mut stream, intersection + 21, 999);
    put_ref(&mut stream, intersection + 23, 997);

    let scan = crate::intersection::scan(&stream, crate::intersection::ChartPointLayout::Xyz3);
    assert!(scan.constructions.is_empty());
    assert!(scan.curves.is_empty());
    assert_eq!(
        scan.rejected,
        crate::intersection::RejectionCounts::default()
    );
}

#[test]
fn uncharted_intersection_requires_exact_topology_bounds() {
    let mut stream = two_support_charted_intersection_curve_stream();
    let intersection = stream
        .windows(4)
        .position(|window| window == [0, 38, 0, 12])
        .expect("intersection record");
    for offset in [23, 25, 27] {
        put_ref(&mut stream, intersection + offset, 1);
    }

    let scan = crate::intersection::scan(&stream, crate::intersection::ChartPointLayout::Xyz3);
    let [uncharted] = scan.uncharted.as_slice() else {
        panic!("one bounded uncharted intersection");
    };
    assert!(uncharted.supports.iter().all(|support| *support > 1));
    assert_ne!(uncharted.supports[0], uncharted.supports[1]);
    assert!(uncharted.tolerance.is_finite() && uncharted.tolerance > 0.0);

    let edge = stream
        .windows(4)
        .position(|window| window == [0, 16, 0, 8])
        .expect("edge record");
    stream[edge + 10..edge + 18].copy_from_slice(&f64::NAN.to_be_bytes());
    assert!(
        crate::intersection::scan(&stream, crate::intersection::ChartPointLayout::Xyz3)
            .uncharted
            .is_empty()
    );
}

#[test]
fn intersection_chart_accepts_one_matching_parameter_complement() {
    let ext11 = ext11_charted_intersection_curve_stream();
    let ext11_start = ext11
        .windows(8)
        .position(|window| window == [0, 40, 0, 0, 0, 2, 0, 20])
        .expect("ext11 chart");
    let complement = ext11[ext11_start..ext11_start + 236].to_vec();

    let base = charted_intersection_curve_topology_partition_stream();
    let mut stream = base.clone();
    stream.extend_from_slice(&complement);
    let [curve] =
        crate::intersection::scan_with_auxiliary_replacements(&stream, &base, &[&complement])
            .curves
            .try_into()
            .expect("complemented curve");
    assert_eq!(curve.parameters, [2.0, 5.0]);

    let base_chart = crate::intersection::chart_source_records(
        &base,
        crate::intersection::ChartPointLayout::Xyz3,
    )[0]
    .pos;
    let (_, base_chart_end) = crate::intersection::chart_source_record_at(
        &base,
        base_chart,
        crate::intersection::ChartPointLayout::Xyz3,
    )
    .expect("base chart bounds");
    let duplicate_chart = base[base_chart..base_chart_end].to_vec();
    let mut duplicate_stream = base.clone();
    duplicate_stream.extend_from_slice(&duplicate_chart);
    let scan = crate::intersection::scan(
        &duplicate_stream,
        crate::intersection::ChartPointLayout::Xyz3,
    );
    assert!(scan.curves.is_empty());
    assert_eq!(scan.rejected.missing_chart, 1);
}

#[test]
fn intersection_chart_accepts_encoded_count_without_arbitrary_ceiling() {
    let count = 1025usize;
    let mut chart = record(40, 60 + count * 24);
    chart[2..6].copy_from_slice(&(count as u32).to_be_bytes());
    put_ref(&mut chart, 6, 20);
    put_f64(&mut chart, 8, 0.0);
    put_f64(&mut chart, 16, 1.0);
    chart[24..28].copy_from_slice(&(count as u32).to_be_bytes());
    put_f64(&mut chart, 28, 0.00001);
    put_f64(&mut chart, 36, 0.001);
    put_f64(&mut chart, 44, -31_415_800_000_000.0);
    put_f64(&mut chart, 52, -31_415_800_000_000.0);
    for index in 0..count {
        put_vec3(
            &mut chart,
            60 + index * 24,
            [index as f64 * 0.001, 0.0, 0.0],
        );
    }

    let [chart] = crate::intersection::chart_source_records(
        &chart,
        crate::intersection::ChartPointLayout::Xyz3,
    )
    .try_into()
    .expect("one wide chart");
    assert_eq!(chart.count, count as u32);
    assert_eq!(chart.chart_count, count as u32);
    assert_eq!(chart.points.len(), count);
}

#[test]
fn decode_lifts_pcurve_only_fin_carrier_to_its_surface() {
    let mut stream = pcurve_topology_partition_stream();
    let edge = stream
        .windows(4)
        .position(|window| window == [0, 16, 0, 8])
        .expect("edge record");
    put_ref(&mut stream, edge + 24, 1);
    let surface_curve = stream
        .windows(4)
        .position(|window| window == [0, 137, 0, 25])
        .expect("surface curve");
    put_ref(&mut stream, surface_curve + 23, 1);

    let mut cur = Cursor::new(prt_with_partition(&stream));
    let result = NxCodec.decode(&mut cur, &DecodeOptions::default()).unwrap();

    let carrier = result.ir.model.edges[0]
        .curve
        .as_ref()
        .and_then(|id| result.ir.model.curves.iter().find(|curve| &curve.id == id))
        .expect("lifted carrier");
    assert!(matches!(carrier.geometry, CurveGeometry::Procedural { .. }));
    let ProceduralCurveDefinition::SurfaceCurve {
        family: cadmpeg_ir::geometry::SurfaceCurveFamily::Parametric,
        context,
        ..
    } = &result.ir.model.procedural_curves[0].definition
    else {
        panic!("parametric surface curve");
    };
    assert_eq!(
        context.sides[0].surface,
        Some(result.ir.model.faces[0].surface.clone())
    );
    assert!(context.sides[0].pcurve.is_some());
    let validation = cadmpeg_ir::validate::validate(&result.ir, Vec::new());
    assert!(validation.is_ok(), "findings: {:?}", validation.findings);
}

#[test]
fn decode_emits_blend_with_extended_support_reference() {
    let stream = blend_surface_with_extended_support_reference();
    let mut cur = Cursor::new(prt_with_partition(&stream));
    let result = NxCodec.decode(&mut cur, &DecodeOptions::default()).unwrap();

    assert_eq!(result.ir.model.procedural_surfaces.len(), 1);
    assert_eq!(
        result.ir.model.faces[0].surface,
        result.ir.model.procedural_surfaces[0].surface
    );
}

#[test]
fn decode_binds_blend_ball_centre_spine() {
    let stream = blend_surface_with_intersection_spine();
    let mut cur = Cursor::new(prt_with_partition(&stream));
    let result = NxCodec.decode(&mut cur, &DecodeOptions::default()).unwrap();

    let ProceduralSurfaceDefinition::Blend { spine, .. } =
        &result.ir.model.procedural_surfaces[0].definition
    else {
        panic!("blend definition");
    };
    assert_eq!(
        spine.as_ref(),
        Some(&result.ir.model.procedural_curves[0].curve)
    );
    assert!(cadmpeg_ir::validate::validate(&result.ir, Vec::new()).is_ok());
}

#[test]
fn decode_resolves_forward_blend_support_reference() {
    let stream = blend_surface_with_forward_blend_support();
    let mut cur = Cursor::new(prt_with_partition(&stream));
    let result = NxCodec.decode(&mut cur, &DecodeOptions::default()).unwrap();

    assert_eq!(result.ir.model.procedural_surfaces.len(), 2);
    let ProceduralSurfaceDefinition::Blend { supports, .. } =
        &result.ir.model.procedural_surfaces[0].definition
    else {
        panic!("blend definition");
    };
    assert_eq!(
        supports[0].as_ref().map(|support| &support.surface),
        Some(&result.ir.model.procedural_surfaces[1].surface)
    );
    assert!(cadmpeg_ir::validate::validate(&result.ir, Vec::new()).is_ok());
}

#[test]
fn decode_reports_status_framed_deltas_records_and_tombstones() {
    let stream = status_framed_deltas_stream();
    assert_eq!(
        crate::deltas::walk(&stream).bytes_decoded,
        stream.len() - DELTAS_PREAMBLE.len()
    );
    let mut cur = Cursor::new(prt_with_partition(&stream));
    let result = NxCodec.decode(&mut cur, &DecodeOptions::default()).unwrap();
    let attributes = &result.ir.source.expect("source metadata").attributes;

    assert_eq!(
        attributes.get("deltas.0.full.FACE").map(String::as_str),
        Some("1")
    );
    assert_eq!(
        attributes
            .get("deltas.0.tombstone.EDGE")
            .map(String::as_str),
        Some("1")
    );
    assert_eq!(
        attributes.get("deltas.0.grammar").map(String::as_str),
        Some("typed_status_framed_records")
    );
}

#[test]
fn decode_accepts_exact_loop_and_rejects_incomplete_fin_deltas() {
    let stream = variable_status_framed_deltas_stream();
    let mut cur = Cursor::new(prt_with_partition(&stream));
    let result = NxCodec.decode(&mut cur, &DecodeOptions::default()).unwrap();
    let attributes = &result.ir.source.expect("source metadata").attributes;

    assert!(!attributes.contains_key("deltas.0.full.FIN"));
    assert_eq!(
        attributes.get("deltas.0.full.LOOP").map(String::as_str),
        Some("1")
    );
}

#[test]
fn deltas_walks_complete_status_prefixed_entity_51_records() {
    let mut stream = vec![0, 81];
    stream.extend_from_slice(&1u32.to_be_bytes());
    stream.extend_from_slice(&10u16.to_be_bytes());
    stream.extend_from_slice(&2u32.to_be_bytes());
    stream.extend_from_slice(&0x21u16.to_be_bytes());
    for (status, reference) in [1, 1, 0, 1, 0, 1].into_iter().zip(3..=8u16) {
        stream.push(status);
        stream.extend_from_slice(&reference.to_be_bytes());
    }
    stream.push(0);
    let entity_len = stream.len();
    stream.extend(status_framed_deltas_point_stream());

    let census = crate::deltas::walk(&stream);
    assert_eq!(census.records.len(), 2);
    assert_eq!(census.records[0].kind, 81);
    assert_eq!(census.records[0].xmt, 10);
    assert_eq!(census.records[0].node_id, None);
    assert_eq!(census.records[0].references, [3, 4, 5, 6, 7, 8]);
    assert_eq!(census.records[0].end, entity_len);
    assert_eq!(census.full_counts["ENTITY_51"], 1);
    assert_eq!(census.bytes_decoded, stream.len());
    let residual = crate::deltas::semantic_residual(&stream);
    let retained = crate::parasolid::entity_51_records(&residual);
    assert_eq!(retained.len(), 1);
    assert_eq!(retained[0].xmt, 10);
    assert!(residual[..stream.len()].iter().all(|byte| *byte == 0xff));

    stream[entity_len - 1] = 1;
    assert_eq!(
        crate::deltas::walk(&stream)
            .records
            .iter()
            .filter(|record| record.kind == 81)
            .count(),
        1
    );
    stream[entity_len - 1] = 2;
    assert!(crate::deltas::walk(&stream)
        .records
        .iter()
        .all(|record| record.kind != 81));
}

#[test]
fn deltas_walks_attribute_records_that_share_a_terminal_zero() {
    let mut stream = vec![0, 84];
    stream.extend_from_slice(&1u32.to_be_bytes());
    stream.extend_from_slice(&9u16.to_be_bytes());
    stream.extend_from_slice(b"a\0");
    stream.push(81);
    stream.extend_from_slice(&1u32.to_be_bytes());
    stream.extend_from_slice(&10u16.to_be_bytes());
    stream.extend_from_slice(&2u32.to_be_bytes());
    stream.extend_from_slice(&0x21u16.to_be_bytes());
    for (status, reference) in [1, 1, 0, 1, 0, 1].into_iter().zip(3..=8u16) {
        stream.push(status);
        stream.extend_from_slice(&reference.to_be_bytes());
    }
    stream.push(0);
    stream.push(82);
    stream.extend_from_slice(&1u32.to_be_bytes());
    stream.extend_from_slice(&11u16.to_be_bytes());
    stream.extend_from_slice(&12u32.to_be_bytes());

    let census = crate::deltas::walk(&stream);

    assert_eq!(
        census
            .records
            .iter()
            .map(|record| record.kind)
            .collect::<Vec<_>>(),
        [84, 81, 82]
    );
    assert_eq!(census.records[1].offset, census.records[0].end - 1);
    assert_eq!(census.records[2].offset, census.records[1].end - 1);
    assert_eq!(census.bytes_decoded, stream.len());
    assert!(crate::deltas::semantic_residual(&stream)[..stream.len()]
        .iter()
        .all(|byte| *byte == 0xff));
}

#[test]
fn deltas_walks_fixed_record_that_shares_a_terminal_zero() {
    let mut stream = vec![0, 84];
    stream.extend_from_slice(&1u32.to_be_bytes());
    stream.extend_from_slice(&9u16.to_be_bytes());
    stream.extend_from_slice(b"a\0");
    let point = status_framed_deltas_point_stream();
    stream.extend_from_slice(&point[1..]);

    let census = crate::deltas::walk(&stream);

    assert_eq!(
        census
            .records
            .iter()
            .map(|record| record.kind)
            .collect::<Vec<_>>(),
        [84, 29]
    );
    assert_eq!(census.records[1].offset, census.records[0].end - 1);
    assert_eq!(census.bytes_decoded, stream.len());
}

#[test]
fn deltas_fixed_records_share_a_terminal_zero_with_their_successor() {
    let mut stream = Vec::new();
    stream.extend_from_slice(&13u16.to_be_bytes());
    stream.extend_from_slice(&47u16.to_be_bytes());
    stream.extend_from_slice(&61u32.to_be_bytes());
    for (reference, status) in [1u16, 2, 1, 3, 1, 1, 4, 3]
        .into_iter()
        .zip([1, 1, 1, 1, 1, 1, 1, 0])
    {
        stream.extend_from_slice(&reference.to_be_bytes());
        stream.push(status);
    }
    let intersection = status_framed_deltas_intersection_stream();
    stream.extend_from_slice(&intersection[1..]);

    let census = crate::deltas::walk(&stream);

    assert_eq!(
        census
            .records
            .iter()
            .map(|record| record.kind)
            .collect::<Vec<_>>(),
        [13, 38]
    );
    assert_eq!(census.records[1].offset, census.records[0].end - 1);
    assert_eq!(census.bytes_decoded, stream.len());
}

#[test]
fn deltas_type_101_record_takes_precedence_over_an_overlapping_fixed_candidate() {
    let mut type_101 = vec![0, 101];
    type_101.extend_from_slice(&2u16.to_be_bytes());
    for reference in 3u16..15 {
        type_101.extend_from_slice(&reference.to_be_bytes());
        type_101.push(1);
    }
    type_101.push(1);
    type_101.extend_from_slice(&[0; 12]);
    for reference in 15u16..18 {
        type_101.extend_from_slice(&reference.to_be_bytes());
        type_101.push(1);
    }

    let mut stream = 13u16.to_be_bytes().to_vec();
    stream.extend(encoded_xmt(256));
    stream.extend_from_slice(&1u32.to_be_bytes());
    for reference in [1u32, 2, 1, 3, 1, 1, 4] {
        stream.extend(encoded_xmt(reference));
        stream.push(1);
    }
    stream.extend_from_slice(&type_101[..3]);
    stream.extend_from_slice(&type_101[3..]);

    let census = crate::deltas::walk(&stream);

    assert_eq!(
        census
            .records
            .iter()
            .map(|record| record.kind)
            .collect::<Vec<_>>(),
        [101]
    );
    assert_eq!(census.records[0].offset, 29);
    assert_eq!(census.bytes_decoded, type_101.len());
}

#[test]
fn deltas_does_not_share_a_consecutive_reference_byte() {
    let mut stream = vec![0, 81];
    stream.extend_from_slice(&1u32.to_be_bytes());
    stream.extend_from_slice(&10u16.to_be_bytes());
    stream.extend_from_slice(&2u32.to_be_bytes());
    stream.extend_from_slice(&0x21u16.to_be_bytes());
    for reference in [3u16, 4, 5, 6, 7, 256] {
        stream.extend_from_slice(&reference.to_be_bytes());
    }
    let point = status_framed_deltas_point_stream();
    stream.extend_from_slice(&point[1..]);

    let census = crate::deltas::walk(&stream);

    assert_eq!(
        census
            .records
            .iter()
            .map(|record| record.kind)
            .collect::<Vec<_>>(),
        [81]
    );
}

#[test]
fn deltas_walks_complete_entity_value_records() {
    let mut stream = vec![0, 82];
    stream.extend_from_slice(&1u32.to_be_bytes());
    stream.extend_from_slice(&20u16.to_be_bytes());
    stream.extend_from_slice(&u32::MAX.to_be_bytes());
    stream.extend_from_slice(&[0, 83, 0xff]);
    stream.extend_from_slice(&1u32.to_be_bytes());
    stream.extend_from_slice(&21u16.to_be_bytes());
    stream.extend_from_slice(&0.25f64.to_be_bytes());
    stream.extend_from_slice(&[0, 84]);
    stream.extend_from_slice(&3u32.to_be_bytes());
    stream.extend_from_slice(&22u16.to_be_bytes());
    stream.extend_from_slice(b"abc\0");
    let decoded_len = stream.len();
    stream.extend_from_slice(&[0xfe, 0xdc, 0xba]);

    let census = crate::deltas::walk(&stream);
    assert_eq!(
        census
            .records
            .iter()
            .map(|record| record.kind)
            .collect::<Vec<_>>(),
        [82, 83, 84]
    );
    assert_eq!(census.full_counts["ENTITY_52"], 1);
    assert_eq!(census.full_counts["ENTITY_53"], 1);
    assert_eq!(census.full_counts["ENTITY_54"], 1);
    assert_eq!(census.bytes_decoded, decoded_len);

    let residual = crate::deltas::semantic_residual(&stream);
    assert!(residual[..decoded_len].iter().all(|byte| *byte == 0xff));
    assert_eq!(&residual[decoded_len..stream.len()], &[0xfe, 0xdc, 0xba]);
    assert_eq!(
        crate::parasolid::entity_52_integer_records(&residual)[0].values,
        [u32::MAX]
    );
    assert_eq!(
        crate::parasolid::entity_53_double_records(&residual)[0].values,
        [0.25]
    );
    assert_eq!(
        crate::parasolid::entity_54_string_records(&residual)[0].value,
        "abc"
    );
}

#[test]
fn deltas_walks_complete_type_91_records() {
    fn record(escape: bool, xmt: u32, flag: u32) -> Vec<u8> {
        let mut bytes = vec![0, 91];
        if escape {
            bytes.push(0xff);
        }
        bytes.extend(encoded_xmt(xmt));
        bytes.extend_from_slice(&flag.to_be_bytes());
        for (reference, status) in [(3u16, 1u8), (4, 1), (5, 0), (6, 1), (7, 0), (8, 0)] {
            bytes.extend_from_slice(&reference.to_be_bytes());
            bytes.push(status);
        }
        bytes
    }

    let direct = record(false, 10, 0);
    let escaped = record(true, 11, 1);
    let zero_flag_escaped = record(true, 12, 0);
    let escaped_with_null_tail = vec![
        0, 91, 0xff, 1, 89, 0, 0, 0, 0, 0, 202, 1, 1, 88, 1, 1, 90, 1, 1, 41, 1, 0, 1, 1, 0, 1, 1,
    ];
    let mut stream = direct.clone();
    stream.extend_from_slice(&escaped);
    stream.extend_from_slice(&zero_flag_escaped);
    stream.extend_from_slice(&escaped_with_null_tail);
    let record_len = stream.len();
    stream.extend_from_slice(&[0xfe, 0xdc]);

    let census = crate::deltas::walk(&stream);
    assert_eq!(census.records.len(), 4);
    assert_eq!(census.records[0].kind, 91);
    assert_eq!(census.records[0].xmt, 10);
    assert_eq!(census.records[0].node_id, None);
    assert_eq!(census.records[0].references, [3, 4, 5, 6, 7, 8]);
    assert_eq!(census.records[0].canonical_bytes, direct);
    assert_eq!(census.records[1].xmt, 11);
    assert_eq!(census.records[1].canonical_bytes, escaped);
    assert_eq!(census.records[2].canonical_bytes, zero_flag_escaped);
    assert_eq!(census.records[3].canonical_bytes, escaped_with_null_tail);
    assert_eq!(census.full_counts["TYPE_91"], 4);
    assert_eq!(census.bytes_decoded, record_len);

    let residual = crate::deltas::semantic_residual(&stream);
    assert!(residual[..record_len].iter().all(|byte| *byte == 0xff));
    assert_eq!(&residual[record_len..stream.len()], &[0xfe, 0xdc]);
    assert!(residual.ends_with(&stream[..record_len]));

    let mut invalid = direct;
    invalid[4..8].copy_from_slice(&2u32.to_be_bytes());
    assert!(crate::deltas::walk(&invalid).records.is_empty());
    invalid[4..8].copy_from_slice(&0u32.to_be_bytes());
    invalid[10] = 2;
    assert!(crate::deltas::walk(&invalid).records.is_empty());
}

#[test]
fn deltas_walks_complete_group_records() {
    let mut direct = vec![0, 90, 0xff, 0xfe, 0, 1];
    direct.extend_from_slice(&7u32.to_be_bytes());
    for reference in [3u16, 4, 5, 6] {
        direct.extend_from_slice(&reference.to_be_bytes());
        direct.push(1);
    }
    direct.push(4);
    direct.extend_from_slice(&8u16.to_be_bytes());
    direct.push(0);
    let direct_len = direct.len();
    direct.extend_from_slice(&[0xfe, 0xdc]);

    let census = crate::deltas::walk(&direct);
    assert_eq!(census.records.len(), 1);
    assert_eq!(census.records[0].kind, 90);
    assert_eq!(census.records[0].xmt, 32_769);
    assert_eq!(census.records[0].node_id, Some(7));
    assert_eq!(census.records[0].references, [3, 4, 5, 6, 8]);
    assert_eq!(census.records[0].canonical_bytes, direct[..direct_len]);
    assert_eq!(census.full_counts["GROUP"], 1);
    assert_eq!(census.bytes_decoded, direct_len);

    let residual = crate::deltas::semantic_residual(&direct);
    assert!(residual[..direct_len].iter().all(|byte| *byte == 0xff));
    assert_eq!(&residual[direct_len..], &[0xfe, 0xdc]);

    let mut escaped = vec![0, 90, 0xff];
    escaped.extend_from_slice(&10u16.to_be_bytes());
    escaped.extend_from_slice(&11u32.to_be_bytes());
    for reference in [3u16, 4, 5, 6] {
        escaped.extend_from_slice(&reference.to_be_bytes());
        escaped.push(1);
    }
    escaped.push(9);
    escaped.extend_from_slice(&8u16.to_be_bytes());
    escaped.push(1);
    assert_eq!(crate::deltas::walk(&escaped).records[0].xmt, 10);

    escaped[11] = 0;
    assert!(crate::deltas::walk(&escaped).records.is_empty());
    escaped[11] = 1;
    escaped[21] = 3;
    assert!(crate::deltas::walk(&escaped).records.is_empty());
    escaped[21] = 9;
    escaped[24] = 2;
    assert!(crate::deltas::walk(&escaped).records.is_empty());
}

#[test]
fn deltas_walks_complete_attdef_lists() {
    let mut direct = vec![0, 74];
    direct.extend_from_slice(&3u32.to_be_bytes());
    direct.extend_from_slice(&10u16.to_be_bytes());
    direct.extend_from_slice(&2u32.to_be_bytes());
    direct.extend_from_slice(&0u32.to_be_bytes());
    for reference in [1u16, 20, 21, 1] {
        direct.extend_from_slice(&reference.to_be_bytes());
        direct.push(1);
    }
    let direct_len = direct.len();
    direct.extend_from_slice(&[0xfe, 0xdc]);

    let census = crate::deltas::walk(&direct);
    assert_eq!(census.records.len(), 1);
    assert_eq!(census.records[0].kind, 74);
    assert_eq!(census.records[0].xmt, 10);
    assert_eq!(census.records[0].node_id, None);
    assert_eq!(census.records[0].references, [1, 20, 21, 1]);
    assert_eq!(census.records[0].canonical_bytes, direct[..direct_len]);
    assert_eq!(census.full_counts["ATTDEF_LIST"], 1);
    assert_eq!(census.bytes_decoded, direct_len);

    let residual = crate::deltas::semantic_residual(&direct);
    assert!(residual[..direct_len].iter().all(|byte| *byte == 0xff));
    assert_eq!(&residual[direct_len..], &[0xfe, 0xdc]);

    let mut escaped = vec![0, 74, 0xff];
    escaped.extend_from_slice(&2u32.to_be_bytes());
    escaped.extend_from_slice(&11u16.to_be_bytes());
    escaped.extend_from_slice(&1u32.to_be_bytes());
    escaped.extend_from_slice(&0u32.to_be_bytes());
    for reference in [1u16, 30, 1] {
        escaped.extend_from_slice(&reference.to_be_bytes());
        escaped.push(1);
    }
    assert_eq!(crate::deltas::walk(&escaped).records[0].xmt, 11);

    escaped[9..13].copy_from_slice(&3u32.to_be_bytes());
    assert!(crate::deltas::walk(&escaped).records.is_empty());
    escaped[9..13].copy_from_slice(&1u32.to_be_bytes());
    escaped[20..22].copy_from_slice(&1u16.to_be_bytes());
    assert!(crate::deltas::walk(&escaped).records.is_empty());
}

#[test]
fn deltas_walks_complete_type_101_records() {
    let mut direct = vec![0, 101];
    direct.extend_from_slice(&2u16.to_be_bytes());
    for reference in 3u16..15 {
        direct.extend_from_slice(&reference.to_be_bytes());
        direct.push(1);
    }
    direct.push(1);
    direct.extend_from_slice(&[0; 12]);
    for reference in 15u16..18 {
        direct.extend_from_slice(&reference.to_be_bytes());
        direct.push(1);
    }
    let direct_len = direct.len();
    direct.extend_from_slice(&[0xfe, 0xdc]);

    let census = crate::deltas::walk(&direct);
    assert_eq!(census.records.len(), 1);
    assert_eq!(census.records[0].kind, 101);
    assert_eq!(census.records[0].xmt, 2);
    assert_eq!(census.records[0].node_id, None);
    assert_eq!(census.records[0].references, (3u32..18).collect::<Vec<_>>());
    assert_eq!(census.records[0].canonical_bytes, direct[..direct_len]);
    assert_eq!(census.full_counts["TYPE_101"], 1);
    assert_eq!(census.bytes_decoded, direct_len);

    let residual = crate::deltas::semantic_residual(&direct);
    assert!(residual[..direct_len].iter().all(|byte| *byte == 0xff));
    assert_eq!(&residual[direct_len..], &[0xfe, 0xdc]);

    let mut escaped = vec![0, 101, 0xff];
    escaped.extend_from_slice(&2u16.to_be_bytes());
    for reference in 3u16..15 {
        escaped.extend_from_slice(&reference.to_be_bytes());
        escaped.push(1);
    }
    escaped.push(1);
    escaped.extend_from_slice(&[0; 12]);
    for reference in 15u16..18 {
        escaped.extend_from_slice(&reference.to_be_bytes());
        escaped.push(1);
    }
    assert_eq!(crate::deltas::walk(&escaped).records[0].xmt, 2);

    escaped[41] = 0;
    assert!(crate::deltas::walk(&escaped).records.is_empty());
    escaped[41] = 1;
    escaped[42] = 1;
    assert!(crate::deltas::walk(&escaped).records.is_empty());
}

#[test]
fn deltas_walks_auxiliary_family_tombstones() {
    let mut stream = Vec::new();
    for kind in [41u16, 45, 125, 136, 141, 204] {
        stream.extend_from_slice(&kind.to_be_bytes());
        stream.extend_from_slice(&(-2i16).to_be_bytes());
        stream.extend_from_slice(&1u16.to_be_bytes());
    }

    let census = crate::deltas::walk(&stream);
    assert_eq!(census.records.len(), 0);
    assert_eq!(census.tombstones.len(), 6);
    assert!(census
        .tombstones
        .iter()
        .all(|tombstone| tombstone.xmt == 32_769));
    for family in [
        "TERM_USE",
        "TYPE_45",
        "B_SURFACE_DATA",
        "B_CURVE_DESCRIPTOR",
        "TYPE_141",
        "SUPPORT_UV",
    ] {
        assert_eq!(census.tombstone_counts[family], 1);
    }
    assert_eq!(census.bytes_decoded, stream.len());
    assert!(crate::deltas::semantic_residual(&stream)
        .iter()
        .all(|byte| *byte == 0xff));
}

#[test]
fn deltas_term_use_numeric_tails_follow_the_declared_endpoint_count() {
    fn term_use(count: u32, xmt: u16, form: [u8; 2], value_count: usize) -> Vec<u8> {
        let mut bytes = 41u16.to_be_bytes().to_vec();
        bytes.extend_from_slice(&count.to_be_bytes());
        bytes.extend_from_slice(&xmt.to_be_bytes());
        bytes.extend_from_slice(&form);
        for coordinate in [1.0f64, 2.0, 3.0] {
            bytes.extend_from_slice(&coordinate.to_be_bytes());
        }
        for ordinal in 0..value_count {
            bytes.extend_from_slice(&(ordinal as f64 + 0.25).to_be_bytes());
        }
        bytes
    }

    let first = term_use(1, 20, *b"L?", 8);
    let second = term_use(2, 21, *b"TF", 19);
    let mut stream = first.clone();
    stream.extend_from_slice(&second);
    let census = crate::deltas::walk(&stream);

    assert_eq!(census.records.len(), 2);
    assert_eq!(census.term_use_numeric_tails.len(), 2);
    assert_eq!(census.term_use_numeric_tails[0].term_use_xmt, 20);
    assert_eq!(census.term_use_numeric_tails[0].term_use_count, 1);
    assert_eq!(census.term_use_numeric_tails[0].values.len(), 8);
    assert_eq!(census.term_use_numeric_tails[1].term_use_xmt, 21);
    assert_eq!(census.term_use_numeric_tails[1].term_use_count, 2);
    assert_eq!(census.term_use_numeric_tails[1].values.len(), 19);
    assert_eq!(census.bytes_decoded, stream.len());

    let mut nonfinite = term_use(1, 22, *b"L?", 8);
    nonfinite[34..42].copy_from_slice(&f64::NAN.to_be_bytes());
    let census = crate::deltas::walk(&nonfinite);
    assert_eq!(census.records.len(), 1);
    assert!(census.term_use_numeric_tails.is_empty());
    assert_eq!(census.bytes_decoded, 34);
}

#[test]
fn deltas_tagged_reference_lanes_require_complete_known_kind_and_xmt_pairs() {
    let stream = [
        0x00, 0x4f, 0x00, 0x0a, // direct type-79 reference
        0x00, 0x50, 0xff, 0xff, 0x00, 0x01, // extended type-80 reference
    ];
    let census = crate::deltas::walk(&stream);
    assert_eq!(census.tagged_reference_lanes.len(), 1);
    assert_eq!(
        census.tagged_reference_lanes[0].references,
        [(79, 10), (80, 32_768)]
    );
    assert_eq!(census.tagged_reference_lanes[0].offset, 0);
    assert_eq!(census.tagged_reference_lanes[0].end, stream.len());
    assert_eq!(census.bytes_decoded, stream.len());

    for invalid in [
        &[0x00, 0x4e, 0x00, 0x0a][..],
        &[0x00, 0x4f, 0x00, 0x01],
        &[0x00, 0x50, 0xff, 0xff, 0x00],
    ] {
        assert!(crate::deltas::walk(invalid)
            .tagged_reference_lanes
            .is_empty());
    }
}

#[test]
fn deltas_point_normalizes_to_partition_record_framing() {
    let record = crate::deltas::walk(&status_framed_deltas_point_stream())
        .records
        .remove(0);
    let mut expected = crate::tests::record(29, 40);
    put_ref(&mut expected, 2, 50);
    expected[4..8].copy_from_slice(&900u32.to_be_bytes());
    for at in [8, 10, 12, 14] {
        put_ref(&mut expected, at, 1);
    }
    put_vec3(&mut expected, 16, [0.0125, -0.002, 0.004]);
    assert_eq!(record.canonical_bytes, expected);
}

#[test]
fn deltas_intersection_normalizes_before_partition_style_decode() {
    let mut stream = status_framed_deltas_intersection_stream();
    stream[10] = 0;
    let record_len = stream.len();
    stream.extend_from_slice(&[0xfe, 0xdc]);
    let census = crate::deltas::walk(&stream);
    assert_eq!(census.records.len(), 1);
    assert_eq!(census.records[0].kind, 38);
    assert_eq!(census.bytes_decoded, record_len);

    let residual = crate::deltas::semantic_residual(&stream);
    let intersections = crate::topology::composite_curves(&residual);
    assert_eq!(intersections.len(), 1);
    assert_eq!(intersections[0].xmt, 12);
    assert_eq!(intersections[0].references, [6, 7, 20, 21, 22, 23]);
}

#[test]
fn deltas_walks_complete_single_byte_intersection_data_records() {
    let mut stream = crate::topology::TYPE_38_SCHEMA_HEADER.to_vec();
    stream.extend_from_slice(&12u16.to_be_bytes());
    stream.extend_from_slice(&7u32.to_be_bytes());
    for reference in [1u16, 1, 1, 1, 1] {
        stream.extend_from_slice(&reference.to_be_bytes());
        stream.push(1);
    }
    stream.push(b'-');
    for reference in [6u16, 7] {
        stream.extend_from_slice(&reference.to_be_bytes());
        stream.push(1);
    }
    for reference in [15u16, 14, 13] {
        stream.extend_from_slice(&reference.to_be_bytes());
        stream.push(0);
    }
    stream.extend_from_slice(&[0, 1, 1]);
    let schema_end = stream.len();
    stream.extend_from_slice(&[0xa5; 100]);

    let record_offset = stream.len();
    stream.extend_from_slice(&[0x5a]);
    stream.extend_from_slice(&12u16.to_be_bytes());
    stream.extend_from_slice(&7u32.to_be_bytes());
    for reference in [1u16, 2, 3, 4, 5] {
        stream.extend_from_slice(&reference.to_be_bytes());
    }
    stream.push(b'+');
    for reference in [6u16, 6, 1, 1, 1, 1] {
        stream.extend_from_slice(&reference.to_be_bytes());
    }
    let record_end = stream.len();
    stream.extend_from_slice(&[0xfe, 0xdc]);

    let census = crate::deltas::walk(&stream);
    assert_eq!(census.records.len(), 1);
    assert_eq!(census.records[0].kind, 90);
    assert_eq!(
        crate::deltas::record_family_name(&census.records[0]),
        Some("INTERSECTION_DATA")
    );
    assert_eq!(census.records[0].xmt, 12);
    assert_eq!(census.records[0].offset, record_offset);
    assert_eq!(
        census.records[0].references,
        [1, 2, 3, 4, 5, 6, 6, 1, 1, 1, 1]
    );
    assert_eq!(
        census.records[0].canonical_bytes,
        stream[record_offset..record_end]
    );
    assert_eq!(census.full_counts["INTERSECTION_DATA"], 1);
    assert_eq!(
        census.bytes_decoded,
        schema_end + (record_end - record_offset)
    );
    let curves = crate::topology::intersection_data_curves(&stream);
    assert_eq!(curves.len(), 1);
    assert_eq!(curves[0].references, [6, 6, 1, 1, 1, 1]);

    let residual = crate::deltas::semantic_residual(&stream);
    assert!(residual[record_offset..record_end]
        .iter()
        .all(|byte| *byte == 0xff));
    let prefix_len = crate::topology::TYPE_38_SCHEMA_HEADER.len() - 1;
    let appended_start = residual.len() - prefix_len - (record_end - record_offset);
    assert_eq!(
        &residual[appended_start..appended_start + prefix_len],
        &crate::topology::TYPE_38_SCHEMA_HEADER[..prefix_len]
    );
    assert_eq!(
        &residual[appended_start + prefix_len..],
        &stream[record_offset..record_end]
    );
}

#[test]
fn semantic_residual_does_not_reemit_historical_intersection_data() {
    let mut stream = deltas_intersection_curve_stream();
    stream.extend_from_slice(&deltas_body_revision(2));

    let residual = crate::deltas::semantic_residual(&stream);

    assert_eq!(residual.len(), stream.len());
}

#[test]
fn deltas_rejects_single_byte_intersection_data_before_its_schema_anchor() {
    let mut stream = vec![0x5a];
    stream.extend_from_slice(&12u16.to_be_bytes());
    stream.extend_from_slice(&7u32.to_be_bytes());
    for reference in [1u16, 1, 1, 1, 1] {
        stream.extend_from_slice(&reference.to_be_bytes());
    }
    stream.push(b'+');
    for reference in [6u16, 6, 1, 1, 1, 1] {
        stream.extend_from_slice(&reference.to_be_bytes());
    }

    let census = crate::deltas::walk(&stream);
    assert!(census.records.iter().all(|record| record.kind != 90));
    assert!(!census.full_counts.contains_key("INTERSECTION_DATA"));
    assert!(crate::topology::intersection_data_curves(&stream).is_empty());
}

#[test]
fn deltas_rejects_denormal_topology_tolerance_payload_coincidences() {
    fn edge(tolerance: f64) -> Vec<u8> {
        let mut bytes = 16u16.to_be_bytes().to_vec();
        bytes.extend(encoded_xmt(20));
        bytes.extend_from_slice(&1u32.to_be_bytes());
        bytes.extend(encoded_xmt(1));
        bytes.push(1);
        bytes.extend_from_slice(&tolerance.to_be_bytes());
        for reference in [2u32, 3, 4, 5, 6, 7, 8] {
            bytes.extend(encoded_xmt(reference));
            bytes.push(1);
        }
        bytes
    }

    let valid = edge(1.0e-8);
    let census = crate::deltas::walk(&valid);
    assert_eq!(census.records.len(), 1);
    assert_eq!(census.records[0].kind, 16);
    assert_eq!(census.bytes_decoded, valid.len());

    let denormal = edge(1.0e-120);
    assert!(crate::deltas::walk(&denormal).records.is_empty());

    let mut vertex = 18u16.to_be_bytes().to_vec();
    vertex.extend(encoded_xmt(20));
    vertex.extend_from_slice(&1u32.to_be_bytes());
    for reference in [2u32, 3, 4, 5, 6] {
        vertex.extend(encoded_xmt(reference));
        vertex.push(1);
    }
    let tolerance_at = vertex.len();
    vertex.extend_from_slice(&1.0e-8f64.to_be_bytes());
    vertex.extend(encoded_xmt(7));
    vertex.push(1);

    let census = crate::deltas::walk(&vertex);
    assert_eq!(census.records.len(), 1);
    assert_eq!(census.records[0].kind, 18);

    vertex[tolerance_at..tolerance_at + 8].copy_from_slice(&1.0e-120f64.to_be_bytes());
    assert!(crate::deltas::walk(&vertex).records.is_empty());
}

#[test]
fn deltas_rejects_denormal_point_payload_coincidences() {
    let mut point = status_framed_deltas_point_stream();
    let position = point.len() - 24;
    for (ordinal, value) in [f64::from_bits(1), f64::from_bits(2), f64::from_bits(3)]
        .into_iter()
        .enumerate()
    {
        point[position + ordinal * 8..position + (ordinal + 1) * 8]
            .copy_from_slice(&value.to_be_bytes());
    }
    assert!(crate::deltas::walk(&point)
        .records
        .iter()
        .all(|record| record.kind != 29));

    point[position..position + 8].copy_from_slice(&1.0e-200f64.to_be_bytes());
    point[position + 8..].fill(0);
    assert_eq!(crate::deltas::walk(&point).full_counts["POINT"], 1);
}

#[test]
fn deltas_walks_complete_intersection_auxiliary_records() {
    let source = ext11_charted_intersection_curve_stream();
    let blend_source = blend_bound_charted_intersection_curve_stream();
    let chart_pos = crate::intersection::chart_source_records(
        &source,
        crate::intersection::ChartPointLayout::Ext11,
    )[0]
    .pos;
    let (_, chart_end) = crate::intersection::chart_source_record_at(
        &source,
        chart_pos,
        crate::intersection::ChartPointLayout::Ext11,
    )
    .expect("chart");
    let term_pos = crate::intersection::term_use_records(&source)[0].pos;
    let (_, term_end) = crate::intersection::term_use_at(&source, term_pos).expect("term use");
    let support_uv_pos = crate::intersection::support_uv_records(&source)[0].pos;
    let (_, support_uv_end) =
        crate::intersection::support_uv_record_at(&source, support_uv_pos).expect("support UV");
    let blend_bound_pos = crate::intersection::blend_bounds(&blend_source)[0].pos;
    let (_, blend_bound_end) =
        crate::intersection::blend_bound_at(&blend_source, blend_bound_pos).expect("blend bound");

    for (bytes, kind, family) in [
        (&source[chart_pos..chart_end], 40, "CHART"),
        (&source[term_pos..term_end], 41, "TERM_USE"),
        (
            &blend_source[blend_bound_pos..blend_bound_end],
            59,
            "BLEND_BOUND",
        ),
        (&source[support_uv_pos..support_uv_end], 204, "SUPPORT_UV"),
    ] {
        let mut stream = bytes.to_vec();
        stream.extend_from_slice(&[0xfe, 0xdc]);
        let census = crate::deltas::walk(&stream);
        assert_eq!(census.records.len(), 1);
        assert_eq!(census.records[0].kind, kind);
        assert_eq!(census.records[0].canonical_bytes, bytes);
        assert_eq!(census.full_counts[family], 1);
        assert_eq!(census.bytes_decoded, bytes.len());

        let residual = crate::deltas::semantic_residual(&stream);
        assert!(residual[..bytes.len()].iter().all(|byte| *byte == 0xff));
        assert!(residual.ends_with(bytes));
    }
}

#[test]
fn deltas_walks_status_framed_blend_bound_records() {
    fn record(escape: bool, xmt: u32, surface: u32) -> Vec<u8> {
        let mut bytes = 59u16.to_be_bytes().to_vec();
        if escape {
            bytes.push(0xff);
        }
        bytes.extend(encoded_xmt(xmt));
        bytes.extend_from_slice(&17u32.to_be_bytes());
        for (reference, status) in [(1u32, 1u8), (3, 1), (40_001, 0), (1, 1), (40_002, 0)] {
            bytes.extend(encoded_xmt(reference));
            bytes.push(status);
        }
        bytes.push(b'+');
        bytes.extend(encoded_xmt(0));
        bytes.extend(encoded_xmt(surface));
        bytes.push(1);
        bytes
    }

    let direct = record(false, 24, 40_003);
    let escaped = record(true, 40_004, 40_005);
    let mut stream = direct.clone();
    stream.extend_from_slice(&escaped);

    let census = crate::deltas::walk(&stream);

    assert_eq!(census.full_counts["BLEND_BOUND"], 2);
    assert_eq!(census.bytes_decoded, stream.len());
    assert_eq!(census.records[0].canonical_bytes, direct);
    assert_eq!(
        census.records[0].references,
        [1, 3, 40_001, 1, 40_002, 0, 40_003]
    );
    assert_eq!(census.records[1].canonical_bytes, escaped);
    assert_eq!(
        crate::intersection::blend_bounds(&stream)
            .into_iter()
            .map(|record| record.framing)
            .collect::<Vec<_>>(),
        [
            crate::intersection::BlendBoundFraming::DeltasDirect,
            crate::intersection::BlendBoundFraming::DeltasEscaped,
        ]
    );

    let mut invalid_status = record(false, 24, 40_003);
    *invalid_status.last_mut().expect("terminal status") = 0;
    assert!(crate::deltas::walk(&invalid_status).records.is_empty());
}

#[test]
fn deltas_walks_complete_nurbs_auxiliary_records() {
    let source = bspline_partition_stream();
    for (kind, family) in [
        (125u16, "B_SURFACE_DATA"),
        (126, "B_SURFACE_DESCRIPTOR"),
        (127, "MULTIPLICITIES"),
        (128, "KNOTS"),
        (135, "B_CURVE_DATA"),
        (136, "B_CURVE_DESCRIPTOR"),
    ] {
        let (pos, auxiliary) = (0..source.len())
            .find_map(|pos| {
                let auxiliary = crate::nurbs::auxiliary_record_at(&source, pos)?;
                (auxiliary.kind == kind).then_some((pos, auxiliary))
            })
            .expect("complete NURBS auxiliary record");
        let bytes = &source[pos..auxiliary.end];
        let mut stream = bytes.to_vec();
        stream.extend_from_slice(&[0xfe, 0xdc]);

        let census = crate::deltas::walk(&stream);
        assert_eq!(census.records.len(), 1);
        assert_eq!(census.records[0].kind, kind);
        assert_eq!(census.records[0].canonical_bytes, bytes);
        assert_eq!(census.full_counts[family], 1);
        assert_eq!(census.bytes_decoded, bytes.len());

        let residual = crate::deltas::semantic_residual(&stream);
        assert!(residual[..bytes.len()].iter().all(|byte| *byte == 0xff));
        assert!(residual.ends_with(bytes));
    }
}

#[test]
fn deltas_walks_complete_status_framed_surface_descriptors() {
    let mut descriptor = 126u16.to_be_bytes().to_vec();
    descriptor.push(0xff);
    descriptor.extend(encoded_xmt(98));
    descriptor.extend_from_slice(&5u32.to_be_bytes());
    descriptor.extend_from_slice(&3u16.to_be_bytes());
    descriptor.extend_from_slice(&30u32.to_be_bytes());
    descriptor.extend_from_slice(&4u32.to_be_bytes());
    descriptor.extend_from_slice(&[6, 5]);
    descriptor.extend_from_slice(&10u32.to_be_bytes());
    descriptor.extend_from_slice(&2u32.to_be_bytes());
    descriptor.extend_from_slice(&1u32.to_be_bytes());
    descriptor.extend_from_slice(&3u16.to_be_bytes());
    for reference in [106u32, 107, 108, 109, 110] {
        descriptor.extend(encoded_xmt(reference));
        descriptor.push(0);
    }
    let descriptor_len = descriptor.len();
    descriptor.extend_from_slice(&[0xfe, 0xdc]);

    let census = crate::deltas::walk(&descriptor);
    assert_eq!(census.records.len(), 1);
    assert_eq!(census.records[0].kind, 126);
    assert_eq!(census.records[0].xmt, 98);
    assert_eq!(census.records[0].end, descriptor_len);
    assert_eq!(census.full_counts["B_SURFACE_DESCRIPTOR"], 1);
    assert_eq!(census.bytes_decoded, descriptor_len);

    let mut invalid_status = descriptor[..descriptor_len].to_vec();
    *invalid_status.last_mut().expect("final reference status") = 1;
    assert!(crate::deltas::walk(&invalid_status).records.is_empty());
}

#[test]
fn deltas_walks_complete_surface_data_headers() {
    fn record(escape: bool, xmt: u32, marker: u8) -> Vec<u8> {
        let mut bytes = 125u16.to_be_bytes().to_vec();
        if escape {
            bytes.push(0xff);
        }
        bytes.extend(encoded_xmt(xmt));
        for value in [0.0f64, 1.0, -0.25, 0.5, 0.0, 1.0, -0.25, 0.5] {
            bytes.extend_from_slice(&value.to_be_bytes());
        }
        bytes.push(marker);
        bytes.extend(std::iter::repeat_n(b'B', usize::from(marker) * 4));
        bytes.extend(std::iter::repeat_n(b'?', 12 - usize::from(marker) * 4));
        for reference in [1u32, 20, 21, 1] {
            bytes.extend(encoded_xmt(reference));
            bytes.push(1);
        }
        bytes
    }

    let direct = record(false, 20, 1);
    let escaped = record(true, 40_000, 2);
    let mut extended_marker_one = record(false, 21, 1);
    extended_marker_one[73..77].fill(b'B');
    let mut stream = direct.clone();
    stream.extend_from_slice(&escaped);
    stream.extend_from_slice(&extended_marker_one);
    let decoded_len = stream.len();
    stream.extend_from_slice(&[0xfe, 0xdc]);

    let census = crate::deltas::walk(&stream);
    assert_eq!(census.full_counts["B_SURFACE_DATA"], 3);
    assert_eq!(census.bytes_decoded, decoded_len);
    assert_eq!(census.records[0].canonical_bytes, direct);
    assert_eq!(census.records[1].canonical_bytes, escaped);
    assert_eq!(census.records[2].canonical_bytes, extended_marker_one);

    let mut invalid_marker = record(false, 20, 2);
    invalid_marker[68] = 3;
    assert!(crate::deltas::walk(&invalid_marker).records.is_empty());

    let mut invalid_status = record(false, 20, 1);
    *invalid_status.last_mut().expect("final status") = 0;
    assert!(crate::deltas::walk(&invalid_status).records.is_empty());
}

#[test]
fn deltas_walks_complete_curve_data_headers() {
    fn record(escape: bool, xmt: u32, mode: u8, reference: u32) -> Vec<u8> {
        let mut bytes = 135u16.to_be_bytes().to_vec();
        if escape {
            bytes.push(0xff);
        }
        bytes.extend(encoded_xmt(xmt));
        bytes.push(mode);
        bytes.extend(encoded_xmt(reference));
        bytes.push(1);
        bytes
    }

    let direct = record(false, 20, 2, 1);
    let escaped = record(true, 40_000, 1, 21);
    let mut stream = direct.clone();
    stream.extend_from_slice(&escaped);
    let decoded_len = stream.len();
    stream.extend_from_slice(&[0xfe, 0xdc]);

    let census = crate::deltas::walk(&stream);
    assert_eq!(census.full_counts["B_CURVE_DATA"], 2);
    assert_eq!(census.bytes_decoded, decoded_len);
    assert_eq!(census.records[0].canonical_bytes, direct);
    assert_eq!(census.records[1].canonical_bytes, escaped);

    let mut invalid_marker = record(false, 20, 2, 1);
    invalid_marker[4] = 3;
    assert!(crate::deltas::walk(&invalid_marker).records.is_empty());

    let mut invalid_status = record(false, 20, 2, 1);
    *invalid_status.last_mut().expect("final status") = 0;
    assert!(crate::deltas::walk(&invalid_status).records.is_empty());
}

#[test]
fn deltas_walks_complete_type_141_records() {
    fn record(escape: bool, xmt: u32, references: [u32; 4], boundary_statuses: [u8; 2]) -> Vec<u8> {
        let mut bytes = 141u16.to_be_bytes().to_vec();
        if escape {
            bytes.push(0xff);
        }
        bytes.extend(encoded_xmt(xmt));
        for (reference, status) in
            references
                .into_iter()
                .zip([boundary_statuses[0], 0, 0, boundary_statuses[1]])
        {
            bytes.extend(encoded_xmt(reference));
            bytes.push(status);
        }
        bytes
    }

    let direct = record(false, 3158, [646, 3943, 3165, 131], [0, 1]);
    let direct_extended = record(false, 33_000, [646, 3943, 3165, 131], [1, 0]);
    let escaped = record(true, 40_000, [40_001, 1, 0, 40_002], [1, 1]);
    let ambiguous_escaped = record(true, 325, [317, 44, 44, 8], [1, 1]);
    let mut stream = direct.clone();
    stream.extend_from_slice(&direct_extended);
    stream.extend_from_slice(&escaped);
    stream.extend_from_slice(&ambiguous_escaped);
    let decoded_len = stream.len();
    stream.extend_from_slice(&[0xfe, 0xdc]);

    let census = crate::deltas::walk(&stream);
    assert_eq!(census.full_counts["TYPE_141"], 4);
    assert_eq!(census.bytes_decoded, decoded_len);
    assert_eq!(census.records[0].canonical_bytes, direct);
    assert_eq!(census.records[1].canonical_bytes, direct_extended);
    assert_eq!(census.records[1].xmt, 33_000);
    assert_eq!(census.records[2].canonical_bytes, escaped);
    assert_eq!(census.records[2].xmt, 40_000);
    assert_eq!(census.records[2].references, [40_001, 1, 0, 40_002]);
    assert_eq!(census.records[3].canonical_bytes, ambiguous_escaped);
    assert_eq!(census.records[3].xmt, 325);
    assert_eq!(census.records[3].references, [317, 44, 44, 8]);

    let residual = crate::deltas::semantic_residual(&stream);
    assert!(residual[..decoded_len].iter().all(|byte| *byte == 0xff));
    assert!(residual.ends_with(&[direct, direct_extended, escaped, ambiguous_escaped].concat()));
}

#[test]
fn deltas_walks_complete_type_45_records() {
    fn record(escape: bool, xmt: u32, values: &[f64], count_offset: usize) -> Vec<u8> {
        let mut bytes = 45u16.to_be_bytes().to_vec();
        if escape {
            bytes.push(0xff);
        }
        bytes.extend_from_slice(
            &u32::try_from(values.len() - count_offset)
                .expect("test value count")
                .to_be_bytes(),
        );
        bytes.extend(encoded_xmt(xmt));
        for value in values {
            bytes.extend_from_slice(&value.to_be_bytes());
        }
        bytes
    }

    let direct = record(false, 33_000, &[1.0, -2.0, 3.0, 4.0, 5.0], 1);
    let escaped = record(true, 40_000, &[0.0, 0.25, -0.5, 0.75, 1.0], 1);
    let mut stream = direct.clone();
    stream.extend_from_slice(&escaped);
    let decoded_len = stream.len();
    stream.extend_from_slice(&[0xfe, 0xdc]);

    let census = crate::deltas::walk(&stream);
    assert_eq!(census.full_counts["TYPE_45"], 2);
    assert_eq!(census.bytes_decoded, decoded_len);
    assert_eq!(census.records[0].canonical_bytes, direct);
    assert_eq!(census.records[0].xmt, 33_000);
    assert_eq!(census.records[1].canonical_bytes, escaped);
    assert_eq!(census.records[1].xmt, 40_000);

    let residual = crate::deltas::semantic_residual(&stream);
    assert!(residual[..decoded_len].iter().all(|byte| *byte == 0xff));
    assert!(residual.ends_with(&[direct, escaped].concat()));

    let mut counted = record(false, 41_000, &[1.0, 2.0, 3.0], 0);
    let counted_end = counted.len();
    let mut surface_header = 125u16.to_be_bytes().to_vec();
    surface_header.extend(encoded_xmt(42_000));
    for value in [0.0f64, 1.0, 0.0, 1.0, 0.0, 1.0, 0.0, 1.0] {
        surface_header.extend_from_slice(&value.to_be_bytes());
    }
    surface_header.extend_from_slice(&[2, b'B', b'B', b'B', b'B', b'B', b'B', b'B', b'B']);
    surface_header.extend_from_slice(b"????");
    for reference in [1u32, 1, 1, 1] {
        surface_header.extend(encoded_xmt(reference));
        surface_header.push(1);
    }
    counted.extend_from_slice(&surface_header);
    let census = crate::deltas::walk(&counted);
    assert_eq!(
        census
            .records
            .iter()
            .map(|record| (record.kind, record.offset, record.end))
            .collect::<Vec<_>>(),
        [(45, 0, counted_end), (125, counted_end, counted.len())]
    );

    let mut counted = record(false, 41_001, &[1.0, 2.0, 3.0], 0);
    let counted_end = counted.len();
    let mut curve_header = 135u16.to_be_bytes().to_vec();
    curve_header.extend(encoded_xmt(42_001));
    curve_header.push(2);
    curve_header.extend(encoded_xmt(1));
    curve_header.push(1);
    counted.extend_from_slice(&curve_header);
    let census = crate::deltas::walk(&counted);
    assert_eq!(
        census
            .records
            .iter()
            .map(|record| (record.kind, record.offset, record.end))
            .collect::<Vec<_>>(),
        [(45, 0, counted_end), (135, counted_end, counted.len())]
    );

    let mut nonfinite = record(false, 12, &[1.0, 2.0, 3.0, 4.0, f64::NAN], 1);
    nonfinite.extend_from_slice(&[0xfe, 0xdc]);
    assert!(crate::deltas::walk(&nonfinite).records.is_empty());

    let subnormal = record(false, 12, &[1.0, 2.0, f64::from_bits(1)], 0);
    assert!(crate::deltas::walk(&subnormal).records.is_empty());
}

#[test]
fn deltas_walks_complete_type_70_records() {
    fn record(escape: bool, xmt: u32, count: u16, trailing_reference: u32) -> Vec<u8> {
        let mut bytes = 70u16.to_be_bytes().to_vec();
        if escape {
            bytes.push(0xff);
        }
        bytes.extend(encoded_xmt(xmt));
        bytes.extend_from_slice(&0u32.to_be_bytes());
        bytes.push(4);
        for reference in [3u32, 1, 1, 0] {
            bytes.push(1);
            bytes.extend(encoded_xmt(reference));
        }
        bytes.extend_from_slice(&count.to_be_bytes());
        bytes.extend_from_slice(&20u32.to_be_bytes());
        bytes.extend_from_slice(&1u32.to_be_bytes());
        for _ in 0..2 {
            bytes.extend(encoded_xmt(trailing_reference));
            bytes.push(0);
        }
        bytes
    }

    let direct = record(false, 7, 11, 52);
    let escaped = record(true, 40_000, 14, 40_001);
    let mut stream = direct.clone();
    stream.extend_from_slice(&escaped);

    let census = crate::deltas::walk(&stream);

    assert_eq!(census.full_counts["TYPE_70"], 2);
    assert_eq!(census.bytes_decoded, stream.len());
    assert_eq!(census.records[0].canonical_bytes, direct);
    assert_eq!(census.records[0].node_id, Some(0));
    assert_eq!(census.records[0].references, [3, 1, 1, 0, 52, 52]);
    assert_eq!(census.records[1].canonical_bytes, escaped);
    assert_eq!(census.records[1].xmt, 40_000);

    let mut mismatched = record(false, 7, 11, 52);
    let end = mismatched.len();
    mismatched[end - 2] = 53;
    assert!(crate::deltas::walk(&mismatched).records.is_empty());
}

#[test]
fn deltas_offset_surface_normalizes_exact_record_envelope() {
    let stream = deltas_offset_surface_partition_stream();
    let record = crate::deltas::walk(&stream).records.remove(0);
    assert_eq!(record.canonical_bytes.len(), 39);
    assert_eq!(
        crate::topology::offset_surfaces(&record.canonical_bytes)[0].distance,
        4.5
    );

    let mut finite_state = stream.clone();
    let state = finite_state.len() - 8;
    put_f64(&mut finite_state, state, 4.0);
    assert_eq!(crate::deltas::walk(&finite_state).records.len(), 1);
    put_f64(&mut finite_state, state, f64::NAN);
    assert!(crate::deltas::walk(&finite_state).records.is_empty());

    let mut invalid_status = stream.clone();
    let offset = invalid_status
        .windows(4)
        .position(|window| window == [0, 60, 0, 12])
        .expect("OFFSET_SURF record");
    invalid_status[offset + 28] = 2;
    assert!(!crate::deltas::walk(&invalid_status)
        .records
        .iter()
        .any(|record| record.kind == 60));

    let mut truncated = stream;
    truncated.pop();
    assert!(!crate::deltas::walk(&truncated)
        .records
        .iter()
        .any(|record| record.kind == 60));
}

#[test]
fn deltas_procedural_wrappers_normalize_complete_record_envelopes() {
    for (stream, family, kind, byte_len) in [
        (
            deltas_blend_surface_partition_stream(),
            "BLEND_SURF",
            56,
            66,
        ),
        (
            deltas_trimmed_curve_partition_stream(),
            "TRIMMED_CURVE",
            133,
            85,
        ),
        (deltas_surface_curve_partition_stream(), "SP_CURVE", 137, 33),
    ] {
        let census = crate::deltas::walk(&stream);
        assert_eq!(census.full_counts.get(family), Some(&1));
        let record = census
            .records
            .iter()
            .find(|record| record.kind == kind)
            .expect("procedural wrapper");
        assert_eq!(record.canonical_bytes.len(), byte_len);
        assert!(crate::topology::Graph::parse(&record.canonical_bytes)
            .get(kind as u8, 12)
            .is_some());
    }

    let mut invalid_blend = deltas_blend_surface_partition_stream();
    let blend = invalid_blend
        .windows(4)
        .position(|window| window == [0, 56, 0, 12])
        .expect("BLEND_SURF record");
    invalid_blend[blend + 24] = b'X';
    assert!(!crate::deltas::walk(&invalid_blend)
        .records
        .iter()
        .any(|record| record.kind == 56));
}

#[test]
fn deltas_fixed_record_boundary_accepts_known_auxiliary_tag() {
    let mut stream = deltas_bspline_curve_wrapper_stream();
    let wrapper_len = stream.len();
    stream.extend_from_slice(&[0, 141, 0xfe]);

    let census = crate::deltas::walk(&stream);
    let wrapper = census
        .records
        .iter()
        .find(|record| record.kind == 134)
        .expect("B_CURVE wrapper");
    assert_eq!(wrapper.end, wrapper_len);
    assert_eq!(wrapper.canonical_bytes.len(), 23);
}

#[test]
fn deltas_fixed_records_accept_direct_extended_and_escaped_envelopes() {
    fn fin(escape: bool, xmt: u32) -> (Vec<u8>, Vec<u8>) {
        let mut source = 17u16.to_be_bytes().to_vec();
        let mut canonical = source.clone();
        if escape {
            source.push(0xff);
            canonical.push(0xff);
        }
        let encoded_identity = encoded_xmt(xmt);
        source.extend_from_slice(&encoded_identity);
        canonical.extend_from_slice(&encoded_identity);
        for reference in 20..29 {
            let encoded_reference = encoded_xmt(reference);
            source.extend_from_slice(&encoded_reference);
            source.push(1);
            canonical.extend_from_slice(&encoded_reference);
        }
        source.push(b'+');
        canonical.push(b'+');
        (source, canonical)
    }

    let (direct_extended, direct_canonical) = fin(false, 32_768);
    let (escaped, escaped_canonical) = fin(true, 40);
    let mut stream = direct_extended.clone();
    stream.extend_from_slice(&escaped);
    let mut escaped_point = vec![0, 29, 0xff, 0, 41];
    escaped_point.extend_from_slice(&42u32.to_be_bytes());
    for reference in 43..47 {
        escaped_point.extend(encoded_xmt(reference));
        escaped_point.push(1);
    }
    for coordinate in [1.0f64, 2.0, 3.0] {
        escaped_point.extend_from_slice(&coordinate.to_be_bytes());
    }
    stream.extend_from_slice(&escaped_point);
    let decoded_len = stream.len();
    stream.extend_from_slice(&[0xfe, 0xdc]);

    let census = crate::deltas::walk(&stream);
    assert_eq!(census.full_counts["FIN"], 2);
    assert_eq!(census.full_counts["POINT"], 1);
    assert_eq!(census.bytes_decoded, decoded_len);
    assert_eq!(census.records[0].xmt, 32_768);
    assert_eq!(census.records[0].canonical_bytes, direct_canonical);
    assert_eq!(census.records[1].xmt, 40);
    assert_eq!(census.records[1].canonical_bytes, escaped_canonical);
    assert_eq!(census.records[2].xmt, 41);
    assert_eq!(census.records[2].node_id, Some(42));
    assert_eq!(census.records[2].position, Some([1.0, 2.0, 3.0]));
}

#[test]
fn merged_deltas_full_record_replaces_partition_node() {
    let partition = topology_partition_stream();
    let mut deltas = status_framed_deltas_point_stream();
    deltas[2..4].copy_from_slice(&11u16.to_be_bytes());
    let merged = crate::deltas::merge_full_records(&partition, &deltas);
    let points = crate::geometry::points(&merged);
    assert_eq!(points.len(), 1);
    assert_eq!(points[0].position.x, 12.5);
    assert_eq!(points[0].position.y, -2.0);
    assert_eq!(points[0].position.z, 4.0);
    assert!(crate::topology::Graph::parse(&merged).get(29, 11).is_some());
}

#[test]
fn merged_tombstone_preserves_a_topology_referenced_carrier() {
    let partition = topology_partition_stream();
    let mut tombstone = Vec::new();
    tombstone.extend_from_slice(&29u16.to_be_bytes());
    tombstone.extend_from_slice(&11u16.to_be_bytes());
    tombstone.extend_from_slice(&[0, 1]);
    let census = crate::deltas::walk(&tombstone);
    assert_eq!(census.tombstones.len(), 1);
    assert_eq!(census.tombstones[0].kind, 29);
    assert_eq!(census.tombstones[0].xmt, 11);
    let merged = crate::deltas::merge_full_records(&partition, &tombstone);
    assert!(crate::topology::Graph::parse(&merged).get(29, 11).is_some());
    assert_eq!(crate::geometry::points(&merged)[0].position.x, 10.0);
}

#[test]
fn merged_exact_key_tombstone_removes_unreferenced_partition_node() {
    let mut partition = record(29, 40);
    put_ref(&mut partition, 2, 11);
    put_vec3(&mut partition, 16, [0.01, 0.02, 0.03]);
    let tombstone = [0, 29, 0, 11, 0, 1];
    let merged = crate::deltas::merge_full_records(&partition, &tombstone);
    assert!(crate::topology::Graph::parse(&merged).get(29, 11).is_none());
}

#[test]
fn merged_deltas_uses_last_full_or_tombstone_event() {
    let partition = topology_partition_stream();
    let tombstone = [0, 29, 0, 11, 0, 1];
    let mut full = status_framed_deltas_point_stream();
    full[2..4].copy_from_slice(&11u16.to_be_bytes());

    let mut delete_then_replace = tombstone.to_vec();
    delete_then_replace.extend_from_slice(&full);
    let merged = crate::deltas::merge_full_records(&partition, &delete_then_replace);
    assert_eq!(crate::geometry::points(&merged)[0].position.x, 12.5);

    let mut replace_then_delete = full;
    replace_then_delete.extend_from_slice(&tombstone);
    let merged = crate::deltas::merge_full_records(&partition, &replace_then_delete);
    assert_eq!(crate::geometry::points(&merged)[0].position.x, 10.0);
}

fn deltas_body_revision(node_id: u32) -> Vec<u8> {
    let mut revision = Vec::with_capacity(32);
    revision.extend_from_slice(&12u16.to_be_bytes());
    revision.extend_from_slice(&3u16.to_be_bytes());
    revision.extend_from_slice(&node_id.to_be_bytes());
    for _ in 0..8 {
        revision.extend_from_slice(&0u16.to_be_bytes());
        revision.push(1);
    }
    revision
}

fn deltas_point(xmt: u16, x: f64) -> Vec<u8> {
    let mut point = status_framed_deltas_point_stream();
    point[2..4].copy_from_slice(&xmt.to_be_bytes());
    point[20..28].copy_from_slice(&x.to_be_bytes());
    point
}

#[test]
fn final_body_revision_scopes_deltas_overlay_events() {
    let mut partition = record(29, 40);
    put_ref(&mut partition, 2, 11);
    put_vec3(&mut partition, 16, [0.01, 0.02, 0.03]);
    let known_tombstone = [0, 29, 0, 11, 0, 1];

    let mut historical_delete = deltas_body_revision(1);
    historical_delete.extend_from_slice(&known_tombstone);
    historical_delete.extend_from_slice(&deltas_body_revision(2));
    let merged = crate::deltas::merge_full_records(&partition, &historical_delete);
    assert!(crate::topology::Graph::parse(&merged).get(29, 11).is_some());

    let mut current_delete = historical_delete;
    current_delete.extend_from_slice(&known_tombstone);
    let merged = crate::deltas::merge_full_records(&partition, &current_delete);
    assert!(crate::topology::Graph::parse(&merged).get(29, 11).is_none());
}

#[test]
fn body_revision_scopes_keep_each_monotonic_sequence_current() {
    let mut deltas = deltas_body_revision(1);
    deltas.extend(deltas_point(50, 0.001));
    deltas.extend(deltas_body_revision(2));
    deltas.extend(deltas_point(50, 0.002));
    deltas.extend(deltas_body_revision(1));
    deltas.extend(deltas_point(51, 0.003));
    deltas.extend(deltas_body_revision(2));
    deltas.extend(deltas_point(51, 0.004));

    let merged = crate::deltas::merge_full_records(&[], &deltas);
    let graph = crate::topology::Graph::parse(&merged);
    assert!(graph.get(29, 50).is_some());
    assert!(graph.get(29, 51).is_some());
    let points = crate::geometry::points(&merged);
    assert!(points
        .iter()
        .any(|point| (point.position.x - 2.0).abs() <= 1e-12));
    assert!(points
        .iter()
        .any(|point| (point.position.x - 4.0).abs() <= 1e-12));
    assert!(!points
        .iter()
        .any(|point| (point.position.x - 1.0).abs() <= 1e-12));
    assert!(!points
        .iter()
        .any(|point| (point.position.x - 3.0).abs() <= 1e-12));
}

#[test]
fn body_revision_scopes_accept_reverse_serialized_counter_direction() {
    let mut deltas = deltas_body_revision(4);
    deltas.extend(deltas_point(50, 0.001));
    deltas.extend(deltas_body_revision(3));
    deltas.extend(deltas_point(50, 0.002));
    deltas.extend(deltas_body_revision(4));
    deltas.extend(deltas_point(51, 0.003));
    deltas.extend(deltas_body_revision(3));
    deltas.extend(deltas_point(51, 0.004));

    let merged = crate::deltas::merge_full_records(&[], &deltas);
    let graph = crate::topology::Graph::parse(&merged);
    assert!(graph.get(29, 50).is_some());
    assert!(graph.get(29, 51).is_some());
    let points = crate::geometry::points(&merged);
    assert!(points
        .iter()
        .any(|point| (point.position.x - 2.0).abs() <= 1e-12));
    assert!(points
        .iter()
        .any(|point| (point.position.x - 4.0).abs() <= 1e-12));
    assert!(!points
        .iter()
        .any(|point| (point.position.x - 1.0).abs() <= 1e-12));
    assert!(!points
        .iter()
        .any(|point| (point.position.x - 3.0).abs() <= 1e-12));
}

#[test]
fn unmatched_tombstones_are_scoped_to_the_final_body_revision() {
    let partition = topology_partition_stream();
    let unknown_tombstone = [0, 29, 0, 99, 0, 1];
    let mut historical_delete = deltas_body_revision(1);
    historical_delete.extend_from_slice(&unknown_tombstone);
    historical_delete.extend_from_slice(&deltas_body_revision(2));
    assert_eq!(
        crate::deltas::unmatched_terminal_tombstones(&partition, &historical_delete),
        0
    );

    historical_delete.extend_from_slice(&unknown_tombstone);
    assert_eq!(
        crate::deltas::unmatched_terminal_tombstones(&partition, &historical_delete),
        1
    );
}

#[test]
fn unmatched_tombstones_are_scoped_per_body_revision_sequence() {
    let partition = topology_partition_stream();
    let historical_first = [0, 29, 0, 98, 0, 1];
    let historical_second = [0, 29, 0, 99, 0, 1];
    let mut deltas = deltas_body_revision(1);
    deltas.extend_from_slice(&historical_first);
    deltas.extend_from_slice(&deltas_body_revision(2));
    deltas.extend_from_slice(&historical_first);
    deltas.extend_from_slice(&deltas_body_revision(1));
    deltas.extend_from_slice(&historical_second);
    deltas.extend_from_slice(&deltas_body_revision(2));

    assert_eq!(
        crate::deltas::unmatched_terminal_tombstones(&partition, &deltas),
        1
    );
    assert_eq!(
        crate::deltas::unmatched_terminal_tombstones_by_family(&partition, &deltas).get("POINT"),
        Some(&1)
    );
}

#[test]
fn semantic_residual_masks_historical_body_revisions() {
    let mut deltas = deltas_body_revision(1);
    let historical_len = deltas.len();
    deltas.extend_from_slice(&[0, 38, 0xaa, 0xbb, 0xcc]);
    deltas.extend_from_slice(&deltas_body_revision(2));
    deltas.extend_from_slice(&[0, 38, 0x11, 0x22, 0x33]);

    let residual = crate::deltas::semantic_residual(&deltas);
    assert!(residual[..historical_len + 5]
        .iter()
        .all(|byte| *byte == 0xff));
    assert!(residual.ends_with(&[0, 38, 0x11, 0x22, 0x33]));
}

#[test]
fn semantic_residual_masks_historical_interleaved_body_sequences() {
    let mut first_historical = status_framed_deltas_intersection_stream();
    first_historical[4..8].copy_from_slice(&1u32.to_be_bytes());
    let mut first_current = status_framed_deltas_intersection_stream();
    first_current[4..8].copy_from_slice(&2u32.to_be_bytes());
    let mut second_historical = status_framed_deltas_intersection_stream();
    second_historical[2..4].copy_from_slice(&13u16.to_be_bytes());
    second_historical[4..8].copy_from_slice(&3u32.to_be_bytes());
    let mut second_current = second_historical.clone();
    second_current[4..8].copy_from_slice(&4u32.to_be_bytes());

    let mut deltas = deltas_body_revision(1);
    deltas.extend_from_slice(&first_historical);
    deltas.extend_from_slice(&deltas_body_revision(2));
    deltas.extend_from_slice(&first_current);
    deltas.extend_from_slice(&deltas_body_revision(1));
    deltas.extend_from_slice(&second_historical);
    deltas.extend_from_slice(&deltas_body_revision(2));
    deltas.extend_from_slice(&second_current);

    let census = crate::deltas::walk(&deltas);
    let first_current_offset = census.body_revisions[1].offset;
    let second_sequence_offset = census.body_revisions[2].offset;
    let second_current_offset = census.body_revisions[3].offset;
    let residual = crate::deltas::semantic_residual(&deltas);
    assert!(residual[..first_current_offset]
        .iter()
        .all(|byte| *byte == 0xff));
    assert!(residual[second_sequence_offset..second_current_offset]
        .iter()
        .all(|byte| *byte == 0xff));
    let mut expected = crate::deltas::walk(&first_current).records[0]
        .canonical_bytes
        .clone();
    expected.extend_from_slice(&crate::deltas::walk(&second_current).records[0].canonical_bytes);
    assert!(residual.ends_with(&expected));
    assert!(!residual
        .windows(first_historical.len())
        .any(|window| window == first_historical));
    assert!(!residual
        .windows(second_historical.len())
        .any(|window| window == second_historical));
}

#[test]
fn unmatched_delta_tombstones_follow_exact_last_event_identity() {
    let partition = topology_partition_stream();
    let known = [0, 29, 0, 11, 0, 1];
    let unknown = [0, 29, 0, 99, 0, 1];
    assert_eq!(
        crate::deltas::unmatched_terminal_tombstones(&partition, &known),
        0
    );
    assert_eq!(
        crate::deltas::unmatched_terminal_tombstones(&partition, &unknown),
        1
    );
    assert_eq!(
        crate::deltas::unmatched_terminal_tombstones_by_family(&partition, &unknown).get("POINT"),
        Some(&1)
    );

    let mut full = status_framed_deltas_point_stream();
    full[2..4].copy_from_slice(&99u16.to_be_bytes());
    let mut add_then_delete = full.clone();
    add_then_delete.extend_from_slice(&unknown);
    assert_eq!(
        crate::deltas::unmatched_terminal_tombstones(&partition, &add_then_delete),
        0
    );

    let mut delete_then_add = unknown.to_vec();
    delete_then_add.extend_from_slice(&full);
    assert_eq!(
        crate::deltas::unmatched_terminal_tombstones(&partition, &delete_then_add),
        0
    );
}

#[test]
fn deltas_tombstone_decodes_compact_and_extended_xmt_identities() {
    let compact = [0, 29, 0, 11, 0, 1];
    let extended = [0, 29, 0xe3, 0xbf, 0, 1];

    assert_eq!(crate::deltas::walk(&compact).tombstones[0].xmt, 11);
    assert_eq!(crate::deltas::walk(&extended).tombstones[0].xmt, 40_000);
}

#[test]
fn deltas_tombstone_is_self_delimiting_before_opaque_bytes() {
    let mut stream = vec![0, 29, 0, 11, 0, 1];
    stream.extend_from_slice(&[0xfe, 0xdc]);

    let census = crate::deltas::walk(&stream);
    assert_eq!(census.tombstones.len(), 1);
    assert_eq!(census.tombstones[0].xmt, 11);
    assert_eq!(census.bytes_decoded, 6);
    assert_eq!(
        crate::deltas::semantic_residual(&stream),
        vec![0xff; 6]
            .into_iter()
            .chain([0xfe, 0xdc])
            .collect::<Vec<_>>()
    );
}

#[test]
fn deltas_body_revision_retains_prefix_identities_and_bounded_state_tail() {
    let mut bytes = vec![0, 12, 3, 0x10];
    bytes.extend_from_slice(&223u32.to_be_bytes());
    bytes.extend_from_slice(&[0xe3, 0xbf, 0, 1, 1]);
    for reference in [6u16, 1, 1, 1, 1, 1, 1] {
        bytes.extend_from_slice(&reference.to_be_bytes());
        bytes.push(1);
    }
    bytes.extend_from_slice(&[0x40, 0x8f, 0x40, 0, 0, 0, 0, 0]);

    let census = crate::deltas::walk(&bytes);

    assert!(census.records.is_empty());
    assert_eq!(census.body_revisions.len(), 1);
    assert_eq!(census.body_revisions[0].xmt, 784);
    assert_eq!(census.body_revisions[0].node_id, 223);
    assert_eq!(
        census.body_revisions[0].references,
        [40_000, 6, 1, 1, 1, 1, 1, 1]
    );
    assert_eq!(census.body_revisions[0].prefix_end, 34);
    assert_eq!(
        &bytes[census.body_revisions[0].prefix_end..census.body_revisions[0].end],
        [0x40, 0x8f, 0x40, 0, 0, 0, 0, 0]
    );
    assert_eq!(census.body_revisions[0].end, bytes.len());
    assert_eq!(census.bytes_decoded, bytes.len());
}

#[test]
fn deltas_reference_state_packets_decode_compact_and_extended_references() {
    let mut packet = vec![0, 1, 0, 1, 0, 4];
    packet.extend_from_slice(&2u16.to_be_bytes());
    packet.extend_from_slice(&3u16.to_be_bytes());
    packet.extend_from_slice(&[0xe3, 0xbf, 0, 1]);
    packet.extend_from_slice(&1u16.to_be_bytes());
    packet.extend_from_slice(&1u16.to_be_bytes());
    for word in [34u32, 6, 11, 22_362, 1] {
        packet.extend_from_slice(&word.to_be_bytes());
    }
    packet.push(65);

    let census = crate::deltas::walk(&packet);

    assert_eq!(census.reference_state_packets.len(), 1);
    assert_eq!(
        census.reference_state_packets[0].frames,
        [crate::deltas::ReferenceStateFrame {
            references: [2, 3, 40_000, 1],
            state_words: [34, 6, 11, 22_362, 1],
            state_byte: 65,
        }]
    );
    assert!(!census.reference_state_packets[0].terminal);
    assert_eq!(census.reference_state_packets[0].offset, 0);
    assert_eq!(census.reference_state_packets[0].end, packet.len());
    assert_eq!(census.bytes_decoded, packet.len());

    let truncated = packet[..packet.len() - 1].to_vec();
    let null_required_reference = [&packet[..6], &[0, 1], &packet[8..]].concat();
    let trailing_byte = [packet.as_slice(), &[0]].concat();
    for malformed in [&truncated, &null_required_reference] {
        assert!(crate::deltas::walk(malformed)
            .reference_state_packets
            .is_empty());
    }
    let trailing_census = crate::deltas::walk(&trailing_byte);
    assert_eq!(trailing_census.reference_state_packets.len(), 1);
    assert_eq!(trailing_census.reference_state_packets[0].end, packet.len());
    assert_eq!(trailing_census.bytes_decoded, packet.len());

    let mut compound = vec![0, 1, 0, 1];
    for (references, words, state_byte) in [
        ([7u16, 1, 8, 1], [0u32; 5], 1),
        ([8, 7, 9, 1], [0, 0, 0, 17, 0], 2),
    ] {
        compound.extend_from_slice(&4u16.to_be_bytes());
        for reference in references {
            compound.extend_from_slice(&reference.to_be_bytes());
        }
        compound.extend_from_slice(&1u16.to_be_bytes());
        for word in words {
            compound.extend_from_slice(&word.to_be_bytes());
        }
        compound.push(state_byte);
    }
    for _ in 0..3 {
        compound.extend_from_slice(&1u16.to_be_bytes());
    }
    compound.extend_from_slice(&1u32.to_be_bytes());

    let compound_census = crate::deltas::walk(&compound);
    assert_eq!(compound_census.reference_state_packets.len(), 1);
    assert_eq!(compound_census.reference_state_packets[0].frames.len(), 2);
    assert!(compound_census.reference_state_packets[0].terminal);
    assert_eq!(
        compound_census.reference_state_packets[0].end,
        compound.len()
    );
    assert_eq!(compound_census.bytes_decoded, compound.len());
}

#[test]
fn deltas_reference_marker_packets_decode_extended_references_atomically() {
    let packet = [
        0xe3, 0xbf, 0x00, 0x01, 0x01, // extended reference 40_000, status
        0x00, 0x01, 0x01, // null reference, status
        0x56, // marker
        0x00, 0x01, 0x01, // null reference, status
    ];

    let census = crate::deltas::walk(&packet);

    assert_eq!(census.reference_marker_packets.len(), 1);
    assert_eq!(census.reference_marker_packets[0].reference, 40_000);
    assert_eq!(census.reference_marker_packets[0].marker, 0x56);
    assert_eq!(census.reference_marker_packets[0].offset, 0);
    assert_eq!(census.reference_marker_packets[0].end, packet.len());
    assert_eq!(census.bytes_decoded, packet.len());

    let truncated = packet[..packet.len() - 1].to_vec();
    let trailing_byte = [packet.as_slice(), &[0]].concat();
    let unknown_marker = [
        0xe3, 0xbf, 0x00, 0x01, 0x01, 0x00, 0x01, 0x01, 0x55, 0x00, 0x01, 0x01,
    ]
    .to_vec();
    for malformed in [&truncated, &trailing_byte, &unknown_marker] {
        assert!(crate::deltas::walk(malformed)
            .reference_marker_packets
            .is_empty());
    }
}

#[test]
fn deltas_region_schema_declaration_exposes_a_following_marker_packet() {
    let mut bytes = vec![
        0x00, 0x13, 0x09, 0x43, 0x43, 0x43, 0x43, 0x43, 0x43, 0x49, 0x05, 0x66, 0x72, 0x61, 0x6d,
        0x65, 0x00, 0xe6, 0x00, 0x01, 0x43, 0x41, 0x05, 0x6f, 0x77, 0x6e, 0x65, 0x72, 0x00, 0x0c,
        0x00, 0x01, 0x5a,
    ];
    bytes.extend_from_slice(&[0xe3, 0xbf, 0, 1]);
    bytes.extend_from_slice(&5u32.to_be_bytes());
    for reference in [1u16, 3, 1, 9] {
        bytes.extend_from_slice(&reference.to_be_bytes());
        bytes.push(1);
    }
    let declaration_end = bytes.len();
    bytes.extend([0, 7, 1, 0, 1, 1, 0x56, 0, 1, 1]);

    let census = crate::deltas::walk(&bytes);

    assert_eq!(census.inline_schema_declarations.len(), 1);
    let declaration = &census.inline_schema_declarations[0];
    assert_eq!(
        declaration.fields,
        crate::deltas::InlineSchemaFields::Region {
            xmt: 40_000,
            state_word: 5,
            references: [1, 3, 1, 9],
        }
    );
    assert_eq!(declaration.offset, 0);
    assert_eq!(declaration.end, declaration_end);
    assert_eq!(census.reference_marker_packets.len(), 1);
    assert_eq!(census.reference_marker_packets[0].offset, declaration_end);
    assert_eq!(census.reference_marker_packets[0].reference, 7);
    assert_eq!(census.bytes_decoded, bytes.len());

    let mut truncated = bytes[..declaration_end - 1].to_vec();
    truncated.extend([0, 7, 1, 0, 1, 1, 0x56, 0, 1, 1]);
    assert!(crate::deltas::walk(&truncated)
        .inline_schema_declarations
        .is_empty());
}

#[test]
fn deltas_body_revision_does_not_absorb_an_adjacent_tagged_reference_lane() {
    let mut bytes = vec![0, 12, 0, 3];
    bytes.extend_from_slice(&223u32.to_be_bytes());
    for reference in [2u16, 3, 4, 5, 6, 7, 8, 9] {
        bytes.extend_from_slice(&reference.to_be_bytes());
        bytes.push(1);
    }
    let lane_offset = bytes.len();
    bytes.extend_from_slice(&29u16.to_be_bytes());
    bytes.extend_from_slice(&10u16.to_be_bytes());

    let census = crate::deltas::walk(&bytes);

    assert_eq!(census.body_revisions.len(), 1);
    assert_eq!(
        census.body_revisions[0].prefix_end,
        census.body_revisions[0].end
    );
    assert_eq!(census.body_revisions[0].end, lane_offset);
    assert_eq!(census.tagged_reference_lanes.len(), 1);
    assert_eq!(census.tagged_reference_lanes[0].offset, lane_offset);
    assert_eq!(census.tagged_reference_lanes[0].references, [(29, 10)]);
    assert_eq!(census.bytes_decoded, bytes.len());
}

#[test]
fn decode_emits_point_added_by_deltas_stream() {
    let mut cur = Cursor::new(prt_with_partition(&deltas_point_partition_stream()));
    let result = NxCodec.decode(&mut cur, &DecodeOptions::default()).unwrap();
    assert_eq!(result.ir.model.points.len(), 1);
    assert_eq!(result.ir.model.points[0].position.x, 12.5);
    assert_eq!(result.ir.model.points[0].position.y, -2.0);
    assert_eq!(result.ir.model.points[0].position.z, 4.0);
}

#[test]
fn decode_replaces_partition_point_with_same_xmt_deltas_point() {
    let partition = topology_partition_stream();
    let mut deltas = deltas_point_partition_stream();
    let record = deltas
        .windows(2)
        .rposition(|window| window == 29u16.to_be_bytes())
        .expect("deltas POINT");
    deltas[record + 2..record + 4].copy_from_slice(&11u16.to_be_bytes());
    let mut cur = Cursor::new(prt_with_streams(&[&partition, &deltas]));
    let result = NxCodec.decode(&mut cur, &DecodeOptions::default()).unwrap();
    assert_eq!(result.ir.model.points.len(), 1);
    assert_eq!(result.ir.model.points[0].position.x, 12.5);
    assert_eq!(result.ir.model.points[0].position.y, -2.0);
    assert_eq!(result.ir.model.points[0].position.z, 4.0);
}

#[test]
fn decode_preserves_partition_edge_topology_over_deltas_history() {
    let partition = topology_partition_stream();
    let deltas = deltas_edge_partition_stream();
    let mut cur = Cursor::new(prt_with_streams(&[&partition, &deltas]));
    let result = NxCodec.decode(&mut cur, &DecodeOptions::default()).unwrap();
    assert_eq!(result.ir.model.edges.len(), 1);
    assert_eq!(result.ir.model.edges[0].tolerance, Some(0.3));
    assert_eq!(
        result.ir.model.edges[0].curve.as_ref(),
        Some(&result.ir.model.curves[0].id)
    );
    assert!(cadmpeg_ir::validate::validate(&result.ir, Vec::new()).is_ok());
}

#[test]
fn decode_preserves_partition_face_and_vertex_topology_over_deltas_history() {
    let partition = topology_partition_stream();
    let deltas = deltas_face_vertex_partition_stream();
    let mut cur = Cursor::new(prt_with_streams(&[&partition, &deltas]));
    let result = NxCodec.decode(&mut cur, &DecodeOptions::default()).unwrap();
    assert_eq!(result.ir.model.faces.len(), 1);
    assert_eq!(result.ir.model.faces[0].tolerance, Some(0.2));
    assert_eq!(result.ir.model.vertices.len(), 1);
    assert_eq!(result.ir.model.vertices[0].tolerance, Some(0.1));
    assert!(cadmpeg_ir::validate::validate(&result.ir, Vec::new()).is_ok());
}

#[test]
fn decode_preserves_partition_loop_topology_over_deltas_history() {
    let partition = topology_partition_stream();
    let deltas = deltas_loop_partition_stream();
    let merged = crate::deltas::merge_full_records(&partition, &deltas);
    assert_eq!(
        crate::topology::Graph::parse(&merged)
            .get(15, 5)
            .and_then(|node| node.u32_at(4)),
        Some(0)
    );
    let mut cur = Cursor::new(prt_with_streams(&[&partition, &deltas]));
    let result = NxCodec.decode(&mut cur, &DecodeOptions::default()).unwrap();
    assert_eq!(result.ir.model.loops.len(), 1);
    assert_eq!(result.ir.model.coedges.len(), 1);
    assert!(cadmpeg_ir::validate::validate(&result.ir, Vec::new()).is_ok());
}

#[test]
fn decode_preserves_partition_shell_topology_over_deltas_history() {
    let partition = topology_partition_stream();
    let deltas = deltas_shell_partition_stream();
    let merged = crate::deltas::merge_full_records(&partition, &deltas);
    assert_eq!(
        crate::topology::Graph::parse(&merged)
            .get(13, 3)
            .and_then(|node| node.u32_at(4)),
        Some(0)
    );
    let mut cur = Cursor::new(prt_with_streams(&[&partition, &deltas]));
    let result = NxCodec.decode(&mut cur, &DecodeOptions::default()).unwrap();
    assert_eq!(result.ir.model.shells.len(), 1);
    assert_eq!(result.ir.model.faces.len(), 1);
    assert!(cadmpeg_ir::validate::validate(&result.ir, Vec::new()).is_ok());
}

#[test]
fn decode_preserves_partition_fin_topology_over_deltas_history() {
    let partition = topology_partition_stream();
    let deltas = deltas_fin_partition_stream();
    let file = prt_with_streams(&[&partition, &deltas]);
    let mut cur = Cursor::new(file);
    let result = NxCodec.decode(&mut cur, &DecodeOptions::default()).unwrap();

    assert_eq!(result.ir.model.coedges.len(), 1);
    assert_eq!(
        result.ir.model.coedges[0].sense,
        cadmpeg_ir::topology::Sense::Forward
    );
    assert!(cadmpeg_ir::validate::validate(&result.ir, Vec::new()).is_ok());
}

#[test]
fn decode_replaces_partition_line_from_status_framed_deltas() {
    let partition = topology_partition_stream();
    let deltas = deltas_line_partition_stream();
    let file = prt_with_streams(&[&partition, &deltas]);
    let mut cur = Cursor::new(file);
    let result = NxCodec.decode(&mut cur, &DecodeOptions::default()).unwrap();

    let CurveGeometry::Line { origin, direction } = result.ir.model.curves[0].geometry else {
        panic!("line");
    };
    assert_eq!(origin, cadmpeg_ir::math::Point3::new(4.0, 5.0, 6.0));
    assert_eq!(direction, Vector3::new(0.0, 1.0, 0.0));
    assert!(cadmpeg_ir::validate::validate(&result.ir, Vec::new()).is_ok());
}

#[test]
fn decode_replaces_partition_plane_from_status_framed_deltas() {
    let partition = topology_partition_stream();
    let deltas = deltas_plane_partition_stream();
    let file = prt_with_streams(&[&partition, &deltas]);
    let mut cur = Cursor::new(file);
    let result = NxCodec.decode(&mut cur, &DecodeOptions::default()).unwrap();

    assert!(matches!(
        result.ir.model.surfaces[0].geometry,
        SurfaceGeometry::Plane { origin, normal, u_axis }
            if origin == cadmpeg_ir::math::Point3::new(1.0, 2.0, 3.0)
                && normal == Vector3::new(0.0, 1.0, 0.0)
                && u_axis == Vector3::new(1.0, 0.0, 0.0)
    ));
    assert_eq!(
        result.ir.model.faces[0].surface,
        result.ir.model.surfaces[0].id
    );
    assert!(cadmpeg_ir::validate::validate(&result.ir, Vec::new()).is_ok());
}

#[test]
fn decode_replaces_partition_offset_surface_from_status_framed_deltas() {
    let partition = offset_surface_topology_partition_stream();
    let deltas = deltas_offset_surface_partition_stream();
    let census = crate::deltas::walk(&deltas);
    assert_eq!(census.full_counts.get("OFFSET_SURF"), Some(&1));
    let merged = crate::deltas::merge_full_records(&partition, &deltas);
    assert_eq!(
        crate::topology::offset_surfaces(&merged)
            .iter()
            .map(|surface| surface.distance)
            .collect::<Vec<_>>(),
        [4.5]
    );
    let file = prt_with_streams(&[&partition, &deltas]);
    let mut cur = Cursor::new(file);
    let result = NxCodec.decode(&mut cur, &DecodeOptions::default()).unwrap();

    let [procedural] = result.ir.model.procedural_surfaces.as_slice() else {
        panic!("one offset surface");
    };
    let ProceduralSurfaceDefinition::Offset { distance, .. } = procedural.definition else {
        panic!("offset surface");
    };
    assert_eq!(distance, 4.5);
    assert_eq!(result.ir.model.faces[0].surface, procedural.surface);
    assert!(cadmpeg_ir::validate::validate(&result.ir, Vec::new()).is_ok());
}

#[test]
fn decode_replaces_partition_blend_surface_from_status_framed_deltas() {
    let partition = blend_surface_topology_partition_stream();
    let deltas = deltas_blend_surface_partition_stream();
    let result = NxCodec
        .decode(
            &mut Cursor::new(prt_with_streams(&[&partition, &deltas])),
            &DecodeOptions::default(),
        )
        .unwrap();

    let ProceduralSurfaceDefinition::Blend { radius, .. } =
        &result.ir.model.procedural_surfaces[0].definition
    else {
        panic!("blend surface");
    };
    assert_eq!(
        *radius,
        BlendRadiusLaw::Constant {
            signed_radius: -4.0
        }
    );
    assert_eq!(
        result.ir.model.faces[0].surface,
        result.ir.model.procedural_surfaces[0].surface
    );
    assert!(cadmpeg_ir::validate::validate(&result.ir, Vec::new()).is_ok());
}

#[test]
fn decode_replaces_partition_trimmed_curve_from_status_framed_deltas() {
    let partition = trimmed_topology_partition_stream();
    let deltas = deltas_trimmed_curve_partition_stream();
    let merged = crate::deltas::merge_full_records(&partition, &deltas);
    assert_eq!(
        crate::topology::trimmed_curves(&merged)[0].parameters,
        [0.000_3, 0.000_7]
    );
    let result = NxCodec
        .decode(
            &mut Cursor::new(prt_with_streams(&[&partition, &deltas])),
            &DecodeOptions::default(),
        )
        .unwrap();

    assert_eq!(result.ir.model.edges[0].param_range, Some([0.3, 0.7]));
    assert!(cadmpeg_ir::validate::validate(&result.ir, Vec::new()).is_ok());
}

#[test]
fn decode_replaces_partition_surface_curve_from_status_framed_deltas() {
    let partition = surface_curve_topology_partition_stream();
    let deltas = deltas_surface_curve_partition_stream();
    let merged = crate::deltas::merge_full_records(&partition, &deltas);
    assert_eq!(
        crate::topology::surface_curves(&merged)[0].tolerance,
        0.000_02
    );
    let result = NxCodec
        .decode(
            &mut Cursor::new(prt_with_streams(&[&partition, &deltas])),
            &DecodeOptions::default(),
        )
        .unwrap();

    assert_eq!(
        result.ir.model.edges[0].curve.as_ref(),
        Some(&result.ir.model.curves[0].id)
    );
    assert!(cadmpeg_ir::validate::validate(&result.ir, Vec::new()).is_ok());
}

#[test]
fn decode_replaces_partition_circle_from_status_framed_deltas() {
    let partition = circle_topology_partition_stream();
    let deltas = deltas_circle_partition_stream();
    let file = prt_with_streams(&[&partition, &deltas]);
    let mut cur = Cursor::new(file);
    let result = NxCodec.decode(&mut cur, &DecodeOptions::default()).unwrap();

    assert!(result.ir.model.curves.iter().any(|curve| matches!(
        curve.geometry,
        CurveGeometry::Circle { center, axis, ref_direction, radius }
            if center == cadmpeg_ir::math::Point3::new(1.0, 2.0, 3.0)
                && axis == Vector3::new(0.0, 1.0, 0.0)
                && ref_direction == Vector3::new(1.0, 0.0, 0.0)
                && radius == 25.0
    )));
    assert!(cadmpeg_ir::validate::validate(&result.ir, Vec::new()).is_ok());
}

#[test]
fn decode_replaces_partition_ellipse_from_status_framed_deltas() {
    let partition = ellipse_topology_partition_stream();
    let deltas = deltas_ellipse_partition_stream();
    let file = prt_with_streams(&[&partition, &deltas]);
    let mut cur = Cursor::new(file);
    let result = NxCodec.decode(&mut cur, &DecodeOptions::default()).unwrap();

    assert!(result.ir.model.curves.iter().any(|curve| matches!(
        curve.geometry,
        CurveGeometry::Ellipse {
            center,
            axis,
            major_direction,
            major_radius,
            minor_radius,
        } if center == cadmpeg_ir::math::Point3::new(1.0, 2.0, 3.0)
            && axis == Vector3::new(0.0, 1.0, 0.0)
            && major_direction == Vector3::new(1.0, 0.0, 0.0)
            && major_radius == 30.0
            && minor_radius == 12.0
    )));
    assert!(cadmpeg_ir::validate::validate(&result.ir, Vec::new()).is_ok());
}

#[test]
fn decode_replaces_partition_cylinder_from_status_framed_deltas() {
    let partition = cylinder_topology_partition_stream();
    let deltas = deltas_cylinder_partition_stream();
    let file = prt_with_streams(&[&partition, &deltas]);
    let mut cur = Cursor::new(file);
    let result = NxCodec.decode(&mut cur, &DecodeOptions::default()).unwrap();

    assert!(result.ir.model.surfaces.iter().any(|surface| matches!(
        surface.geometry,
        SurfaceGeometry::Cylinder { origin, axis, ref_direction, radius }
            if origin == cadmpeg_ir::math::Point3::new(1.0, 2.0, 3.0)
                && axis == Vector3::new(0.0, 1.0, 0.0)
                && ref_direction == Vector3::new(1.0, 0.0, 0.0)
                && radius == 25.0
    )));
    assert!(cadmpeg_ir::validate::validate(&result.ir, Vec::new()).is_ok());
}

#[test]
fn decode_replaces_partition_cone_from_status_framed_deltas() {
    let partition = cone_topology_partition_stream();
    let deltas = deltas_cone_partition_stream();
    let file = prt_with_streams(&[&partition, &deltas]);
    let mut cur = Cursor::new(file);
    let result = NxCodec.decode(&mut cur, &DecodeOptions::default()).unwrap();

    assert!(result.ir.model.surfaces.iter().any(|surface| matches!(
        surface.geometry,
        SurfaceGeometry::Cone { origin, axis, ref_direction, radius, ratio, half_angle }
            if origin == cadmpeg_ir::math::Point3::new(1.0, 2.0, 3.0)
                && axis == Vector3::new(0.0, 1.0, 0.0)
                && ref_direction == Vector3::new(1.0, 0.0, 0.0)
                && radius == 25.0
                && ratio == 1.0
                && (half_angle - std::f64::consts::FRAC_PI_6).abs() < 1e-12
    )));
    assert!(cadmpeg_ir::validate::validate(&result.ir, Vec::new()).is_ok());
}

#[test]
fn decode_replaces_partition_sphere_from_status_framed_deltas() {
    let partition = sphere_topology_partition_stream();
    let deltas = deltas_sphere_partition_stream();
    let file = prt_with_streams(&[&partition, &deltas]);
    let mut cur = Cursor::new(file);
    let result = NxCodec.decode(&mut cur, &DecodeOptions::default()).unwrap();

    assert!(result.ir.model.surfaces.iter().any(|surface| matches!(
        surface.geometry,
        SurfaceGeometry::Sphere { center, axis, ref_direction, radius }
            if center == cadmpeg_ir::math::Point3::new(1.0, 2.0, 3.0)
                && axis == Vector3::new(0.0, 1.0, 0.0)
                && ref_direction == Vector3::new(1.0, 0.0, 0.0)
                && radius == 25.0
    )));
    assert!(cadmpeg_ir::validate::validate(&result.ir, Vec::new()).is_ok());
}

#[test]
fn decode_replaces_partition_torus_from_status_framed_deltas() {
    let partition = torus_topology_partition_stream();
    let deltas = deltas_torus_partition_stream();
    let file = prt_with_streams(&[&partition, &deltas]);
    let mut cur = Cursor::new(file);
    let result = NxCodec.decode(&mut cur, &DecodeOptions::default()).unwrap();

    assert!(result.ir.model.surfaces.iter().any(|surface| matches!(
        surface.geometry,
        SurfaceGeometry::Torus {
            center,
            axis,
            ref_direction,
            major_radius,
            minor_radius,
        } if center == cadmpeg_ir::math::Point3::new(1.0, 2.0, 3.0)
            && axis == Vector3::new(0.0, 1.0, 0.0)
            && ref_direction == Vector3::new(1.0, 0.0, 0.0)
            && major_radius == 40.0
            && minor_radius == 15.0
    )));
    assert!(cadmpeg_ir::validate::validate(&result.ir, Vec::new()).is_ok());
}

#[test]
fn intersection_pcurve_attachment_requires_face_incidence() {
    let ir = cadmpeg_ir::examples::unit_cube();
    let edge = cadmpeg_ir::ids::EdgeId("synthetic:cube:edge#0".into());
    let surface = ir
        .model
        .coedges
        .iter()
        .find(|coedge| coedge.edge == edge && coedge.id.0.contains("bottom"))
        .and_then(|coedge| {
            let loop_ = ir
                .model
                .loops
                .iter()
                .find(|loop_| loop_.id == coedge.owner_loop)?;
            ir.model
                .faces
                .iter()
                .find(|face| face.id == loop_.face)
                .map(|face| face.surface.clone())
        })
        .expect("bottom support surface");
    let pcurve = |end| PcurveGeometry::Nurbs {
        degree: 1,
        knots: vec![0.0, 0.0, 1.0, 1.0],
        control_points: vec![Point2::new(0.0, 0.0), end],
        weights: None,
        periodic: false,
    };

    assert!(crate::decode::pcurve_matches_edge(
        &ir,
        &edge,
        &surface,
        &pcurve(Point2::new(10.0, 0.0)),
        None,
    ));
    assert!(!crate::decode::pcurve_matches_edge(
        &ir,
        &edge,
        &surface,
        &pcurve(Point2::new(10.0, 5.0)),
        None,
    ));
}

#[test]
fn decode_derives_analytic_support_uv_without_serialized_values() {
    let stream = charted_intersection_without_uv_stream();
    let mut cur = Cursor::new(prt_with_partition(&stream));
    let result = NxCodec.decode(&mut cur, &DecodeOptions::default()).unwrap();

    let carrier = result
        .ir
        .model
        .curves
        .iter()
        .find(|curve| curve.id == result.ir.model.procedural_curves[0].curve)
        .expect("intersection carrier");
    assert!(matches!(carrier.geometry, CurveGeometry::Nurbs(_)));
    let cadmpeg_ir::geometry::ProceduralCurveDefinition::Intersection { context, .. } =
        &result.ir.model.procedural_curves[0].definition
    else {
        panic!("intersection definition");
    };
    assert!(context.sides[0].pcurve.is_some());
    assert!(cadmpeg_ir::validate::validate(&result.ir, Vec::new()).is_ok());
}

#[test]
fn decode_accepts_intersection_terms_within_chart_tolerance() {
    let stream = charted_intersection_with_approximated_term_stream();
    let mut cur = Cursor::new(prt_with_partition(&stream));
    let result = NxCodec.decode(&mut cur, &DecodeOptions::default()).unwrap();

    let carrier = result
        .ir
        .model
        .curves
        .iter()
        .find(|curve| curve.id == result.ir.model.procedural_curves[0].curve)
        .expect("intersection carrier");
    assert!(matches!(carrier.geometry, CurveGeometry::Nurbs(_)));
    assert!(cadmpeg_ir::validate::validate(&result.ir, Vec::new()).is_ok());
}

#[test]
fn decode_emits_ext11_deltas_intersection_chart() {
    let stream = ext11_charted_intersection_curve_stream();
    let partition = charted_intersection_curve_topology_partition_stream();
    let mut cur = Cursor::new(prt_with_ext11_intersection(&partition, &stream));
    let result = NxCodec.decode(&mut cur, &DecodeOptions::default()).unwrap();

    let curve_id = &result.ir.model.procedural_curves[0].curve;
    let curve = result
        .ir
        .model
        .curves
        .iter()
        .find(|curve| &curve.id == curve_id)
        .expect("intersection cache");
    let CurveGeometry::Nurbs(nurbs) = &curve.geometry else {
        panic!("NURBS chart cache");
    };
    assert_eq!(nurbs.control_points[1].x, 10.0);
    assert_eq!(nurbs.knots, vec![2.0, 2.0, 5.0, 5.0]);
}

#[test]
fn decode_assigns_ext11_uv_lanes_by_unique_surface_evaluation() {
    let stream = two_support_ext11_charted_intersection_curve_stream(false);
    let partition =
        two_support_charted_intersection_curve_stream_with_second_plane_axis([0.0, 0.0, 1.0]);
    let mut cur = Cursor::new(prt_with_ext11_intersection(&partition, &stream));
    let result = NxCodec.decode(&mut cur, &DecodeOptions::default()).unwrap();

    let cadmpeg_ir::geometry::ProceduralCurveDefinition::Intersection { context, .. } =
        &result.ir.model.procedural_curves[0].definition
    else {
        panic!("typed intersection");
    };
    let [Some(PcurveGeometry::Nurbs {
        control_points: first,
        ..
    }), Some(PcurveGeometry::Nurbs {
        control_points: second,
        ..
    })] = context.sides.clone().map(|side| side.pcurve)
    else {
        panic!("two ext11 pcurves");
    };
    assert_eq!(first, [Point2::new(0.0, 0.0), Point2::new(10.0, 0.0)]);
    assert_eq!(second, [Point2::new(0.0, 0.0), Point2::new(0.0, 10.0)]);
    assert!(cadmpeg_ir::validate::validate(&result.ir, Vec::new()).is_ok());
}

#[test]
fn ext11_uv_assignment_eliminates_the_complementary_support_lane() {
    let stream = two_support_ext11_charted_intersection_curve_stream(false);
    let partition =
        two_support_charted_intersection_curve_stream_with_second_plane_axis([0.0, 0.0, 1.0]);
    let mut cur = Cursor::new(prt_with_ext11_intersection(&partition, &stream));
    let mut result = NxCodec.decode(&mut cur, &DecodeOptions::default()).unwrap();
    let surfaces = [
        result.ir.model.surfaces[0].id.clone(),
        result.ir.model.surfaces[1].id.clone(),
    ];
    result.ir.model.surfaces[1].geometry = SurfaceGeometry::Unknown { record: None };
    let lanes = [
        Some(vec![[0.0, 0.0], [0.01, 0.0]]),
        Some(vec![[0.0, 0.0], [0.0, 0.01]]),
    ];

    let assigned = crate::decode::assign_ext11_support_uv_to_surfaces(
        &result.ir,
        [&surfaces[0], &surfaces[1]],
        &[
            cadmpeg_ir::math::Point3::new(0.0, 0.0, 0.0),
            cadmpeg_ir::math::Point3::new(10.0, 0.0, 0.0),
        ],
        0.01,
        &lanes,
    )
    .unwrap();

    assert_eq!(assigned, [lanes[0].clone(), None]);
}

#[test]
fn topology_selects_one_candidate_at_an_ambiguous_record_offset() {
    let mut stream = vec![0; 26];
    stream[..7].copy_from_slice(&[0, 12, 0xff, 0xfe, 0x00, 0x02, 0x01]);
    let mut successor = record(12, 24);
    put_ref(&mut successor, 2, 3);
    stream.extend_from_slice(&successor);
    let graph = crate::topology::Graph::parse(&stream);
    assert_eq!(graph.of_kind(12).count(), 2);
    assert_eq!(graph.at_pos(0).map(|node| node.xmt), Some(65_536));
    assert_eq!(graph.at_pos(26).map(|node| node.xmt), Some(3));
}

#[test]
fn topology_disambiguates_direct_large_index_from_escaped_compact_record() {
    let mut stream = vec![0; 25];
    stream[..6].copy_from_slice(&[0, 17, 0xff, 0x7f, 0x00, 0x01]);
    for index in 0..8 {
        put_ref(&mut stream, 6 + index * 2, 2);
    }
    stream[22..24].copy_from_slice(b"++");
    stream[24] = b'+';

    let mut successor = record(17, 23);
    put_ref(&mut successor, 2, 7);
    for index in 0..9 {
        put_ref(&mut successor, 4 + index * 2, 2);
    }
    successor[22] = b'+';
    stream.extend_from_slice(&successor);

    let graph = crate::topology::Graph::parse(&stream);
    assert_eq!(graph.at_pos(0).map(|node| node.xmt), Some(32_896));
    assert_eq!(graph.at_pos(0).map(crate::topology::Node::end), Some(25));
    assert_eq!(graph.at_pos(25).map(|node| node.xmt), Some(7));

    let mut ambiguous = stream[..25].to_vec();
    ambiguous.extend_from_slice(&[0; 5]);
    assert!(crate::topology::Graph::parse(&ambiguous)
        .at_pos(0)
        .is_none());
}

#[test]
fn topology_rejects_duplicate_fixed_record_identity() {
    let mut first = record(29, 40);
    put_ref(&mut first, 2, 11);
    put_vec3(&mut first, 16, [0.01, 0.02, 0.03]);
    let mut duplicate = record(29, 40);
    put_ref(&mut duplicate, 2, 11);
    put_vec3(&mut duplicate, 16, [0.04, 0.05, 0.06]);
    first.extend(duplicate);

    let graph = crate::topology::Graph::parse(&first);
    assert!(graph.get(29, 11).is_none());
    assert!(graph.of_kind(29).next().is_none());
}

#[test]
fn trimmed_curves_reject_nonfinite_endpoint_witnesses() {
    let mut stream = trimmed_topology_partition_stream();
    let trim = stream
        .windows(4)
        .position(|window| window == [0, 133, 0, 12])
        .expect("trimmed curve");
    put_f64(&mut stream, trim + 21, f64::NAN);
    assert!(crate::topology::trimmed_curves(&stream).is_empty());

    put_f64(&mut stream, trim + 21, f64::MAX);
    assert!(crate::topology::trimmed_curves(&stream).is_empty());
}

#[test]
fn nurbs_carriers_reject_nonfinite_millimeter_control_points() {
    let mut surface = bspline_partition_stream();
    let payload = surface
        .windows(4)
        .position(|window| window == [0, 125, 0, 21])
        .expect("surface payload");
    put_f64(&mut surface, payload + 97, f64::MAX);
    assert!(crate::nurbs::surfaces(&surface).is_empty());

    let mut curve = bspline_partition_stream();
    let payload = curve
        .windows(4)
        .position(|window| window == [0, 135, 0, 41])
        .expect("curve payload");
    put_f64(&mut curve, payload + 15, f64::MAX);
    assert!(crate::nurbs::curves(&curve).is_empty());

    let descriptor = curve
        .windows(4)
        .position(|window| window == [0, 136, 0, 40])
        .expect("curve descriptor");
    put_ref(&mut curve, descriptor + 10, 2);
    put_f64(&mut curve, payload + 15, f64::MAX);
    put_f64(&mut curve, payload + 31, f64::MIN_POSITIVE);
    assert!(crate::nurbs::pcurves(&curve).is_empty());
}

#[test]
fn nurbs_periodicity_uses_logical_flags_not_knot_types() {
    let mut surface = bspline_partition_stream();
    let surface_descriptor = surface
        .windows(4)
        .position(|window| window == [0, 126, 0, 20])
        .expect("surface descriptor");
    surface[surface_descriptor + 4] = 1;
    surface[surface_descriptor + 5] = 0;
    surface[surface_descriptor + 18] = 2;
    surface[surface_descriptor + 19] = 3;
    let [surface] = crate::nurbs::surfaces(&surface)
        .try_into()
        .expect("one surface");
    let SurfaceGeometry::Nurbs(surface) = surface.geometry else {
        panic!("expected NURBS surface");
    };
    assert!(surface.u_periodic);
    assert!(!surface.v_periodic);

    let mut open_surface = bspline_partition_stream();
    let surface_descriptor = open_surface
        .windows(4)
        .position(|window| window == [0, 126, 0, 20])
        .expect("surface descriptor");
    open_surface[surface_descriptor + 4] = 0;
    open_surface[surface_descriptor + 18] = 6;
    let [open_surface] = crate::nurbs::surfaces(&open_surface)
        .try_into()
        .expect("one surface");
    let SurfaceGeometry::Nurbs(open_surface) = open_surface.geometry else {
        panic!("expected NURBS surface");
    };
    assert!(!open_surface.u_periodic);

    let mut curve = bspline_partition_stream();
    let curve_descriptor = curve
        .windows(4)
        .position(|window| window == [0, 136, 0, 40])
        .expect("curve descriptor");
    curve[curve_descriptor + 16] = 2;
    curve[curve_descriptor + 17] = 1;
    let [curve] = crate::nurbs::curves(&curve).try_into().expect("one curve");
    let CurveGeometry::Nurbs(curve) = curve.geometry else {
        panic!("expected NURBS curve");
    };
    assert!(curve.periodic);

    let mut open_curve = bspline_partition_stream();
    let curve_descriptor = open_curve
        .windows(4)
        .position(|window| window == [0, 136, 0, 40])
        .expect("curve descriptor");
    open_curve[curve_descriptor + 16] = 6;
    open_curve[curve_descriptor + 17] = 0;
    let [open_curve] = crate::nurbs::curves(&open_curve)
        .try_into()
        .expect("one curve");
    let CurveGeometry::Nurbs(open_curve) = open_curve.geometry else {
        panic!("expected NURBS curve");
    };
    assert!(!open_curve.periodic);

    let mut pcurve = bspline_partition_stream();
    let pcurve_descriptor = pcurve
        .windows(4)
        .position(|window| window == [0, 136, 0, 40])
        .expect("pcurve descriptor");
    put_ref(&mut pcurve, pcurve_descriptor + 10, 2);
    pcurve[pcurve_descriptor + 16] = 6;
    pcurve[pcurve_descriptor + 17] = 0;
    let payload = pcurve
        .windows(4)
        .position(|window| window == [0, 135, 0, 41])
        .expect("pcurve payload");
    for (index, value) in [0.0, 0.0, 1.0, 0.02, 0.0, 1.0].into_iter().enumerate() {
        put_f64(&mut pcurve, payload + 15 + index * 8, value);
    }
    let [pcurve] = crate::nurbs::pcurves(&pcurve)
        .try_into()
        .expect("one pcurve");
    let PcurveGeometry::Nurbs { periodic, .. } = pcurve.geometry else {
        panic!("expected NURBS pcurve");
    };
    assert!(!periodic);
}

#[test]
fn nurbs_accepts_encoded_cardinality_without_arbitrary_ceiling() {
    fn curve_stream(degree: u16, poles: u16) -> Vec<u8> {
        assert!(poles > degree);
        let distinct = usize::from(poles) + 1;
        let mut stream = Vec::new();

        let mut wrapper = record(134, 23);
        put_ref(&mut wrapper, 2, 50);
        wrapper[18] = b'+';
        put_ref(&mut wrapper, 19, 40);
        put_ref(&mut wrapper, 21, 41);
        stream.extend(wrapper);

        let mut descriptor = record(136, 27);
        put_ref(&mut descriptor, 2, 40);
        put_ref(&mut descriptor, 4, degree);
        put_ref(&mut descriptor, 8, poles);
        put_ref(&mut descriptor, 10, 3);
        put_ref(&mut descriptor, 14, distinct as u16);
        descriptor[16] = 2;
        descriptor[20] = 2;
        put_ref(&mut descriptor, 23, 42);
        put_ref(&mut descriptor, 25, 43);
        stream.extend(descriptor);

        let value_count = usize::from(poles) * 3;
        let mut payload = record(135, 15 + value_count * 8);
        put_ref(&mut payload, 2, 41);
        payload[9..13].copy_from_slice(&(value_count as u32).to_be_bytes());
        put_ref(&mut payload, 13, 1);
        for pole in 0..usize::from(poles) {
            let at = 15 + pole * 24;
            put_f64(&mut payload, at, pole as f64 * 0.01);
            put_f64(&mut payload, at + 8, 0.0);
            put_f64(&mut payload, at + 16, 0.0);
        }
        stream.extend(payload);

        let mut multiplicities = record(127, 8 + distinct * 2);
        multiplicities[4..6].copy_from_slice(&(distinct as u16).to_be_bytes());
        put_ref(&mut multiplicities, 6, 42);
        put_ref(&mut multiplicities, 8, degree + 1);
        for index in 1..distinct {
            put_ref(&mut multiplicities, 8 + index * 2, 1);
        }
        stream.extend(multiplicities);

        let mut knots = record(128, 8 + distinct * 8);
        knots[4..6].copy_from_slice(&(distinct as u16).to_be_bytes());
        put_ref(&mut knots, 6, 43);
        for index in 0..distinct {
            put_f64(&mut knots, 8 + index * 8, index as f64);
        }
        stream.extend(knots);
        stream
    }

    fn surface_stream(u_degree: u16, u_poles: u16, v_degree: u16, v_poles: u16) -> Vec<u8> {
        assert!(u_poles > u_degree && v_poles > v_degree);
        let u_distinct = usize::from(u_poles) + 1;
        let v_distinct = usize::from(v_poles) + 1;
        let poles = usize::from(u_poles) * usize::from(v_poles);
        let mut stream = Vec::new();

        let mut wrapper = record(124, 23);
        put_ref(&mut wrapper, 2, 10);
        wrapper[18] = b'+';
        put_ref(&mut wrapper, 19, 20);
        put_ref(&mut wrapper, 21, 21);
        stream.extend(wrapper);

        let mut descriptor = record(126, 48);
        put_ref(&mut descriptor, 2, 20);
        put_ref(&mut descriptor, 6, u_degree);
        put_ref(&mut descriptor, 8, v_degree);
        put_ref(&mut descriptor, 12, u_poles);
        put_ref(&mut descriptor, 16, v_poles);
        descriptor[18] = 2;
        descriptor[19] = 2;
        descriptor[20..24].copy_from_slice(&(u_distinct as u32).to_be_bytes());
        descriptor[24..28].copy_from_slice(&(v_distinct as u32).to_be_bytes());
        put_ref(&mut descriptor, 36, 30);
        put_ref(&mut descriptor, 38, 31);
        put_ref(&mut descriptor, 40, 32);
        put_ref(&mut descriptor, 42, 33);
        put_ref(&mut descriptor, 44, 125);
        put_ref(&mut descriptor, 46, 21);
        stream.extend(descriptor);

        let value_count = poles * 3;
        let mut payload = record(125, 97 + value_count * 8);
        put_ref(&mut payload, 2, 21);
        payload[90] = b'+';
        payload[91..95].copy_from_slice(&(value_count as u32).to_be_bytes());
        put_ref(&mut payload, 95, 1);
        for v in 0..usize::from(v_poles) {
            for u in 0..usize::from(u_poles) {
                let at = 97 + (v * usize::from(u_poles) + u) * 24;
                put_f64(&mut payload, at, u as f64 * 0.001);
                put_f64(&mut payload, at + 8, v as f64 * 0.001);
                put_f64(&mut payload, at + 16, 0.0);
            }
        }
        stream.extend(payload);

        for (reference, degree, distinct) in
            [(30, u_degree, u_distinct), (31, v_degree, v_distinct)]
        {
            let mut multiplicities = record(127, 8 + distinct * 2);
            multiplicities[4..6].copy_from_slice(&(distinct as u16).to_be_bytes());
            put_ref(&mut multiplicities, 6, reference);
            put_ref(&mut multiplicities, 8, degree + 1);
            for index in 1..distinct {
                put_ref(&mut multiplicities, 8 + index * 2, 1);
            }
            stream.extend(multiplicities);
        }
        for (reference, distinct) in [(32, u_distinct), (33, v_distinct)] {
            let mut knots = record(128, 8 + distinct * 8);
            knots[4..6].copy_from_slice(&(distinct as u16).to_be_bytes());
            put_ref(&mut knots, 6, reference);
            for index in 0..distinct {
                put_f64(&mut knots, 8 + index * 8, index as f64);
            }
            stream.extend(knots);
        }
        stream
    }

    let [high_degree] = crate::nurbs::curves(&curve_stream(11, 12))
        .try_into()
        .expect("one high-degree curve");
    let CurveGeometry::Nurbs(high_degree) = high_degree.geometry else {
        panic!("expected high-degree NURBS curve");
    };
    assert_eq!(high_degree.degree, 11);
    assert_eq!(high_degree.control_points.len(), 12);
    assert_eq!(high_degree.knots.len(), 24);

    let [wide_curve] = crate::nurbs::curves(&curve_stream(1, 5000))
        .try_into()
        .expect("one wide curve");
    let CurveGeometry::Nurbs(wide_curve) = wide_curve.geometry else {
        panic!("expected wide NURBS curve");
    };
    assert_eq!(wide_curve.control_points.len(), 5000);
    assert_eq!(wide_curve.knots.len(), 5002);

    let [wide_surface] = crate::nurbs::surfaces(&surface_stream(1, 2001, 1, 2))
        .try_into()
        .expect("one wide surface");
    let SurfaceGeometry::Nurbs(wide_surface) = wide_surface.geometry else {
        panic!("expected wide NURBS surface");
    };
    assert_eq!(wide_surface.control_points.len(), 4002);
    assert_eq!(wide_surface.u_knots.len(), 2003);
    assert_eq!(wide_surface.v_knots.len(), 4);

    let mut wide_curve_pole_count = curve_stream(1, 12);
    let curve_descriptor = wide_curve_pole_count
        .windows(4)
        .position(|window| window == [0, 136, 0, 40])
        .expect("curve descriptor");
    wide_curve_pole_count[curve_descriptor + 6] = 1;
    assert!(crate::nurbs::curves(&wide_curve_pole_count).is_empty());

    let mut wide_curve_distinct_count = curve_stream(1, 12);
    wide_curve_distinct_count[curve_descriptor + 12] = 1;
    assert!(crate::nurbs::curves(&wide_curve_distinct_count).is_empty());

    let mut wide_surface_pole_count = surface_stream(1, 2, 1, 2);
    let surface_descriptor = wide_surface_pole_count
        .windows(4)
        .position(|window| window == [0, 126, 0, 20])
        .expect("surface descriptor");
    wide_surface_pole_count[surface_descriptor + 10] = 1;
    assert!(crate::nurbs::surfaces(&wide_surface_pole_count).is_empty());

    let mut wide_surface_distinct_count = surface_stream(1, 2, 1, 2);
    wide_surface_distinct_count[surface_descriptor + 20] = 1;
    assert!(crate::nurbs::surfaces(&wide_surface_distinct_count).is_empty());
}

#[test]
fn nurbs_carriers_reject_invalid_basis_cardinality() {
    let mut surface = bspline_partition_stream();
    let descriptor = surface
        .windows(4)
        .position(|window| window == [0, 126, 0, 20])
        .expect("surface descriptor");
    put_ref(&mut surface, descriptor + 6, 2);
    assert!(crate::nurbs::surfaces(&surface).is_empty());

    let mut curve = bspline_partition_stream();
    let descriptor = curve
        .windows(4)
        .position(|window| window == [0, 136, 0, 40])
        .expect("curve descriptor");
    put_ref(&mut curve, descriptor + 4, 2);
    assert!(crate::nurbs::curves(&curve).is_empty());

    put_ref(&mut curve, descriptor + 10, 2);
    assert!(crate::nurbs::pcurves(&curve).is_empty());

    let mut short_knots = bspline_partition_stream();
    let multiplicities = short_knots
        .windows(12)
        .position(|record| record[..2] == [0, 127] && record[6..8] == 42u16.to_be_bytes())
        .expect("curve multiplicities");
    put_ref(&mut short_knots, multiplicities + 10, 1);
    assert!(crate::nurbs::curves(&short_knots).is_empty());
}

#[test]
fn nurbs_surface_rejects_mismatched_descriptor_payload_reference() {
    let mut stream = bspline_partition_stream();
    let descriptor = stream
        .windows(4)
        .position(|window| window == [0, 126, 0, 20])
        .expect("surface descriptor");
    put_ref(&mut stream, descriptor + 46, 22);
    assert!(crate::nurbs::surfaces(&stream).is_empty());
}

#[test]
fn nurbs_carriers_reject_duplicate_support_identities() {
    fn duplicate_record(stream: &mut Vec<u8>, tag: u8, xmt_offset: usize, xmt: u16, len: usize) {
        let start = stream
            .windows(len)
            .rposition(|record| {
                record[..2] == [0, tag] && record[xmt_offset..xmt_offset + 2] == xmt.to_be_bytes()
            })
            .expect("support record");
        let duplicate = stream[start..start + len].to_vec();
        stream.extend(duplicate);
    }

    for (tag, xmt_offset, xmt, len) in [
        (126, 2, 20, 48),
        (125, 2, 21, 193),
        (127, 6, 30, 12),
        (128, 6, 32, 24),
    ] {
        let mut stream = bspline_partition_stream();
        duplicate_record(&mut stream, tag, xmt_offset, xmt, len);
        assert!(
            crate::nurbs::surfaces(&stream).is_empty(),
            "duplicate type {tag}"
        );
    }

    for (tag, xmt_offset, xmt, len) in [
        (136, 2, 40, 27),
        (135, 2, 41, 63),
        (127, 6, 42, 12),
        (128, 6, 43, 24),
    ] {
        let mut stream = bspline_partition_stream();
        duplicate_record(&mut stream, tag, xmt_offset, xmt, len);
        assert!(
            crate::nurbs::curves(&stream).is_empty(),
            "duplicate type {tag}"
        );
    }
}

#[test]
fn nurbs_decodes_descriptors_at_the_stream_boundary() {
    fn move_record_to_end(stream: &mut Vec<u8>, tag: u8, xmt: u16, len: usize) {
        let start = stream
            .windows(len)
            .position(|record| record[..2] == [0, tag] && record[2..4] == xmt.to_be_bytes())
            .expect("descriptor record");
        let record = stream.drain(start..start + len).collect::<Vec<_>>();
        stream.extend(record);
    }

    let mut surface = bspline_partition_stream();
    move_record_to_end(&mut surface, 126, 20, 48);
    assert_eq!(crate::nurbs::surfaces(&surface).len(), 1);

    let mut curve = bspline_partition_stream();
    move_record_to_end(&mut curve, 136, 40, 27);
    assert_eq!(crate::nurbs::curves(&curve).len(), 1);
}

#[test]
fn intersection_chart_rejects_nonfinite_millimeter_tolerance() {
    let mut stream = charted_intersection_curve_topology_partition_stream();
    let chart = stream
        .windows(2)
        .position(|window| window == [0, 40])
        .expect("chart record");
    put_f64(&mut stream, chart + 28, f64::MAX);
    assert!(
        crate::intersection::curves(&stream, crate::intersection::ChartPointLayout::Xyz3)
            .is_empty()
    );
}

#[test]
fn intersection_chart_layout_is_selected_by_stream_kind() {
    let ext11 = ext11_charted_intersection_curve_stream();
    assert!(crate::intersection::chart_source_records(
        &ext11,
        crate::intersection::ChartPointLayout::Xyz3,
    )
    .is_empty());
    let [chart] = crate::intersection::chart_source_records(
        &ext11,
        crate::intersection::ChartPointLayout::Ext11,
    )
    .try_into()
    .expect("one ext11 chart");
    assert_eq!(
        chart.point_layout,
        crate::intersection::ChartPointLayout::Ext11
    );
    assert_eq!(chart.native_parameters, Some(vec![2.0, 5.0]));
}

#[test]
fn intersection_chart_accepts_finite_model_coordinates_without_magnitude_bound() {
    let mut stream = charted_intersection_curve_topology_partition_stream();
    let chart = stream
        .windows(2)
        .position(|window| window == [0, 40])
        .expect("chart record");
    put_vec3(&mut stream, chart + 60, [1_000.0, 0.0, 0.0]);
    put_vec3(&mut stream, chart + 84, [1_000.01, 0.0, 0.0]);
    let [chart] = crate::intersection::chart_source_records(
        &stream,
        crate::intersection::ChartPointLayout::Xyz3,
    )
    .try_into()
    .expect("one large-coordinate chart");
    assert_eq!(chart.points[0].x, 1_000_000.0);
    assert_eq!(chart.points[1].x, 1_000_010.0);
}

#[test]
fn decode_replaces_ambiguous_ext11_uv_lanes_from_analytic_supports() {
    let stream = two_support_ext11_charted_intersection_curve_stream(true);
    let partition = two_support_charted_intersection_curve_stream();
    let mut cur = Cursor::new(prt_with_ext11_intersection(&partition, &stream));
    let result = NxCodec.decode(&mut cur, &DecodeOptions::default()).unwrap();

    let cadmpeg_ir::geometry::ProceduralCurveDefinition::Intersection { context, .. } =
        &result.ir.model.procedural_curves[0].definition
    else {
        panic!("typed intersection");
    };
    assert!(context.sides.iter().all(|side| side.pcurve.is_some()));
    assert!(cadmpeg_ir::validate::validate(&result.ir, Vec::new()).is_ok());
}

#[test]
fn decode_completes_one_non_sentinel_ext11_uv_lane_analytically() {
    let stream = partial_ext11_charted_intersection_curve_stream();
    let partition =
        two_support_charted_intersection_curve_stream_with_second_plane_axis([0.0, 0.0, 1.0]);
    let mut cur = Cursor::new(prt_with_ext11_intersection(&partition, &stream));
    let result = NxCodec.decode(&mut cur, &DecodeOptions::default()).unwrap();

    let cadmpeg_ir::geometry::ProceduralCurveDefinition::Intersection { context, .. } =
        &result.ir.model.procedural_curves[0].definition
    else {
        panic!("typed intersection");
    };
    assert!(context.sides[0].pcurve.is_some());
    assert!(context.sides[1].pcurve.is_some());
    assert!(cadmpeg_ir::validate::validate(&result.ir, Vec::new()).is_ok());
}

#[test]
fn completed_intersection_support_lane_attaches_after_topology_emission() {
    let mut ir = cadmpeg_ir::examples::unit_cube();
    let edge = cadmpeg_ir::ids::EdgeId("synthetic:cube:edge#0".into());
    let target = ir
        .model
        .coedges
        .iter_mut()
        .find(|coedge| coedge.edge == edge && coedge.id.0.contains("bottom"))
        .expect("bottom coedge");
    target.id = cadmpeg_ir::ids::CoedgeId("nx:s0:fin#42".into());
    target.pcurves.clear();
    let owner_loop = target.owner_loop.clone();
    let surface = ir
        .model
        .loops
        .iter()
        .find(|loop_| loop_.id == owner_loop)
        .and_then(|loop_| {
            ir.model
                .faces
                .iter()
                .find(|face| face.id == loop_.face)
                .map(|face| face.surface.clone())
        })
        .expect("bottom support");
    let curve = ir
        .model
        .edges
        .iter()
        .find(|candidate| candidate.id == edge)
        .and_then(|edge| edge.curve.clone())
        .expect("edge curve");
    let edge_tolerance = ir
        .model
        .edges
        .iter()
        .find(|candidate| candidate.id == edge)
        .and_then(|edge| edge.tolerance);
    ir.model
        .procedural_curves
        .push(cadmpeg_ir::geometry::ProceduralCurve {
            id: cadmpeg_ir::ids::ProceduralCurveId("nx:test:intersection#0".into()),
            curve,
            definition: ProceduralCurveDefinition::Intersection {
                context: cadmpeg_ir::geometry::IntcurveSupportContext {
                    sides: [
                        cadmpeg_ir::geometry::IntcurveSupportSide {
                            surface: Some(surface),
                            pcurve_parameter_range: None,
                            pcurve: Some(PcurveGeometry::Nurbs {
                                degree: 1,
                                knots: vec![0.0, 0.0, 1.0, 1.0],
                                control_points: vec![Point2::new(0.0, 0.0), Point2::new(10.0, 0.0)],
                                weights: None,
                                periodic: false,
                            }),
                        },
                        cadmpeg_ir::geometry::IntcurveSupportSide {
                            surface: None,
                            pcurve_parameter_range: None,
                            pcurve: None,
                        },
                    ],
                    parameter_range: [0.0, 1.0],
                    discontinuities: [Vec::new(), Vec::new(), Vec::new()],
                },
                discontinuity_flag: false,
            },
            cache_fit_tolerance: None,
        });
    let mut annotations = cadmpeg_ir::AnnotationBuilder::new();
    let source_stream = annotations.stream("nx:test");

    crate::decode::attach_completed_intersection_pcurves(
        &mut ir,
        &crate::topology::Graph::parse(&[]),
        "nx:s0",
        source_stream,
        &mut annotations,
    );

    let completed = ir
        .model
        .pcurves
        .iter()
        .find(|pcurve| pcurve.id.0.contains("intersection-pcurve-completed"))
        .expect("validated completed support lane attaches");
    assert_eq!(completed.fit_tolerance, edge_tolerance);
    assert!(ir.model.coedges.iter().any(|coedge| coedge
        .pcurves
        .iter()
        .any(|pcurve| pcurve.pcurve == completed.id)));
}

#[test]
fn ext11_uv_completion_runs_after_support_incidence_resolution() {
    let stream = two_support_ext11_charted_intersection_curve_stream(false);
    let partition =
        two_support_charted_intersection_curve_stream_with_second_plane_axis([0.0, 0.0, 1.0]);
    let mut cur = Cursor::new(prt_with_ext11_intersection(&partition, &stream));
    let mut result = NxCodec.decode(&mut cur, &DecodeOptions::default()).unwrap();
    let procedural_id = result.ir.model.procedural_curves[0].id.clone();
    let cadmpeg_ir::geometry::ProceduralCurveDefinition::Intersection { context, .. } =
        &mut result.ir.model.procedural_curves[0].definition
    else {
        panic!("typed intersection");
    };
    for side in &mut context.sides {
        side.pcurve = None;
    }
    let pending = vec![(
        procedural_id,
        vec![
            cadmpeg_ir::math::Point3::new(0.0, 0.0, 0.0),
            cadmpeg_ir::math::Point3::new(10.0, 0.0, 0.0),
        ],
        vec![0.0, 0.01],
        0.01,
        [
            Some(vec![[0.0, 0.0], [0.01, 0.0]]),
            Some(vec![[0.0, 0.0], [0.0, 0.01]]),
        ],
    )];

    crate::decode::complete_ext11_support_uv(&mut result.ir, &pending);

    let cadmpeg_ir::geometry::ProceduralCurveDefinition::Intersection { context, .. } =
        &result.ir.model.procedural_curves[0].definition
    else {
        panic!("typed intersection");
    };
    assert!(context.sides.iter().all(|side| side.pcurve.is_some()));
    assert!(cadmpeg_ir::validate::validate(&result.ir, Vec::new()).is_ok());
}

#[test]
fn analytic_uv_completion_fills_missing_intersection_support_lanes() {
    let stream = two_support_ext11_charted_intersection_curve_stream(false);
    let partition =
        two_support_charted_intersection_curve_stream_with_second_plane_axis([0.0, 0.0, 1.0]);
    let mut cur = Cursor::new(prt_with_ext11_intersection(&partition, &stream));
    let mut result = NxCodec.decode(&mut cur, &DecodeOptions::default()).unwrap();
    let procedural_id = result.ir.model.procedural_curves[0].id.clone();
    let ProceduralCurveDefinition::Intersection { context, .. } =
        &mut result.ir.model.procedural_curves[0].definition
    else {
        panic!("typed intersection");
    };
    for side in &mut context.sides {
        side.pcurve = None;
    }
    let pending = vec![(
        procedural_id,
        vec![
            cadmpeg_ir::math::Point3::new(0.0, 0.0, 0.0),
            cadmpeg_ir::math::Point3::new(10.0, 0.0, 0.0),
        ],
        vec![0.0, 0.01],
        0.01,
        [None, None],
    )];

    crate::decode::complete_support_uv(&mut result.ir, &pending);

    let ProceduralCurveDefinition::Intersection { context, .. } =
        &result.ir.model.procedural_curves[0].definition
    else {
        panic!("typed intersection");
    };
    assert!(context.sides.iter().all(|side| side.pcurve.is_some()));
    assert!(cadmpeg_ir::validate::validate(&result.ir, Vec::new()).is_ok());
}

#[test]
fn support_uv_completion_closes_blend_spine_dependencies_to_a_fixed_point() {
    use cadmpeg_ir::geometry::{BlendSupport, ProceduralSurface, Surface};
    use cadmpeg_ir::ids::{ProceduralCurveId, ProceduralSurfaceId, SurfaceId};

    let stream = two_support_ext11_charted_intersection_curve_stream(false);
    let partition =
        two_support_charted_intersection_curve_stream_with_second_plane_axis([0.0, 0.0, 1.0]);
    let mut cur = Cursor::new(prt_with_ext11_intersection(&partition, &stream));
    let mut result = NxCodec.decode(&mut cur, &DecodeOptions::default()).unwrap();
    let spine_id = result.ir.model.procedural_curves[0].id.clone();
    let spine_curve = result.ir.model.procedural_curves[0].curve.clone();
    let ProceduralCurveDefinition::Intersection { context, .. } =
        &result.ir.model.procedural_curves[0].definition
    else {
        panic!("typed intersection");
    };
    let spine_surfaces = context
        .sides
        .each_ref()
        .map(|side| side.surface.clone().unwrap());
    let radius = 2.0;
    let offset_surfaces = [0usize, 1usize].map(|side| {
        let support = result
            .ir
            .model
            .surfaces
            .iter()
            .find(|surface| surface.id == spine_surfaces[side])
            .unwrap();
        let SurfaceGeometry::Plane {
            origin,
            normal,
            u_axis,
        } = support.geometry
        else {
            panic!("plane support");
        };
        let id = SurfaceId(format!("synthetic:offset-support-{side}"));
        result.ir.model.surfaces.push(Surface {
            id: id.clone(),
            geometry: SurfaceGeometry::Plane {
                origin: cadmpeg_ir::math::Point3::new(
                    origin.x + radius * normal.x,
                    origin.y + radius * normal.y,
                    origin.z + radius * normal.z,
                ),
                normal,
                u_axis,
            },
            source_object: None,
        });
        id
    });
    let blend = SurfaceId("synthetic:dependent-blend".into());
    let blend_construction = ProceduralSurfaceId("synthetic:dependent-blend-definition".into());
    result.ir.model.surfaces.push(Surface {
        id: blend.clone(),
        geometry: SurfaceGeometry::Procedural {
            construction: blend_construction.clone(),
        },
        source_object: None,
    });
    result.ir.model.procedural_surfaces.push(ProceduralSurface {
        id: blend_construction,
        surface: blend.clone(),
        definition: ProceduralSurfaceDefinition::Blend {
            supports: offset_surfaces.map(|surface| {
                Some(BlendSupport {
                    surface,
                    reversed: false,
                })
            }),
            spine: Some(spine_curve.clone()),
            radius: BlendRadiusLaw::Constant {
                signed_radius: radius,
            },
            cross_section: BlendCrossSection::Circular,
            native: None,
        },
        cache_fit_tolerance: None,
        record_bounds: None,
    });
    let parameters = vec![0.0, 0.01];
    let spine_carrier = result
        .ir
        .model
        .curves
        .iter()
        .find(|curve| curve.id == spine_curve)
        .expect("blend spine carrier");
    assert!(
        cadmpeg_ir::eval::curve_point(&spine_carrier.geometry, 0.0).is_some(),
        "spine carrier: {:?}",
        spine_carrier.geometry
    );
    let points = parameters
        .iter()
        .map(|parameter| {
            crate::decode::blend_surface_point(&result.ir, &blend, *parameter, 0.5).unwrap()
        })
        .collect::<Vec<_>>();

    let dependent_id = ProceduralCurveId("synthetic:dependent-intersection".into());
    let mut dependent = result.ir.model.procedural_curves[0].clone();
    dependent.id = dependent_id.clone();
    let ProceduralCurveDefinition::Intersection { context, .. } = &mut dependent.definition else {
        unreachable!()
    };
    context.sides[0].surface = Some(blend);
    context.sides[0].pcurve = None;
    context.sides[1].surface = None;
    context.sides[1].pcurve = None;
    result.ir.model.procedural_curves.insert(0, dependent);
    let ProceduralCurveDefinition::Intersection { context, .. } =
        &mut result.ir.model.procedural_curves[1].definition
    else {
        unreachable!()
    };
    for side in &mut context.sides {
        side.pcurve = None;
    }
    let pending = vec![
        (dependent_id, points, parameters.clone(), 0.01, [None, None]),
        (
            spine_id,
            vec![
                cadmpeg_ir::math::Point3::new(0.0, 0.0, 0.0),
                cadmpeg_ir::math::Point3::new(10.0, 0.0, 0.0),
            ],
            parameters,
            0.01,
            [None, None],
        ),
    ];

    crate::decode::complete_support_uv(&mut result.ir, &pending);

    let ProceduralCurveDefinition::Intersection { context, .. } =
        &result.ir.model.procedural_curves[0].definition
    else {
        unreachable!()
    };
    assert!(context.sides[0].pcurve.is_some());
}

#[test]
fn analytic_uv_completion_replaces_a_sentinel_contaminated_support_lane() {
    let stream = two_support_ext11_charted_intersection_curve_stream(false);
    let partition =
        two_support_charted_intersection_curve_stream_with_second_plane_axis([0.0, 0.0, 1.0]);
    let mut cur = Cursor::new(prt_with_ext11_intersection(&partition, &stream));
    let mut result = NxCodec.decode(&mut cur, &DecodeOptions::default()).unwrap();
    let procedural_id = result.ir.model.procedural_curves[0].id.clone();
    let ProceduralCurveDefinition::Intersection { context, .. } =
        &mut result.ir.model.procedural_curves[0].definition
    else {
        panic!("typed intersection");
    };
    let Some(PcurveGeometry::Nurbs { control_points, .. }) = context.sides[0].pcurve.as_mut()
    else {
        panic!("NURBS support lane");
    };
    control_points[1] = Point2::new(
        crate::decode::MISSING_TOLERANCE,
        crate::decode::MISSING_TOLERANCE,
    );
    let pending = vec![(
        procedural_id,
        vec![
            cadmpeg_ir::math::Point3::new(0.0, 0.0, 0.0),
            cadmpeg_ir::math::Point3::new(10.0, 0.0, 0.0),
        ],
        vec![0.0, 0.01],
        0.01,
        [None, None],
    )];

    crate::decode::complete_support_uv(&mut result.ir, &pending);

    let ProceduralCurveDefinition::Intersection { context, .. } =
        &result.ir.model.procedural_curves[0].definition
    else {
        panic!("typed intersection");
    };
    let Some(PcurveGeometry::Nurbs { control_points, .. }) = context.sides[0].pcurve.as_ref()
    else {
        panic!("NURBS support lane");
    };
    assert!(control_points.iter().all(|point| {
        point.u.to_bits() != crate::decode::MISSING_TOLERANCE.to_bits()
            && point.v.to_bits() != crate::decode::MISSING_TOLERANCE.to_bits()
    }));
    assert!(cadmpeg_ir::validate::validate(&result.ir, Vec::new()).is_ok());
}

#[test]
fn analytic_uv_completion_replaces_a_finite_mismatched_support_lane() {
    let stream = two_support_ext11_charted_intersection_curve_stream(false);
    let partition =
        two_support_charted_intersection_curve_stream_with_second_plane_axis([0.0, 0.0, 1.0]);
    let mut cur = Cursor::new(prt_with_ext11_intersection(&partition, &stream));
    let mut result = NxCodec.decode(&mut cur, &DecodeOptions::default()).unwrap();
    let procedural_id = result.ir.model.procedural_curves[0].id.clone();
    let ProceduralCurveDefinition::Intersection { context, .. } =
        &mut result.ir.model.procedural_curves[0].definition
    else {
        panic!("typed intersection");
    };
    let Some(PcurveGeometry::Nurbs { control_points, .. }) = context.sides[0].pcurve.as_mut()
    else {
        panic!("NURBS support lane");
    };
    for point in control_points {
        point.u += 100.0;
    }
    let pending = vec![(
        procedural_id,
        vec![
            cadmpeg_ir::math::Point3::new(0.0, 0.0, 0.0),
            cadmpeg_ir::math::Point3::new(10.0, 0.0, 0.0),
        ],
        vec![0.0, 0.01],
        0.01,
        [None, None],
    )];

    crate::decode::invalidate_inconsistent_support_uv(&mut result.ir, &pending);
    crate::decode::complete_support_uv(&mut result.ir, &pending);

    assert!(cadmpeg_ir::validate::validate(&result.ir, Vec::new()).is_ok());
}

#[test]
fn equivalent_offset_supports_share_a_complete_parameter_lane() {
    use cadmpeg_ir::geometry::{ProceduralCurve, ProceduralSurface, Surface};
    use cadmpeg_ir::ids::{CurveId, ProceduralCurveId, ProceduralSurfaceId, SurfaceId};
    use cadmpeg_ir::math::{Point3, Vector3};

    let mut ir = cadmpeg_ir::document::CadIr::empty(cadmpeg_ir::units::Units::default());
    let supports = [SurfaceId("support-a".into()), SurfaceId("support-b".into())];
    for support in &supports {
        ir.model.surfaces.push(Surface {
            id: support.clone(),
            geometry: SurfaceGeometry::Plane {
                origin: Point3::new(0.0, 0.0, 0.0),
                normal: Vector3::new(0.0, 0.0, 1.0),
                u_axis: Vector3::new(1.0, 0.0, 0.0),
            },
            source_object: None,
        });
    }
    let offsets = [SurfaceId("offset-a".into()), SurfaceId("offset-b".into())];
    for (ordinal, (surface, support)) in offsets.iter().zip(&supports).enumerate() {
        let construction = ProceduralSurfaceId(format!("offset-construction-{ordinal}"));
        ir.model.surfaces.push(Surface {
            id: surface.clone(),
            geometry: SurfaceGeometry::Procedural {
                construction: construction.clone(),
            },
            source_object: None,
        });
        ir.model.procedural_surfaces.push(ProceduralSurface {
            id: construction,
            surface: surface.clone(),
            definition: ProceduralSurfaceDefinition::Offset {
                support: support.clone(),
                distance: 30.0,
                u_sense: Some(0),
                v_sense: Some(0),
                extension_flags: Vec::new(),
                revision_form: None,
            },
            cache_fit_tolerance: None,
            record_bounds: None,
        });
    }
    ir.model.procedural_curves.push(ProceduralCurve {
        id: ProceduralCurveId("intersection".into()),
        curve: CurveId("curve".into()),
        definition: ProceduralCurveDefinition::Intersection {
            context: cadmpeg_ir::geometry::IntcurveSupportContext {
                sides: [
                    cadmpeg_ir::geometry::IntcurveSupportSide {
                        surface: Some(offsets[0].clone()),
                        pcurve_parameter_range: None,
                        pcurve: None,
                    },
                    cadmpeg_ir::geometry::IntcurveSupportSide {
                        surface: Some(offsets[1].clone()),
                        pcurve_parameter_range: None,
                        pcurve: Some(PcurveGeometry::Line {
                            origin: Point2::new(1.0, 2.0),
                            direction: Point2::new(3.0, 4.0),
                        }),
                    },
                ],
                parameter_range: [0.0, 1.0],
                discontinuities: [Vec::new(), Vec::new(), Vec::new()],
            },
            discontinuity_flag: false,
        },
        cache_fit_tolerance: None,
    });

    assert!(crate::decode::parameterization_equivalent_surfaces(
        &ir,
        &offsets[0],
        &offsets[1]
    ));
    crate::decode::complete_parameterization_equivalent_support_uv(&mut ir);
    let ProceduralCurveDefinition::Intersection { context, .. } =
        &ir.model.procedural_curves[0].definition
    else {
        panic!("intersection");
    };
    assert_eq!(context.sides[0].pcurve, context.sides[1].pcurve);

    let ProceduralSurfaceDefinition::Offset { distance, .. } =
        &mut ir.model.procedural_surfaces[1].definition
    else {
        unreachable!()
    };
    *distance = 31.0;
    assert!(!crate::decode::parameterization_equivalent_surfaces(
        &ir,
        &offsets[0],
        &offsets[1]
    ));
}

#[test]
fn nurbs_parameter_solver_inverts_a_rational_surface_point() {
    let surface = cadmpeg_ir::geometry::NurbsSurface {
        u_degree: 1,
        v_degree: 1,
        u_knots: vec![0.0, 0.0, 1.0, 1.0],
        v_knots: vec![0.0, 0.0, 1.0, 1.0],
        u_count: 2,
        v_count: 2,
        control_points: vec![
            cadmpeg_ir::math::Point3::new(0.0, 0.0, 0.0),
            cadmpeg_ir::math::Point3::new(0.0, 10.0, 0.0),
            cadmpeg_ir::math::Point3::new(10.0, 0.0, 0.0),
            cadmpeg_ir::math::Point3::new(10.0, 10.0, 0.0),
        ],
        weights: Some(vec![1.0, 2.0, 3.0, 4.0]),
        u_periodic: false,
        v_periodic: false,
    };
    let expected = Point2::new(0.37, 0.61);
    let point = cadmpeg_ir::eval::nurbs_surface_point(&surface, expected.u, expected.v).unwrap();

    let actual = crate::decode::nurbs_parameters(&surface, point, None).unwrap();

    assert!((actual.u - expected.u).abs() < 1.0e-10);
    assert!((actual.v - expected.v).abs() < 1.0e-10);

    let after_invalid_seed =
        crate::decode::nurbs_parameters(&surface, point, Some(Point2::new(f64::NAN, 0.5))).unwrap();
    assert!((after_invalid_seed.u - expected.u).abs() < 1.0e-10);
    assert!((after_invalid_seed.v - expected.v).abs() < 1.0e-10);
}

#[test]
fn surface_intersection_continuation_corrects_a_chart_selected_branch() {
    use cadmpeg_ir::geometry::Surface;
    use cadmpeg_ir::ids::SurfaceId;
    use cadmpeg_ir::math::Point3;

    let mut ir = cadmpeg_ir::document::CadIr::empty(cadmpeg_ir::units::Units::default());
    let first = SurfaceId("synthetic:first-intersection-plane".into());
    let second = SurfaceId("synthetic:second-intersection-plane".into());
    ir.model.surfaces.extend([
        Surface {
            id: first.clone(),
            geometry: SurfaceGeometry::Plane {
                origin: Point3::new(0.0, 0.0, 0.0),
                normal: Vector3::new(1.0, 0.0, 0.0),
                u_axis: Vector3::new(0.0, 0.0, 1.0),
            },
            source_object: None,
        },
        Surface {
            id: second.clone(),
            geometry: SurfaceGeometry::Plane {
                origin: Point3::new(0.0, 0.0, 0.0),
                normal: Vector3::new(0.0, 1.0, 0.0),
                u_axis: Vector3::new(0.0, 0.0, 1.0),
            },
            source_object: None,
        },
    ]);
    let chart = vec![
        Point3::new(1.0e-4, -2.0e-4, 0.0),
        Point3::new(-1.0e-4, 2.0e-4, 2.0),
        Point3::new(2.0e-4, 1.0e-4, 5.0),
    ];
    let lanes = crate::decode::continue_surface_intersection_parameters(
        &ir,
        [&first, &second],
        &chart,
        1.0e-3,
    )
    .unwrap();
    assert_eq!(lanes[0].len(), chart.len());
    for (ordinal, expected_z) in [0.0, 2.0, 5.0].into_iter().enumerate() {
        let first_point = cadmpeg_ir::eval::model_surface_point_by_id(
            &cadmpeg_ir::index::ModelIndex::new(&ir),
            &first,
            lanes[0][ordinal].u,
            lanes[0][ordinal].v,
        )
        .unwrap();
        let second_point = cadmpeg_ir::eval::model_surface_point_by_id(
            &cadmpeg_ir::index::ModelIndex::new(&ir),
            &second,
            lanes[1][ordinal].u,
            lanes[1][ordinal].v,
        )
        .unwrap();
        assert!((first_point.x - second_point.x).abs() < 1.0e-10);
        assert!((first_point.y - second_point.y).abs() < 1.0e-10);
        assert!((first_point.z - second_point.z).abs() < 1.0e-10);
        assert!((first_point.z - expected_z).abs() < 1.0e-10);
    }

    let off_branch = [chart[0], Point3::new(1.0, 1.0, 2.0)];
    assert!(crate::decode::continue_surface_intersection_parameters(
        &ir,
        [&first, &second],
        &off_branch,
        1.0e-3,
    )
    .is_none());
    assert!(crate::decode::continue_surface_intersection_parameters(
        &ir,
        [&first, &first],
        &chart,
        1.0e-3,
    )
    .is_none());

    let cylinder = SurfaceId("synthetic:intersection-cylinder".into());
    let section_plane = SurfaceId("synthetic:intersection-section-plane".into());
    ir.model.surfaces.extend([
        Surface {
            id: cylinder.clone(),
            geometry: SurfaceGeometry::Cylinder {
                origin: Point3::new(0.0, 0.0, 0.0),
                axis: Vector3::new(0.0, 0.0, 1.0),
                ref_direction: Vector3::new(1.0, 0.0, 0.0),
                radius: 2.0,
            },
            source_object: None,
        },
        Surface {
            id: section_plane.clone(),
            geometry: SurfaceGeometry::Plane {
                origin: Point3::new(0.0, 0.0, 0.0),
                normal: Vector3::new(0.0, 0.0, 1.0),
                u_axis: Vector3::new(1.0, 0.0, 0.0),
            },
            source_object: None,
        },
    ]);
    let circular_chart =
        [0.0_f64, 0.3, 0.8].map(|angle| Point3::new(2.0 * angle.cos(), 2.0 * angle.sin(), 1.0e-5));
    let circular_lanes = crate::decode::continue_surface_intersection_parameters(
        &ir,
        [&cylinder, &section_plane],
        &circular_chart,
        1.0e-3,
    )
    .unwrap();
    for (cylinder_uv, plane_uv) in circular_lanes[0].iter().zip(&circular_lanes[1]) {
        let cylinder_point = cadmpeg_ir::eval::model_surface_point_by_id(
            &cadmpeg_ir::index::ModelIndex::new(&ir),
            &cylinder,
            cylinder_uv.u,
            cylinder_uv.v,
        )
        .unwrap();
        let plane_point = cadmpeg_ir::eval::model_surface_point_by_id(
            &cadmpeg_ir::index::ModelIndex::new(&ir),
            &section_plane,
            plane_uv.u,
            plane_uv.v,
        )
        .unwrap();
        assert!((cylinder_point.x - plane_point.x).abs() < 1.0e-8);
        assert!((cylinder_point.y - plane_point.y).abs() < 1.0e-8);
        assert!((cylinder_point.z - plane_point.z).abs() < 1.0e-8);
    }

    let tangent_cylinder = SurfaceId("synthetic:tangent-cylinder".into());
    let tangent_plane = SurfaceId("synthetic:tangent-plane".into());
    ir.model.surfaces.extend([
        Surface {
            id: tangent_cylinder.clone(),
            geometry: SurfaceGeometry::Cylinder {
                origin: Point3::new(0.0, 0.0, 1.0),
                axis: Vector3::new(0.0, 1.0, 0.0),
                ref_direction: Vector3::new(0.0, 0.0, -1.0),
                radius: 1.0,
            },
            source_object: None,
        },
        Surface {
            id: tangent_plane.clone(),
            geometry: SurfaceGeometry::Plane {
                origin: Point3::new(0.0, 0.0, 0.0),
                normal: Vector3::new(0.0, 0.0, 1.0),
                u_axis: Vector3::new(1.0, 0.0, 0.0),
            },
            source_object: None,
        },
    ]);
    let tangent_chart = [0.0, 1.0, 3.0, 6.0].map(|y| Point3::new(0.0, y, 0.0));
    let tangent_lanes = crate::decode::continue_surface_intersection_parameters(
        &ir,
        [&tangent_cylinder, &tangent_plane],
        &tangent_chart,
        1.0e-8,
    )
    .unwrap();
    for (ordinal, y) in [0.0, 1.0, 3.0, 6.0].into_iter().enumerate() {
        assert!((tangent_lanes[0][ordinal].v - y).abs() < 1.0e-10);
        assert!((tangent_lanes[1][ordinal].v - y).abs() < 1.0e-10);
    }

    let seam_chart = [3.0_f64, 3.1, 3.2, 3.3]
        .map(|angle| Point3::new(2.0 * angle.cos(), 2.0 * angle.sin(), 1.0e-5));
    let seam_lanes = crate::decode::continue_surface_intersection_parameters(
        &ir,
        [&cylinder, &section_plane],
        &seam_chart,
        1.0e-3,
    )
    .unwrap();
    assert!(seam_lanes[0].windows(2).all(|pair| pair[0].u < pair[1].u));
    assert!(seam_lanes[0].last().unwrap().u > std::f64::consts::PI);

    let periodic_nurbs = SurfaceId("synthetic:periodic-nurbs-prism".into());
    let nurbs_section = SurfaceId("synthetic:periodic-nurbs-section".into());
    let periodic_geometry = cadmpeg_ir::geometry::NurbsSurface {
        u_degree: 1,
        v_degree: 1,
        u_knots: vec![0.0, 0.0, 1.0, 2.0, 3.0, 4.0, 4.0],
        v_knots: vec![0.0, 0.0, 1.0, 1.0],
        u_count: 5,
        v_count: 2,
        control_points: [(1.0, 0.0), (0.0, 1.0), (-1.0, 0.0), (0.0, -1.0), (1.0, 0.0)]
            .into_iter()
            .flat_map(|(x, y)| [Point3::new(x, y, 0.0), Point3::new(x, y, 1.0)])
            .collect(),
        weights: None,
        u_periodic: true,
        v_periodic: false,
    };
    ir.model.surfaces.extend([
        Surface {
            id: periodic_nurbs.clone(),
            geometry: SurfaceGeometry::Nurbs(periodic_geometry.clone()),
            source_object: None,
        },
        Surface {
            id: nurbs_section.clone(),
            geometry: SurfaceGeometry::Plane {
                origin: Point3::new(0.0, 0.0, 0.5),
                normal: Vector3::new(0.0, 0.0, 1.0),
                u_axis: Vector3::new(1.0, 0.0, 0.0),
            },
            source_object: None,
        },
    ]);
    let nurbs_chart = [3.8, 3.9, 4.1, 4.2]
        .map(|u| cadmpeg_ir::eval::nurbs_surface_point(&periodic_geometry, u, 0.5).unwrap());
    let nurbs_lanes = crate::decode::continue_surface_intersection_parameters(
        &ir,
        [&periodic_nurbs, &nurbs_section],
        &nurbs_chart,
        1.0e-8,
    )
    .unwrap();
    assert!(nurbs_lanes[0].windows(2).all(|pair| pair[0].u < pair[1].u));
    assert!(nurbs_lanes[0].last().unwrap().u > 4.0);
}

#[test]
fn surface_intersection_jacobian_is_stable_at_large_model_coordinates() {
    use cadmpeg_ir::geometry::Surface;
    use cadmpeg_ir::ids::SurfaceId;
    use cadmpeg_ir::math::Point3;

    let mut ir = cadmpeg_ir::document::CadIr::empty(cadmpeg_ir::units::Units::default());
    let horizontal = SurfaceId("synthetic:large-horizontal-plane".into());
    let vertical = SurfaceId("synthetic:large-vertical-plane".into());
    let origin = Point3::new(1.0e16, 1.0e16, 0.0);
    ir.model.surfaces.extend([
        Surface {
            id: horizontal.clone(),
            geometry: SurfaceGeometry::Plane {
                origin,
                normal: Vector3::new(0.0, 0.0, 1.0),
                u_axis: Vector3::new(1.0, 0.0, 0.0),
            },
            source_object: None,
        },
        Surface {
            id: vertical.clone(),
            geometry: SurfaceGeometry::Plane {
                origin,
                normal: Vector3::new(0.0, 1.0, 0.0),
                u_axis: Vector3::new(1.0, 0.0, 0.0),
            },
            source_object: None,
        },
    ]);
    let chart =
        [0.0, 4.0, 8.0].map(|distance| Point3::new(origin.x + distance, origin.y, origin.z));

    let lanes = crate::decode::continue_surface_intersection_parameters(
        &ir,
        [&horizontal, &vertical],
        &chart,
        0.1,
    )
    .expect("exact plane partials keep the continuation Jacobian full rank");

    for (ordinal, expected) in [0.0, 4.0, 8.0].into_iter().enumerate() {
        assert_eq!(lanes[0][ordinal], Point2::new(expected, 0.0));
        assert_eq!(lanes[1][ordinal], Point2::new(expected, 0.0));
    }
}

#[test]
fn damped_intersection_correction_reduces_a_rank_deficient_system() {
    let matrix = [
        [1.0, 0.0, -1.0, 0.0],
        [0.0, 1.0, 0.0, -1.0],
        [0.0, 0.0, 0.0, 0.0],
        [1.0, 0.0, 1.0, 0.0],
    ];
    let rhs = [2.0, -4.0, 0.0, 6.0];

    let step = crate::decode::solve_damped_least_squares_4x4(matrix, rhs).unwrap();
    let residual = std::array::from_fn::<_, 4, _>(|row| {
        (0..4)
            .map(|column| matrix[row][column] * step[column])
            .sum::<f64>()
            - rhs[row]
    });

    assert!(residual.iter().all(|value| value.abs() < 1.0e-8));
    assert!(step.iter().all(|value| value.is_finite()));
    for (actual, expected) in step.into_iter().zip([4.0, -2.0, 2.0, 2.0]) {
        assert!((actual - expected).abs() < 1.0e-8);
    }
}

#[test]
fn periodic_surface_lookup_rejects_a_cyclic_offset_graph() {
    use cadmpeg_ir::geometry::{ProceduralSurface, Surface};
    use cadmpeg_ir::ids::{ProceduralSurfaceId, SurfaceId};

    let mut ir = cadmpeg_ir::document::CadIr::empty(cadmpeg_ir::units::Units::default());
    let surfaces = [SurfaceId("cycle-a".into()), SurfaceId("cycle-b".into())];
    let constructions = [
        ProceduralSurfaceId("cycle-construction-a".into()),
        ProceduralSurfaceId("cycle-construction-b".into()),
    ];
    for side in 0..2 {
        ir.model.surfaces.push(Surface {
            id: surfaces[side].clone(),
            geometry: SurfaceGeometry::Procedural {
                construction: constructions[side].clone(),
            },
            source_object: None,
        });
        ir.model.procedural_surfaces.push(ProceduralSurface {
            id: constructions[side].clone(),
            surface: surfaces[side].clone(),
            definition: ProceduralSurfaceDefinition::Offset {
                support: surfaces[1 - side].clone(),
                distance: 1.0,
                u_sense: Some(0),
                v_sense: Some(0),
                extension_flags: Vec::new(),
                revision_form: None,
            },
            cache_fit_tolerance: None,
            record_bounds: None,
        });
    }

    assert_eq!(
        crate::decode::surface_parameter_periods(&ir, &surfaces[0]),
        [None, None]
    );
}

#[test]
fn nurbs_parameter_solver_rejects_a_remote_local_minimum_seed() {
    let mut control_points = Vec::new();
    for (x, z) in [
        (-10.0, 0.0),
        (0.0, 0.0),
        (10.0, 2.0),
        (0.0, 4.0),
        (-10.0, 4.0),
    ] {
        control_points.extend([
            cadmpeg_ir::math::Point3::new(x, 0.0, z),
            cadmpeg_ir::math::Point3::new(x, 10.0, z),
        ]);
    }
    let surface = cadmpeg_ir::geometry::NurbsSurface {
        u_degree: 1,
        v_degree: 1,
        u_knots: vec![0.0, 0.0, 0.25, 0.5, 0.75, 1.0, 1.0],
        v_knots: vec![0.0, 0.0, 1.0, 1.0],
        u_count: 5,
        v_count: 2,
        control_points,
        weights: None,
        u_periodic: false,
        v_periodic: false,
    };
    let expected = Point2::new(0.125, 0.3);
    let point = cadmpeg_ir::eval::nurbs_surface_point(&surface, expected.u, expected.v).unwrap();

    let actual =
        crate::decode::nurbs_parameters(&surface, point, Some(Point2::new(0.875, 0.3))).unwrap();

    assert!((actual.u - expected.u).abs() < 1.0e-10);
    assert!((actual.v - expected.v).abs() < 1.0e-10);
}

#[test]
fn nurbs_parameter_solver_preserves_close_equal_branches() {
    let mut control_points = Vec::new();
    for (x, z) in [(-1.0, 0.0), (0.0, 0.0), (1.0, 1.0), (0.0, 0.0), (-1.0, 2.0)] {
        control_points.extend([
            cadmpeg_ir::math::Point3::new(x, 0.0, z),
            cadmpeg_ir::math::Point3::new(x, 10.0, z),
        ]);
    }
    let surface = cadmpeg_ir::geometry::NurbsSurface {
        u_degree: 1,
        v_degree: 1,
        u_knots: vec![0.0, 0.0, 0.4999, 0.5, 0.5001, 1.0, 1.0],
        v_knots: vec![0.0, 0.0, 1.0, 1.0],
        u_count: 5,
        v_count: 2,
        control_points,
        weights: Some(vec![1.0, 1.2, 1.0, 1.2, 1.0, 1.2, 1.0, 1.2, 1.0, 1.2]),
        u_periodic: false,
        v_periodic: false,
    };
    let expected = Point2::new(0.5001, 0.3);
    let point = cadmpeg_ir::eval::nurbs_surface_point(&surface, expected.u, expected.v).unwrap();

    let actual =
        crate::decode::nurbs_parameters(&surface, point, Some(Point2::new(0.50011, 0.3))).unwrap();

    assert!((actual.u - expected.u).abs() < 1.0e-10);
    assert!((actual.v - expected.v).abs() < 1.0e-10);
}

#[test]
fn nurbs_curve_closest_parameter_does_not_trust_a_remote_seed() {
    use cadmpeg_ir::geometry::{Curve, NurbsCurve};
    use cadmpeg_ir::ids::CurveId;

    let mut ir = cadmpeg_ir::document::CadIr::empty(cadmpeg_ir::units::Units::default());
    let curve = CurveId("synthetic:piecewise-spine".into());
    ir.model.curves.push(Curve {
        id: curve.clone(),
        geometry: CurveGeometry::Nurbs(NurbsCurve {
            degree: 1,
            knots: vec![0.0, 0.0, 0.5, 1.0, 1.0],
            control_points: vec![
                cadmpeg_ir::math::Point3::new(-10.0, 0.0, 0.0),
                cadmpeg_ir::math::Point3::new(0.0, 0.0, 0.0),
                cadmpeg_ir::math::Point3::new(10.0, 10.0, 0.0),
            ],
            weights: None,
            periodic: false,
        }),
        source_object: None,
    });

    let actual = crate::decode::closest_spine_parameter(
        &ir,
        &curve,
        cadmpeg_ir::math::Point3::new(-5.0, 2.0, 0.0),
        Some(0.9),
    )
    .unwrap();

    assert!((actual - 0.25).abs() < 1.0e-10);
}

#[test]
fn spine_contact_pcurve_inverts_linear_and_rational_support_parameters() {
    let pcurve = PcurveGeometry::Nurbs {
        degree: 1,
        knots: vec![2.0, 2.0, 5.0, 9.0, 9.0],
        control_points: vec![
            Point2::new(-1.0, 3.0),
            Point2::new(2.0, 6.0),
            Point2::new(6.0, 4.0),
        ],
        weights: None,
        periodic: false,
    };

    let first =
        crate::decode::closest_pcurve_parameters(&pcurve, Point2::new(0.5, 4.5), None).unwrap()[0];
    let second =
        crate::decode::closest_pcurve_parameters(&pcurve, Point2::new(5.0, 4.5), None).unwrap()[0];

    assert!((first - 3.5).abs() < 1.0e-12);
    assert!((second - 8.0).abs() < 1.0e-12);

    let rational = PcurveGeometry::Nurbs {
        degree: 1,
        knots: vec![0.0, 0.0, 1.0, 1.0],
        control_points: vec![Point2::new(0.0, 0.0), Point2::new(1.0, 0.0)],
        weights: Some(vec![1.0, 2.0]),
        periodic: false,
    };
    let rational_parameter =
        crate::decode::closest_pcurve_parameters(&rational, Point2::new(0.5, 0.0), None).unwrap()
            [0];
    assert!((rational_parameter - 1.0 / 3.0).abs() < 1.0e-10);

    let quadratic = PcurveGeometry::Nurbs {
        degree: 2,
        knots: vec![0.0, 0.0, 0.0, 1.0, 1.0, 1.0],
        control_points: vec![
            Point2::new(0.0, 0.0),
            Point2::new(1.0, 1.0),
            Point2::new(2.0, 0.0),
        ],
        weights: None,
        periodic: false,
    };
    let quadratic_parameter =
        crate::decode::closest_pcurve_parameters(&quadratic, Point2::new(1.0, 0.5), None).unwrap()
            [0];
    assert!((quadratic_parameter - 0.5).abs() < 1.0e-10);

    let folded = PcurveGeometry::Nurbs {
        degree: 1,
        knots: vec![0.0, 0.0, 1.0, 2.0, 2.0],
        control_points: vec![
            Point2::new(0.0, 0.0),
            Point2::new(1.0, 0.0),
            Point2::new(0.0, 0.0),
        ],
        weights: None,
        periodic: false,
    };
    let first_fold =
        crate::decode::closest_pcurve_parameters(&folded, Point2::new(0.0, 0.0), Some(0.1))
            .unwrap()[0];
    let second_fold =
        crate::decode::closest_pcurve_parameters(&folded, Point2::new(0.0, 0.0), Some(1.9))
            .unwrap()[0];
    assert_eq!(first_fold, 0.0);
    assert_eq!(second_fold, 2.0);
    assert_eq!(
        crate::decode::closest_pcurve_parameters(&folded, Point2::new(0.0, 0.0), Some(0.1))
            .unwrap(),
        [0.0, 2.0]
    );
    assert_eq!(
        crate::decode::closest_pcurve_parameters(&folded, Point2::new(0.0, 0.0), Some(1.9))
            .unwrap(),
        [2.0, 0.0]
    );

    let mut rational_folded = folded.clone();
    let PcurveGeometry::Nurbs { weights, .. } = &mut rational_folded else {
        unreachable!("folded test pcurve is NURBS");
    };
    *weights = Some(vec![1.0; 3]);
    assert_eq!(
        crate::decode::closest_pcurve_parameters(
            &rational_folded,
            Point2::new(0.0, 0.0),
            Some(0.1),
        )
        .unwrap(),
        [0.0, 2.0]
    );
    assert_eq!(
        crate::decode::closest_pcurve_parameters(
            &rational_folded,
            Point2::new(0.0, 0.0),
            Some(1.9),
        )
        .unwrap(),
        [2.0, 0.0]
    );

    let quadratic_folded = PcurveGeometry::Nurbs {
        degree: 2,
        knots: vec![0.0, 0.0, 0.0, 1.0, 1.0, 1.0],
        control_points: vec![
            Point2::new(0.0, 0.0),
            Point2::new(1.0, 0.0),
            Point2::new(0.0, 0.0),
        ],
        weights: None,
        periodic: false,
    };
    assert_eq!(
        crate::decode::closest_pcurve_parameters(
            &quadratic_folded,
            Point2::new(0.0, 0.0),
            Some(0.1),
        )
        .unwrap(),
        [0.0, 1.0]
    );
    assert_eq!(
        crate::decode::closest_pcurve_parameters(
            &quadratic_folded,
            Point2::new(0.0, 0.0),
            Some(0.9),
        )
        .unwrap(),
        [1.0, 0.0]
    );
}

#[test]
fn blend_contact_offset_requires_the_radius_magnitude() {
    assert!(crate::decode::blend_contact_offset_matches(2.0, 5.0, 3.0));
    assert!(crate::decode::blend_contact_offset_matches(2.0, -1.0, 3.0));
    assert!(crate::decode::blend_contact_offset_matches(
        2.0,
        f64::from_bits(5.0f64.to_bits() + 1),
        3.0,
    ));
    assert!(!crate::decode::blend_contact_offset_matches(
        2.0, 5.001, 3.0
    ));
}

#[test]
fn blend_contact_matches_separate_analytic_offset_carriers() {
    use cadmpeg_ir::geometry::Surface;
    use cadmpeg_ir::ids::SurfaceId;
    use cadmpeg_ir::math::Point3;

    let mut ir = cadmpeg_ir::document::CadIr::empty(cadmpeg_ir::units::Units::default());
    let support = SurfaceId("synthetic:support-cylinder".into());
    let offset = SurfaceId("synthetic:offset-cylinder".into());
    let cylinder = |id, radius| Surface {
        id,
        geometry: SurfaceGeometry::Cylinder {
            origin: Point3::new(-46.75, 0.0, -112.06),
            axis: Vector3::new(1.0, 0.0, 0.0),
            ref_direction: Vector3::new(0.0, 0.0, -1.0),
            radius,
        },
        source_object: None,
    };
    ir.model.surfaces.extend([
        cylinder(support.clone(), 294.0),
        cylinder(offset.clone(), 299.0),
    ]);

    assert_eq!(
        crate::decode::constant_surface_offset_between(&ir, &support, &offset, 0),
        Some(5.0)
    );
    let SurfaceGeometry::Cylinder { origin, .. } = &mut ir.model.surfaces[1].geometry else {
        unreachable!()
    };
    origin.y = 1.0;
    assert!(crate::decode::constant_surface_offset_between(&ir, &support, &offset, 0).is_none());

    let support_plane = SurfaceId("synthetic:support-plane".into());
    let offset_plane = SurfaceId("synthetic:offset-plane".into());
    let plane = |id, origin| Surface {
        id,
        geometry: SurfaceGeometry::Plane {
            origin,
            normal: Vector3::new(0.0, 0.0, 1.0),
            u_axis: Vector3::new(1.0, 0.0, 0.0),
        },
        source_object: None,
    };
    ir.model.surfaces.extend([
        plane(support_plane.clone(), Point3::new(10.0, 20.0, 30.0)),
        plane(offset_plane.clone(), Point3::new(10.0, 20.0, 35.0)),
    ]);
    assert_eq!(
        crate::decode::constant_surface_offset_between(&ir, &support_plane, &offset_plane, 0),
        Some(5.0)
    );
    let SurfaceGeometry::Plane { origin, .. } = &mut ir.model.surfaces[3].geometry else {
        unreachable!()
    };
    origin.x += 1.0;
    assert!(
        crate::decode::constant_surface_offset_between(&ir, &support_plane, &offset_plane, 0)
            .is_none()
    );
}

#[test]
fn blend_contact_matches_concentric_blend_carriers() {
    use cadmpeg_ir::geometry::{BlendSupport, ProceduralSurface, Surface};
    use cadmpeg_ir::ids::{CurveId, ProceduralSurfaceId, SurfaceId};
    use cadmpeg_ir::math::Point3;

    let mut ir = cadmpeg_ir::document::CadIr::empty(cadmpeg_ir::units::Units::default());
    let first = SurfaceId("synthetic:first".into());
    let second = SurfaceId("synthetic:second".into());
    let first_offset = SurfaceId("synthetic:first-offset".into());
    let second_offset = SurfaceId("synthetic:second-offset".into());
    let plane = |id, origin, normal, u_axis| Surface {
        id,
        geometry: SurfaceGeometry::Plane {
            origin,
            normal,
            u_axis,
        },
        source_object: None,
    };
    ir.model.surfaces.extend([
        plane(
            first.clone(),
            Point3::new(0.0, 0.0, 0.0),
            Vector3::new(1.0, 0.0, 0.0),
            Vector3::new(0.0, 0.0, 1.0),
        ),
        plane(
            second.clone(),
            Point3::new(0.0, 0.0, 0.0),
            Vector3::new(0.0, 1.0, 0.0),
            Vector3::new(0.0, 0.0, 1.0),
        ),
        plane(
            first_offset.clone(),
            Point3::new(3.0, 0.0, 0.0),
            Vector3::new(1.0, 0.0, 0.0),
            Vector3::new(0.0, 0.0, 1.0),
        ),
        plane(
            second_offset.clone(),
            Point3::new(0.0, 3.0, 0.0),
            Vector3::new(0.0, 1.0, 0.0),
            Vector3::new(0.0, 0.0, 1.0),
        ),
    ]);

    let spine = CurveId("synthetic:shared-spine".into());
    let inner = SurfaceId("synthetic:inner-blend".into());
    let outer = SurfaceId("synthetic:outer-blend".into());
    for (surface, supports, radius) in [
        (inner.clone(), [first, second], 0.7),
        (outer.clone(), [first_offset, second_offset], 3.7),
    ] {
        let construction = ProceduralSurfaceId(format!("{}:construction", surface.0));
        ir.model.surfaces.push(Surface {
            id: surface.clone(),
            geometry: SurfaceGeometry::Procedural {
                construction: construction.clone(),
            },
            source_object: None,
        });
        ir.model.procedural_surfaces.push(ProceduralSurface {
            id: construction,
            surface,
            definition: ProceduralSurfaceDefinition::Blend {
                supports: supports.map(|surface| {
                    Some(BlendSupport {
                        surface,
                        reversed: false,
                    })
                }),
                spine: Some(spine.clone()),
                radius: BlendRadiusLaw::Constant {
                    signed_radius: radius,
                },
                cross_section: BlendCrossSection::Circular,
                native: None,
            },
            cache_fit_tolerance: None,
            record_bounds: None,
        });
    }

    assert_eq!(
        crate::decode::constant_surface_offset_between(&ir, &inner, &outer, 0),
        Some(3.0)
    );
    let outer_definition = ir
        .model
        .procedural_surfaces
        .iter_mut()
        .find(|candidate| candidate.surface == outer)
        .unwrap();
    let ProceduralSurfaceDefinition::Blend { supports, .. } = &mut outer_definition.definition
    else {
        unreachable!()
    };
    supports[0].as_mut().unwrap().reversed = true;
    assert!(crate::decode::constant_surface_offset_between(&ir, &inner, &outer, 0).is_none());
}

#[test]
fn closest_spine_parameter_inverts_periodic_analytic_curves() {
    use cadmpeg_ir::geometry::Curve;
    use cadmpeg_ir::ids::CurveId;
    use cadmpeg_ir::math::Point3;

    let mut ir = cadmpeg_ir::document::CadIr::empty(cadmpeg_ir::units::Units::default());
    let ellipse = CurveId("synthetic:ellipse-spine".into());
    let geometry = CurveGeometry::Ellipse {
        center: Point3::new(2.0, 3.0, 4.0),
        axis: Vector3::new(0.0, 1.0, 0.0),
        major_direction: Vector3::new(1.0, 0.0, 0.0),
        major_radius: 12.0,
        minor_radius: 5.0,
    };
    let parameter = 1.2;
    let mut point = cadmpeg_ir::eval::curve_point(&geometry, parameter).unwrap();
    point.y += 3.0;
    ir.model.curves.push(Curve {
        id: ellipse.clone(),
        geometry,
        source_object: None,
    });

    let first = crate::decode::closest_spine_parameter(&ir, &ellipse, point, None).unwrap();
    let continued = crate::decode::closest_spine_parameter(
        &ir,
        &ellipse,
        point,
        Some(parameter + std::f64::consts::TAU),
    )
    .unwrap();

    assert!((first - parameter).abs() < 1.0e-8, "{first}");
    assert!(
        (continued - parameter - std::f64::consts::TAU).abs() < 1.0e-8,
        "{continued}"
    );

    let center = Point3::new(2.0, 3.0, 4.0);
    let upper = crate::decode::closest_spine_parameter(&ir, &ellipse, center, Some(1.4)).unwrap();
    let lower = crate::decode::closest_spine_parameter(&ir, &ellipse, center, Some(4.8)).unwrap();
    assert!(
        (upper - std::f64::consts::FRAC_PI_2).abs() < 1.0e-8,
        "{upper}"
    );
    assert!(
        (lower - 3.0 * std::f64::consts::FRAC_PI_2).abs() < 1.0e-8,
        "{lower}"
    );
}

#[test]
fn rolling_ball_blend_parameters_invert_the_canal_surface_law() {
    use cadmpeg_ir::geometry::{
        BlendSupport, Curve, IntcurveSupportContext, IntcurveSupportSide, ProceduralCurve,
        ProceduralCurveDefinition, ProceduralSurface, Surface,
    };
    use cadmpeg_ir::ids::{
        CurveId, EdgeId, ProceduralCurveId, ProceduralSurfaceId, SurfaceId, VertexId,
    };
    use cadmpeg_ir::topology::Edge;

    let mut ir = cadmpeg_ir::document::CadIr::empty(cadmpeg_ir::units::Units::default());
    let first = SurfaceId("synthetic:first-plane".into());
    let second = SurfaceId("synthetic:second-plane".into());
    ir.model.surfaces.extend([
        Surface {
            id: first.clone(),
            geometry: SurfaceGeometry::Plane {
                origin: cadmpeg_ir::math::Point3::new(0.0, 0.0, 0.0),
                normal: Vector3::new(1.0, 0.0, 0.0),
                u_axis: Vector3::new(0.0, 0.0, 1.0),
            },
            source_object: None,
        },
        Surface {
            id: second.clone(),
            geometry: SurfaceGeometry::Plane {
                origin: cadmpeg_ir::math::Point3::new(0.0, 0.0, 0.0),
                normal: Vector3::new(0.0, 1.0, 0.0),
                u_axis: Vector3::new(0.0, 0.0, 1.0),
            },
            source_object: None,
        },
    ]);
    let first_spine_side = SurfaceId("synthetic:first-spine-side".into());
    let second_spine_side = SurfaceId("synthetic:second-spine-side".into());
    ir.model.surfaces.extend([
        Surface {
            id: first_spine_side.clone(),
            geometry: SurfaceGeometry::Plane {
                origin: cadmpeg_ir::math::Point3::new(2.0, 0.0, 0.0),
                normal: Vector3::new(1.0, 0.0, 0.0),
                u_axis: Vector3::new(0.0, 0.0, 1.0),
            },
            source_object: None,
        },
        Surface {
            id: second_spine_side.clone(),
            geometry: SurfaceGeometry::Plane {
                origin: cadmpeg_ir::math::Point3::new(0.0, 2.0, 0.0),
                normal: Vector3::new(0.0, 1.0, 0.0),
                u_axis: Vector3::new(0.0, 0.0, 1.0),
            },
            source_object: None,
        },
    ]);
    let spine = CurveId("synthetic:spine".into());
    ir.model.curves.push(Curve {
        id: spine.clone(),
        geometry: CurveGeometry::Line {
            origin: cadmpeg_ir::math::Point3::new(2.0, 2.0, 0.0),
            direction: Vector3::new(0.0, 0.0, 1.0),
        },
        source_object: None,
    });
    let surface = SurfaceId("synthetic:blend".into());
    let construction = ProceduralSurfaceId("synthetic:blend-construction".into());
    ir.model.surfaces.push(Surface {
        id: surface.clone(),
        geometry: SurfaceGeometry::Procedural {
            construction: construction.clone(),
        },
        source_object: None,
    });
    ir.model.procedural_surfaces.push(ProceduralSurface {
        id: construction,
        surface: surface.clone(),
        definition: ProceduralSurfaceDefinition::Blend {
            supports: [
                Some(BlendSupport {
                    surface: first.clone(),
                    reversed: false,
                }),
                Some(BlendSupport {
                    surface: second.clone(),
                    reversed: false,
                }),
            ],
            spine: Some(spine.clone()),
            radius: BlendRadiusLaw::Constant { signed_radius: 2.0 },
            cross_section: BlendCrossSection::Circular,
            native: None,
        },
        cache_fit_tolerance: None,
        record_bounds: None,
    });
    let expected = Point2::new(8.0, 0.35);
    let point = crate::decode::blend_surface_point(&ir, &surface, expected.u, expected.v).unwrap();

    assert_eq!(
        crate::decode::blend_spine_cache_fit_tolerance(&ir, &surface, 0.25),
        0.25
    );
    ir.model.procedural_curves.push(ProceduralCurve {
        id: ProceduralCurveId("synthetic:spine-construction".into()),
        curve: spine.clone(),
        definition: ProceduralCurveDefinition::Intersection {
            context: IntcurveSupportContext {
                sides: [
                    IntcurveSupportSide {
                        surface: Some(first_spine_side),
                        pcurve_parameter_range: None,
                        pcurve: Some(PcurveGeometry::Line {
                            origin: Point2::new(0.0, -2.0),
                            direction: Point2::new(1.0, 0.0),
                        }),
                    },
                    IntcurveSupportSide {
                        surface: Some(second_spine_side),
                        pcurve_parameter_range: None,
                        pcurve: Some(PcurveGeometry::Line {
                            origin: Point2::new(0.0, 2.0),
                            direction: Point2::new(1.0, 0.0),
                        }),
                    },
                ],
                parameter_range: [0.0, 10.0],
                discontinuities: [Vec::new(), Vec::new(), Vec::new()],
            },
            discontinuity_flag: false,
        },
        cache_fit_tolerance: Some(0.75),
    });
    assert_eq!(
        crate::decode::blend_spine_cache_fit_tolerance(&ir, &surface, 0.25),
        1.0
    );

    let actual = crate::decode::blend_surface_parameters(&ir, &surface, point, None).unwrap();

    assert!((actual.u - expected.u).abs() < 1.0e-8);
    assert!((actual.v - expected.v).abs() < 1.0e-8);
    let continued = crate::decode::blend_surface_parameters_for_fit(
        &ir,
        &surface,
        point,
        Some(Point2::new(expected.u + 0.1, expected.v - 0.05)),
        1.0e-8,
    )
    .unwrap();
    assert!((continued.u - expected.u).abs() < 1.0e-8);
    assert!((continued.v - expected.v).abs() < 1.0e-8);

    let mut varying_frame = ir.clone();
    varying_frame
        .model
        .curves
        .iter_mut()
        .find(|curve| curve.id == spine)
        .unwrap()
        .geometry = CurveGeometry::Parabola {
        vertex: cadmpeg_ir::math::Point3::new(2.0, 2.0, 0.0),
        axis: Vector3::new(0.0, 1.0, 0.0),
        major_direction: Vector3::new(1.0, 0.0, 0.0),
        focal_distance: 0.5,
    };
    let ProceduralCurveDefinition::Intersection { context, .. } = &mut varying_frame
        .model
        .procedural_curves
        .iter_mut()
        .find(|curve| curve.curve == spine)
        .unwrap()
        .definition
    else {
        unreachable!()
    };
    context.sides[0].pcurve = Some(PcurveGeometry::Offset {
        distance: 0.1,
        basis: Box::new(context.sides[0].pcurve.take().unwrap()),
    });
    let parameters = Point2::new(0.4, 0.35);
    let exact = crate::decode::blend_surface_u_derivative(
        &varying_frame,
        &surface,
        parameters.u,
        parameters.v,
        0,
    )
    .expect("complete rolling-ball frame has an exact derivative");
    let step = 1.0e-6;
    let before = crate::decode::blend_surface_point(
        &varying_frame,
        &surface,
        parameters.u - step,
        parameters.v,
    )
    .unwrap();
    let after = crate::decode::blend_surface_point(
        &varying_frame,
        &surface,
        parameters.u + step,
        parameters.v,
    )
    .unwrap();
    let numerical = Vector3::new(
        (after.x - before.x) / (2.0 * step),
        (after.y - before.y) / (2.0 * step),
        (after.z - before.z) / (2.0 * step),
    );
    assert!((exact.x - numerical.x).abs() < 1.0e-7);
    assert!((exact.y - numerical.y).abs() < 1.0e-7);
    assert!((exact.z - numerical.z).abs() < 1.0e-7);

    let mut translated = ir.clone();
    for carrier in &mut translated.model.surfaces {
        if let SurfaceGeometry::Plane { origin, .. } = &mut carrier.geometry {
            origin.x += 1.0e12;
            origin.y += 1.0e12;
            origin.z += 1.0e12;
        }
    }
    let CurveGeometry::Line { origin, .. } = &mut translated
        .model
        .curves
        .iter_mut()
        .find(|curve| curve.id == spine)
        .expect("translated spine")
        .geometry
    else {
        unreachable!()
    };
    origin.x += 1.0e12;
    origin.y += 1.0e12;
    origin.z += 1.0e12;
    let translated_point =
        crate::decode::blend_surface_point(&translated, &surface, expected.u, expected.v).unwrap();
    let translated_parameters = crate::decode::blend_surface_parameters_for_fit(
        &translated,
        &surface,
        translated_point,
        Some(Point2::new(expected.u + 0.1, expected.v - 0.05)),
        1.0e-3,
    )
    .expect("exact section tangent is independent of model-space magnitude");
    assert!((translated_parameters.u - expected.u).abs() < 1.0e-3);
    assert!((translated_parameters.v - expected.v).abs() < 1.0e-3);

    let boundary_curve = CurveId("synthetic:blend-boundary-curve".into());
    ir.model.procedural_curves.push(ProceduralCurve {
        id: ProceduralCurveId("synthetic:blend-boundary".into()),
        curve: boundary_curve.clone(),
        definition: ProceduralCurveDefinition::Intersection {
            context: IntcurveSupportContext {
                sides: [
                    IntcurveSupportSide {
                        surface: Some(first.clone()),
                        pcurve_parameter_range: None,
                        pcurve: Some(PcurveGeometry::Line {
                            origin: Point2::new(0.0, -2.0),
                            direction: Point2::new(1.0, 0.0),
                        }),
                    },
                    IntcurveSupportSide {
                        surface: Some(surface.clone()),
                        pcurve_parameter_range: None,
                        pcurve: None,
                    },
                ],
                parameter_range: [0.0, 1.0],
                discontinuities: [Vec::new(), Vec::new(), Vec::new()],
            },
            discontinuity_flag: false,
        },
        cache_fit_tolerance: None,
    });
    ir.model.edges.push(Edge {
        id: EdgeId("synthetic:blend-boundary-edge".into()),
        curve: Some(boundary_curve),
        start: VertexId("synthetic:blend-boundary-start".into()),
        end: VertexId("synthetic:blend-boundary-end".into()),
        param_range: Some([0.0, 1.0]),
        tolerance: Some(1.0e-8),
    });
    crate::decode::complete_intersection_pcurves_from_opposite_charts(&mut ir);
    let ProceduralCurveDefinition::Intersection { context, .. } =
        &ir.model.procedural_curves.last().unwrap().definition
    else {
        unreachable!()
    };
    let PcurveGeometry::Nurbs { control_points, .. } = context.sides[1].pcurve.as_ref().unwrap()
    else {
        unreachable!()
    };
    assert_eq!(control_points.first(), Some(&Point2::new(0.0, 0.0)));
    assert_eq!(control_points.last(), Some(&Point2::new(1.0, 0.0)));
    assert_eq!(
        crate::decode::blend_boundary_parameter_from_support_spine(
            &ir,
            &surface,
            &first,
            cadmpeg_ir::math::Point3::new(0.0, 2.0, 0.0),
            None,
            1.0e-8,
        ),
        Some(Point2::new(0.0, 0.0))
    );
    ir.model
        .procedural_curves
        .iter_mut()
        .find(|procedural| procedural.curve == spine)
        .unwrap()
        .definition = ProceduralCurveDefinition::Unknown {
        native_kind: None,
        record: None,
    };
    assert_eq!(
        crate::decode::blend_boundary_parameter_from_support_spine(
            &ir,
            &surface,
            &first,
            cadmpeg_ir::math::Point3::new(0.0, 2.0, 0.0),
            None,
            1.0e-8,
        ),
        Some(Point2::new(0.0, 0.0))
    );

    ir.model
        .curves
        .iter_mut()
        .find(|curve| curve.id == spine)
        .unwrap()
        .geometry = CurveGeometry::Nurbs(cadmpeg_ir::geometry::NurbsCurve {
        degree: 1,
        knots: vec![0.0, 0.0, 10.0, 10.0],
        control_points: vec![
            cadmpeg_ir::math::Point3::new(2.0, 2.0, 0.0),
            cadmpeg_ir::math::Point3::new(2.0, 2.0, 10.0),
        ],
        weights: None,
        periodic: false,
    });
    let coarse = crate::decode::coarse_blend_surface_parameters(&ir, &surface, point, 0).unwrap();
    let coarse_point =
        crate::decode::blend_surface_point(&ir, &surface, coarse.u, coarse.v).unwrap();
    assert!(
        ((coarse_point.x - point.x).powi(2)
            + (coarse_point.y - point.y).powi(2)
            + (coarse_point.z - point.z).powi(2))
        .sqrt()
            < 1.0
    );

    let refined = crate::decode::refine_blend_surface_parameters(
        &ir,
        &surface,
        point,
        Point2::new(expected.u + 0.5, expected.v + 0.1),
        0,
    )
    .unwrap();
    let refined_point =
        crate::decode::blend_surface_point(&ir, &surface, refined.u, refined.v).unwrap();
    let refined_error = ((refined_point.x - point.x).powi(2)
        + (refined_point.y - point.y).powi(2)
        + (refined_point.z - point.z).powi(2))
    .sqrt();
    assert!(refined_error < 1.0e-9);

    let third = SurfaceId("synthetic:third-plane".into());
    ir.model.surfaces.push(Surface {
        id: third.clone(),
        geometry: SurfaceGeometry::Plane {
            origin: cadmpeg_ir::math::Point3::new(0.0, 8.0, 0.0),
            normal: Vector3::new(0.0, 1.0, 0.0),
            u_axis: Vector3::new(0.0, 0.0, 1.0),
        },
        source_object: None,
    });
    let outer_spine = CurveId("synthetic:outer-spine".into());
    ir.model.curves.push(Curve {
        id: outer_spine.clone(),
        geometry: CurveGeometry::Line {
            origin: cadmpeg_ir::math::Point3::new(4.0, 6.0, 0.0),
            direction: Vector3::new(0.0, 0.0, 1.0),
        },
        source_object: None,
    });
    let outer = SurfaceId("synthetic:outer-blend".into());
    let outer_construction = ProceduralSurfaceId("synthetic:outer-blend-construction".into());
    ir.model.surfaces.push(Surface {
        id: outer.clone(),
        geometry: SurfaceGeometry::Procedural {
            construction: outer_construction.clone(),
        },
        source_object: None,
    });
    ir.model.procedural_surfaces.push(ProceduralSurface {
        id: outer_construction,
        surface: outer.clone(),
        definition: ProceduralSurfaceDefinition::Blend {
            supports: [
                Some(BlendSupport {
                    surface,
                    reversed: false,
                }),
                Some(BlendSupport {
                    surface: third,
                    reversed: false,
                }),
            ],
            spine: Some(outer_spine),
            radius: BlendRadiusLaw::Constant { signed_radius: 1.5 },
            cross_section: BlendCrossSection::Circular,
            native: None,
        },
        cache_fit_tolerance: None,
        record_bounds: None,
    });
    let expected = Point2::new(4.0, 0.2);
    let point = crate::decode::blend_surface_point(&ir, &outer, expected.u, expected.v).unwrap();
    let actual = crate::decode::blend_surface_parameters(&ir, &outer, point, None).unwrap();
    assert!((actual.u - expected.u).abs() < 1.0e-8);
    assert!((actual.v - expected.v).abs() < 1.0e-8);

    let outer_definition = ir
        .model
        .procedural_surfaces
        .iter_mut()
        .find(|candidate| candidate.surface == outer)
        .unwrap();
    let ProceduralSurfaceDefinition::Blend { supports, .. } = &mut outer_definition.definition
    else {
        panic!("blend definition");
    };
    supports[0].as_mut().unwrap().surface = outer.clone();
    assert!(crate::decode::blend_surface_point(&ir, &outer, expected.u, expected.v).is_none());
}

#[test]
fn decode_emits_both_intersection_support_pcurves() {
    let stream = two_support_charted_intersection_curve_stream();
    let mut cur = Cursor::new(prt_with_partition(&stream));
    let result = NxCodec.decode(&mut cur, &DecodeOptions::default()).unwrap();

    let cadmpeg_ir::geometry::ProceduralCurveDefinition::Intersection { context, .. } =
        &result.ir.model.procedural_curves[0].definition
    else {
        panic!("typed intersection");
    };
    assert!(context.sides[0].surface.is_some());
    assert!(context.sides[0].pcurve.is_some());
    assert!(context.sides[1].surface.is_some());
    assert!(context.sides[1].pcurve.is_some());
    assert!(cadmpeg_ir::validate::validate(&result.ir, Vec::new()).is_ok());
}

#[test]
fn decode_discards_serialized_support_uv_lane_that_misses_chart() {
    let stream =
        two_support_charted_intersection_curve_stream_with_second_plane_axis([0.0, 0.0, 1.0]);
    let mut cur = Cursor::new(prt_with_partition(&stream));
    let result = NxCodec.decode(&mut cur, &DecodeOptions::default()).unwrap();

    let cadmpeg_ir::geometry::ProceduralCurveDefinition::Intersection { context, .. } =
        &result.ir.model.procedural_curves[0].definition
    else {
        panic!("typed intersection");
    };
    assert!(context.sides[0].pcurve.is_some());
    let Some(PcurveGeometry::Nurbs { control_points, .. }) = context.sides[1].pcurve.as_ref()
    else {
        panic!("completed second support pcurve");
    };
    assert_eq!(control_points.first(), Some(&Point2::new(0.0, 0.0)));
    assert_eq!(control_points.last(), Some(&Point2::new(0.0, 10.0)));
    assert!(control_points.iter().all(|point| point.u == 0.0));
    assert!(cadmpeg_ir::validate::validate(&result.ir, Vec::new()).is_ok());
}

#[test]
fn intersection_support_order_follows_type_38_values_marker() {
    let mut stream = two_support_charted_intersection_curve_stream();
    let uv = stream
        .windows(8)
        .position(|window| window == [0, 204, 0, 0, 0, 8, 0, 23])
        .expect("support UV record");
    stream[uv + 8] = 3;

    let scan = crate::intersection::scan(&stream, crate::intersection::ChartPointLayout::Xyz3);
    let [curve] = scan.curves.as_slice() else {
        panic!("one charted intersection");
    };
    assert_eq!(curve.supports, [13, 6]);
}

#[test]
fn decode_retains_uncharted_intersection_without_inventing_a_range() {
    let mut stream = two_support_charted_intersection_curve_stream();
    let intersection = stream
        .windows(4)
        .position(|window| window == [0, 38, 0, 12])
        .expect("intersection record");
    for offset in [23, 25, 27] {
        put_ref(&mut stream, intersection + offset, 1);
    }
    let mut cur = Cursor::new(prt_with_partition(&stream));
    let result = NxCodec.decode(&mut cur, &DecodeOptions::default()).unwrap();

    let procedural = &result.ir.model.procedural_curves[0];
    let cadmpeg_ir::geometry::ProceduralCurveDefinition::TolerantIntersection {
        supports,
        parameterization,
        ..
    } = &procedural.definition
    else {
        panic!("typed tolerant intersection");
    };
    assert_ne!(supports[0], supports[1]);
    assert!(parameterization.is_none());
    let curve = result
        .ir
        .model
        .curves
        .iter()
        .find(|curve| curve.id == procedural.curve)
        .expect("intersection carrier");
    assert!(matches!(curve.geometry, CurveGeometry::Procedural { .. }));
    assert!(result
        .ir
        .model
        .edges
        .iter()
        .filter(|edge| edge.curve.as_ref() == Some(&procedural.curve))
        .all(|edge| edge.param_range.is_none()));
    assert!(cadmpeg_ir::validate::validate(&result.ir, Vec::new()).is_ok());
}

#[test]
fn terminal_plane_intersection_without_a_direct_carrier_remains_unresolved() {
    let mut stream = charted_intersection_with_edge_endpoint_witnesses_stream();
    let intersection = stream
        .windows(4)
        .position(|window| window == [0, 38, 0, 12])
        .expect("intersection record");
    put_ref(&mut stream, intersection + 21, 13);
    for offset in [23, 25, 27] {
        put_ref(&mut stream, intersection + offset, 1);
    }
    let second_support_source = two_support_charted_intersection_curve_stream();
    let second_support = second_support_source
        .windows(4)
        .position(|window| window == [0, 50, 0, 13])
        .expect("second plane");
    stream.extend_from_slice(&second_support_source[second_support..second_support + 91]);

    let mut cur = Cursor::new(prt_with_partition(&stream));
    let result = NxCodec.decode(&mut cur, &DecodeOptions::default()).unwrap();
    let procedural = &result.ir.model.procedural_curves[0];
    let cadmpeg_ir::geometry::ProceduralCurveDefinition::TolerantIntersection {
        parameterization: None,
        ..
    } = &procedural.definition
    else {
        panic!("unresolved tolerant intersection");
    };
    let edge = result
        .ir
        .model
        .edges
        .iter()
        .find(|edge| edge.curve.as_ref() == Some(&procedural.curve))
        .expect("carrying edge");
    assert_eq!(edge.param_range, None);
    assert!(cadmpeg_ir::validate::validate(&result.ir, Vec::new()).is_ok());
}

#[test]
fn terminal_cylinder_generator_without_a_direct_carrier_remains_unresolved() {
    let mut stream = charted_intersection_with_edge_endpoint_witnesses_stream();
    let intersection = stream
        .windows(4)
        .position(|window| window == [0, 38, 0, 12])
        .expect("intersection record");
    put_ref(&mut stream, intersection + 21, 13);
    for offset in [23, 25, 27] {
        put_ref(&mut stream, intersection + offset, 1);
    }
    let mut cylinder = record(51, 99);
    put_ref(&mut cylinder, 2, 13);
    cylinder[18] = b'+';
    put_vec3(&mut cylinder, 19, [0.0, -0.001, 0.0]);
    put_vec3(&mut cylinder, 43, [1.0, 0.0, 0.0]);
    put_f64(&mut cylinder, 67, 0.001);
    put_vec3(&mut cylinder, 75, [0.0, 1.0, 0.0]);
    stream.extend(cylinder);

    let mut cur = Cursor::new(prt_with_partition(&stream));
    let result = NxCodec.decode(&mut cur, &DecodeOptions::default()).unwrap();
    let procedural = &result.ir.model.procedural_curves[0];
    let cadmpeg_ir::geometry::ProceduralCurveDefinition::TolerantIntersection {
        parameterization: None,
        ..
    } = &procedural.definition
    else {
        panic!("unresolved tolerant intersection");
    };
    assert!(cadmpeg_ir::validate::validate(&result.ir, Vec::new()).is_ok());

    let second_point = stream
        .windows(4)
        .position(|window| window == [0, 29, 0, 15])
        .expect("second endpoint");
    put_vec3(&mut stream, second_point + 16, [0.01, -0.002, 0.0]);
    let mut cur = Cursor::new(prt_with_partition(&stream));
    let cross_branch = NxCodec.decode(&mut cur, &DecodeOptions::default()).unwrap();
    assert!(matches!(
        cross_branch.ir.model.procedural_curves[0].definition,
        cadmpeg_ir::geometry::ProceduralCurveDefinition::TolerantIntersection {
            parameterization: None,
            ..
        }
    ));
}

#[test]
fn terminal_cone_generator_without_a_direct_carrier_remains_unresolved() {
    let mut stream = charted_intersection_with_edge_endpoint_witnesses_stream();
    let intersection = stream
        .windows(4)
        .position(|window| window == [0, 38, 0, 12])
        .expect("intersection record");
    put_ref(&mut stream, intersection + 21, 13);
    for offset in [23, 25, 27] {
        put_ref(&mut stream, intersection + offset, 1);
    }
    let sin_half = 0.5;
    let cos_half = 3.0_f64.sqrt() * 0.5;
    let mut cone = record(52, 115);
    put_ref(&mut cone, 2, 13);
    cone[18] = b'+';
    put_vec3(&mut cone, 19, [-0.0005, -0.001 * cos_half, 0.0]);
    put_vec3(&mut cone, 43, [cos_half, -sin_half, 0.0]);
    put_f64(&mut cone, 67, 0.001);
    put_f64(&mut cone, 75, sin_half);
    put_f64(&mut cone, 83, cos_half);
    put_vec3(&mut cone, 91, [sin_half, cos_half, 0.0]);
    stream.extend(cone);

    let mut cur = Cursor::new(prt_with_partition(&stream));
    let result = NxCodec.decode(&mut cur, &DecodeOptions::default()).unwrap();
    let procedural = &result.ir.model.procedural_curves[0];
    let cadmpeg_ir::geometry::ProceduralCurveDefinition::TolerantIntersection {
        parameterization: None,
        ..
    } = &procedural.definition
    else {
        panic!("unresolved tolerant intersection");
    };
    assert!(cadmpeg_ir::validate::validate(&result.ir, Vec::new()).is_ok());
}

#[test]
fn terminal_sphere_and_torus_meridians_without_a_direct_carrier_remain_unresolved() {
    let terminal_stream = || {
        let mut stream = charted_intersection_with_edge_endpoint_witnesses_stream();
        let intersection = stream
            .windows(4)
            .position(|window| window == [0, 38, 0, 12])
            .expect("intersection record");
        put_ref(&mut stream, intersection + 21, 13);
        for offset in [23, 25, 27] {
            put_ref(&mut stream, intersection + offset, 1);
        }
        stream
    };
    let radial_height = (0.01_f64.powi(2) - 0.005_f64.powi(2)).sqrt();
    let mut sphere = record(53, 99);
    put_ref(&mut sphere, 2, 13);
    sphere[18] = b'+';
    put_vec3(&mut sphere, 19, [0.005, -radial_height, 0.0]);
    put_f64(&mut sphere, 43, 0.01);
    put_vec3(&mut sphere, 51, [1.0, 0.0, 0.0]);
    put_vec3(&mut sphere, 75, [0.0, 1.0, 0.0]);

    let mut torus = record(54, 107);
    put_ref(&mut torus, 2, 13);
    torus[18] = b'+';
    put_vec3(&mut torus, 19, [0.005, -0.03 - radial_height, 0.0]);
    put_vec3(&mut torus, 43, [1.0, 0.0, 0.0]);
    put_f64(&mut torus, 67, 0.03);
    put_f64(&mut torus, 75, 0.01);
    put_vec3(&mut torus, 83, [0.0, 1.0, 0.0]);

    for (family, record) in [("sphere", sphere), ("torus", torus)] {
        let mut stream = terminal_stream();
        stream.extend(record);
        let result = NxCodec
            .decode(
                &mut Cursor::new(prt_with_partition(&stream)),
                &DecodeOptions::default(),
            )
            .unwrap();
        let procedural = &result.ir.model.procedural_curves[0];
        let cadmpeg_ir::geometry::ProceduralCurveDefinition::TolerantIntersection {
            parameterization: None,
            ..
        } = &procedural.definition
        else {
            panic!("unresolved {family} meridian");
        };
        let edge = result
            .ir
            .model
            .edges
            .iter()
            .find(|edge| edge.curve.as_ref() == Some(&procedural.curve))
            .expect("carrying edge");
        assert_eq!(edge.param_range, None);
        assert!(cadmpeg_ir::validate::validate(&result.ir, Vec::new()).is_ok());
    }
}

#[test]
fn decode_emits_inline_descriptor_intersection_witnesses() {
    let stream = inline_descriptor_intersection_curve_stream();
    let mut cur = Cursor::new(prt_with_partition(&stream));
    let result = NxCodec.decode(&mut cur, &DecodeOptions::default()).unwrap();

    assert!(matches!(
        result.ir.model.procedural_curves[0].definition,
        cadmpeg_ir::geometry::ProceduralCurveDefinition::Intersection { .. }
    ));
    assert!(matches!(
        result
            .ir
            .model
            .curves
            .iter()
            .find(|curve| curve.id == result.ir.model.procedural_curves[0].curve)
            .expect("intersection curve")
            .geometry,
        CurveGeometry::Nurbs(_)
    ));
}

#[test]
fn decode_emits_topology_when_record_xmt_uses_extended_encoding() {
    let stream = large_xmt_headers(&topology_partition_stream());
    let mut cur = Cursor::new(prt_with_partition(&stream));
    let result = NxCodec.decode(&mut cur, &DecodeOptions::default()).unwrap();

    assert_eq!(result.ir.model.faces.len(), 1);
    assert_eq!(result.ir.model.edges.len(), 1);
    assert_eq!(result.ir.model.vertices.len(), 1);
    assert!(cadmpeg_ir::validate::validate(&result.ir, Vec::new()).is_ok());
}

#[test]
fn decode_maps_parasolid_tolerance_sentinel_to_none() {
    let stream = topology_with_missing_tolerances();
    let mut cur = Cursor::new(prt_with_partition(&stream));
    let result = NxCodec.decode(&mut cur, &DecodeOptions::default()).unwrap();

    assert_eq!(result.ir.model.vertices[0].tolerance, None);
    assert_eq!(result.ir.model.edges[0].tolerance, None);
    assert_eq!(result.ir.model.faces[0].tolerance, None);
}

#[test]
fn decode_dual_writes_inline_entity_metadata_to_annotations() {
    let mut cur = Cursor::new(topology_part_prt());
    let result = NxCodec.decode(&mut cur, &DecodeOptions::default()).unwrap();
    let ir = &result.ir;
    let annotations = &result.source_fidelity.annotations;

    macro_rules! assert_arena_annotations {
        ($arena:expr) => {
            for entity in $arena {
                let provenance = annotations
                    .provenance
                    .get(&entity.id.to_string())
                    .expect("annotation provenance");
                assert!(annotations.streams[provenance.stream as usize].starts_with("nx:"));
                assert!(provenance.tag.is_some());
            }
        };
    }

    assert_arena_annotations!(&ir.model.bodies);
    assert_arena_annotations!(&ir.model.regions);
    assert_arena_annotations!(&ir.model.shells);
    assert_arena_annotations!(&ir.model.faces);
    assert_arena_annotations!(&ir.model.loops);
    assert_arena_annotations!(&ir.model.coedges);
    assert_arena_annotations!(&ir.model.edges);
    assert_arena_annotations!(&ir.model.vertices);
    assert_arena_annotations!(&ir.model.points);
    assert_arena_annotations!(&ir.model.surfaces);
    assert_arena_annotations!(&ir.model.curves);
    let unknowns = ir.native_unknowns("nx").unwrap();
    assert_arena_annotations!(&unknowns);

    let point_note = &annotations.exactness[&ir.model.points[0].id.to_string()];
    assert_eq!(point_note.entity, Exactness::ByteExact);
    assert_eq!(point_note.fields["position"], Exactness::Derived);
    let surface_note = &annotations.exactness[&ir.model.surfaces[0].id.to_string()];
    assert_eq!(surface_note.fields["geometry"], Exactness::Derived);
    let curve_note = &annotations.exactness[&ir.model.curves[0].id.to_string()];
    assert_eq!(curve_note.fields["geometry"], Exactness::Derived);
    for id in [
        ir.model.vertices[0].id.to_string(),
        ir.model.edges[0].id.to_string(),
        ir.model.faces[0].id.to_string(),
    ] {
        assert_eq!(
            annotations.exactness[&id].fields["tolerance"],
            Exactness::Derived
        );
    }
}

#[test]
fn decode_transfers_bspline_surface_and_curve() {
    let stream = bspline_partition_stream();
    let mut cur = Cursor::new(prt_with_partition(&stream));
    let result = NxCodec.decode(&mut cur, &DecodeOptions::default()).unwrap();
    let surface = result
        .ir
        .model
        .surfaces
        .iter()
        .find_map(|surface| match &surface.geometry {
            SurfaceGeometry::Nurbs(surface) => Some(surface),
            _ => None,
        })
        .expect("B-spline surface");
    assert_eq!(surface.u_knots, vec![0.0, 0.0, 1.0, 1.0]);
    assert_eq!(surface.control_points.len(), 4);
    assert!((surface.control_points[1].y - 20.0).abs() < 1e-9);
    let curve = result
        .ir
        .model
        .curves
        .iter()
        .find_map(|curve| match &curve.geometry {
            CurveGeometry::Nurbs(curve) => Some(curve),
            _ => None,
        })
        .expect("B-spline curve");
    assert_eq!(curve.knots, vec![0.0, 0.0, 1.0, 1.0]);
    assert_eq!(curve.control_points.len(), 2);
    assert!((curve.control_points[1].x - 20.0).abs() < 1e-9);
}

#[test]
fn nurbs_decodes_extended_xmt_arrays_payload_and_long_surface_descriptor() {
    let surfaces = crate::nurbs::surfaces(&extended_bspline_surface_stream());
    assert_eq!(surfaces.len(), 1);
    let SurfaceGeometry::Nurbs(surface) = &surfaces[0].geometry else {
        panic!("expected NURBS surface");
    };
    assert_eq!(surface.u_knots, vec![0.0, 0.0, 1.0, 1.0]);
    assert_eq!(surface.v_knots, vec![0.0, 0.0, 1.0, 1.0]);
    assert_eq!(surface.control_points.len(), 4);
    assert_eq!(surface.control_points[3].y, 20.0);
}

#[test]
fn nurbs_decodes_escaped_surface_payload_envelope() {
    let mut stream = bspline_partition_stream();
    let payload = stream
        .windows(4)
        .position(|window| window == [0, 125, 0, 21])
        .expect("surface payload");
    stream.insert(payload + 2, 0xff);

    let surfaces = crate::nurbs::surfaces(&stream);
    assert_eq!(surfaces.len(), 1);
    let SurfaceGeometry::Nurbs(surface) = &surfaces[0].geometry else {
        panic!("expected NURBS surface");
    };
    assert_eq!(surface.control_points.len(), 4);
    assert_eq!(surface.control_points[3].y, 20.0);
}

#[test]
fn nurbs_coalesces_equivalent_surface_descriptor_representations() {
    let mut stream = bspline_partition_stream();
    let mut descriptor = record(126, 49);
    put_ref(&mut descriptor, 2, 20);
    put_ref(&mut descriptor, 6, 1);
    put_ref(&mut descriptor, 8, 1);
    put_ref(&mut descriptor, 12, 2);
    put_ref(&mut descriptor, 16, 2);
    descriptor[18] = 5;
    descriptor[19] = 5;
    descriptor[20..24].copy_from_slice(&2u32.to_be_bytes());
    descriptor[24..28].copy_from_slice(&2u32.to_be_bytes());
    let mut at = 34;
    for reference in [9, 30, 31, 32, 33] {
        put_ref(&mut descriptor, at, reference);
        at += 2;
        descriptor[at] = 0;
        at += 1;
    }
    stream.extend(descriptor);

    let surfaces = crate::nurbs::surfaces(&stream);
    assert_eq!(surfaces.len(), 1);
    let SurfaceGeometry::Nurbs(surface) = &surfaces[0].geometry else {
        panic!("expected NURBS surface");
    };
    assert_eq!(surface.control_points.len(), 4);
}

#[test]
fn nurbs_coalesces_equivalent_curve_descriptor_representations() {
    let mut stream = bspline_partition_stream();
    let mut descriptor = record(136, 30);
    put_ref(&mut descriptor, 2, 40);
    put_ref(&mut descriptor, 4, 1);
    put_ref(&mut descriptor, 8, 2);
    put_ref(&mut descriptor, 10, 3);
    put_ref(&mut descriptor, 14, 2);
    descriptor[16] = 5;
    descriptor[20] = 1;
    let mut at = 21;
    for reference in [41, 42, 43] {
        put_ref(&mut descriptor, at, reference);
        at += 2;
        descriptor[at] = 0;
        at += 1;
    }
    stream.extend(descriptor);

    let curves = crate::nurbs::curves(&stream);
    assert_eq!(curves.len(), 1);
    let CurveGeometry::Nurbs(curve) = &curves[0].geometry else {
        panic!("expected NURBS curve");
    };
    assert_eq!(curve.control_points.len(), 2);
}

#[test]
fn nurbs_decodes_escaped_curve_descriptor_and_payload_count() {
    let mut stream = bspline_partition_stream();
    let descriptor = stream
        .windows(4)
        .position(|window| window == [0, 136, 0, 40])
        .expect("curve descriptor");
    stream.insert(descriptor + 2, 0xff);
    let payload = stream
        .windows(4)
        .position(|window| window == [0, 135, 0, 41])
        .expect("curve payload");
    stream.insert(payload + 2, 0xff);
    stream.insert(payload + 10, 0xff);

    let curves = crate::nurbs::curves(&stream);
    assert_eq!(curves.len(), 1);
    let CurveGeometry::Nurbs(curve) = &curves[0].geometry else {
        panic!("expected NURBS curve");
    };
    assert_eq!(curve.control_points.len(), 2);
    assert_eq!(curve.control_points[1].x, 20.0);
}

#[test]
fn nurbs_compact_curve_descriptor_survives_a_status_prefix_collision() {
    let mut stream = bspline_partition_stream();
    let descriptor = stream
        .windows(4)
        .position(|window| window == [0, 136, 0, 40])
        .expect("curve descriptor");
    stream[descriptor + 17..descriptor + 21].copy_from_slice(&[0, 0, 0, 1]);

    assert_eq!(crate::nurbs::curves(&stream).len(), 1);
}

#[test]
fn nurbs_decodes_dimension_four_rational_curve() {
    let mut stream = bspline_partition_stream();
    let descriptor = stream
        .windows(4)
        .position(|window| window == [0, 136, 0, 40])
        .expect("curve descriptor");
    put_ref(&mut stream, descriptor + 10, 4);

    let payload = stream
        .windows(4)
        .position(|window| window == [0, 135, 0, 41])
        .expect("curve payload");
    let old_payload_len = 15 + 6 * 8;
    let mut rational_payload = record(135, 15 + 8 * 8);
    put_ref(&mut rational_payload, 2, 41);
    rational_payload[9..13].copy_from_slice(&8u32.to_be_bytes());
    for (index, value) in [0.0, 0.0, 0.0, 1.0, 0.04, 0.0, 0.0, 2.0]
        .into_iter()
        .enumerate()
    {
        put_f64(&mut rational_payload, 15 + index * 8, value);
    }
    stream.splice(payload..payload + old_payload_len, rational_payload);

    let curves = crate::nurbs::curves(&stream);
    assert_eq!(curves.len(), 1);
    let CurveGeometry::Nurbs(curve) = &curves[0].geometry else {
        panic!("expected NURBS curve");
    };
    assert_eq!(curve.weights.as_deref(), Some([1.0, 2.0].as_slice()));
    assert_eq!(curve.control_points[1].x, 20.0);
}

#[test]
fn decode_replaces_partition_bspline_surface_wrapper_from_deltas() {
    let partition = bspline_surface_replacement_partition_stream();
    let deltas = deltas_bspline_surface_wrapper_stream();
    let file = prt_with_streams(&[&partition, &deltas]);
    let mut cur = Cursor::new(file);
    let result = NxCodec.decode(&mut cur, &DecodeOptions::default()).unwrap();

    assert!(result.ir.model.surfaces.iter().any(|surface| matches!(
        &surface.geometry,
        SurfaceGeometry::Nurbs(nurbs)
            if nurbs.control_points.iter().any(|point| point.y == 30.0)
    )));
    assert!(cadmpeg_ir::validate::validate(&result.ir, Vec::new()).is_ok());
}

#[test]
fn decode_replaces_partition_bspline_curve_wrapper_from_deltas() {
    let partition = bspline_curve_replacement_partition_stream();
    let deltas = deltas_bspline_curve_wrapper_stream();
    let file = prt_with_streams(&[&partition, &deltas]);
    let mut cur = Cursor::new(file);
    let result = NxCodec.decode(&mut cur, &DecodeOptions::default()).unwrap();

    assert!(result.ir.model.curves.iter().any(|curve| matches!(
        &curve.geometry,
        CurveGeometry::Nurbs(nurbs)
            if nurbs.control_points.iter().any(|point| point.y == 10.0)
    )));
    assert!(cadmpeg_ir::validate::validate(&result.ir, Vec::new()).is_ok());
}

#[test]
fn decode_uses_partner_fin_vertex_for_edge_endpoint() {
    let mut cur = Cursor::new(prt_with_partition(
        &partnered_trimmed_topology_partition_stream(),
    ));
    let result = NxCodec.decode(&mut cur, &DecodeOptions::default()).unwrap();
    let edge = result.ir.model.edges.first().expect("edge");
    assert_ne!(edge.start, edge.end);
    assert_eq!(edge.param_range, Some([0.25, 0.75]));
    assert_eq!(result.ir.model.coedges.len(), 2);
    assert!(cadmpeg_ir::validate::validate(&result.ir, Vec::new()).is_ok());
}

#[test]
fn decode_resolves_forward_trimmed_curve_chain() {
    let mut cur = Cursor::new(prt_with_partition(&forward_trimmed_curve_chain_stream()));
    let result = NxCodec.decode(&mut cur, &DecodeOptions::default()).unwrap();
    let edge = result.ir.model.edges.first().expect("edge");
    assert_eq!(edge.curve.as_ref(), Some(&result.ir.model.curves[0].id));
    assert_eq!(edge.param_range, Some([0.25, 0.75]));
    assert!(cadmpeg_ir::validate::validate(&result.ir, Vec::new()).is_ok());
}

#[test]
fn decode_retains_a_curve_when_its_trim_range_misses_edge_vertices() {
    let mut cur = Cursor::new(prt_with_partition(
        &mismatched_trimmed_topology_partition_stream(),
    ));
    let result = NxCodec.decode(&mut cur, &DecodeOptions::default()).unwrap();
    let edge = result.ir.model.edges.first().expect("edge");
    let carrier = edge
        .curve
        .as_ref()
        .and_then(|id| result.ir.model.curves.iter().find(|curve| curve.id == *id))
        .expect("edge carrier");
    assert!(matches!(carrier.geometry, CurveGeometry::Line { .. }));
    assert_eq!(edge.param_range, None);
    assert!(cadmpeg_ir::validate::validate(&result.ir, Vec::new()).is_ok());
}

#[test]
fn decode_omits_overflowing_line_trim_range() {
    let mut stream = trimmed_topology_partition_stream();
    let trim = stream
        .windows(4)
        .position(|window| window == [0, 133, 0, 12])
        .expect("trimmed curve");
    put_f64(&mut stream, trim + 69, f64::MAX);

    let mut cur = Cursor::new(prt_with_partition(&stream));
    let result = NxCodec.decode(&mut cur, &DecodeOptions::default()).unwrap();
    assert_eq!(result.ir.model.edges[0].param_range, None);
    assert!(cadmpeg_ir::validate::validate(&result.ir, Vec::new()).is_ok());
}

#[test]
fn decode_resolves_extended_xmt_reference_inside_edge_record() {
    let mut cur = Cursor::new(prt_with_partition(
        &topology_with_extended_edge_curve_reference(),
    ));
    let result = NxCodec.decode(&mut cur, &DecodeOptions::default()).unwrap();
    assert_eq!(result.ir.model.edges.len(), 1);
    assert_eq!(
        result.ir.model.edges[0].curve.as_ref(),
        Some(&result.ir.model.curves[0].id)
    );
}

#[test]
fn decode_tracks_extended_face_reference_shift() {
    let mut cur = Cursor::new(prt_with_partition(
        &topology_with_extended_face_attribute_reference(),
    ));
    let result = NxCodec.decode(&mut cur, &DecodeOptions::default()).unwrap();

    assert_eq!(result.ir.model.faces.len(), 1);
    assert_eq!(result.ir.model.faces[0].tolerance, Some(0.2));
    assert_eq!(
        result.ir.model.faces[0].surface,
        result.ir.model.surfaces[0].id
    );
    assert!(cadmpeg_ir::validate::validate(&result.ir, Vec::new()).is_ok());
}

#[test]
fn decode_tracks_extended_edge_reference_shift() {
    let mut cur = Cursor::new(prt_with_partition(
        &topology_with_extended_edge_attribute_reference(),
    ));
    let result = NxCodec.decode(&mut cur, &DecodeOptions::default()).unwrap();

    assert_eq!(result.ir.model.edges.len(), 1);
    assert_eq!(result.ir.model.edges[0].tolerance, Some(0.3));
    assert_eq!(
        result.ir.model.edges[0].curve.as_ref(),
        Some(&result.ir.model.curves[0].id)
    );
}

#[test]
fn decode_tracks_all_extended_topology_reference_shifts() {
    let mut cur = Cursor::new(prt_with_partition(
        &topology_with_extended_internal_topology_references(),
    ));
    let result = NxCodec.decode(&mut cur, &DecodeOptions::default()).unwrap();

    assert_eq!(result.ir.model.bodies.len(), 1);
    assert_eq!(result.ir.model.shells.len(), 1);
    assert_eq!(result.ir.model.faces.len(), 1);
    assert_eq!(result.ir.model.loops.len(), 1);
    assert_eq!(result.ir.model.coedges.len(), 1);
    assert_eq!(result.ir.model.edges.len(), 1);
    assert_eq!(result.ir.model.vertices.len(), 1);
    assert_eq!(result.ir.model.vertices[0].tolerance, Some(0.1));
    assert_eq!(result.ir.model.points[0].position.x, 10.0);
    assert!(cadmpeg_ir::validate::validate(&result.ir, Vec::new()).is_ok());
}

#[test]
fn decode_tracks_fully_extended_geometry_header_shift() {
    let stream = topology_with_fully_extended_geometry_headers();
    let graph = crate::topology::Graph::parse(&stream);
    assert!(matches!(
        graph
            .get(50, 6)
            .and_then(super::topology::Node::surface_geometry),
        Some(SurfaceGeometry::Plane { .. })
    ));
    assert!(matches!(
        graph
            .get(30, 9)
            .and_then(super::topology::Node::curve_geometry),
        Some(CurveGeometry::Line { .. })
    ));

    let mut cur = Cursor::new(prt_with_partition(&stream));
    let result = NxCodec.decode(&mut cur, &DecodeOptions::default()).unwrap();

    assert_eq!(result.ir.model.faces.len(), 1);
    assert_eq!(result.ir.model.edges.len(), 1);
    assert!(matches!(
        result.ir.model.surfaces[0].geometry,
        SurfaceGeometry::Plane { .. }
    ));
    assert!(matches!(
        result.ir.model.curves[0].geometry,
        CurveGeometry::Line { .. }
    ));
}

#[test]
fn decode_tracks_geometry_envelope_escape_shift() {
    let mut cur = Cursor::new(prt_with_partition(
        &topology_with_escaped_geometry_envelopes(),
    ));
    let result = NxCodec.decode(&mut cur, &DecodeOptions::default()).unwrap();

    assert!(matches!(
        result.ir.model.surfaces[0].geometry,
        SurfaceGeometry::Plane { .. }
    ));
    assert!(matches!(
        result.ir.model.curves[0].geometry,
        CurveGeometry::Line { .. }
    ));
    assert!(cadmpeg_ir::validate::validate(&result.ir, Vec::new()).is_ok());
}

#[test]
fn analytic_scanner_accepts_positive_subnormal_radius() {
    let mut cy = record(0x33, 99);
    put_ref(&mut cy, 2, 2);
    cy[18] = b'+';
    put_vec3(&mut cy, 19, [0.003_175, 0.0, 0.0]);
    put_vec3(&mut cy, 43, [0.0, 0.0, 1.0]);
    put_f64(&mut cy, 67, f64::from_bits(1)); // smallest positive subnormal
    put_vec3(&mut cy, 75, [1.0, 0.0, 0.0]);
    assert_eq!(crate::geometry::surfaces(&cy).len(), 1);
}

#[test]
fn graph_owned_analytic_geometry_has_no_scanner_magnitude_limit() {
    let mut cylinder = record(0x33, 99);
    put_ref(&mut cylinder, 2, 2);
    cylinder[18] = b'+';
    put_vec3(&mut cylinder, 19, [1_001.0, 0.0, 0.0]);
    put_vec3(&mut cylinder, 43, [0.0, 0.0, 1.0]);
    put_f64(&mut cylinder, 67, f64::from_bits(1));
    put_vec3(&mut cylinder, 75, [1.0, 0.0, 0.0]);

    assert_eq!(crate::geometry::surfaces(&cylinder).len(), 1);
    let geometry =
        crate::geometry::decode_surface_record(&cylinder, 0x33, 0).expect("graph-owned cylinder");
    let SurfaceGeometry::Cylinder { origin, radius, .. } = geometry else {
        panic!("cylinder")
    };
    assert_eq!(origin.x, 1_001_000.0);
    assert_eq!(radius, f64::from_bits(1) * 1000.0);

    put_f64(&mut cylinder, 67, f64::INFINITY);
    assert!(crate::geometry::decode_surface_record(&cylinder, 0x33, 0).is_none());
}

#[test]
fn ellipse_requires_ordered_serialized_radii() {
    let mut ellipse = record(0x20, 107);
    put_ref(&mut ellipse, 2, 2);
    ellipse[18] = b'+';
    put_vec3(&mut ellipse, 19, [0.0, 0.0, 0.0]);
    put_vec3(&mut ellipse, 43, [0.0, 0.0, 1.0]);
    put_vec3(&mut ellipse, 67, [1.0, 0.0, 0.0]);
    put_f64(&mut ellipse, 91, 0.01);
    put_f64(&mut ellipse, 99, 0.01 + 5.0e-10);

    assert!(crate::geometry::curves(&ellipse).is_empty());
    assert!(crate::geometry::decode_curve_record(&ellipse, 0x20, 0).is_none());

    put_f64(&mut ellipse, 99, 0.01);
    assert_eq!(crate::geometry::curves(&ellipse).len(), 1);
}

#[test]
fn graph_owned_point_has_no_scanner_magnitude_limit() {
    let mut stream = topology_partition_stream();
    let point = stream
        .windows(4)
        .position(|window| window == [0, 29, 0, 11])
        .expect("point record");
    put_vec3(&mut stream, point + 16, [1_001.0, f64::from_bits(1), 0.0]);

    assert_eq!(crate::geometry::points(&stream).len(), 1);
    let graph = crate::topology::Graph::parse(&stream);
    assert_eq!(
        graph
            .get(29, 11)
            .and_then(crate::topology::Node::point_position),
        Some(cadmpeg_ir::math::Point3::new(
            1_001_000.0,
            f64::from_bits(1) * 1000.0,
            0.0,
        ))
    );

    put_vec3(&mut stream, point + 16, [f64::INFINITY, 0.0, 0.0]);
    assert!(crate::topology::Graph::parse(&stream).get(29, 11).is_none());
}

#[test]
fn decoded_tolerance_has_no_model_magnitude_limit() {
    assert_eq!(crate::decode::decoded_tolerance(1_001.0), Some(1_001_000.0));
    assert_eq!(crate::decode::decoded_tolerance(0.0), None);
    assert_eq!(crate::decode::decoded_tolerance(f64::INFINITY), None);
    assert_eq!(crate::decode::decoded_tolerance(f64::MAX), None);
}

#[test]
fn analytic_frame_gate_rejects_nonorthogonal_reference_direction() {
    let mut plane = record(0x32, 91);
    put_ref(&mut plane, 2, 2);
    plane[18] = b'+';
    put_vec3(&mut plane, 19, [0.0, 0.0, 0.0]);
    put_vec3(&mut plane, 43, [0.0, 0.0, 1.0]);
    put_vec3(&mut plane, 67, [0.0, 0.0, 1.0]);
    assert!(crate::geometry::surfaces(&plane).is_empty());

    put_vec3(&mut plane, 67, [1.0, 0.0, 0.0]);
    assert_eq!(crate::geometry::surfaces(&plane).len(), 1);
}

#[test]
fn analytic_scanner_does_not_rescan_a_complete_invalid_frame() {
    let mut stream = vec![0; 91];
    stream[1] = 0x32;
    put_ref(&mut stream, 2, 2);
    stream[18] = b'+';

    // A valid LINE-looking record begins inside the complete PLANE frame. The
    // outer plane remains invalid because its normal reads the line origin.
    stream[24] = 0;
    stream[25] = 0x1e;
    put_ref(&mut stream, 26, 3);
    stream[42] = b'+';
    put_vec3(&mut stream, 43, [0.0, 0.0, 0.0]);
    put_vec3(&mut stream, 67, [1.0, 0.0, 0.0]);

    assert!(crate::geometry::surfaces(&stream).is_empty());
    assert!(crate::geometry::curves(&stream).is_empty());
}

#[test]
fn cone_gate_rejects_nonfinite_or_degenerate_half_angle() {
    let mut cone = record(0x34, 115);
    put_ref(&mut cone, 2, 2);
    cone[18] = b'+';
    put_vec3(&mut cone, 19, [0.0, 0.0, 0.0]);
    put_vec3(&mut cone, 43, [0.0, 0.0, 1.0]);
    put_f64(&mut cone, 67, 0.0);
    put_f64(&mut cone, 75, std::f64::consts::FRAC_1_SQRT_2);
    put_f64(&mut cone, 83, std::f64::consts::FRAC_1_SQRT_2);
    put_vec3(&mut cone, 91, [1.0, 0.0, 0.0]);
    assert_eq!(crate::geometry::surfaces(&cone).len(), 1);

    for (sine, cosine) in [(f64::NAN, 1.0), (0.0, 1.0), (1.0, 0.0)] {
        put_f64(&mut cone, 75, sine);
        put_f64(&mut cone, 83, cosine);
        assert!(crate::geometry::surfaces(&cone).is_empty());
    }
}

#[test]
fn analytic_scanners_include_extended_reference_shifts_in_record_ownership() {
    let mut surfaces = vec![0; 184];
    surfaces[1] = 0x32;
    surfaces[2..6].copy_from_slice(&encoded_xmt(32_768));
    surfaces[20] = b'+';
    put_vec3(&mut surfaces, 21, [0.0, 0.0, 0.0]);
    put_vec3(&mut surfaces, 45, [0.0, 0.0, 1.0]);
    put_vec3(&mut surfaces, 69, [1.0, 0.0, 0.0]);
    surfaces[93] = 0;
    surfaces[94] = 0x32;
    put_ref(&mut surfaces, 95, 3);
    surfaces[111] = b'+';
    put_vec3(&mut surfaces, 112, [0.0, 0.0, 0.0]);
    put_vec3(&mut surfaces, 136, [0.0, 0.0, 1.0]);
    put_vec3(&mut surfaces, 160, [1.0, 0.0, 0.0]);
    assert_eq!(crate::geometry::surfaces(&surfaces).len(), 2);

    let mut curves = vec![0; 136];
    curves[1] = 0x1e;
    curves[2..6].copy_from_slice(&encoded_xmt(32_768));
    curves[20] = b'+';
    put_vec3(&mut curves, 21, [0.0, 0.0, 0.0]);
    put_vec3(&mut curves, 45, [1.0, 0.0, 0.0]);
    curves[69] = 0;
    curves[70] = 0x1e;
    put_ref(&mut curves, 71, 3);
    curves[87] = b'+';
    put_vec3(&mut curves, 88, [0.0, 0.0, 0.0]);
    put_vec3(&mut curves, 112, [1.0, 0.0, 0.0]);
    assert_eq!(crate::geometry::curves(&curves).len(), 2);
}

#[test]
fn analytic_scanner_resolves_envelope_escape_framing() {
    let mut plane = vec![0; 92];
    plane[1] = 0x32;
    plane[2] = 0xff;
    put_ref(&mut plane, 3, 2);
    plane[19] = b'+';
    put_vec3(&mut plane, 20, [0.0, 0.0, 0.0]);
    put_vec3(&mut plane, 44, [0.0, 0.0, 1.0]);
    put_vec3(&mut plane, 68, [1.0, 0.0, 0.0]);

    assert_eq!(crate::geometry::surfaces(&plane).len(), 1);
}

#[test]
fn analytic_record_ownership_is_shared_across_carrier_families() {
    let mut stream = vec![0; 158];
    stream[1] = 0x1e;
    put_ref(&mut stream, 2, 2);
    stream[18] = b'+';
    put_vec3(&mut stream, 19, [0.0, 0.0, 0.0]);
    put_vec3(&mut stream, 43, [1.0, 0.0, 0.0]);

    stream[67] = 0;
    stream[68] = 0x32;
    put_ref(&mut stream, 69, 3);
    stream[85] = b'+';
    put_vec3(&mut stream, 86, [0.0, 0.0, 0.0]);
    put_vec3(&mut stream, 110, [0.0, 0.0, 1.0]);
    put_vec3(&mut stream, 134, [1.0, 0.0, 0.0]);

    assert_eq!(crate::geometry::curves(&stream).len(), 1);
    assert_eq!(crate::geometry::surfaces(&stream).len(), 1);
    assert!(crate::geometry::points(&stream).is_empty());
}

#[test]
fn decode_assembly_reports_external_dependency() {
    let mut cur = Cursor::new(assembly_prt());
    let result = NxCodec.decode(&mut cur, &DecodeOptions::default()).unwrap();
    assert!(!result.report.geometry_transferred);
    assert!(result
        .report
        .losses
        .iter()
        .any(|l| l.message.contains("assembly")));
}

#[test]
fn external_reference_string_table_is_end_anchored() {
    let table = b"prefix\x01\x02\x00\x00\x00\x09\x00child.prt\x0c\x00nested/b.prt";
    let (_, strings) = crate::container::parse_extref_string_table(table).expect("string table");
    assert_eq!(
        strings
            .into_iter()
            .map(|(_, value)| value)
            .collect::<Vec<_>>(),
        ["child.prt", "nested/b.prt"]
    );

    let mut trailed = table.to_vec();
    trailed.push(0);
    assert!(crate::container::parse_extref_string_table(&trailed).is_none());
    assert!(crate::container::parse_extref_string_table(b"\x01\xff\xff\xff\xff").is_none());
}

#[test]
fn external_reference_record_parser_requires_sorted_doubled_handle_set() {
    let mut payload = b"EXTREFSTREAM".to_vec();
    payload.extend_from_slice(&3u32.to_le_bytes());
    payload.extend_from_slice(&0u32.to_le_bytes());
    payload.extend_from_slice(&0u32.to_le_bytes());
    payload.push(0);
    payload.extend_from_slice(&6u32.to_le_bytes());
    payload.extend_from_slice(&41u32.to_le_bytes());
    payload.extend_from_slice(&0u32.to_le_bytes());
    payload.extend_from_slice(&0u32.to_le_bytes());
    assert_eq!(payload.len(), 41);
    payload.extend_from_slice(&[1, 0, 0, 0]);
    payload.extend_from_slice(&2u16.to_be_bytes());
    payload.push(1);
    for value in [8u32, 11, 12, 4] {
        payload.extend_from_slice(&value.to_le_bytes());
    }
    payload.extend_from_slice(&[1, 4]);
    for handle in [0x1020_3040u32, 0x2030_4050, 0x2030_4050] {
        payload.push(0xe0);
        payload.extend_from_slice(&handle.to_be_bytes());
    }
    payload.push(4);
    payload.extend_from_slice(b"\x01\x01\x00\x00\x00\x09\x00child.prt");

    let records = crate::container::parse_extref_records(&payload);
    let indexed = crate::container::parse_extref_record_index(&payload).expect("record index");
    assert_eq!(indexed.len(), 1);
    assert_eq!(indexed[0].record_id, 6);
    assert_eq!(indexed[0].offset, 41);
    assert_eq!(indexed[0].byte_len, 41);
    assert_eq!(records.len(), 1);
    assert_eq!(records[0].record_id, 6);
    assert_eq!(records[0].declared_count, 2);
    assert_eq!(records[0].id_slots, [8, 11, 12, 4]);
    assert_eq!(records[0].handles, [0x1020_3040, 0x2030_4050]);
    assert!(records[0].closing_duplicate);
    assert_eq!(records[0].tail_byte_len, 0);

    let duplicate = payload
        .windows(5)
        .rposition(|window| window == [0xe0, 0x20, 0x30, 0x40, 0x50])
        .expect("closing duplicate");
    payload[duplicate + 1] = 0x10;
    assert!(crate::container::parse_extref_records(&payload).is_empty());
    assert_eq!(
        crate::container::parse_extref_record_index(&payload)
            .expect("opaque indexed record")
            .len(),
        1
    );
}

#[test]
fn external_reference_empty_record_parser_requires_the_complete_form() {
    assert_eq!(
        crate::container::parse_extref_empty_record(&[1, 0, 0, 0, 0, 1]),
        Some(false)
    );
    assert_eq!(
        crate::container::parse_extref_empty_record(&[1, 0, 0, 0, 0, 1, 1]),
        Some(true)
    );
    assert_eq!(
        crate::container::parse_extref_empty_record(&[1, 0, 0, 0, 0, 1, 0]),
        None
    );
    assert_eq!(
        crate::container::parse_extref_empty_record(&[1, 0, 0, 0, 0]),
        None
    );
}

#[test]
fn external_reference_tail_pairs_require_adjacent_complete_tokens() {
    let bytes = [
        0xff, 0xe0, 0x12, 0x34, 0x56, 0x78, 0xca, 0xbc, 0xde, 0xf0, 0xe0, 0x00, 0x00, 0x00, 0x01,
        0x00,
    ];
    assert_eq!(
        crate::container::parse_extref_reference_pairs(&bytes),
        vec![(1, 0x1234_5678, 0x0abc_def0)]
    );
    assert!(crate::container::parse_extref_reference_pairs(&bytes[10..]).is_empty());
}

#[test]
fn container_reads_rmfastload_active_ids() {
    let container = container::scan_bytes(rmfastload_prt()).unwrap();
    let (entry, table) = container
        .rmfastload_object_id_table()
        .expect("RMFastLoad object-id table");
    assert_eq!(entry.name, "/Root/FastLoad/RMFastLoad");
    assert_eq!(table.registry_offset, 0);
    assert_eq!(table.count_offset, b"UGS::Solid::Topol".len());
    assert_eq!(table.raw_count, 50u32.to_le_bytes());
    assert_eq!(
        table
            .object_ids
            .iter()
            .map(|object_id| object_id.value)
            .collect::<Vec<_>>(),
        (1..=50).collect::<Vec<_>>()
    );
    assert_eq!(table.object_ids[0].offset, table.count_offset + 4);
    assert_eq!(table.object_ids[0].raw, 1u32.to_le_bytes());
    assert_eq!(table.object_ids[49].offset, table.count_offset + 4 + 49 * 4);
    assert_eq!(table.object_ids[49].raw, 50u32.to_le_bytes());
}

#[test]
fn container_reads_rmfastload_table_from_product_boundary_without_range_floor() {
    let mut payload = b"UGS::Solid::Topol".to_vec();
    append_rmfastload_table(&mut payload, [0, u32::MAX, 7]);
    let file = prt_with_named_payloads(&[("/Root/FastLoad/RMFastLoad", payload)]);
    let container = container::scan_bytes(file).unwrap();
    let (_, table) = container
        .rmfastload_object_id_table()
        .expect("product-bounded RMFastLoad table");
    assert_eq!(table.object_ids.len(), 3);
    assert_eq!(
        table
            .object_ids
            .iter()
            .map(|object_id| object_id.value)
            .collect::<Vec<_>>(),
        [0, u32::MAX, 7]
    );
}

#[test]
fn container_bounds_rmfastload_table_at_its_first_product_record() {
    let mut payload = b"UGS::Solid::Topol".to_vec();
    append_rmfastload_table(&mut payload, [1, 2, 3]);
    append_rmfastload_table(&mut payload, [4, 5]);
    let file = prt_with_named_payloads(&[("/Root/FastLoad/RMFastLoad", payload)]);
    let container = container::scan_bytes(file).unwrap();
    let (_, table) = container
        .rmfastload_object_id_table()
        .expect("first product-bounded table");
    assert_eq!(
        table
            .object_ids
            .iter()
            .map(|object_id| object_id.value)
            .collect::<Vec<_>>(),
        [1, 2, 3]
    );
}

#[test]
fn decode_retains_every_rmfastload_active_body() {
    let mut cur = Cursor::new(prt_with_two_active_bodies_and_rmfastload());
    let result = NxCodec.decode(&mut cur, &DecodeOptions::default()).unwrap();

    assert_eq!(result.ir.model.bodies.len(), 2);
    assert_eq!(result.ir.model.faces.len(), 100);
    assert_eq!(
        result
            .ir
            .source
            .as_ref()
            .and_then(|source| source.attributes.get("rmfastload_active_body_count"))
            .map(String::as_str),
        Some("2")
    );
    assert!(result
        .report
        .losses
        .iter()
        .all(|loss| !loss.message.contains("sub-body partition")));
    assert!(cadmpeg_ir::validate::validate(&result.ir, Vec::new()).is_ok());
}

#[test]
fn decode_resolves_all_terminal_feature_bodies_without_active_selection() {
    let file = prt_with_two_terminal_bodies();
    assert_eq!(extract_streams(&file).len(), 2);
    let mut cur = Cursor::new(file);
    let result = NxCodec.decode(&mut cur, &DecodeOptions::default()).unwrap();

    assert_eq!(result.ir.model.bodies.len(), 2);
    assert_eq!(
        result
            .ir
            .source
            .as_ref()
            .and_then(|source| source.attributes.get("active_body_selector"))
            .map(String::as_str),
        Some("terminal_feature_body_lineage")
    );
    assert_eq!(
        result
            .ir
            .source
            .as_ref()
            .and_then(|source| source.attributes.get("feature_terminal_body_count"))
            .map(String::as_str),
        Some("2")
    );
    assert!(result
        .report
        .losses
        .iter()
        .all(|loss| !loss.message.contains("sub-body partition")));
    assert!(cadmpeg_ir::validate::validate(&result.ir, Vec::new()).is_ok());
}

#[test]
fn decode_selects_active_shell_when_body_record_is_absent() {
    let mut cur = Cursor::new(prt_with_missing_active_body_record());
    let result = NxCodec.decode(&mut cur, &DecodeOptions::default()).unwrap();

    assert_eq!(result.ir.model.bodies.len(), 1);
    assert!(result.ir.model.bodies[0].id.0.starts_with("nx:s0:"));
    assert_eq!(result.ir.model.faces.len(), 50);
    assert!(result
        .report
        .losses
        .iter()
        .all(|loss| !loss.message.contains("sub-body partition")));
    assert!(cadmpeg_ir::validate::validate(&result.ir, Vec::new()).is_ok());
}

#[test]
fn decode_keeps_bodies_when_rmfastload_overlap_is_weak() {
    let mut cur = Cursor::new(prt_with_weak_rmfastload_overlap());
    let result = NxCodec.decode(&mut cur, &DecodeOptions::default()).unwrap();

    assert_eq!(result.ir.model.bodies.len(), 2);
    assert!(result
        .ir
        .source
        .as_ref()
        .is_none_or(|source| !source.attributes.contains_key("active_body_selector")));
    assert!(result
        .report
        .losses
        .iter()
        .any(|loss| loss.message.contains("sub-body partition")));
}

#[test]
fn container_only_preserves_streams_without_geometry() {
    let mut cur = Cursor::new(single_part_prt());
    let opts = options_in(DecodeMode::Salvage, true);
    let result = NxCodec.decode(&mut cur, &opts).unwrap();
    assert!(!result.report.geometry_transferred);
    assert!(result.report.container_only);
    assert_eq!(result.ir.native_unknowns("nx").unwrap().len(), 1);
    assert!(result.ir.model.points.is_empty());
}

#[test]
fn inspect_enumerates_streams_and_names_schema() {
    let mut cur = Cursor::new(single_part_prt());
    let summary = NxCodec
        .inspect(&mut cur, &InspectOptions::default())
        .unwrap();
    assert_eq!(summary.format, "nx");
    assert_eq!(summary.container_kind, "splmsstr");
    assert!(summary.entries.iter().any(|e| e.role == "parasolid-stream"));
    assert!(summary.notes.iter().any(|n| n.contains("partition")));
}

#[test]
fn inspect_classifies_named_container_streams() {
    let file = prt_with_named_payloads(&[
        ("/Root/FastLoad/RMFastLoad", vec![1]),
        ("/Root/FastLoad/Structure", vec![2]),
        ("/Root/FastLoad/JT", vec![3]),
        ("/Root/UG_PART/DisplayJT", vec![4]),
        ("/Root/UG_PART/ExternalReferences", vec![5]),
        ("/Root/UG_PART/LastSavedToggleInfoStream", vec![6]),
        ("/Root/images/preview", vec![7]),
        ("/Root/materialsTif/Steel", vec![8]),
        ("/Root/part/arrangements", vec![9]),
        ("/Root/part/attrs", vec![10]),
        ("/Root/qafmetadata", vec![11]),
        ("/Root/vendor/private", vec![12]),
    ]);
    let summary = NxCodec
        .inspect(&mut Cursor::new(file), &InspectOptions::default())
        .unwrap();
    let roles = summary
        .entries
        .iter()
        .map(|entry| (entry.name.as_str(), entry.role.as_str()))
        .collect::<std::collections::BTreeMap<_, _>>();
    assert_eq!(roles["/Root/FastLoad/RMFastLoad"], "active-body-index");
    assert_eq!(roles["/Root/FastLoad/Structure"], "fast-load-structure");
    assert_eq!(roles["/Root/FastLoad/JT"], "fast-load-jt");
    assert_eq!(roles["/Root/UG_PART/DisplayJT"], "display-jt");
    assert_eq!(
        roles["/Root/UG_PART/ExternalReferences"],
        "external-references"
    );
    assert_eq!(
        roles["/Root/UG_PART/LastSavedToggleInfoStream"],
        "save-toggle-info"
    );
    assert_eq!(roles["/Root/images/preview"], "preview-image");
    assert_eq!(roles["/Root/materialsTif/Steel"], "material-texture");
    assert_eq!(roles["/Root/part/arrangements"], "arrangements");
    assert_eq!(roles["/Root/part/attrs"], "part-attributes");
    assert_eq!(roles["/Root/qafmetadata"], "asset-catalog");
    assert_eq!(roles["/Root/vendor/private"], "named-opaque-stream");
}

#[test]
fn decode_retains_unsupported_named_stream_payloads() {
    let structure = b"opaque fast-load structure".to_vec();
    let fast_load_jt = b"opaque fast-load JT".to_vec();
    let toggle = b"opaque save-toggle state".to_vec();
    let vendor = b"opaque vendor stream".to_vec();
    let file = prt_with_named_payloads(&[
        ("/Root/FastLoad/Structure", structure.clone()),
        ("/Root/FastLoad/JT", fast_load_jt.clone()),
        ("/Root/UG_PART/LastSavedToggleInfoStream", toggle.clone()),
        ("/Root/vendor/private", vendor.clone()),
    ]);
    let result = NxCodec
        .decode(&mut Cursor::new(file), &DecodeOptions::default())
        .unwrap();
    let unknowns = result.ir.native_unknowns("nx").unwrap();
    assert_eq!(unknowns.len(), 4);
    assert_eq!(
        result
            .source_fidelity
            .retained_records
            .iter()
            .map(|record| record.byte_len)
            .collect::<Vec<_>>(),
        vec![
            structure.len() as u64,
            fast_load_jt.len() as u64,
            toggle.len() as u64,
            vendor.len() as u64
        ]
    );
    assert!(unknowns
        .iter()
        .all(|unknown| unknown.id.0.starts_with("nx:container-entry:opaque#")));
    for name in [
        "/Root/FastLoad/Structure",
        "/Root/FastLoad/JT",
        "/Root/UG_PART/LastSavedToggleInfoStream",
        "/Root/vendor/private",
    ] {
        assert!(result
            .report
            .losses
            .iter()
            .any(|loss| loss.message.contains(name)));
    }
    assert!(cadmpeg_ir::validate::validate(&result.ir, Vec::new()).is_ok());
}

#[test]
fn design_intent_losses_distinguish_native_and_sketch_gaps() {
    use cadmpeg_ir::document::CadIr;
    use cadmpeg_ir::features::{
        BooleanOp, ConfigurationBodies, ConfigurationId, DesignConfiguration, Feature,
        FeatureDefinition, FeatureId,
    };

    let mut ir = CadIr::empty(cadmpeg_ir::units::Units::default());
    for (ordinal, kind) in ["DELETE", "DELETE"].into_iter().enumerate() {
        ir.model.features.push(Feature {
            id: FeatureId(format!("test:feature#{ordinal}")),
            ordinal: ordinal as u64,
            name: None,
            suppressed: None,
            parent: None,
            dependencies: Vec::new(),
            source_properties: Default::default(),
            source_tag: None,
            source_text: None,
            source_content: Vec::new(),
            outputs: Vec::new(),
            definition: FeatureDefinition::Native {
                kind: kind.to_string(),
                parameters: Default::default(),
                properties: Default::default(),
            },
            native_ref: None,
        });
    }
    ir.model.features.push(Feature {
        id: FeatureId("test:feature#sketch".into()),
        ordinal: 3,
        name: None,
        suppressed: None,
        parent: None,
        dependencies: Vec::new(),
        source_properties: Default::default(),
        source_tag: None,
        source_text: None,
        source_content: Vec::new(),
        outputs: Vec::new(),
        definition: FeatureDefinition::Sketch {
            space: cadmpeg_ir::features::SketchSpace::Unresolved,
            sketch: None,
        },
        native_ref: None,
    });
    ir.model.features.push(Feature {
        id: FeatureId("test:feature#incomplete-delete".into()),
        ordinal: 10,
        name: None,
        suppressed: Some(false),
        parent: None,
        dependencies: Vec::new(),
        source_properties: Default::default(),
        source_tag: None,
        source_text: None,
        source_content: Vec::new(),
        outputs: Vec::new(),
        definition: FeatureDefinition::DeleteBody {
            bodies: cadmpeg_ir::features::BodySelection::Unresolved,
            mode: cadmpeg_ir::features::BodyRetentionMode::DeleteSelected,
        },
        native_ref: None,
    });
    for (ordinal, definition) in [
        FeatureDefinition::DatumPlaneUnresolved,
        FeatureDefinition::DatumCoordinateSystemUnresolved,
        FeatureDefinition::LoftUnresolved,
        FeatureDefinition::FreeformSurfaceUnresolved,
        FeatureDefinition::LoftUnresolved,
    ]
    .into_iter()
    .enumerate()
    {
        ir.model.features.push(Feature {
            id: FeatureId(format!("test:feature#unresolved-{ordinal}")),
            ordinal: ordinal as u64 + 4,
            name: None,
            suppressed: None,
            parent: None,
            dependencies: Vec::new(),
            source_properties: Default::default(),
            source_tag: None,
            source_text: None,
            source_content: Vec::new(),
            outputs: Vec::new(),
            definition,
            native_ref: None,
        });
    }
    ir.model.features.push(Feature {
        id: FeatureId("test:feature#incomplete-block".into()),
        ordinal: 9,
        name: None,
        suppressed: None,
        parent: None,
        dependencies: Vec::new(),
        source_properties: Default::default(),
        source_tag: None,
        source_text: None,
        source_content: Vec::new(),
        outputs: Vec::new(),
        definition: FeatureDefinition::Block {
            dimensions: None,
            placement: None,
            op: BooleanOp::Unresolved,
        },
        native_ref: None,
    });
    ir.model.features.push(Feature {
        id: FeatureId("test:feature#incomplete-sweep".into()),
        ordinal: 11,
        name: None,
        suppressed: None,
        parent: None,
        dependencies: Vec::new(),
        source_properties: Default::default(),
        source_tag: None,
        source_text: None,
        source_content: Vec::new(),
        outputs: Vec::new(),
        definition: FeatureDefinition::Sweep {
            section: cadmpeg_ir::features::SweepSection::Unresolved(None),
            sections: Vec::new(),
            path: None,
            path_extent: None,
            guide_rail: None,
            taper: None,
            mode: cadmpeg_ir::features::SweepMode::Unresolved,
            orientation: None,
            transition: None,
            transformation: None,
            path_tangent: false,
            linearize: false,
            twist: None,
            scale: None,
            allow_multi_profile_faces: None,
        },
        native_ref: None,
    });
    ir.model.configurations.extend([
        DesignConfiguration {
            id: ConfigurationId("test:configuration#0".into()),
            ordinal: 0,
            active: true,
            source_index: Some(0),
            name: "Model".into(),
            material: None,
            properties: Default::default(),
            parameter_overrides: Default::default(),
            suppressed_features: Vec::new(),
            bodies: ConfigurationBodies::Resolved(Vec::new()),
            parameter_values: Default::default(),
            feature_states: Default::default(),
            native_ref: None,
        },
        DesignConfiguration {
            id: ConfigurationId("test:configuration#1".into()),
            ordinal: 1,
            active: false,
            source_index: Some(1),
            name: "Arrangement".into(),
            material: None,
            properties: Default::default(),
            parameter_overrides: Default::default(),
            suppressed_features: Vec::new(),
            bodies: ConfigurationBodies::Unresolved,
            parameter_values: Default::default(),
            feature_states: Default::default(),
            native_ref: None,
        },
    ]);

    let mut losses = Vec::new();
    crate::decode::append_design_intent_losses(&ir, &mut losses);

    assert_eq!(losses.len(), 7);
    assert_eq!(losses[0].code.category(), LossCategory::DesignIntent);
    assert!(losses[0]
        .message
        .contains("10 NX feature history operation"));
    assert_eq!(losses[1].code.category(), LossCategory::DesignIntent);
    assert!(losses[1].message.contains("2 NX design configuration"));
    assert_eq!(losses[2].code.category(), LossCategory::DesignIntent);
    assert!(losses[2].message.contains("DELETE (2)"));
    assert_eq!(losses[3].code.category(), LossCategory::DesignIntent);
    assert!(losses[3].message.contains("datum coordinate system (1)"));
    assert!(losses[3].message.contains("datum plane (1)"));
    assert!(losses[3].message.contains("freeform surface (1)"));
    assert!(losses[3].message.contains("loft (2)"));
    assert_eq!(losses[4].code.category(), LossCategory::DesignIntent);
    assert!(losses[4].message.contains("block (1)"));
    assert!(losses[4].message.contains("sweep (1)"));
    assert_eq!(losses[5].code.category(), LossCategory::DesignIntent);
    assert!(losses[5].message.contains("delete body (1)"));
    assert!(losses[5].message.contains("sketch (1)"));
    assert_eq!(losses[6].code.category(), LossCategory::DesignIntent);
    assert!(losses[6].message.contains("1 NX sketch history feature"));
    assert!(losses[6].message.contains("1 have no neutral sketch graph"));

    let sketch_id = cadmpeg_ir::sketches::SketchId("test:sketch#0".into());
    ir.model.sketches.push(cadmpeg_ir::sketches::Sketch {
        id: sketch_id.clone(),
        name: None,
        configuration: None,
        placement: cadmpeg_ir::sketches::SketchPlacement::Resolved {
            origin: cadmpeg_ir::math::Point3::new(0.0, 0.0, 0.0),
            normal: cadmpeg_ir::math::Vector3::new(0.0, 0.0, 1.0),
            u_axis: cadmpeg_ir::math::Vector3::new(1.0, 0.0, 0.0),
        },
        profiles: Vec::new(),
        native_ref: None,
    });
    ir.model.features[2].definition = FeatureDefinition::Sketch {
        space: cadmpeg_ir::features::SketchSpace::Planar,
        sketch: Some(sketch_id),
    };
    losses.clear();
    crate::decode::append_design_intent_losses(&ir, &mut losses);

    assert_eq!(losses.len(), 6);
    assert!(losses[4].message.contains("block (1)"));
    assert!(!losses[5].message.contains("sketch"));
}

#[test]
fn design_intent_losses_ignore_unresolved_suppression_outside_active_closure() {
    use cadmpeg_ir::features::{Feature, FeatureDefinition, FeatureId};

    let mut ir = cadmpeg_ir::examples::unit_cube();
    let body = ir.model.bodies[0].id.clone();
    ir.model.features.extend([
        Feature {
            id: FeatureId("test:feature#active".into()),
            ordinal: 0,
            name: Some("active".into()),
            suppressed: Some(false),
            parent: None,
            dependencies: Vec::new(),
            source_properties: Default::default(),
            source_tag: None,
            source_text: None,
            source_content: Vec::new(),
            outputs: vec![body],
            definition: FeatureDefinition::DatumPoint {
                position: cadmpeg_ir::math::Point3::new(0.0, 0.0, 0.0),
            },
            native_ref: None,
        },
        Feature {
            id: FeatureId("test:feature#inactive".into()),
            ordinal: 1,
            name: Some("inactive".into()),
            suppressed: None,
            parent: None,
            dependencies: Vec::new(),
            source_properties: Default::default(),
            source_tag: None,
            source_text: None,
            source_content: Vec::new(),
            outputs: Vec::new(),
            definition: FeatureDefinition::DatumPoint {
                position: cadmpeg_ir::math::Point3::new(1.0, 0.0, 0.0),
            },
            native_ref: None,
        },
        Feature {
            id: FeatureId("test:feature#inactive-native".into()),
            ordinal: 2,
            name: Some("inactive-native".into()),
            suppressed: None,
            parent: None,
            dependencies: Vec::new(),
            source_properties: Default::default(),
            source_tag: None,
            source_text: None,
            source_content: Vec::new(),
            outputs: Vec::new(),
            definition: FeatureDefinition::Native {
                kind: "DELETE".into(),
                parameters: Default::default(),
                properties: Default::default(),
            },
            native_ref: None,
        },
        Feature {
            id: FeatureId("test:feature#inactive-datum-csys".into()),
            ordinal: 3,
            name: Some("inactive-datum-csys".into()),
            suppressed: None,
            parent: None,
            dependencies: Vec::new(),
            source_properties: Default::default(),
            source_tag: None,
            source_text: None,
            source_content: Vec::new(),
            outputs: Vec::new(),
            definition: FeatureDefinition::DatumCoordinateSystemUnresolved,
            native_ref: None,
        },
        Feature {
            id: FeatureId("test:feature#inactive-sketch".into()),
            ordinal: 4,
            name: Some("inactive-sketch".into()),
            suppressed: None,
            parent: None,
            dependencies: Vec::new(),
            source_properties: Default::default(),
            source_tag: None,
            source_text: None,
            source_content: Vec::new(),
            outputs: Vec::new(),
            definition: FeatureDefinition::Sketch {
                space: cadmpeg_ir::features::SketchSpace::Unresolved,
                sketch: None,
            },
            native_ref: None,
        },
    ]);

    let mut losses = Vec::new();
    crate::decode::append_design_intent_losses(&ir, &mut losses);
    assert!(losses.is_empty());
}

#[test]
fn design_intent_losses_accept_output_free_local_body_operations() {
    use cadmpeg_ir::document::CadIr;
    use cadmpeg_ir::features::{Feature, FeatureDefinition, FeatureId, PatternKind};

    let mut ir = CadIr::empty(cadmpeg_ir::units::Units::default());
    let mut source_properties = std::collections::BTreeMap::new();
    source_properties.insert(
        "primary_body_reference".to_string(),
        "reference".to_string(),
    );
    ir.model.features.push(Feature {
        id: FeatureId("test:feature#local-pattern".into()),
        ordinal: 0,
        name: Some("Pattern Geometry".into()),
        suppressed: Some(false),
        parent: None,
        dependencies: Vec::new(),
        source_properties,
        source_tag: Some("Pattern Geometry".into()),
        source_text: None,
        source_content: Vec::new(),
        outputs: Vec::new(),
        definition: FeatureDefinition::Pattern {
            seeds: Vec::new(),
            pattern: PatternKind::Unresolved { form: None },
        },
        native_ref: None,
    });

    let mut losses = Vec::new();
    crate::decode::append_design_intent_losses(&ir, &mut losses);

    assert_eq!(losses.len(), 1);
    assert!(losses[0]
        .message
        .contains("incomplete neutral construction fields"));
    assert!(losses[0].message.contains("pattern (1)"));
}

#[test]
fn design_intent_losses_accept_pattern_construction_without_body_reference() {
    use cadmpeg_ir::features::{Feature, FeatureDefinition, FeatureId, PatternKind};

    let feature = Feature {
        id: FeatureId("test:feature#pattern-construction".into()),
        ordinal: 0,
        name: Some("Pattern Geometry".into()),
        suppressed: Some(false),
        parent: None,
        dependencies: Vec::new(),
        source_properties: Default::default(),
        source_tag: Some("Pattern Geometry".into()),
        source_text: None,
        source_content: Vec::new(),
        outputs: Vec::new(),
        definition: FeatureDefinition::Pattern {
            seeds: Vec::new(),
            pattern: PatternKind::Unresolved { form: None },
        },
        native_ref: None,
    };
    let mut ir = cadmpeg_ir::document::CadIr::empty(cadmpeg_ir::units::Units::default());
    ir.model.features.push(feature);

    let mut losses = Vec::new();
    crate::decode::append_design_intent_losses(&ir, &mut losses);

    assert_eq!(losses.len(), 1);
    assert!(losses[0]
        .message
        .contains("incomplete neutral construction fields"));
    assert!(losses[0].message.contains("pattern (1)"));

    ir.model.features[0]
        .source_properties
        .insert("body_reference.0".into(), "42".into());
    losses.clear();
    crate::decode::append_design_intent_losses(&ir, &mut losses);
    assert_eq!(losses.len(), 1);
    assert!(losses[0]
        .message
        .contains("output lineage is missing, duplicated"));
}

#[test]
fn design_intent_losses_accept_unbound_trim_surface_construction() {
    use cadmpeg_ir::features::{
        FaceSelection, Feature, FeatureDefinition, FeatureId, PathRef, TrimRegion,
    };

    let mut ir = cadmpeg_ir::document::CadIr::empty(cadmpeg_ir::units::Units::default());
    ir.model.features.push(Feature {
        id: FeatureId("test:feature#construction-trim".into()),
        ordinal: 0,
        name: Some("TRIMMED_SH".into()),
        suppressed: Some(false),
        parent: None,
        dependencies: Vec::new(),
        source_properties: Default::default(),
        source_tag: Some("TRIMMED_SH".into()),
        source_text: None,
        source_content: Vec::new(),
        outputs: Vec::new(),
        definition: FeatureDefinition::TrimSurface {
            faces: FaceSelection::Faces(vec![cadmpeg_ir::ids::FaceId("face".into())]),
            tool: PathRef::Edges(vec![cadmpeg_ir::ids::EdgeId("edge".into())]),
            keep: TrimRegion::Inside,
        },
        native_ref: None,
    });

    let mut losses = Vec::new();
    crate::decode::append_design_intent_losses(&ir, &mut losses);
    assert!(losses.is_empty());

    ir.model.features[0]
        .source_properties
        .insert("body_reference.0".into(), "42".into());
    crate::decode::append_design_intent_losses(&ir, &mut losses);
    assert_eq!(losses.len(), 1);
    assert!(losses[0]
        .message
        .contains("output lineage is missing, duplicated"));
}

#[test]
fn output_free_local_body_construction_requires_unbound_primary_body() {
    use cadmpeg_ir::features::{Feature, FeatureDefinition, FeatureId, PatternKind};

    let mut source_properties = std::collections::BTreeMap::new();
    source_properties.insert(
        "primary_body_reference".to_string(),
        "reference".to_string(),
    );
    let mut feature = Feature {
        id: FeatureId("test:feature#local-pattern".into()),
        ordinal: 0,
        name: Some("Pattern Geometry".into()),
        suppressed: Some(false),
        parent: None,
        dependencies: Vec::new(),
        source_properties,
        source_tag: Some("Pattern Geometry".into()),
        source_text: None,
        source_content: Vec::new(),
        outputs: Vec::new(),
        definition: FeatureDefinition::Pattern {
            seeds: Vec::new(),
            pattern: PatternKind::Unresolved { form: None },
        },
        native_ref: None,
    };

    assert!(crate::decode::output_free_local_body_construction(&feature));

    feature.source_properties.remove("primary_body_reference");
    feature
        .source_properties
        .insert("body_reference.0".to_string(), "42".to_string());
    assert!(!crate::decode::output_free_local_body_construction(
        &feature
    ));

    feature.source_properties.insert(
        "primary_body_reference".to_string(),
        "reference".to_string(),
    );
    feature.source_properties.insert(
        "primary_body_segment_use".to_string(),
        "segment-use".to_string(),
    );
    assert!(!crate::decode::output_free_local_body_construction(
        &feature
    ));
}

#[path = "integration_tests.rs"]
mod integration_tests;

#[test]
fn extraction_uses_ug_part_bounds_and_all_standard_zlib_headers() {
    let part = zlib_compress_at_level(&partition_stream(), 6);
    assert_eq!(&part[..2], b"\x78\x9c");

    let mut decoy_stream = partition_stream();
    let schema = b"SCH_TEST_1_9999";
    let decoy = b"SCH_FAKE_1_9999";
    let pos = decoy_stream
        .windows(schema.len())
        .position(|w| w == schema)
        .unwrap();
    decoy_stream[pos..pos + schema.len()].copy_from_slice(decoy);
    let decoy = zlib_compress(&decoy_stream);

    let file = prt_with_named_payloads(&[
        ("/Root/UG_PART/UG_PART", part),
        ("/Root/FastLoad/JT", decoy),
    ]);

    let streams = extract_streams(&file);
    assert_eq!(streams.len(), 1);
    assert_eq!(streams[0].schema.as_deref(), Some("SCH_TEST_1_9999"));
}

#[test]
fn extraction_rejects_zlib_members_with_invalid_integrity_trailers() {
    let compressed = zlib_compress(&partition_stream());
    let mut corrupt = compressed.clone();
    *corrupt.last_mut().expect("zlib integrity trailer") ^= 0x01;
    let corrupt = prt_with_named_payloads(&[("/Root/UG_PART/UG_PART", corrupt)]);
    assert!(extract_streams(&corrupt).is_empty());

    let truncated = prt_with_named_payloads(&[(
        "/Root/UG_PART/UG_PART",
        compressed[..compressed.len() - 1].to_vec(),
    )]);
    assert!(extract_streams(&truncated).is_empty());

    let mut indexed = segment_stream_payload();
    *indexed.last_mut().expect("indexed zlib integrity trailer") ^= 0x01;
    let indexed = prt_with_named_payloads(&[("/Root/UG_PART/UG_PART", indexed)]);
    let arena = cadmpeg_core::decode::DecodeArena::new();
    let policy = cadmpeg_core::decode::DecodePolicy::default();
    let (ctx, root) =
        cadmpeg_core::decode::DecodeContext::from_root_bytes(&indexed, &arena, &policy)
            .expect("bounded test input");
    let container = container::scan_bytes(indexed.clone()).expect("test SPLMSSTR container");
    assert!(parasolid::extract_streams(&ctx, root, &container).is_err());
}

#[test]
fn extraction_uses_ordered_segment_wrappers_in_indexed_payloads() {
    let decoy = zlib_compress(
        b"PS\0\0 (partition) SCH_DECOY_1_9999 unindexed payload with more than sixty-four inflated bytes........",
    );
    let real = zlib_compress(
        b"PS\0\0 (deltas) SCH_REAL_1_9999 indexed payload with more than sixty-four inflated bytes..........",
    );
    let mut payload = Vec::new();
    for word in [0_u32, 9, 11, 1, 1, 24] {
        payload.extend_from_slice(&word.to_le_bytes());
    }
    payload.extend_from_slice(&decoy);
    let wrapper_offset = payload.len();
    payload[0..4].copy_from_slice(
        &u32::try_from(wrapper_offset)
            .expect("synthetic wrapper offset")
            .to_le_bytes(),
    );
    payload.extend_from_slice(&0x8000_0000_u32.to_le_bytes());
    payload.extend_from_slice(&0_u32.to_le_bytes());
    payload.extend_from_slice(&real);

    let file = prt_with_named_payloads(&[("/Root/UG_PART/UG_PART", payload)]);
    let streams = extract_streams(&file);
    assert_eq!(streams.len(), 1);
    assert_eq!(streams[0].kind, StreamKind::Deltas);
    assert_eq!(streams[0].schema.as_deref(), Some("SCH_REAL_1_9999"));
}

/// Phase 0 golden serialized-output snapshots.
///
/// These freeze the NX codec's complete observable output before the native-tier
/// refactor begins. For each fixture the harness runs `NxCodec::decode` and
/// `NxCodec::inspect`, then serializes the full [`DecodeResult`] (the decoded
/// `CadIr` including the `nx` native-namespace arenas, the [`DecodeReport`], and
/// the [`SourceFidelity`] sidecar carrying provenance/exactness annotations) plus
/// the [`ContainerSummary`] into one deterministic pretty-JSON document, compared
/// byte-for-byte against a committed golden file under `tests/golden/`.
///
/// Serialization goes through `serde_json::to_value` (whose object maps are
/// `BTreeMap`, so keys sort) and then `to_string_pretty`. Every IR container that
/// reaches the wire is `BTreeMap`- or `Vec`-backed and codec output is sorted by
/// id, so the bytes are stable across runs; `golden_output_is_deterministic`
/// asserts that directly.
///
/// Regenerate after an intended output change with:
///   `UPDATE_GOLDEN=1 cargo test-fast golden`
/// then review the golden diff before committing. Regenerate with the workspace
/// feature set (`test-fast` / `--workspace`), NOT `-p cadmpeg-codec-nx`: the
/// fixtures zlib-compress their streams through `flate2`, and Cargo feature
/// unification selects the `zlib-rs` backend for the full-workspace build but
/// `miniz_oxide` for an isolated crate build. The two backends emit different
/// compressed bytes, so the container byte length, `sha256`, and byte-ledger
/// totals in these snapshots are only stable under the workspace build (the one
/// the commit hook and CI run). This is a build-config sensitivity of the
/// fixtures, not codec nondeterminism: `golden_output_is_deterministic` confirms
/// decode output is a pure function of the input bytes.
mod golden {
    use std::collections::BTreeSet;
    use std::io::Cursor;
    use std::path::{Path, PathBuf};

    use cadmpeg_ir::codec::{CodecEntry, DecodeOptions};

    use super::*;

    /// Every arena name production writes via the native catalogue, extracted
    /// mechanically. This is the coverage denominator; `arena_coverage_is_a_subset`
    /// fails if production introduces an arena name this list does not know, which
    /// keeps the denominator honest as the code evolves.
    const KNOWN_ARENAS: &[&str] = &[
        "class_definitions",
        "configuration_attribute_uses",
        "configurations",
        "data_block_abr_reference_lanes",
        "data_block_column_index_tables",
        "data_block_control_class_references",
        "data_block_control_forms",
        "data_block_control_handle_pairs",
        "data_block_control_index_values",
        "data_block_control_references",
        "data_block_control_values",
        "data_block_counted_index_lanes",
        "data_block_index_rows",
        "data_block_linked_index_rows",
        "data_block_object_frames",
        "data_block_references",
        "data_block_target_index_rows",
        "data_blocks",
        "display_jt_base_node_data",
        "display_jt_compressed_element_sequences",
        "display_jt_compressed_elements",
        "display_jt_coordinate_array_headers",
        "display_jt_documents",
        "display_jt_geometric_transform_attributes",
        "display_jt_material_attributes",
        "display_jt_group_node_data",
        "display_jt_indices",
        "display_jt_initial_face_degree_symbols",
        "display_jt_instance_nodes",
        "display_jt_partition_nodes",
        "display_jt_polygon_meshes",
        "display_jt_range_lod_nodes",
        "display_jt_segments",
        "display_jt_shape_lod_bindings",
        "display_jt_shape_lod_elements",
        "display_jt_string_property_atoms",
        "display_jt_topology_packet_sequences",
        "display_jt_tri_strip_lod_headers",
        "display_jt_tri_strip_shape_nodes",
        "display_jt_vertex_colors",
        "display_jt_vertex_coordinates",
        "display_jt_vertex_flags",
        "display_jt_vertex_normals",
        "display_jt_vertex_records_headers",
        "display_jt_vertex_texture_coordinates",
        "expression_declarations",
        "expressions",
        "external_reference_empty_records",
        "external_reference_indexed_records",
        "external_reference_record_children",
        "external_reference_record_string_uses",
        "external_reference_records",
        "external_reference_tail_reference_pairs",
        "external_references",
        "fast_load_component_occurrences",
        "fast_load_component_object_groups",
        "fast_load_component_prototypes",
        "fast_load_component_uuids",
        "feature_block_construction_payloads",
        "feature_block_construction_references",
        "feature_block_constructions",
        "feature_block_dimensions",
        "feature_block_payload_named_records",
        "feature_block_payload_names",
        "feature_block_payload_point_groups",
        "feature_block_payload_points",
        "feature_block_payload_scalars",
        "feature_body_reference_occurrences",
        "feature_body_references",
        "feature_body_data_block_uses",
        "feature_body_segment_uses",
        "feature_boolean_operations",
        "feature_datum_csys_block_uses",
        "feature_datum_csys_column_row_uses",
        "feature_datum_csys_constructions",
        "feature_datum_csys_descriptors",
        "feature_datum_csys_payload_fixed_pairs",
        "feature_datum_csys_payload_scalar_pairs",
        "feature_datum_csys_payload_scalars",
        "feature_datum_csys_payloads",
        "feature_datum_plane_block_uses",
        "feature_datum_plane_csys_identity_uses",
        "feature_datum_plane_descriptors",
        "feature_datum_plane_headers",
        "feature_datum_plane_payload_scalar_pairs",
        "feature_datum_plane_payloads",
        "feature_draft_construction_binary32_lanes",
        "feature_draft_construction_fixed_lanes",
        "feature_draft_construction_graph_payloads",
        "feature_draft_construction_graph_strings",
        "feature_draft_construction_identity_frames",
        "feature_draft_construction_index_lanes",
        "feature_draft_construction_payloads",
        "feature_draft_construction_references",
        "feature_draft_construction_terminal_lanes",
        "feature_delete_construction_payloads",
        "feature_delete_reference_fields",
        "feature_extrude_32_constructions",
        "feature_extrude_construction_profiles",
        "feature_extrude_payload_32_branches",
        "feature_extrude_payload_headers",
        "feature_extrude_profile_references",
        "feature_fset_construction_payloads",
        "feature_fset_reference_graphs",
        "feature_input_block_identity_groups",
        "feature_input_blocks",
        "feature_input_column_row_uses",
        "feature_input_column_targets",
        "feature_identical_instance_output_lanes",
        "feature_hole_package_construction_group_lanes",
        "feature_hole_package_construction_group_uses",
        "feature_multi_instance_output_lanes",
        "feature_operation_body_11_continuations",
        "feature_operation_body_members",
        "feature_operation_body_operands",
        "feature_operation_body_reference_lanes",
        "feature_operation_body_scalar_triples",
        "feature_operation_labels",
        "feature_operation_records",
        "feature_operation_common_frames",
        "feature_operation_terminal_discriminators",
        "feature_operation_terminal_frames",
        "feature_parameter_bindings",
        "feature_parameter_uses",
        "feature_pattern_construction_fixed_lanes",
        "feature_pattern_construction_payloads",
        "feature_pattern_construction_strings",
        "feature_pattern_references",
        "feature_pattern_transform_lanes",
        "feature_payload_strings",
        "feature_point_construction_headers",
        "feature_point_construction_scalar_lanes",
        "feature_projected_curve_construction_payloads",
        "feature_projected_curve_construction_strings",
        "feature_projected_curve_references",
        "feature_simple_hole_construction_groups",
        "feature_simple_hole_repeated_scalar_lane_block_references",
        "feature_simple_hole_repeated_scalar_lanes",
        "feature_simple_hole_templates",
        "feature_sketch_construction_inputs",
        "feature_sketch_construction_payloads",
        "feature_sketch_datum_csys_dependencies",
        "feature_sketch_fixed_points",
        "feature_sketch_named_point_block_uses",
        "feature_sketch_payload_coordinate_pairs",
        "feature_sketch_payload_fixed_pairs",
        "feature_sketch_payload_mixed_pairs",
        "feature_sketch_payload_named_records",
        "feature_sketch_payload_names",
        "feature_sketch_payload_scalars",
        "feature_sketch_point_groups",
        "feature_sketch_point_uses",
        "feature_sketch_points",
        "feature_sketch_preceding_named_point_uses",
        "feature_sketch_records",
        "feature_sketch_references",
        "feature_surface_construction_branches",
        "feature_surface_construction_payloads",
        "feature_surface_construction_references",
        "feature_surface_construction_scalar_pairs",
        "feature_surface_construction_strings",
        "field_definitions",
        "material_texture_assets",
        "material_texture_catalog_entries",
        "object_records",
        "object_record_handle_pairs",
        "object_references",
        "object_uuid_values",
        "offset_store_named_points",
        "om_record_areas",
        "parasolid_attribute_class_uses",
        "parasolid_attribute_field_uses",
        "parasolid_attribute_field_names",
        "parasolid_attribute_definitions",
        "parasolid_blend_bound_records",
        "parasolid_blend_surface_records",
        "parasolid_chart_records",
        "parasolid_deltas_body_revisions",
        "parasolid_deltas_transmit_headers",
        "parasolid_deltas_terminal_null_references",
        "parasolid_deltas_records",
        "parasolid_deltas_residual_spans",
        "parasolid_deltas_tagged_reference_lanes",
        "parasolid_deltas_reference_type_maps",
        "parasolid_deltas_reference_state_packets",
        "parasolid_deltas_schema_reference_preambles",
        "parasolid_deltas_reference_marker_packets",
        "parasolid_deltas_type_150_state_packets",
        "parasolid_deltas_inline_schema_declarations",
        "parasolid_deltas_inline_body_states",
        "parasolid_deltas_term_use_numeric_tails",
        "parasolid_deltas_tombstones",
        "parasolid_entity_51_numeric_uses",
        "parasolid_entity_51_records",
        "parasolid_entity_51_structured_uses",
        "parasolid_entity_51_string_uses",
        "parasolid_entity_52_integer_records",
        "parasolid_entity_53_double_records",
        "parasolid_entity_54_string_records",
        "parasolid_entity_57_axis_records",
        "parasolid_entity_58_tag_records",
        "parasolid_entity_62_unicode_records",
        "parasolid_entity_vector_records",
        "parasolid_field_names_records",
        "parasolid_intersection_records",
        "parasolid_offset_surface_records",
        "parasolid_support_uv_records",
        "parasolid_surface_curve_records",
        "parasolid_term_use_records",
        "parasolid_topology_attribute_class_uses",
        "parasolid_topology_attribute_list_references",
        "parasolid_trimmed_curve_records",
        "part_attributes",
        "part_color_definitions",
        "part_color_tables",
        "rm_display_color_assignments",
        "persistent_handles",
        "rm_creation_display_data_relations",
        "rmfastload_object_id_tables",
        "rmfastload_object_ids",
        "saved_toggle_entries",
        "saved_toggle_streams",
        "segment_body_bindings",
        "segment_body_lineage_statuses",
        "segment_index_rows",
        "segment_om_links",
        "segment_stream_links",
        "store_headers",
        "string_values",
    ];

    /// A floor on distinct arenas the golden fixtures collectively populate.
    /// Frozen from the generated snapshots; if a refactor drops an arena from
    /// every fixture, `arena_coverage_meets_floor` fails. Raise it (never lower
    /// it) when new covering fixtures are added.
    const ARENA_COVERAGE_FLOOR: usize = 122;

    /// Build the covering fixture set: `(golden name, full `.prt` bytes)`. Each
    /// stream builder is wrapped exactly as its originating white-box test wraps
    /// it (`prt_with_partition` for a lone partition, `prt_with_streams` for a
    /// partition paired with an equal-schema deltas stream, `prt_with_named_payloads`
    /// for an OM record area), so the bytes exercise the real decode path.
    fn fixtures() -> Vec<(&'static str, Vec<u8>)> {
        let mut f: Vec<(&'static str, Vec<u8>)> = Vec::new();

        // Self-contained `.prt` images.
        f.push(("single_part_prt", single_part_prt()));
        f.push(("topology_part_prt", topology_part_prt()));
        f.push(("prt_with_arrangements", prt_with_arrangements()));
        f.push((
            "prt_with_arrangement_attribute_none",
            prt_with_arrangement_attribute(None),
        ));
        f.push(("prt_with_indexed_om_section", prt_with_indexed_om_section()));
        f.push((
            "prt_with_size_framed_om_section",
            prt_with_size_framed_om_section(),
        ));
        f.push(("assembly_prt", assembly_prt()));
        f.push((
            "assembly_with_external_paths",
            assembly_with_external_paths(),
        ));
        f.push(("rmfastload_prt", rmfastload_prt()));
        f.push((
            "prt_with_two_bodies_and_rmfastload",
            prt_with_two_bodies_and_rmfastload(),
        ));
        f.push((
            "prt_with_two_active_bodies_and_rmfastload",
            prt_with_two_active_bodies_and_rmfastload(),
        ));
        f.push((
            "prt_with_missing_active_body_record",
            prt_with_missing_active_body_record(),
        ));
        f.push((
            "prt_with_weak_rmfastload_overlap",
            prt_with_weak_rmfastload_overlap(),
        ));

        // Parasolid neutral-binary attribute/entity records in a partition stream.
        f.push((
            "parasolid_entity_records",
            prt_with_partition(&parasolid_entity_records_stream()),
        ));

        // Embedded DisplayJT stream: outer index, one JT document, one segment.
        f.push((
            "display_jt_basic",
            prt_with_named_payloads(&[
                ("/Root/UG_PART/UG_PART", zlib_compress(&partition_stream())),
                ("/Root/UG_PART/DisplayJT", display_jt_basic_stream()),
            ]),
        ));
        f.push((
            "display_jt_scene_graph",
            prt_with_named_payloads(&[
                ("/Root/UG_PART/UG_PART", zlib_compress(&partition_stream())),
                ("/Root/UG_PART/DisplayJT", display_jt_scene_graph_stream()),
            ]),
        ));
        f.push((
            "display_jt_shape_lod",
            prt_with_named_payloads(&[
                ("/Root/UG_PART/UG_PART", zlib_compress(&partition_stream())),
                ("/Root/UG_PART/DisplayJT", display_jt_shape_lod_stream()),
            ]),
        ));
        f.push((
            "display_jt_string_property",
            prt_with_named_payloads(&[
                ("/Root/UG_PART/UG_PART", zlib_compress(&partition_stream())),
                (
                    "/Root/UG_PART/DisplayJT",
                    display_jt_string_property_stream(),
                ),
            ]),
        ));

        // Offset-store control blocks: the plain form resolves class-registry
        // ordinals; the handle form carries two adjacent persistent handles.
        f.push((
            "data_block_control_class_references",
            prt_with_named_payloads(&[("/Root/UG_PART/UG_PART", offset_only_indexed_om_section())]),
        ));
        f.push((
            "offset_store_named_point",
            prt_with_named_payloads(&[(
                "/Root/UG_PART/UG_PART",
                offset_only_indexed_om_section_with_named_point(),
            )]),
        ));
        f.push((
            "data_block_control_index_values",
            prt_with_named_payloads(&[(
                "/Root/UG_PART/UG_PART",
                offset_only_indexed_om_section_with_index_values(),
            )]),
        ));
        // EXTREFSTREAM index, string table, and handle-set records.
        f.push((
            "external_reference_stream",
            prt_with_named_payloads(&[
                ("/Root/UG_PART/UG_PART", zlib_compress(&partition_stream())),
                ("/Root/ExternalReferences", external_reference_stream()),
            ]),
        ));

        f.push(("data_block_control_handles", {
            let mut control = Vec::new();
            control.extend_from_slice(&[0xe0, 0, 0, 0, 1]);
            control.extend_from_slice(&[0xe0, 0, 0, 0, 2]);
            prt_with_named_payloads(&[(
                "/Root/UG_PART/UG_PART",
                offset_only_indexed_om_section_with_control(&control),
            )])
        }));

        // OM record areas / feature history, wrapped as a named UG_PART payload.
        f.push((
            "om_record_area",
            prt_with_named_payloads(&[("/Root/UG_PART/UG_PART", segment_om_record_area_payload())]),
        ));
        f.push((
            "om_record_area_input_store",
            prt_with_named_payloads(&[(
                "/Root/UG_PART/UG_PART",
                segment_om_record_area_with_input_store_payload(),
            )]),
        ));
        f.push((
            "multi_section_feature_history",
            prt_with_named_payloads(&[(
                "/Root/UG_PART/UG_PART",
                multi_section_feature_history_payload(),
            )]),
        ));
        f.push(("composed_feature_history", composed_feature_history_prt()));
        f.push((
            "segment_index_rows",
            prt_with_named_payloads(&[("/Root/UG_PART/UG_PART", segment_index_payload())]),
        ));
        f.push((
            "segment_stream_links",
            prt_with_named_payloads(&[("/Root/UG_PART/UG_PART", segment_stream_payload())]),
        ));
        f.push((
            "segment_body_bindings",
            prt_with_named_payloads(&[(
                "/Root/UG_PART/UG_PART",
                segment_body_binding_payload("partition"),
            )]),
        ));
        f.push((
            "material_texture_assets",
            prt_with_named_payloads(&[
                ("/Root/UG_PART/UG_PART", zlib_compress(&partition_stream())),
                (
                    "/Root/materialsTif/AISI Steel 4340",
                    vec![b'I', b'I', 42, 0, 8, 0, 0, 0, 0, 0],
                ),
                (
                    "/Root/materialsTif/Truncated",
                    vec![b'I', b'I', 42, 0, 40, 0, 0, 0, 0, 0],
                ),
            ]),
        ));
        f.push(("material_texture_catalog", prt_with_named_payloads(&[
            ("/Root/UG_PART/UG_PART", zlib_compress(&partition_stream())),
            ("/Root/materialsTif/unmap$1", vec![b'M', b'M', 0, 42, 0, 0, 0, 8, 0, 0]),
            ("/Root/qafmetadata", br#"<?xml version="1.0" encoding="UTF-8"?>
<folderContents>
<folderProperties location="images/preview" unmappedLocation="images/preview"><createTime>2026-07-15T08:00:00</createTime><modifyTime>2026-07-15T08:00:01</modifyTime></folderProperties>
<folderProperties location="materialsTif/unmap$1" unmappedLocation="materialsTif/Carbon Fiber Harness Satin Coated"><createTime>2026-07-15T08:01:00</createTime><modifyTime>2026-07-15T08:02:00</modifyTime></folderProperties>
</folderContents>"#.to_vec()),
        ])));
        f.push(("om_repeated_operations", {
            let section = size_framed_om_section_with_repeated_operations(12);
            let mut payload = Vec::new();
            for word in [24_u32, 9, 11, 1, 1, 24] {
                payload.extend_from_slice(&word.to_le_bytes());
            }
            payload.extend_from_slice(&section);
            prt_with_named_payloads(&[("/Root/UG_PART/UG_PART", payload)])
        }));

        // Lone partition streams, each wrapped with `prt_with_partition`.
        let partitions: Vec<(&'static str, Vec<u8>)> = vec![
            (
                "topology_with_missing_tolerances",
                topology_with_missing_tolerances(),
            ),
            ("partition_stream", partition_stream()),
            (
                "offset_surface_topology_partition_stream",
                offset_surface_topology_partition_stream(),
            ),
            (
                "offset_surface_with_fully_extended_common_header",
                offset_surface_with_fully_extended_common_header(),
            ),
            (
                "surface_curve_topology_partition_stream",
                surface_curve_topology_partition_stream(),
            ),
            (
                "pcurve_topology_partition_stream",
                pcurve_topology_partition_stream(),
            ),
            (
                "shared_region_shells_partition_stream",
                shared_region_shells_partition_stream(),
            ),
            (
                "blend_surface_topology_partition_stream",
                blend_surface_topology_partition_stream(),
            ),
            (
                "blend_surface_with_extended_support_reference",
                blend_surface_with_extended_support_reference(),
            ),
            (
                "blend_surface_with_intersection_spine",
                blend_surface_with_intersection_spine(),
            ),
            (
                "blend_surface_with_forward_blend_support",
                blend_surface_with_forward_blend_support(),
            ),
            (
                "intersection_curve_topology_partition_stream",
                intersection_curve_topology_partition_stream(),
            ),
            (
                "charted_intersection_curve_topology_partition_stream",
                charted_intersection_curve_topology_partition_stream(),
            ),
            (
                "charted_intersection_with_edge_endpoint_witnesses_stream",
                charted_intersection_with_edge_endpoint_witnesses_stream(),
            ),
            (
                "charted_intersection_without_uv_stream",
                charted_intersection_without_uv_stream(),
            ),
            (
                "charted_intersection_with_approximated_term_stream",
                charted_intersection_with_approximated_term_stream(),
            ),
            (
                "two_support_charted_intersection_curve_stream",
                two_support_charted_intersection_curve_stream(),
            ),
            (
                "blend_bound_charted_intersection_curve_stream",
                blend_bound_charted_intersection_curve_stream(),
            ),
            (
                "inline_descriptor_intersection_curve_stream",
                inline_descriptor_intersection_curve_stream(),
            ),
            (
                "circle_topology_partition_stream",
                circle_topology_partition_stream(),
            ),
            (
                "ellipse_topology_partition_stream",
                ellipse_topology_partition_stream(),
            ),
            (
                "cylinder_topology_partition_stream",
                cylinder_topology_partition_stream(),
            ),
            (
                "cone_topology_partition_stream",
                cone_topology_partition_stream(),
            ),
            (
                "sphere_topology_partition_stream",
                sphere_topology_partition_stream(),
            ),
            (
                "torus_topology_partition_stream",
                torus_topology_partition_stream(),
            ),
            ("bspline_partition_stream", bspline_partition_stream()),
            (
                "extended_bspline_surface_stream",
                extended_bspline_surface_stream(),
            ),
            (
                "bspline_surface_replacement_partition_stream",
                bspline_surface_replacement_partition_stream(),
            ),
            (
                "bspline_curve_replacement_partition_stream",
                bspline_curve_replacement_partition_stream(),
            ),
            (
                "trimmed_topology_partition_stream",
                trimmed_topology_partition_stream(),
            ),
            (
                "mismatched_trimmed_topology_partition_stream",
                mismatched_trimmed_topology_partition_stream(),
            ),
            (
                "partnered_trimmed_topology_partition_stream",
                partnered_trimmed_topology_partition_stream(),
            ),
            (
                "forward_trimmed_curve_chain_stream",
                forward_trimmed_curve_chain_stream(),
            ),
            (
                "topology_with_extended_edge_curve_reference",
                topology_with_extended_edge_curve_reference(),
            ),
            (
                "topology_with_extended_face_attribute_reference",
                topology_with_extended_face_attribute_reference(),
            ),
            (
                "topology_with_extended_edge_attribute_reference",
                topology_with_extended_edge_attribute_reference(),
            ),
            (
                "topology_with_extended_internal_topology_references",
                topology_with_extended_internal_topology_references(),
            ),
            (
                "topology_with_fully_extended_geometry_headers",
                topology_with_fully_extended_geometry_headers(),
            ),
            (
                "topology_with_escaped_geometry_envelopes",
                topology_with_escaped_geometry_envelopes(),
            ),
            (
                "deltas_intersection_curve_stream",
                deltas_intersection_curve_stream(),
            ),
            ("status_framed_deltas_stream", status_framed_deltas_stream()),
            (
                "variable_status_framed_deltas_stream",
                variable_status_framed_deltas_stream(),
            ),
            (
                "status_framed_deltas_point_stream",
                status_framed_deltas_point_stream(),
            ),
            (
                "deltas_point_partition_stream",
                deltas_point_partition_stream(),
            ),
            ("many_face_partition_stream", many_face_partition_stream(1)),
            (
                "large_xmt_headers_topology",
                large_xmt_headers(&topology_partition_stream()),
            ),
        ];
        for (name, stream) in partitions {
            f.push((name, prt_with_partition(&stream)));
        }

        // Deltas streams paired with an equal-schema partition via `prt_with_streams`.
        let deltas_pairs: Vec<(&'static str, Vec<u8>, Vec<u8>)> = vec![
            (
                "deltas_edge",
                topology_partition_stream(),
                deltas_edge_partition_stream(),
            ),
            (
                "deltas_face_vertex",
                topology_partition_stream(),
                deltas_face_vertex_partition_stream(),
            ),
            (
                "deltas_loop",
                topology_partition_stream(),
                deltas_loop_partition_stream(),
            ),
            (
                "deltas_shell",
                topology_partition_stream(),
                deltas_shell_partition_stream(),
            ),
            (
                "deltas_fin",
                topology_partition_stream(),
                deltas_fin_partition_stream(),
            ),
            (
                "deltas_line",
                topology_partition_stream(),
                deltas_line_partition_stream(),
            ),
            (
                "deltas_plane",
                topology_partition_stream(),
                deltas_plane_partition_stream(),
            ),
            (
                "deltas_offset_surface",
                offset_surface_topology_partition_stream(),
                deltas_offset_surface_partition_stream(),
            ),
            (
                "deltas_blend_surface",
                blend_surface_topology_partition_stream(),
                deltas_blend_surface_partition_stream(),
            ),
            (
                "deltas_trimmed_curve",
                trimmed_topology_partition_stream(),
                deltas_trimmed_curve_partition_stream(),
            ),
            (
                "deltas_surface_curve",
                surface_curve_topology_partition_stream(),
                deltas_surface_curve_partition_stream(),
            ),
            (
                "deltas_circle",
                circle_topology_partition_stream(),
                deltas_circle_partition_stream(),
            ),
            (
                "deltas_ellipse",
                ellipse_topology_partition_stream(),
                deltas_ellipse_partition_stream(),
            ),
            (
                "deltas_cylinder",
                cylinder_topology_partition_stream(),
                deltas_cylinder_partition_stream(),
            ),
            (
                "deltas_cone",
                cone_topology_partition_stream(),
                deltas_cone_partition_stream(),
            ),
            (
                "deltas_sphere",
                sphere_topology_partition_stream(),
                deltas_sphere_partition_stream(),
            ),
            (
                "deltas_torus",
                torus_topology_partition_stream(),
                deltas_torus_partition_stream(),
            ),
            (
                "deltas_bspline_surface",
                bspline_surface_replacement_partition_stream(),
                deltas_bspline_surface_wrapper_stream(),
            ),
            (
                "deltas_bspline_curve",
                bspline_curve_replacement_partition_stream(),
                deltas_bspline_curve_wrapper_stream(),
            ),
        ];
        for (name, partition, delta) in deltas_pairs {
            f.push((name, prt_with_streams(&[&partition, &delta])));
        }

        let ext11_pairs = [
            (
                "ext11_charted_intersection_curve_stream",
                charted_intersection_curve_topology_partition_stream(),
                ext11_charted_intersection_curve_stream(),
            ),
            (
                "two_support_ext11_charted_intersection_curve_stream",
                two_support_charted_intersection_curve_stream_with_second_plane_axis([
                    0.0, 0.0, 1.0,
                ]),
                two_support_ext11_charted_intersection_curve_stream(false),
            ),
            (
                "two_support_ext11_charted_intersection_curve_stream_ambiguous",
                two_support_charted_intersection_curve_stream(),
                two_support_ext11_charted_intersection_curve_stream(true),
            ),
            (
                "partial_ext11_charted_intersection_curve_stream",
                two_support_charted_intersection_curve_stream_with_second_plane_axis([
                    0.0, 0.0, 1.0,
                ]),
                partial_ext11_charted_intersection_curve_stream(),
            ),
        ];
        for (name, partition, ext11) in ext11_pairs {
            f.push((name, prt_with_ext11_intersection(&partition, &ext11)));
        }

        f
    }

    /// Serialize the complete decode + inspect output for one fixture as stable
    /// pretty JSON. Decode/inspect errors are frozen too (a `.prt` that fails to
    /// decode is a real, contract-relevant behavior), so this never panics on
    /// codec output.
    fn snapshot(bytes: &[u8]) -> String {
        let decode =
            match NxCodec.decode(&mut Cursor::new(bytes.to_vec()), &DecodeOptions::default()) {
                Ok(result) => serde_json::json!({
                    "ir": serde_json::to_value(&result.ir).expect("serialize ir"),
                    "report": serde_json::to_value(&result.report).expect("serialize report"),
                    "source_fidelity": serde_json::to_value(&result.source_fidelity)
                        .expect("serialize source_fidelity"),
                }),
                Err(err) => serde_json::json!({ "decode_error": err.to_string() }),
            };
        let inspect =
            match NxCodec.inspect(&mut Cursor::new(bytes.to_vec()), &InspectOptions::default()) {
                Ok(summary) => serde_json::to_value(&summary).expect("serialize inspect"),
                Err(err) => serde_json::json!({ "inspect_error": err.to_string() }),
            };
        let combined = serde_json::json!({ "decode": decode, "inspect": inspect });
        let mut text = serde_json::to_string_pretty(&combined).expect("serialize snapshot");
        text.push('\n');
        text
    }

    fn golden_dir() -> PathBuf {
        Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/golden")
    }

    fn golden_path(name: &str) -> PathBuf {
        golden_dir().join(format!("{name}.json"))
    }

    /// First line that differs between two documents, 1-based, with both sides
    /// truncated for a readable failure. `None` when the shorter side is a prefix
    /// of the longer (length-only difference).
    fn first_line_diff(expected: &str, actual: &str) -> (usize, String, String) {
        let mut exp = expected.lines();
        let mut act = actual.lines();
        let mut line = 0usize;
        loop {
            line += 1;
            match (exp.next(), act.next()) {
                (Some(e), Some(a)) if e == a => {}
                (e, a) => {
                    let trunc = |s: Option<&str>| match s {
                        Some(s) if s.len() > 200 => format!("{}…", &s[..200]),
                        Some(s) => s.to_string(),
                        None => "<end of file>".to_string(),
                    };
                    return (line, trunc(e), trunc(a));
                }
            }
        }
    }

    fn update_requested() -> bool {
        std::env::var_os("UPDATE_GOLDEN").is_some()
    }

    #[test]
    fn golden_snapshots_are_byte_identical() {
        let update = update_requested();
        if update {
            std::fs::create_dir_all(golden_dir()).expect("create golden dir");
        }
        let mut failures: Vec<String> = Vec::new();
        for (name, bytes) in fixtures() {
            let actual = snapshot(&bytes);
            let path = golden_path(name);
            if update {
                std::fs::write(&path, actual.as_bytes())
                    .unwrap_or_else(|e| panic!("write golden {name}: {e}"));
                continue;
            }
            let expected = match std::fs::read_to_string(&path) {
                Ok(text) => text,
                Err(e) => {
                    failures.push(format!(
                        "fixture `{name}`: cannot read golden {} ({e}); run `UPDATE_GOLDEN=1 cargo test-fast golden`",
                        path.display()
                    ));
                    continue;
                }
            };
            if let Err(mismatch) = cadmpeg_core::golden::snapshots_agree(&expected, &actual) {
                failures.push(format!(
                    "fixture `{name}`: output diverged from golden {mismatch}"
                ));
            }
        }
        assert!(
            failures.is_empty(),
            "{} golden snapshot(s) drifted; if the change is intended run `UPDATE_GOLDEN=1 cargo test-fast golden` and review the diff:\n\n{}",
            failures.len(),
            failures.join("\n\n")
        );
    }

    /// Guards against nondeterministic codec output (`HashMap` iteration order,
    /// timestamps): decoding the same bytes twice must produce identical JSON.
    #[test]
    fn golden_output_is_deterministic() {
        for (name, bytes) in fixtures() {
            let first = snapshot(&bytes);
            let second = snapshot(&bytes);
            if first != second {
                let (line, a, b) = first_line_diff(&first, &second);
                panic!("fixture `{name}`: nondeterministic output at line {line}\n    run 1: {a}\n    run 2: {b}");
            }
        }
    }

    /// Union of `nx`-namespace arenas the fixture set populates.
    fn covered_arenas() -> BTreeSet<String> {
        let mut covered = BTreeSet::new();
        for (_, bytes) in fixtures() {
            let Ok(result) = NxCodec.decode(&mut Cursor::new(bytes), &DecodeOptions::default())
            else {
                continue;
            };
            if let Some(namespace) = result.ir.native.namespace("nx") {
                for (arena, records) in &namespace.arenas {
                    if !records.is_empty() {
                        covered.insert(arena.clone());
                    }
                }
            }
        }
        covered
    }

    /// Every arena a fixture populates must be a name production actually writes.
    /// A failure here means `KNOWN_ARENAS` (the coverage denominator) is stale.
    #[test]
    fn arena_coverage_is_a_subset() {
        let known: BTreeSet<&str> = KNOWN_ARENAS.iter().copied().collect();
        let unknown: Vec<String> = covered_arenas()
            .into_iter()
            .filter(|a| a != "unknowns" && !known.contains(a.as_str()))
            .collect();
        assert!(
            unknown.is_empty(),
            "fixtures populated arenas absent from KNOWN_ARENAS (update the denominator): {unknown:?}"
        );
    }

    /// Freezes the collective arena coverage floor so a refactor cannot silently
    /// stop populating an arena across the whole fixture set. Prints the fraction
    /// under `--nocapture`.
    #[test]
    fn arena_coverage_meets_floor() {
        let covered = covered_arenas();
        let known: BTreeSet<&str> = KNOWN_ARENAS.iter().copied().collect();
        let hit = covered
            .iter()
            .filter(|a| known.contains(a.as_str()))
            .count();
        let uncovered: Vec<&str> = KNOWN_ARENAS
            .iter()
            .copied()
            .filter(|a| !covered.contains(*a))
            .collect();
        println!(
            "golden arena coverage: {hit}/{} known arenas ({:.1}%)\nuncovered: {uncovered:?}",
            KNOWN_ARENAS.len(),
            100.0 * hit as f64 / KNOWN_ARENAS.len() as f64,
        );
        assert!(
            hit >= ARENA_COVERAGE_FLOOR,
            "arena coverage regressed: {hit} < floor {ARENA_COVERAGE_FLOOR}"
        );
    }

    /// The catalogue is the single source of truth for arena names: every arena
    /// appears exactly once across `CATALOGUE`, there is one row per model field
    /// (229), and the catalogue's arena set is exactly `KNOWN_ARENAS`. The exact
    /// equality is the relationship the fixtures confirm — every arena a fixture
    /// can populate is a catalogue arena, and every catalogue arena is a name
    /// `KNOWN_ARENAS` tracks. A single production site (`native::attach`) emits
    /// arenas, all of them catalogue-driven, so no non-catalogued arena exists.
    #[test]
    fn catalogue_arenas_match_known_arenas() {
        use cadmpeg_ir::native::catalogue::Phase;

        use crate::native::catalogue::CATALOGUE;

        assert_eq!(CATALOGUE.len(), 229, "one catalogue row per model field");
        assert_eq!(
            CATALOGUE
                .iter()
                .filter(|row| row.phase == Phase::GroupA)
                .count(),
            107,
            "group A family count"
        );
        assert_eq!(
            CATALOGUE
                .iter()
                .filter(|row| row.phase == Phase::GroupB)
                .count(),
            9,
            "group B family count"
        );

        let mut catalogue_arenas = BTreeSet::new();
        for row in CATALOGUE {
            assert!(
                catalogue_arenas.insert(row.arena),
                "arena {:?} appears in more than one catalogue row",
                row.arena
            );
        }
        assert_eq!(
            catalogue_arenas.len(),
            CATALOGUE.len(),
            "every catalogue row owns a distinct arena"
        );

        let known: BTreeSet<&str> = KNOWN_ARENAS.iter().copied().collect();
        let catalogue_not_known: Vec<&str> = catalogue_arenas.difference(&known).copied().collect();
        let known_not_catalogue: Vec<&str> = known.difference(&catalogue_arenas).copied().collect();
        assert!(
            catalogue_not_known.is_empty(),
            "catalogue arenas absent from KNOWN_ARENAS: {catalogue_not_known:?}"
        );
        assert!(
            known_not_catalogue.is_empty(),
            "KNOWN_ARENAS entries absent from CATALOGUE: {known_not_catalogue:?}"
        );
    }
}
