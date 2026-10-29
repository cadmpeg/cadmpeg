// SPDX-License-Identifier: Apache-2.0
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
    let first = bytes
        .chunks_exact(81)
        .position(|card| card[72] == b'P')
        .unwrap()
        * 81;
    let parts = crate::test_support::given_up_parts(decoded.source_fidelity());
    assert_eq!(parts.len(), 1);
    assert!(!parts[0].untyped().is_empty());
    assert_eq!(parts[0].data(), Some(&bytes[first..first + 81]));
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
    let mut homes = crate::test_support::given_up_parts(decoded.source_fidelity());
    assert_eq!(homes.len(), 2);
    homes.sort_unstable_by_key(|part| part.offset());
    for (ordinal, part) in homes.into_iter().enumerate() {
        let start = first + ordinal * 81;
        assert!(!part.untyped().is_empty());
        assert_eq!(part.offset(), u64::try_from(start).unwrap());
        assert_eq!(part.byte_len(), 81);
        assert_eq!(part.data(), Some(&bytes[start..start + 81]));
    }
    assert_eq!(
        crate::test_support::given_up_count(decoded.report(), "iges.parameter_record"),
        2
    );
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

    let start = bytes
        .chunks_exact(81)
        .position(|line| line[72] == b'P')
        .unwrap()
        * 81;
    let decoded = crate::test_support::decode(bytes.clone());
    let native = decoded.ir().native.namespace("iges").unwrap();
    assert_eq!(native.arenas()["entities"].len(), 2);
    assert!(decoded.ir().model.points.is_empty());
    assert!(native.arenas()["quarantined_parameter_records"].is_empty());
    let parts = crate::test_support::given_up_parts(decoded.source_fidelity());
    assert_eq!(parts.len(), 1);
    let part = parts[0];
    assert!(!part.untyped().is_empty());
    assert_eq!(
        crate::test_support::given_up_count(decoded.report(), "iges.parameter_record"),
        1
    );
    assert_eq!(part.stream(), "");
    assert_eq!(part.offset(), u64::try_from(start).unwrap());
    assert_eq!(part.byte_len(), 81);
    assert_eq!(part.data(), Some(&bytes[start..start + 81]));
    let unreadable: Vec<_> = decoded
        .report()
        .losses
        .iter()
        .filter(|loss| loss.code.local_code() == "record.unreadable")
        .collect();
    assert_eq!(unreadable.len(), 1);
    assert_eq!(unreadable[0].severity, cadmpeg_ir::report::Severity::Error);
    assert!(unreadable[0].message.contains("ownership-conflict"));
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
    let parts = crate::test_support::given_up_parts(decoded.source_fidelity());
    assert_eq!(parts.len(), 1);
    assert!(!parts[0].untyped().is_empty());
    let start = bytes
        .chunks_exact(81)
        .position(|line| line[72] == b'P')
        .unwrap()
        * 81;
    assert_eq!(parts[0].data(), Some(&bytes[start + 8..start + 15]));
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
        let fidelity = decoded.source_fidelity();
        let parts = crate::test_support::given_up_parts(fidelity);
        assert_eq!(parts.len(), 1);
        let part = parts[0];
        assert!(!part.untyped().is_empty());
        assert_eq!(
            crate::test_support::given_up_count(decoded.report(), "iges.parameter_field"),
            1
        );
        assert_eq!(part.offset(), 4);
        assert_eq!(part.byte_len(), u64::try_from(x.len() + 1).unwrap());
        assert_eq!(part.data(), Some(format!("{x},").as_bytes()));
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

#[test]
fn unequal_overlapping_parameter_claims_give_up_only_the_proved_section_ancestor() {
    for (first_entry, first_count, second_start, second_count) in
        [(0, 1, 1, 2), (0, 2, 2, 1), (1, 1, 2, 2)]
    {
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
                label: "DISJOINT".into(),
                status: "00000000",
                parameters: "116,7,8,9,0;".into(),
            },
        ]);
        let original = bytes.clone();
        let baseline = crate::test_support::decode(original.clone());
        assert_eq!(baseline.ir().model.points.len(), 3);
        let directory = bytes
            .chunks_exact(81)
            .position(|card| card[72] == b'D')
            .unwrap()
            * 81;
        let start = bytes
            .chunks_exact(81)
            .position(|card| card[72] == b'P')
            .unwrap()
            * 81;
        let end = start + 3 * 81;
        let first_directory = directory + first_entry * 162;
        bytes[first_directory + 81 + 24..first_directory + 81 + 32]
            .copy_from_slice(format!("{first_count:>8}").as_bytes());
        bytes[first_directory + 162 + 8..first_directory + 162 + 16]
            .copy_from_slice(format!("{second_start:>8}").as_bytes());
        bytes[first_directory + 243 + 24..first_directory + 243 + 32]
            .copy_from_slice(format!("{second_count:>8}").as_bytes());
        assert_eq!(&bytes[start..end], &original[start..end]);
        assert_eq!(&bytes[..directory], &original[..directory]);
        assert_eq!(&bytes[end..], &original[end..]);

        let decoded = crate::test_support::decode(bytes.clone());
        assert_eq!(
            serde_json::to_value(&decoded.ir().source).unwrap(),
            serde_json::to_value(&baseline.ir().source).unwrap()
        );
        assert!(decoded.ir().model.points.is_empty());
        assert!(decoded.ir().model.vertices.is_empty());
        let native = decoded.ir().native.namespace("iges").unwrap();
        for arena in [
            "quarantined_directory_records",
            "quarantined_parameter_records",
        ] {
            assert!(native.arenas().get(arena).is_none_or(Vec::is_empty));
        }
        let mut expected_entities = serde_json::to_value(
            &baseline.ir().native.namespace("iges").unwrap().arenas()["entities"],
        )
        .unwrap();
        let mut counts = [1, 1, 1];
        counts[first_entry] = first_count;
        counts[first_entry + 1] = second_count;
        for (entity, count) in expected_entities
            .as_array_mut()
            .unwrap()
            .iter_mut()
            .zip(counts)
        {
            entity["parameter_line_count"] = serde_json::json!(count);
            entity["parameters"] = serde_json::json!([]);
            entity["comment"] = serde_json::to_value(cadmpeg_ir::native::bytes::NativeBytes::from(
                Vec::<u8>::new(),
            ))
            .unwrap();
        }
        assert_eq!(
            serde_json::to_value(&native.arenas()["entities"]).unwrap(),
            expected_entities
        );
        assert_eq!(
            native.arenas()["display_attributes"],
            baseline.ir().native.namespace("iges").unwrap().arenas()["display_attributes"]
        );
        let fidelity = decoded.source_fidelity();
        let parts = crate::test_support::given_up_parts(fidelity);
        assert_eq!(parts.len(), 1);
        let part = parts[0];
        assert!(!part.untyped().is_empty());
        assert_eq!(part.offset(), u64::try_from(start).unwrap());
        assert_eq!(part.byte_len(), u64::try_from(end - start).unwrap());
        assert_eq!(part.data(), Some(&bytes[start..end]));
        assert_eq!(
            fidelity
                .retained_records()
                .keys()
                .filter(|id| id.as_str() != crate::SOURCE_IMAGE_ID)
                .count(),
            1,
        );
        assert_eq!(
            fidelity
                .retained_record(crate::SOURCE_IMAGE_ID)
                .unwrap()
                .data(),
            Some(bytes.as_slice()),
        );
        assert_eq!(
            baseline
                .source_fidelity()
                .retained_record(crate::SOURCE_IMAGE_ID)
                .unwrap()
                .data(),
            Some(original.as_slice()),
        );
        let unreadable: Vec<_> = decoded
            .report()
            .losses
            .iter()
            .filter(|loss| loss.code.local_code() == "record.unreadable")
            .collect();
        assert_eq!(unreadable.len(), 1);
        assert_eq!(unreadable[0].severity, cadmpeg_ir::report::Severity::Error);
        assert!(unreadable[0].message.contains("iges.parameter_section"));
        assert!(unreadable[0]
            .message
            .contains("unequal Parameter claims overlap"));
        assert!(decoded.report().losses.iter().all(|loss| loss.code
            != crate::loss::IgesLossCode::ParameterDataQuarantined.kind()
            && loss.code != crate::loss::IgesLossCode::GeometryNotProjected.kind()));
        assert!(cadmpeg_ir::validate_neutral(decoded.ir(), Vec::new())
            .unwrap()
            .is_ok());
    }
}

#[test]
fn a_missing_declared_parameter_owner_has_one_actual_unclaimed_section_home() {
    let original = owned_test_file(&[OwnedTestEntity {
        entity_type: 116,
        form: 0,
        label: "POINT".into(),
        status: "00000000",
        parameters: "116,1,2,3,0;".into(),
    }]);
    let baseline = crate::test_support::decode(original.clone());
    assert_eq!(baseline.ir().model.points.len(), 1);
    let mut bytes = original.clone();
    let directory = bytes
        .chunks_exact(81)
        .position(|card| card[72] == b'D')
        .unwrap()
        * 81;
    let start = bytes
        .chunks_exact(81)
        .position(|card| card[72] == b'P')
        .unwrap()
        * 81;
    bytes[directory + 81 + 24..directory + 81 + 32].copy_from_slice(b"       4");
    bytes[start + 64..start + 72].copy_from_slice(b"      99");
    assert_eq!(&bytes[..directory], &original[..directory]);
    assert_eq!(&bytes[start..start + 64], &original[start..start + 64]);
    assert_eq!(&bytes[start + 72..], &original[start + 72..]);

    let decoded = crate::test_support::decode(bytes.clone());
    assert_eq!(
        serde_json::to_value(&decoded.ir().source).unwrap(),
        serde_json::to_value(&baseline.ir().source).unwrap()
    );
    assert!(decoded.ir().model.points.is_empty());
    assert!(decoded.ir().model.vertices.is_empty());
    let native = decoded.ir().native.namespace("iges").unwrap();
    for name in [
        "quarantined_directory_records",
        "quarantined_parameter_records",
    ] {
        assert!(native.arenas().get(name).is_none_or(Vec::is_empty));
    }
    let mut expected =
        serde_json::to_value(&baseline.ir().native.namespace("iges").unwrap().arenas()["entities"])
            .unwrap();
    expected[0]["parameter_line_count"] = serde_json::json!(4);
    expected[0]["parameters"] = serde_json::json!([]);
    expected[0]["comment"] = serde_json::to_value(cadmpeg_ir::native::bytes::NativeBytes::from(
        Vec::<u8>::new(),
    ))
    .unwrap();
    assert_eq!(
        serde_json::to_value(&native.arenas()["entities"]).unwrap(),
        expected
    );
    assert_eq!(
        native.arenas()["display_attributes"],
        baseline.ir().native.namespace("iges").unwrap().arenas()["display_attributes"]
    );
    let fidelity = decoded.source_fidelity();
    let parts = crate::test_support::given_up_parts(fidelity);
    assert_eq!(parts.len(), 1);
    let part = parts[0];
    assert!(!part.untyped().is_empty());
    assert_eq!(part.offset(), u64::try_from(start).unwrap());
    assert_eq!(part.byte_len(), 81);
    assert_eq!(part.data(), Some(&bytes[start..start + 81]));
    assert_eq!(
        fidelity
            .retained_records()
            .keys()
            .filter(|id| id.as_str() != crate::SOURCE_IMAGE_ID)
            .count(),
        1,
    );
    assert_eq!(
        fidelity
            .retained_record(crate::SOURCE_IMAGE_ID)
            .unwrap()
            .data(),
        Some(bytes.as_slice()),
    );
    assert_eq!(
        baseline
            .source_fidelity()
            .retained_record(crate::SOURCE_IMAGE_ID)
            .unwrap()
            .data(),
        Some(original.as_slice()),
    );
    let losses: Vec<_> = decoded
        .report()
        .losses
        .iter()
        .filter(|loss| loss.code.local_code() == "record.unreadable")
        .collect();
    assert_eq!(losses.len(), 1);
    assert_eq!(losses[0].severity, cadmpeg_ir::report::Severity::Error);
    assert!(losses[0].message.contains("iges.parameter_section"));
    assert!(losses[0].message.contains("locate no Parameter cards"));
    assert!(decoded.report().losses.iter().all(|loss| loss.code
        != crate::loss::IgesLossCode::ParameterDataQuarantined.kind()
        && loss.code != crate::loss::IgesLossCode::GeometryNotProjected.kind()));
    assert!(cadmpeg_ir::validate_neutral(decoded.ir(), Vec::new())
        .unwrap()
        .is_ok());
}

#[test]
fn an_unlocated_parameter_owner_does_not_discard_a_current_bounded_peer() {
    let mut bytes = points("116,1,2,3,0;");
    let baseline = crate::test_support::decode(bytes.clone());
    let directory = bytes
        .chunks_exact(81)
        .position(|card| card[72] == b'D')
        .unwrap()
        * 81;
    let start = bytes
        .chunks_exact(81)
        .position(|card| card[72] == b'P')
        .unwrap()
        * 81;
    bytes[directory + 162 + 8..directory + 162 + 16].copy_from_slice(b"       0");
    bytes[start + 81 + 64..start + 81 + 72].copy_from_slice(b"      99");
    let decoded = crate::test_support::decode(bytes);
    assert_eq!(decoded.ir().model.points.len(), 1);
    assert_eq!(
        serde_json::to_value(&decoded.ir().model.points[0]).unwrap(),
        serde_json::to_value(&baseline.ir().model.points[0]).unwrap()
    );
    assert_eq!(
        decoded.ir().native.namespace("iges").unwrap().arenas()["entities"].len(),
        2
    );
    assert!(crate::test_support::given_up_parts(decoded.source_fidelity()).is_empty());
    assert!(cadmpeg_ir::validate_neutral(decoded.ir(), Vec::new())
        .unwrap()
        .is_ok());
}
