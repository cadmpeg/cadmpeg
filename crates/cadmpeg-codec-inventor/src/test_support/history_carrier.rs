// SPDX-License-Identifier: Apache-2.0
//! Byte builders of a synthetic ASM carrier with a `delta_state` chain.

use cadmpeg_asm::kernel_header::RefWidth;

pub(crate) const SPHERE_BODY_KEY: i64 = 9;
pub(crate) const CURRENT_BODY_KEY: i64 = 7;

pub(crate) fn state_evidence(
    model: &cadmpeg_ir::CadIr,
) -> Box<cadmpeg_asm::brep::transfer::InspectionGeometry> {
    let body_keys = model
        .native
        .namespace("inventor")
        .map_or_else(Vec::new, |namespace| {
            namespace
                .arena_as("body_native_keys")
                .expect("fixture keys decode")
        });
    Box::new(cadmpeg_asm::brep::transfer::InspectionGeometry {
        geometry: model.geometry.clone(),
        attributes: model.model.attributes.clone(),
        body_keys,
    })
}

pub(crate) fn put_ref(out: &mut Vec<u8>, width: RefWidth, tag: u8, value: i64) {
    out.push(tag);
    match width {
        RefWidth::Four => out.extend_from_slice(
            &i32::try_from(value)
                .expect("fixture value fits i32")
                .to_le_bytes(),
        ),
        RefWidth::Eight => out.extend_from_slice(&value.to_le_bytes()),
    }
}

pub(crate) fn put_name(out: &mut Vec<u8>, name: &str) {
    let parts: Vec<&str> = name.split('-').collect();
    for (index, part) in parts.iter().enumerate() {
        out.push(if index + 1 == parts.len() { 0x0d } else { 0x0e });
        out.push(u8::try_from(part.len()).expect("fixture name fits a byte"));
        out.extend_from_slice(part.as_bytes());
    }
}

/// A record whose payload is references and integers: `(tag, value)` pairs.
pub(crate) fn put_plain(out: &mut Vec<u8>, width: RefWidth, name: &str, fields: &[(u8, i64)]) {
    put_fields(out, width, name, fields);
    out.push(0x11);
}

/// The name and fields of a record, before its terminator.
pub(crate) fn put_fields(out: &mut Vec<u8>, width: RefWidth, name: &str, fields: &[(u8, i64)]) {
    put_name(out, name);
    for &(tag, value) in fields {
        put_ref(out, width, tag, value);
    }
}

pub(crate) const REF: u8 = 0x0c;
pub(crate) const INT: u8 = 0x04;

pub(crate) fn put_header(out: &mut Vec<u8>, width: RefWidth) {
    let (magic, len): (&[u8], usize) = match width {
        RefWidth::Four => (b"ASM BinaryFile4", 31),
        RefWidth::Eight => (b"ASM BinaryFile8", 47),
    };
    out.extend_from_slice(magic);
    out.extend_from_slice(&23_200_u32.to_le_bytes());
    out.resize(len, 0);
    for _ in 0..3 {
        out.extend_from_slice(&[0x07, 0]);
    }
    for _ in 0..3 {
        out.push(0x06);
        out.extend_from_slice(&1.0f64.to_le_bytes());
    }
}

/// The records of a sphere solid below a body, from slot `body + 1` on:
/// lump, shell, face, sphere surface. `body` is the slot of the body record.
pub(crate) fn put_sphere_solid(
    out: &mut Vec<u8>,
    width: RefWidth,
    body: i64,
    key: i64,
    refer: i64,
) {
    put_sphere_with_attribute(out, width, body, key, refer, -1);
}

fn put_sphere_with_attribute(
    out: &mut Vec<u8>,
    width: RefWidth,
    body: i64,
    key: i64,
    refer: i64,
    attribute: i64,
) {
    let (lump, shell, face, surface) = (body + 1, body + 2, body + 3, body + 4);
    put_plain(
        out,
        width,
        "body",
        &[
            (REF, attribute),
            (INT, key),
            (REF, refer),
            (REF, lump),
            (REF, -1),
            (REF, -1),
        ],
    );
    put_plain(
        out,
        width,
        "lump",
        &[
            (REF, -1),
            (INT, -1),
            (REF, -1),
            (REF, -1),
            (REF, shell),
            (REF, body),
        ],
    );
    put_plain(
        out,
        width,
        "shell",
        &[
            (REF, -1),
            (INT, -1),
            (REF, -1),
            (REF, -1),
            (REF, -1),
            (REF, face),
            (REF, -1),
            (REF, lump),
        ],
    );
    put_fields(
        out,
        width,
        "face",
        &[
            (REF, -1),
            (INT, -1),
            (REF, -1),
            (REF, -1),
            (REF, -1),
            (REF, shell),
            (REF, -1),
            (REF, surface),
        ],
    );
    out.extend_from_slice(&[0x0b, 0x0b, 0x11]);
    put_name(out, "sphere-surface");
    put_ref(out, width, REF, -1);
    put_ref(out, width, INT, -1);
    put_ref(out, width, REF, -1);
    out.push(0x13);
    for value in [0.0f64, 0.0, 0.0] {
        out.extend_from_slice(&value.to_le_bytes());
    }
    out.push(0x06);
    out.extend_from_slice(&2.5f64.to_le_bytes());
    for triple in [[1.0f64, 0.0, 0.0], [0.0, 0.0, 1.0]] {
        out.push(0x14);
        for value in triple {
            out.extend_from_slice(&value.to_le_bytes());
        }
    }
    out.extend_from_slice(&[0x0b; 5]);
    out.push(0x11);
}

/// A `delta_state` record with one board of `(old, new)` changes.
pub(crate) fn put_state(
    out: &mut Vec<u8>,
    width: RefWidth,
    ids: (i64, i64, i64, i64),
    changes: &[(i64, i64)],
) {
    let (id, previous, next, node) = ids;
    put_name(out, "delta_state");
    for value in [id, 1, 0] {
        put_ref(out, width, INT, value);
    }
    for value in [previous, next, node, -1, -1] {
        put_ref(out, width, REF, value);
    }
    out.push(0x0b);
    put_ref(out, width, INT, 1);
    put_ref(out, width, REF, node);
    put_ref(out, width, INT, 1);
    for &(old, new) in changes {
        put_ref(out, width, INT, 1);
        put_ref(out, width, REF, old);
        put_ref(out, width, REF, new);
    }
    put_ref(out, width, INT, 0);
    put_ref(out, width, INT, 0);
    out.push(0x11);
}

/// A carrier whose current body has key 7 and whose older state holds the same
/// sphere solid under key 9. `dangling` makes the archived body refer to a slot
/// that no state holds.
pub(crate) fn carrier_bytes(width: RefWidth, dangling: bool) -> Vec<u8> {
    carrier_bytes_with_name(width, dangling, None)
}

pub(crate) fn carrier_bytes_with_name(
    width: RefWidth,
    dangling: bool,
    name: Option<&str>,
) -> Vec<u8> {
    let mut out = Vec::new();
    put_header(&mut out, width);
    put_plain(&mut out, width, "asmheader", &[]);
    let attribute = if name.is_some() { 6 } else { -1 };
    put_sphere_with_attribute(&mut out, width, 1, CURRENT_BODY_KEY, -1, attribute);
    if let Some(name) = name {
        put_body_name(&mut out, width, name);
    }
    let last_active = if name.is_some() { 6 } else { 5 };
    put_state(&mut out, width, (13, -1, 1, 0), &[(last_active + 1, 1)]);
    let refer = if dangling { 99 } else { -1 };
    put_plain(
        &mut out,
        width,
        "body",
        &[
            (REF, attribute),
            (INT, SPHERE_BODY_KEY),
            (REF, refer),
            (REF, 2),
            (REF, -1),
            (REF, -1),
        ],
    );
    let inserts = (1..=last_active).map(|slot| (-1, slot)).collect::<Vec<_>>();
    put_state(&mut out, width, (12, 0, -1, 1), &inserts);
    out
}

fn put_body_name(out: &mut Vec<u8>, width: RefWidth, name: &str) {
    put_fields(out, width, "string_attrib-name_attrib-gen-attrib", &[]);
    out.extend_from_slice(b"\x07\x04name");
    out.push(0x08);
    out.extend_from_slice(
        &u16::try_from(name.len())
            .expect("fixture name length")
            .to_le_bytes(),
    );
    out.extend_from_slice(name.as_bytes());
    out.push(0x11);
}

/// Source index of the loft that the naming tags of [`lofted_carrier_bytes`]
/// name.
pub(crate) const LOFT_GENERATOR: i64 = 17;
/// Source index of a feature that only the head state of
/// [`lofted_carrier_bytes`] lists.
pub(crate) const LATER_GENERATOR: i64 = 31;
/// Source indices of the two section sketches of the loft.
pub(crate) const LOFT_SECTIONS: [u32; 2] = [19, 23];
/// Associative curve names of the section boundaries.
pub(crate) const LOFT_CURVES: [i64; 2] = [8, 3];

const LEGACY_CAP_CLASS: &str =
    "NMx_Generated_Brep_tag-NMx_Brep_Name_tag-NMx_Brep_tag-NamingMatching-attrib";
const LEGACY_SIDE_CLASS: &str =
    "NMx_Loft_Surface_Brep_tag-NMx_Brep_Name_tag-NMx_Brep_tag-NamingMatching-attrib";

/// A legacy naming attribute of `owner` that lists `source` and carries `tail`.
fn put_legacy_tag(
    out: &mut Vec<u8>,
    width: RefWidth,
    class: &str,
    owner: i64,
    source: i64,
    tail: &[i64],
) {
    let mut fields = vec![
        (REF, -1),
        (INT, -1),
        (REF, -1),
        (REF, -1),
        (REF, owner),
        (INT, 38),
        (INT, 1),
        (INT, source),
        (INT, -1),
    ];
    fields.extend(tail.iter().map(|value| (INT, *value)));
    put_plain(out, width, class, &fields);
}

/// A carrier whose current body is a solid of three planar faces with legacy
/// loft naming tags: two caps and one side face of the loft `LOFT_GENERATOR`
/// over `LOFT_SECTIONS`. The older state holds the faces and the tags under
/// body key 9, so it is the oldest state that lists the loft. The head state
/// replaces the body with key 7 and adds one more tag, which lists
/// `LATER_GENERATOR`.
///
/// Slots: 0 asmheader, 1 body, 2 lump, 3 shell, 4 to 6 faces, 7 plane, 8 to
/// 10 tags of the faces, 11 the tag of the head state and 12 the body name.
pub(crate) fn lofted_carrier_bytes(width: RefWidth, name: &str) -> Vec<u8> {
    let mut out = Vec::new();
    put_header(&mut out, width);
    put_plain(&mut out, width, "asmheader", &[]);
    let body = |key: i64| {
        [
            (REF, 12),
            (INT, key),
            (REF, -1),
            (REF, 2),
            (REF, -1),
            (REF, -1),
        ]
    };
    put_plain(&mut out, width, "body", &body(CURRENT_BODY_KEY));
    put_plain(
        &mut out,
        width,
        "lump",
        &[
            (REF, -1),
            (INT, -1),
            (REF, -1),
            (REF, -1),
            (REF, 3),
            (REF, 1),
        ],
    );
    put_plain(
        &mut out,
        width,
        "shell",
        &[
            (REF, -1),
            (INT, -1),
            (REF, -1),
            (REF, -1),
            (REF, -1),
            (REF, 4),
            (REF, -1),
            (REF, 2),
        ],
    );
    for face in 0..3_i64 {
        let next = if face < 2 { 5 + face } else { -1 };
        put_fields(
            &mut out,
            width,
            "face",
            &[
                (REF, 8 + face),
                (INT, -1),
                (REF, -1),
                (REF, next),
                (REF, -1),
                (REF, 3),
                (REF, -1),
                (REF, 7),
            ],
        );
        out.extend_from_slice(&[0x0b, 0x0b, 0x11]);
    }
    put_name(&mut out, "plane-surface");
    put_ref(&mut out, width, REF, -1);
    put_ref(&mut out, width, INT, -1);
    put_ref(&mut out, width, REF, -1);
    out.push(0x13);
    for value in [0.0f64, 0.0, 0.0] {
        out.extend_from_slice(&value.to_le_bytes());
    }
    for triple in [[0.0f64, 0.0, 1.0], [1.0, 0.0, 0.0]] {
        out.push(0x14);
        for value in triple {
            out.extend_from_slice(&value.to_le_bytes());
        }
    }
    out.extend_from_slice(&[0x0b; 5]);
    out.push(0x11);
    let [first, last] = LOFT_SECTIONS.map(i64::from);
    let [first_curve, last_curve] = LOFT_CURVES;
    put_legacy_tag(
        &mut out,
        width,
        LEGACY_CAP_CLASS,
        4,
        LOFT_GENERATOR,
        &[0, first, 260],
    );
    put_legacy_tag(
        &mut out,
        width,
        LEGACY_CAP_CLASS,
        5,
        LOFT_GENERATOR,
        &[0, last, 260],
    );
    put_legacy_tag(
        &mut out,
        width,
        LEGACY_SIDE_CLASS,
        6,
        LOFT_GENERATOR,
        &[
            2,
            first_curve,
            first,
            258,
            0,
            0,
            last_curve,
            last,
            258,
            0,
            0,
        ],
    );
    put_legacy_tag(
        &mut out,
        width,
        LEGACY_CAP_CLASS,
        4,
        LATER_GENERATOR,
        &[0, first, 260],
    );
    put_body_name(&mut out, width, name);
    put_state(&mut out, width, (13, -1, 1, 0), &[(13, 1), (-1, 11)]);
    put_plain(&mut out, width, "body", &body(SPHERE_BODY_KEY));
    let inserts = (1..=10)
        .chain([12])
        .map(|slot| (-1, slot))
        .collect::<Vec<_>>();
    put_state(&mut out, width, (12, 0, -1, 1), &inserts);
    out
}

/// Naming tag that lists one source index, in the class chain of the
/// schema-38, -40 and -42 tags with the stored schema `schema`.
pub(crate) fn put_naming_tag(out: &mut Vec<u8>, width: RefWidth, schema: i64, source: i64) {
    put_plain(
        out,
        width,
        "NMx_Generated_Brep_tag-NMx_Brep_Name_tag-NMx_Brep_tag-NamingMatching-attrib",
        &[
            (REF, -1),
            (INT, -1),
            (REF, -1),
            (REF, -1),
            (REF, -1),
            (INT, schema),
            (INT, 1),
            (INT, source),
            (INT, -1),
            (INT, 0),
        ],
    );
}

/// A carrier whose current model holds no body: the head state holds the
/// header and a naming tag that lists `source`; the state before it also
/// holds a sphere solid with body key `key`; the oldest state holds the
/// header only. The state that first lists `source` therefore introduces the
/// body `key`. `schema` is the stored schema of the tag.
pub(crate) fn introducing_carrier_bytes(
    width: RefWidth,
    schema: i64,
    source: i64,
    key: i64,
) -> Vec<u8> {
    // Active slots are the header and the tag; the archived solid takes
    // slots 2 to 6.
    let solid = 2;
    let mut out = Vec::new();
    put_header(&mut out, width);
    put_plain(&mut out, width, "asmheader", &[]);
    put_naming_tag(&mut out, width, schema, source);
    let deletes: Vec<(i64, i64)> = (solid..solid + 5).map(|slot| (slot, -1)).collect();
    put_state(&mut out, width, (13, -1, 1, 0), &deletes);
    put_sphere_solid(&mut out, width, solid, key, -1);
    let inserts: Vec<(i64, i64)> = std::iter::once(1)
        .chain(solid..solid + 5)
        .map(|slot| (-1, slot))
        .collect();
    put_state(&mut out, width, (12, 0, 2, 1), &inserts);
    put_state(&mut out, width, (11, 1, -1, 2), &[]);
    out
}

pub(crate) fn framed_carrier(bytes: &[u8]) -> Vec<u8> {
    let mut payload = Vec::new();
    payload.extend_from_slice(&1_u32.to_le_bytes());
    payload.extend_from_slice(&2_u16.to_le_bytes());
    payload.extend_from_slice(&3_u32.to_le_bytes());
    payload.extend_from_slice(&4_u32.to_le_bytes());
    payload.extend_from_slice(bytes);
    payload.extend_from_slice(&5_u32.to_le_bytes());
    payload.push(1);
    payload.extend_from_slice(&(-1_i32).to_le_bytes());
    payload.extend_from_slice(&6_u32.to_le_bytes());
    payload.extend_from_slice(&u32::MAX.to_le_bytes());
    payload
}
