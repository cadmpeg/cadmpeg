// SPDX-License-Identifier: Apache-2.0
//! Synthetic assembly documents (`docs/formats/solidedge.md` §9).

use super::{cluster_and_map, document, put_reals, put_words, type_table, Object};

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

fn occurrence_body(spec: &OccurrenceSpec) -> Vec<u8> {
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
