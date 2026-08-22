// SPDX-License-Identifier: Apache-2.0

//! Deterministic IGES stress inputs for byte-freeze and timing sweeps.
//!
//! Writes six multi-megabyte files, each leaning on one decode hot path:
//! dense Type 102 composite chains, Type 128/142/144 trimmed surfaces,
//! counted lists filled to their declared boundaries, deep trailing pointer
//! groups, long Type 212/213 text runs, and a soup of unowned free curves.
//! Every value comes from fixed literals and a constant-seed LCG — no clock,
//! no environment, no float formatting — so each run reproduces the same
//! bytes, and the tests in this file pin every file's length and SHA-256.
//! After any deliberate change to the emitted bytes, regenerate and repin
//! those digests from a fresh run.
//!
//! ```text
//! cargo run -p cadmpeg-codec-iges --bin iges_stress -- OUTPUT_DIRECTORY
//! cargo run -p cadmpeg-codec-iges --bin iges_stress -- --fast OUTPUT_DIRECTORY
//! ```

use std::env;
use std::fmt::Write as _;
use std::fs;
use std::path::PathBuf;
use std::process::ExitCode;

use cadmpeg_ir::hash::sha256_hex;

const CARD_DATA_COLUMNS: usize = 72;
const PARAMETER_COLUMNS: usize = 64;
const DIRECTORY_FIELD_COLUMNS: usize = 8;
const SEQUENCE_COLUMNS: usize = 7;
const INFALLIBLE: &str = "writing to a String never fails";

const GLOBAL: &str = "1H,,1H;,7Hproduct,8Hpart.igs,7Hcadmpeg,3H0.1,32,38,6,308,15,0H,1.0,2,2HMM,1,1.0,15H20260714.000000,0.001,1000.0,6Hauthor,3Horg,11,0,0H,0H;";

const RIGHT_ANGLE: &str = "1.5707963267948966";

const INDEPENDENT: &str = "00000000";
const PHYSICALLY_DEPENDENT: &str = "00010000";
const PARAMETRIC_DEPENDENT: &str = "00010500";

struct Item {
    entity_type: u32,
    form: i64,
    label: &'static str,
    status: &'static str,
    parameters: String,
}

fn item(
    entity_type: u32,
    form: i64,
    label: &'static str,
    status: &'static str,
    parameters: String,
) -> Item {
    Item {
        entity_type,
        form,
        label,
        status,
        parameters,
    }
}

struct Record(String);

impl Record {
    fn new(entity_type: u32) -> Self {
        let mut text = String::new();
        write!(text, "{entity_type}").expect(INFALLIBLE);
        Self(text)
    }

    fn integer(&mut self, value: i64) {
        write!(self.0, ",{value}").expect(INFALLIBLE);
    }

    fn integers(&mut self, values: &[i64]) {
        for value in values {
            self.integer(*value);
        }
    }

    fn real(&mut self, thousandths: i64) {
        let magnitude = thousandths.unsigned_abs();
        let sign = if thousandths < 0 { "-" } else { "" };
        write!(
            self.0,
            ",{sign}{}.{:03}",
            magnitude / 1000,
            magnitude % 1000
        )
        .expect(INFALLIBLE);
    }

    fn reals(&mut self, values: &[i64]) {
        for value in values {
            self.real(*value);
        }
    }

    fn verbatim(&mut self, value: &str) {
        write!(self.0, ",{value}").expect(INFALLIBLE);
    }

    fn hollerith(&mut self, value: &str) {
        write!(self.0, ",{}H{value}", value.len()).expect(INFALLIBLE);
    }

    fn finish(self) -> String {
        let mut text = self.0;
        text.push(';');
        text
    }
}

struct Lcg(u64);

impl Lcg {
    const fn new(seed: u64) -> Self {
        Self(seed)
    }

    fn next(&mut self) -> u64 {
        self.0 = self
            .0
            .wrapping_mul(6_364_136_223_846_793_005)
            .wrapping_add(1_442_695_040_888_963_407);
        self.0 >> 16
    }

    fn below(&mut self, bound: u64) -> u64 {
        self.next() % bound
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Scale {
    Full,
    Fast,
}

impl Scale {
    const fn pick(self, full: usize, fast: usize) -> usize {
        match self {
            Self::Full => full,
            Self::Fast => fast,
        }
    }
}

fn right_aligned(field: &mut [u8], value: u64) {
    field.fill(b' ');
    let mut index = field.len();
    let mut rest = value;
    loop {
        assert!(index > 0, "value does not fit the fixed field width");
        index -= 1;
        field[index] = b'0' + (rest % 10) as u8;
        rest /= 10;
        if rest == 0 {
            break;
        }
    }
}

fn card(out: &mut Vec<u8>, data: &[u8], section: u8, sequence: u32) {
    assert!(
        data.len() <= CARD_DATA_COLUMNS,
        "card data exceeds the seventy-two data columns"
    );
    let start = out.len();
    out.extend_from_slice(data);
    out.resize(start + CARD_DATA_COLUMNS, b' ');
    out.push(section);
    let mut field = [b' '; SEQUENCE_COLUMNS];
    right_aligned(&mut field, u64::from(sequence));
    out.extend_from_slice(&field);
    out.push(b'\n');
}

fn directory_card(out: &mut Vec<u8>, fields: [&str; 9], sequence: u32) {
    let mut data = [b' '; CARD_DATA_COLUMNS];
    for (index, field) in fields.iter().enumerate() {
        assert!(
            field.len() <= DIRECTORY_FIELD_COLUMNS,
            "directory field exceeds eight columns"
        );
        let end = index * DIRECTORY_FIELD_COLUMNS + DIRECTORY_FIELD_COLUMNS;
        data[end - field.len()..end].copy_from_slice(field.as_bytes());
    }
    card(out, &data, b'D', sequence);
}

fn parameter_card(out: &mut Vec<u8>, data: &[u8], owner: u32, sequence: u32) {
    assert!(
        data.len() <= PARAMETER_COLUMNS,
        "parameter fragment exceeds the sixty-four data columns"
    );
    let mut payload = [b' '; CARD_DATA_COLUMNS];
    payload[..data.len()].copy_from_slice(data);
    right_aligned(&mut payload[PARAMETER_COLUMNS..], u64::from(owner));
    card(out, &payload, b'P', sequence);
}

fn directory_sequence(index: usize) -> u32 {
    u32::try_from(index * 2 + 1).expect("directory sequence fits a thirty-two bit value")
}

fn assemble(start_text: &str, entities: &[Item]) -> Vec<u8> {
    let mut bytes = Vec::new();
    card(&mut bytes, start_text.as_bytes(), b'S', 1);
    let mut global_cards: u32 = 0;
    for chunk in GLOBAL.as_bytes().chunks(CARD_DATA_COLUMNS) {
        global_cards += 1;
        card(&mut bytes, chunk, b'G', global_cards);
    }
    let mut parameter_start: u32 = 1;
    for (index, entry) in entities.iter().enumerate() {
        assert!(
            !entry.parameters.is_empty(),
            "every entity owns at least one parameter card"
        );
        let sequence = directory_sequence(index);
        let line_count = u32::try_from(entry.parameters.len().div_ceil(PARAMETER_COLUMNS))
            .expect("parameter card count fits a thirty-two bit value");
        let entity_type = entry.entity_type.to_string();
        let form = entry.form.to_string();
        let start_field = parameter_start.to_string();
        let count_field = line_count.to_string();
        directory_card(
            &mut bytes,
            [
                &entity_type,
                &start_field,
                "0",
                "0",
                "0",
                "0",
                "0",
                "0",
                entry.status,
            ],
            sequence,
        );
        directory_card(
            &mut bytes,
            [
                &entity_type,
                "0",
                "0",
                &count_field,
                &form,
                "",
                "",
                entry.label,
                "0",
            ],
            sequence + 1,
        );
        parameter_start += line_count;
    }
    let mut parameter_sequence: u32 = 1;
    for (index, entry) in entities.iter().enumerate() {
        let owner = directory_sequence(index);
        for chunk in entry.parameters.as_bytes().chunks(PARAMETER_COLUMNS) {
            parameter_card(&mut bytes, chunk, owner, parameter_sequence);
            parameter_sequence += 1;
        }
    }
    let terminate = format!(
        "S0000001G{global_cards:07}D{:07}P{:07}",
        entities.len() * 2,
        parameter_sequence - 1
    );
    card(&mut bytes, terminate.as_bytes(), b'T', 1);
    bytes
}

fn chain_vertex(index: usize) -> [i64; 3] {
    let step = index as i64;
    [step * 125, (step * step) % 977 * 37, 0]
}

fn chain_segment(index: usize) -> Item {
    let start = chain_vertex(index);
    let end = chain_vertex(index + 1);
    if index.is_multiple_of(2) {
        let mut record = Record::new(110);
        record.reals(&start);
        record.reals(&end);
        item(110, 0, "CHAINLIN", PHYSICALLY_DEPENDENT, record.finish())
    } else {
        let mut record = Record::new(126);
        record.integers(&[1, 1, 1, 0, 1, 0]);
        record.integers(&[0, 0, 1, 1]);
        record.integers(&[1, 1]);
        record.reals(&start);
        record.reals(&end);
        record.integers(&[0, 1]);
        record.integers(&[0, 0, 1]);
        item(126, 0, "CHAINNUR", PHYSICALLY_DEPENDENT, record.finish())
    }
}

fn composite_chains(scale: Scale) -> Vec<u8> {
    let pool = scale.pick(12_000, 400);
    let children = scale.pick(240, 24);
    let overlapping = scale.pick(1_000, 40);
    let nests = scale.pick(200, 10);
    let nest_size = 8;
    let tiles = pool / children;
    assert!(tiles > nest_size, "the chain holds at least one nested run");
    let mut entities = Vec::with_capacity(pool + tiles + overlapping + nests);
    for index in 0..pool {
        entities.push(chain_segment(index));
    }
    let tile_base = entities.len();
    for tile in 0..tiles {
        let mut record = Record::new(102);
        record.integer(i64::try_from(children).expect("child count fits a signed integer"));
        for child in 0..children {
            record.integer(i64::from(directory_sequence(tile * children + child)));
        }
        entities.push(item(
            102,
            0,
            "TILECURV",
            PHYSICALLY_DEPENDENT,
            record.finish(),
        ));
    }
    for index in 0..overlapping {
        let first = (index * 7) % (pool - children);
        let mut record = Record::new(102);
        record.integer(i64::try_from(children).expect("child count fits a signed integer"));
        for child in 0..children {
            record.integer(i64::from(directory_sequence(first + child)));
        }
        entities.push(item(102, 0, "OVERCURV", INDEPENDENT, record.finish()));
    }
    for index in 0..nests {
        let first = (index * 3) % (tiles - nest_size);
        let mut record = Record::new(102);
        record.integer(i64::try_from(nest_size).expect("nest size fits a signed integer"));
        for child in 0..nest_size {
            record.integer(i64::from(directory_sequence(tile_base + first + child)));
        }
        entities.push(item(102, 0, "NESTCURV", INDEPENDENT, record.finish()));
    }
    assemble("cadmpeg IGES stress: dense Type 102 chains", &entities)
}

fn unit_square(scale: i64, offset: i64) -> [[i64; 2]; 5] {
    [
        [offset, offset],
        [offset + scale, offset],
        [offset + scale, offset + scale],
        [offset, offset + scale],
        [offset, offset],
    ]
}

fn closed_polyline(common: i64, corners: [[i64; 2]; 5]) -> String {
    let mut record = Record::new(106);
    record.integers(&[1, 5]);
    record.real(common);
    for corner in corners {
        record.reals(&corner);
    }
    record.finish()
}

fn bilinear_patch(height: i64) -> String {
    let mut record = Record::new(128);
    record.integers(&[1, 1, 1, 1, 0, 0, 1, 0, 0]);
    record.integers(&[0, 0, 1, 1]);
    record.integers(&[0, 0, 1, 1]);
    record.integers(&[1, 1, 1, 1]);
    for corner in [[0, 0], [1000, 0], [0, 1000], [1000, 1000]] {
        record.reals(&[corner[0], corner[1], height]);
    }
    record.integers(&[0, 1, 0, 1]);
    record.finish()
}

fn trimmed_surfaces(scale: Scale) -> Vec<u8> {
    let blocks = scale.pick(1_400, 30);
    let inner_loops = 3;
    let mut entities = Vec::with_capacity(blocks * (inner_loops * 3 + 5));
    for block in 0..blocks {
        let base = entities.len();
        let height = (block as i64 % 200) * 5;
        entities.push(item(
            128,
            0,
            "PATCH",
            PHYSICALLY_DEPENDENT,
            bilinear_patch(height),
        ));
        let mut boundaries = Vec::with_capacity(inner_loops + 1);
        for loop_index in 0..=inner_loops {
            let corners = if loop_index == 0 {
                unit_square(1000, 0)
            } else {
                unit_square(150, 100 + 250 * (loop_index as i64 - 1))
            };
            let model = entities.len();
            entities.push(item(
                106,
                63,
                "LOOPMODL",
                PHYSICALLY_DEPENDENT,
                closed_polyline(height, corners),
            ));
            let pcurve = entities.len();
            entities.push(item(
                106,
                63,
                "LOOPPCUR",
                PARAMETRIC_DEPENDENT,
                closed_polyline(0, corners),
            ));
            let mut record = Record::new(142);
            record.integers(&[
                0,
                i64::from(directory_sequence(base)),
                i64::from(directory_sequence(pcurve)),
                i64::from(directory_sequence(model)),
                1,
            ]);
            boundaries.push(entities.len());
            entities.push(item(
                142,
                0,
                "ONSURF",
                PHYSICALLY_DEPENDENT,
                record.finish(),
            ));
        }
        let mut record = Record::new(144);
        record.integers(&[
            i64::from(directory_sequence(base)),
            1,
            i64::try_from(inner_loops).expect("inner loop count fits a signed integer"),
            i64::from(directory_sequence(boundaries[0])),
        ]);
        for boundary in &boundaries[1..] {
            record.integer(i64::from(directory_sequence(*boundary)));
        }
        entities.push(item(144, 0, "TRIMMED", INDEPENDENT, record.finish()));
    }
    assemble(
        "cadmpeg IGES stress: Type 128/142/144 trimmed surfaces",
        &entities,
    )
}

fn counted_lists(scale: Scale) -> Vec<u8> {
    let points = scale.pick(2_000, 100);
    let group_members = scale.pick(1_500, 60);
    let groups = scale.pick(150, 6);
    let levels = scale.pick(1_200, 50);
    let level_properties = scale.pick(150, 6);
    let triples = scale.pick(800, 40);
    let copious_records = scale.pick(120, 8);
    let positions = scale.pick(1_000, 40);
    let arrays = scale.pick(150, 6);
    let mut seed = Lcg::new(0x1963_0503_1120_0001);
    let mut entities = Vec::new();
    for index in 0..points {
        let step = index as i64;
        let mut record = Record::new(116);
        record.reals(&[step * 7, step * 13 % 4001, step * 3 % 907]);
        record.integer(0);
        entities.push(item(116, 0, "SITE", INDEPENDENT, record.finish()));
    }
    for index in 0..groups {
        let mut record = Record::new(402);
        record.integer(i64::try_from(group_members).expect("member count fits a signed integer"));
        for member in 0..group_members {
            let target = (index * 13 + member) % points;
            record.integer(i64::from(directory_sequence(target)));
        }
        entities.push(item(402, 7, "GROUP", INDEPENDENT, record.finish()));
    }
    for index in 0..level_properties {
        let mut record = Record::new(406);
        record.integer(i64::try_from(levels).expect("level count fits a signed integer"));
        let base = i64::try_from(index * levels).expect("level base fits a signed integer");
        for level in 0..levels {
            record.integer(base + i64::try_from(level).expect("level fits a signed integer"));
        }
        entities.push(item(406, 1, "LEVELS", INDEPENDENT, record.finish()));
    }
    for index in 0..copious_records {
        let mut record = Record::new(106);
        record.integers(&[
            2,
            i64::try_from(triples).expect("tuple count fits a signed integer"),
        ]);
        let defaulted_tail = index % 2 == 1;
        for tuple in 0..triples {
            let step = (index * triples + tuple) as i64;
            record.real(step % 9973 * 3);
            record.real(step % 8971 * 5);
            if !defaulted_tail || tuple + 1 < triples {
                record.real(step % 7963 * 7);
            }
        }
        entities.push(item(106, 2, "TUPLES", INDEPENDENT, record.finish()));
    }
    for index in 0..arrays {
        let base = seed.below(points as u64) as usize;
        let mut record = Record::new(412);
        record.integer(i64::from(directory_sequence(base)));
        record.real(1_000);
        record.reals(&[0, 0, 0]);
        record.integers(&[8, 8]);
        record.reals(&[2_000, 2_000]);
        record.real(0);
        record.integer(i64::try_from(positions).expect("position count fits a signed integer"));
        record.integer(0);
        for position in 0..positions {
            record.integer(
                i64::try_from((index + position) % 64 + 1).expect("position fits a signed integer"),
            );
        }
        entities.push(item(412, 0, "ARRAY", INDEPENDENT, record.finish()));
    }
    assemble(
        "cadmpeg IGES stress: counted lists at their boundaries",
        &entities,
    )
}

fn trailing_groups(scale: Scale) -> Vec<u8> {
    let points = scale.pick(240, 24);
    let associations = scale.pick(64, 8);
    let properties = scale.pick(64, 8);
    let carriers = scale.pick(1_500, 40);
    let association_depth = scale.pick(180, 12);
    let property_depth = scale.pick(180, 12);
    let mut entities = Vec::new();
    for index in 0..points {
        let step = index as i64;
        let mut record = Record::new(116);
        record.reals(&[step * 11, step * 17 % 3001, step * 5 % 1009]);
        record.integer(0);
        entities.push(item(116, 0, "ANCHOR", INDEPENDENT, record.finish()));
    }
    let association_base = entities.len();
    for index in 0..associations {
        let mut record = Record::new(402);
        record.integer(3);
        for member in 0..3usize {
            record.integer(i64::from(directory_sequence((index * 3 + member) % points)));
        }
        entities.push(item(402, 7, "ASSOC", INDEPENDENT, record.finish()));
    }
    let property_base = entities.len();
    for index in 0..properties {
        let mut record = Record::new(406);
        record.integer(3);
        let base = i64::try_from(index * 3).expect("level base fits a signed integer");
        record.integers(&[base, base + 1, base + 2]);
        entities.push(item(406, 1, "PROPERTY", INDEPENDENT, record.finish()));
    }
    let append_groups = |record: &mut Record, index: usize| {
        record.integer(
            i64::try_from(association_depth).expect("association depth fits a signed integer"),
        );
        for slot in 0..association_depth {
            let target = association_base + (index * 5 + slot) % associations;
            record.integer(i64::from(directory_sequence(target)));
        }
        record
            .integer(i64::try_from(property_depth).expect("property depth fits a signed integer"));
        for slot in 0..property_depth {
            let target = property_base + (index * 3 + slot) % properties;
            record.integer(i64::from(directory_sequence(target)));
        }
    };
    for index in 0..carriers {
        let step = index as i64;
        match index % 3 {
            0 => {
                let mut record = Record::new(110);
                record.reals(&[step * 9, step * 3 % 5003, 0]);
                record.reals(&[step * 9 + 1_000, step * 3 % 5003, 0]);
                append_groups(&mut record, index);
                entities.push(item(110, 0, "CARRYLIN", INDEPENDENT, record.finish()));
            }
            1 => {
                let mut record = Record::new(116);
                record.reals(&[step * 4, step * 6 % 4003, step * 2 % 2003]);
                record.integer(0);
                append_groups(&mut record, index);
                entities.push(item(116, 0, "CARRYPNT", INDEPENDENT, record.finish()));
            }
            _ => {
                let mut record = Record::new(123);
                record.reals(&[0, 0, 1_000]);
                append_groups(&mut record, index);
                entities.push(item(
                    123,
                    0,
                    "CARRYDIR",
                    PHYSICALLY_DEPENDENT,
                    record.finish(),
                ));
            }
        }
    }
    assemble(
        "cadmpeg IGES stress: deep trailing pointer groups",
        &entities,
    )
}

fn text_run(seed: &mut Lcg, length: usize) -> String {
    const ALPHABET: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZ0123456789-_.:+*/#";
    let mut text = String::with_capacity(length);
    for _ in 0..length {
        let pick = seed.below(ALPHABET.len() as u64) as usize;
        text.push(char::from(ALPHABET[pick]));
    }
    text
}

fn annotation_runs(scale: Scale) -> Vec<u8> {
    let general_notes = scale.pick(400, 12);
    let general_strings = scale.pick(60, 8);
    let new_notes = scale.pick(200, 8);
    let new_strings = scale.pick(30, 5);
    let mut seed = Lcg::new(0x0212_0213_0000_002d);
    let mut entities = Vec::new();
    for index in 0..general_notes {
        let mut record = Record::new(212);
        record.integer(i64::try_from(general_strings).expect("string count fits a signed integer"));
        for string in 0..general_strings {
            let length = 40 + (index + string) % 41;
            let text = text_run(&mut seed, length);
            record.integer(i64::try_from(text.len()).expect("text length fits a signed integer"));
            record.reals(&[3_000, 2_000]);
            record.integer(1);
            record.verbatim(RIGHT_ANGLE);
            record.real(0);
            record.integers(&[0, 0]);
            record.reals(&[(index as i64 % 97) * 250, (string as i64 % 89) * 250, 0]);
            record.hollerith(&text);
        }
        entities.push(item(212, 0, "NOTE", INDEPENDENT, record.finish()));
    }
    for index in 0..new_notes {
        let mut record = Record::new(213);
        record.reals(&[40_000, 20_000]);
        record.integer(2);
        record.reals(&[0, 20_000, 0, 0, 0, 18_000, 0, -5_000]);
        record.integer(i64::try_from(new_strings).expect("string count fits a signed integer"));
        for string in 0..new_strings {
            let length = 32 + (index + string) % 49;
            let style = text_run(&mut seed, 4);
            let text = text_run(&mut seed, length);
            record.integer(0);
            record.reals(&[2_000, 3_000, -500]);
            record.real(0);
            record.integer(18);
            record.real(0);
            record.hollerith(&style);
            record.integer(i64::try_from(text.len()).expect("text length fits a signed integer"));
            record.reals(&[12_000, 3_000]);
            record.integer(1);
            record.verbatim(RIGHT_ANGLE);
            record.real(0);
            record.integers(&[0, 0]);
            record.reals(&[(index as i64 % 83) * 250, (string as i64 % 79) * 250, 0]);
            record.hollerith(&text);
        }
        entities.push(item(213, 0, "NEWNOTE", INDEPENDENT, record.finish()));
    }
    assemble(
        "cadmpeg IGES stress: long Type 212 and 213 text runs",
        &entities,
    )
}

fn free_curve_soup(scale: Scale) -> Vec<u8> {
    let curves = scale.pick(20_000, 400);
    let path_points = scale.pick(24, 6);
    let mut entities = Vec::with_capacity(curves);
    for index in 0..curves {
        let step = index as i64;
        let plane = step % 61 * 250;
        match index % 5 {
            0 => {
                let mut record = Record::new(110);
                record.reals(&[step * 3 % 7001, step * 5 % 6007, plane]);
                record.reals(&[step * 3 % 7001 + 1_500, step * 5 % 6007 + 750, plane]);
                entities.push(item(110, 0, "FREELINE", INDEPENDENT, record.finish()));
            }
            1 => {
                let radius = 500 + step % 47 * 125;
                let centre = [step * 7 % 8009, step * 11 % 8011];
                let mut record = Record::new(100);
                record.real(plane);
                record.reals(&centre);
                record.reals(&[centre[0] + radius, centre[1]]);
                record.reals(&[centre[0], centre[1] + radius]);
                entities.push(item(100, 0, "FREEARC", INDEPENDENT, record.finish()));
            }
            2 => {
                let mut record = Record::new(126);
                record.integers(&[1, 1, 1, 0, 1, 0]);
                record.integers(&[0, 0, 1, 1]);
                record.integers(&[1, 1]);
                record.reals(&[step * 13 % 5003, step * 17 % 5009, plane]);
                record.reals(&[step * 13 % 5003 + 2_000, step * 17 % 5009 + 1_000, plane]);
                record.integers(&[0, 1]);
                record.integers(&[0, 0, 1]);
                entities.push(item(126, 0, "FREENURB", INDEPENDENT, record.finish()));
            }
            3 => {
                let mut record = Record::new(106);
                record.integers(&[
                    2,
                    i64::try_from(path_points).expect("path point count fits a signed integer"),
                ]);
                for point in 0..path_points {
                    let along = i64::try_from(point).expect("path index fits a signed integer");
                    record.reals(&[
                        step * 2 % 4001 + along * 300,
                        step * 6 % 4003 + along * along * 25,
                        plane,
                    ]);
                }
                entities.push(item(106, 12, "FREEPATH", INDEPENDENT, record.finish()));
            }
            _ => {
                let mut record = Record::new(116);
                record.reals(&[step * 19 % 9001, step * 23 % 9007, plane]);
                record.integer(0);
                entities.push(item(116, 0, "FREEPNT", INDEPENDENT, record.finish()));
            }
        }
    }
    assemble("cadmpeg IGES stress: free-curve soup", &entities)
}

fn generate(scale: Scale) -> Vec<(&'static str, Vec<u8>)> {
    vec![
        ("stress-composite-chains.igs", composite_chains(scale)),
        ("stress-trimmed-surfaces.igs", trimmed_surfaces(scale)),
        ("stress-counted-lists.igs", counted_lists(scale)),
        ("stress-trailing-groups.igs", trailing_groups(scale)),
        ("stress-annotation-runs.igs", annotation_runs(scale)),
        ("stress-free-curve-soup.igs", free_curve_soup(scale)),
    ]
}

fn usage() -> ExitCode {
    eprintln!("usage: iges_stress [--fast] OUTPUT_DIRECTORY");
    ExitCode::from(2)
}

fn main() -> ExitCode {
    let mut directory: Option<PathBuf> = None;
    let mut scale = Scale::Full;
    for argument in env::args_os().skip(1) {
        if argument == "--fast" {
            scale = Scale::Fast;
        } else if argument.to_string_lossy().starts_with('-') || directory.is_some() {
            return usage();
        } else {
            directory = Some(PathBuf::from(argument));
        }
    }
    let Some(directory) = directory else {
        return usage();
    };
    if let Err(error) = fs::create_dir_all(&directory) {
        eprintln!("create {}: {error}", directory.display());
        return ExitCode::from(2);
    }
    for (name, bytes) in generate(scale) {
        let path = directory.join(name);
        if let Err(error) = fs::write(&path, &bytes) {
            eprintln!("write {}: {error}", path.display());
            return ExitCode::from(2);
        }
        println!("{name} {} {}", bytes.len(), sha256_hex(&bytes));
    }
    ExitCode::SUCCESS
}

#[cfg(test)]
mod tests {
    use super::{generate, Scale, CARD_DATA_COLUMNS, SEQUENCE_COLUMNS};
    use cadmpeg_ir::hash::sha256_hex;

    const CARD_LINE_BYTES: usize = CARD_DATA_COLUMNS + 1 + SEQUENCE_COLUMNS + 1;

    const FULL_DIGESTS: [(&str, usize, &str); 6] = [
        (
            "stress-composite-chains.igs",
            5_330_610,
            "787b28febef22358ac6bacb44b140741ada4700664b40b2bcb48f0ceb0fc2a9c",
        ),
        (
            "stress-trimmed-surfaces.igs",
            5_783_724,
            "04f7dc5690ce076ebf0a4ff4a26fa5189d67dfb7461dfdad0c6ebc0b3d36cb3a",
        ),
        (
            "stress-counted-lists.igs",
            6_417_306,
            "b825511a6b72329a523dc257cb6ff3255af3dae42d59e9e028c66eff305bbc04",
        ),
        (
            "stress-trailing-groups.igs",
            3_167_748,
            "79df7f99dacda278a0c1c129f1435d036f63dcba0d0c8afc70e34e84e3080afb",
        ),
        (
            "stress-annotation-runs.igs",
            5_337_819,
            "3dddb78059bc61b9be521b1d87a1f48ef7100b5c39eae66104f3ba6870c69553",
        ),
        (
            "stress-free-curve-soup.igs",
            7_286_355,
            "68b9bb8eeb7e7b9f5798905c7a06dede9779ac7e4d6a207de841ac29155156be",
        ),
    ];

    const FAST_DIGESTS: [(&str, usize, &str); 6] = [
        (
            "stress-composite-chains.igs",
            134_298,
            "10712a5f40f54528554fee0796139ddd7e1a2c01ce5f060f610e10e1f1eb8d7a",
        ),
        (
            "stress-trimmed-surfaces.igs",
            124_254,
            "d273b56eed232a441736c5093cc00d6aa730f353c190a3a4f968b4e458fc46f6",
        ),
        (
            "stress-counted-lists.igs",
            41_715,
            "350f9446b94a4ce1bc04198d11556a08449d36bf1a051a6af16970eb729899bc",
        ),
        (
            "stress-trailing-groups.igs",
            23_004,
            "d72828ca71deaec62c2653d65a6b661e714f67d2274ae255ca91d43925521dd0",
        ),
        (
            "stress-annotation-runs.igs",
            27_054,
            "eb86ca3acf860230ee8acfcded747026c05340a58be8f79c275f69092e730cb8",
        ),
        (
            "stress-free-curve-soup.igs",
            110_484,
            "d154fd237ae50f1ccb11ce67b96dd596b4cc0b4a95142308596882cd42b9fe0c",
        ),
    ];

    fn framing_holds(bytes: &[u8]) {
        assert_eq!(bytes.len() % CARD_LINE_BYTES, 0);
        let mut markers = Vec::new();
        let mut counts = [0u32; 5];
        for card in bytes.chunks(CARD_LINE_BYTES) {
            assert_eq!(card[CARD_LINE_BYTES - 1], b'\n');
            let marker = card[CARD_DATA_COLUMNS];
            let section = match marker {
                b'S' => 0,
                b'G' => 1,
                b'D' => 2,
                b'P' => 3,
                b'T' => 4,
                _ => panic!("unknown section marker"),
            };
            if markers.last() != Some(&section) {
                assert!(markers.last().is_none_or(|last| *last < section));
                markers.push(section);
            }
            counts[section] += 1;
            let sequence: u32 = std::str::from_utf8(&card[CARD_DATA_COLUMNS + 1..])
                .expect("sequence field is ASCII")
                .trim()
                .parse()
                .expect("sequence field is a decimal integer");
            assert_eq!(sequence, counts[section]);
        }
        assert_eq!(markers, [0, 1, 2, 3, 4]);
        assert_eq!(counts[0], 1);
        assert_eq!(counts[4], 1);
        assert_eq!(counts[2] % 2, 0);
        let terminate = &bytes[bytes.len() - CARD_LINE_BYTES..];
        assert_eq!(
            std::str::from_utf8(&terminate[..32]).expect("terminate card is ASCII"),
            format!(
                "S0000001G{:07}D{:07}P{:07}",
                counts[1], counts[2], counts[3]
            )
        );
    }

    fn scale_matches_pins(scale: Scale, pins: [(&str, usize, &str); 6]) {
        let generated = generate(scale);
        assert_eq!(generated.len(), pins.len());
        for ((name, bytes), (expected_name, expected_length, expected_digest)) in
            generated.into_iter().zip(pins)
        {
            assert_eq!(name, expected_name);
            framing_holds(&bytes);
            assert_eq!(bytes.len(), expected_length, "{name}");
            assert_eq!(sha256_hex(&bytes), expected_digest, "{name}");
        }
    }

    #[test]
    fn full_scale_output_matches_pinned_digests() {
        scale_matches_pins(Scale::Full, FULL_DIGESTS);
    }

    #[test]
    fn fast_scale_output_matches_pinned_digests() {
        scale_matches_pins(Scale::Fast, FAST_DIGESTS);
    }

    #[test]
    fn repeated_generation_is_byte_identical() {
        for ((first_name, first), (second_name, second)) in
            generate(Scale::Fast).into_iter().zip(generate(Scale::Fast))
        {
            assert_eq!(first_name, second_name);
            assert_eq!(first, second, "{first_name}");
        }
    }
}
