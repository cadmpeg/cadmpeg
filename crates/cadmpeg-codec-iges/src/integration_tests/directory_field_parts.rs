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

fn first_directory(bytes: &[u8]) -> usize {
    bytes
        .chunks_exact(81)
        .position(|card| card[72] == b'D')
        .expect("real Directory card")
        * 81
}

#[test]
fn each_optional_directory_field_has_one_exact_failure_home_and_keeps_both_real_points() {
    let original = points();
    let first = first_directory(&original);
    for (relative, field) in [
        (24, "line_font"),
        (32, "level"),
        (40, "view"),
        (56, "label_display"),
        (81 + 8, "line_weight"),
        (81 + 16, "color"),
        (81 + 64, "subscript"),
    ] {
        let start = first + relative;
        let mut good = original.clone();
        good[start..start + 8].copy_from_slice(b"       0");
        let baseline = crate::test_support::decode(good.clone());
        let mut bad = good.clone();
        bad[start + 7] = b'x';
        assert_eq!(good.len(), bad.len());
        assert_eq!(good.iter().zip(&bad).filter(|(a, b)| a != b).count(), 1);
        let decoded = crate::test_support::decode(bad.clone());
        assert_eq!(decoded.ir().model.points.len(), 2);
        assert_eq!(
            serde_json::to_value(&decoded.ir().model.points).expect("point values"),
            serde_json::to_value(&baseline.ir().model.points).expect("baseline points")
        );
        let native = decoded.ir().native.namespace("iges").expect("native IGES");
        let baseline_native = baseline
            .ir()
            .native
            .namespace("iges")
            .expect("baseline IGES");
        assert_eq!(native.arenas()["entities"].len(), 2);
        let id = "iges:entity:directory#1";
        let entity = native.arenas()["entities"]
            .iter()
            .find(|record| record.id() == id)
            .expect("field owner");
        let expected = baseline_native.arenas()["entities"]
            .iter()
            .find(|record| record.id() == id)
            .expect("original owner");
        let actual = serde_json::to_value(entity).expect("native entity");
        let mut expected = serde_json::to_value(expected).expect("original native entity");
        assert_eq!(expected[field], 0);
        expected[field] = serde_json::Value::Null;
        assert_eq!(actual, expected);
        let peer = |namespace: &cadmpeg_ir::NativeNamespace| {
            serde_json::to_value(
                namespace.arenas()["entities"]
                    .iter()
                    .find(|record| record.id() == "iges:entity:directory#3")
                    .expect("independent peer"),
            )
            .expect("peer value")
        };
        assert_eq!(peer(native), peer(baseline_native));
        let unknowns = native
            .arena_as::<NativeUnknownRecord>("unknowns")
            .expect("shared unknowns");
        assert_eq!(unknowns.len(), 1);
        assert_eq!(
            unknowns[0]
                .links
                .iter()
                .map(|link| link.as_str())
                .collect::<Vec<_>>(),
            [entity.id()]
        );
        let fidelity = decoded.source_fidelity();
        let unknown = unknowns[0].id.as_str();
        let part = &fidelity.parts()[unknown];
        assert_eq!(part.level().as_str(), "iges.directory_field");
        assert_eq!(part.state(), &PartState::GivenUp {});
        assert_eq!(part.stream(), "file");
        assert_eq!(part.offset(), u64::try_from(start).expect("source offset"));
        assert_eq!(part.byte_len(), 8);
        assert!(part.framing_bytes().is_empty());
        let parent = &fidelity.parts()[part.parent().expect("actual Directory parent")];
        assert_eq!(parent.level().as_str(), "iges.directory_entry");
        assert_eq!(parent.state(), &PartState::Decoded {});
        assert_eq!(parent.offset(), u64::try_from(first).expect("entry offset"));
        assert_eq!(
            fidelity
                .retained_record(unknown)
                .expect("one retained field")
                .data(),
            Some(&bad[start..start + 8])
        );
        assert_eq!(
            fidelity
                .retained_records()
                .keys()
                .filter(|key| key.as_str() == unknown)
                .count(),
            1
        );
        let layout = &fidelity.record_layout()[unknown];
        assert_eq!(layout.stream(), part.stream());
        assert_eq!(layout.offset(), part.offset());
        assert_eq!(layout.byte_len(), 8);
        assert!(layout.reads().is_empty());
        assert_eq!(
            fidelity.record_layout()["iges:entity:directory#3"],
            baseline.source_fidelity().record_layout()["iges:entity:directory#3"]
        );
        let losses: Vec<_> = decoded
            .report()
            .losses
            .iter()
            .filter(|loss| loss.code.local_code() == "record.unreadable")
            .collect();
        assert_eq!(losses.len(), 1);
        assert_eq!(losses[0].severity, cadmpeg_ir::report::Severity::Error);
        assert!(losses[0].message.contains("iges.directory_field"));
        assert!(
            !decoded
                .report()
                .losses
                .iter()
                .any(|loss| loss.code
                    == crate::loss::IgesLossCode::DirectoryMetadataUnreadable.kind())
        );
        assert!(native.arenas()["quarantined_directory_records"].is_empty());
        assert!(native.arenas()["quarantined_parameter_records"].is_empty());
        assert!(cadmpeg_ir::validate_neutral(decoded.ir(), Vec::new())
            .expect("neutral validity")
            .is_ok());
        assert_eq!(good[start..start + 8], *b"       0");
        assert_eq!(bad[start..start + 8], *b"       x");
    }
}

#[test]
fn actual_optional_directory_scalar_source_flip_changes_only_its_current_native_field() {
    let original = points();
    let offset = first_directory(&original) + 81 + 8;
    let mut outputs = Vec::new();
    for value in [2, 3] {
        let mut source = original.clone();
        source[offset..offset + 8].copy_from_slice(if value == 2 {
            b"       2"
        } else {
            b"       3"
        });
        let decoded = crate::test_support::decode(source.clone());
        let native = decoded.ir().native.namespace("iges").expect("native IGES");
        let owner = native.arenas()["entities"]
            .iter()
            .find(|record| record.id() == "iges:entity:directory#1")
            .expect("actual owner");
        let mut owner = serde_json::to_value(owner).expect("native owner");
        assert_eq!(owner["line_weight"], value);
        owner["line_weight"] = serde_json::Value::Null;
        assert_eq!(decoded.ir().model.points.len(), 2);
        assert!(!decoded
            .report()
            .losses
            .iter()
            .any(|loss| loss.code.local_code() == "record.unreadable"));
        let fields: Vec<_> = decoded
            .source_fidelity()
            .parts()
            .values()
            .filter(|part| part.level().as_str() == "iges.directory_field")
            .collect();
        assert_eq!(fields.len(), 14);
        assert!(fields
            .iter()
            .all(|part| part.state() == &PartState::Decoded {}));
        outputs.push(owner);
        assert_eq!(source.len(), original.len());
    }
    assert_eq!(outputs[0], outputs[1]);
}
