// SPDX-License-Identifier: Apache-2.0
use cadmpeg_ir::source_fidelity::parts::PartState;
use cadmpeg_ir::NativeUnknownRecord;

use crate::test_support::binary_parts::{
    broken_parameter_reference, broken_transform_chain, broken_transform_parameter,
    directory_admission_fault, DirectoryAdmissionFault,
};

#[test]
fn binary_format_five_parameter_reference_gives_up_its_owner_and_keeps_the_real_peer() {
    recovered_peer(
        broken_parameter_reference(false),
        broken_parameter_reference(true),
        3,
        2,
    );
}

#[test]
fn binary_transform_dependencies_drop_transitively_and_keep_the_independent_point_identity() {
    recovered_peer(
        broken_transform_chain(false),
        broken_transform_chain(true),
        5,
        4,
    );
}

#[test]
fn binary_late_transform_parameter_fault_drops_dependents_and_preserves_the_independent_peer() {
    recovered_peer(
        broken_transform_chain(false),
        broken_transform_parameter(),
        5,
        4,
    );
}

#[test]
fn binary_directory_mandatory_scalar_faults_preserve_the_independent_peer() {
    for fault in [
        DirectoryAdmissionFault::Structure,
        DirectoryAdmissionFault::Form,
    ] {
        let original = directory_admission_fault(fault, false);
        recovered_peer(original, directory_admission_fault(fault, true), 3, 2);
    }
}

#[test]
fn binary_directory_invalid_status_preserves_the_independent_peer() {
    recovered_peer(
        directory_admission_fault(DirectoryAdmissionFault::Status, false),
        directory_admission_fault(DirectoryAdmissionFault::Status, true),
        3,
        2,
    );
}

#[test]
fn binary_directory_invalid_entity_type_preserves_the_independent_peer() {
    recovered_peer(
        directory_admission_fault(DirectoryAdmissionFault::EntityType, false),
        directory_admission_fault(DirectoryAdmissionFault::EntityType, true),
        3,
        2,
    );
}

fn recovered_peer(original: Vec<u8>, bytes: Vec<u8>, ordinal: u32, failures: usize) {
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
    let target = format!("iges:entity:directory#{ordinal}");
    let expected = baseline.ir().native.namespace("iges").unwrap().arenas()["entities"]
        .iter()
        .find(|record| record.id() == target)
        .unwrap();
    assert_eq!(native.arenas()["entities"].len(), 1);
    assert_eq!(&native.arenas()["entities"][0], expected);
    assert!(native.arenas()["transformations"].is_empty());
    assert!(native.arenas()["quarantined_directory_records"].is_empty());
    assert!(native.arenas()["quarantined_parameter_records"].is_empty());
    let unknowns = native.arena_as::<NativeUnknownRecord>("unknowns").unwrap();
    assert_eq!(unknowns.len(), failures);
    let fidelity = decoded.source_fidelity();
    for record in &unknowns {
        let part = &fidelity.parts()[&record.id];
        assert_eq!(part.state(), &PartState::GivenUp {});
        assert!(matches!(
            part.level().as_str(),
            "iges.binary_directory_record" | "iges.binary_parameter_record"
        ));
        let begin = usize::try_from(part.offset()).unwrap();
        let end = begin + usize::try_from(part.byte_len()).unwrap();
        assert_eq!(
            fidelity.retained_record(record.id.as_str()).unwrap().data(),
            Some(&bytes[begin..end])
        );
        let layout = &fidelity.record_layout()[record.id.as_str()];
        assert_eq!(layout.offset(), part.offset());
        assert_eq!(layout.byte_len(), part.byte_len());
        assert!(layout.reads().is_empty());
    }
    let mut expected_keys: Vec<_> = unknowns.iter().map(|record| record.id.as_str()).collect();
    expected_keys.push(&target);
    expected_keys.sort_unstable();
    assert_eq!(
        fidelity
            .record_layout()
            .keys()
            .map(String::as_str)
            .collect::<Vec<_>>(),
        expected_keys
    );
    assert_eq!(fidelity.record_layout().len(), failures + 1);
    let (peer_stream, peer_offset, peer_len) =
        super::layout_oracle::peer_parameter(&bytes, ordinal);
    let peer_layout = &fidelity.record_layout()[&target];
    assert_eq!(peer_layout.stream(), peer_stream);
    assert_eq!(peer_layout.offset(), peer_offset);
    assert_eq!(peer_layout.byte_len(), peer_len);
    assert_eq!(
        peer_layout,
        &baseline.source_fidelity().record_layout()[&target]
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
