// SPDX-License-Identifier: Apache-2.0
mod resource_limits;
mod work_admission;
#[test]
fn document_mesh_budget_admission_refuses_overflow() {
    let budget = super::MeshBudget {
        used: usize::MAX,
        limit: usize::MAX,
    };
    assert!(format!(
        "{:?}",
        budget.admit(&cadmpeg_test_support::service_decode_context(), 1)
    )
    .contains("ResourceLimit"));
}

#[test]
fn numerical_audit_quad_uses_shorter_large_diagonal() {
    let vertices = [
        Point3::new(0.0, 0.0, 0.0),
        Point3::new(1.0e200, 0.0, 0.0),
        Point3::new(2.0e200, 2.0e200, 0.0),
        Point3::new(0.0, 1.0e200, 0.0),
    ];
    assert_eq!(
        with_expand(&[], |expand| {
            super::triangulate_faces(expand.ctx(), &[[0, 1, 2, 3]], &vertices, |point| point)
        })
        .expect("one quad fits the service limit"),
        vec![[0, 1, 3], [1, 2, 3]]
    );
}

use std::io::Write;

use cadmpeg_core::decode::{DecodeArena, DecodePolicy};
use cadmpeg_ir::scalar::FiniteBinary32;
use flate2::write::ZlibEncoder;
use flate2::Compression;

use super::{
    consume_optional_chunk, decode, parse_f32_points, parse_mesh_correspondence_userdata,
    quad_face_count, read_buffer, read_faces, read_mapping_tag, read_ngons, read_raw_channels,
    read_v4v5_ngon_userdata, synchronization_ok, triangulate_faces, MeshBudget, MeshBufferSpec,
    MeshDecodeOptions, MeshExpand, MAX_BUFFER_OUTPUT, OPENNURBS4, V4V5_MESH_NGON_USERDATA,
    V5_MESH_DOUBLE_VERTICES,
};
use crate::chunks::{chunk_at, ArchiveVersion, BoundedReader, FramingError};
use crate::curves::GeometryError;
use crate::loss::Diagnostics;
use crate::objects::{ClassUserdata, UserdataDescriptor};
use crate::settings::MillimeterScale;
use cadmpeg_core::decode::DecodeContext;
use cadmpeg_ir::math::{Point3, Vector3};
use std::ops::Range;

fn with_expand<R>(data: &[u8], f: impl FnOnce(MeshExpand<'_>) -> R) -> R {
    let arena = DecodeArena::new();
    let policy = DecodePolicy::default();
    let (ctx, root) = DecodeContext::from_root_bytes(data, &arena, &policy).expect("root view");
    f(MeshExpand::new(&ctx, root))
}

/// A vertex channel's length is a fact of the detached payload, not of any
/// byte of the file, so its refusal names no offset instead of byte 0.
#[test]
fn a_point_channel_refusal_names_no_byte() {
    let error = with_expand(&[0_u8; 5], |expand| {
        parse_f32_points(expand.ctx(), &[0_u8; 5]).expect_err("channel length")
    });
    assert!(matches!(
        error,
        GeometryError::Malformed(FramingError::Unpositioned { ref message })
            if message == "invalid f32 point channel length"
    ));
    assert_eq!(
        error.to_string(),
        "framing error: invalid f32 point channel length"
    );
}

#[test]
fn mesh_float_point_lanes_refuse_before_allocating_their_counts() {
    let mut policy = DecodePolicy::service();
    policy.limits.max_collection_items = 0;
    let f32_refusal = with_expand_policy(&[0_u8; 12], policy, |expand| {
        parse_f32_points(expand.ctx(), &[0_u8; 12]).expect_err("one f32 point exceeds zero")
    });
    assert!(matches!(
        f32_refusal,
        GeometryError::Codec(cadmpeg_core::CodecError::ResourceLimit(limit))
            if limit.operation == "Rhino mesh f32 points"
    ));
    let f64_refusal = with_expand_policy(&[0_u8; 24], policy, |expand| {
        super::parse_f64_points(expand.ctx(), &[0_u8; 24])
            .expect_err("one f64 point exceeds zero")
    });
    assert!(matches!(
        f64_refusal,
        GeometryError::Codec(cadmpeg_core::CodecError::ResourceLimit(limit))
            if limit.operation == "Rhino mesh f64 points"
    ));
}

fn raw_channels_with_one_vertex([normal, uv, curvature, color]: [bool; 4]) -> Vec<u8> {
    let mut raw = Vec::new();
    raw.extend(1_i32.to_le_bytes());
    raw.extend([0_u8; 12]);
    for (present, bytes) in [(normal, 12), (uv, 8), (curvature, 16), (color, 4)] {
        raw.extend(i32::from(present).to_le_bytes());
        if present {
            raw.extend(std::iter::repeat_n(0_u8, bytes));
        }
    }
    raw
}

#[test]
fn raw_mesh_channels_refuse_retained_copies_before_allocation() {
    for (uv, curvature, color, _byte_limit, operation) in [
        (true, false, false, 7, "Rhino mesh raw UV channel"),
        (false, true, false, 15, "Rhino mesh raw curvature channel"),
        (false, false, true, 3, "Rhino mesh raw color channel"),
    ] {
        let raw = raw_channels_with_one_vertex([false, uv, curvature, color]);
        let run = |cap| {
            let mut policy = DecodePolicy::service();
            policy.limits.max_retained_bytes = cap;
            let refused = with_expand_policy(&raw, policy, |expand| {
                let mut reader = BoundedReader::new(&raw, 0, raw.len()).expect("reader");
                read_raw_channels(
                    expand.ctx(),
                    &mut expand
                        .ctx()
                        .reserve_scoped(0, "Rhino mesh source vertex scratch")
                        .expect("scratch"),
                    &mut reader,
                    1,
                    &mut super::MeshChannels::default(),
                )
                .expect_err("raw channel bytes exceed the retention limit")
            });
            refused
        };
        let refused = run(crate::test_support::retained_limit_at(
            operation,
            0,
            |cap| match run(cap) {
                GeometryError::Codec(cadmpeg_core::CodecError::ResourceLimit(limit)) => limit,
                error => panic!("unexpected resource refusal: {error:?}"),
            },
        ));
        assert!(matches!(
            refused,
            GeometryError::Codec(cadmpeg_core::CodecError::ResourceLimit(limit))
                if limit.operation == operation
        ));
    }
}

#[test]
fn raw_mesh_normal_collection_refusal_is_not_a_warning() {
    let raw = raw_channels_with_one_vertex([true, false, false, false]);
    // One vertex and one projected normal are the only numeric collections.
    cadmpeg_test_support::refusal::resource_limit_at(
        cadmpeg_core::decode::ResourceDimension::CollectionItems,
        "Rhino mesh f32 normals",
        |cap| {
            let mut policy = DecodePolicy::service();
            policy.limits.max_collection_items = cap;
            let refused = with_expand_policy(&raw, policy, |expand| {
                let mut reader = BoundedReader::new(&raw, 0, raw.len()).expect("reader");
                read_raw_channels(
                    expand.ctx(),
                    &mut expand
                        .ctx()
                        .reserve_scoped(0, "Rhino mesh source vertex scratch")
                        .expect("scratch"),
                    &mut reader,
                    1,
                    &mut super::MeshChannels::default(),
                )
                .expect_err("normal output exceeds the collection limit")
            });
            match refused {
                GeometryError::Codec(error) => Err::<(), _>(error),
                error => panic!("unexpected refusal: {error:?}"),
            }
        },
    );
}

#[test]
fn negative_raw_channel_diagnostic_refuses_collection_limit() {
    let raw = (-1_i32).to_le_bytes();
    let mut policy = DecodePolicy::service();
    policy.limits.max_collection_items = 0;
    let refused = with_expand_policy(&raw, policy, |expand| {
        let mut reader = BoundedReader::new(&raw, 0, raw.len()).expect("reader");
        super::read_counted_raw(
            expand.ctx(),
            &mut reader,
            1,
            12,
            "normals",
            &mut Diagnostics::new(),
        )
        .expect_err("diagnostic exceeds zero collection items")
    });
    assert!(matches!(
        refused,
        GeometryError::Codec(cadmpeg_core::CodecError::ResourceLimit(limit))
            if limit.operation == "Rhino diagnostics"
    ));
}

/// Like [`with_expand`], but under a caller-supplied policy.
fn with_expand_policy<R>(
    data: &[u8],
    policy: DecodePolicy,
    f: impl FnOnce(MeshExpand<'_>) -> R,
) -> R {
    let arena = DecodeArena::new();
    let (ctx, root) = DecodeContext::from_root_bytes(data, &arena, &policy).expect("root view");
    f(MeshExpand::new(&ctx, root))
}

fn chunk(body: &[u8]) -> Vec<u8> {
    let mut result = 0x4000_8000_u32.to_le_bytes().to_vec();
    result
        .extend((i64::try_from(body.len() + 4).expect("fixture value fits i64")).to_le_bytes());
    result.extend(body);
    result.extend(crc32fast::hash(body).to_le_bytes());
    result
}

fn buffer(value: &[u8], method: u8) -> Vec<u8> {
    let mut result = (u32::try_from(value.len()).expect("fixture value fits u32"))
        .to_le_bytes()
        .to_vec();
    result.extend(crc32fast::hash(value).to_le_bytes());
    result.push(method);
    if method == 0 {
        result.extend(value);
    } else {
        let mut encoder = ZlibEncoder::new(Vec::new(), Compression::default());
        encoder.write_all(value).expect("zlib write");
        result.extend(chunk(&encoder.finish().expect("zlib finish")));
    }
    result
}

fn v5_double_userdata_payload(points: &[[f64; 3]]) -> Vec<u8> {
    let mut body = Vec::new();
    body.extend(1_i32.to_le_bytes());
    body.extend(0_i32.to_le_bytes());
    body.extend(3_i32.to_le_bytes());
    body.extend(3_i32.to_le_bytes());
    body.extend(0_u32.to_le_bytes());
    body.extend(0_u32.to_le_bytes());
    body.extend((i32::try_from(points.len()).expect("fixture value fits i32")).to_le_bytes());
    for point in points {
        for coordinate in point {
            body.extend(coordinate.to_le_bytes());
        }
    }
    chunk(&body)
}

fn v5_double_userdata_descriptor(range: Range<usize>) -> UserdataDescriptor {
    UserdataDescriptor::Known(ClassUserdata {
        range: range.clone(),
        version: (2, 2),
        class_uuid: V5_MESH_DOUBLE_VERTICES,
        item_uuid: V5_MESH_DOUBLE_VERTICES,
        copy_count: 1,
        transform_range: 0..0,
        application_uuid: None,
        save_context: None,
        payload_range: range,
    })
}

fn v4v5_ngon_userdata_payload(
    minor: i32,
    vertices: &[i32],
    faces: &[i32],
    mesh_face_count: i32,
    mesh_vertex_count: i32,
    suffix: &[u8],
) -> Vec<u8> {
    let mut body = Vec::new();
    body.extend(1_i32.to_le_bytes());
    body.extend(minor.to_le_bytes());
    body.extend(1_i32.to_le_bytes());
    body.extend((i32::try_from(vertices.len()).expect("fixture value fits i32")).to_le_bytes());
    body.extend(vertices.iter().flat_map(|value| value.to_le_bytes()));
    body.extend(faces.iter().flat_map(|value| value.to_le_bytes()));
    if minor >= 1 {
        body.extend(mesh_face_count.to_le_bytes());
        body.extend(mesh_vertex_count.to_le_bytes());
    }
    body.extend(suffix);
    chunk(&body)
}

fn v4v5_ngon_userdata_descriptor(range: Range<usize>) -> UserdataDescriptor {
    UserdataDescriptor::Known(ClassUserdata {
        range: range.clone(),
        version: (2, 2),
        class_uuid: V4V5_MESH_NGON_USERDATA,
        item_uuid: V4V5_MESH_NGON_USERDATA,
        copy_count: 1,
        transform_range: 0..0,
        application_uuid: Some(OPENNURBS4),
        save_context: None,
        payload_range: range,
    })
}

fn correspondence_userdata_payload(version: i32, mapping: bool) -> Vec<u8> {
    let mut body = version.to_le_bytes().to_vec();
    body.extend(7_i32.to_le_bytes());
    for value in 0..30 {
        body.extend((f64::from(value)).to_le_bytes());
    }
    if mapping {
        body.extend(2_i32.to_le_bytes());
        body.extend(17_i32.to_le_bytes());
        body.extend((-1_i32).to_le_bytes());
    } else {
        body.extend(23_i32.to_le_bytes());
    }
    body.extend([0xde, 0xad]);
    body
}

#[test]
fn mesh_correspondence_userdata_reads_v1_and_rejects_later_major() {
    for mapping in [true, false] {
        let body = correspondence_userdata_payload(1, mapping);
        parse_mesh_correspondence_userdata(&body, 0..body.len(), mapping)
            .expect("version-one correspondence payload");
        let future = correspondence_userdata_payload(2, mapping);
        assert!(matches!(
            parse_mesh_correspondence_userdata(&future, 0..future.len(), mapping),
            Err(GeometryError::UnsupportedVersion { .. })
        ));
    }
}

fn compressed_mesh() -> Vec<u8> {
    let mut payload = vec![0x30];
    payload.extend(3_i32.to_le_bytes());
    payload.extend(1_i32.to_le_bytes());
    for _ in 0..4 {
        payload.extend(0.0_f64.to_le_bytes());
        payload.extend(1.0_f64.to_le_bytes());
    }
    payload.extend([0; 16]);
    payload.extend([0; 64]);
    payload.extend(0_i32.to_le_bytes());
    payload.extend([0; 5]);
    payload.extend(1_i32.to_le_bytes());
    payload.extend([0, 1, 2, 2]);
    let mut vertices = Vec::new();
    for value in [0.0_f32, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 1.0, 0.0] {
        vertices.extend(value.to_le_bytes());
    }
    payload.extend(buffer(&vertices, 0));
    for _ in 0..4 {
        payload.extend(0_u32.to_le_bytes());
    }
    payload
}

#[test]
fn compressed_mesh_retains_admitted_positions_and_normals() {
    let mut bytes = compressed_mesh();
    bytes.truncate(bytes.len() - 16);
    let mut normals = Vec::new();
    for value in [0.0_f32, 0.0, 1.0, 0.0, 0.0, 1.0, 0.0, 0.0, 1.0] {
        normals.extend(value.to_le_bytes());
    }
    bytes.extend(buffer(&normals, 0));
    for _ in 0..3 {
        bytes.extend(0_u32.to_le_bytes());
    }
    let decoded = with_expand(&bytes, |expand| {
        decode(
            expand,
            &bytes,
            0..bytes.len(),
            ArchiveVersion::V5,
            MeshDecodeOptions {
                writer_version: None,
                association: None,
                id: crate::mesh::MeshId::Ready(
                    cadmpeg_ir::tessellation::TessellationId::mint(
                        "synthetic:test:tessellation#admitted-lanes",
                    )
                    .expect("valid identity"),
                ),
                scale: MillimeterScale::IDENTITY,
                userdata: &[],
            },
            &mut MeshBudget::new(),
        )
    })
    .expect("finite mesh lanes");
    assert_eq!(
        decoded.tessellation.vertices()[1].get(),
        Point3::new(1.0, 0.0, 0.0)
    );
    let normals = decoded.tessellation.vertex_normals();
    assert_eq!(normals.len(), 3);
    assert_eq!(normals[2].get(), Vector3::new(0.0, 0.0, 1.0));
    assert!(decoded.warnings.is_empty(), "{:?}", decoded.warnings);
}

#[test]
fn mesh_scaled_vertices_refuse_before_the_output_array() {
    let bytes = compressed_mesh();
    let mut policy = DecodePolicy::service();
    policy.limits.max_collection_items = 4;
    let refused = with_expand_policy(&bytes, policy, |expand| {
        decode(
            expand,
            &bytes,
            0..bytes.len(),
            ArchiveVersion::V5,
            MeshDecodeOptions {
                writer_version: None,
                association: None,
                id: crate::mesh::MeshId::Ready(
                    cadmpeg_ir::tessellation::TessellationId::mint(
                        "synthetic:test:tessellation#scaled-limit",
                    )
                    .expect("valid identity"),
                ),
                scale: MillimeterScale::IDENTITY,
                userdata: &[],
            },
            &mut MeshBudget::new(),
        )
        .expect_err("three scaled vertices exceed four cumulative items")
    });
    assert!(matches!(
        refused,
        GeometryError::Codec(cadmpeg_core::CodecError::ResourceLimit(refusal))
            if refusal.operation == "Rhino mesh scaled vertices"
    ));
}

#[test]
fn later_mesh_fields_require_the_post_2006_writer_gate() {
    let mut bytes = compressed_mesh();
    bytes[0] = 0x35;
    bytes.extend([0_u8; 16]);
    bytes.extend(0_u32.to_le_bytes());
    bytes.extend(0_i32.to_le_bytes());
    bytes.push(0);
    let decoded = with_expand(&bytes, |expand| {
        decode(
            expand,
            &bytes,
            0..bytes.len(),
            ArchiveVersion::V5,
            MeshDecodeOptions {
                writer_version: None,
                association: None,
                id: crate::mesh::MeshId::Ready(
                    cadmpeg_ir::tessellation::TessellationId::mint(
                        "synthetic:test:tessellation#legacy-minor-five",
                    )
                    .expect("valid identity"),
                ),
                scale: MillimeterScale::IDENTITY,
                userdata: &[],
            },
            &mut MeshBudget::new(),
        )
    });
    let decoded = decoded.expect("unstamped post-2006 fields are recoverable");
    assert!(decoded.losses.iter().any(|loss| {
        loss.code == crate::loss::RhinoLossCode::SourceWriterStampUnverified.kind()
    }));
}

#[test]
fn v5_double_userdata_restores_exact_vertices_without_crc_admission() {
    let delta = 2_f64.powi(-25);
    let points = [[0.0, 0.0, 0.0], [1.0 + delta, 0.0, 0.0], [0.0, 1.0, 0.0]];
    let mut bytes = compressed_mesh();
    let payload_start = bytes.len();
    bytes.extend(v5_double_userdata_payload(&points));
    let descriptor = v5_double_userdata_descriptor(payload_start..bytes.len());
    let decoded = with_expand(&bytes, |expand| {
        decode(
            expand,
            &bytes,
            0..payload_start,
            ArchiveVersion::V5,
            MeshDecodeOptions {
                writer_version: None,
                association: None,
                id: crate::mesh::MeshId::Ready(
                    cadmpeg_ir::tessellation::TessellationId::mint(
                        "synthetic:test:tessellation#v5-double",
                    )
                    .expect("valid identity"),
                ),
                scale: MillimeterScale::IDENTITY,
                userdata: std::slice::from_ref(&descriptor),
            },
            &mut MeshBudget::new(),
        )
    })
    .expect("V5 double userdata mesh");
    assert_eq!(decoded.tessellation.vertices()[1].x, 1.0 + delta);
    assert!(decoded.warnings.is_empty(), "{:?}", decoded.warnings);
}

#[test]
fn v5_double_userdata_collections_refuse_before_allocation() {
    let points = [[0.0, 0.0, 0.0], [1.0, 0.0, 0.0], [0.0, 1.0, 0.0]];
    let mut bytes = compressed_mesh();
    let payload_start = bytes.len();
    bytes.extend(v5_double_userdata_payload(&points));
    let descriptor = v5_double_userdata_descriptor(payload_start..bytes.len());
    // The three admitted vertices are read and validated directly; no raw array exists.
    let operation = "Rhino V5 mesh admitted double vertices";
    cadmpeg_test_support::refusal::resource_limit_at(
        cadmpeg_core::decode::ResourceDimension::CollectionItems,
        operation,
        |cap| {
            let mut policy = DecodePolicy::service();
            policy.limits.max_collection_items = cap;
            let refused = with_expand_policy(&bytes, policy, |expand| {
                decode(
                    expand,
                    &bytes,
                    0..payload_start,
                    ArchiveVersion::V5,
                    MeshDecodeOptions {
                        writer_version: None,
                        association: None,
                        id: crate::mesh::MeshId::Ready(
                            cadmpeg_ir::tessellation::TessellationId::mint(
                                "synthetic:test:tessellation#v5-double-limit",
                            )
                            .expect("valid identity"),
                        ),
                        scale: MillimeterScale::IDENTITY,
                        userdata: std::slice::from_ref(&descriptor),
                    },
                    &mut MeshBudget::new(),
                )
                .expect_err("double vertex collection exceeds its item limit")
            });
            match refused {
                GeometryError::Codec(error) => Err::<(), _>(error),
                error => panic!("unexpected refusal: {error:?}"),
            }
        },
    );
}

#[test]
fn v5_double_userdata_count_mismatch_retains_float_vertices() {
    let delta = 2_f64.powi(-25);
    let points = [[0.0, 0.0, 0.0], [1.0 + delta, 0.0, 0.0]];
    let mut bytes = compressed_mesh();
    let payload_start = bytes.len();
    bytes.extend(v5_double_userdata_payload(&points));
    let descriptor = v5_double_userdata_descriptor(payload_start..bytes.len());
    let decoded = with_expand(&bytes, |expand| {
        decode(
            expand,
            &bytes,
            0..payload_start,
            ArchiveVersion::V5,
            MeshDecodeOptions {
                writer_version: None,
                association: None,
                id: crate::mesh::MeshId::Ready(
                    cadmpeg_ir::tessellation::TessellationId::mint(
                        "synthetic:test:tessellation#v5-double-mismatch",
                    )
                    .expect("valid identity"),
                ),
                scale: MillimeterScale::IDENTITY,
                userdata: std::slice::from_ref(&descriptor),
            },
            &mut MeshBudget::new(),
        )
    })
    .expect("float mesh survives V5 double userdata mismatch");
    assert_eq!(decoded.tessellation.vertices()[1].x, 1.0);
    assert!(decoded
        .warnings
        .iter()
        .any(|warning| warning.starts_with("redundant V5 mesh double-precision userdata")));
}

#[test]
fn v4v5_ngon_userdata_reports_admitted_group_count() {
    let mut bytes = compressed_mesh();
    let payload_start = bytes.len();
    bytes.extend(v4v5_ngon_userdata_payload(
        1,
        &[0, 1, 2],
        &[0, -1, -1],
        1,
        3,
        &[0xa5, 0x5a],
    ));
    let descriptor = v4v5_ngon_userdata_descriptor(payload_start..bytes.len());
    let decoded = with_expand(&bytes, |expand| {
        decode(
            expand,
            &bytes,
            0..payload_start,
            ArchiveVersion::V5,
            MeshDecodeOptions {
                writer_version: None,
                association: None,
                id: crate::mesh::MeshId::Ready(
                    cadmpeg_ir::tessellation::TessellationId::mint(
                        "synthetic:test:tessellation#v4v5-ngon",
                    )
                    .expect("valid identity"),
                ),
                scale: MillimeterScale::IDENTITY,
                userdata: std::slice::from_ref(&descriptor),
            },
            &mut MeshBudget::new(),
        )
    })
    .expect("legacy n-gon userdata mesh");
    assert_eq!(decoded.ngon_count, 1);
    assert!(decoded.warnings.is_empty(), "{:?}", decoded.warnings);
}

#[test]
fn v4v5_ngon_userdata_refuses_record_and_index_work_limits() {
    let bytes = v4v5_ngon_userdata_payload(1, &[0, 1, 2], &[0, -1, -1], 1, 3, &[]);
    let descriptor = v4v5_ngon_userdata_descriptor(0..bytes.len());
    let extra = descriptor.known().expect("known n-gon userdata");
    assert_eq!(
        with_expand(&bytes, |expand| {
            read_v4v5_ngon_userdata(expand.ctx(), &bytes, extra, ArchiveVersion::V5, 3, 1)
        })
        .expect("one n-gon fits service limits"),
        Some(1)
    );
    for operation in [
        "Rhino V4V5 mesh ngon records",
        "Rhino V4V5 mesh ngon indices",
    ] {
        let refused = cadmpeg_test_support::refusal::resource_limit_at(
            cadmpeg_core::decode::ResourceDimension::WorkUnits,
            operation,
            |cap| {
                let arena = DecodeArena::new();
                let mut policy = DecodePolicy::service();
                policy.limits.max_work_units = cap;
                let (ctx, _) = DecodeContext::from_root_bytes(&bytes, &arena, &policy)
                    .expect("fixture source fits service limits");
                match read_v4v5_ngon_userdata(
                    &ctx,
                    &bytes,
                    extra,
                    ArchiveVersion::V5,
                    3,
                    1,
                ) {
                    Err(GeometryError::Codec(error)) => {
                        assert_eq!(ctx.resource_refusal(), match error {
                            cadmpeg_core::CodecError::ResourceLimit(refusal) => Some(refusal),
                            _ => None,
                        });
                        Err::<(), _>(error)
                    }
                    Ok(_) => panic!("n-gon work boundary was not reached"),
                    Err(error) => panic!("unexpected n-gon refusal: {error:?}"),
                }
            },
        );
        assert!(matches!(
            refused,
            cadmpeg_core::CodecError::ResourceLimit(refusal)
                if refusal.operation == operation
        ));
    }
}

#[test]
fn v4v5_ngon_userdata_is_retained_in_a_later_archive_band() {
    let mut bytes = compressed_mesh();
    let payload_start = bytes.len();
    bytes.extend(v4v5_ngon_userdata_payload(
        1,
        &[0, 1, 2],
        &[0, -1, -1],
        1,
        3,
        &[],
    ));
    let descriptor = v4v5_ngon_userdata_descriptor(payload_start..bytes.len());
    let decoded = with_expand(&bytes, |expand| {
        decode(
            expand,
            &bytes,
            0..payload_start,
            ArchiveVersion::V6,
            MeshDecodeOptions {
                writer_version: None,
                association: None,
                id: crate::mesh::MeshId::Ready(
                    cadmpeg_ir::tessellation::TessellationId::mint(
                        "synthetic:test:tessellation#v4v5-ngon-later",
                    )
                    .expect("valid identity"),
                ),
                scale: MillimeterScale::IDENTITY,
                userdata: std::slice::from_ref(&descriptor),
            },
            &mut MeshBudget::new(),
        )
    })
    .expect("later-band legacy n-gon userdata mesh");
    assert_eq!(decoded.ngon_count, 1);
}

#[test]
fn v4v5_ngon_userdata_zero_counts_validate_indices() {
    let mut bytes = compressed_mesh();
    let payload_start = bytes.len();
    bytes.extend(v4v5_ngon_userdata_payload(
        0,
        &[0, 1, 2],
        &[0, -1, -1],
        0,
        0,
        &[],
    ));
    let descriptor = v4v5_ngon_userdata_descriptor(payload_start..bytes.len());
    let decoded = with_expand(&bytes, |expand| {
        decode(
            expand,
            &bytes,
            0..payload_start,
            ArchiveVersion::V5,
            MeshDecodeOptions {
                writer_version: None,
                association: None,
                id: crate::mesh::MeshId::Ready(
                    cadmpeg_ir::tessellation::TessellationId::mint(
                        "synthetic:test:tessellation#v4v5-ngon-old",
                    )
                    .expect("valid identity"),
                ),
                scale: MillimeterScale::IDENTITY,
                userdata: std::slice::from_ref(&descriptor),
            },
            &mut MeshBudget::new(),
        )
    })
    .expect("old legacy n-gon userdata mesh");
    assert_eq!(decoded.ngon_count, 1);
}

#[test]
fn v4v5_ngon_userdata_rejects_bad_validation_counts() {
    let mut bytes = compressed_mesh();
    let payload_start = bytes.len();
    bytes.extend(v4v5_ngon_userdata_payload(
        1,
        &[0, 1, 2],
        &[0, -1, -1],
        99,
        3,
        &[],
    ));
    let descriptor = v4v5_ngon_userdata_descriptor(payload_start..bytes.len());
    let decoded = with_expand(&bytes, |expand| {
        decode(
            expand,
            &bytes,
            0..payload_start,
            ArchiveVersion::V5,
            MeshDecodeOptions {
                writer_version: None,
                association: None,
                id: crate::mesh::MeshId::Ready(
                    cadmpeg_ir::tessellation::TessellationId::mint(
                        "synthetic:test:tessellation#v4v5-ngon-invalid",
                    )
                    .expect("valid identity"),
                ),
                scale: MillimeterScale::IDENTITY,
                userdata: std::slice::from_ref(&descriptor),
            },
            &mut MeshBudget::new(),
        )
    })
    .expect("float mesh survives invalid legacy n-gon userdata");
    assert_eq!(decoded.ngon_count, 0);
    assert!(decoded
        .warnings
        .iter()
        .any(|warning| warning.starts_with("V4/V5 mesh n-gon userdata")));
}

#[test]
fn v4v5_ngon_userdata_old_counts_reject_bad_indices() {
    let mut bytes = compressed_mesh();
    let payload_start = bytes.len();
    bytes.extend(v4v5_ngon_userdata_payload(
        0,
        &[0, 1, 99],
        &[0, -1, -1],
        0,
        0,
        &[],
    ));
    let descriptor = v4v5_ngon_userdata_descriptor(payload_start..bytes.len());
    let decoded = with_expand(&bytes, |expand| {
        decode(
            expand,
            &bytes,
            0..payload_start,
            ArchiveVersion::V5,
            MeshDecodeOptions {
                writer_version: None,
                association: None,
                id: crate::mesh::MeshId::Ready(
                    cadmpeg_ir::tessellation::TessellationId::mint(
                        "synthetic:test:tessellation#v4v5-ngon-bad-index",
                    )
                    .expect("valid identity"),
                ),
                scale: MillimeterScale::IDENTITY,
                userdata: std::slice::from_ref(&descriptor),
            },
            &mut MeshBudget::new(),
        )
    })
    .expect("float mesh survives invalid old legacy n-gon userdata");
    assert_eq!(decoded.ngon_count, 0);
    assert!(decoded
        .warnings
        .iter()
        .any(|warning| warning.starts_with("V4/V5 mesh n-gon userdata")));
}

#[test]
fn v4v5_ngon_userdata_crc_rejects_records() {
    let mut bytes = compressed_mesh();
    let payload_start = bytes.len();
    bytes.extend(v4v5_ngon_userdata_payload(
        1,
        &[0, 1, 2],
        &[0, -1, -1],
        1,
        3,
        &[],
    ));
    let crc = bytes.len() - 1;
    bytes[crc] ^= 1;
    let descriptor = v4v5_ngon_userdata_descriptor(payload_start..bytes.len());
    let decoded = with_expand(&bytes, |expand| {
        decode(
            expand,
            &bytes,
            0..payload_start,
            ArchiveVersion::V5,
            MeshDecodeOptions {
                writer_version: None,
                association: None,
                id: crate::mesh::MeshId::Ready(
                    cadmpeg_ir::tessellation::TessellationId::mint(
                        "synthetic:test:tessellation#v4v5-ngon-crc",
                    )
                    .expect("valid identity"),
                ),
                scale: MillimeterScale::IDENTITY,
                userdata: std::slice::from_ref(&descriptor),
            },
            &mut MeshBudget::new(),
        )
    })
    .expect("float mesh survives corrupt legacy n-gon userdata");
    assert_eq!(decoded.ngon_count, 0);
    assert!(decoded
        .warnings
        .iter()
        .any(|warning| warning.starts_with("V4/V5 mesh n-gon userdata")));
}

#[test]
fn stored_buffer_consumes_adjacent_bytes() {
    let mut bytes = buffer(&[1, 2, 3], 0);
    bytes.push(0xaa);
    with_expand(&bytes, |expand| {
        let mut reader = BoundedReader::new(&bytes, 0, bytes.len()).expect("reader");
        let mut warnings = Diagnostics::new();
        let mut document_budget = MeshBudget::new();
        assert_eq!(
            read_buffer(
                expand,
                &mut reader,
                MeshBufferSpec {
                    expected: 3,
                    name: "test"
                },
                &mut warnings,
                &mut document_budget,
                ArchiveVersion::V8,
                None
            )
            .expect("buffer")
            .as_deref(),
            Some(&[1, 2, 3][..])
        );
        assert_eq!(reader.u8().expect("adjacent"), 0xaa);
        assert!(warnings.is_empty());
    });
}

#[test]
fn zlib_buffer_consumes_one_stream_only() {
    let mut bytes = buffer(&[4, 5, 6, 7], 1);
    bytes.extend(buffer(&[8], 0));
    with_expand(&bytes, |expand| {
        let mut reader = BoundedReader::new(&bytes, 0, bytes.len()).expect("reader");
        let mut warnings = Diagnostics::new();
        let mut document_budget = MeshBudget::new();
        assert_eq!(
            read_buffer(
                expand,
                &mut reader,
                MeshBufferSpec {
                    expected: 4,
                    name: "test"
                },
                &mut warnings,
                &mut document_budget,
                ArchiveVersion::V8,
                None
            )
            .expect("buffer")
            .as_deref(),
            Some(&[4, 5, 6, 7][..])
        );
        assert_eq!(
            read_buffer(
                expand,
                &mut reader,
                MeshBufferSpec {
                    expected: 1,
                    name: "test"
                },
                &mut warnings,
                &mut document_budget,
                ArchiveVersion::V8,
                None
            )
            .expect("next")
            .as_deref(),
            Some(&[8][..])
        );
    });
}

#[test]
fn crc_mismatch_consumes_boundary_drops_channel_and_retains_budget_charge() {
    let mut bytes = buffer(&[1, 2], 0);
    bytes[4..8].copy_from_slice(&0_u32.to_le_bytes());
    with_expand(&bytes, |expand| {
        let mut reader = BoundedReader::new(&bytes, 0, bytes.len()).expect("reader");
        let mut warnings = Diagnostics::new();
        let mut document_budget = MeshBudget::new();
        assert_eq!(
            read_buffer(
                expand,
                &mut reader,
                MeshBufferSpec {
                    expected: 2,
                    name: "test"
                },
                &mut warnings,
                &mut document_budget,
                ArchiveVersion::V8,
                None
            )
            .expect("buffer"),
            None
        );
        assert_eq!(reader.remaining(), 0);
        assert_eq!(warnings.len(), 1);
        assert_eq!(document_budget.used, 2);
    });
}

#[test]
fn dropped_compressed_buffer_keeps_its_document_budget_charge() {
    // Wrong stored CRC: inflate succeeds, CRC fails, buffer is dropped; the
    // document budget must still charge the retained arena bytes.
    let mut bytes = buffer(&[1, 2, 3, 4], 1);
    bytes[4..8].copy_from_slice(&0_u32.to_le_bytes());
    let mut document_budget = MeshBudget::with_limit(4);
    with_expand(&bytes, |expand| {
        let mut warnings = Diagnostics::new();
        let mut first = BoundedReader::new(&bytes, 0, bytes.len()).expect("reader");
        assert_eq!(
            read_buffer(
                expand,
                &mut first,
                MeshBufferSpec {
                    expected: 4,
                    name: "first"
                },
                &mut warnings,
                &mut document_budget,
                ArchiveVersion::V8,
                None
            )
            .expect("first buffer inflates then drops"),
            None
        );
        assert_eq!(document_budget.used, 4);
        assert_eq!(warnings.len(), 1);
        let mut second = BoundedReader::new(&bytes, 0, bytes.len()).expect("reader");
        let refused = read_buffer(
            expand,
            &mut second,
            MeshBufferSpec {
                expected: 4,
                name: "second",
            },
            &mut warnings,
            &mut document_budget,
            ArchiveVersion::V8,
            None,
        );
        assert!(
            refused.is_err(),
            "a dropped-but-retained buffer must still occupy the document cap"
        );
    });
}

#[test]
fn bad_method_and_truncated_zlib_fail() {
    let mut bad = vec![1, 0, 0, 0];
    bad.extend(0_u32.to_le_bytes());
    bad.push(9);
    with_expand(&bad, |expand| {
        let mut reader = BoundedReader::new(&bad, 0, bad.len()).expect("reader");
        assert!(read_buffer(
            expand,
            &mut reader,
            MeshBufferSpec {
                expected: 1,
                name: "bad"
            },
            &mut Diagnostics::new(),
            &mut MeshBudget::new(),
            ArchiveVersion::V8,
            None
        )
        .is_err());
    });
    let mut truncated = buffer(&[1, 2, 3], 1);
    truncated.truncate(truncated.len() - 2);
    with_expand(&truncated, |expand| {
        let mut reader = BoundedReader::new(&truncated, 0, truncated.len()).expect("reader");
        assert!(read_buffer(
            expand,
            &mut reader,
            MeshBufferSpec {
                expected: 3,
                name: "short"
            },
            &mut Diagnostics::new(),
            &mut MeshBudget::new(),
            ArchiveVersion::V8,
            None
        )
        .is_err());
    });
}

#[test]
fn output_cap_rejects_before_allocation() {
    let mut bytes = (u32::try_from(MAX_BUFFER_OUTPUT).expect("cap") + 1)
        .to_le_bytes()
        .to_vec();
    bytes.extend([0; 5]);
    with_expand(&bytes, |expand| {
        let mut reader = BoundedReader::new(&bytes, 0, bytes.len()).expect("reader");
        assert!(read_buffer(
            expand,
            &mut reader,
            MeshBufferSpec {
                expected: 1,
                name: "bomb"
            },
            &mut Diagnostics::new(),
            &mut MeshBudget::new(),
            ArchiveVersion::V8,
            None
        )
        .is_err());
    });
}

#[test]
fn document_buffer_ceiling_refuses_before_another_channel() {
    let bytes = buffer(&[1], 0);
    with_expand(&bytes, |expand| {
        let mut reader = BoundedReader::new(&bytes, 0, bytes.len()).expect("reader");
        let mut document_budget = MeshBudget {
            used: MAX_BUFFER_OUTPUT,
            limit: MAX_BUFFER_OUTPUT,
        };
        assert!(matches!(read_buffer(expand, &mut reader,
            MeshBufferSpec { expected: 1, name: "budget" }, &mut Diagnostics::new(),
            &mut document_budget, ArchiveVersion::V8, None),
            Err(GeometryError::Codec(cadmpeg_core::CodecError::ResourceLimit(limit)))
                if limit.operation == "Rhino document mesh buffer bytes"));
    });
}

#[test]
fn document_buffer_budget_is_shared_across_meshes() {
    let bytes = buffer(&[1], 0);
    let mut document_budget = MeshBudget::with_limit(1);
    with_expand(&bytes, |expand| {
        for expected_success in [true, false] {
            let mut reader = BoundedReader::new(&bytes, 0, bytes.len()).expect("reader");
            let result = read_buffer(
                expand,
                &mut reader,
                MeshBufferSpec {
                    expected: 1,
                    name: "aggregate",
                },
                &mut Diagnostics::new(),
                &mut document_budget,
                ArchiveVersion::V8,
                None,
            );
            assert_eq!(result.is_ok(), expected_success);
        }
    });
}

#[test]
fn document_budget_rejects_second_complete_mesh() {
    let bytes = compressed_mesh();
    let mut budget = MeshBudget::with_limit(36);
    with_expand(&bytes, |expand| {
        decode(
            expand,
            &bytes,
            0..bytes.len(),
            ArchiveVersion::V5,
            MeshDecodeOptions {
                writer_version: None,
                association: None,
                id: crate::mesh::MeshId::Ready(
                    cadmpeg_ir::tessellation::TessellationId::mint(
                        "synthetic:test:tessellation#first",
                    )
                    .expect("valid identity"),
                ),
                scale: MillimeterScale::IDENTITY,
                userdata: &[],
            },
            &mut budget,
        )
        .expect("first mesh");
        let error = decode(
            expand,
            &bytes,
            0..bytes.len(),
            ArchiveVersion::V5,
            MeshDecodeOptions {
                writer_version: None,
                association: None,
                id: crate::mesh::MeshId::Ready(
                    cadmpeg_ir::tessellation::TessellationId::mint(
                        "synthetic:test:tessellation#second",
                    )
                    .expect("valid identity"),
                ),
                scale: MillimeterScale::IDENTITY,
                userdata: &[],
            },
            &mut budget,
        )
        .expect_err("second mesh exceeds aggregate budget");
        assert!(matches!(error,
            GeometryError::Codec(cadmpeg_core::CodecError::ResourceLimit(limit))
                if limit.operation == "Rhino document mesh buffer bytes"));
    });
}

#[test]
fn optional_chunks_use_absolute_offsets() {
    let mut bytes = vec![0; 11];
    bytes.extend(chunk(&[1, 2, 3]));
    let end = bytes.len();
    let mut reader = BoundedReader::new(&bytes, 11, end).expect("reader");
    consume_optional_chunk(
        &mut reader,
        ArchiveVersion::V5,
        &mut Diagnostics::new(),
        "optional",
    )
    .expect("chunk");
    assert_eq!(reader.position(), end);
}

#[test]
fn face_widths_and_quad_split_are_deterministic() {
    for (vertices, width) in [(255_usize, 1_i32), (256, 2), (65_535, 2), (65_536, 4)] {
        let mut bytes = width.to_le_bytes().to_vec();
        for index in [0_u32, 1, 2, 2] {
            match width {
                1 => bytes.push(u8::try_from(index).expect("fixture value fits u8")),
                2 => bytes.extend(
                    (u16::try_from(index).expect("fixture value fits u16")).to_le_bytes(),
                ),
                4 => bytes.extend(index.to_le_bytes()),
                _ => unreachable!(),
            }
        }
        with_expand(&bytes, |expand| {
            let mut reader = BoundedReader::new(&bytes, 0, bytes.len()).expect("reader");
            assert_eq!(
                read_faces(expand.ctx(), &mut reader, vertices, 1).expect("face"),
                vec![[0, 1, 2, 2]]
            );
        });
    }
}

#[test]
fn synchronization_uses_relative_max_coordinate_tolerance() {
    assert!(synchronization_ok(
        &cadmpeg_test_support::service_decode_context(),
        &[[0.0, 0.0, 0.0]],
        &[([0.0, 0.0, 0.0].map(|value| FiniteBinary32::new(value).expect("finite")))]
    )
    .expect("synchronization scan admitted"));
    assert!(synchronization_ok(
        &cadmpeg_test_support::service_decode_context(),
        &[[1_000_000.0, 0.0, 0.0]],
        &[([1_000_000.5, 0.0, 0.0].map(|value| FiniteBinary32::new(value).expect("finite")))]
    )
    .expect("synchronization scan admitted"));
    assert!(!synchronization_ok(
        &cadmpeg_test_support::service_decode_context(),
        &[[1_000_000.0, 0.0, 0.0]],
        &[([1_002.0, 0.0, 0.0].map(|value| FiniteBinary32::new(value).expect("finite")))]
    )
    .expect("synchronization scan admitted"));
}

#[test]
fn mapping_and_ngon_chunks_validate_nested_versions() {
    let mut mapping = 1_i32.to_le_bytes().to_vec();
    mapping.extend(1_i32.to_le_bytes());
    mapping.extend([0; 16]);
    mapping.extend(7_i32.to_le_bytes());
    mapping.extend((0..16).flat_map(|_| 1.0_f64.to_le_bytes()));
    mapping.extend(3_u32.to_le_bytes());
    let mapping = chunk(&mapping);
    let mut bytes = vec![0; 3];
    bytes.extend(mapping);
    let end = bytes.len();
    let mut reader = BoundedReader::new(&bytes, 3, end).expect("reader");
    read_mapping_tag(
        &cadmpeg_test_support::service_decode_context(),
        &mut reader,
        ArchiveVersion::V5,
        &mut Diagnostics::new(),
    )
    .expect("mapping");

    let mut ngon = 1_i32.to_le_bytes().to_vec();
    ngon.extend(0_i32.to_le_bytes());
    ngon.extend(1_u32.to_le_bytes());
    ngon.extend(3_u32.to_le_bytes());
    ngon.extend([0_u32, 1, 2].into_iter().flat_map(u32::to_le_bytes));
    ngon.extend(1_u32.to_le_bytes());
    let ngon = chunk(&ngon);
    let mut bytes = vec![0; 5];
    bytes.extend(ngon);
    let end = bytes.len();
    let mut reader = BoundedReader::new(&bytes, 5, end).expect("reader");
    read_ngons(
        &cadmpeg_test_support::service_decode_context(),
        &mut reader,
        ArchiveVersion::V5,
        3,
        1,
        &mut Diagnostics::new(),
    )
    .expect("ngon");
}

#[test]
fn nested_mapping_crc_mismatch_warns_and_consumes_boundary() {
    let mut mapping = 1_i32.to_le_bytes().to_vec();
    mapping.extend(1_i32.to_le_bytes());
    mapping.extend([0; 16]);
    mapping.extend(7_i32.to_le_bytes());
    mapping.extend((0..16).flat_map(|_| 1.0_f64.to_le_bytes()));
    mapping.extend(3_u32.to_le_bytes());
    let mut bytes = chunk(&mapping);
    let crc = bytes.len() - 1;
    bytes[crc] ^= 1;
    let end = bytes.len();
    let mut reader = BoundedReader::new(&bytes, 0, end).expect("reader");
    let mut warnings = Diagnostics::new();
    read_mapping_tag(
        &cadmpeg_test_support::service_decode_context(),
        &mut reader,
        ArchiveVersion::V5,
        &mut warnings,
    )
    .expect("mapping");
    assert_eq!(reader.position(), end);
    assert_eq!(warnings.len(), 1);
    assert!(warnings[0].contains("mapping tag CRC mismatch"));
}

#[test]
fn mapping_crc_diagnostic_refuses_collection_limit() {
    let mut mapping = 1_i32.to_le_bytes().to_vec();
    mapping.extend(1_i32.to_le_bytes());
    mapping.extend([0; 16]);
    mapping.extend(7_i32.to_le_bytes());
    mapping.extend((0..16).flat_map(|_| 1.0_f64.to_le_bytes()));
    mapping.extend(3_u32.to_le_bytes());
    let mut bytes = chunk(&mapping);
    let crc = bytes.len() - 1;
    bytes[crc] ^= 1;
    let mut policy = DecodePolicy::service();
    policy.limits.max_collection_items = 0;
    let refused = with_expand_policy(&bytes, policy, |expand| {
        let mut reader = BoundedReader::new(&bytes, 0, bytes.len()).expect("reader");
        read_mapping_tag(
            expand.ctx(),
            &mut reader,
            ArchiveVersion::V5,
            &mut Diagnostics::new(),
        )
        .expect_err("checksum diagnostic exceeds zero collection items")
    });
    assert!(matches!(
        refused,
        GeometryError::Codec(cadmpeg_core::CodecError::ResourceLimit(limit))
            if limit.operation == "Rhino diagnostics"
    ));
}

#[test]
fn future_v5_mesh_minor_is_retained_unsupported() {
    let bytes = [0x38_u8];
    let result = with_expand(&bytes, |expand| {
        decode(
            expand,
            &bytes,
            0..bytes.len(),
            ArchiveVersion::V5,
            MeshDecodeOptions {
                writer_version: None,
                association: None,
                id: crate::mesh::MeshId::Ready(
                    cadmpeg_ir::tessellation::TessellationId::mint(
                        "synthetic:test:tessellation#future-v5-minor",
                    )
                    .expect("valid identity"),
                ),
                scale: MillimeterScale::IDENTITY,
                userdata: &[],
            },
            &mut MeshBudget::new(),
        )
    });
    assert!(matches!(
        result,
        Err(GeometryError::UnsupportedVersion { .. })
    ));
}

#[test]
fn archive_booleans_normalize_nonzero_values() {
    let bytes = [2_u8];
    let mut reader = BoundedReader::new(&bytes, 0, bytes.len()).expect("reader");
    assert!(reader.bool().expect("nonzero boolean"));
}

#[test]
fn nested_compressed_buffer_inflates_from_a_child_window() {
    let inner = buffer(&[9, 8, 7, 6], 1);
    let bytes = chunk(&inner);
    with_expand(&bytes, |expand| {
        let outer =
            chunk_at(&bytes, 0, bytes.len(), ArchiveVersion::V8, false).expect("outer chunk");
        let mut child = BoundedReader::new(&bytes, outer.body().start, outer.body().end)
            .expect("child reader");
        let decoded = read_buffer(
            expand,
            &mut child,
            MeshBufferSpec {
                expected: 4,
                name: "nested",
            },
            &mut Diagnostics::new(),
            &mut MeshBudget::new(),
            ArchiveVersion::V8,
            None,
        )
        .expect("nested buffer");
        assert_eq!(decoded.as_deref(), Some(&[9, 8, 7, 6][..]));
    });
}

#[test]
fn cumulative_compressed_expansion_trips_the_platform_decompression_ceiling() {
    // Two 3-byte expansions under a shared 4-byte decompression ceiling.
    let first = buffer(&[1, 2, 3], 1);
    let second = buffer(&[4, 5, 6], 1);
    let mut data = first.clone();
    data.extend_from_slice(&second);
    let mut policy = DecodePolicy::desktop();
    policy.limits.max_decompressed_bytes_total = 4;
    with_expand_policy(&data, policy, |expand| {
        let mut reader = BoundedReader::new(&data, 0, data.len()).expect("reader");
        let decoded = read_buffer(
            expand,
            &mut reader,
            MeshBufferSpec {
                expected: 3,
                name: "first",
            },
            &mut Diagnostics::new(),
            &mut MeshBudget::new(),
            ArchiveVersion::V8,
            None,
        )
        .expect("first expansion");
        assert_eq!(decoded.as_deref(), Some(&[1, 2, 3][..]));
        let refused = read_buffer(
            expand,
            &mut reader,
            MeshBufferSpec {
                expected: 3,
                name: "second",
            },
            &mut Diagnostics::new(),
            &mut MeshBudget::new(),
            ArchiveVersion::V8,
            None,
        );
        assert!(refused.is_err(), "cumulative expansion must be refused");
    });
}

#[test]
fn compressed_buffer_expansion_limit_is_a_codec_resource_refusal() {
    let data = buffer(&[1, 2, 3], 1);
    let mut policy = DecodePolicy::service();
    policy.limits.max_decompressed_bytes_total = 2;
    let refused = with_expand_policy(&data, policy, |expand| {
        let mut reader = BoundedReader::new(&data, 0, data.len()).expect("reader");
        read_buffer(
            expand,
            &mut reader,
            MeshBufferSpec {
                expected: 3,
                name: "vertices",
            },
            &mut Diagnostics::new(),
            &mut MeshBudget::new(),
            ArchiveVersion::V8,
            None,
        )
        .expect_err("three expanded bytes exceed the two-byte limit")
    });
    assert!(matches!(
        refused,
        GeometryError::Codec(cadmpeg_core::CodecError::ResourceLimit(_))
    ));
}

#[test]
fn stored_buffer_retention_limit_refuses_before_copy() {
    let data = buffer(&[1, 2, 3], 0);
    let mut policy = DecodePolicy::service();
    policy.limits.max_retained_bytes = 2;
    let refused = with_expand_policy(&data, policy, |expand| {
        let mut reader = BoundedReader::new(&data, 0, data.len()).expect("reader");
        read_buffer(
            expand,
            &mut reader,
            MeshBufferSpec {
                expected: 3,
                name: "vertices",
            },
            &mut Diagnostics::new(),
            &mut MeshBudget::new(),
            ArchiveVersion::V8,
            None,
        )
        .expect_err("three stored bytes exceed the two-byte limit")
    });
    assert!(matches!(
        refused,
        GeometryError::Codec(cadmpeg_core::CodecError::ResourceLimit(_))
    ));
}

#[test]
fn read_faces_preserves_quad_indices_until_vertices_are_available() {
    let mut raw = 1_i32.to_le_bytes().to_vec();
    raw.extend([0, 1, 2, 2]); // triangle (indices[2] == indices[3])
    raw.extend([0, 1, 2, 0]); // quad -> two triangles
    with_expand(&raw, |expand| {
        let mut reader = BoundedReader::new(&raw, 0, raw.len()).expect("reader");
        let faces = read_faces(expand.ctx(), &mut reader, 3, 2).expect("faces");
        assert_eq!(faces, vec![[0, 1, 2, 2], [0, 1, 2, 0]]);
    });
}

#[test]
fn mesh_faces_refuse_a_collection_limit_below_the_face_count() {
    let mut raw = 1_i32.to_le_bytes().to_vec();
    raw.extend([0, 1, 2, 2]);
    let mut policy = DecodePolicy::service();
    policy.limits.max_collection_items = 0;
    let refused = with_expand_policy(&raw, policy, |expand| {
        let mut reader = BoundedReader::new(&raw, 0, raw.len()).expect("reader");
        read_faces(expand.ctx(), &mut reader, 3, 1)
            .expect_err("one face exceeds zero collection items")
    });
    assert!(matches!(
        refused,
        GeometryError::Codec(cadmpeg_core::CodecError::ResourceLimit(limit))
            if limit.operation == "Rhino mesh faces"
    ));
}

#[test]
fn mesh_triangles_refuse_a_collection_limit_below_the_quad_output() {
    let vertices = [
        Point3::new(0.0, 0.0, 0.0),
        Point3::new(1.0, 0.0, 0.0),
        Point3::new(1.0, 1.0, 0.0),
        Point3::new(0.0, 1.0, 0.0),
    ];
    let mut policy = DecodePolicy::service();
    policy.limits.max_collection_items = 1;
    let refused = with_expand_policy(&[], policy, |expand| {
        triangulate_faces(expand.ctx(), &[[0, 1, 2, 3]], &vertices, |point| point)
            .expect_err("two triangles exceed one collection item")
    });
    assert!(matches!(
        refused,
        GeometryError::Codec(cadmpeg_core::CodecError::ResourceLimit(limit))
            if limit.operation == "Rhino mesh triangles"
    ));
}

#[test]
fn quad_uses_shorter_diagonal_and_collapses_duplicate_vertex() {
    let vertices = vec![
        Point3::new(0.0, 0.0, 0.0),
        Point3::new(2.0, 0.0, 0.0),
        Point3::new(2.0, 2.0, 0.0),
        Point3::new(1.0, 1.0, 0.0),
    ];
    assert_eq!(
        with_expand(&[], |expand| {
            triangulate_faces(
                expand.ctx(),
                &[[0, 1, 2, 3], [0, 1, 2, 2]],
                &vertices,
                |point| point,
            )
        })
        .expect("quad and triangle fit the service limit"),
        vec![[0, 1, 3], [1, 2, 3], [0, 1, 2]]
    );
    assert_eq!(
        quad_face_count(
            &cadmpeg_test_support::service_decode_context(),
            &[[0, 1, 2, 3], [0, 1, 2, 2]]
        )
        .expect("quad count admitted"),
        1
    );
}

#[test]
fn read_faces_truncated_at_record_boundary() {
    let mut raw = 1_i32.to_le_bytes().to_vec();
    raw.extend([0, 1, 2, 2]); // only one of the two declared faces
    with_expand(&raw, |expand| {
        let mut reader = BoundedReader::new(&raw, 0, raw.len()).expect("reader");
        assert!(read_faces(expand.ctx(), &mut reader, 3, 2).is_err());
    });
}
