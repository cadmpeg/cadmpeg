// SPDX-License-Identifier: Apache-2.0
//! The record roster of a written Base Feature design (f3d.md §3.1,
//! "Records of a Base Feature design"), read back from the archive's
//! segments.

use std::collections::BTreeSet;
use std::io::{Cursor, Read};

use super::{loss, scoped_cube, u32_at, u64_at, write_bytes};

/// One segment's `MetaStream` fields and its `BulkStream`.
pub(super) struct Segment {
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
    pub(super) primary: Vec<(u64, u64)>,
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

pub(super) fn segment(archive: &[u8], folder: &str) -> Segment {
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
    pub(super) fn record(&self, index: u64) -> &[u8] {
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
    pub(super) fn type_of(&self, index: u64) -> (&str, u32) {
        self.types
            .iter()
            .find(|row| row.4.contains(&index))
            .map_or_else(
                || panic!("record {index} has a type"),
                |row| (row.0.as_str(), row.2),
            )
    }

    /// The record indices of the type `guid`, each with the type's version.
    pub(super) fn records_of(&self, guid: &str) -> Vec<(u64, u32)> {
        self.types
            .iter()
            .filter(|row| row.0.eq_ignore_ascii_case(guid))
            .flat_map(|row| row.4.iter().map(|index| (*index, row.2)))
            .collect()
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
fn timestamps_epochs_and_duration_are_the_written_values() {
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
    let (report, _, _) = write_bytes(&scoped_cube());
    let stamp_notes = loss(&report, "export/record.residual-unavailable")
        .into_iter()
        .filter(|message| message.contains("fixed-clock"))
        .collect::<Vec<_>>();
    assert_eq!(stamp_notes.len(), 1, "{:#?}", report.losses);
    for rec in ["record 206", "record 210", "record 214"] {
        assert!(stamp_notes[0].contains(rec), "{rec}: {}", stamp_notes[0]);
    }
}

#[test]
fn an_unmodeled_or_unset_length_unit_falls_back_to_mm_with_a_loss() {
    use cadmpeg_ir::design::units::{UnitSymbol, UnitSystem};
    use cadmpeg_ir::products::{ExternalDocument, ProductDefinition};
    for (units, label) in [
        (None, "no declared length unit"),
        (
            Some(UnitSystem {
                length: UnitSymbol::Km,
                angle: UnitSymbol::Deg,
                mass: None,
                time: None,
            }),
            "length unit Km",
        ),
    ] {
        let mut ir = scoped_cube();
        // The root definition the design scope names, so that the writer
        // reads its units.
        let mut definition = ProductDefinition::root(super::root(), ExternalDocument::default());
        definition.bodies = ir
            .geometry
            .bodies
            .iter()
            .map(|body| body.id.clone())
            .collect();
        definition.units = units;
        ir.model.product_definitions.push(definition);
        let (report, _, _) = write_bytes(&ir);
        assert!(
            loss(&report, "export/record.residual-unavailable")
                .iter()
                .any(|message| message.contains("MmMKS")
                    && message.contains(label)
                    && message.contains("record 0")),
            "{label}: {:#?}",
            report.losses
        );
    }
}

#[test]
fn the_item_tail_epoch_and_duration_are_named() {
    let (report, _, _) = write_bytes(&scoped_cube());
    let notes = loss(&report, "export/record.residual-unavailable")
        .into_iter()
        .filter(|message| message.contains("item tail"))
        .collect::<Vec<_>>();
    let [note] = notes.as_slice() else {
        panic!("one item-tail note: {:#?}", report.losses);
    };
    for named in [
        "epoch 250 in both epoch fields",
        "states 252 for the generation words 1234, 8, 36",
        "duration 0.000631",
        "F3D-EPOCH",
        "DR-913",
        "(1): record 209 (Base Feature scope: its item tail)",
    ] {
        assert!(note.contains(named), "{named}: {note}");
    }
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

/// The `(target, segment)` of the cross-segment reference at `at`.
fn cross(record: &[u8], at: usize) -> (u64, u32) {
    assert_eq!((record[at], record[at + 9], record[at + 10]), (1, 0, 1));
    (u64_at(record, at + 1), u32_at(record, at + 11))
}

#[test]
fn the_act_segment_is_stated() {
    let (_, archive) = design();
    let act = segment(&archive, "FusionACTSegmentType1");
    assert_eq!(
        (act.short_name.as_str(), act.full_name.as_str()),
        ("FusionACTSegmentType", "FusionACTSegmentType")
    );
    assert_eq!(
        (act.id, act.code, act.words),
        (1, (5, 0), [1234, 8, 36, 708_837_440])
    );
    assert_eq!(act.named, [0, 1, 5, 6]);
    assert!(act.secondary.is_empty());
    assert_eq!(act.next, 12);
    let written = act.primary.iter().map(|(id, _)| *id).collect::<Vec<_>>();
    assert_eq!(written, [0, 1, 2, 3, 4, 5, 6, 7, 8, 10]);
    for (index, length) in [
        (0, 833),
        (1, 75),
        (3, 583),
        (4, 56),
        (5, 96),
        (6, 200),
        (7, 583),
        (8, 403),
        (10, 403),
    ] {
        assert_eq!(act.record(index).len(), length, "ACT record {index}");
    }
    let mut table = Reader {
        bytes: act.record(0),
        at: 29,
    };
    let entries = (0..table.u32())
        .map(|_| {
            let target = u64_at(table.take(11), 1);
            (target, table.utf16())
        })
        .collect::<Vec<_>>();
    assert_eq!(
        entries,
        [
            (8, "0_187".to_owned()),
            (10, "0_215".to_owned()),
            (3, "1_2".to_owned()),
            (7, "1_6".to_owned())
        ]
    );
    let aspects = |index: u64| {
        let mut reader = Reader {
            bytes: act.record(index),
            at: 21,
        };
        let pairs = (0..reader.u32())
            .map(|_| (reader.ascii(), reader.utf16()))
            .collect::<Vec<_>>();
        (pairs, reader.utf16())
    };
    let _manifest = table.utf16();
    let table_guid = table.utf16();
    table.u32();
    let table_aspects = (0..table.u32())
        .map(|_| (table.ascii(), table.utf16()))
        .collect::<Vec<_>>();
    assert_eq!(table_aspects[1], ("Geometry".to_owned(), table_guid));
    let (component, key) = aspects(3);
    assert_eq!(key, "1_2");
    assert_eq!(
        component[2].1, component[5].1,
        "PhysicalMaterial is Transform"
    );
    let (instance, key) = aspects(7);
    assert_eq!(key, "1_6");
    for (aspect, guid) in &instance {
        assert_eq!(aspect == "Properties", guid != &instance[0].1, "{aspect}");
    }
    let (design_aspects, key) = aspects(8);
    assert_eq!((design_aspects.len(), key.as_str()), (4, "0_187"));
    let (design, _) = design();
    let mut body = Reader {
        bytes: design.record(187),
        at: 196,
    };
    assert_eq!(body.utf16(), design_aspects[1].1, "the mass-block GUID");
    let name = |record: &[u8], at: usize| {
        let mut reader = Reader { bytes: record, at };
        reader.utf16()
    };
    assert_eq!(name(act.record(2), 57), name(design.record(3), 888));
    assert!(design_aspects
        .iter()
        .chain(&component)
        .chain(&instance)
        .all(|(_, guid)| guid.chars().all(|c| !c.is_ascii_uppercase())));
}

#[test]
fn the_browser_segment_is_stated() {
    let (_, archive) = design();
    let browser = segment(&archive, "FusionBrowserSegmentType1");
    assert_eq!((browser.id, browser.code), (2, (6, 0)));
    assert_eq!(browser.named, [0]);
    assert_eq!(browser.next, 31);
    let written = browser
        .primary
        .iter()
        .map(|(id, _)| *id)
        .collect::<Vec<_>>();
    assert_eq!(written, (0..31).collect::<Vec<_>>());
    for (index, length) in [
        (0, 1042),
        (1, 464),
        (2, 148),
        (10, 194),
        (16, 115),
        (29, 153),
    ] {
        assert_eq!(
            browser.record(index).len(),
            length,
            "Browser record {index}"
        );
    }
    let [(root, offset)] = browser.secondary.as_slice() else {
        panic!("one paired header: {:?}", browser.secondary);
    };
    assert_eq!((*root, *offset), (0, browser.primary[0].1 + 1002));
    assert_eq!(cross(browser.record(16), 24), (193, 0));
    assert_eq!(cross(browser.record(29), 24), (6, 0));
    let entries = &browser.record(0)[61..61 + 24 * 26];
    let shown = entries
        .chunks(26)
        .map(|pair| (cross(pair, 0).0, u64_at(pair, 16)))
        .collect::<BTreeSet<_>>();
    assert!(shown.contains(&(43, 1)) && shown.contains(&(2, 30)) && shown.len() == 24);
}

#[test]
fn every_cross_segment_reference_names_its_stated_target() {
    let (design, archive) = design();
    let act = segment(&archive, "FusionACTSegmentType1");
    let name = u64::from(u32_at(design.record(3), 888));
    let after_name = usize::try_from(892 + 2 * name).expect("offset");
    assert_eq!(cross(design.record(208), 38), (2, 1));
    assert_eq!(cross(design.record(3), 785), (2, 1));
    assert_eq!(cross(design.record(6), 21), (4, 1));
    assert_eq!(cross(design.record(43), 214), (6, 1));
    assert_eq!(cross(act.record(2), 42), (3, 0));
    assert_eq!(cross(act.record(4), 21), (6, 0));
    assert_eq!(cross(act.record(6), 168), (43, 0));
    assert_eq!(u64_at(design.record(3), after_name + 2), 43);
}

/// `name` with each lowercase 36-character GUID replaced by `<uuid>`.
fn masked(name: &str) -> String {
    let is_guid = |text: &str| {
        text.len() == 36
            && text.char_indices().all(|(at, character)| match at {
                8 | 13 | 18 | 23 => character == '-',
                _ => character.is_ascii_digit() || ('a'..='f').contains(&character),
            })
    };
    let mut out = String::new();
    let mut at = 0;
    while at < name.len() {
        if name.get(at..at + 36).is_some_and(is_guid) {
            out.push_str("<uuid>");
            at += 36;
        } else {
            let character = name[at..].chars().next().expect("a character");
            out.push(character);
            at += character.len_utf8();
        }
    }
    out
}

#[test]
fn the_archive_holds_the_stated_members() {
    let (_, _, bytes) = write_bytes(&scoped_cube());
    let zip = zip::ZipArchive::new(Cursor::new(bytes.clone())).expect("a ZIP archive");
    let names = zip
        .file_names()
        .map(|name| masked(&name.expect("a member name")))
        .collect::<BTreeSet<_>>();
    let folder = crate::manifest::GENERATED_DESIGN_ASSET_FOLDER;
    let mut expected = ["Manifest.dat".to_owned(), "Properties.dat".to_owned()]
        .into_iter()
        .collect::<BTreeSet<_>>();
    for name in [
        "Manifest.dat",
        "FusionDesignSegmentType1/MetaStream.dat",
        "FusionDesignSegmentType1/BulkStream.dat",
        "FusionACTSegmentType1/MetaStream.dat",
        "FusionACTSegmentType1/BulkStream.dat",
        "FusionBrowserSegmentType1/MetaStream.dat",
        "FusionBrowserSegmentType1/BulkStream.dat",
        "Breps.BlobParts/BREP.<uuid>.smbh",
        "Breps.BlobParts/BREP.<uuid>.smb",
        "DesignConfigurationTable.BlobParts/DesignConfigurationTable.<uuid>.dsgcfg",
        "DesignConfigurationTable.BlobParts/DesignConfigurationRule.<uuid>.dsgcfgrule",
        "Images.BlobParts/",
    ] {
        expected.insert(format!("{folder}/{name}"));
    }
    assert_eq!(names, expected);
    let (design, _) = design();
    for (record, at) in [(185, 62), (186, 61)] {
        let mut reader = Reader {
            bytes: design.record(record),
            at,
        };
        let basename = reader.utf16();
        let configuration = member(
            &bytes,
            &format!("DesignConfigurationTable.BlobParts/{basename}"),
        );
        assert_eq!(configuration, b"{}");
    }
}

/// The flags word and the `scale` of a binary ASM stream.
fn stream_header(stream: &[u8]) -> (u64, f64) {
    let mut at = 47;
    for _ in 0..3 {
        at += 2 + usize::from(stream[at + 1]);
    }
    assert_eq!(stream[at], 0x06);
    (u64_at(stream, 39), f64_at(stream, at + 1))
}

#[test]
fn the_snapshot_is_the_solved_part_of_the_history_stream() {
    let (design, archive) = design();
    let basename = |record: u64, at: usize| {
        let mut reader = Reader {
            bytes: design.record(record),
            at,
        };
        reader.utf16()
    };
    let history = member(&archive, &format!("Breps.BlobParts/{}", basename(197, 53)));
    let snapshot = member(&archive, &format!("Breps.BlobParts/{}", basename(195, 64)));
    assert_eq!(stream_header(&history), (3, 40.0));
    assert_eq!(stream_header(&snapshot), (2, 10.0));
    let has = |stream: &[u8], text: &[u8]| stream.windows(text.len()).any(|window| window == text);
    assert!(has(&history, b"History"));
    assert!(!has(&snapshot, b"History"));
    assert!(snapshot.ends_with(b"data"));
    assert!(snapshot.len() < history.len());
}

/// The user parameters of a design whose timeline items own no parameter
/// store family discriminator `6`, the value of every parameter record in a
/// Design stream whose component naming space is record 6 (DR-911).
#[test]
fn user_parameters_without_an_owned_parameter_store_discriminator_six() {
    let (report, _, bytes) = write_bytes(&scoped_cube());
    let design = segment(&bytes, "FusionDesignSegmentType1");
    let records = design.records_of(super::super::types::PARAMETER.guid);
    assert!(!records.is_empty());
    for (index, _) in records {
        assert_eq!(u32_at(design.record(index), 22), 6, "record {index}");
    }
    assert!(
        loss(&report, "export/record.residual-unavailable")
            .iter()
            .any(|message| message.contains("user parameter")
                && message.contains("family discriminator 6")),
        "{:#?}",
        report.losses
    );
}

#[test]
fn a_design_with_no_written_parameter_is_written_as_its_roster() {
    let mut ir = scoped_cube();
    ir.design.parameters.clear();
    let (report, decoded, bytes) = write_bytes(&ir);
    let design = segment(&bytes, "FusionDesignSegmentType1");
    assert_eq!(parameters(&design), 0);
    assert_eq!(design.record(17).len(), 61);
    assert_eq!(design.primary.len(), 214);
    assert!(decoded.model.parameters.is_empty());
    assert!(!report
        .losses
        .iter()
        .any(|loss| loss.message.contains("stores no design for these features")));
}
