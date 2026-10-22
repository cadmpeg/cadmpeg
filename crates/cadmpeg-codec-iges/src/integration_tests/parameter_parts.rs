// SPDX-License-Identifier: Apache-2.0
use cadmpeg_ir::NativeUnknownRecord;

use crate::test_support::test_owned::{owned_test_file, OwnedTestEntity};

fn points(first: &str) -> Vec<u8> {
    owned_test_file(&[
        OwnedTestEntity {
            entity_type: 116,
            form: 0,
            label: "FIRST".into(),
            status: "00000000",
            parameters: first.into(),
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

#[test]
fn public_parameter_failure_keeps_one_shared_source_home_and_checked_peer_content() {
    let baseline = crate::test_support::decode(points("116,1,2,3,0;"));
    let bytes = points("110,1,2,3,0;");
    let decoded = crate::test_support::decode(bytes.clone());
    assert_eq!(decoded.ir().model.points.len(), 1);
    let peer = &decoded.ir().model.points[0];
    let original = baseline
        .ir()
        .model
        .points
        .iter()
        .find(|point| point.id == peer.id)
        .unwrap();
    assert_eq!(
        serde_json::to_value(peer).unwrap(),
        serde_json::to_value(original).unwrap()
    );
    let native = decoded.ir().native.namespace("iges").unwrap();
    assert!(native
        .arenas()
        .get("quarantined_parameter_records")
        .is_none_or(Vec::is_empty));
    let unknowns = native.arena_as::<NativeUnknownRecord>("unknowns").unwrap();
    assert_eq!(unknowns.len(), 1);
    let first = bytes
        .chunks_exact(81)
        .position(|card| card[72] == b'P')
        .unwrap()
        * 81;
    let fidelity = decoded.source_fidelity();
    assert_eq!(
        fidelity
            .retained_record(unknowns[0].id.as_str())
            .unwrap()
            .data(),
        Some(&bytes[first..first + 81])
    );
    assert!(!fidelity.record_layout()["iges:entity:directory#3"]
        .reads()
        .is_empty());
    assert!(!fidelity
        .record_layout()
        .contains_key("iges:entity:directory#1"));
    assert_eq!(
        decoded
            .report()
            .losses
            .iter()
            .filter(|loss| loss.code.local_code() == "record.unreadable")
            .count(),
        1
    );
    assert!(decoded
        .report()
        .losses
        .iter()
        .all(|loss| loss.code != crate::loss::IgesLossCode::ParameterDataQuarantined.kind()));
}

#[test]
fn conflicted_parameter_parts_keep_an_unrelated_current_point_and_exact_source_homes() {
    let mut bytes = owned_test_file(&[
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
        OwnedTestEntity {
            entity_type: 116,
            form: 0,
            label: "PEER".into(),
            status: "00000000",
            parameters: "116,7,8,9,0;".into(),
        },
    ]);
    let baseline = crate::test_support::decode(bytes.clone());
    let first = bytes
        .chunks_exact(81)
        .position(|card| card[72] == b'P')
        .unwrap()
        * 81;
    bytes[first + 64..first + 72].copy_from_slice(b"       3");
    bytes[first + 81 + 64..first + 81 + 72].copy_from_slice(b"       1");
    let decoded = crate::test_support::decode(bytes.clone());
    assert_eq!(decoded.ir().model.points.len(), 1);
    let peer = &decoded.ir().model.points[0];
    let original = baseline
        .ir()
        .model
        .points
        .iter()
        .find(|point| point.id == peer.id)
        .unwrap();
    assert_eq!(
        serde_json::to_value(peer).unwrap(),
        serde_json::to_value(original).unwrap()
    );
    let native = decoded.ir().native.namespace("iges").unwrap();
    assert!(native.arenas()["quarantined_parameter_records"].is_empty());
    let unknowns = native.arena_as::<NativeUnknownRecord>("unknowns").unwrap();
    assert_eq!(unknowns.len(), 2);
    let mut homes: Vec<_> = unknowns
        .iter()
        .map(|unknown| (unknown, &decoded.source_fidelity().parts()[&unknown.id]))
        .collect();
    homes.sort_unstable_by_key(|(_, part)| part.offset());
    for (ordinal, (unknown, part)) in homes.into_iter().enumerate() {
        let start = first + ordinal * 81;
        assert_eq!(part.level().as_str(), "iges.parameter_record");
        assert_eq!(
            part.state(),
            &cadmpeg_ir::source_fidelity::parts::PartState::GivenUp {}
        );
        assert_eq!(part.offset(), u64::try_from(start).unwrap());
        assert_eq!(part.byte_len(), 81);
        assert_eq!(
            decoded
                .source_fidelity()
                .retained_record(unknown.id.as_str())
                .unwrap()
                .data(),
            Some(&bytes[start..start + 81])
        );
    }
    assert!(
        decoded.source_fidelity().record_layout()["iges:entity:directory#5"]
            .reads()
            .values()
            .any(|ranges| !ranges.is_empty())
    );
    for id in ["iges:entity:directory#1", "iges:entity:directory#3"] {
        assert!(!decoded.source_fidelity().record_layout().contains_key(id));
    }
    let unreadable: Vec<_> = decoded
        .report()
        .losses
        .iter()
        .filter(|loss| loss.code.local_code() == "record.unreadable")
        .collect();
    assert_eq!(unreadable.len(), 2);
    assert!(unreadable
        .iter()
        .all(|loss| loss.severity == cadmpeg_ir::report::Severity::Error
            && loss.message.contains("ownership-conflict")));
    assert!(decoded
        .report()
        .losses
        .iter()
        .all(|loss| loss.code != crate::loss::IgesLossCode::ParameterDataQuarantined.kind()));
}

#[test]
fn original_duplicate_claims_match_one_independent_physical_frame_and_one_helper_home() {
    let mut bytes = owned_test_file(&[
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
    ]);
    let start_field = bytes
        .chunks_exact(81)
        .position(|line| line[72] == b'D')
        .map(|first| (first + 2) * 81 + 8)
        .expect("Directory card");
    bytes[start_field..start_field + 8].copy_from_slice(b"       1");

    let arena = cadmpeg_core::decode::DecodeArena::new();
    let (ctx, root) = cadmpeg_core::decode::DecodeContext::from_root_bytes(
        &bytes,
        &arena,
        &cadmpeg_core::decode::DecodePolicy::service(),
    )
    .unwrap();
    let mut frames = Vec::new();
    let claims =
        cadmpeg_test_support::contract::iges::frame_parameter_owners(&ctx, root, &mut frames)
            .unwrap();
    assert_eq!(claims.len(), 2);
    assert_eq!(claims[0].record(), claims[1].record());
    assert_eq!(claims[1].directory().start(), claims[0].directory().end());
    assert_eq!(&claims[0].directory().window()[8..16], b"       1");
    assert_eq!(&claims[1].directory().window()[8..16], b"       1");
    assert_eq!(
        frames
            .iter()
            .filter(|frame| frame.level().as_str() == "iges.parameter_record")
            .count(),
        1
    );
    let frame = &frames[claims[0].record()];
    let decoded = crate::test_support::decode(bytes.clone());
    let native = decoded.ir().native.namespace("iges").unwrap();
    assert_eq!(native.arenas()["entities"].len(), 2);
    assert!(decoded.ir().model.points.is_empty());
    assert!(native.arenas()["quarantined_parameter_records"].is_empty());
    let unknowns = native.arena_as::<NativeUnknownRecord>("unknowns").unwrap();
    assert_eq!(unknowns.len(), 1);
    let fidelity = decoded.source_fidelity();
    let part = &fidelity.parts()[&unknowns[0].id];
    assert_eq!(part.level(), frame.level());
    assert_eq!(part.stream(), frame.capture().stream());
    assert_eq!(part.offset(), frame.capture().offset());
    assert_eq!(part.byte_len(), frame.capture().byte_len());
    assert_eq!(part.framing_bytes(), frame.framing());
    assert_eq!(
        part.state(),
        &cadmpeg_ir::source_fidelity::parts::PartState::GivenUp {}
    );
    assert_eq!(
        fidelity
            .retained_record(unknowns[0].id.as_str())
            .unwrap()
            .data(),
        Some(frame.view().window())
    );
    let unreadable: Vec<_> = decoded
        .report()
        .losses
        .iter()
        .filter(|loss| loss.code.local_code() == "record.unreadable")
        .collect();
    assert_eq!(unreadable.len(), 1);
    assert_eq!(unreadable[0].severity, cadmpeg_ir::report::Severity::Error);
    assert!(unreadable[0].message.contains("ownership-conflict"));
    assert!(ctx.take_record_captures().unwrap().is_empty());
}
