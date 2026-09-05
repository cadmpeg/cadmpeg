// SPDX-License-Identifier: Apache-2.0
#![allow(dead_code, clippy::disallowed_methods)]

use super::*;
use crate::test_support::test_dump::*;
use cadmpeg_ir::report::Severity;

#[derive(Clone, Copy)]
#[expect(
    clippy::struct_excessive_bools,
    reason = "independent fixture toggles model orthogonal archive mutations"
)]
struct Fixture {
    archive: ArchiveVersion,
    minor: i32,
    reversed_edge: bool,
    open_ring: bool,
    bad_pointer_type: bool,
    null_endpoint: bool,
    omit_vertex_edge: bool,
    vertex_tag: u8,
    edge_tag: u8,
    end_sharpness: f64,
    level_count: usize,
    render_mesh: bool,
    future_additions: bool,
    saved_limit_points: bool,
    symmetry_type: u8,
    symmetry_coordinate_system: u8,
}

impl Default for Fixture {
    fn default() -> Self {
        Self {
            archive: ArchiveVersion::V5,
            minor: 0,
            reversed_edge: false,
            open_ring: false,
            bad_pointer_type: false,
            null_endpoint: false,
            omit_vertex_edge: false,
            vertex_tag: 1,
            edge_tag: 1,
            end_sharpness: 0.25,
            level_count: 1,
            render_mesh: false,
            future_additions: false,
            saved_limit_points: false,
            symmetry_type: 0,
            symmetry_coordinate_system: 0,
        }
    }
}

fn anonymous(body: &[u8]) -> Vec<u8> {
    let mut bytes = ANONYMOUS.to_le_bytes().to_vec();
    bytes.extend_from_slice(
        &i64::try_from(body.len() + 4)
            .expect("anonymous length")
            .to_le_bytes(),
    );
    bytes.extend_from_slice(body);
    bytes.extend_from_slice(&crc32fast::hash(body).to_le_bytes());
    bytes
}

fn anonymous_mixed(body: &[u8], children: &[Range<usize>]) -> Vec<u8> {
    let direct = crate::chunks::direct_checksum_ranges(&(0..body.len()), children)
        .expect("valid SubD fixture children");
    let mut hasher = crc32fast::Hasher::new();
    for range in direct {
        hasher.update(&body[range]);
    }
    let mut bytes = ANONYMOUS.to_le_bytes().to_vec();
    bytes.extend_from_slice(
        &i64::try_from(body.len() + 4)
            .expect("anonymous length")
            .to_le_bytes(),
    );
    bytes.extend_from_slice(body);
    bytes.extend_from_slice(&hasher.finalize().to_le_bytes());
    bytes
}

fn rotate_symmetry(symmetry_type: u8, include_legacy_plane: bool) -> Vec<u8> {
    rotate_symmetry_with_coordinate(symmetry_type, include_legacy_plane, 0)
}

fn rotate_symmetry_with_coordinate(
    symmetry_type: u8,
    include_legacy_plane: bool,
    coordinate_system: u8,
) -> Vec<u8> {
    let mut transform = 1_i32.to_le_bytes().to_vec();
    transform.extend(2_i32.to_le_bytes());
    for value in [0.0_f64, 0.0, 0.0, 0.0, 0.0, 1.0] {
        transform.extend(value.to_le_bytes());
    }
    if include_legacy_plane {
        for _ in 0..4 {
            transform.extend(f64::NAN.to_le_bytes());
        }
    }
    let mut body = 1_i32.to_le_bytes().to_vec();
    body.extend(2_i32.to_le_bytes());
    body.push(symmetry_type);
    body.extend(0_u32.to_le_bytes());
    body.extend(0_u32.to_le_bytes());
    body.extend([0_u8; 16]);
    body.extend(anonymous(&transform));
    body.push(coordinate_system);
    anonymous(&body)
}

#[test]
fn rotate_symmetry_accepts_nan_padding_and_prototype_omission() {
    for bytes in [rotate_symmetry(2, true), rotate_symmetry(113, false)] {
        let mut reader =
            BoundedReader::new(&bytes, 0, bytes.len()).expect("bounded symmetry reader");
        read_symmetry(
            &mut reader,
            ArchiveVersion::V5,
            &mut Vec::new(),
            &mut Vec::new(),
        )
        .expect("rotate symmetry");
        assert_eq!(reader.remaining(), 0);
    }
}

#[test]
fn unknown_symmetry_enums_map_to_unset_without_dropping_the_chunk() {
    let mut diagnostics = Vec::new();
    let bytes = rotate_symmetry(6, false);
    let mut reader = BoundedReader::new(&bytes, 0, bytes.len()).expect("bounded symmetry reader");
    read_symmetry(
        &mut reader,
        ArchiveVersion::V5,
        &mut diagnostics,
        &mut Vec::new(),
    )
    .expect("unknown symmetry type is recoverable");
    assert_eq!(reader.remaining(), 0);
    assert_eq!(diagnostics, vec![SubdEnumDiagnostic::SymmetryType(6)]);

    let bytes = rotate_symmetry_with_coordinate(2, true, 7);
    let mut reader = BoundedReader::new(&bytes, 0, bytes.len()).expect("bounded symmetry reader");
    diagnostics.clear();
    read_symmetry(
        &mut reader,
        ArchiveVersion::V5,
        &mut diagnostics,
        &mut Vec::new(),
    )
    .expect("unknown symmetry coordinate system is recoverable");
    assert_eq!(reader.remaining(), 0);
    assert_eq!(
        diagnostics,
        vec![SubdEnumDiagnostic::SymmetryCoordinateSystem(7)]
    );
}

fn pointer(bytes: &mut Vec<u8>, id: u32, flags: u8) {
    bytes.extend_from_slice(&id.to_le_bytes());
    bytes.push(flags);
}

fn base(bytes: &mut Vec<u8>, fixture: Fixture, archive_id: u32, level: u16) {
    bytes.extend_from_slice(&archive_id.to_le_bytes());
    bytes.extend_from_slice(&(archive_id + 100).to_le_bytes());
    bytes.extend_from_slice(&level.to_le_bytes());
    if fixture.archive.value() < 70 {
        bytes.extend([0, 0]);
    } else {
        bytes.extend([0, 0, 0]);
        if fixture.future_additions {
            bytes.push(254);
            bytes.extend(anonymous(&[1, 0, 0, 0]));
            bytes.push(3);
            bytes.extend([7, 8, 9]);
        }
        bytes.push(255);
    }
}

fn record_end(bytes: &mut Vec<u8>, fixture: Fixture) {
    bytes.push(if fixture.archive.value() < 70 { 0 } else { 255 });
}

fn vertex(
    bytes: &mut Vec<u8>,
    fixture: Fixture,
    archive_id: u32,
    point_value: [f64; 3],
    edges: &[u32],
    level: u16,
) {
    base(bytes, fixture, archive_id, level);
    bytes.push(fixture.vertex_tag);
    for value in point_value {
        bytes.extend_from_slice(&value.to_le_bytes());
    }
    let serialized_edges = if fixture.omit_vertex_edge && archive_id == 1 {
        &edges[1..]
    } else {
        edges
    };
    bytes.extend_from_slice(
        &u16::try_from(serialized_edges.len())
            .expect("edge count")
            .to_le_bytes(),
    );
    bytes.extend_from_slice(&1_u16.to_le_bytes());
    if fixture.saved_limit_points && archive_id == 1 {
        bytes.push(4);
        bytes.extend_from_slice(&1_u32.to_le_bytes());
        for value in 0..12 {
            bytes.extend_from_slice(&(f64::from(value)).to_le_bytes());
        }
        pointer(bytes, 9, 0);
    } else {
        bytes.push(0);
    }
    bytes.extend_from_slice(
        &u16::try_from(serialized_edges.len())
            .expect("edge count")
            .to_le_bytes(),
    );
    for edge in serialized_edges {
        pointer(bytes, *edge, 0);
    }
    bytes.extend_from_slice(&1_u16.to_le_bytes());
    pointer(bytes, 9, 0);
    record_end(bytes, fixture);
}

fn edge(bytes: &mut Vec<u8>, fixture: Fixture, archive_id: u32, endpoints: [u32; 2], level: u16) {
    base(bytes, fixture, archive_id, level);
    bytes.push(fixture.edge_tag);
    bytes.extend_from_slice(&1_u16.to_le_bytes());
    bytes.extend_from_slice(&0.125_f64.to_le_bytes());
    bytes.extend_from_slice(&0.875_f64.to_le_bytes());
    bytes.extend_from_slice(&0.25_f64.to_le_bytes());
    bytes.extend_from_slice(&2_u16.to_le_bytes());
    let first = if fixture.null_endpoint && archive_id == 5 {
        0
    } else {
        endpoints[0]
    };
    pointer(bytes, first, 0);
    pointer(
        bytes,
        endpoints[1],
        if fixture.bad_pointer_type && archive_id == 5 {
            0x2
        } else {
            0
        },
    );
    bytes.extend_from_slice(&1_u16.to_le_bytes());
    pointer(bytes, 9, 0);
    if fixture.archive.value() < 70 {
        bytes.push(0);
    } else {
        if fixture.archive.value() >= 80 {
            bytes.push(8);
            bytes.extend_from_slice(&fixture.end_sharpness.to_le_bytes());
        }
        bytes.push(255);
    }
}

fn face(bytes: &mut Vec<u8>, fixture: Fixture, level: u16) {
    base(bytes, fixture, 9, level);
    bytes.extend_from_slice(&9_u32.to_le_bytes());
    bytes.extend_from_slice(&0_u32.to_le_bytes());
    bytes.extend_from_slice(&4_u16.to_le_bytes());
    bytes.extend_from_slice(&4_u16.to_le_bytes());
    pointer(bytes, 5, 0);
    pointer(bytes, 6, u8::from(fixture.reversed_edge));
    pointer(bytes, 7, 0);
    pointer(bytes, if fixture.open_ring { 5 } else { 8 }, 0);
    record_end(bytes, fixture);
}

fn level(fixture: Fixture, level: u16) -> Vec<u8> {
    let mut body = Vec::new();
    body.extend_from_slice(&1_i32.to_le_bytes());
    body.extend_from_slice(&1_i32.to_le_bytes());
    body.extend_from_slice(&level.to_le_bytes());
    body.extend([4, 4, 4]);
    for value in [0.0_f64, 0.0, 0.0, 1.0, 1.0, 0.0] {
        body.extend_from_slice(&value.to_le_bytes());
    }
    for partition in [1_u32, 5, 9, 10] {
        body.extend_from_slice(&partition.to_le_bytes());
    }
    vertex(&mut body, fixture, 1, [0.0, 0.0, 0.0], &[5, 8], level);
    vertex(&mut body, fixture, 2, [1.0, 0.0, 0.0], &[5, 6], level);
    vertex(&mut body, fixture, 3, [1.0, 1.0, 0.0], &[6, 7], level);
    vertex(&mut body, fixture, 4, [0.0, 1.0, 0.0], &[7, 8], level);
    edge(&mut body, fixture, 5, [1, 2], level);
    edge(
        &mut body,
        fixture,
        6,
        if fixture.reversed_edge {
            [3, 2]
        } else {
            [2, 3]
        },
        level,
    );
    edge(&mut body, fixture, 7, [3, 4], level);
    edge(&mut body, fixture, 8, [4, 1], level);
    face(&mut body, fixture, level);
    body.push(u8::from(fixture.render_mesh));
    if fixture.render_mesh {
        body.extend(anonymous(&[1, 0, 0, 0]));
    }
    anonymous(&body)
}

fn payload(fixture: Fixture) -> Vec<u8> {
    let mut body = Vec::new();
    let mut children = Vec::new();
    body.extend_from_slice(&1_i32.to_le_bytes());
    body.extend_from_slice(&fixture.minor.to_le_bytes());
    body.extend_from_slice(
        &u32::try_from(fixture.level_count)
            .expect("level count")
            .to_le_bytes(),
    );
    body.extend_from_slice(&9_u32.to_le_bytes());
    body.extend_from_slice(&9_u32.to_le_bytes());
    body.extend_from_slice(&9_u32.to_le_bytes());
    for value in [0.0_f64, 0.0, 0.0, 1.0, 1.0, 0.0] {
        body.extend_from_slice(&value.to_le_bytes());
    }
    for level_index in 0..fixture.level_count {
        let child = level(fixture, u16::try_from(level_index).expect("level index"));
        children.push(body.len()..body.len() + child.len());
        body.extend(child);
    }
    if fixture.minor >= 1 {
        body.push(0);
        let mut mapping = Vec::new();
        mapping.extend_from_slice(&1_i32.to_le_bytes());
        mapping.extend_from_slice(&0_i32.to_le_bytes());
        mapping.extend([0; 16]);
        mapping.extend_from_slice(&0_i32.to_le_bytes());
        for index in 0..16 {
            mapping.extend_from_slice(&(if index % 5 == 0 { 1.0_f64 } else { 0.0 }).to_le_bytes());
        }
        let child = anonymous(&mapping);
        children.push(body.len()..body.len() + child.len());
        body.extend(child);
    }
    if fixture.minor >= 2 {
        let child = if fixture.symmetry_type == 0 {
            let mut symmetry = Vec::new();
            symmetry.extend_from_slice(&1_i32.to_le_bytes());
            symmetry.extend_from_slice(&4_i32.to_le_bytes());
            symmetry.push(0);
            anonymous(&symmetry)
        } else {
            rotate_symmetry_with_coordinate(
                fixture.symmetry_type,
                fixture.symmetry_type == 2,
                fixture.symmetry_coordinate_system,
            )
        };
        children.push(body.len()..body.len() + child.len());
        body.extend(child);
    }
    if fixture.minor >= 3 {
        body.extend_from_slice(&42_u64.to_le_bytes());
    }
    if fixture.minor >= 4 {
        body.push(0);
        body.extend([0; 16]);
        body.push(0);
        let mut hash = Vec::new();
        hash.extend_from_slice(&1_i32.to_le_bytes());
        hash.extend_from_slice(&1_i32.to_le_bytes());
        hash.push(1);
        let child = anonymous(&hash);
        children.push(body.len()..body.len() + child.len());
        body.extend(child);
    }
    let mut payload = vec![1];
    payload.extend(anonymous_mixed(&body, &children));
    payload
}

pub(crate) fn quad_payload(archive: ArchiveVersion) -> Vec<u8> {
    payload(Fixture {
        archive,
        ..Fixture::default()
    })
}

fn decode_fixture(fixture: Fixture, scale: f64) -> Result<DecodedSubd, SubdError> {
    let bytes = payload(fixture);
    decode(
        &bytes,
        0..bytes.len(),
        fixture.archive,
        scale,
        "test:subd#0".into(),
    )
}

fn proxy_userdata(
    embedded: &[u8],
    fingerprint: MeshProxyFingerprint,
    transform_identity: bool,
) -> (Vec<u8>, UserdataDescriptor) {
    let mut bytes = Vec::new();
    for index in 0..16 {
        bytes.extend_from_slice(
            &(if transform_identity && index % 5 == 0 {
                1.0_f64
            } else {
                0.0_f64
            })
            .to_le_bytes(),
        );
    }
    let transform_range = 0..bytes.len();
    let mut body = Vec::new();
    body.extend_from_slice(&1_i32.to_le_bytes());
    body.extend_from_slice(&1_i32.to_le_bytes());
    body.push(1);
    body.extend_from_slice(embedded);
    body.extend_from_slice(
        &i32::try_from(fingerprint.face_count)
            .expect("proxy face count")
            .to_le_bytes(),
    );
    body.extend_from_slice(
        &i32::try_from(fingerprint.vertex_count)
            .expect("proxy vertex count")
            .to_le_bytes(),
    );
    for digest in [fingerprint.face_sha1, fingerprint.vertex_sha1] {
        let mut hash = Vec::new();
        hash.extend_from_slice(&1_i32.to_le_bytes());
        hash.extend_from_slice(&0_i32.to_le_bytes());
        hash.extend_from_slice(&digest);
        body.extend_from_slice(&anonymous(&hash));
    }
    let payload_start = bytes.len();
    bytes.extend_from_slice(&anonymous(&body));
    let payload_range = payload_start..bytes.len();
    let descriptor = UserdataDescriptor::Known {
        range: payload_range.clone(),
        version: (2, 2),
        class_uuid: SUBD_MESH_PROXY_USERDATA,
        item_uuid: SUBD_MESH_PROXY_USERDATA,
        copy_count: 1,
        transform_range,
        application_uuid: None,
        last_saved_as_goo: Some(false),
        archive_version: Some(50),
        writer_version: Some(202_401_010),
        payload_range,
    };
    (bytes, descriptor)
}

#[test]
fn mesh_proxy_requires_identity_and_parent_fingerprint() {
    let fingerprint = MeshProxyFingerprint {
        face_count: 4,
        vertex_count: 4,
        face_sha1: [0x11; 20],
        vertex_sha1: [0x22; 20],
    };
    let embedded = payload(Fixture::default());
    let (bytes, descriptor) = proxy_userdata(&embedded, fingerprint, true);
    let decoded = decode_mesh_proxy(
        &bytes,
        &descriptor,
        ArchiveVersion::V5,
        1.0,
        "test:proxy-subd#0".into(),
        fingerprint,
    )
    .expect("valid proxy framing")
    .expect("valid proxy transfer");
    assert!(matches!(decoded, DecodedSubd::Surface { .. }));

    let mut wrong_hash = fingerprint;
    wrong_hash.face_sha1[0] ^= 1;
    let (bytes, descriptor) = proxy_userdata(&embedded, fingerprint, true);
    assert!(decode_mesh_proxy(
        &bytes,
        &descriptor,
        ArchiveVersion::V5,
        1.0,
        "test:proxy-subd#0".into(),
        wrong_hash,
    )
    .expect("wrong hash is an admission rejection")
    .is_none());

    let empty_parent = MeshProxyFingerprint {
        face_count: 0,
        vertex_count: 0,
        face_sha1: EMPTY_CONTENT_SHA1,
        vertex_sha1: EMPTY_CONTENT_SHA1,
    };
    let (bytes, descriptor) = proxy_userdata(&embedded, empty_parent, true);
    assert!(decode_mesh_proxy(
        &bytes,
        &descriptor,
        ArchiveVersion::V5,
        1.0,
        "test:proxy-subd#0".into(),
        empty_parent,
    )
    .expect("empty parent is an admission rejection")
    .is_none());

    let (bytes, descriptor) = proxy_userdata(&embedded, fingerprint, false);
    assert!(decode_mesh_proxy(
        &bytes,
        &descriptor,
        ArchiveVersion::V5,
        1.0,
        "test:proxy-subd#0".into(),
        fingerprint,
    )
    .expect("nonidentity userdata transform is an admission rejection")
    .is_none());
}

#[test]
fn decodes_empty_outer_subd_without_carrier() {
    assert!(matches!(
        decode(&[0], 0..1, ArchiveVersion::V5, 1.0, "test:subd#0".into())
            .expect("required invariant"),
        DecodedSubd::Empty
    ));
    assert!(decode(&[2], 0..1, ArchiveVersion::V5, 1.0, "test:subd#0".into()).is_err());
}

#[test]
fn nested_crc_mismatch_warns_without_discarding_subd() {
    let fixture = Fixture::default();
    let mut bytes = payload(fixture);
    let crc = bytes.len() - 1;
    bytes[crc] ^= 1;
    let decoded = decode(
        &bytes,
        0..bytes.len(),
        fixture.archive,
        1.0,
        "test:subd#0".into(),
    )
    .expect("recoverable checksum mismatch");
    let DecodedSubd::Surface { warnings, .. } = decoded else {
        panic!("expected surface");
    };
    assert_eq!(warnings.len(), 1);
    assert!(warnings[0].contains("SubD anonymous CRC mismatch"));
}

#[test]
fn decodes_minor_suffix_gates_across_archive_bands() {
    for (minor, archive) in [
        (0, ArchiveVersion::V5),
        (1, ArchiveVersion::V6),
        (2, ArchiveVersion::V7),
        (3, ArchiveVersion::V7),
        (4, ArchiveVersion::V8),
    ] {
        let decoded = decode_fixture(
            Fixture {
                archive,
                minor,
                ..Fixture::default()
            },
            1.0,
        )
        .expect("required invariant");
        assert!(matches!(decoded, DecodedSubd::Surface { .. }));
    }
}

#[test]
fn decodes_valid_old_and_new_component_bases() {
    for archive in [
        ArchiveVersion::V5,
        ArchiveVersion::V6,
        ArchiveVersion::V7,
        ArchiveVersion::V8,
    ] {
        assert!(decode_fixture(
            Fixture {
                archive,
                ..Fixture::default()
            },
            1.0
        )
        .is_ok());
    }
}

#[test]
fn preserves_directed_reversed_face_edge_use() {
    let DecodedSubd::Surface { surface, .. } = decode_fixture(
        Fixture {
            reversed_edge: true,
            ..Fixture::default()
        },
        1.0,
    )
    .expect("required invariant") else {
        panic!("expected surface");
    };
    assert!(surface.faces[0].edges[1].reversed);
    assert_eq!(surface.edges[1].vertices, [2, 1]);
}

#[test]
fn rejects_open_or_repeated_face_rings() {
    assert!(decode_fixture(
        Fixture {
            open_ring: true,
            ..Fixture::default()
        },
        1.0
    )
    .is_err());
}

#[test]
fn rejects_pointer_type_null_and_reciprocity_errors() {
    for fixture in [
        Fixture {
            bad_pointer_type: true,
            ..Fixture::default()
        },
        Fixture {
            null_endpoint: true,
            ..Fixture::default()
        },
        Fixture {
            omit_vertex_edge: true,
            ..Fixture::default()
        },
    ] {
        assert!(decode_fixture(fixture, 1.0).is_err());
    }
}

#[test]
fn preserves_vertex_edge_tags_and_sector_coefficients() {
    let DecodedSubd::Surface { surface, .. } = decode_fixture(
        Fixture {
            vertex_tag: 4,
            edge_tag: 4,
            ..Fixture::default()
        },
        1.0,
    )
    .expect("required invariant") else {
        panic!("expected surface");
    };
    assert_eq!(surface.vertices[0].tag, SubdVertexTag::Dart);
    assert_eq!(surface.edges[0].tag, SubdEdgeTag::SmoothX);
    assert_eq!(surface.edges[0].sector_coefficients, [0.125, 0.875]);
}

#[test]
fn maps_scalar_and_preserves_v8_two_ended_sharpness() {
    let DecodedSubd::Surface { surface, .. } =
        decode_fixture(Fixture::default(), 1.0).expect("required invariant")
    else {
        panic!("expected old surface");
    };
    assert_eq!(surface.edges[0].sharpness, [0.25, 0.25]);
    let DecodedSubd::Surface { surface, .. } = decode_fixture(
        Fixture {
            archive: ArchiveVersion::V8,
            end_sharpness: 0.75,
            ..Fixture::default()
        },
        1.0,
    )
    .expect("required invariant") else {
        panic!("expected V8 surface");
    };
    assert_eq!(surface.edges[0].sharpness, [0.25, 0.75]);
}

#[test]
fn consumes_saved_limit_points_and_future_additions() {
    assert!(decode_fixture(
        Fixture {
            archive: ArchiveVersion::V7,
            saved_limit_points: true,
            future_additions: true,
            ..Fixture::default()
        },
        1.0
    )
    .is_ok());
}

#[test]
fn validates_higher_levels_and_render_mesh_chunks() {
    let decoded = decode_fixture(
        Fixture {
            archive: ArchiveVersion::V8,
            level_count: 2,
            render_mesh: true,
            ..Fixture::default()
        },
        1.0,
    )
    .expect("required invariant");
    let DecodedSubd::Surface {
        neutral_metadata, ..
    } = decoded
    else {
        panic!("expected surface");
    };
    assert!(neutral_metadata);
}

#[test]
fn scales_control_points_once_without_scaling_edge_metadata() {
    let DecodedSubd::Surface { surface, .. } =
        decode_fixture(Fixture::default(), 25.4).expect("required invariant")
    else {
        panic!("expected surface");
    };
    assert_eq!(surface.vertices[2].point, Point3::new(25.4, 25.4, 0.0));
    assert_eq!(surface.edges[0].sharpness, [0.25, 0.25]);
    assert_eq!(surface.edges[0].sector_coefficients, [0.125, 0.875]);
}

#[test]
fn rejects_noncontiguous_partitions_and_future_versions() {
    let mut bytes = payload(Fixture::default());
    let subd_chunk_header = 1 + 12;
    let subd_version_and_header = 8 + 4 + 12 + 48;
    let level_chunk_header = 12;
    let level_partition_offset =
        subd_chunk_header + subd_version_and_header + level_chunk_header + 8 + 2 + 3 + 48;
    bytes[level_partition_offset..level_partition_offset + 4].copy_from_slice(&2_u32.to_le_bytes());
    assert!(decode(
        &bytes,
        0..bytes.len(),
        ArchiveVersion::V5,
        1.0,
        "test:subd#0".into()
    )
    .is_err());

    let mut future = payload(Fixture::default());
    future[(1 + 12)..=16].copy_from_slice(&2_i32.to_le_bytes());
    assert!(matches!(
        decode(
            &future,
            0..future.len(),
            ArchiveVersion::V5,
            1.0,
            "test:subd#0".into()
        ),
        Err(SubdError::UnsupportedVersion { .. })
    ));
}

#[test]
fn subd_decode_commits_association_link_exactness_status_and_report() {
    let archive = ArchiveVersion::V5;
    let uuid = [
        0xd9, 0xa4, 0x9b, 0xf0, 0x5b, 0x45, 0xc3, 0x42, 0xba, 0x3b, 0xe6, 0xcc, 0xac, 0xef, 0x85,
        0x3b,
    ];
    let object = object_record_with_payload(
        archive,
        0x0004_0000,
        uuid,
        &crate::subd::tests::quad_payload(archive),
    );
    let bytes = minimal_document(
        "50",
        &[
            table(archive, 0x1000_0014, &[]),
            table(archive, 0x1000_0015, &[]),
            table(archive, 0x1000_0013, &[object]),
        ],
    );
    let mut scan = crate::container::scan_owned(bytes).expect("required invariant");
    set_test_units(&mut scan, 25.4);
    let result = crate::decode::decode_for_test(&scan);
    assert_eq!(result.ir().model.subds.len(), 1);
    let subd = &result.ir().model.subds[0];
    assert!(subd.source_object.is_some());
    assert_eq!(subd.vertices[2].point.x, 25.4);
    assert_eq!(
        result
            .source_fidelity()
            .annotations
            .exactness()
            .get(&subd.id.to_string())
            .map(|note| note.entity()),
        Some(cadmpeg_ir::Exactness::Derived)
    );
    assert_eq!(
        result
            .ir()
            .native_unknowns("rhino")
            .expect("required invariant")[0]
            .links,
        vec![subd.id.to_string()]
    );
    assert!(result.report().geometry_transferred());
    assert!(result.report().losses.iter().any(|loss| loss.code
        == crate::loss::RhinoLossCode::ObjectRecordCensus.kind()
        && loss.message.contains("decoded 1/1 Rhino object records")));
    assert!(cadmpeg_ir::validate_neutral(result.ir(), result.report().losses.clone()).is_ok());
}

#[test]
fn unknown_subd_symmetry_type_preserves_surface_and_native_source_bytes() {
    let archive = ArchiveVersion::V5;
    let uuid = [
        0xd9, 0xa4, 0x9b, 0xf0, 0x5b, 0x45, 0xc3, 0x42, 0xba, 0x3b, 0xe6, 0xcc, 0xac, 0xef, 0x85,
        0x3b,
    ];
    let payload = payload(Fixture {
        minor: 2,
        symmetry_type: 6,
        ..Fixture::default()
    });
    let object = object_record_with_payload(archive, 0x0004_0000, uuid, &payload);
    let bytes = minimal_document(
        "50",
        &[
            table(archive, 0x1000_0014, &[]),
            table(archive, 0x1000_0015, &[]),
            table(archive, 0x1000_0013, &[object]),
        ],
    );
    let mut scan = crate::container::scan_owned(bytes).expect("required invariant");
    set_test_units(&mut scan, 1.0);
    let result = crate::decode::decode_for_test(&scan);
    assert_eq!(result.ir().model.subds.len(), 1);
    assert!(result.report().losses.iter().any(|loss| {
        loss.code == crate::loss::RhinoLossCode::EnumerationValueDegraded.kind()
            && loss.message.contains("symmetry type 6")
    }));
    let retained = result
        .source_fidelity()
        .retained_records
        .iter()
        .find(|record| record.id().starts_with("rhino:object:record#"))
        .expect("SubD source record is retained");
    assert!(retained
        .data()
        .is_some_and(|data| data.windows(payload.len()).any(|window| window == payload)));
}

#[test]
fn malformed_subd_is_atomic_and_later_object_recovers() {
    let archive = ArchiveVersion::V5;
    let uuid = [
        0xd9, 0xa4, 0x9b, 0xf0, 0x5b, 0x45, 0xc3, 0x42, 0xba, 0x3b, 0xe6, 0xcc, 0xac, 0xef, 0x85,
        0x3b,
    ];
    let malformed = object_record_with_payload(archive, 0x0004_0000, uuid, &[2]);
    let empty = object_record_with_payload(archive, 0x0004_0000, uuid, &[0]);
    let bytes = minimal_document(
        "50",
        &[
            table(archive, 0x1000_0014, &[]),
            table(archive, 0x1000_0015, &[]),
            table(archive, 0x1000_0013, &[malformed, empty]),
        ],
    );
    let mut scan = crate::container::scan_owned(bytes).expect("required invariant");
    set_test_units(&mut scan, 1.0);
    let result = crate::decode::decode_for_test(&scan);
    assert!(result.ir().model.subds.is_empty());
    assert_eq!(
        result
            .ir()
            .native_unknowns("rhino")
            .expect("required invariant")
            .len(),
        2
    );
    assert!(result
        .report()
        .losses
        .iter()
        .any(|loss| loss.severity == Severity::Error));
    assert!(result.report().losses.iter().any(|loss| loss.code
        == crate::loss::RhinoLossCode::ObjectRecordCensus.kind()
        && loss.message.contains("decoded 1/2 Rhino object records")));
    assert!(cadmpeg_ir::validate_neutral(result.ir(), result.report().losses.clone()).is_ok());
}
