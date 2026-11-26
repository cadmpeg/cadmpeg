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
    owned_test_file_with_global_and_directory_fields, OwnedTestEntity,
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

fn decode(file: &[u8]) -> CadIr {
    IgesCodec
        .decode(&mut Cursor::new(file), &DecodeOptions::default())
        .unwrap()
        .ir()
        .clone()
}

/// What a semantic write of `ir` at `version` does with each native record.
fn dispositions(ir: &CadIr, version: IgesVersion) -> NativeDispositions {
    let recovered = cadmpeg_ir::codec::write::edge_recovery::recover_edge_curves(ir).unwrap();
    crate::writer::target::synthesis_and_dispositions(&recovered.document, version)
        .unwrap()
        .1
}

/// The bytes and the loss messages of a semantic write of `ir` at `version`.
fn write(ir: &CadIr, version: IgesVersion) -> (Vec<u8>, Vec<String>) {
    let plan = IgesCodec
        .plan(
            EncodeInput::new(ir, None),
            TargetRequest::Explicit(version.descriptor().id.as_str()),
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
/// own constant.
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
    let bytes = crate::writer::source_global::source_global_bytes(ir).unwrap();
    crate::global::source_fields(&bytes).unwrap()
}

/// A write restates the source's Global fields that describe the model and
/// its exchange, and declares the Global cards Regenerated.
#[test]
fn the_written_global_restates_the_source_fields() {
    let ir = decode(&point_file(RESTATED_GLOBAL));
    let (bytes, messages) = write(&ir, IgesVersion::V5_3);
    let source = global_fields(&ir);
    let written = global_fields(&decode(&bytes));
    for number in [3, 4, 5, 6, 12, 13, 16, 17, 18, 21, 22, 24, 25, 26] {
        assert_eq!(
            written[number - 1],
            source[number - 1],
            "Global field {number}"
        );
    }
    assert_eq!(
        written[12],
        SourceField::Real(cadmpeg_ir::scalar::FiniteReal::new(2.5).unwrap())
    );
    let native = dispositions(&ir, IgesVersion::V5_3);
    for card in ir.native.namespace("iges").unwrap().arenas()["cards"]
        .iter()
        .filter(|card| card.field("section").unwrap().as_str() == Some("global"))
    {
        assert_eq!(
            native.disposition("iges", "cards", card.id()),
            NativeDisposition::Regenerated
        );
    }
    assert!(
        messages
            .iter()
            .all(|message| !message.contains("Global cards")),
        "{messages:#?}"
    );
}

/// A source field the output version has no place for drops the Global cards,
/// with a reason that names it: IGES 4.0 holds no model modification date or
/// application protocol, and dates with two-digit years only. The fields the
/// output holds are restated.
#[test]
fn a_global_field_the_output_version_lacks_drops_the_global_cards() {
    let ir = decode(&point_file(RESTATED_GLOBAL));
    let (bytes, messages) = write(&ir, IgesVersion::V4_0);
    let written = global_fields(&decode(&bytes));
    assert_eq!(
        written[12],
        SourceField::Real(cadmpeg_ir::scalar::FiniteReal::new(2.5).unwrap())
    );
    assert_eq!(written[20], SourceField::Text(b"Ada".to_vec()));
    let native = dispositions(&ir, IgesVersion::V4_0);
    let global = ir.native.namespace("iges").unwrap().arenas()["cards"]
        .iter()
        .filter(|card| card.field("section").unwrap().as_str() == Some("global"))
        .map(|card| card.id().to_owned())
        .collect::<Vec<_>>();
    assert!(!global.is_empty());
    for card in &global {
        assert_eq!(
            native.disposition("iges", "cards", card),
            NativeDisposition::Dropped
        );
    }
    assert!(
        messages.iter().any(|message| message.contains(&format!(
            "that are Global cards whose fields the semantic writer cannot restate: file \
             generation date, model modification date, application protocol: {}",
            global.join(", ")
        ))),
        "{messages:#?}"
    );
}

/// On the trimmed-sheet path the writer states each face of an open shell as
/// a trimmed surface and writes no shell record.
#[test]
fn an_open_shell_written_as_trimmed_surfaces_drops_its_shell_record() {
    let ir = decode(EXPLICIT_OPEN_SHELL);
    let native = dispositions(&ir, IgesVersion::V5_3);
    let shell = entity(&ir, 23);
    assert_eq!(
        native.disposition("iges", "entities", &shell),
        NativeDisposition::Dropped
    );
    for sequence in [15, 17, 19, 21] {
        let id = entity(&ir, sequence);
        assert_eq!(
            native.disposition("iges", "entities", &id),
            NativeDisposition::Regenerated,
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
}
