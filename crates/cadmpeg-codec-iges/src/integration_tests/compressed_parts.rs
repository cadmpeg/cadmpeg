// SPDX-License-Identifier: Apache-2.0

#[test]
fn compressed_bad_mandatory_field_preserves_the_original_later_peer() {
    recovered(false, 3, 1);
}

#[test]
fn compressed_failed_field_stays_in_force_until_the_actual_source_override() {
    recovered(true, 5, 2);
}

fn recovered(inherited: bool, ordinal: u32, failures: usize) {
    let original = crate::test_support::compressed_parts::points(false, inherited);
    let bytes = crate::test_support::compressed_parts::points(true, inherited);
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
    let expected = baseline
        .ir()
        .model
        .vertices
        .iter()
        .find(|record| record.id == vertex)
        .unwrap();
    assert_eq!(
        serde_json::to_value(&decoded.ir().model.vertices[0]).unwrap(),
        serde_json::to_value(expected).unwrap()
    );
    let native = decoded.ir().native.namespace("iges").unwrap();
    let id = format!("iges:entity:directory#{ordinal}");
    let expected = baseline.ir().native.namespace("iges").unwrap().arenas()["entities"]
        .iter()
        .find(|record| record.id() == id)
        .unwrap();
    assert_eq!(native.arenas()["entities"].len(), 1);
    assert_eq!(&native.arenas()["entities"][0], expected);
    assert!(native.arenas()["quarantined_directory_records"].is_empty());
    assert!(native.arenas()["quarantined_parameter_records"].is_empty());
    let parts = crate::test_support::given_up_parts(decoded.source_fidelity());
    assert_eq!(parts.len(), failures);
    assert_eq!(
        crate::test_support::given_up_count(decoded.report(), "iges.compressed_data_record"),
        failures
    );
    for part in parts {
        assert!(!part.untyped().is_empty());
        let begin = usize::try_from(part.offset()).unwrap();
        let end = begin + usize::try_from(part.byte_len()).unwrap();
        assert_eq!(part.data(), Some(&bytes[begin..end]));
    }
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

#[test]
fn compressed_missing_initial_structure_gives_up_only_its_known_counted_record() {
    missing_structure(false, 3, 1);
}

#[test]
fn compressed_partial_inheritance_is_repaired_only_by_the_actual_later_override() {
    missing_structure(true, 5, 2);
}

fn missing_structure(inherited: bool, ordinal: u32, failures: usize) {
    let (original, bytes) =
        crate::test_support::compressed_parts::without_initial_field(b"@3_0", inherited);
    assert_eq!(original.len() - bytes.len(), b"@3_0".len());
    let baseline = crate::test_support::decode(original);
    assert_eq!(
        baseline.ir().model.points.len(),
        if inherited { 3 } else { 2 }
    );
    let decoded = crate::test_support::decode(bytes.clone());
    let point = crate::ids::point(&crate::ids::Stem::directory(ordinal));
    let vertex = crate::ids::vertex(&crate::ids::Stem::directory(ordinal));
    assert_eq!(decoded.ir().model.points.len(), 1);
    assert_eq!(decoded.ir().model.vertices.len(), 1);
    assert_eq!(decoded.ir().model.points[0].id, point);
    assert_eq!(decoded.ir().model.vertices[0].id, vertex);
    assert_eq!(decoded.ir().model.vertices[0].point, point);
    assert_eq!(
        &decoded.ir().model.points[0],
        baseline
            .ir()
            .model
            .points
            .iter()
            .find(|record| record.id == point)
            .expect("same original later point")
    );
    assert_eq!(
        &decoded.ir().model.vertices[0],
        baseline
            .ir()
            .model
            .vertices
            .iter()
            .find(|record| record.id == vertex)
            .expect("same original later vertex")
    );
    let native = decoded.ir().native.namespace("iges").expect("native IGES");
    let id = format!("iges:entity:directory#{ordinal}");
    let expected = baseline
        .ir()
        .native
        .namespace("iges")
        .expect("baseline IGES")
        .arenas()["entities"]
        .iter()
        .find(|record| record.id() == id)
        .expect("same original Directory identity");
    assert_eq!(native.arenas()["entities"].len(), 1);
    assert_eq!(&native.arenas()["entities"][0], expected);
    assert!(native.arenas()["quarantined_directory_records"].is_empty());
    assert!(native.arenas()["quarantined_parameter_records"].is_empty());
    let parts = crate::test_support::given_up_parts(decoded.source_fidelity());
    assert_eq!(parts.len(), failures);
    assert_eq!(
        crate::test_support::given_up_count(decoded.report(), "iges.compressed_data_record"),
        failures
    );
    for part in parts {
        assert!(!part.untyped().is_empty());
        let begin = usize::try_from(part.offset()).expect("small offset");
        let end = begin + usize::try_from(part.byte_len()).expect("small length");
        assert_eq!(part.data(), Some(&bytes[begin..end]));
    }
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
        .expect("validation")
        .is_ok());
}

#[test]
fn compressed_missing_parameter_line_count_gives_up_its_actual_known_data_section() {
    let (original, bytes) =
        crate::test_support::compressed_parts::without_initial_field(b"@14_1", false);
    assert_eq!(original.len() - bytes.len(), b"@14_1".len());
    unknown_count(original, &bytes);
}

#[test]
fn compressed_unreadable_parameter_line_count_gives_up_its_actual_known_data_section() {
    let (original, bytes) = crate::test_support::compressed_parts::with_unreadable_initial_count();
    assert_eq!(original.len(), bytes.len());
    assert_eq!(
        original
            .iter()
            .zip(&bytes)
            .filter(|(left, right)| left != right)
            .count(),
        1
    );
    unknown_count(original, &bytes);
}

fn unknown_count(original: Vec<u8>, bytes: &[u8]) {
    let baseline = crate::test_support::decode(original);
    assert_eq!(baseline.ir().model.points.len(), 2);
    let mut offset = 0;
    let mut begin = None;
    let mut extent = None;
    for line in bytes.split_inclusive(|byte| *byte == b'\n') {
        if line.starts_with(b"D1@") {
            begin = Some(offset);
        }
        if line.get(72) == Some(&b'T') {
            extent = Some(begin.expect("actual first Data record")..offset);
            break;
        }
        offset += line.len();
    }
    let extent = extent.expect("original bounded Data section");
    let decoded = crate::test_support::decode(bytes.to_vec());
    assert!(decoded.ir().model.points.is_empty());
    assert!(decoded.ir().model.vertices.is_empty());
    assert_eq!(decoded.ir().tolerances, baseline.ir().tolerances);
    let native = decoded.ir().native.namespace("iges").expect("native IGES");
    assert!(native.arenas().get("entities").is_none_or(Vec::is_empty));
    let parts = crate::test_support::given_up_parts(decoded.source_fidelity());
    assert_eq!(parts.len(), 1);
    let part = parts[0];
    assert!(!part.untyped().is_empty());
    assert_eq!(
        crate::test_support::given_up_count(decoded.report(), "iges.compressed_data_section"),
        1
    );
    assert_eq!(
        part.offset(),
        u64::try_from(extent.start).expect("small source start")
    );
    assert_eq!(
        part.byte_len(),
        u64::try_from(extent.len()).expect("small source length")
    );
    assert_eq!(part.data(), Some(&bytes[extent]));
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
        .expect("validation")
        .is_ok());
}
