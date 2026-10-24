// SPDX-License-Identifier: Apache-2.0

use cadmpeg_core::decode::View;
use cadmpeg_core::CodecError;

use super::{
    decode, exact_stream, generation, Container, Generation, MemberGeometry, PackageResidue,
};
use crate::layout::token::{
    STREAM_ATTRIBUTES, STREAM_EDGE_TESSELLATION, STREAM_EXACT_F64, STREAM_SURFACE_TESSELLATION,
};
use crate::loss::PvzLossCode;
use crate::test_support::ol::{
    appearance_reference, appearance_values, edge_header, member, patch_header, primitive,
    stream_index, tlv, vertex_block,
};
use crate::test_support::{bracket_member, plate_member};

const RED: [f32; 14] = [
    1.0, 0.0, 0.0, 0.8, 0.0, 0.0, 0.5, 0.5, 0.5, 32.0, 0.0, 0.0, 0.0, 1.0,
];
const SQUARE: [[f32; 3]; 4] = [
    [0.0, 0.0, 0.0],
    [1.0, 0.0, 0.0],
    [0.0, 1.0, 0.0],
    [1.0, 1.0, 0.0],
];

fn geometry(bytes: &[u8]) -> Result<MemberGeometry, CodecError> {
    let ctx = cadmpeg_test_support::service_decode_context();
    decode(&ctx, View::over_retained(bytes))
}

fn losses(geometry: &MemberGeometry) -> Vec<cadmpeg_ir::report::loss::LossKind> {
    let mut residue = PackageResidue::default();
    residue.absorb("member.ol", &geometry.residue);
    residue.losses().into_iter().map(|loss| loss.code).collect()
}

/// A member whose surface stream holds `records` after its stream index.
fn surface_member(attributes: &[Vec<u8>], records: &[Vec<u8>]) -> Vec<u8> {
    let mut attribute_stream = stream_index(STREAM_ATTRIBUTES);
    attribute_stream.extend(attributes.concat());
    let mut surface = stream_index(STREAM_SURFACE_TESSELLATION);
    surface.extend(records.concat());
    member(
        "DIV-OL08BL",
        false,
        &[
            (STREAM_ATTRIBUTES, attribute_stream),
            (STREAM_SURFACE_TESSELLATION, surface),
        ],
    )
}

#[test]
fn each_signature_selects_its_generation() {
    for expected in Generation::ALL {
        let mut member = expected.signature().as_bytes().to_vec();
        member.extend_from_slice(b"\0\x02 body");
        assert_eq!(generation(View::over_retained(&member)), Some(expected));
    }
    for member in [
        b"DIV-OL07BL body".as_slice(),
        b"DIV-OL09B",
        b"PV-PVS0304",
        b"",
    ] {
        assert_eq!(generation(View::over_retained(member)), None, "{member:?}");
    }
}

#[test]
fn the_directory_partitions_the_member_with_or_without_the_preamble() {
    let ctx = cadmpeg_test_support::service_decode_context();
    for preamble in [false, true] {
        let bytes = member(
            "DIV-OL09BL",
            preamble,
            &[
                (STREAM_ATTRIBUTES, stream_index(STREAM_ATTRIBUTES)),
                (STREAM_EXACT_F64, stream_index(STREAM_EXACT_F64)),
            ],
        );
        let container = Container::open(&ctx, View::over_retained(&bytes)).unwrap();
        assert_eq!(container.preamble, preamble);
        let ids: Vec<_> = container.streams.iter().map(|(id, _)| *id).collect();
        assert_eq!(ids, [STREAM_ATTRIBUTES, STREAM_EXACT_F64]);
        let last = container.streams.last().unwrap().1;
        assert_eq!(last.end(), bytes.len());
    }
}

#[test]
fn a_directory_that_does_not_tile_the_member_is_refused() {
    let ctx = cadmpeg_test_support::service_decode_context();
    let good = member(
        "DIV-OL08BL",
        true,
        &[(STREAM_ATTRIBUTES, stream_index(STREAM_ATTRIBUTES))],
    );
    let mut version = good.clone();
    version[10] = 2;
    let mut length = good.clone();
    length[14] += 1;
    let mut sentinel = good.clone();
    sentinel[30] = 0xfe;
    let mut preamble_tag = good.clone();
    preamble_tag[38] ^= 1;
    let mut trailing = good.clone();
    trailing.push(0);
    let out_of_order = member(
        "DIV-OL08BL",
        false,
        &[
            (
                STREAM_SURFACE_TESSELLATION,
                stream_index(STREAM_SURFACE_TESSELLATION),
            ),
            (STREAM_ATTRIBUTES, stream_index(STREAM_ATTRIBUTES)),
        ],
    );
    for bad in [
        version,
        length,
        sentinel,
        preamble_tag,
        trailing,
        out_of_order,
    ] {
        assert!(Container::open(&ctx, View::over_retained(&bad)).is_err());
    }
    assert!(Container::open(&ctx, View::over_retained(b"DIV-OL08BL")).is_err());
    assert!(Container::open(&ctx, View::over_retained(b"DIV-OL07BL and more bytes here")).is_err());
}

#[test]
fn the_exact_stream_is_expanded_on_request() {
    let ctx = cadmpeg_test_support::service_decode_context();
    let plate = plate_member();
    let exact = exact_stream(&ctx, View::over_retained(&plate))
        .unwrap()
        .unwrap();
    assert_eq!(exact.stream_id, STREAM_EXACT_F64);
    assert_eq!(exact.payload.window(), stream_index(STREAM_EXACT_F64));

    let bracket = bracket_member();
    assert!(exact_stream(&ctx, View::over_retained(&bracket))
        .unwrap()
        .is_none());

    let two = member(
        "DIV-OL08BL",
        false,
        &[
            (0x0100_0003, stream_index(0x0100_0003)),
            (STREAM_EXACT_F64, stream_index(STREAM_EXACT_F64)),
        ],
    );
    assert!(exact_stream(&ctx, View::over_retained(&two)).is_err());
}

#[test]
fn a_patch_takes_the_appearance_its_table_entry_defines() {
    let bytes = surface_member(
        &[
            appearance_values(RED),
            tlv(6, b"\x0a\0\0\0plain.png\0\0\0\0\0\0\0\0\x01\x01"),
            appearance_reference("red", 7),
            appearance_reference("red again", 7),
            appearance_reference("undefined", 9),
        ],
        &[
            patch_header(1, 0),
            vertex_block(&SQUARE, None),
            primitive(0x1b, &[0, 1, 2]),
            patch_header(2, 1),
            vertex_block(&SQUARE, None),
            primitive(0x1b, &[1, 3, 2]),
        ],
    );
    let geometry = geometry(&bytes).unwrap();
    let defined = &geometry.appearances.defined[&7];
    assert_eq!(defined.name, "red");
    assert_eq!(defined.values.base, [1.0, 0.0, 0.0]);
    assert_eq!(defined.values.shininess, 32.0);
    assert_eq!(defined.values.opacity, 1.0);
    let meshes: Vec<_> = geometry
        .meshes
        .iter()
        .map(|mesh| (mesh.appearance, mesh.triangles.clone()))
        .collect();
    assert_eq!(
        meshes,
        [(Some(7), vec![[0, 1, 2]]), (None, vec![[1, 3, 2]])]
    );
    assert!(losses(&geometry).contains(&PvzLossCode::AttributeAppearanceUnresolved.kind()));
}

#[test]
fn fans_strips_and_lists_expand_to_triangles() {
    let bytes = surface_member(
        &[],
        &[
            patch_header(0xffff_ffff, 0),
            vertex_block(&SQUARE, None),
            primitive(0x19, &[0, 1, 3, 2]),
            primitive(0x1a, &[0, 1, 2, 3]),
            primitive(0x1b, &[3, 2, 1]),
            tlv(0, &[]),
        ],
    );
    let geometry = geometry(&bytes).unwrap();
    assert_eq!(geometry.meshes.len(), 1);
    let mesh = &geometry.meshes[0];
    assert_eq!(
        mesh.triangles,
        [[0, 1, 3], [0, 3, 2], [0, 1, 2], [2, 1, 3], [3, 2, 1]]
    );
    assert_eq!(mesh.patches, [(0, vec![0, 1, 2, 3, 4])]);
    assert_eq!(mesh.positions, SQUARE);
    assert!(mesh.normals.is_none());
}

#[test]
fn layouts_with_normals_form_their_own_mesh_and_texture_coordinates_are_counted() {
    let mut textured = vec![5_u8];
    textured.extend(1_u32.to_le_bytes());
    for value in [0.5_f32, 0.5, 0.5, 0.0, 0.0, 1.0, 0.25, 0.75] {
        textured.extend(value.to_le_bytes());
    }
    let bytes = surface_member(
        &[],
        &[
            patch_header(0xffff_ffff, 0),
            vertex_block(&SQUARE, Some(&[[0.0, 0.0, 1.0]; 4])),
            primitive(0x1b, &[0, 1, 2]),
            vertex_block(&SQUARE[..3], None),
            primitive(0x1b, &[0, 1, 2]),
            patch_header(0xffff_ffff, 1),
            tlv(0x0100_0016, &textured),
            primitive(0x1b, &[0, 0, 0]),
            tlv(0x0100_000b, &[]),
        ],
    );
    let geometry = geometry(&bytes).unwrap();
    let shapes: Vec<_> = geometry
        .meshes
        .iter()
        .map(|mesh| {
            (
                mesh.positions.len(),
                mesh.normals.as_ref().map(Vec::len),
                mesh.patches.clone(),
            )
        })
        .collect();
    assert_eq!(
        shapes,
        [
            (5, Some(5), vec![(0, vec![0]), (1, vec![1])]),
            (3, None, vec![(0, vec![0])]),
        ]
    );
    assert_eq!(geometry.meshes[0].triangles[1], [4, 4, 4]);
    assert_eq!(geometry.residue.texture_vertices, 1);
    assert_eq!(
        losses(&geometry),
        [
            PvzLossCode::TessellationPatchFields.kind(),
            PvzLossCode::TessellationRecordsUndecoded.kind(),
            PvzLossCode::TessellationTextureCoordinates.kind(),
        ]
    );
}

#[test]
fn a_malformed_surface_stream_is_reported_and_the_member_survives() {
    for records in [
        vec![vertex_block(&SQUARE, None)],
        vec![patch_header(0xffff_ffff, 0), primitive(0x1b, &[0, 1, 2])],
        vec![
            patch_header(0xffff_ffff, 0),
            vertex_block(&SQUARE, None),
            primitive(0x1b, &[0, 1, 4]),
        ],
        vec![
            patch_header(0xffff_ffff, 0),
            vertex_block(&SQUARE, None),
            primitive(0x1b, &[0, 1]),
        ],
        vec![
            patch_header(0xffff_ffff, 0),
            tlv(0x0100_0016, &[2, 0, 0, 0, 0]),
        ],
        vec![tlv(0x0100_000d, &[0; 12])],
    ] {
        let geometry = geometry(&surface_member(&[], &records)).unwrap();
        assert!(geometry.meshes.is_empty());
        assert_eq!(
            losses(&geometry),
            [PvzLossCode::GeometryStreamUndecoded.kind()]
        );
    }
}

#[test]
fn edge_groups_are_decoded_and_reported() {
    let plate = geometry(&plate_member()).unwrap();
    assert!(plate.has_exact);
    assert_eq!(plate.residue.edge_index_tables, 1);
    assert!(losses(&plate).contains(&PvzLossCode::TessellationEdgesNotTransferred.kind()));

    let mut edges = stream_index(STREAM_EDGE_TESSELLATION);
    edges.extend(edge_header(3));
    edges.extend(vertex_block(&SQUARE[..2], None));
    edges.extend(primitive(0x1d, &[0, 1, 1]));
    let odd = member(
        "DIV-OL08BL",
        false,
        &[
            (STREAM_ATTRIBUTES, stream_index(STREAM_ATTRIBUTES)),
            (STREAM_EDGE_TESSELLATION, edges),
        ],
    );
    assert_eq!(
        losses(&geometry(&odd).unwrap()),
        [PvzLossCode::GeometryStreamUndecoded.kind()]
    );
}

#[test]
fn unknown_records_and_streams_are_counted() {
    let geometry = geometry(&bracket_member()).unwrap();
    assert_eq!(geometry.meshes.len(), 1);
    assert_eq!(
        losses(&geometry),
        [
            PvzLossCode::AttributeRecordsUndecoded.kind(),
            PvzLossCode::AttributeAppearanceTail.kind(),
            PvzLossCode::TessellationPatchFields.kind(),
            PvzLossCode::GeometryPreambleUndecoded.kind(),
        ]
    );
    let other = member(
        "DIV-OL08BL",
        false,
        &[
            (STREAM_ATTRIBUTES, stream_index(STREAM_ATTRIBUTES)),
            (0x0100_0009, vec![1]),
        ],
    );
    assert_eq!(
        losses(&self::geometry(&other).unwrap()),
        [PvzLossCode::GeometryStreamUndecoded.kind()]
    );
}

#[test]
fn a_line_group_in_the_surface_stream_is_counted_not_triangulated() {
    let bytes = surface_member(
        &[],
        &[
            edge_header(0),
            vertex_block(&SQUARE[..2], None),
            primitive(0x1d, &[0, 1]),
            patch_header(0xffff_ffff, 0),
            vertex_block(&SQUARE, None),
            primitive(0x1b, &[0, 1, 2]),
        ],
    );
    let geometry = geometry(&bytes).unwrap();
    assert_eq!(geometry.meshes.len(), 1);
    assert_eq!(geometry.meshes[0].triangles, [[0, 1, 2]]);
    assert_eq!(
        geometry.residue.surface_records.0.values().sum::<usize>(),
        3
    );
    assert_eq!(
        losses(&geometry),
        [
            PvzLossCode::TessellationPatchFields.kind(),
            PvzLossCode::TessellationRecordsUndecoded.kind(),
        ]
    );
}
