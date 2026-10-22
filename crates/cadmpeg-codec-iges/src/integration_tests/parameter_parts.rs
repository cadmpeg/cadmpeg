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

#[test]
fn point_semantic_part_keeps_exact_peer_placement_and_native_pipeline() {
    let baseline = crate::test_support::decode(points("116,1,2,3,0;"));
    let bytes = points("116,1,2,broken,0;");
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
    assert_eq!(
        peer.position().get(),
        cadmpeg_ir::math::Point3::new(4.0, 5.0, 6.0)
    );
    assert_eq!(decoded.ir().model.vertices.len(), 1);
    assert_eq!(decoded.ir().model.vertices[0].point, peer.id);
    let native = decoded.ir().native.namespace("iges").unwrap();
    assert_eq!(native.arenas()["entities"].len(), 2);
    let unknowns = native.arena_as::<NativeUnknownRecord>("unknowns").unwrap();
    assert_eq!(unknowns.len(), 1);
    let fidelity = decoded.source_fidelity();
    let part = &fidelity.parts()[&unknowns[0].id];
    assert_eq!(
        part.state(),
        &cadmpeg_ir::source_fidelity::parts::PartState::GivenUp {}
    );
    let start = bytes
        .chunks_exact(81)
        .position(|line| line[72] == b'P')
        .unwrap()
        * 81;
    assert_eq!(
        fidelity
            .retained_record(unknowns[0].id.as_str())
            .unwrap()
            .data(),
        Some(&bytes[start + 8..start + 15])
    );
    assert_eq!(
        decoded
            .report()
            .losses
            .iter()
            .filter(|loss| loss.code.local_code() == "record.unreadable")
            .count(),
        1
    );
    assert!(cadmpeg_ir::validate_neutral(decoded.ir(), Vec::new())
        .unwrap()
        .is_ok());
}

#[test]
fn coordinate_field_failure_keeps_other_native_fields_and_an_unchanged_peer() {
    let original_bytes = points("116,1,2,3,0;");
    let baseline = crate::test_support::decode(original_bytes.clone());
    let mut bytes = original_bytes.clone();
    let start = bytes
        .chunks_exact(81)
        .position(|card| card[72] == b'P')
        .unwrap()
        * 81;
    bytes[start + 8] = 0xff;
    assert_eq!(bytes.len(), original_bytes.len());
    assert_eq!(
        bytes
            .iter()
            .zip(&original_bytes)
            .enumerate()
            .filter_map(|(index, (current, original))| (current != original).then_some(index))
            .collect::<Vec<_>>(),
        [start + 8]
    );
    let decoded = crate::test_support::decode(bytes.clone());
    crate::test_support::parameter_parts::assert_one_parameter_field(&decoded, &bytes, 8, b"\xff,");
    let native = decoded.ir().native.namespace("iges").unwrap();
    let old = baseline.ir().native.namespace("iges").unwrap();
    let mut first = serde_json::to_value(&native.arenas()["entities"][0]).unwrap();
    let mut old_first = serde_json::to_value(&old.arenas()["entities"][0]).unwrap();
    assert!(first["parameters"][3].is_null());
    first["parameters"][3] = serde_json::Value::Null;
    old_first["parameters"][3] = serde_json::Value::Null;
    assert_eq!(first, old_first);
    assert_eq!(
        serde_json::to_value(&native.arenas()["entities"][1]).unwrap(),
        serde_json::to_value(&old.arenas()["entities"][1]).unwrap()
    );
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
    assert_eq!(decoded.ir().model.vertices.len(), 1);
    let vertex = &decoded.ir().model.vertices[0];
    let old_vertex = baseline
        .ir()
        .model
        .vertices
        .iter()
        .find(|item| item.id == vertex.id)
        .unwrap();
    assert_eq!(
        serde_json::to_value(vertex).unwrap(),
        serde_json::to_value(old_vertex).unwrap()
    );
}

#[test]
fn unreadable_display_literal_keeps_exact_geometry_and_other_native_fields() {
    let mut bytes = points("116,1,2,3,0;");
    let baseline = crate::test_support::decode(bytes.clone());
    let start = bytes
        .chunks_exact(81)
        .position(|card| card[72] == b'P')
        .unwrap()
        * 81;
    bytes[start + 10] = 0xff;
    let decoded = crate::test_support::decode(bytes.clone());
    crate::test_support::parameter_parts::assert_one_parameter_field(
        &decoded, &bytes, 10, b"\xff;",
    );
    assert_eq!(
        serde_json::to_value(&decoded.ir().model).unwrap(),
        serde_json::to_value(&baseline.ir().model).unwrap()
    );
    let current = decoded.ir().native.namespace("iges").unwrap();
    let original = baseline.ir().native.namespace("iges").unwrap();
    let mut current_first = current.arenas()["entities"][0].fields().unwrap();
    let mut old_first = original.arenas()["entities"][0].fields().unwrap();
    assert!(current_first["parameters"][4].is_null());
    current_first["parameters"][4] = serde_json::Value::Null;
    old_first["parameters"][4] = serde_json::Value::Null;
    assert_eq!(current_first, old_first);
    assert_eq!(
        serde_json::to_value(&current.arenas()["entities"][1]).unwrap(),
        serde_json::to_value(&original.arenas()["entities"][1]).unwrap()
    );
}

#[test]
fn cross_card_point_numeric_fields_keep_typed_neighbors_and_an_exact_peer() {
    use crate::test_support::test_owned::owned_test_file_with_raw_parameters;
    use cadmpeg_ir::source_fidelity::parts::PartState;

    for zeroes in [59, 60] {
        let mut x = "0".repeat(zeroes);
        x.push('1');
        let entities = |parameters: String| {
            [
                OwnedTestEntity {
                    entity_type: 116,
                    form: 0,
                    label: "SPLIT".into(),
                    status: "00010000",
                    parameters,
                },
                OwnedTestEntity {
                    entity_type: 116,
                    form: 0,
                    label: "PEER".into(),
                    status: "00000000",
                    parameters: "116,4,5,6,0;".into(),
                },
            ]
        };
        let baseline = crate::test_support::decode(owned_test_file_with_raw_parameters(&entities(
            "116,1,2,3,0;".into(),
        )));
        let bytes = owned_test_file_with_raw_parameters(&entities(format!("116,{x},2,3,0;")));
        let decoded = crate::test_support::decode(bytes.clone());
        let first = bytes
            .chunks_exact(81)
            .position(|card| card[72] == b'P')
            .unwrap()
            * 81;
        assert_eq!(&bytes[first + 4..first + 64], &x.as_bytes()[..60]);
        let native = decoded.ir().native.namespace("iges").unwrap();
        assert_eq!(native.arenas()["entities"].len(), 2);
        let fields = native.arenas()["entities"][0].fields().unwrap();
        let parameters = &fields["parameters"];
        assert_eq!(parameters.as_array().unwrap().len(), 5);
        assert_eq!(parameters[0]["value"]["value"], 116);
        assert!(parameters[1].is_null());
        for (index, value) in [(2, 2), (3, 3), (4, 0)] {
            assert_eq!(parameters[index]["value"]["value"], value);
        }
        assert_eq!(
            serde_json::to_value(&native.arenas()["entities"][1]).unwrap(),
            serde_json::to_value(
                &baseline.ir().native.namespace("iges").unwrap().arenas()["entities"][1]
            )
            .unwrap()
        );
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
        assert_eq!(decoded.ir().model.vertices.len(), 1);
        let vertex = &decoded.ir().model.vertices[0];
        let original_vertex = baseline
            .ir()
            .model
            .vertices
            .iter()
            .find(|item| item.id == vertex.id)
            .unwrap();
        assert_eq!(
            serde_json::to_value(vertex).unwrap(),
            serde_json::to_value(original_vertex).unwrap()
        );
        let unknowns = native.arena_as::<NativeUnknownRecord>("unknowns").unwrap();
        assert_eq!(unknowns.len(), 1);
        let fidelity = decoded.source_fidelity();
        let part = &fidelity.parts()[&unknowns[0].id];
        assert_eq!(part.level().as_str(), "iges.parameter_field");
        assert_eq!(part.state(), &PartState::GivenUp {});
        assert_eq!(part.offset(), 4);
        assert_eq!(part.byte_len(), u64::try_from(x.len() + 1).unwrap());
        assert_eq!(
            part.framing_bytes(),
            &[u64::try_from(x.len() + 4).unwrap()..u64::try_from(x.len() + 5).unwrap()]
        );
        assert_eq!(
            fidelity
                .retained_record(unknowns[0].id.as_str())
                .unwrap()
                .data(),
            Some(format!("{x},").as_bytes())
        );
        let parent = &fidelity.parts()[part.parent().unwrap()];
        assert_eq!(parent.level().as_str(), "iges.parameter_record");
        assert_eq!(parent.state(), &PartState::Decoded {});
        assert_eq!(parent.offset(), u64::try_from(first).unwrap());
        assert_eq!(parent.byte_len(), 162);
        assert_eq!(
            fidelity
                .parts()
                .values()
                .filter(|part| part.state() == &PartState::GivenUp {})
                .count(),
            1
        );
        assert_eq!(decoded.report().losses.len(), 1);
        assert_eq!(
            decoded.report().losses[0].code.local_code(),
            "record.unreadable"
        );
        assert_eq!(
            decoded.report().losses[0].severity,
            cadmpeg_ir::report::Severity::Error
        );
        assert!(decoded.report().losses[0]
            .message
            .contains("numeric field or its delimiter crosses a card boundary"));
        assert!(cadmpeg_ir::validate_neutral(decoded.ir(), Vec::new())
            .unwrap()
            .is_ok());
    }
}
