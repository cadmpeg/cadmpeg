// SPDX-License-Identifier: Apache-2.0
//! The semantic writer declares a native record Regenerated exactly when its
//! output states the record again (§3.4, Rule W3).
#![allow(clippy::unwrap_used)]

use std::io::Cursor;

use cadmpeg_ir::codec::write::target::TargetRequest;
use cadmpeg_ir::codec::write::{EncodeInput, Encoder, NativeDisposition, NativeDispositions};
use cadmpeg_ir::codec::{Codec, DecodeOptions};
use cadmpeg_ir::CadIr;

use crate::global::SourceField;
use crate::test_support::test_owned::{
    owned_test_file, owned_test_file_with_global_and_directory_fields, OwnedTestEntity,
};
use crate::{IgesCodec, IgesVersion};

const TRIMMED_PLANE: &[u8] = include_bytes!("../../../tests/golden/fixtures/trimmed_plane.igs");
const INNER_LOOP_PLANE: &[u8] =
    include_bytes!("../../../tests/golden/fixtures/trimmed_plane_inner_loop.igs");
const BOUNDED_PLANE: &[u8] =
    include_bytes!("../../../tests/golden/fixtures/parametrically_bounded_plane.igs");
const MULTI_PCURVE_BOUNDARY: &[u8] =
    include_bytes!("../../../tests/golden/fixtures/multi_pcurve_boundary.igs");
const SCALAR_PROPERTY_FORMS: &[u8] =
    include_bytes!("../../../tests/golden/fixtures/scalar_property_forms.igs");
const EXPLICIT_OPEN_SHELL: &[u8] =
    include_bytes!("../../../tests/golden/fixtures/explicit_open_shell.igs");
const NESTED_TRANSFORMED_POINT: &[u8] =
    include_bytes!("../../../tests/golden/fixtures/nested_transformed_point.igs");

fn decode(file: &[u8]) -> CadIr {
    IgesCodec
        .decode(&mut Cursor::new(file), &DecodeOptions::default())
        .unwrap()
        .ir()
        .clone()
}

/// What a semantic write of `ir` at `version` does with each native record.
fn dispositions(ir: &CadIr, version: IgesVersion) -> NativeDispositions {
    crate::test_support::with_policy_context(
        &[],
        &cadmpeg_core::decode::DecodePolicy {
            limits: cadmpeg_core::decode::ResourceLimits::desktop(),
        },
        |ctx| {
            let recovered =
                cadmpeg_ir::codec::write::edge_recovery::recover_edge_curves(ir, ctx).unwrap();
            crate::writer::target::synthesis_and_dispositions(ctx, &recovered.document, version)
                .unwrap()
                .1
        },
    )
}

/// The bytes and the loss messages of a semantic write of `ir` at `version`.
fn write(ir: &CadIr, version: IgesVersion) -> (Vec<u8>, Vec<String>) {
    let plan = IgesCodec
        .plan(
            EncodeInput::new(ir, None)
                .with_target(TargetRequest::Explicit(version.descriptor().id.as_str())),
        )
        .unwrap();
    let messages = plan
        .report()
        .losses
        .iter()
        .map(|loss| loss.message.clone())
        .collect();
    let mut bytes = Vec::new();
    plan.write_to(&mut bytes).unwrap();
    (bytes, messages)
}

fn entity(ir: &CadIr, sequence: u32) -> String {
    let id = format!("iges:entity:directory#{sequence}");
    assert!(
        ir.native.namespace("iges").unwrap().arenas()["entities"]
            .iter()
            .any(|record| record.id() == id),
        "the source holds {id}"
    );
    id
}

/// The ids of the Directory and Parameter cards of the entity at `sequence`.
fn entity_cards(ir: &CadIr, sequence: u32) -> Vec<String> {
    let arenas = ir.native.namespace("iges").unwrap().arenas();
    let record = arenas["entities"]
        .iter()
        .find(|record| record.id() == format!("iges:entity:directory#{sequence}"))
        .unwrap();
    let field = |name: &str| record.field(name).unwrap().as_u64().unwrap();
    let parameters =
        field("parameter_start")..field("parameter_start") + field("parameter_line_count");
    let directory = u64::from(sequence)..u64::from(sequence) + 2;
    let cards = arenas["cards"]
        .iter()
        .filter(|card| {
            let sequence = card.field("sequence").unwrap().as_u64().unwrap();
            match card.field("section").unwrap().as_str().unwrap() {
                "directory" => directory.contains(&sequence),
                "parameter" => parameters.contains(&sequence),
                _ => false,
            }
        })
        .map(|card| card.id().to_owned())
        .collect::<Vec<_>>();
    assert_eq!(cards.len(), 2 + parameters.count());
    cards
}

/// `trimmed_plane` without its Type 144, so that no face writes the Type
/// 142 boundary.
fn trimmed_plane_without_its_face() -> Vec<u8> {
    let text = String::from_utf8(TRIMMED_PLANE.to_vec()).unwrap();
    let kept = text
        .lines()
        .filter(|line| !(line.starts_with("     144") || line.starts_with("144,")))
        .map(|line| {
            if line.ends_with("T      1") {
                format!("{:<72}T      1", "S0000001G0000002D0000008P0000004")
            } else {
                line.to_owned()
            }
        })
        .collect::<Vec<_>>();
    assert_eq!(
        kept.len(),
        text.lines().count() - 3,
        "the fixture holds one Type 144"
    );
    let mut file = kept.join("\n");
    file.push('\n');
    file.into_bytes()
}

/// A parameter curve (use flag 05) that a written Type 141 or 142 carries is
/// stated again in the output boundary.
#[test]
fn a_parameter_curve_a_written_boundary_carries_is_regenerated() {
    for (name, file, pcurves) in [
        ("trimmed_plane", TRIMMED_PLANE.to_vec(), &[5][..]),
        (
            "trimmed_plane_inner_loop",
            INNER_LOOP_PLANE.to_vec(),
            &[5, 11],
        ),
        ("parametrically_bounded_plane", BOUNDED_PLANE.to_vec(), &[5]),
        (
            "multi_pcurve_boundary",
            MULTI_PCURVE_BOUNDARY.to_vec(),
            &[5, 7],
        ),
    ] {
        let ir = decode(&file);
        let native = dispositions(&ir, IgesVersion::V5_3);
        for sequence in pcurves {
            let id = entity(&ir, *sequence);
            assert_eq!(
                native.disposition("iges", "entities", &id),
                NativeDisposition::Regenerated,
                "{name} {id}"
            );
            for card in entity_cards(&ir, *sequence) {
                assert_eq!(
                    native.disposition("iges", "cards", &card),
                    NativeDisposition::Regenerated,
                    "{name} {card}"
                );
            }
        }
    }
}

/// A parameter curve that no written boundary carries is dropped as one.
#[test]
fn a_parameter_curve_no_written_boundary_carries_is_dropped() {
    let ir = decode(&trimmed_plane_without_its_face());
    let pcurve = entity(&ir, 5);
    assert_eq!(
        dispositions(&ir, IgesVersion::V5_3).disposition("iges", "entities", &pcurve),
        NativeDisposition::Dropped
    );
    let (_, messages) = write(&ir, IgesVersion::V5_3);
    let reason = "that are parameter curves no written boundary carries";
    let named = messages.iter().any(|message| {
        message.contains("arena `entities`")
            && message.split(reason).nth(1).is_some_and(|tail| {
                tail.trim_start_matches(':')
                    .split(';')
                    .next()
                    .is_some_and(|ids| ids.split(',').any(|id| id.trim() == pcurve))
            })
    });
    assert!(named, "{messages:#?}");
}

/// A Type 406 the writer does not encode leaves the file with its Directory
/// and Parameter cards, for a reason that names its form.
#[test]
fn a_dropped_property_drops_its_entity_and_cards() {
    let ir = decode(SCALAR_PROPERTY_FORMS);
    let native = dispositions(&ir, IgesVersion::V5_3);
    let (_, messages) = write(&ir, IgesVersion::V5_3);
    for sequence in (1..=27).step_by(2) {
        let id = entity(&ir, sequence);
        assert_eq!(
            native.disposition("iges", "entities", &id),
            NativeDisposition::Dropped,
            "{id}"
        );
        assert_eq!(
            native.disposition(
                "iges",
                "properties",
                &format!("iges:application:property#D{sequence}")
            ),
            NativeDisposition::Dropped,
            "{id}"
        );
        for card in entity_cards(&ir, sequence) {
            assert_eq!(
                native.disposition("iges", "cards", &card),
                NativeDisposition::Dropped,
                "{card}"
            );
        }
    }
    let reason = "that are properties of a form the semantic writer does not encode";
    for arena in ["entities", "properties", "cards"] {
        assert!(
            messages
                .iter()
                .any(|message| message.contains(&format!("arena `{arena}`"))
                    && message.contains(reason)),
            "{arena}: {messages:#?}"
        );
    }
    assert!(
        messages
            .iter()
            .all(|message| !message.contains("whose name properties")),
        "{messages:#?}"
    );
}

/// A Global section whose fields the writer restates, each other than its
/// own constant, at model scale 2.5.
const RESTATED_GLOBAL: &[u8] = b"1H,,1H;,6Hwidget,5Hw.igs,3Hsys,3H9.9,32,38,6,308,15,3Hrcv,2.5,\
2,2HMM,4,0.5,15H20250102.030405,0.001,1000.0,3HAda,3HLab,11,3,15H20250607.080910,5HAP214;";

fn point_file(global: &[u8]) -> Vec<u8> {
    owned_test_file_with_global_and_directory_fields(
        &[OwnedTestEntity {
            entity_type: 116,
            form: 0,
            label: "POINT".into(),
            status: "00000000",
            parameters: "116,1,2,3,0;".into(),
        }],
        global,
        &[],
        &[],
        &[],
        &[],
        &[],
    )
}

/// The Global fields of a decoded file, by field number from 1.
fn global_fields(ir: &CadIr) -> Vec<SourceField> {
    crate::global::source_fields(&global_bytes(ir)).unwrap()
}

fn global_bytes(ir: &CadIr) -> Vec<u8> {
    crate::writer::source_global::source_global_bytes(ir).unwrap()
}

/// The millimetre width of each line weight number the Global section of a
/// decoded file states.
fn line_widths_mm(ir: &CadIr) -> Vec<f64> {
    let context = crate::global::resolved(&global_bytes(ir))
        .unwrap()
        .length_context()
        .unwrap();
    let widths = (1..)
        .map_while(|number| context.line_weight_mm(number))
        .collect::<Vec<_>>();
    assert!(!widths.is_empty(), "the Global states a line weight scale");
    widths
}

fn point_positions(ir: &CadIr) -> Vec<[f64; 3]> {
    ir.geometry
        .points
        .iter()
        .map(|point| {
            let position = point.position().get();
            [position.x, position.y, position.z]
        })
        .collect()
}

fn assert_close(read_back: &[f64], source: &[f64]) {
    assert_eq!(read_back.len(), source.len(), "{read_back:?} {source:?}");
    for (read_back, source) in read_back.iter().zip(source) {
        assert!(
            (read_back - source).abs() <= 1e-9 * source.abs().max(1.0),
            "{read_back} {source}"
        );
    }
}

/// The Global cards of a decoded file.
fn global_cards(ir: &CadIr) -> Vec<String> {
    let cards = ir.native.namespace("iges").unwrap().arenas()["cards"]
        .iter()
        .filter(|card| card.field("section").unwrap().as_str() == Some("global"))
        .map(|card| card.id().to_owned())
        .collect::<Vec<_>>();
    assert!(!cards.is_empty());
    cards
}

/// The written Global section restates the source Global cards, so a write
/// of `ir` at `version` declares each of them Regenerated.
fn assert_global_regenerated(ir: &CadIr, version: IgesVersion) {
    let native = dispositions(ir, version);
    for card in global_cards(ir) {
        assert_eq!(
            native.disposition("iges", "cards", &card),
            NativeDisposition::Regenerated,
            "{card}"
        );
    }
}

/// The loss messages that charge a source Global field the write states
/// with another value.
fn global_field_charges(messages: &[String]) -> Vec<&str> {
    messages
        .iter()
        .map(String::as_str)
        .filter(|message| message.starts_with("IGES Global ") && message.contains(" (field "))
        .collect()
}

/// A file at model scale 0.5 in centimetres reads back from the write at the
/// same millimetre point and line width. The writer states lengths at model
/// scale 1 in millimetres, and charges each source field that states the
/// scale and the unit with its source and written values.
#[test]
fn a_scaled_source_reads_back_at_the_same_lengths() {
    let ir = decode(NESTED_TRANSFORMED_POINT);
    let (bytes, messages) = write(&ir, IgesVersion::V5_3);
    let read_back = decode(&bytes);
    assert_eq!(point_positions(&ir), [[0.0, 80.0, 60.0]]);
    assert_close(
        &point_positions(&read_back).concat(),
        &point_positions(&ir).concat(),
    );
    assert_close(&line_widths_mm(&read_back), &line_widths_mm(&ir));
    assert_global_regenerated(&ir, IgesVersion::V5_3);
    assert_eq!(
        global_field_charges(&messages),
        [
            "IGES Global model space scale (field 13) changed from 5.0000000000000000D-1 to 1.0",
            "IGES Global units flag (field 14) changed from 10 to 2",
            "IGES Global units name (field 15) changed from 2HCM to 2HMM",
        ]
    );
}

/// A write restates the source's Global fields that describe the model and
/// its exchange, states its own system id and preprocessor version, and
/// reads back at the source's millimetre point and line widths. The source
/// model scale is the one field it does not state.
#[test]
fn the_written_global_restates_the_source_fields() {
    let ir = decode(&point_file(RESTATED_GLOBAL));
    let (bytes, messages) = write(&ir, IgesVersion::V5_3);
    let read_back = decode(&bytes);
    let source = global_fields(&ir);
    let written = global_fields(&read_back);
    for number in [3, 4, 12, 16, 18, 21, 22, 24, 25, 26] {
        assert_eq!(
            written[number - 1],
            source[number - 1],
            "Global field {number}"
        );
    }
    assert_eq!(written[4], SourceField::Text(b"cadmpeg".to_vec()));
    assert_eq!(written[5], SourceField::Text(b"0.1".to_vec()));
    assert_close(&point_positions(&read_back).concat(), &[0.4, 0.8, 1.2]);
    assert_close(&point_positions(&ir).concat(), &[0.4, 0.8, 1.2]);
    assert_close(&line_widths_mm(&read_back), &line_widths_mm(&ir));
    assert_global_regenerated(&ir, IgesVersion::V5_3);
    assert_eq!(
        global_field_charges(&messages),
        ["IGES Global model space scale (field 13) changed from 2.5000000000000000D0 to 1.0"]
    );
}

/// A Global at model scale 1 in millimetres whose every field the writer
/// restates is charged nothing.
#[test]
fn a_restated_global_keeps_its_cards() {
    let global = b"1H,,1H;,6Hwidget,5Hw.igs,3Hsys,3H9.9,32,38,6,308,15,3Hrcv,1.0,2,2HMM,4,0.5,\
15H20250102.030405,0.001,1000.0,3HAda,3HLab,11,3,15H20250607.080910,5HAP214;";
    let ir = decode(&point_file(global));
    let (bytes, messages) = write(&ir, IgesVersion::V5_3);
    assert_close(&line_widths_mm(&decode(&bytes)), &line_widths_mm(&ir));
    assert_global_regenerated(&ir, IgesVersion::V5_3);
    assert!(global_field_charges(&messages).is_empty(), "{messages:#?}");
}

/// A source field-18 date is the date the written Global states.
#[test]
fn a_stated_generation_date_is_restated() {
    let ir = decode(&point_file(RESTATED_GLOBAL));
    let (bytes, _) = write(&ir, IgesVersion::V5_3);
    assert_eq!(
        global_fields(&decode(&bytes))[17],
        SourceField::Text(b"20250102.030405".to_vec())
    );
}

/// A source value the output version does not admit is written as the
/// writer's value, which the output version admits, and the field is charged
/// with both values: a relative line width 0 of an IGES 5.0 source in an
/// IGES 5.3 write, and 40000 line weight gradations in an IGES 4.0 write.
#[test]
fn a_global_value_the_output_version_rejects_is_replaced_and_named() {
    let relative = b"1H,,1H;,,,,,32,38,6,308,15,,1.0,2,2HMM,4,0.0,,0.001,1000.0,,,9,0;";
    let ir = decode(&point_file(relative));
    let (bytes, messages) = write(&ir, IgesVersion::V5_3);
    line_widths_mm(&decode(&bytes));
    assert_eq!(
        global_field_charges(&messages),
        ["IGES Global maximum line width (field 17) changed from 0 to 1.0"]
    );

    let fine = b"1H,,1H;,,,,,32,38,6,308,15,,1.0,2,2HMM,40000,0.5,,0.001,1000.0,,,11,0;";
    let ir = decode(&point_file(fine));
    assert_eq!(line_widths_mm(&ir).len(), 40000);
    let (bytes, messages) = write(&ir, IgesVersion::V4_0);
    line_widths_mm(&decode(&bytes));
    assert_eq!(
        global_field_charges(&messages),
        ["IGES Global maximum line weight gradations (field 16) changed from 40000 to 1"]
    );
}

/// A source field the output version has no place for is charged with its
/// source value: IGES 4.0 holds no model modification date or application
/// protocol, and dates with two-digit years only. The fields the output
/// holds are restated, and the Global cards are Regenerated.
#[test]
fn a_global_field_the_output_version_lacks_is_charged() {
    let ir = decode(&point_file(RESTATED_GLOBAL));
    let (bytes, messages) = write(&ir, IgesVersion::V4_0);
    let written = global_fields(&decode(&bytes));
    assert_eq!(written[20], SourceField::Text(b"Ada".to_vec()));
    assert_global_regenerated(&ir, IgesVersion::V4_0);
    let charges = global_field_charges(&messages);
    assert_eq!(charges.len(), 4, "{charges:#?}");
    assert_eq!(
        charges[0],
        "IGES Global model space scale (field 13) changed from 2.5000000000000000D0 to 1.0"
    );
    assert!(
        charges[1].starts_with(
            "IGES Global file generation date (field 18) changed from 15H20250102.030405 to 13H"
        ),
        "{charges:#?}"
    );
    assert_eq!(
        charges[2..],
        [
            "IGES Global model modification date (field 25) changed from 15H20250607.080910 to \
             no value: the written Global has no field 25",
            "IGES Global application protocol (field 26) changed from 5HAP214 to no value: the \
             written Global has no field 26",
        ]
    );
}

/// A one-face open shell whose loop edge uses carry parameter curves.
fn shell_with_parameter_curves() -> Vec<u8> {
    let entity = |entity_type, form, status, parameters: &str| OwnedTestEntity {
        entity_type,
        form,
        label: String::new(),
        status,
        parameters: parameters.into(),
    };
    let pcurve = |[x0, y0, x1, y1]: [u8; 4]| {
        entity(
            126,
            1,
            "00010500",
            &format!("126,1,1,1,0,1,0,0,0,1,1,1,1,{x0},{y0},0,{x1},{y1},0,0,1,0,0,1;"),
        )
    };
    owned_test_file(&[
        entity(116, 0, "00010000", "116,0,0,0;"),
        entity(123, 0, "00010000", "123,0,0,1;"),
        entity(190, 0, "00010000", "190,1,3;"),
        entity(110, 0, "00010000", "110,0,0,0,1,0,0;"),
        entity(110, 0, "00010000", "110,1,0,0,0,1,0;"),
        entity(110, 0, "00010000", "110,0,1,0,0,0,0;"),
        entity(502, 1, "00010000", "502,3,0,0,0,1,0,0,0,1,0;"),
        entity(
            504,
            1,
            "00010001",
            "504,3,7,13,1,13,2,9,13,2,13,3,11,13,3,13,1;",
        ),
        pcurve([0, 0, 1, 0]),
        pcurve([1, 0, 0, 1]),
        pcurve([0, 1, 0, 0]),
        entity(
            508,
            1,
            "00010000",
            "508,3,0,15,1,1,1,0,17,0,15,2,1,1,0,19,0,15,3,1,1,0,21;",
        ),
        entity(510, 1, "00010000", "510,5,1,1,23;"),
        entity(514, 2, "00000000", "514,1,25,1;"),
    ])
}

/// A parameter curve of a top-level shell's loop is stated again when the
/// written face states it.
#[test]
fn a_parameter_curve_of_a_written_shell_loop_is_regenerated() {
    let ir = decode(&shell_with_parameter_curves());
    let pcurves = ir
        .geometry
        .pcurves
        .iter()
        .map(|pcurve| pcurve.id.as_str())
        .collect::<Vec<_>>();
    assert_eq!(
        pcurves,
        [
            "iges:model:pcurve#D27:D23:0:0",
            "iges:model:pcurve#D27:D23:1:0",
            "iges:model:pcurve#D27:D23:2:0"
        ]
    );
    let native = dispositions(&ir, IgesVersion::V5_3);
    for sequence in [17, 19, 21] {
        let id = entity(&ir, sequence);
        assert_eq!(
            native.disposition("iges", "entities", &id),
            NativeDisposition::Regenerated,
            "{id}"
        );
    }
}

/// On the trimmed-sheet path the writer states each face of an open shell as
/// a trimmed surface and writes no shell, vertex list, or edge list record.
/// Each line edge is a segment of the written boundary and reads back as the
/// same line.
#[test]
fn an_open_shell_written_as_trimmed_surfaces_drops_its_shell_and_lists() {
    let ir = decode(EXPLICIT_OPEN_SHELL);
    let native = dispositions(&ir, IgesVersion::V5_3);
    let shell = entity(&ir, 23);
    assert_eq!(
        native.disposition("iges", "entities", &shell),
        NativeDisposition::Dropped
    );
    for sequence in [7, 9, 11, 13, 19, 21] {
        let id = entity(&ir, sequence);
        assert_eq!(
            native.disposition("iges", "entities", &id),
            NativeDisposition::Regenerated,
            "{id}"
        );
    }
    let lists = [entity(&ir, 15), entity(&ir, 17)];
    for id in &lists {
        assert_eq!(
            native.disposition("iges", "entities", id),
            NativeDisposition::Dropped,
            "{id}"
        );
    }
    let (_, messages) = write(&ir, IgesVersion::V5_3);
    assert!(
        messages.iter().any(|message| message.contains(&format!(
            "that are shells the semantic writer writes as separate trimmed surfaces with no \
             shell record: {shell}"
        ))),
        "{messages:#?}"
    );
    assert!(
        messages.iter().any(|message| message.contains(&format!(
            "that are vertex or edge lists of shells the semantic writer writes as trimmed \
             surfaces, whose boundaries state no vertex or edge list: {}",
            lists.join(", ")
        ))),
        "{messages:#?}"
    );
}
