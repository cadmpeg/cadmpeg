// SPDX-License-Identifier: Apache-2.0
//! Synthetic pattern and mirror documents (`docs/formats/solidedge.md` §8).

use super::{
    cluster_and_map, document, navigator_body, part_body, part_body_with_profile, profile_group,
    put_reals, put_words, type_table, Member2d, Object, ProfileFrame, DRESSUP_CLASSES,
};

/// The class names of a pattern document, by form.
const PATTERN_CLASSES: [&str; 21] = [
    DRESSUP_CLASSES[0],
    DRESSUP_CLASSES[1],
    DRESSUP_CLASSES[2],
    DRESSUP_CLASSES[3],
    DRESSUP_CLASSES[4],
    DRESSUP_CLASSES[5],
    DRESSUP_CLASSES[6],
    DRESSUP_CLASSES[7],
    DRESSUP_CLASSES[8],
    DRESSUP_CLASSES[9],
    DRESSUP_CLASSES[10],
    DRESSUP_CLASSES[11],
    DRESSUP_CLASSES[12],
    DRESSUP_CLASSES[13],
    DRESSUP_CLASSES[14],
    DRESSUP_CLASSES[15],
    DRESSUP_CLASSES[16],
    DRESSUP_CLASSES[17],
    DRESSUP_CLASSES[18],
    DRESSUP_CLASSES[19],
    "CLSID_JPattern2d",
];

/// The GUID that precedes the pattern parameters, in stored byte order.
const PATTERN_GUID: [u8; 16] = [
    0xc0, 0xee, 0x45, 0x01, 0x02, 0x16, 0xce, 0x11, 0xb7, 0xee, 0x08, 0x00, 0x36, 0x01, 0xe5, 0x3b,
];

/// The seed feature, the pattern group and the fast-pattern part.
const SEED: u32 = 10;
const GROUP: u32 = 11;
const FAST_PART: u32 = 12;
/// The profile group of the fast-pattern part, and its first member.
const PATTERN_SET: u32 = 100;
const FIRST_MEMBER: u32 = 200;
/// The mirror plane object.
const PLANE: u32 = 20;
/// The form of `CLSID_JPattern2d` in [`PATTERN_CLASSES`].
const PATTERN_2D_FORM: u16 = 20;
/// Bytes of a group object, the frame included.
const GROUP_LEN: usize = 341;

/// A pattern group. The seed feature is object 10 and the group is object 11.
pub(crate) struct PatternSpec<'a> {
    pub(crate) frame: &'a ProfileFrame,
    /// The seed identifier the group stores.
    pub(crate) seed: u32,
    /// The pattern kind word.
    pub(crate) kind: u32,
    pub(crate) counts: [u32; 2],
    pub(crate) steps: [f64; 2],
    /// The curves the pattern references, in order, after the pattern member.
    pub(crate) geometry: &'a [Member2d],
    /// Whether the parameter GUID is present.
    pub(crate) guid: bool,
}

/// The bytes after the frame of a `CLSID_JPattern2d` object that references
/// `references` objects by triples.
fn pattern_2d_body(spec: &PatternSpec<'_>) -> Vec<u8> {
    let mut object = vec![0_u8; 8 + 20];
    for index in 0..spec.geometry.len() {
        let id = FIRST_MEMBER + 1 + u32::try_from(index).unwrap();
        for word in [0x10_u32, 0x8000_0000, id] {
            object.extend_from_slice(&word.to_le_bytes());
        }
    }
    if spec.guid {
        object.extend_from_slice(&PATTERN_GUID);
    }
    let after = object.len();
    object.resize(after + 50, 0);
    object[after + 14..after + 18].copy_from_slice(&spec.kind.to_le_bytes());
    object[after + 26..after + 30].copy_from_slice(&spec.counts[0].to_le_bytes());
    object[after + 30..after + 34].copy_from_slice(&spec.counts[1].to_le_bytes());
    object[after + 34..after + 42].copy_from_slice(&spec.steps[0].to_le_bytes());
    object[after + 42..after + 50].copy_from_slice(&spec.steps[1].to_le_bytes());
    object.split_off(8)
}

fn group_body() -> Vec<u8> {
    vec![0_u8; GROUP_LEN - 8]
}

/// The document of `objects` plus the seed feature, the group `group` and the
/// base planes, whose navigator lists the seed and the group.
fn assemble(group: Vec<u8>, mut extra: Vec<Object>) -> Vec<u8> {
    let mut objects = Vec::new();
    for id in 1..=3 {
        objects.push(Object {
            id,
            form: 0,
            flags: 0,
            body: vec![0; 8],
        });
    }
    objects.push(Object {
        id: SEED,
        form: 1,
        flags: 0,
        body: part_body("JMProtrusionFunction"),
    });
    objects.push(Object {
        id: GROUP,
        form: 2,
        flags: 0,
        body: group,
    });
    objects.push(Object {
        id: 7,
        form: 3,
        flags: 0,
        body: vec![0; 8],
    });
    objects.push(Object {
        id: 5000,
        form: 4,
        flags: 0,
        body: navigator_body([1, 2, 3], &[SEED, GROUP]),
    });
    objects.append(&mut extra);
    objects.sort_by_key(|object| object.id);
    let (cluster, map) = cluster_and_map(&objects);
    document(&[
        ("PSMcluster0", cluster),
        ("PSMspacemap/0x00000000", map),
        ("PSMtypetable", type_table(&PATTERN_CLASSES, &[])),
    ])
}

/// The profile objects of a pattern: the profile group of the pattern part,
/// whose first member is the `CLSID_JPattern2d` object [`FIRST_MEMBER`].
pub(super) fn pattern_profile_objects(spec: &PatternSpec<'_>) -> Vec<Object> {
    let mut members = vec![Member2d::Constraint];
    members.extend(spec.geometry.iter().map(|member| match member {
        Member2d::Line(values) => Member2d::Line(*values),
        Member2d::Arc(values) => Member2d::Arc(*values),
        _ => panic!("a pattern references lines and arcs"),
    }));
    let mut extra = profile_group(PATTERN_SET, spec.frame, 1, FIRST_MEMBER, &members);
    // The first member stands for the pattern object.
    for object in &mut extra {
        if object.id == FIRST_MEMBER {
            object.form = PATTERN_2D_FORM;
            object.body = pattern_2d_body(spec);
        }
    }
    extra
}

/// The class names of a pattern document, by form, and the identifier of the
/// pattern object.
pub(super) const fn pattern_class_names() -> [&'static str; 21] {
    PATTERN_CLASSES
}

/// The identifier of the `CLSID_JPattern2d` object of [`pattern_profile_objects`].
pub(super) const PATTERN_2D_OBJECT: u32 = FIRST_MEMBER;

/// A document with a pattern group over the seed feature.
pub(crate) fn pattern_document(spec: &PatternSpec<'_>) -> Vec<u8> {
    let mut group = group_body();
    put_words(&mut group, 106, &[FAST_PART]);
    put_words(&mut group, 337, &[spec.seed]);
    let mut extra = pattern_profile_objects(spec);
    extra.push(Object {
        id: FAST_PART,
        form: 1,
        flags: 0,
        body: part_body_with_profile("JMFastPatternFunction", 68, PATTERN_SET + 1),
    });
    assemble(group, extra)
}

/// A mirror group.
pub(crate) struct MirrorSpec {
    /// The seed count the group stores.
    pub(crate) count: u32,
    /// The seed identifier stored at offset 96.
    pub(crate) seed: u32,
    /// The point and normal of the plane object, in metres, or `None` for a
    /// group that names no plane.
    pub(crate) plane: Option<([f64; 3], [f64; 3])>,
}

/// A document with a mirror group over the seed feature.
pub(crate) fn mirror_document(spec: &MirrorSpec) -> Vec<u8> {
    let mut group = group_body();
    put_words(&mut group, 92, &[spec.count, spec.seed]);
    let mut extra = Vec::new();
    if let Some((point, normal)) = spec.plane {
        put_words(&mut group, 329, &[PLANE]);
        let mut body = vec![0_u8; 61 - 8];
        put_reals(&mut body, 13, &point);
        put_reals(&mut body, 37, &normal);
        extra.push(Object {
            id: PLANE,
            form: 0,
            flags: 0,
            body,
        });
    }
    assemble(group, extra)
}
