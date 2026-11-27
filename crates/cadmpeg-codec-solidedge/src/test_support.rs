// SPDX-License-Identifier: Apache-2.0
#![allow(clippy::unwrap_used)]

use cadmpeg_container::compound::CompoundClassId;
use cadmpeg_test_support::bytes::{put_u16, put_u32};

/// Root class of a part document.
pub(crate) const PART: CompoundClassId = CompoundClassId::from_guid(
    0x23c5_2e80,
    0x4698,
    0x11ce,
    [0xb3, 0x07, 0x08, 0x00, 0x36, 0x3a, 0x1e, 0x02],
);
/// Root class of a sheet-metal document.
pub(crate) const SHEET_METAL: CompoundClassId = CompoundClassId::from_guid(
    0xdd85_22e0,
    0x2375,
    0x11d0,
    [0xac, 0x05, 0x08, 0x00, 0x36, 0xfd, 0x18, 0x02],
);
/// Root class of an assembly document.
pub(crate) const ASSEMBLY: CompoundClassId = CompoundClassId::from_guid(
    0x00c6_bf00,
    0x483b,
    0x11ce,
    [0x95, 0x1a, 0x08, 0x00, 0x36, 0x01, 0xbe, 0x52],
);
/// Root class of a family-of-assemblies document.
pub(crate) const FAMILY: CompoundClassId = CompoundClassId::from_guid(
    0x04d6_13a0,
    0xa322,
    0x40b5,
    [0xa2, 0xa4, 0x36, 0xca, 0x0d, 0xe6, 0xf5, 0xd9],
);
/// A root class that no Solid Edge document kind uses.
pub(crate) const OTHER: CompoundClassId =
    CompoundClassId::from_guid(0x0000_0001, 0x0002, 0x0003, [4, 5, 6, 7, 8, 9, 10, 11]);

/// Compound document whose root holds the two named stream entries and the given class.
pub(crate) fn compound_with(first: &str, second: &str, class: CompoundClassId) -> Vec<u8> {
    let mut file = cadmpeg_test_support::compound::compound_fixture();
    let directory = &mut file[512..1024];
    directory[80..96].copy_from_slice(&class.to_bytes());
    directory[128..192].fill(0);
    name(&mut directory[128..256], first);
    put_u32(directory, 128 + 72, 2);
    let entry = &mut directory[256..384];
    name(entry, second);
    entry[66] = 2;
    entry[67] = 0;
    entry[68..80].fill(0xff);
    put_u32(entry, 116, 0xffff_fffe);
    file
}

pub(crate) fn compound() -> Vec<u8> {
    compound_with("PSMroots", "JSitesList", PART)
}

fn name(entry: &mut [u8], value: &str) {
    for (i, unit) in value.encode_utf16().enumerate() {
        put_u16(entry, i * 2, unit);
    }
    put_u16(entry, 64, u16::try_from((value.len() + 1) * 2).unwrap());
}

/// A Solid Edge directory whose `PSMcluster0` stream holds `cluster`,
/// zero-padded to the 4096-byte regular-stream cutoff.
pub(crate) fn with_cluster(cluster: &[u8]) -> Vec<u8> {
    assert!(cluster.len() <= 4096);
    let mut file = compound();
    let directory = &mut file[512..1024];
    // PSMroots becomes empty so the cluster owns the data chain.
    put_u32(directory, 128 + 116, 0xffff_fffe);
    put_u32(directory, 128 + 120, 0);
    put_u32(directory, 256 + 72, 3);
    let entry = &mut directory[384..512];
    entry.fill(0);
    name(entry, "PSMcluster0");
    entry[66] = 2;
    entry[67] = 1;
    entry[68..80].fill(0xff);
    put_u32(entry, 116, 1);
    put_u32(entry, 120, 4096);
    let data = &mut file[1024..1024 + 4096];
    data.fill(0);
    data[..cluster.len()].copy_from_slice(cluster);
    file
}

/// One property record value.
pub(crate) enum RecordValue {
    Word(u32, u32),
    Real(f64),
}

/// A property set with index `index` holding `records` in order.
pub(crate) fn property_set(index: u32, records: &[(&str, RecordValue)]) -> Vec<u8> {
    property_set_of_form(1, index, records)
}

/// A property set with set form `form`.
pub(crate) fn property_set_of_form(
    form: u16,
    index: u32,
    records: &[(&str, RecordValue)],
) -> Vec<u8> {
    let mut out = vec![0, 0, 0, 0, 0x51, 0x51];
    out.extend_from_slice(&form.to_le_bytes());
    out.extend_from_slice(&index.to_le_bytes());
    out.extend_from_slice(&u32::try_from(records.len()).unwrap().to_le_bytes());
    for (record, (name, value)) in records.iter().enumerate() {
        let units = name.encode_utf16().chain([0]).collect::<Vec<_>>();
        out.extend_from_slice(&u32::try_from(units.len() * 2).unwrap().to_le_bytes());
        for unit in units {
            out.extend_from_slice(&unit.to_le_bytes());
        }
        out.extend_from_slice(&u32::try_from(record).unwrap().to_le_bytes());
        out.extend_from_slice(&1_u32.to_le_bytes());
        match value {
            RecordValue::Word(kind, word) => {
                out.extend_from_slice(&kind.to_le_bytes());
                out.extend_from_slice(&word.to_le_bytes());
            }
            RecordValue::Real(real) => {
                out.extend_from_slice(&4_u32.to_le_bytes());
                out.extend_from_slice(&real.to_le_bytes());
            }
        }
    }
    out
}

const SECTOR: usize = 512;
const MINI_SECTOR: usize = 64;
const MINI_CUTOFF: usize = 4096;
const FAT_ENTRIES: usize = SECTOR / 4;
const END_OF_CHAIN: u32 = 0xffff_fffe;
const FAT_SECTOR: u32 = 0xffff_fffd;
const FREE: u32 = 0xffff_ffff;

/// A compound document node of [`document`].
struct Node {
    name: String,
    stream: Option<Vec<u8>>,
    parent: usize,
}

/// The sort key of a compound directory name: length, then upper case.
fn name_key(name: &str) -> (usize, String) {
    (name.encode_utf16().count(), name.to_uppercase())
}

fn chunks(len: usize, size: usize) -> usize {
    len.div_ceil(size)
}

/// A compound document with a Solid Edge directory signature and the streams
/// `streams`, each named by its path (`PSMspacemap/0x00000000` puts a stream
/// in a storage). A stream under 4096 bytes lives in the mini stream, so its
/// length is exact.
pub(crate) fn document(streams: &[(&str, Vec<u8>)]) -> Vec<u8> {
    let mut nodes = vec![Node {
        name: "Root Entry".into(),
        stream: None,
        parent: 0,
    }];
    for name in ["PSMroots", "JSitesList"] {
        nodes.push(Node {
            name: name.into(),
            stream: Some(Vec::new()),
            parent: 0,
        });
    }
    for (path, data) in streams {
        let mut parent = 0;
        let mut parts = path.split('/').peekable();
        while let Some(part) = parts.next() {
            if parts.peek().is_some() {
                parent = match nodes.iter().position(|node| {
                    node.parent == parent && node.name == part && node.stream.is_none()
                }) {
                    Some(found) => found,
                    None => {
                        nodes.push(Node {
                            name: part.into(),
                            stream: None,
                            parent,
                        });
                        nodes.len() - 1
                    }
                };
            } else {
                nodes.push(Node {
                    name: part.into(),
                    stream: Some(data.clone()),
                    parent,
                });
            }
        }
    }

    // Sector plan: directory, mini FAT, mini stream, regular streams, FAT.
    let directory_sectors = chunks(nodes.len(), 4);
    let mut mini = Vec::new();
    let mut mini_start = vec![END_OF_CHAIN; nodes.len()];
    let mut regular_start = vec![END_OF_CHAIN; nodes.len()];
    let mut regular = Vec::new();
    for (index, node) in nodes.iter().enumerate() {
        let Some(data) = &node.stream else { continue };
        if data.is_empty() {
            continue;
        }
        if data.len() < MINI_CUTOFF {
            mini_start[index] = u32::try_from(mini.len() / MINI_SECTOR).unwrap();
            mini.extend_from_slice(data);
            mini.resize(chunks(mini.len(), MINI_SECTOR) * MINI_SECTOR, 0);
        } else {
            regular_start[index] = u32::try_from(regular.len() / SECTOR).unwrap();
            regular.extend_from_slice(data);
            regular.resize(chunks(regular.len(), SECTOR) * SECTOR, 0);
        }
    }
    let mini_sectors = mini.len() / MINI_SECTOR;
    let mini_fat_sectors = chunks(mini_sectors * 4, SECTOR);
    let mini_stream_sectors = chunks(mini.len(), SECTOR);
    let regular_sectors = regular.len() / SECTOR;
    let base = directory_sectors + mini_fat_sectors + mini_stream_sectors + regular_sectors;
    let mut fat_sectors = 1;
    while base + fat_sectors > fat_sectors * FAT_ENTRIES {
        fat_sectors += 1;
    }
    assert!(fat_sectors <= 109);
    let total = base + fat_sectors;
    let mini_fat_at = directory_sectors;
    let mini_stream_at = mini_fat_at + mini_fat_sectors;
    let regular_at = mini_stream_at + mini_stream_sectors;
    let fat_at = regular_at + regular_sectors;

    let mut fat = vec![FREE; fat_sectors * FAT_ENTRIES];
    let chain = |table: &mut [u32], first: usize, count: usize| {
        for (offset, entry) in table[first..first + count].iter_mut().enumerate() {
            *entry = if offset + 1 == count {
                END_OF_CHAIN
            } else {
                u32::try_from(first + offset + 1).unwrap()
            };
        }
    };
    chain(&mut fat, 0, directory_sectors);
    chain(&mut fat, mini_fat_at, mini_fat_sectors);
    chain(&mut fat, mini_stream_at, mini_stream_sectors);
    for (index, node) in nodes.iter().enumerate() {
        if let Some(data) = node
            .stream
            .as_ref()
            .filter(|data| data.len() >= MINI_CUTOFF)
        {
            chain(
                &mut fat,
                regular_at + usize::try_from(regular_start[index]).unwrap(),
                chunks(data.len(), SECTOR),
            );
        }
    }
    fat[fat_at..total].fill(FAT_SECTOR);
    let mut mini_fat = vec![FREE; mini_fat_sectors * FAT_ENTRIES];
    for (index, node) in nodes.iter().enumerate() {
        if let Some(data) = node
            .stream
            .as_ref()
            .filter(|data| (1..MINI_CUTOFF).contains(&data.len()))
        {
            let first = usize::try_from(mini_start[index]).unwrap();
            let count = chunks(data.len(), MINI_SECTOR);
            chain(&mut mini_fat, first, count);
        }
    }

    // Directory entries; every sibling run is a sorted right-linked chain of
    // black nodes.
    let mut directory = vec![0_u8; directory_sectors * SECTOR];
    for entry in directory.chunks_exact_mut(128) {
        entry[68..80].fill(0xff);
    }
    for (index, node) in nodes.iter().enumerate() {
        let entry = &mut directory[index * 128..(index + 1) * 128];
        for (unit_at, unit) in node.name.encode_utf16().enumerate() {
            put_u16(entry, unit_at * 2, unit);
        }
        put_u16(
            entry,
            64,
            u16::try_from((node.name.encode_utf16().count() + 1) * 2).unwrap(),
        );
        entry[67] = 1;
        let mut siblings: Vec<usize> = (1..nodes.len())
            .filter(|&other| nodes[other].parent == index && other != index)
            .collect();
        siblings.sort_by_key(|&other| name_key(&nodes[other].name));
        if index == 0 {
            entry[66] = 5;
            entry[80..96].copy_from_slice(&PART.to_bytes());
            put_u32(
                entry,
                116,
                if mini.is_empty() {
                    END_OF_CHAIN
                } else {
                    u32::try_from(mini_stream_at).unwrap()
                },
            );
            entry[120..128].copy_from_slice(&u64::try_from(mini.len()).unwrap().to_le_bytes());
        } else if let Some(data) = &node.stream {
            entry[66] = 2;
            let start = if data.is_empty() {
                END_OF_CHAIN
            } else if data.len() < MINI_CUTOFF {
                mini_start[index]
            } else {
                u32::try_from(regular_at).unwrap() + regular_start[index]
            };
            put_u32(entry, 116, start);
            entry[120..128].copy_from_slice(&u64::try_from(data.len()).unwrap().to_le_bytes());
        } else {
            entry[66] = 1;
        }
        if let Some(&first) = siblings.first() {
            put_u32(entry, 76, u32::try_from(first).unwrap());
        }
    }
    for index in 0..nodes.len() {
        let mut siblings: Vec<usize> = (1..nodes.len())
            .filter(|&other| nodes[other].parent == index && other != index)
            .collect();
        siblings.sort_by_key(|&other| name_key(&nodes[other].name));
        for pair in siblings.windows(2) {
            put_u32(
                &mut directory,
                pair[0] * 128 + 72,
                u32::try_from(pair[1]).unwrap(),
            );
        }
    }

    let mut file = vec![0_u8; SECTOR];
    file[..8].copy_from_slice(&[0xd0, 0xcf, 0x11, 0xe0, 0xa1, 0xb1, 0x1a, 0xe1]);
    put_u16(&mut file, 24, 0x003e);
    put_u16(&mut file, 26, 3);
    put_u16(&mut file, 28, 0xfffe);
    put_u16(&mut file, 30, 9);
    put_u16(&mut file, 32, 6);
    put_u32(&mut file, 44, u32::try_from(fat_sectors).unwrap());
    put_u32(&mut file, 48, 0);
    put_u32(&mut file, 56, u32::try_from(MINI_CUTOFF).unwrap());
    put_u32(
        &mut file,
        60,
        if mini_fat_sectors == 0 {
            END_OF_CHAIN
        } else {
            u32::try_from(mini_fat_at).unwrap()
        },
    );
    put_u32(&mut file, 64, u32::try_from(mini_fat_sectors).unwrap());
    put_u32(&mut file, 68, END_OF_CHAIN);
    for index in 0..109 {
        let value = if index < fat_sectors {
            u32::try_from(fat_at + index).unwrap()
        } else {
            FREE
        };
        put_u32(&mut file, 76 + index * 4, value);
    }
    file.extend_from_slice(&directory);
    for entry in &mini_fat {
        file.extend_from_slice(&entry.to_le_bytes());
    }
    let mut padded = mini.clone();
    padded.resize(mini_stream_sectors * SECTOR, 0);
    file.extend_from_slice(&padded);
    file.extend_from_slice(&regular);
    for entry in &fat {
        file.extend_from_slice(&entry.to_le_bytes());
    }
    assert_eq!(file.len(), (total + 1) * SECTOR);
    file
}

/// One object of a synthetic PSM cluster.
pub(crate) struct Object {
    pub(crate) id: u32,
    pub(crate) form: u16,
    pub(crate) flags: u16,
    /// The bytes after the 8-byte frame.
    pub(crate) body: Vec<u8>,
}

/// The cluster stream and the space-map stream of `objects`, in order.
pub(crate) fn cluster_and_map(objects: &[Object]) -> (Vec<u8>, Vec<u8>) {
    let count = u32::try_from(objects.len()).unwrap();
    let mut cluster = Vec::new();
    cluster.extend_from_slice(&0x6C90_F544_u32.to_le_bytes());
    cluster.extend_from_slice(&count.to_le_bytes());
    let mut map = b"sseg".to_vec();
    map.extend_from_slice(&u16::try_from(objects.len()).unwrap().to_le_bytes());
    map.extend_from_slice(&0_u16.to_le_bytes());
    map.extend_from_slice(&u16::try_from(objects.len() + 1).unwrap().to_le_bytes());
    map.extend_from_slice(&0_u16.to_le_bytes());
    for object in objects {
        let offset = u32::try_from(cluster.len()).unwrap();
        cluster.extend_from_slice(&0x5151_u16.to_le_bytes());
        cluster.extend_from_slice(&object.form.to_le_bytes());
        cluster.extend_from_slice(&object.id.to_le_bytes());
        cluster.extend_from_slice(&object.body);
        map.extend_from_slice(&16_u16.to_le_bytes());
        map.extend_from_slice(&offset.to_le_bytes());
        map.extend_from_slice(&object.id.to_le_bytes());
        map.extend_from_slice(&(object.form << 6).to_le_bytes());
        map.extend_from_slice(&0_u16.to_le_bytes());
        map.extend_from_slice(&0_u16.to_le_bytes());
        map.extend_from_slice(&object.flags.to_le_bytes());
    }
    (cluster, map)
}

fn section_header(count: usize) -> Vec<u8> {
    let count = u16::try_from(count).unwrap();
    let mut header = b"ePyT".to_vec();
    header.resize(20, 0);
    header[10..12].copy_from_slice(&count.to_le_bytes());
    header[14..16].copy_from_slice(&count.to_le_bytes());
    header
}

/// A type table whose named declarations are `names` in order and whose
/// compact records carry the values `links`.
pub(crate) fn type_table(names: &[&str], links: &[u16]) -> Vec<u8> {
    let mut table = section_header(names.len());
    for (index, name) in names.iter().enumerate() {
        table.extend_from_slice(&[u8::try_from(index).unwrap() ^ 0xa5; 16]);
        table.extend_from_slice(&u32::try_from(index).unwrap().to_le_bytes());
        table.extend_from_slice(&1_u16.to_le_bytes());
        table.extend_from_slice(&0xffff_u16.to_le_bytes());
        table.extend_from_slice(&7_u32.to_le_bytes());
        table.extend_from_slice(&u16::try_from(name.len() + 1).unwrap().to_le_bytes());
        table.extend_from_slice(name.as_bytes());
        table.push(0);
        table.extend_from_slice(&2_u16.to_le_bytes());
        table.extend_from_slice(b"x\0");
    }
    table.extend_from_slice(&section_header(links.len()));
    for link in links {
        let mut record = [0_u8; 26];
        record[18..20].copy_from_slice(&link.to_le_bytes());
        table.extend_from_slice(&record);
    }
    table
}

/// The navigator GUID in stored byte order.
pub(crate) const NAVIGATOR_GUID: [u8; 16] = [
    0x50, 0x6D, 0xF6, 0x44, 0xD1, 0x4D, 0xCE, 0x11, 0xBF, 0x4E, 0x08, 0x00, 0x36, 0x01, 0xC7, 0x2A,
];

/// The bytes after the frame of a navigator object that lists the three base
/// planes `planes` and then `features`.
pub(crate) fn navigator_body(planes: [u32; 3], features: &[u32]) -> Vec<u8> {
    navigator_body_last(planes, features, features.last().copied().unwrap_or(0))
}

/// As [`navigator_body`], with the last-feature identifier `last`.
pub(crate) fn navigator_body_last(planes: [u32; 3], features: &[u32], last: u32) -> Vec<u8> {
    let mut body = 0_u32.to_le_bytes().to_vec();
    body.extend_from_slice(&NAVIGATOR_GUID);
    let word = |body: &mut Vec<u8>, value: u32| body.extend_from_slice(&value.to_le_bytes());
    word(&mut body, 2);
    word(&mut body, 100);
    word(&mut body, 101);
    word(&mut body, 1);
    word(&mut body, 102);
    body.extend_from_slice(&[0; 147]);
    word(&mut body, 31);
    body.extend_from_slice(b"JMFeatureNavigatorPartModified\0");
    body.extend_from_slice(&[0; 68]);
    for value in [0, 0, 1, 1] {
        word(&mut body, value);
    }
    word(&mut body, 7);
    word(&mut body, last);
    word(
        &mut body,
        u32::try_from(planes.len() + features.len()).unwrap(),
    );
    for id in planes.iter().chain(features) {
        word(&mut body, 0);
        word(&mut body, *id);
    }
    word(&mut body, 0);
    body
}

/// The bytes after the frame of a `CLSID_JMPart` object with function name
/// `function`.
pub(crate) fn part_body(function: &str) -> Vec<u8> {
    part_body_state(function, 0)
}

/// As [`part_body`], with the state word `state` at object offset 8.
pub(crate) fn part_body_state(function: &str, state: u32) -> Vec<u8> {
    // The frame is 8 bytes; the counted block starts at object offset 132.
    let mut body = vec![0_u8; 132 - 8];
    body[..4].copy_from_slice(&state.to_le_bytes());
    body.extend_from_slice(&6_u32.to_le_bytes());
    body.extend_from_slice(&[0xee; 6]);
    body.extend_from_slice(&u32::try_from(function.len() + 1).unwrap().to_le_bytes());
    body.extend_from_slice(function.as_bytes());
    body.push(0);
    body
}

/// The bytes after the frame of a `CLSID_JMFeatureGroup` object that lists
/// `members`, each pair led by `marker`.
fn folder_body(members: &[u32], marker: u32) -> Vec<u8> {
    let mut body = u32::try_from(members.len()).unwrap().to_le_bytes().to_vec();
    for member in members {
        body.extend_from_slice(&marker.to_le_bytes());
        body.extend_from_slice(&member.to_le_bytes());
    }
    body
}

/// One feature of [`feature_document`].
pub(crate) enum Feature<'a> {
    /// A `CLSID_JMPart` with this function name.
    Part(&'a str),
    /// A `CLSID_JMPart` with this function name and state word.
    PartState(&'a str, u32),
    /// A `CLSID_JMGroup`.
    Group,
    /// A `CLSID_JMFeatureGroup` that lists these member object identifiers.
    Folder(&'a [u32]),
    /// A `CLSID_JMFeatureGroup` whose pairs carry the marker 2.
    FolderBadMarker(&'a [u32]),
    /// An object of this other class.
    Other(&'a str),
}

/// A document whose cluster 0 holds the three base planes, the given
/// features and a navigator that lists them in order. Every object's form is
/// its declaration index, since the compact table is empty.
pub(crate) fn feature_document(features: &[Feature<'_>]) -> Vec<u8> {
    feature_document_last(features, None)
}

/// As [`feature_document`], with the navigator's last-feature identifier
/// `last` when given.
pub(crate) fn feature_document_last(features: &[Feature<'_>], last: Option<u32>) -> Vec<u8> {
    let mut classes = vec![
        "ReferencePlaneObject",
        "CLSID_JMPart",
        "CLSID_JMGroup",
        "CLSID_JMPartOwner",
        "FeatureNavigatorObject",
        "CLSID_JMFeatureGroup",
    ];
    let mut objects = Vec::new();
    for id in 1..=3 {
        objects.push(Object {
            id,
            form: 0,
            flags: 0,
            body: vec![0; 8],
        });
    }
    let mut listed = Vec::new();
    for (index, feature) in features.iter().enumerate() {
        let id = 10 + u32::try_from(index).unwrap();
        listed.push(id);
        let (form, body) = match feature {
            Feature::Part(function) => (1, part_body(function)),
            Feature::PartState(function, state) => (1, part_body_state(function, *state)),
            Feature::Group => (2, vec![0; 8]),
            Feature::Folder(members) => (5, folder_body(members, 1)),
            Feature::FolderBadMarker(members) => (5, folder_body(members, 2)),
            Feature::Other(class) => {
                classes.push(class);
                (u16::try_from(classes.len() - 1).unwrap(), vec![0; 8])
            }
        };
        objects.push(Object {
            id,
            form,
            flags: 0,
            body,
        });
    }
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
        body: navigator_body_last(
            [1, 2, 3],
            &listed,
            last.unwrap_or(listed.last().copied().unwrap_or(0)),
        ),
    });
    objects.sort_by_key(|object| object.id);
    let (cluster, map) = cluster_and_map(&objects);
    document(&[
        ("PSMcluster0", cluster),
        ("PSMspacemap/0x00000000", map),
        ("PSMtypetable", type_table(&classes, &[])),
    ])
}

pub(crate) fn with_partitions(streams: &[Vec<u8>]) -> Vec<u8> {
    let fat = 2 + 8 * streams.len();
    let mut file = vec![0u8; (fat + 2) * 512];
    let base = compound();
    file[..512].copy_from_slice(&base[..512]);
    put_u32(&mut file, 76, u32::try_from(fat).unwrap());
    let directory = &mut file[512..1536];
    for entry in directory.chunks_exact_mut(128) {
        entry[68..80].fill(255);
    }
    entry(directory, 0, "Root Entry", 5, 1, None);
    directory[80..96].copy_from_slice(&PART.to_bytes());
    entry(directory, 1, "PSMroots", 2, u32::MAX, None);
    put_u32(directory, 128 + 72, 3);
    entry(directory, 2, "JSitesList", 2, u32::MAX, None);
    put_u32(directory, 384 + 72, 2);
    entry(directory, 3, "PARASOLID", 1, 4, None);
    for (index, stream) in streams.iter().enumerate() {
        assert!(stream.len() <= 4096);
        let slot = 4 + index;
        entry(
            directory,
            slot,
            &format!("STREAM{index}.P_B"),
            2,
            u32::MAX,
            Some(2 + 8 * index),
        );
        if index + 1 < streams.len() {
            put_u32(directory, slot * 128 + 72, u32::try_from(slot + 1).unwrap());
        }
    }
    for (index, stream) in streams.iter().enumerate() {
        let start = (3 + 8 * index) * 512;
        file[start..start + stream.len()].copy_from_slice(stream);
    }
    let table = &mut file[(fat + 1) * 512..(fat + 2) * 512];
    table.fill(255);
    put_u32(table, 0, 1);
    put_u32(table, 4, 0xffff_fffe);
    for index in 0..streams.len() {
        for sector in 2 + index * 8..10 + index * 8 {
            put_u32(
                table,
                sector * 4,
                if sector == 9 + index * 8 {
                    0xffff_fffe
                } else {
                    u32::try_from(sector + 1).unwrap()
                },
            );
        }
    }
    put_u32(table, fat * 4, 0xffff_fffd);
    file
}

fn entry(
    directory: &mut [u8],
    index: usize,
    value: &str,
    kind: u8,
    child: u32,
    sector: Option<usize>,
) {
    let bytes = &mut directory[index * 128..(index + 1) * 128];
    name(bytes, value);
    bytes[66] = kind;
    bytes[67] = 1;
    put_u32(bytes, 76, child);
    put_u32(
        bytes,
        116,
        sector.map_or(0xffff_fffe, |value| u32::try_from(value).unwrap()),
    );
    bytes[120..128].copy_from_slice(&(if sector.is_some() { 4096u64 } else { 0 }).to_le_bytes());
}

pub(crate) fn sphere_partition() -> Vec<u8> {
    let description = b": TRANSMIT FILE (partition)";
    let schema = b"SCH_999_13006";
    let mut bytes = b"PS".to_vec();
    bytes.extend_from_slice(&u32::try_from(description.len()).unwrap().to_be_bytes());
    bytes.extend_from_slice(description);
    bytes.extend_from_slice(&u32::try_from(schema.len()).unwrap().to_be_bytes());
    bytes.extend_from_slice(schema);
    bytes.extend_from_slice(&0u32.to_be_bytes());
    node(&mut bytes, 12, 2);
    integer(&mut bytes, 0);
    for _ in 0..6 {
        pointer(&mut bytes, 1);
    }
    real(&mut bytes, 1.0e-8);
    real(&mut bytes, 1.0e-8);
    for _ in 0..3 {
        pointer(&mut bytes, 1);
    }
    bytes.push(0);
    pointer(&mut bytes, 1);
    bytes.extend_from_slice(&[1, 0]);
    pointer(&mut bytes, 3);
    for _ in 0..6 {
        pointer(&mut bytes, 1);
    }
    node(&mut bytes, 13, 3);
    integer(&mut bytes, 0);
    pointer(&mut bytes, 1);
    pointer(&mut bytes, 2);
    pointer(&mut bytes, 1);
    pointer(&mut bytes, 4);
    for _ in 0..4 {
        pointer(&mut bytes, 1);
    }
    node(&mut bytes, 14, 4);
    integer(&mut bytes, 0);
    pointer(&mut bytes, 1);
    real(&mut bytes, 0.0);
    for _ in 0..3 {
        pointer(&mut bytes, 1);
    }
    pointer(&mut bytes, 3);
    pointer(&mut bytes, 5);
    bytes.push(b'+');
    for _ in 0..5 {
        pointer(&mut bytes, 1);
    }
    node(&mut bytes, 53, 5);
    integer(&mut bytes, 0);
    pointer(&mut bytes, 1);
    pointer(&mut bytes, 2);
    for _ in 0..3 {
        pointer(&mut bytes, 1);
    }
    bytes.push(b'+');
    for value in [0.0, 0.0, 0.0, 0.007, 0.0, 0.0, 1.0, 1.0, 0.0, 0.0] {
        real(&mut bytes, value);
    }
    bytes.extend_from_slice(&[0, 1, 0, 1]);
    bytes
}

/// One solid body holding two solid regions and the void region, each solid
/// region owning one shell with one face on the shared sphere surface.
pub(crate) fn two_solid_regions_partition() -> Vec<u8> {
    shell_regions_partition(b'S', false)
}

/// One solid body holding a solid region with one face and a void region with
/// one cavity shell and one face, besides the exterior void region that owns
/// no shell.
pub(crate) fn solid_with_cavity_partition() -> Vec<u8> {
    shell_regions_partition(b'V', false)
}

/// The two solid regions of [`two_solid_regions_partition`], with a shell that
/// holds no face under the exterior void region.
pub(crate) fn solid_regions_with_void_shell_partition() -> Vec<u8> {
    shell_regions_partition(b'S', true)
}

/// A body with a region of type `S` owning shell 3 and a region of type
/// `second` owning shell 6, each shell with one face, then the exterior void
/// region, which owns shell 11 without a face when `exterior_shell` is set.
fn shell_regions_partition(second: u8, exterior_shell: bool) -> Vec<u8> {
    let sphere = sphere_partition();
    let records = cadmpeg_parasolid::records::PrimaryRecords::read(&sphere)
        .unwrap()
        .unwrap();
    let header_len = records.nodes().next().unwrap().range().start;
    let mut bytes = sphere[..header_len].to_vec();
    node(&mut bytes, 12, 2);
    integer(&mut bytes, 0);
    pointers(&mut bytes, &[1; 6]);
    real(&mut bytes, 1.0e-8);
    real(&mut bytes, 1.0e-8);
    pointers(&mut bytes, &[1; 3]);
    bytes.push(0);
    pointer(&mut bytes, 1);
    bytes.extend_from_slice(&[1, 0]);
    pointer(&mut bytes, 3);
    pointers(&mut bytes, &[1, 1, 1, 8, 1, 1]);
    for (shell, face, region) in [(3u16, 4u16, 8u16), (6, 7, 9)] {
        node(&mut bytes, 13, shell);
        integer(&mut bytes, 0);
        pointers(&mut bytes, &[1, 2, 1, face, 1, 1, region, 1]);
        node(&mut bytes, 14, face);
        integer(&mut bytes, 0);
        pointer(&mut bytes, 1);
        real(&mut bytes, 0.0);
        pointers(&mut bytes, &[1, 1, 1, shell, 5]);
        bytes.push(b'+');
        pointers(&mut bytes, &[1; 5]);
    }
    if exterior_shell {
        node(&mut bytes, 13, 11);
        integer(&mut bytes, 0);
        pointers(&mut bytes, &[1, 2, 1, 1, 1, 1, 10, 1]);
    }
    node(&mut bytes, 53, 5);
    integer(&mut bytes, 0);
    pointers(&mut bytes, &[1, 2, 1, 1, 1]);
    bytes.push(b'+');
    for value in [0.0, 0.0, 0.0, 0.007, 0.0, 0.0, 1.0, 1.0, 0.0, 0.0] {
        real(&mut bytes, value);
    }
    for (region, next, previous, shell, kind) in [
        (8u16, 9u16, 1u16, 3u16, b'S'),
        (9, 10, 8, 6, second),
        (10, 1, 9, if exterior_shell { 11 } else { 1 }, b'V'),
    ] {
        node(&mut bytes, 19, region);
        integer(&mut bytes, 0);
        pointers(&mut bytes, &[1, 2, next, previous, shell]);
        bytes.push(kind);
    }
    bytes.extend_from_slice(&[0, 1, 0, 1]);
    bytes
}

fn node(bytes: &mut Vec<u8>, kind: u16, index: u16) {
    bytes.extend_from_slice(&kind.to_be_bytes());
    pointer(bytes, index);
}
fn pointer(bytes: &mut Vec<u8>, value: u16) {
    bytes.extend_from_slice(&value.to_be_bytes());
}
fn integer(bytes: &mut Vec<u8>, value: u32) {
    bytes.extend_from_slice(&value.to_be_bytes());
}
fn real(bytes: &mut Vec<u8>, value: f64) {
    bytes.extend_from_slice(&value.to_be_bytes());
}

pub(crate) fn cube_partition() -> Vec<u8> {
    let sphere = sphere_partition();
    let records = cadmpeg_parasolid::records::PrimaryRecords::read(&sphere)
        .unwrap()
        .unwrap();
    let header_len = records.nodes().next().unwrap().range().start;
    let mut bytes = sphere[..header_len].to_vec();
    let coordinates = [
        [0.0, 0.0, 0.0],
        [0.002, 0.0, 0.0],
        [0.002, 0.002, 0.0],
        [0.0, 0.002, 0.0],
        [0.0, 0.0, 0.002],
        [0.002, 0.0, 0.002],
        [0.002, 0.002, 0.002],
        [0.0, 0.002, 0.002],
    ];
    let rings = [
        [0usize, 3, 2, 1],
        [4, 5, 6, 7],
        [0, 1, 5, 4],
        [1, 2, 6, 5],
        [2, 3, 7, 6],
        [3, 0, 4, 7],
    ];
    let edge_ends = [
        [0usize, 1],
        [1, 2],
        [2, 3],
        [3, 0],
        [4, 5],
        [5, 6],
        [6, 7],
        [7, 4],
        [0, 4],
        [1, 5],
        [2, 6],
        [3, 7],
    ];
    let normals = [
        [0.0, 0.0, -1.0],
        [0.0, 0.0, 1.0],
        [0.0, -1.0, 0.0],
        [1.0, 0.0, 0.0],
        [0.0, 1.0, 0.0],
        [-1.0, 0.0, 0.0],
    ];
    let x_axes = [
        [1.0, 0.0, 0.0],
        [1.0, 0.0, 0.0],
        [1.0, 0.0, 0.0],
        [0.0, 1.0, 0.0],
        [1.0, 0.0, 0.0],
        [0.0, 1.0, 0.0],
    ];
    let mut incidences = Vec::new();
    for (face, ring) in rings.iter().enumerate() {
        for side in 0..4 {
            let ends = [ring[side], ring[(side + 1) % 4]];
            let edge = edge_ends
                .iter()
                .position(|pair| *pair == ends || *pair == [ends[1], ends[0]])
                .unwrap();
            incidences.push((face, side, edge, ends, edge_ends[edge] == ends));
        }
    }
    node(&mut bytes, 12, 2);
    integer(&mut bytes, 0);
    pointers(&mut bytes, &[1; 6]);
    real(&mut bytes, 1.0e-8);
    real(&mut bytes, 1.0e-8);
    pointers(&mut bytes, &[1; 3]);
    bytes.push(0);
    pointer(&mut bytes, 1);
    bytes.extend_from_slice(&[1, 0]);
    pointer(&mut bytes, 3);
    pointers(&mut bytes, &[1; 6]);
    node(&mut bytes, 13, 3);
    integer(&mut bytes, 0);
    pointers(&mut bytes, &[1, 2, 1, 4, 1, 1, 1, 1]);
    for face in 0..6 {
        node(&mut bytes, 14, wire_id(4, face));
        integer(&mut bytes, 0);
        pointer(&mut bytes, 1);
        real(&mut bytes, 0.0);
        pointers(
            &mut bytes,
            &[
                if face == 5 { 1 } else { wire_id(5, face) },
                if face == 0 { 1 } else { wire_id(3, face) },
                wire_id(16, face),
                3,
                wire_id(10, face),
            ],
        );
        bytes.push(b'+');
        pointers(&mut bytes, &[1; 5]);
        node(&mut bytes, 50, wire_id(10, face));
        integer(&mut bytes, 0);
        pointers(&mut bytes, &[1, 2, 1, 1, 1]);
        bytes.push(b'+');
        vector(&mut bytes, coordinates[rings[face][0]]);
        vector(&mut bytes, normals[face]);
        vector(&mut bytes, x_axes[face]);
        node(&mut bytes, 15, wire_id(16, face));
        integer(&mut bytes, 0);
        pointers(&mut bytes, &[1, wire_id(62, face * 4), wire_id(4, face), 1]);
    }
    for (vertex, position) in coordinates.iter().enumerate() {
        let point = 37 - u16::try_from(vertex).unwrap();
        node(&mut bytes, 18, wire_id(22, vertex));
        integer(&mut bytes, 0);
        pointers(&mut bytes, &[1, 1, 1, 1, point]);
        real(&mut bytes, 0.0);
        pointer(&mut bytes, 2);
        node(&mut bytes, 29, point);
        integer(&mut bytes, 0);
        pointers(&mut bytes, &[1, 2, 1, 1]);
        vector(&mut bytes, *position);
    }
    for (edge, [start, end]) in edge_ends.iter().copied().enumerate() {
        let fin = incidences
            .iter()
            .position(|incidence| incidence.2 == edge)
            .unwrap();
        node(&mut bytes, 16, wire_id(38, edge));
        integer(&mut bytes, 0);
        pointer(&mut bytes, 1);
        real(&mut bytes, 0.0);
        pointers(
            &mut bytes,
            &[wire_id(62, fin), 1, 1, wire_id(50, edge), 1, 1, 2],
        );
        node(&mut bytes, 30, wire_id(50, edge));
        integer(&mut bytes, 0);
        pointers(&mut bytes, &[1, 2, 1, 1, 1]);
        bytes.push(b'+');
        vector(&mut bytes, coordinates[start]);
        vector(
            &mut bytes,
            std::array::from_fn(|axis| (coordinates[end][axis] - coordinates[start][axis]) / 0.002),
        );
    }
    for (fin, (face, side, edge, ends, forward)) in incidences.iter().copied().enumerate() {
        let other = incidences
            .iter()
            .enumerate()
            .position(|(index, incidence)| index != fin && incidence.2 == edge)
            .unwrap();
        node(&mut bytes, 17, wire_id(62, fin));
        pointers(
            &mut bytes,
            &[
                1,
                wire_id(16, face),
                wire_id(62, face * 4 + (side + 1) % 4),
                wire_id(62, face * 4 + (side + 3) % 4),
                wire_id(22, ends[1]),
                wire_id(62, other),
                wire_id(38, edge),
                1,
                1,
            ],
        );
        bytes.push(if forward { b'+' } else { b'-' });
    }
    bytes.extend_from_slice(&[0, 1, 0, 1]);
    bytes
}

/// A planar disc face of radius 5 mm bounded by one ring edge on a circle, with
/// no vertex: the single fin is its own successor and predecessor.
pub(crate) fn disc_partition() -> Vec<u8> {
    ring_face_partition(None)
}

/// The ring face of `disc_partition` on a `SWEPT_SURF` whose section is the
/// ring's circle, swept along +Z with the stated scale.
pub(crate) fn swept_ring_partition(scale: f64) -> Vec<u8> {
    ring_face_partition(Some(scale))
}

/// The ring face of `disc_partition` whose edge names no curve and whose fin
/// names an `SP_CURVE` (type 137, node 10) on no surface: the edge has
/// neither a vertex nor a carrier.
pub(crate) fn uncarried_ring_partition() -> Vec<u8> {
    uncarried(disc_partition())
}

/// The ring face of `swept_ring_partition` with the edge and fin of
/// `uncarried_ring_partition`.
pub(crate) fn uncarried_swept_ring_partition(scale: f64) -> Vec<u8> {
    uncarried(swept_ring_partition(scale))
}

fn uncarried(mut bytes: Vec<u8>) -> Vec<u8> {
    let records = cadmpeg_parasolid::records::PrimaryRecords::read(&bytes)
        .unwrap()
        .unwrap();
    let curve = |kind: u16| {
        records
            .nodes()
            .find(|node| node.kind() == kind)
            .unwrap()
            .field_range("curve")
            .unwrap()
    };
    let (edge, fin) = (curve(16), curve(17));
    bytes[edge].copy_from_slice(&1u16.to_be_bytes());
    bytes[fin].copy_from_slice(&10u16.to_be_bytes());
    bytes.truncate(bytes.len() - 4);
    node(&mut bytes, 137, 10);
    integer(&mut bytes, 0);
    pointers(&mut bytes, &[1; 5]);
    bytes.push(b'+');
    pointers(&mut bytes, &[1; 3]);
    real(&mut bytes, 0.0);
    bytes.extend_from_slice(&[0, 1, 0, 1]);
    bytes
}

fn ring_face_partition(swept: Option<f64>) -> Vec<u8> {
    let sphere = sphere_partition();
    let records = cadmpeg_parasolid::records::PrimaryRecords::read(&sphere)
        .unwrap()
        .unwrap();
    let header_len = records.nodes().next().unwrap().range().start;
    let mut bytes = sphere[..header_len].to_vec();
    node(&mut bytes, 12, 2);
    integer(&mut bytes, 0);
    pointers(&mut bytes, &[1; 6]);
    real(&mut bytes, 1.0e-8);
    real(&mut bytes, 1.0e-8);
    pointers(&mut bytes, &[1; 3]);
    bytes.push(0);
    pointer(&mut bytes, 1);
    bytes.extend_from_slice(&[1, 0]);
    pointer(&mut bytes, 3);
    pointers(&mut bytes, &[1; 6]);
    node(&mut bytes, 13, 3);
    integer(&mut bytes, 0);
    pointers(&mut bytes, &[1, 2, 1, 4, 1, 1, 1, 1]);
    node(&mut bytes, 14, 4);
    integer(&mut bytes, 0);
    pointer(&mut bytes, 1);
    real(&mut bytes, 0.0);
    pointers(&mut bytes, &[1, 1, 6, 3, 5]);
    bytes.push(b'+');
    pointers(&mut bytes, &[1; 5]);
    if let Some(scale) = swept {
        node(&mut bytes, 67, 5);
        integer(&mut bytes, 0);
        pointers(&mut bytes, &[1, 2, 1, 1, 1]);
        bytes.push(b'+');
        pointer(&mut bytes, 9);
        vector(&mut bytes, [0.0, 0.0, 1.0]);
        real(&mut bytes, scale);
    } else {
        node(&mut bytes, 50, 5);
        integer(&mut bytes, 0);
        pointers(&mut bytes, &[1, 2, 1, 1, 1]);
        bytes.push(b'+');
        vector(&mut bytes, [0.0, 0.0, 0.0]);
        vector(&mut bytes, [0.0, 0.0, 1.0]);
        vector(&mut bytes, [1.0, 0.0, 0.0]);
    }
    node(&mut bytes, 15, 6);
    integer(&mut bytes, 0);
    pointers(&mut bytes, &[1, 7, 4, 1]);
    node(&mut bytes, 16, 8);
    integer(&mut bytes, 0);
    pointer(&mut bytes, 1);
    real(&mut bytes, 0.0);
    pointers(&mut bytes, &[7, 1, 1, 9, 1, 1, 2]);
    node(&mut bytes, 31, 9);
    integer(&mut bytes, 0);
    pointers(&mut bytes, &[1, 2, 1, 1, 1]);
    bytes.push(b'+');
    vector(&mut bytes, [0.0, 0.0, 0.0]);
    vector(&mut bytes, [0.0, 0.0, 1.0]);
    vector(&mut bytes, [1.0, 0.0, 0.0]);
    real(&mut bytes, 0.005);
    node(&mut bytes, 17, 7);
    pointers(&mut bytes, &[1, 6, 7, 7, 1, 1, 8, 1, 1]);
    bytes.push(b'+');
    bytes.extend_from_slice(&[0, 1, 0, 1]);
    bytes
}

fn wire_id(base: u16, index: usize) -> u16 {
    base + u16::try_from(index).unwrap()
}
fn pointers(bytes: &mut Vec<u8>, values: &[u16]) {
    for value in values {
        pointer(bytes, *value);
    }
}
fn vector(bytes: &mut Vec<u8>, values: [f64; 3]) {
    for value in values {
        real(bytes, value);
    }
}

pub(crate) fn chart_partition(scale: f64) -> Vec<u8> {
    let sphere = sphere_partition();
    let records = cadmpeg_parasolid::records::PrimaryRecords::read(&sphere)
        .unwrap()
        .unwrap();
    let start = records.nodes().next().unwrap().range().start;
    let mut bytes = sphere[..start].to_vec();
    node(&mut bytes, 38, 2);
    integer(&mut bytes, 0);
    for _ in 0..5 {
        pointer(&mut bytes, 1);
    }
    bytes.push(b'+');
    for reference in [3, 4, 5, 6, 7] {
        pointer(&mut bytes, reference);
    }
    let points = [
        [0.0, 0.0, 0.0],
        [1.0, 0.0, 0.0],
        [1.0, 1.0, 0.0],
        [0.0, 1.0, 0.0],
        [0.0, 0.0, 0.0],
    ];
    bytes.extend_from_slice(&40u16.to_be_bytes());
    integer(&mut bytes, u32::try_from(points.len()).unwrap());
    pointer(&mut bytes, 5);
    real(&mut bytes, 0.0);
    real(&mut bytes, scale);
    integer(&mut bytes, u32::try_from(points.len()).unwrap());
    real(&mut bytes, 1.0e-6);
    real(&mut bytes, 0.0);
    real(&mut bytes, -3.14158e13);
    real(&mut bytes, -3.14158e13);
    for value in points.into_iter().flatten() {
        real(&mut bytes, value);
    }
    for index in [6, 7] {
        bytes.extend_from_slice(&41u16.to_be_bytes());
        integer(&mut bytes, 1);
        pointer(&mut bytes, index);
        bytes.push(b'H');
        for _ in 0..3 {
            real(&mut bytes, 0.0);
        }
    }
    bytes.extend_from_slice(&[0, 1, 0, 1]);
    bytes
}

/// The chart partition plus trimmed curves: node 8 restricts the
/// intersection, node 9 restricts node 8, node 10 restricts itself and node 11
/// restricts a node that is absent.
pub(crate) fn trimmed_chart_partition() -> Vec<u8> {
    let mut bytes = chart_partition(1.0);
    bytes.truncate(bytes.len() - 4);
    for (index, basis) in [(8, 2), (9, 8), (10, 10), (11, 99)] {
        node(&mut bytes, 133, index);
        integer(&mut bytes, 0);
        for _ in 0..5 {
            pointer(&mut bytes, 1);
        }
        bytes.push(b'+');
        pointer(&mut bytes, basis);
        for _ in 0..6 {
            real(&mut bytes, 0.0);
        }
        real(&mut bytes, 0.0);
        real(&mut bytes, 1.0);
    }
    bytes.extend_from_slice(&[0, 1, 0, 1]);
    bytes
}
