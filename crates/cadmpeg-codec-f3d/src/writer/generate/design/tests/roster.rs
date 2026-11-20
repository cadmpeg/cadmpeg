// SPDX-License-Identifier: Apache-2.0
//! The record roster of a written Base Feature design (f3d.md §3.1,
//! "Records of a Base Feature design"), read back from the archive's
//! segments.

use std::collections::BTreeSet;
use std::io::{Cursor, Read};

use super::{scoped_cube, u32_at, u64_at, write_bytes};

/// One segment's `MetaStream` fields and its `BulkStream`.
struct Segment {
    short_name: String,
    id: u32,
    words: [u32; 4],
    asset: String,
    full_name: String,
    add_in: String,
    code: (u32, u32),
    /// `(type GUID, base GUID, version, add-in, entity IDs)` in table order.
    types: Vec<(String, String, u32, String, Vec<u64>)>,
    named: Vec<u64>,
    primary: Vec<(u64, u64)>,
    secondary: Vec<(u64, u64)>,
    next: u64,
    bulk: Vec<u8>,
}

struct Reader<'a> {
    bytes: &'a [u8],
    at: usize,
}

impl Reader<'_> {
    fn take(&mut self, count: usize) -> &[u8] {
        let slice = &self.bytes[self.at..self.at + count];
        self.at += count;
        slice
    }
    fn u32(&mut self) -> u32 {
        u32::from_le_bytes(self.take(4).try_into().expect("four bytes"))
    }
    fn u64(&mut self) -> u64 {
        u64::from_le_bytes(self.take(8).try_into().expect("eight bytes"))
    }
    fn ascii(&mut self) -> String {
        let length = usize::try_from(self.u32()).expect("length");
        String::from_utf8(self.take(length).to_vec()).expect("ASCII")
    }
    fn utf16(&mut self) -> String {
        let length = usize::try_from(self.u32()).expect("length");
        let units = self
            .take(2 * length)
            .chunks(2)
            .map(|pair| u16::from_le_bytes([pair[0], pair[1]]))
            .collect::<Vec<_>>();
        String::from_utf16(&units).expect("UTF-16")
    }
    fn pairs(&mut self) -> Vec<(u64, u64)> {
        let count = self.u32();
        (0..count).map(|_| (self.u64(), self.u64())).collect()
    }
}

fn member(archive: &[u8], name: &str) -> Vec<u8> {
    let mut zip = zip::ZipArchive::new(Cursor::new(archive)).expect("a ZIP archive");
    let mut bytes = Vec::new();
    zip.by_name(&format!(
        "{}/{name}",
        crate::manifest::GENERATED_DESIGN_ASSET_FOLDER
    ))
    .unwrap_or_else(|_| panic!("the member {name}"))
    .read_to_end(&mut bytes)
    .expect("the member reads");
    bytes
}

fn segment(archive: &[u8], folder: &str) -> Segment {
    let meta = member(archive, &format!("{folder}/MetaStream.dat"));
    let mut reader = Reader {
        bytes: &meta,
        at: 0,
    };
    let short_name = reader.ascii();
    let id = reader.u32();
    let asset = reader.utf16();
    let words = [reader.u32(), reader.u32(), reader.u32(), reader.u32()];
    let full_name = reader.ascii();
    let add_in = reader.ascii();
    let code = (reader.u32(), reader.u32());
    let types = (0..reader.u32())
        .map(|_| {
            let guid = reader.ascii();
            let base = reader.ascii();
            let version = reader.u32();
            let add_in = reader.ascii();
            let ids = (0..reader.u32()).map(|_| reader.u64()).collect();
            (guid, base, version, add_in, ids)
        })
        .collect();
    let named = (0..reader.u32()).map(|_| reader.u64()).collect();
    let primary = reader.pairs();
    let secondary = reader.pairs();
    let next = reader.u64();
    Segment {
        short_name,
        id,
        words,
        asset,
        full_name,
        add_in,
        code,
        types,
        named,
        primary,
        secondary,
        next,
        bulk: member(archive, &format!("{folder}/BulkStream.dat")),
    }
}

impl Segment {
    /// The bytes of record `index`, bounded by the next primary entry.
    fn record(&self, index: u64) -> &[u8] {
        let position = self
            .primary
            .iter()
            .position(|(id, _)| *id == index)
            .unwrap_or_else(|| panic!("record {index}"));
        let start = usize::try_from(self.primary[position].1).expect("offset");
        let end = self
            .primary
            .get(position + 1)
            .map_or(self.bulk.len(), |(_, next)| {
                usize::try_from(*next).expect("offset")
            });
        &self.bulk[start..end]
    }

    /// The type GUID and version of record `index`.
    fn type_of(&self, index: u64) -> (&str, u32) {
        self.types
            .iter()
            .find(|row| row.4.contains(&index))
            .map_or_else(
                || panic!("record {index} has a type"),
                |row| (row.0.as_str(), row.2),
            )
    }
}

fn design() -> (Segment, Vec<u8>) {
    let (_, _, bytes) = write_bytes(&scoped_cube());
    (segment(&bytes, "FusionDesignSegmentType1"), bytes)
}

fn f64_at(bytes: &[u8], at: usize) -> f64 {
    f64::from_le_bytes(bytes[at..at + 8].try_into().expect("eight bytes"))
}

/// The written user parameters: the count of the parameter collection.
fn parameters(design: &Segment) -> u64 {
    u64::from(u32_at(design.record(17), 46))
}

#[test]
fn the_one_body_roster_is_the_stated_roster() {
    let (design, _) = design();
    let parameters = parameters(&design);
    let unused = [8, 9, 35, 179, 180, 181, 182, 183, 216, 217, 218];
    let expected = (0..=224)
        .filter(|index| !unused.contains(index))
        .chain(225..225 + parameters)
        .collect::<Vec<_>>();
    let written = design.primary.iter().map(|(id, _)| *id).collect::<Vec<_>>();
    assert_eq!(written, expected);
    assert!(design.primary.windows(2).all(|pair| pair[0].1 < pair[1].1));
    for (index, guid, version) in [
        (0, "23FD63FB-4EBE-4c85-A42B-5D9809AA3499", 17),
        (32, "64435F6E-077A-4f6b-A7B6-537BF84BA414", 0),
        (47, "21086CE0-C398-4b1b-9CD6-86F092429833", 2),
        (172, "d9554b4f-2333-42c2-9495-2148d96f7437", 11),
        (188, "702b9cd2-537c-429e-8cc4-22beeeb98c37", 1),
        (195, "40DCC3F41D-E94F-B3B1-3E95-9B6ABE572C", 1),
        (209, "D7015B81-AB4D-4C89-BED7-AA4411E1964A", 4),
        (224, "D26351F0-5940-4D23-AA20-2C35475A6D9E", 2),
    ] {
        assert_eq!(design.type_of(index), (guid, version), "record {index}");
    }
    let stated = [
        (0, 494),
        (1, 334),
        (2, 22),
        (4, 77),
        (5, 36),
        (6, 52),
        (7, 103),
        (10, 115),
        (11, 70),
        (12, 42),
        (13, 46),
        (14, 41),
        (15, 45),
        (18, 44),
        (19, 42),
        (23, 59),
        (24, 115),
        (25, 165),
        (26, 205),
        (29, 252),
        (32, 55),
        (34, 80),
        (36, 60),
        (37, 42),
        (38, 62),
        (39, 60),
        (40, 61),
        (41, 72),
        (42, 67),
        (43, 252),
        (44, 85),
        (45, 114),
        (46, 103),
        (47, 356),
        (58, 95),
        (161, 48),
        (162, 51),
        (163, 70),
        (164, 153),
        (165, 54),
        (171, 56),
        (172, 261),
        (173, 25),
        (177, 99),
        (178, 57),
        (184, 38),
        (185, 202),
        (186, 207),
        (187, 891),
        (188, 133),
        (189, 95),
        (190, 144),
        (191, 72),
        (193, 93),
        (194, 60),
        (195, 158),
        (196, 39),
        (197, 149),
        (198, 203),
        (199, 88),
        (200, 70),
        (201, 161),
        (202, 40),
        (203, 80),
        (204, 36),
        (205, 79),
        (206, 58),
        (207, 47),
        (208, 72),
        (209, 353),
        (210, 96),
        (211, 33),
        (212, 72),
        (213, 120),
        (214, 58),
        (215, 891),
        (223, 197),
        (224, 151),
    ];
    for (index, length) in stated {
        assert_eq!(design.record(index).len(), length, "record {index}");
    }
    let length = |index: u64| u64::try_from(design.record(index).len()).expect("length");
    let name = u64::from(u32_at(design.record(3), 888));
    assert_eq!(length(3), 904 + 2 * name);
    assert_eq!(length(17), 61 + 11 * parameters);
    let entities = u64::from(u32_at(design.record(219), 45));
    assert_eq!(entities, 6 + 12, "the cube's faces and edges");
    for companion in [192, 219] {
        assert_eq!(length(companion), 49 + 8 * entities);
    }
}

#[test]
fn the_design_metastream_carries_the_stated_header_and_indexes() {
    let (design, _) = design();
    assert_eq!(design.short_name, "FusionDesignSegmentType");
    assert_eq!(design.full_name, "FusionDesignSegmentType");
    assert_eq!((design.id, design.code), (0, (1, 0)));
    assert_eq!(design.words, [1234, 8, 36, 708_837_440]);
    assert_eq!(design.asset, "00000000-0000-0000-0000-000000000000");
    assert_eq!(design.add_in, "Fusion");
    assert_eq!(
        design.named,
        [0, 1, 4, 43, 44, 45, 46, 161, 177, 184, 185, 186, 191]
    );
    let highest = design
        .primary
        .iter()
        .map(|(id, _)| *id)
        .max()
        .expect("records");
    assert_eq!(design.next, highest + 1);
    let paired = design
        .secondary
        .iter()
        .map(|(id, _)| *id)
        .collect::<BTreeSet<_>>();
    assert_eq!(
        paired,
        BTreeSet::from([
            3, 23, 32, 33, 34, 36, 38, 39, 40, 41, 42, 45, 162, 177, 178, 194, 198, 199, 205, 209,
            212, 213, 223, 224
        ])
    );
    for (record, offset) in &design.secondary {
        let (_, start) = design
            .primary
            .iter()
            .find(|(id, _)| id == record)
            .expect("a primary entry");
        let inside = usize::try_from(offset - start).expect("offset");
        let frame = design.record(*record);
        assert!(inside > 0 && inside + 11 <= frame.len());
        assert_eq!(u32_at(frame, inside), 3);
        assert_eq!(u64::from(u32_at(frame, inside + 7)), *record);
    }
}

#[test]
fn the_type_table_lists_the_empty_rows_with_their_spellings() {
    let (design, _) = design();
    let empty = design
        .types
        .iter()
        .filter(|row| row.4.is_empty())
        .map(|row| row.0.as_str())
        .collect::<Vec<_>>();
    assert_eq!(empty.len(), 49, "{empty:?}");
    for guid in [
        "03bc52c5-f13f-4f61-9e9f-e5c1e876c3ef",
        "1666C0B1-46B3-45b7-8AA1-C618F7CFA703",
        "A7AEA631-985B-4DD1-8CE2-DE2C-14B54081",
        "CB844AB6-240D-4fc9-9C9F-3679DC896D6F",
    ] {
        assert!(empty.contains(&guid), "{guid}");
    }
    let guids = design
        .types
        .iter()
        .map(|row| &row.0)
        .collect::<BTreeSet<_>>();
    assert_eq!(guids.len(), design.types.len(), "one row per type");
}

#[test]
fn timestamps_epochs_and_duration_are_the_stated_values() {
    let (design, _) = design();
    let stamp = |record: u64, at: usize| u64_at(design.record(record), at);
    // The component table: `00 00`, no entry, owner `t_c`.
    let component = design.record(206);
    assert_eq!(
        (&component[25..27], u32_at(component, 27)),
        (&[0, 0][..], 0)
    );
    let t_c = stamp(206, 31 + 11);
    // The scope table: `01 01`, the component record and the scope.
    let scope = design.record(210);
    assert_eq!((&scope[25..27], u32_at(scope, 27)), (&[1, 1][..], 2));
    assert_eq!((stamp(210, 42), stamp(210, 61)), (t_c, stamp(210, 80)));
    let t_s = stamp(210, 61);
    let result = design.record(214);
    assert_eq!(&result[25..27], [0, 0]);
    let t_r = stamp(214, 42);
    assert!(t_c < t_s && t_s < t_r, "{t_c} {t_s} {t_r}");
    let tail = &design.record(209)[193 + 52..271 + 52];
    assert_eq!((u32_at(tail, 35), u32_at(tail, 47)), (250, 250));
    assert!((f64_at(tail, 39) - 0.000_631).abs() < 1e-12);
}

#[test]
fn the_home_camera_fits_the_body_box() {
    let (design, _) = design();
    let home = design.record(170);
    let point = |at: usize| [0, 1, 2].map(|axis| f64_at(home, at + 8 * axis));
    let (eye, target) = (point(77), point(105));
    for axis in 0..3 {
        assert!((target[axis] - 0.5).abs() < 1e-9, "{target:?}");
    }
    let extents = [f64_at(home, 43), f64_at(home, 51)];
    let distance = 2.5 * extents[0].max(extents[1]);
    let offset = [0, 1, 2].map(|axis| eye[axis] - target[axis]);
    let expected = [1.0, -1.0, 1.0].map(|sign| sign * distance / 3f64.sqrt());
    for axis in 0..3 {
        assert!((offset[axis] - expected[axis]).abs() < 1e-9, "{offset:?}");
    }
    let radius = f64_at(design.record(0), 426);
    assert!(radius >= 3f64.sqrt() / 2.0, "{radius}");
    let scene = [0, 1, 2, 3, 4, 5].map(|value| f64_at(design.record(189), 46 + 8 * value));
    for (value, expected) in scene.into_iter().zip([1.0, 1.0, 1.0, 0.0, 0.0, 0.0]) {
        assert!((value - expected).abs() < 1e-9, "{scene:?}");
    }
}

#[test]
fn the_body_maps_name_the_history_stream() {
    let (design, archive) = design();
    let map = design.record(197);
    assert_eq!(
        (u32_at(map, 21), u64_at(map, 25), u64_at(map, 33)),
        (1, 0, 187)
    );
    let mut reader = Reader { bytes: map, at: 53 };
    let basename = reader.utf16();
    let uuid = basename
        .strip_prefix("BREP.")
        .and_then(|rest| rest.strip_suffix(".smbh"))
        .expect("a BREP history basename");
    assert_eq!(uuid.len(), 36);
    let stream = member(&archive, &format!("Breps.BlobParts/{basename}"));
    assert_eq!(u64_at(&stream, 39), 3);
    let at = 47
        + [0, 1, 2].iter().fold(0, |at, _| {
            let start = 47 + at;
            at + 2 + usize::from(stream[start + 1])
        });
    assert_eq!(stream[at], 0x06);
    assert_eq!(f64_at(&stream, at + 1), 40.0);
}

#[test]
fn two_writes_are_identical() {
    let (_, _, first) = write_bytes(&scoped_cube());
    let (_, _, second) = write_bytes(&scoped_cube());
    assert!(first == second, "the archive is a function of the document");
}
