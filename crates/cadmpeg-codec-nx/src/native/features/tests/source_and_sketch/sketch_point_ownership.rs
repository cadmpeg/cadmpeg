use super::*;
use crate::native::features::payload_name::FeaturePayloadName;
use crate::om::scalar_pair::{PairPosition, SketchPairForm};

#[test]
fn sketch_named_records_own_fixed_pairs_within_their_intervals() {
    use super::super::{
        feature_sketch_fixed_points, feature_sketch_payload_named_records,
        FeatureConstructionPayload, FeatureSketchPayloadFixedPair,
    };
    let payload = FeatureConstructionPayload {
        id: "payload".to_string(),
        operation_label: "sketch".to_string(),
        owner: crate::native::features::FeatureConstructionOwner::Sketch {
            construction_inputs: "inputs".to_string(),
        },
        content: crate::native::features::payload_content::FeaturePayloadContent::new(
            vec![
                crate::native::features::payload_content::FeaturePayloadBlock {
                    id: "block".to_string(),
                    byte_len: 100,
                    source_offset: 1000,
                },
            ],
            crate::native::hex::Sha256Hex::digest(b"00"),
        )
        .unwrap(),
    };
    let name = |id: &str, ordinal, offset| FeaturePayloadName {
        id: id.to_string(),
        operation_label: "sketch".to_string(),
        construction_payload: "payload".to_string(),
        ordinal,
        frame: crate::om::name_field::NameField::new(
            format!("Point{}", ordinal + 1),
            offset,
            Some(crate::om::compact::CompactIndexTarget {
                atom: crate::om::compact::CompactIndexAtom::from_wire(1, &[1]).unwrap(),
                target: Some(1001 + offset),
            }),
        )
        .unwrap(),
        source_offset: 1000 + offset,
    };
    let pair = FeatureSketchPayloadFixedPair {
        id: "pair".to_string(),
        operation_label: "sketch".to_string(),
        construction_payload: "payload".to_string(),
        ordinal: 0,
        values: [[0; 7], [8, 0, 0, 0, 0, 0, 0]]
            .map(crate::om::sketch_scalar::SketchScaledAtom::from_raw),
        position: PairPosition::new(SketchPairForm::Legacy, 20).unwrap(),
        source_offset: 1020,
        value_source_offsets: [1028, 1037],
    };

    let names = [name("first", 0, 10), name("second", 1, 50)];
    let auxiliary_pair = FeatureSketchPayloadFixedPair {
        id: "auxiliary-pair".to_string(),
        ordinal: 1,
        position: PairPosition::new(SketchPairForm::ThreeMember, 40).unwrap(),
        source_offset: 1040,
        value_source_offsets: [1055, 1064],
        ..pair.clone()
    };
    let pairs = [pair, auxiliary_pair];
    let records = feature_sketch_payload_named_records(&[payload], &names, &[], &pairs, &[]);
    assert_eq!(records[0].fixed_pairs, ["pair", "auxiliary-pair"]);
    assert!(records[1].fixed_pairs.is_empty());
    let points = feature_sketch_fixed_points(&records, &names, &pairs);
    assert_eq!(points.len(), 1);
    assert_eq!(points[0].name, "Point1");
    assert_eq!(points[0].values, [0.5, 0.75]);
}

#[test]
fn sketch_named_point_block_uses_require_exact_shared_block_identity() {
    use super::super::{
        feature_sketch_named_point_block_uses, FeatureSketchReference, OffsetStoreNamedPoint,
    };

    let point = OffsetStoreNamedPoint {
        id: "nx:offset-store:named-point#2-10".to_string(),
        name: "Point1".to_string(),
        data_blocks: vec!["block-10".to_string(), "block-11".to_string()],
        values: [(1.0, 100), (2.0, 120)].map(|(value, source_offset)| {
            crate::native::features::FeatureBinary64ScalarToken {
                scalar: crate::om::scalar::ShiftedBinary64::try_from(shifted_f64_bytes(value))
                    .unwrap(),
                source_offset,
            }
        }),
        source_offset: 90,
    };
    let reference =
        |id: &str, ordinal: u32, count: u8, block: Option<&str>| FeatureSketchReference {
            id: id.to_string(),
            operation_label: "nx:feature-history:operation-label#1-4".to_string(),
            position: crate::om::sketch_references::SketchReferencePosition::new(count, ordinal)
                .unwrap(),
            token: crate::om::reference_index::ReferenceIndexToken::from_wire(
                10 + ordinal,
                &[0xf0, (10 + ordinal) as u8],
            )
            .unwrap(),
            data_block: block.map(str::to_string),
            source_offset: 200 + u64::from(ordinal),
        };
    let uses = feature_sketch_named_point_block_uses(
        &[
            reference("miss", 0, 2, Some("block-9")),
            reference("hit", 1, 2, Some("block-11")),
            reference("unresolved", 2, 3, None),
        ],
        &[point],
    );
    assert_eq!(uses.len(), 1);
    assert_eq!(uses[0].sketch_reference, "hit");
    assert_eq!(uses[0].reference_ordinal, 1);
    assert_eq!(uses[0].point_block_ordinal, 1);
    assert_eq!(uses[0].data_block, "block-11");
}

#[test]
fn sketch_preceding_named_point_uses_require_a_complete_unique_consecutive_lane() {
    use super::super::{
        feature_sketch_preceding_named_point_uses, FeatureSketchReference, OffsetStoreNamedPoint,
    };

    let reference = |ordinal, declared_count, block: Option<&str>| FeatureSketchReference {
        id: format!("reference-{ordinal}"),
        operation_label: "nx:feature-history:operation-label#1-4".to_string(),
        position: crate::om::sketch_references::SketchReferencePosition::new(
            declared_count,
            ordinal,
        )
        .unwrap(),
        token: crate::om::reference_index::ReferenceIndexToken::from_wire(
            12 + ordinal,
            &[0xf0, (12 + ordinal) as u8],
        )
        .unwrap(),
        data_block: block.map(str::to_string),
        source_offset: 300 + u64::from(ordinal),
    };
    let references = [
        reference(0, 2, Some("nx:om-data-blocks-2:block#12")),
        reference(1, 2, Some("nx:om-data-blocks-2:block#13")),
    ];
    let point = |id: &str, blocks: &[&str]| OffsetStoreNamedPoint {
        id: id.to_string(),
        name: "Point1".to_string(),
        data_blocks: blocks.iter().map(|block| (*block).to_string()).collect(),
        values: [(1.0, 200), (2.0, 220)].map(|(value, source_offset)| {
            crate::native::features::FeatureBinary64ScalarToken {
                scalar: crate::om::scalar::ShiftedBinary64::try_from(shifted_f64_bytes(value))
                    .unwrap(),
                source_offset,
            }
        }),
        source_offset: 190,
    };
    let preceding = point(
        "nx:offset-store:named-point#2-10",
        &[
            "nx:om-data-blocks-2:block#10",
            "nx:om-data-blocks-2:block#11",
        ],
    );
    let uses =
        feature_sketch_preceding_named_point_uses(&references, std::slice::from_ref(&preceding));
    assert_eq!(uses.len(), 1);
    assert_eq!(uses[0].first_sketch_reference, references[0].id);
    assert_eq!(uses[0].named_point, preceding.id);
    assert_eq!(uses[0].following_data_block, "nx:om-data-blocks-2:block#12");

    let ambiguous = point(
        "nx:offset-store:named-point#2-11",
        &["nx:om-data-blocks-2:block#11"],
    );
    assert!(feature_sketch_preceding_named_point_uses(
        &references,
        &[preceding.clone(), ambiguous]
    )
    .is_empty());
    let gap = point(
        "nx:offset-store:named-point#2-9",
        &["nx:om-data-blocks-2:block#9"],
    );
    let other_store = point(
        "nx:offset-store:named-point#3-11",
        &["nx:om-data-blocks-3:block#11"],
    );
    assert!(feature_sketch_preceding_named_point_uses(&references, &[gap, other_store]).is_empty());

    let unresolved = [references[0].clone(), reference(1, 2, None)];
    assert!(feature_sketch_preceding_named_point_uses(
        &unresolved,
        std::slice::from_ref(&preceding)
    )
    .is_empty());
    let noncontiguous = [
        references[0].clone(),
        reference(2, 3, Some("nx:om-data-blocks-2:block#13")),
    ];
    assert!(feature_sketch_preceding_named_point_uses(
        &noncontiguous,
        std::slice::from_ref(&preceding),
    )
    .is_empty());
    let mut bad_terminal = serde_json::to_value(&references[1]).unwrap();
    bad_terminal["terminal"] = serde_json::json!(false);
    assert!(
        serde_json::from_value::<FeatureSketchReference>(bad_terminal)
            .unwrap_err()
            .to_string()
            .contains("terminal")
    );
}

#[test]
fn sketch_point_uses_retain_identical_witnesses_and_reject_conflicts() {
    use super::super::{
        feature_sketch_point_groups, feature_sketch_point_uses, FeatureSketchNamedPointBlockUse,
        FeatureSketchPoint, OffsetStoreNamedPoint,
    };

    let operation_label = "nx:feature-history:operation-label#1-4".to_string();
    let point = FeatureSketchPoint {
        id: "payload-point".to_string(),
        operation_label: operation_label.clone(),
        named_record: "named-record".to_string(),
        name: "Point1".to_string(),
        coordinates: [1.0, 2.0],
        scalar_fields: ["scalar-1".to_string(), "scalar-2".to_string()],
    };
    let named_point = OffsetStoreNamedPoint {
        id: "named-point".to_string(),
        name: "Point1".to_string(),
        data_blocks: vec!["block-10".to_string()],
        values: [(1.0, 200), (2.0, 220)].map(|(value, source_offset)| {
            crate::native::features::FeatureBinary64ScalarToken {
                scalar: crate::om::scalar::ShiftedBinary64::try_from(shifted_f64_bytes(value))
                    .unwrap(),
                source_offset,
            }
        }),
        source_offset: 190,
    };
    let block_use = FeatureSketchNamedPointBlockUse {
        id: "nx:feature-history:sketch-named-point-block-use#1-4-0".to_string(),
        operation_label,
        sketch_reference: "reference".to_string(),
        reference_ordinal: 0,
        named_point: named_point.id.clone(),
        data_block: "block-10".to_string(),
        point_block_ordinal: 0,
        source_offset: 300,
    };
    let mut second_block_use = block_use.clone();
    second_block_use.id = "nx:feature-history:sketch-named-point-block-use#1-4-1".to_string();
    second_block_use.sketch_reference = "reference-2".to_string();
    second_block_use.reference_ordinal = 1;
    second_block_use.source_offset = 301;

    let groups = feature_sketch_point_groups(std::slice::from_ref(&point));
    let uses = feature_sketch_point_uses(
        &groups,
        std::slice::from_ref(&named_point),
        &[second_block_use.clone(), block_use.clone()],
    );
    assert_eq!(uses.len(), 1);
    assert_eq!(uses[0].sketch_point_group, groups[0].id);
    assert_eq!(uses[0].named_point, named_point.id);
    assert_eq!(
        uses[0]
            .references
            .iter()
            .map(|reference| reference.sketch_reference.as_str())
            .collect::<Vec<_>>(),
        ["reference", "reference-2"]
    );
    assert_eq!(uses[0].references.len(), 2);
    assert_eq!(
        uses[0]
            .references
            .iter()
            .map(|reference| reference.source_offset)
            .collect::<Vec<_>>(),
        [300, 301]
    );

    let mut different = point.clone();
    different.id = "different".to_string();
    different.coordinates[1] = f64::from_bits(2.0_f64.to_bits() + 1);
    let different_groups = feature_sketch_point_groups(std::slice::from_ref(&different));
    assert!(feature_sketch_point_uses(
        &different_groups,
        std::slice::from_ref(&named_point),
        std::slice::from_ref(&block_use),
    )
    .is_empty());
    let mut duplicate = point.clone();
    duplicate.id = "payload-point-2".to_string();
    let duplicate_groups = feature_sketch_point_groups(&[point.clone(), duplicate.clone()]);
    assert_eq!(duplicate_groups[0].points, [point.id.clone(), duplicate.id]);
    let uses = feature_sketch_point_uses(
        &duplicate_groups,
        std::slice::from_ref(&named_point),
        std::slice::from_ref(&block_use),
    );
    assert_eq!(uses[0].sketch_point_group, duplicate_groups[0].id);
    let conflicting_groups = feature_sketch_point_groups(&[point, different]);
    assert!(conflicting_groups.is_empty());
    assert!(
        feature_sketch_point_uses(&conflicting_groups, &[named_point], &[block_use]).is_empty()
    );
}
