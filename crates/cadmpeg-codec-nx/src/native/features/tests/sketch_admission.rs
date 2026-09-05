// SPDX-License-Identifier: Apache-2.0
#![allow(unused_imports)]

use super::*;

#[test]
fn sketch_fixed_points_require_one_owned_finite_point_pair() {
    let name = FeatureSketchPayloadName {
        id: "name".to_string(),
        operation_label: "sketch".to_string(),
        construction_payload: "payload".to_string(),
        ordinal: 0,
        type_code: Some(FeaturePayloadTypeCode {
            value: 1,
            raw: vec![1],
            payload_offset: 1,
            source_offset: Some(1001),
        }),
        value: "Point1".to_string(),
        payload_offset: 0,
        source_offset: 1000,
    };
    let record = FeatureSketchPayloadNamedRecord {
        id: "record".to_string(),
        operation_label: "sketch".to_string(),
        construction_payload: "payload".to_string(),
        name_field: name.id.clone(),
        scalar_fields: Vec::new(),
        fixed_pairs: vec!["pair-1".to_string(), "pair-2".to_string()],
        mixed_pairs: Vec::new(),
        payload_start_offset: 0,
        payload_end_offset: 100,
    };
    let pair = |id: &str, discriminator: u8| FeatureSketchPayloadFixedPair {
        id: id.to_string(),
        operation_label: "sketch".to_string(),
        construction_payload: "payload".to_string(),
        ordinal: 0,
        values: [0.5, -0.5],
        raw_values: [[0; 7]; 2],
        discriminator: vec![discriminator],
        payload_offset: 20,
        value_payload_offsets: [28, 37],
        source_offset: 1020,
        value_source_offsets: [1028, 1037],
    };
    let pairs = [pair("pair-1", 0x04), pair("pair-2", 0x08)];
    assert!(feature_sketch_fixed_points(&[record], std::slice::from_ref(&name), &pairs).is_empty());

    let mut foreign = pairs[0].clone();
    foreign.id = "foreign".to_string();
    foreign.operation_label = "other-operation".to_string();
    let record = FeatureSketchPayloadNamedRecord {
        id: "record".to_string(),
        operation_label: "sketch".to_string(),
        construction_payload: "payload".to_string(),
        name_field: "name".to_string(),
        scalar_fields: Vec::new(),
        fixed_pairs: vec![foreign.id.clone()],
        mixed_pairs: Vec::new(),
        payload_start_offset: 0,
        payload_end_offset: 100,
    };
    assert!(feature_sketch_fixed_points(&[record], &[name], &[foreign]).is_empty());
}

#[test]
fn sketch_points_require_owned_finite_scalar_fields() {
    let name = FeatureSketchPayloadName {
        id: "name".to_string(),
        operation_label: "sketch".to_string(),
        construction_payload: "payload".to_string(),
        ordinal: 0,
        type_code: Some(FeaturePayloadTypeCode {
            value: 1,
            raw: vec![1],
            payload_offset: 1,
            source_offset: Some(1001),
        }),
        value: "Point1".to_string(),
        payload_offset: 0,
        source_offset: 1000,
    };
    let record = FeatureSketchPayloadNamedRecord {
        id: "record".to_string(),
        operation_label: "sketch".to_string(),
        construction_payload: "payload".to_string(),
        name_field: name.id.clone(),
        scalar_fields: vec!["scalar-1".to_string(), "scalar-2".to_string()],
        fixed_pairs: Vec::new(),
        mixed_pairs: Vec::new(),
        payload_start_offset: 0,
        payload_end_offset: 100,
    };
    let scalar = |id: &str, value: f64| FeatureSketchPayloadScalar {
        id: id.to_string(),
        operation_label: "sketch".to_string(),
        construction_payload: "payload".to_string(),
        ordinal: 0,
        field_code: 100,
        value,
        raw_value: [0; 8],
        payload_offset: 20,
        source_offset: 1020,
    };
    let scalars = [scalar("scalar-1", 1.0), scalar("scalar-2", 2.0)];
    assert_eq!(
        feature_sketch_points(
            std::slice::from_ref(&record),
            std::slice::from_ref(&name),
            &scalars
        )
        .len(),
        1
    );

    let mut foreign = scalars[0].clone();
    foreign.id = "foreign".to_string();
    foreign.operation_label = "other-operation".to_string();
    let foreign_record = FeatureSketchPayloadNamedRecord {
        scalar_fields: vec![foreign.id.clone(), scalars[1].id.clone()],
        ..record.clone()
    };
    assert!(feature_sketch_points(
        &[foreign_record],
        std::slice::from_ref(&name),
        &[foreign, scalars[1].clone()]
    )
    .is_empty());

    let mut nonfinite = scalars;
    nonfinite[1].value = f64::NAN;
    assert!(feature_sketch_points(&[record], &[name], &nonfinite).is_empty());
}
