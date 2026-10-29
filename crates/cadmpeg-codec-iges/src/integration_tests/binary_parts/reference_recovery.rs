// SPDX-License-Identifier: Apache-2.0

use crate::test_support::binary_parts::{
    broken_parameter_reference, broken_transform_chain, broken_transform_parameter,
    directory_admission_fault, DirectoryAdmissionFault,
};

/// The records a damaged decode keeps beside its independent peer.
struct Recovery<'a> {
    /// Directory ordinal of the independent peer.
    peer: u32,
    /// Given-up parts, each with one `record.unreadable` loss.
    failures: usize,
    /// Directory records whose Parameter Data record is given up. They keep
    /// their Directory fields with no parameters (#215 rule 1).
    detached: &'a [u32],
    /// Unchanged native entities whose neutral record needs a record that a
    /// detached entry no longer forms, and is dropped (#215 rule 2).
    dependents: &'a [u32],
}

#[test]
fn binary_format_five_parameter_reference_gives_up_its_parameter_record_and_keeps_the_real_peer() {
    recovered_peer(
        broken_parameter_reference(false),
        broken_parameter_reference(true),
        &Recovery {
            peer: 3,
            failures: 1,
            detached: &[1],
            dependents: &[],
        },
    );
}

#[test]
fn binary_transform_dependencies_drop_transitively_and_keep_the_independent_point_identity() {
    recovered_peer(
        broken_transform_chain(false),
        broken_transform_chain(true),
        &Recovery {
            peer: 5,
            failures: 4,
            detached: &[],
            dependents: &[],
        },
    );
}

#[test]
fn binary_late_transform_value_fault_gives_up_only_its_value_part_and_drops_the_dependent_projection(
) {
    let original = broken_transform_chain(false);
    let bytes = broken_transform_parameter();
    assert_eq!(original.len(), bytes.len());
    assert_eq!(
        original.iter().zip(&bytes).filter(|(a, b)| a != b).count(),
        1
    );
    // The one changed byte is the control byte of the Transformation
    // Matrix's last value, T3 (coefficient 12). Its format 7 has no width,
    // so that 5-byte group, the last of the record, is one given-up
    // `iges.binary_parameter_value`, and T3 is `null`. The record keeps its
    // other 11 coefficients (iges.md, Binary Parameter Data; #215 rule 1).
    let control = original
        .iter()
        .zip(&bytes)
        .position(|(a, b)| a != b)
        .unwrap();
    let given_up = control..control + 5;
    let baseline = crate::test_support::decode(original);
    let decoded = crate::test_support::decode(bytes.clone());

    // The point D3 needs the complete transformation, so its neutral record
    // is dropped (#215 rule 2) and charges `geometry.not-projected` (iges.md,
    // a missing required geometry field). The peer D5 is unchanged.
    let peer = crate::ids::point(&crate::ids::Stem::directory(5_u32));
    let vertex = crate::ids::vertex(&crate::ids::Stem::directory(5_u32));
    assert_eq!(baseline.ir().model.points.len(), 2);
    assert_eq!(decoded.ir().model.points.len(), 1);
    assert_eq!(decoded.ir().model.points[0].id, peer);
    assert_eq!(
        serde_json::to_value(&decoded.ir().model.points[0]).unwrap(),
        serde_json::to_value(
            baseline
                .ir()
                .model
                .points
                .iter()
                .find(|point| point.id == peer)
                .unwrap()
        )
        .unwrap()
    );
    assert_eq!(decoded.ir().model.vertices.len(), 1);
    assert_eq!(decoded.ir().model.vertices[0].id, vertex);

    let native = decoded.ir().native.namespace("iges").unwrap();
    let old = baseline.ir().native.namespace("iges").unwrap();
    let entities = &native.arenas()["entities"];
    let old_entities = &old.arenas()["entities"];
    assert_eq!(
        entities
            .iter()
            .map(|record| record.id())
            .collect::<Vec<_>>(),
        old_entities
            .iter()
            .map(|record| record.id())
            .collect::<Vec<_>>()
    );
    assert_eq!(
        crate::test_support::binary_parts::entity_without_padding(&entities[0]),
        crate::test_support::binary_parts::entity_with_given_up_rest(&old_entities[0], 11, 1)
    );
    assert_eq!(entities[1..], old_entities[1..]);
    let mut transformations = serde_json::to_value(&old.arenas()["transformations"]).unwrap();
    transformations[0]["coefficients"][11] = serde_json::Value::Null;
    assert_eq!(
        serde_json::to_value(&native.arenas()["transformations"]).unwrap(),
        transformations
    );
    assert!(native.arenas()["quarantined_parameter_records"].is_empty());

    let fidelity = decoded.source_fidelity();
    let parts = crate::test_support::given_up_parts(fidelity);
    assert_eq!(parts.len(), 1);
    let part = parts[0];
    assert!(!part.untyped().is_empty());
    assert_eq!(
        crate::test_support::given_up_count(decoded.report(), "iges.binary_parameter_value"),
        1
    );
    assert_eq!(part.offset(), u64::try_from(given_up.start).unwrap());
    assert_eq!(part.byte_len(), u64::try_from(given_up.len()).unwrap());
    assert_eq!(part.data(), Some(&bytes[given_up]));
    let codes = |result: &cadmpeg_ir::codec::DecodeResult| {
        let mut codes: Vec<_> = result
            .report()
            .losses
            .iter()
            .map(|loss| loss.code.local_code().to_owned())
            .collect();
        codes.sort_unstable();
        codes
    };
    let mut expected = codes(&baseline);
    expected.extend(["geometry.not-projected", "record.unreadable"].map(str::to_owned));
    expected.sort_unstable();
    assert_eq!(codes(&decoded), expected);
    assert!(decoded.report().losses.iter().any(|loss| {
        loss.code.local_code() == "geometry.not-projected" && loss.message.contains("D1")
    }));
    assert!(cadmpeg_ir::validate_neutral(decoded.ir(), Vec::new())
        .unwrap()
        .is_ok());
}

#[test]
fn binary_directory_mandatory_scalar_faults_preserve_the_independent_peer() {
    for fault in [
        DirectoryAdmissionFault::Structure,
        DirectoryAdmissionFault::Form,
    ] {
        let original = directory_admission_fault(fault, false);
        recovered_peer(
            original,
            directory_admission_fault(fault, true),
            &DIRECTORY_FAULT,
        );
    }
}

#[test]
fn binary_directory_invalid_status_preserves_the_independent_peer() {
    recovered_peer(
        directory_admission_fault(DirectoryAdmissionFault::Status, false),
        directory_admission_fault(DirectoryAdmissionFault::Status, true),
        &DIRECTORY_FAULT,
    );
}

#[test]
fn binary_directory_invalid_entity_type_preserves_the_independent_peer() {
    recovered_peer(
        directory_admission_fault(DirectoryAdmissionFault::EntityType, false),
        directory_admission_fault(DirectoryAdmissionFault::EntityType, true),
        &DIRECTORY_FAULT,
    );
}

/// A failed Directory record and the Parameter record that needs it.
const DIRECTORY_FAULT: Recovery<'static> = Recovery {
    peer: 3,
    failures: 2,
    detached: &[],
    dependents: &[],
};

fn recovered_peer(original: Vec<u8>, bytes: Vec<u8>, recovery: &Recovery<'_>) {
    let Recovery {
        peer: ordinal,
        failures,
        detached,
        dependents,
    } = *recovery;
    assert_eq!(original.len(), bytes.len());
    assert_eq!(
        original.iter().zip(&bytes).filter(|(a, b)| a != b).count(),
        1
    );
    let baseline = crate::test_support::decode(original);
    let decoded = crate::test_support::decode(bytes.clone());
    let point = crate::ids::point(&crate::ids::Stem::directory(ordinal));
    let vertex = crate::ids::vertex(&crate::ids::Stem::directory(ordinal));
    assert_eq!(decoded.ir().model.points.len(), 1);
    assert_eq!(decoded.ir().model.vertices.len(), 1);
    assert_eq!(decoded.ir().model.points[0].id, point);
    assert_eq!(decoded.ir().model.vertices[0].id, vertex);
    assert_eq!(decoded.ir().model.vertices[0].point, point);
    let expected = baseline
        .ir()
        .model
        .points
        .iter()
        .find(|record| record.id == point)
        .unwrap();
    assert_eq!(
        serde_json::to_value(&decoded.ir().model.points[0]).unwrap(),
        serde_json::to_value(expected).unwrap()
    );
    let expected_vertex = baseline
        .ir()
        .model
        .vertices
        .iter()
        .find(|record| record.id == vertex)
        .unwrap();
    assert_eq!(
        serde_json::to_value(&decoded.ir().model.vertices[0]).unwrap(),
        serde_json::to_value(expected_vertex).unwrap()
    );
    let native = decoded.ir().native.namespace("iges").unwrap();
    let old = baseline.ir().native.namespace("iges").unwrap();
    let entity = |ordinal: u32| format!("iges:entity:directory#{ordinal}");
    let mut kept: Vec<_> = detached
        .iter()
        .chain(dependents)
        .copied()
        .chain([ordinal])
        .collect();
    kept.sort_unstable();
    assert_eq!(
        native.arenas()["entities"]
            .iter()
            .map(|record| record.id().to_owned())
            .collect::<Vec<_>>(),
        kept.iter()
            .map(|ordinal| entity(*ordinal))
            .collect::<Vec<_>>()
    );
    for record in &native.arenas()["entities"] {
        let expected = old.arenas()["entities"]
            .iter()
            .find(|baseline| baseline.id() == record.id())
            .unwrap();
        if detached
            .iter()
            .any(|ordinal| entity(*ordinal) == record.id())
        {
            assert_eq!(
                serde_json::to_value(record).unwrap(),
                crate::test_support::binary_parts::entity_without_parameter_data(expected)
            );
        } else {
            assert_eq!(record, expected);
        }
    }
    // A detached Transformation Matrix keeps its record; its positional
    // coefficient list holds `null` for every value (#215 rule 1).
    let transformations: Vec<_> = old
        .arenas()
        .get("transformations")
        .into_iter()
        .flatten()
        .filter(|record| {
            detached.iter().any(|ordinal| {
                record.field("source_entity") == Some(serde_json::json!(entity(*ordinal)))
            })
        })
        .map(|record| {
            let mut value = serde_json::to_value(record).unwrap();
            value["coefficients"] = serde_json::Value::Array(vec![serde_json::Value::Null; 12]);
            value
        })
        .collect();
    assert_eq!(
        serde_json::to_value(&native.arenas()["transformations"]).unwrap(),
        serde_json::Value::Array(transformations)
    );
    assert!(native.arenas()["quarantined_directory_records"].is_empty());
    assert!(native.arenas()["quarantined_parameter_records"].is_empty());
    let fidelity = decoded.source_fidelity();
    let parts = crate::test_support::given_up_parts(fidelity);
    assert_eq!(parts.len(), failures);
    for part in parts {
        assert!(!part.untyped().is_empty());
        let begin = usize::try_from(part.offset()).unwrap();
        let end = begin + usize::try_from(part.byte_len()).unwrap();
        assert_eq!(part.data(), Some(&bytes[begin..end]));
    }
    assert_eq!(
        crate::test_support::given_up_count(decoded.report(), "iges.binary_directory_record")
            + crate::test_support::given_up_count(decoded.report(), "iges.binary_parameter_record"),
        failures
    );
    assert_eq!(
        decoded
            .report()
            .losses
            .iter()
            .filter(|loss| loss.code.local_code() == "record.unreadable")
            .count(),
        failures
    );
    assert!(cadmpeg_ir::validate_neutral(decoded.ir(), Vec::new())
        .unwrap()
        .is_ok());
}
