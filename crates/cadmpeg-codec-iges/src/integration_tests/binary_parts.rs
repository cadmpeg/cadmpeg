// SPDX-License-Identifier: Apache-2.0

use crate::test_support::binary_parts::{malformed_directory, point_after_void, point_pair};

#[test]
fn binary_bounded_directory_failure_keeps_real_later_point_and_one_exact_source_home() {
    let original = point_after_void(false);
    let baseline = crate::test_support::decode(original.clone());
    let failed = malformed_directory();
    let bytes = point_after_void(true);
    assert_eq!(original.len(), bytes.len());
    assert_eq!(
        original.iter().zip(&bytes).filter(|(a, b)| a != b).count(),
        1
    );
    let decoded = crate::test_support::decode(bytes.clone());
    assert_eq!(decoded.ir().model.points.len(), 1);
    assert_eq!(decoded.ir().model.vertices.len(), 1);
    let source_point = crate::ids::point(&crate::ids::Stem::directory(3_u32));
    assert_eq!(decoded.ir().model.points[0].id, source_point);
    assert_eq!(decoded.ir().model.vertices[0].point, source_point);
    assert_eq!(
        serde_json::to_value(&decoded.ir().model.points).unwrap(),
        serde_json::to_value(&baseline.ir().model.points).unwrap()
    );
    assert_eq!(
        serde_json::to_value(&decoded.ir().model.vertices).unwrap(),
        serde_json::to_value(&baseline.ir().model.vertices).unwrap()
    );
    let native = decoded.ir().native.namespace("iges").unwrap();
    let baseline_native = baseline.ir().native.namespace("iges").unwrap();
    let expected_native = baseline_native.arenas()["entities"]
        .iter()
        .find(|record| record.id() == "iges:entity:directory#3")
        .unwrap();
    assert_eq!(native.arenas()["entities"].len(), 1);
    assert_eq!(&native.arenas()["entities"][0], expected_native);
    let expected_display: Vec<_> = baseline_native.arenas()["display_attributes"]
        .iter()
        .filter(|record| record.id().ends_with("#D3"))
        .collect();
    let actual_display: Vec<_> = native.arenas()["display_attributes"].iter().collect();
    assert_eq!(actual_display, expected_display);
    assert!(native.arenas()["quarantined_directory_records"].is_empty());
    assert!(native.arenas()["quarantined_parameter_records"].is_empty());
    let fidelity = decoded.source_fidelity();
    let parts = crate::test_support::given_up_parts(fidelity);
    assert_eq!(parts.len(), 1);
    let rejected = parts[0];
    assert!(!rejected.untyped().is_empty());
    assert_eq!(rejected.byte_len(), u64::try_from(failed.len()).unwrap());
    let start = usize::try_from(rejected.offset()).unwrap();
    assert_eq!(&bytes[start..start + failed.len()], failed.as_slice());
    assert_eq!(rejected.data(), Some(failed.as_slice()));
    assert_eq!(rejected.stream(), "");
    let losses: Vec<_> = decoded
        .report()
        .losses
        .iter()
        .filter(|loss| loss.code.local_code() == "record.unreadable")
        .collect();
    assert_eq!(losses.len(), 1);
    assert_eq!(losses[0].severity, cadmpeg_ir::report::Severity::Error);
    assert!(losses[0].message.contains("iges.binary_directory_record"));
    assert!(cadmpeg_ir::validate_neutral(decoded.ir(), Vec::new())
        .unwrap()
        .is_ok());
}

#[test]
fn binary_failed_directory_and_its_bounded_parameter_keep_the_independent_current_pair() {
    let original = point_pair(false);
    let baseline = crate::test_support::decode(original.clone());
    let bytes = point_pair(true);
    assert_eq!(original.len(), bytes.len());
    assert_eq!(
        original.iter().zip(&bytes).filter(|(a, b)| a != b).count(),
        1
    );
    let decoded = crate::test_support::decode(bytes.clone());
    let point_id = crate::ids::point(&crate::ids::Stem::directory(3_u32));
    let vertex_id = crate::ids::vertex(&crate::ids::Stem::directory(3_u32));
    assert_eq!(decoded.ir().model.points.len(), 1);
    assert_eq!(decoded.ir().model.vertices.len(), 1);
    assert_eq!(decoded.ir().model.points[0].id, point_id);
    assert_eq!(decoded.ir().model.vertices[0].id, vertex_id);
    assert_eq!(decoded.ir().model.vertices[0].point, point_id);
    let baseline_point = baseline
        .ir()
        .model
        .points
        .iter()
        .find(|point| point.id == point_id)
        .unwrap();
    let baseline_vertex = baseline
        .ir()
        .model
        .vertices
        .iter()
        .find(|vertex| vertex.id == vertex_id)
        .unwrap();
    assert_eq!(
        serde_json::to_value(&decoded.ir().model.points[0]).unwrap(),
        serde_json::to_value(baseline_point).unwrap()
    );
    assert_eq!(
        serde_json::to_value(&decoded.ir().model.vertices[0]).unwrap(),
        serde_json::to_value(baseline_vertex).unwrap()
    );
    let native = decoded.ir().native.namespace("iges").unwrap();
    let expected = baseline.ir().native.namespace("iges").unwrap().arenas()["entities"]
        .iter()
        .find(|record| record.id() == "iges:entity:directory#3")
        .unwrap();
    assert_eq!(native.arenas()["entities"].len(), 1);
    assert_eq!(&native.arenas()["entities"][0], expected);
    let fidelity = decoded.source_fidelity();
    let parts = crate::test_support::given_up_parts(fidelity);
    assert_eq!(parts.len(), 2);
    for part in parts {
        assert!(!part.untyped().is_empty());
        let start = usize::try_from(part.offset()).unwrap();
        let end = start + usize::try_from(part.byte_len()).unwrap();
        assert_eq!(part.data(), Some(&bytes[start..end]));
    }
    for level in [
        "iges.binary_directory_record",
        "iges.binary_parameter_record",
    ] {
        assert_eq!(
            crate::test_support::given_up_count(decoded.report(), level),
            1
        );
    }
    assert_eq!(
        decoded
            .report()
            .losses
            .iter()
            .filter(|loss| loss.code.local_code() == "record.unreadable")
            .count(),
        2
    );
    assert!(native.arenas()["quarantined_directory_records"].is_empty());
    assert!(native.arenas()["quarantined_parameter_records"].is_empty());
    assert!(cadmpeg_ir::validate_neutral(decoded.ir(), Vec::new())
        .unwrap()
        .is_ok());
}

mod reference_recovery;
