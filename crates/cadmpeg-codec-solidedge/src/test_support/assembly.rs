// SPDX-License-Identifier: Apache-2.0
//! Synthetic assembly documents (`docs/formats/solidedge.md` §9).

use super::patterns::{pattern_class_names, pattern_profile_objects, PATTERN_2D_OBJECT};
use super::{cluster_and_map, document, put_reals, put_words, type_table, Object, PatternSpec};

/// The class names of an assembly document, by form.
const ASSEMBLY_CLASSES: [&str; 3] = [
    "References Collection Object",
    "CLSID_JAsmReference",
    "CLSID_JSite",
];
const COLLECTION_FORM: u16 = 0;
const REFERENCE_FORM: u16 = 1;
const SITE_FORM: u16 = 2;
/// The object identifier of the references collection.
pub(crate) const COLLECTION: u32 = 1;
/// Bytes of an occurrence object, the frame included.
const OCCURRENCE_LEN: usize = 229;
/// The file moniker CLSID in stored byte order.
const MONIKER_CLSID: [u8; 16] = [0x03, 0x03, 0, 0, 0, 0, 0, 0, 0xc0, 0, 0, 0, 0, 0, 0, 0x46];

/// The row-major identity rotation.
const IDENTITY: [f64; 9] = [1.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 1.0];

/// One occurrence object.
pub(crate) struct OccurrenceSpec {
    pub(crate) id: u32,
    pub(crate) state: u32,
    pub(crate) site: u32,
    pub(crate) rotation: [f64; 9],
    /// The translation in metres.
    pub(crate) translation: [f64; 3],
    pub(crate) unit: f64,
    /// The object length to keep, the frame included, or `None` for all of it.
    pub(crate) keep: Option<usize>,
}

impl OccurrenceSpec {
    /// An occurrence of `site` at identity.
    pub(crate) fn at_identity(id: u32, site: u32) -> Self {
        Self {
            id,
            state: 2,
            site,
            rotation: IDENTITY,
            translation: [0.0; 3],
            unit: 1.0,
            keep: None,
        }
    }
}

/// One site object and its link stream.
pub(crate) struct SiteSpec<'a> {
    pub(crate) id: u32,
    /// The absolute and relative paths of the two monikers, or `None` for a
    /// site with no link stream.
    pub(crate) links: Option<[&'a str; 2]>,
    /// The Unicode path of each moniker's extension, or `None` for a moniker
    /// that stores no Unicode path.
    pub(crate) long: [Option<&'a str>; 2],
}

/// One file moniker: the CLSID, the up-level count, the counted path and the
/// terminating words.
fn moniker(up_level: u16, path: &str, long: Option<&str>) -> Vec<u8> {
    let mut bytes = MONIKER_CLSID.to_vec();
    bytes.extend_from_slice(&up_level.to_le_bytes());
    let length = u32::try_from(path.len() + 1).unwrap();
    bytes.extend_from_slice(&length.to_le_bytes());
    bytes.extend_from_slice(path.as_bytes());
    bytes.push(0);
    bytes.extend_from_slice(&0xFFFF_u16.to_le_bytes());
    bytes.extend_from_slice(&0xDEAD_u16.to_le_bytes());
    bytes.extend_from_slice(&[0; 20]);
    match long {
        None => bytes.extend_from_slice(&0_u32.to_le_bytes()),
        Some(long) => {
            let units: Vec<u8> = long
                .encode_utf16()
                .chain([0])
                .flat_map(u16::to_le_bytes)
                .collect();
            let count = u32::try_from(units.len()).unwrap();
            bytes.extend_from_slice(&(count + 6).to_le_bytes());
            bytes.extend_from_slice(&count.to_le_bytes());
            bytes.extend_from_slice(&3_u16.to_le_bytes());
            bytes.extend_from_slice(&units);
        }
    }
    bytes
}

/// The link stream of a site: a 12-byte header, the absolute moniker, other
/// bytes, then the relative moniker.
pub(crate) fn link_stream(links: [&str; 2], long: [Option<&str>; 2]) -> Vec<u8> {
    let mut bytes = vec![0_u8; 12];
    bytes.extend_from_slice(&moniker(0, links[0], long[0]));
    bytes.extend_from_slice(&[0x11; 6]);
    bytes.extend_from_slice(&moniker(1, links[1], long[1]));
    bytes
}

pub(super) fn occurrence_body(spec: &OccurrenceSpec) -> Vec<u8> {
    let mut body = vec![0_u8; OCCURRENCE_LEN - 8];
    put_words(&mut body, 16, &[spec.state]);
    put_words(&mut body, 113, &[spec.site]);
    put_reals(&mut body, 125, &spec.rotation);
    put_reals(&mut body, 197, &spec.translation);
    put_reals(&mut body, 221, &[spec.unit]);
    if let Some(keep) = spec.keep {
        body.truncate(keep.checked_sub(8).unwrap());
    }
    body
}

/// A document whose references collection lists `listed` and that holds the
/// occurrence objects `occurrences` and the sites `sites`.
pub(crate) fn assembly_document(
    listed: &[u32],
    occurrences: &[OccurrenceSpec],
    sites: &[SiteSpec<'_>],
) -> Vec<u8> {
    assembly_document_with_structure(listed, occurrences, sites, None)
}

/// The stream of an occurrence structure list: a 20-byte header with the entry
/// count at offset 16, then 20-byte entries of `(flags, object)`.
pub(crate) fn structure_stream(entries: &[(u32, u32)]) -> Vec<u8> {
    let mut bytes = vec![0_u8; 16];
    bytes.extend_from_slice(&u32::try_from(entries.len()).unwrap().to_le_bytes());
    for (flags, object) in entries {
        bytes.extend_from_slice(&flags.to_le_bytes());
        bytes.extend_from_slice(&[0; 4]);
        bytes.extend_from_slice(&object.to_le_bytes());
        bytes.extend_from_slice(&[0; 8]);
    }
    bytes
}

/// The bytes of a property list of `pairs` pairs.
fn property_list(pairs: u32) -> Vec<u8> {
    let mut bytes = pairs.to_le_bytes().to_vec();
    for key in 0..pairs {
        bytes.extend_from_slice(&key.to_le_bytes());
        bytes.extend_from_slice(&[0; 4]);
    }
    bytes
}

/// An entry of an occurrence structure list that carries a property list of
/// `pairs` pairs where `pairs` is `Some`.
pub(crate) fn structure_entry(flags: u32, object: u32, pairs: Option<u32>) -> Vec<u8> {
    let detail = if pairs.is_some() {
        0x40_0000 | 0x1_0000_u32
    } else {
        0x40_0000
    };
    let mut bytes = flags.to_le_bytes().to_vec();
    bytes.extend_from_slice(&detail.to_le_bytes());
    bytes.extend_from_slice(&object.to_le_bytes());
    bytes.extend_from_slice(&0x8000_0027_u32.to_le_bytes());
    bytes.extend_from_slice(&[0; 4]);
    if let Some(pairs) = pairs {
        bytes.extend(property_list(pairs));
    }
    bytes
}

/// A node of an occurrence structure list with the members `members`.
pub(crate) fn structure_node(object: u32, pairs: Option<u32>, members: &[Vec<u8>]) -> Vec<u8> {
    let detail = if pairs.is_some() { 0x1_0000_u32 } else { 0 };
    let mut bytes = 0x8000_0000_u32.to_le_bytes().to_vec();
    bytes.extend_from_slice(&detail.to_le_bytes());
    bytes.extend_from_slice(&object.to_le_bytes());
    if let Some(pairs) = pairs {
        bytes.extend(property_list(pairs));
    }
    bytes.extend_from_slice(&u32::try_from(members.len()).unwrap().to_le_bytes());
    for member in members {
        bytes.extend_from_slice(member);
    }
    bytes
}

/// A structure list of the items `items`, each an entry or a node.
pub(crate) fn structure_items(items: &[Vec<u8>]) -> Vec<u8> {
    let mut bytes = vec![0_u8; 16];
    bytes.extend_from_slice(&u32::try_from(items.len()).unwrap().to_le_bytes());
    for item in items {
        bytes.extend_from_slice(item);
    }
    bytes
}

/// As [`assembly_document`], with the `Preview/Structure/IOT` stream `structure`
/// where it is `Some`.
pub(crate) fn assembly_document_with_structure(
    listed: &[u32],
    occurrences: &[OccurrenceSpec],
    sites: &[SiteSpec<'_>],
    structure: Option<Vec<u8>>,
) -> Vec<u8> {
    let mut collection = vec![0_u8; 12];
    put_words(&mut collection, 16, &[u32::try_from(listed.len()).unwrap()]);
    collection.resize(12 + listed.len().div_ceil(8), 0);
    for id in listed {
        collection.extend_from_slice(&id.to_le_bytes());
    }
    let mut objects = vec![Object {
        id: COLLECTION,
        form: COLLECTION_FORM,
        flags: 0,
        body: collection,
    }];
    for occurrence in occurrences {
        objects.push(Object {
            id: occurrence.id,
            form: REFERENCE_FORM,
            flags: 0,
            body: occurrence_body(occurrence),
        });
    }
    let mut streams = Vec::new();
    for site in sites {
        objects.push(Object {
            id: site.id,
            form: SITE_FORM,
            flags: 0,
            body: vec![0; 16],
        });
        if let Some(links) = site.links {
            streams.push((
                format!("JSite{}/\u{1}Ole", site.id),
                link_stream(links, site.long),
            ));
        }
    }
    objects.sort_by_key(|object| object.id);
    let (cluster, map) = cluster_and_map(&objects);
    let mut files = vec![
        ("PSMcluster0".to_owned(), cluster),
        ("PSMspacemap/0x00000000".to_owned(), map),
        (
            "PSMtypetable".to_owned(),
            type_table(&ASSEMBLY_CLASSES, &[]),
        ),
    ];
    files.extend(streams);
    if let Some(structure) = structure {
        files.push(("Preview/Structure/IOT".to_owned(), structure));
    }
    let borrowed: Vec<(&str, Vec<u8>)> = files
        .iter()
        .map(|(name, bytes)| (name.as_str(), bytes.clone()))
        .collect();
    document(&borrowed)
}

/// The copies of an assembly pattern: `(first index, second index, reference
/// object)`.
pub(crate) type CopySpec = (u32, u32, u32);

/// An assembly pattern over the seed occurrence `references[0]`, whose copies
/// are the other `references`.
pub(crate) struct AssemblyPatternSpec<'a> {
    pub(crate) pattern: &'a PatternSpec<'a>,
    pub(crate) references: &'a [OccurrenceSpec],
    pub(crate) copies: &'a [CopySpec],
    /// The layout object the pattern names, or the pattern-2d object.
    pub(crate) layout: Option<u32>,
}

/// The object of the pattern, the first object of its copies and the site.
pub(crate) const PATTERN_OBJECT: u32 = 30;
const FIRST_COPY_OBJECT: u32 = 40;
pub(crate) const PATTERN_SITE: u32 = 6;

fn pattern_object_body(spec: &AssemblyPatternSpec<'_>) -> Vec<u8> {
    let mut body = vec![0_u8; 20 - 8 + 48];
    put_words(&mut body, 16, &[spec.references[0].id]);
    put_words(&mut body, 20, &[spec.layout.unwrap_or(PATTERN_2D_OBJECT)]);
    body.extend_from_slice(&u32::try_from(spec.copies.len()).unwrap().to_le_bytes());
    body.push(0);
    for copy in 0..spec.copies.len() {
        let id = FIRST_COPY_OBJECT + u32::try_from(copy).unwrap();
        body.extend_from_slice(&id.to_le_bytes());
        body.push(0x10);
        body.extend_from_slice(id.to_string().as_bytes());
        body.push(0);
    }
    body.extend_from_slice(&[0; 12]);
    body
}

fn copy_object_body(spec: &AssemblyPatternSpec<'_>, copy: CopySpec) -> Vec<u8> {
    let mut body = vec![0_u8; 40 - 8];
    put_words(&mut body, 12, &[copy.1, copy.0]);
    put_words(&mut body, 24, &[spec.references[0].id]);
    put_words(&mut body, 36, &[copy.2]);
    body
}

/// The bytes after the frame of the `CLSID_JPattern2d` object of an assembly
/// layout: the references to the sketch geometry, the anchor text and the
/// parameters 84 bytes after it.
fn layout_object_body(pattern: &PatternSpec<'_>) -> Vec<u8> {
    let mut object = vec![0_u8; 20];
    for index in 0..pattern.geometry.len() {
        let id = PATTERN_2D_OBJECT + 1 + u32::try_from(index).unwrap();
        for word in [0x10_u32, 0x8000_0000, id] {
            object.extend_from_slice(&word.to_le_bytes());
        }
    }
    object.extend_from_slice(b"JPattern2dGeomsFunction\0");
    let base = object.len() + 84;
    object.resize(base + 50, 0);
    object[base + 14..base + 18].copy_from_slice(&pattern.kind.to_le_bytes());
    object[base + 26..base + 30].copy_from_slice(&pattern.counts[0].to_le_bytes());
    object[base + 30..base + 34].copy_from_slice(&pattern.counts[1].to_le_bytes());
    object[base + 34..base + 42].copy_from_slice(&pattern.steps[0].to_le_bytes());
    object[base + 42..base + 50].copy_from_slice(&pattern.steps[1].to_le_bytes());
    object
}

/// An assembly document with a pattern: the pattern part's profile objects,
/// the references collection listing every reference, the references, one
/// site, the pattern object and its copy objects.
pub(crate) fn assembly_pattern_document(spec: &AssemblyPatternSpec<'_>) -> Vec<u8> {
    let first = u32::try_from(pattern_class_names().len()).unwrap();
    let form = |offset: u32| u16::try_from(first + offset).unwrap();
    let mut names: Vec<&str> = pattern_class_names().to_vec();
    names.extend([
        "References Collection Object",
        "CLSID_JAsmReference",
        "CLSID_JSite",
        "CLSID_JAsmPattern",
        "CLSID_JAsmPatternOcc",
    ]);
    let listed: Vec<u32> = spec
        .references
        .iter()
        .map(|occurrence| occurrence.id)
        .collect();
    let mut collection = vec![0_u8; 12];
    put_words(&mut collection, 16, &[u32::try_from(listed.len()).unwrap()]);
    collection.resize(12 + listed.len().div_ceil(8), 0);
    for id in &listed {
        collection.extend_from_slice(&id.to_le_bytes());
    }
    let mut objects = pattern_profile_objects(spec.pattern);
    for object in &mut objects {
        if object.id == PATTERN_2D_OBJECT {
            object.body = layout_object_body(spec.pattern);
        }
    }
    objects.push(Object {
        id: COLLECTION,
        form: form(0),
        flags: 0,
        body: collection,
    });
    for occurrence in spec.references {
        objects.push(Object {
            id: occurrence.id,
            form: form(1),
            flags: 0,
            body: occurrence_body(occurrence),
        });
    }
    objects.push(Object {
        id: PATTERN_SITE,
        form: form(2),
        flags: 0,
        body: vec![0; 16],
    });
    objects.push(Object {
        id: PATTERN_OBJECT,
        form: form(3),
        flags: 0,
        body: pattern_object_body(spec),
    });
    for (index, copy) in spec.copies.iter().enumerate() {
        objects.push(Object {
            id: FIRST_COPY_OBJECT + u32::try_from(index).unwrap(),
            form: form(4),
            flags: 0,
            body: copy_object_body(spec, *copy),
        });
    }
    objects.sort_by_key(|object| object.id);
    let (cluster, map) = cluster_and_map(&objects);
    document(&[
        ("PSMcluster0", cluster),
        ("PSMspacemap/0x00000000", map),
        ("PSMtypetable", type_table(&names, &[])),
    ])
}
