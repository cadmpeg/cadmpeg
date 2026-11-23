// SPDX-License-Identifier: Apache-2.0
//! Synthetic assembly documents (`docs/formats/solidedge.md` §9).

use super::{
    cluster_and_map, document, document_with_root, put_reals, put_words, type_table, Object,
};

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
}

/// One file moniker: the CLSID, the up-level count, the counted path and the
/// terminating words.
fn moniker(up_level: u16, path: &str) -> Vec<u8> {
    let mut bytes = MONIKER_CLSID.to_vec();
    bytes.extend_from_slice(&up_level.to_le_bytes());
    let length = u32::try_from(path.len() + 1).unwrap();
    bytes.extend_from_slice(&length.to_le_bytes());
    bytes.extend_from_slice(path.as_bytes());
    bytes.push(0);
    bytes.extend_from_slice(&0xFFFF_u16.to_le_bytes());
    bytes.extend_from_slice(&0xDEAD_u16.to_le_bytes());
    bytes
}

/// The link stream of a site: a 12-byte header, the absolute moniker, other
/// bytes, then the relative moniker.
fn link_stream(links: [&str; 2]) -> Vec<u8> {
    let mut bytes = vec![0_u8; 12];
    bytes.extend_from_slice(&moniker(0, links[0]));
    bytes.extend_from_slice(&[0x11; 6]);
    bytes.extend_from_slice(&moniker(1, links[1]));
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
    let files = assembly_streams(listed, occurrences, sites);
    let borrowed: Vec<(&str, Vec<u8>)> = files
        .iter()
        .map(|(name, bytes)| (name.as_str(), bytes.clone()))
        .collect();
    document(&borrowed)
}

/// The streams of [`assembly_document`], by path.
fn assembly_streams(
    listed: &[u32],
    occurrences: &[OccurrenceSpec],
    sites: &[SiteSpec<'_>],
) -> Vec<(String, Vec<u8>)> {
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
            streams.push((format!("JSite{}/\u{1}Ole", site.id), link_stream(links)));
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
    files
}

/// The `FamilyInfo` stream naming `active`.
pub(crate) fn family_info(active: &str) -> Vec<u8> {
    unit_string(active)
}

fn unit_string(name: &str) -> Vec<u8> {
    let units: Vec<u16> = name.encode_utf16().chain([0]).collect();
    let mut bytes = u32::try_from(units.len()).unwrap().to_le_bytes().to_vec();
    for unit in units {
        bytes.extend_from_slice(&unit.to_le_bytes());
    }
    bytes
}

/// The member table: a header and one record per `(name, listed)` member. A
/// record holds the name, the empty prefix list, two blocks, the entries of
/// `listed` and a trailer.
pub(crate) fn member_table(members: &[(&str, &[u32])]) -> Vec<u8> {
    let mut bytes = b"\x44\xf5\x90\x6c\x03\0\0\0QQP\0\0\xa0\0\0".to_vec();
    bytes.extend_from_slice(&u32::try_from(members.len()).unwrap().to_le_bytes());
    for (ordinal, (name, listed)) in members.iter().enumerate() {
        bytes.extend_from_slice(b"QQQ\0");
        bytes.extend_from_slice(&(0xa001 + u32::try_from(ordinal).unwrap()).to_le_bytes());
        bytes.extend_from_slice(&unit_string(name));
        bytes.extend_from_slice(&[0; 16]);
        for _ in 0..2 {
            bytes.extend_from_slice(&1_u32.to_le_bytes());
            bytes.extend_from_slice(&[0x5a; 16]);
            bytes.extend_from_slice(&0_u32.to_le_bytes());
        }
        bytes.extend_from_slice(&u32::try_from(listed.len()).unwrap().to_le_bytes());
        for id in *listed {
            bytes.extend_from_slice(&id.to_le_bytes());
            bytes.extend_from_slice(&[0; 13 * 8 + 3]);
        }
        bytes.extend_from_slice(&0_u32.to_le_bytes());
        bytes.extend_from_slice(&3_u32.to_le_bytes());
    }
    bytes
}

/// One member storage of a family document.
pub(crate) struct MemberSpec<'a> {
    pub(crate) name: &'a str,
    pub(crate) listed: &'a [u32],
    pub(crate) occurrences: Vec<OccurrenceSpec>,
}

/// A family of assemblies: the root streams, the master storage holding the
/// member table and one storage per member with the occurrences of its spec.
/// `info` is the `FamilyInfo` stream; `table` replaces the member table when
/// given.
pub(crate) fn family_document(
    info: Vec<u8>,
    table: Option<Vec<u8>>,
    members: &[MemberSpec<'_>],
    sites: &[SiteSpec<'_>],
) -> Vec<u8> {
    let mut files = vec![(
        "Master\u{1}/Family of Assemblies Members".to_owned(),
        table.unwrap_or_else(|| {
            member_table(
                &members
                    .iter()
                    .map(|member| (member.name, member.listed))
                    .collect::<Vec<_>>(),
            )
        }),
    )];
    for member in members {
        let listed: Vec<u32> = member.occurrences.iter().map(|spec| spec.id).collect();
        for (path, bytes) in assembly_streams(&listed, &member.occurrences, sites) {
            files.push((format!("{}/{path}", member.name), bytes));
        }
        files.push((format!("{}/PSMroots", member.name), Vec::new()));
        files.push((format!("{}/JSitesList", member.name), Vec::new()));
    }
    let borrowed: Vec<(&str, Vec<u8>)> = files
        .iter()
        .map(|(name, bytes)| (name.as_str(), bytes.clone()))
        .collect();
    document_with_root(
        &[("FamilyInfo", info), ("FOAPublishData", vec![0; 8])],
        &borrowed,
    )
}
