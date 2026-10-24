// SPDX-License-Identifier: Apache-2.0
//! Synthetic `.pvz` packages for crate tests.
//!
//! The `PVS_*` and `OL_*` members carry a signature and arbitrary bytes: they
//! exercise detection, inspection, and classification. [`assembly`] builds a
//! complete package from the specification grammar.

pub(crate) mod ol;
pub(crate) mod pvs;

use cadmpeg_test_support::zip_fixture::{archive, crc32, RawEntry};

use crate::layout::token;
use pvs::{definition, occurrence, pool_atom, property_set, Node, Place};

/// A structure member with the `PV-PVS0301` signature.
pub(crate) const PVS_0301: &[u8] = b"PV-PVS0301\0synthetic structure body";
/// A structure member with the `PV-PVS0304` signature.
pub(crate) const PVS_0304: &[u8] = b"PV-PVS0304\0synthetic structure body";
/// A geometry member with the `DIV-OL08BL` signature.
pub(crate) const OL_08: &[u8] = b"DIV-OL08BL\0synthetic geometry body";
/// A geometry member with the `DIV-OL09BL` signature.
pub(crate) const OL_09: &[u8] = b"DIV-OL09BL\0synthetic geometry body";

/// A ZIP archive of stored members, in the given order.
pub(crate) fn package(members: &[(&str, &[u8])]) -> Vec<u8> {
    archive(
        &members
            .iter()
            .map(|(name, bytes)| RawEntry::stored(name, bytes))
            .collect::<Vec<_>>(),
    )
}

/// `plain` as one final stored DEFLATE block (RFC 1951 section 3.2.4).
pub(crate) fn deflate_stored_block(plain: &[u8]) -> Vec<u8> {
    let len = u16::try_from(plain.len()).expect("a stored block holds at most 65535 bytes");
    let mut block = vec![0x01];
    block.extend_from_slice(&len.to_le_bytes());
    block.extend_from_slice(&(!len).to_le_bytes());
    block.extend_from_slice(plain);
    block
}

/// `plain` as one GZIP member (RFC 1952) whose DEFLATE body is one stored
/// block, so the bytes depend on no compressor.
pub(crate) fn gzip(plain: &[u8]) -> Vec<u8> {
    let mut member = vec![0x1f, 0x8b, 0x08, 0x00, 0, 0, 0, 0, 0x00, 0xff];
    member.extend(deflate_stored_block(plain));
    member.extend_from_slice(&crc32(plain).to_le_bytes());
    member.extend_from_slice(&u32::try_from(plain.len()).unwrap().to_le_bytes());
    member
}

/// Bounds of the bracket geometry element.
const BRACKET_BOUNDS: [f32; 6] = [0.0, 0.0, 0.0, 0.01, 0.01, 0.01];
/// A 90-degree rotation about Z combined with the uniform scale 2: the child
/// X axis is parent Y, the child Y axis is parent −X (§3.10).
pub(crate) const SCALED_TURN: [f64; 9] = [0.0, 2.0, 0.0, -2.0, 0.0, 0.0, 0.0, 0.0, 2.0];

/// The structure member of [`assembly`]: `TOP.ASM` places `BRACKET.PRT`
/// twice and `PLATE.PRT` once; the member names come from the string pool.
pub(crate) fn assembly_structure(signature: &str) -> Vec<u8> {
    let pool = Node::leaf(0x01, pvs::string_pool_payload(&["bracket.ol", "plate.ol"])).encode();
    let plate_geometry = {
        let mut payload = pool_atom(1);
        payload.extend([0_u8; 24]);
        Node::typed(0x23, 0x01, payload)
    };
    let structure = Node::list(
        0x02,
        vec![
            definition(
                "TOP.ASM",
                vec![
                    occurrence(1, Place::Translation([0.1, 0.0, 0.0]), "41"),
                    occurrence(1, Place::Full([0.0, 0.2, 0.0], SCALED_TURN), "42"),
                    occurrence(2, Place::Identity, "43"),
                ],
            ),
            Node::parent(
                0x20,
                pvs::inline_atom("BRACKET.PRT"),
                vec![{
                    let mut payload = pool_atom(0);
                    payload.extend(BRACKET_BOUNDS.iter().flat_map(|value| value.to_le_bytes()));
                    Node::typed(0x23, 0x01, payload)
                }],
            ),
            definition("PLATE.PRT", vec![plate_geometry]),
        ],
    )
    .encode();
    let properties = Node::list(
        0x03,
        vec![
            property_set(1, &[("PART_NUMBER", "TOP-001")]),
            property_set(2, &[("MATERIAL", "STEEL")]),
        ],
    )
    .encode();
    pvs::member(
        signature,
        &[(1, "", pool), (2, "", structure), (3, "", properties)],
    )
}

/// The `bracket.ol` member of [`assembly`]: a red tetrahedron with edge 0.01
/// along each axis.
pub(crate) fn bracket_member() -> Vec<u8> {
    let mut attributes = ol::stream_index(token::STREAM_ATTRIBUTES);
    attributes.extend(ol::appearance_values([
        1.0, 0.0, 0.0, 0.8, 0.0, 0.0, 0.5, 0.5, 0.5, 32.0, 0.0, 0.0, 0.0, 1.0,
    ]));
    attributes.extend(ol::appearance_reference("red", 7));
    attributes.extend(ol::tlv(3, b"!pvFId"));
    let mut surface = ol::stream_index(token::STREAM_SURFACE_TESSELLATION);
    surface.extend(ol::patch_header(0, 0));
    surface.extend(ol::vertex_block(
        &[
            [0.0, 0.0, 0.0],
            [0.01, 0.0, 0.0],
            [0.0, 0.01, 0.0],
            [0.0, 0.0, 0.01],
        ],
        None,
    ));
    surface.extend(ol::primitive(0x1b, &[0, 2, 1, 0, 1, 3, 1, 2, 3, 2, 0, 3]));
    surface.extend(ol::tlv(0, &[]));
    ol::member(
        "DIV-OL09BL",
        true,
        &[
            (token::STREAM_ATTRIBUTES, attributes),
            (token::STREAM_SURFACE_TESSELLATION, surface),
        ],
    )
}

/// The `plate.ol` member of [`assembly`]: an unpainted unit-normal square as
/// one triangle strip, its edge tessellation, and an exact stream.
pub(crate) fn plate_member() -> Vec<u8> {
    let attributes = ol::stream_index(token::STREAM_ATTRIBUTES);
    let mut surface = ol::stream_index(token::STREAM_SURFACE_TESSELLATION);
    surface.extend(ol::patch_header(0xffff_ffff, 0));
    surface.extend(ol::vertex_block(
        &[
            [0.0, 0.0, 0.0],
            [0.02, 0.0, 0.0],
            [0.0, 0.02, 0.0],
            [0.02, 0.02, 0.0],
        ],
        Some(&[[0.0, 0.0, 1.0]; 4]),
    ));
    surface.extend(ol::primitive(0x1a, &[0, 1, 2, 3]));
    let mut edges = ol::stream_index(token::STREAM_EDGE_TESSELLATION);
    edges.extend(ol::primitive(token::EDGE_TESS_INDEX_TABLE, &[1, 0]));
    edges.extend(ol::edge_header(0));
    edges.extend(ol::vertex_block(&[[0.0, 0.0, 0.0], [0.02, 0.0, 0.0]], None));
    edges.extend(ol::primitive(0x1c, &[0, 1]));
    let exact = ol::stream_index(token::STREAM_EXACT_F64);
    ol::member(
        "DIV-OL08BL",
        false,
        &[
            (token::STREAM_ATTRIBUTES, attributes),
            (token::STREAM_SURFACE_TESSELLATION, surface),
            (token::STREAM_EDGE_TESSELLATION, edges),
            (token::STREAM_EXACT_F64, exact),
        ],
    )
}

/// A complete package: the [`assembly_structure`] member with `signature`,
/// [`bracket_member`], and [`plate_member`].
pub(crate) fn assembly(signature: &str) -> Vec<u8> {
    package(&[
        ("model.pvs", &assembly_structure(signature)),
        ("bracket.ol", &bracket_member()),
        ("plate.ol", &plate_member()),
    ])
}

/// A geometry member holding the closed cube of [`exact::cube_stream`]:
/// its exact stream, with `f64` scalars when `double` is set, and a red
/// surface tessellation with one patch per exact face.
pub(crate) fn cube_member(double: bool) -> Vec<u8> {
    exact_member(exact::cube_stream(double, [false; 6], [false; 6]))
}

/// A geometry member holding [`exact::cube_and_sheet_stream`] and the red
/// cube tessellation of [`cube_member`].
pub(crate) fn cube_and_sheet_member() -> Vec<u8> {
    exact_member(exact::cube_and_sheet_stream(true))
}

/// A geometry member holding the exact stream `stream` and a red surface
/// tessellation with one patch per cube face.
fn exact_member(stream: exact::Stream) -> Vec<u8> {
    let mut attributes = ol::stream_index(token::STREAM_ATTRIBUTES);
    attributes.extend(ol::appearance_values([
        1.0, 0.0, 0.0, 0.8, 0.0, 0.0, 0.5, 0.5, 0.5, 32.0, 0.0, 0.0, 0.0, 1.0,
    ]));
    attributes.extend(ol::appearance_reference("red", 7));
    let size = 0.01_f32;
    let corners: Vec<[f32; 3]> = (0..8_usize)
        .map(|index| [0, 1, 2].map(|bit| if index >> bit & 1 == 1 { size } else { 0.0 }))
        .collect();
    let mut surface = ol::stream_index(token::STREAM_SURFACE_TESSELLATION);
    for (face, ring) in exact::CUBE_FACES.iter().enumerate() {
        surface.extend(ol::patch_header(0, u32::try_from(face).unwrap()));
        surface.extend(ol::vertex_block(&ring.map(|corner| corners[corner]), None));
        surface.extend(ol::primitive(0x1b, &[0, 1, 2, 0, 2, 3]));
    }
    let id = stream.id();
    ol::member(
        "DIV-OL09BL",
        false,
        &[
            (token::STREAM_ATTRIBUTES, attributes),
            (token::STREAM_SURFACE_TESSELLATION, surface),
            (id, stream.finish()),
        ],
    )
}

/// A package whose `TOP.ASM` places `CUBE64.PRT` and `CUBE32.PRT`, the
/// [`cube_member`] with `f64` and with `f32` exact scalars, and `PLATE.PRT`,
/// the [`plate_member`] whose exact stream does not decode.
pub(crate) fn exact_assembly() -> Vec<u8> {
    let pool = Node::leaf(
        0x01,
        pvs::string_pool_payload(&["cube64.ol", "cube32.ol", "plate.ol"]),
    )
    .encode();
    let part = |name: &str, member: u64| {
        let mut payload = pool_atom(member);
        payload.extend([0_u8; 24]);
        definition(name, vec![Node::typed(0x23, 0x01, payload)])
    };
    let structure = Node::list(
        0x02,
        vec![
            definition(
                "TOP.ASM",
                vec![
                    occurrence(1, Place::Identity, "41"),
                    occurrence(2, Place::Translation([0.02, 0.0, 0.0]), "42"),
                    occurrence(3, Place::Translation([0.0, 0.02, 0.0]), "43"),
                ],
            ),
            part("CUBE64.PRT", 0),
            part("CUBE32.PRT", 1),
            part("PLATE.PRT", 2),
        ],
    )
    .encode();
    let structure = pvs::member("PV-PVS0304", &[(1, "", pool), (2, "", structure)]);
    package(&[
        ("model.pvs", &structure),
        ("cube64.ol", &cube_member(true)),
        ("cube32.ol", &cube_member(false)),
        ("plate.ol", &plate_member()),
    ])
}

/// A package whose `TOP.ASM` places `PART.PRT`, the
/// [`cube_and_sheet_member`] `part.ol`.
pub(crate) fn cube_and_sheet_assembly() -> Vec<u8> {
    part_package(&cube_and_sheet_member())
}

/// A package whose `TOP.ASM` places `PART.PRT` once with the identity
/// placement; `PART.PRT` names the geometry member `part.ol`, `member`.
pub(crate) fn part_package(member: &[u8]) -> Vec<u8> {
    let pool = Node::leaf(0x01, pvs::string_pool_payload(&["part.ol"])).encode();
    let mut payload = pool_atom(0);
    payload.extend([0_u8; 24]);
    let structure = Node::list(
        0x02,
        vec![
            definition("TOP.ASM", vec![occurrence(1, Place::Identity, "41")]),
            definition("PART.PRT", vec![Node::typed(0x23, 0x01, payload)]),
        ],
    )
    .encode();
    let structure = pvs::member("PV-PVS0304", &[(1, "", pool), (2, "", structure)]);
    package(&[("model.pvs", &structure), ("part.ol", member)])
}

/// A geometry member holding one red appearance and the exact stream
/// `stream`, with no tessellation.
pub(crate) fn solid_member(stream: exact::Stream) -> Vec<u8> {
    let mut attributes = ol::stream_index(token::STREAM_ATTRIBUTES);
    attributes.extend(ol::appearance_values([
        1.0, 0.0, 0.0, 0.8, 0.0, 0.0, 0.5, 0.5, 0.5, 32.0, 0.0, 0.0, 0.0, 1.0,
    ]));
    attributes.extend(ol::appearance_reference("red", 7));
    let id = stream.id();
    ol::member(
        "DIV-OL09BL",
        false,
        &[
            (token::STREAM_ATTRIBUTES, attributes),
            (id, stream.finish()),
        ],
    )
}

/// The [`part_package`] of the [`solid_member`] of `stream`.
pub(crate) fn solid_package(stream: exact::Stream) -> Vec<u8> {
    part_package(&solid_member(stream))
}

/// The [`solid_package`] of the cube of [`solids::cube`] whose face `face`
/// has the carrier `carrier` and whose other faces are planes.
pub(crate) fn cube_package(face: usize, carrier: solids::PlanarCarrier) -> Vec<u8> {
    solid_package(solids::polyhedron_stream(&solids::cube(|index| {
        if index == face {
            carrier
        } else {
            solids::PlanarCarrier::Plane
        }
    })))
}

pub(crate) mod exact;
pub(crate) mod solids;
