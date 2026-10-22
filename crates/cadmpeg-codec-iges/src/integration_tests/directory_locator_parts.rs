// SPDX-License-Identifier: Apache-2.0
use cadmpeg_ir::source_fidelity::parts::PartState;
use cadmpeg_ir::NativeUnknownRecord;

use crate::test_support::test_owned::{owned_test_file, OwnedTestEntity};

fn points() -> Vec<u8> {
    owned_test_file(&[
        OwnedTestEntity {
            entity_type: 116,
            form: 0,
            label: "FIRST".into(),
            status: "00000000",
            parameters: "116,1,2,3,0;".into(),
        },
        OwnedTestEntity {
            entity_type: 116,
            form: 0,
            label: "SECOND".into(),
            status: "00000000",
            parameters: "116,4,5,6,0;".into(),
        },
    ])
}

fn section(bytes: &[u8], code: u8) -> usize {
    bytes
        .chunks_exact(81)
        .position(|card| card[72] == code)
        .expect("fixture contains the actual section")
        * 81
}

#[test]
fn unlocatable_parameter_data_fails_its_actual_directory_callback_and_keeps_the_peer() {
    let original = points();
    let baseline = crate::test_support::decode(original.clone());
    let directory = section(&original, b'D') + 162;
    let parameter = section(&original, b'P');
    let mut bytes = original;
    bytes[directory + 8..directory + 16].copy_from_slice(b"       0");
    bytes[parameter + 81 + 64..parameter + 81 + 72].copy_from_slice(b"      99");
    let decoded = crate::test_support::decode(bytes.clone());
    assert_eq!(decoded.ir().model.points.len(), 1);
    assert_eq!(decoded.ir().model.vertices.len(), 1);
    assert_eq!(
        serde_json::to_value(&decoded.ir().model.points[0]).expect("serialize current point"),
        serde_json::to_value(&baseline.ir().model.points[0]).expect("serialize baseline point")
    );
    assert_eq!(
        serde_json::to_value(&decoded.ir().model.vertices[0]).expect("serialize current vertex"),
        serde_json::to_value(&baseline.ir().model.vertices[0]).expect("serialize baseline vertex")
    );
    assert_eq!(
        decoded.ir().source.as_ref().expect("source").attributes,
        baseline
            .ir()
            .source
            .as_ref()
            .expect("baseline source")
            .attributes
    );
    let native = decoded.ir().native.namespace("iges").expect("IGES native");
    let old = baseline
        .ir()
        .native
        .namespace("iges")
        .expect("baseline native");
    for arena in ["entities", "display_attributes"] {
        assert_eq!(native.arenas()[arena].len(), 1);
        assert_eq!(native.arenas()[arena][0], old.arenas()[arena][0]);
    }
    for arena in [
        "quarantined_directory_records",
        "quarantined_parameter_records",
    ] {
        assert!(native.arenas().get(arena).is_none_or(Vec::is_empty));
    }
    let unknowns = native
        .arena_as::<NativeUnknownRecord>("unknowns")
        .expect("typed unknowns");
    assert_eq!(unknowns.len(), 1);
    let fidelity = decoded.source_fidelity();
    let id = unknowns[0].id.as_str();
    let part = &fidelity.parts()[id];
    assert_eq!(part.level().as_str(), "iges.directory_entry");
    assert_eq!(part.state(), &PartState::GivenUp {});
    assert_eq!(part.offset(), u64::try_from(directory).expect("offset"));
    assert_eq!(part.byte_len(), 162);
    assert_eq!(
        fidelity
            .retained_record(id)
            .expect("one failure image")
            .data(),
        Some(&bytes[directory..directory + 162])
    );
    assert!(fidelity.record_layout()[id].reads().is_empty());
    assert!(!fidelity
        .record_layout()
        .contains_key("iges:entity:directory#3"));
    assert_eq!(
        fidelity.record_layout()["iges:entity:directory#1"],
        baseline.source_fidelity().record_layout()["iges:entity:directory#1"]
    );
    let peer = fidelity
        .parts()
        .values()
        .find(|part| {
            part.level().as_str() == "iges.parameter_record"
                && part.offset() == u64::try_from(parameter).expect("Parameter offset")
        })
        .expect("valid peer retains its actual Parameter part");
    assert_eq!(peer.byte_len(), 81);
    assert_eq!(peer.state(), &PartState::Decoded {});
    assert_eq!(
        fidelity
            .parts()
            .values()
            .filter(|part| part.state() == &PartState::GivenUp {})
            .count(),
        1
    );
    let unreadable: Vec<_> = decoded
        .report()
        .losses
        .iter()
        .filter(|loss| loss.code.local_code() == "record.unreadable")
        .collect();
    assert_eq!(unreadable.len(), 1);
    assert_eq!(unreadable[0].severity, cadmpeg_ir::report::Severity::Error);
    assert!(unreadable[0].message.contains("iges:entity:directory#3"));
    assert!(unreadable[0].message.contains("iges.directory_entry"));
    assert!(unreadable[0]
        .message
        .contains("cannot locate Parameter Data"));
    let stream = part.stream();
    cadmpeg_test_support::contract::parts::check_one_home(
        cadmpeg_ir::CodecFormat::Iges,
        &decoded.ir().native,
        fidelity,
        &decoded.report().losses,
        stream,
        |requested, range| {
            if requested != stream {
                return None;
            }
            let start = usize::try_from(range.start).ok()?;
            let end = usize::try_from(range.end).ok()?;
            bytes.get(start..end)
        },
    )
    .expect("actual Directory failure has one source home");
    assert!(cadmpeg_ir::validate_neutral(decoded.ir(), Vec::new())
        .expect("neutral validation")
        .is_ok());
}

#[test]
fn a_real_back_pointer_run_admits_a_directory_with_an_unusable_declaration() {
    let original = points();
    let baseline = crate::test_support::decode(original.clone());
    let directory = section(&original, b'D') + 162;
    let mut bytes = original;
    bytes[directory + 8..directory + 16].copy_from_slice(b"       0");
    let decoded = crate::test_support::decode(bytes);
    assert_eq!(
        serde_json::to_value(&decoded.ir().model.points).expect("current points"),
        serde_json::to_value(&baseline.ir().model.points).expect("baseline points")
    );
    let native = decoded.ir().native.namespace("iges").expect("IGES native");
    assert_eq!(native.arenas()["entities"].len(), 2);
    assert!(native
        .arena_as::<NativeUnknownRecord>("unknowns")
        .expect("typed unknowns")
        .is_empty());
    assert!(!decoded
        .report()
        .losses
        .iter()
        .any(|loss| loss.code.local_code() == "record.unreadable"));
    for sequence in [1, 3] {
        let id = format!("iges:entity:directory#{sequence}");
        let layout = &decoded.source_fidelity().record_layout()[&id];
        assert_eq!(layout.byte_len(), 81);
        assert!(!layout.reads().is_empty());
        assert!(
            layout.offset() >= u64::try_from(section(&points(), b'P')).expect("Parameter start")
        );
    }
}
