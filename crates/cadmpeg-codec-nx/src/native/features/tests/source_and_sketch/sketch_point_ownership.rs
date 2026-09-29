use crate::test_support::test_bytes::shifted_f64_bytes;

use crate::native::features::feature_sketch_fixed_points;
use crate::native::features::feature_sketch_named_point_block_uses;
use crate::native::features::feature_sketch_payload_named_records;
use crate::native::features::feature_sketch_point_groups;
use crate::native::features::feature_sketch_point_uses;
use crate::native::features::feature_sketch_preceding_named_point_uses;
use crate::native::features::payload_name::FeaturePayloadName;
use crate::native::features::FeatureConstructionPayload;
use crate::native::features::FeatureSketchNamedPointBlockUse;
use crate::native::features::FeatureSketchPayloadFixedPair;
use crate::native::features::FeatureSketchPoint;
use crate::native::features::FeatureSketchReference;
use crate::native::features::OffsetStoreNamedPoint;
use crate::om::scalar_pair::{PairPosition, SketchPairForm};

fn sketch_point_groups(
    points: &[FeatureSketchPoint],
) -> Vec<crate::native::features::FeatureSketchPointGroup> {
    crate::test_support::with_decode_context(|ctx| feature_sketch_point_groups(ctx, points))
        .expect("sketch point groups")
}

fn sketch_group_limit(dimension: cadmpeg_core::decode::ResourceDimension) {
    let point = FeatureSketchPoint {
        id: "nx:feature-history:sketch-point#0-0".into(),
        operation_label: "operation".into(),
        named_record: "record".into(),
        name: "Point1".into(),
        coordinates: cadmpeg_ir::units::FiniteVector::new([1.0, 2.0]).expect("finite point"),
        scalar_fields: ["scalar-a".into(), "scalar-b".into()],
    };
    assert_eq!(sketch_point_groups(std::slice::from_ref(&point)).len(), 1);
    let arena = cadmpeg_core::decode::DecodeArena::new();
    let mut policy = cadmpeg_core::decode::DecodePolicy::service();
    match dimension {
        cadmpeg_core::decode::ResourceDimension::CollectionItems => {
            policy.limits.max_collection_items = 0;
        }
        cadmpeg_core::decode::ResourceDimension::RetainedBytes => {
            policy.limits.max_retained_bytes = 0;
        }
        cadmpeg_core::decode::ResourceDimension::MaterializedBytes => {
            policy.limits.max_materialized_bytes = 0;
        }
        cadmpeg_core::decode::ResourceDimension::WorkUnits => policy.limits.max_work_units = 0,
        _ => panic!("unsupported sketch group test dimension"),
    }
    let (ctx, _) = cadmpeg_core::decode::DecodeContext::from_root_bytes(&[], &arena, &policy)
        .expect("empty test root");
    let error =
        feature_sketch_point_groups(&ctx, &[point]).expect_err("sketch group resource limit");
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit) if limit.dimension == dimension)
    );
}

#[test]
fn sketch_point_group_route_refuses_collection_limit() {
    sketch_group_limit(cadmpeg_core::decode::ResourceDimension::CollectionItems);
}

#[test]
fn sketch_point_group_route_refuses_retained_limit() {
    sketch_group_limit(cadmpeg_core::decode::ResourceDimension::RetainedBytes);
}

#[test]
fn sketch_point_group_route_refuses_scoped_limit() {
    sketch_group_limit(cadmpeg_core::decode::ResourceDimension::MaterializedBytes);
}

#[test]
fn sketch_point_group_route_refuses_work_limit() {
    sketch_group_limit(cadmpeg_core::decode::ResourceDimension::WorkUnits);
}

#[test]
fn sketch_named_records_own_fixed_pairs_within_their_intervals() {
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
    let records = crate::test_support::with_decode_context(|ctx| {
        feature_sketch_payload_named_records(ctx, &[payload], &names, &[], &pairs, &[])
    })
    .expect("named sketch payload records");
    assert_eq!(records[0].fixed_pairs, ["pair", "auxiliary-pair"]);
    assert!(records[1].fixed_pairs.is_empty());
    let points = crate::test_support::with_decode_context(|ctx| {
        feature_sketch_fixed_points(ctx, &records, &names, &pairs)
    })
    .unwrap();
    assert_eq!(points.len(), 1);
    assert_eq!(points[0].name, "Point1");
    assert_eq!(
        points[0].values.map(cadmpeg_ir::scalar::FiniteReal::get),
        [0.5, 0.75]
    );
}

#[test]
fn sketch_named_point_block_uses_require_exact_shared_block_identity() {
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
            position: crate::om::sketch_references::SketchReferencePosition::new(
                crate::om::sketch_references::SketchReferenceCount::from_count_byte(count),
                ordinal,
            )
            .unwrap(),
            token: crate::om::reference_index::ReferenceIndexToken::from_wire(
                10 + ordinal,
                &[0xf0, (10 + ordinal) as u8],
            )
            .unwrap(),
            data_block: block.map(str::to_string),
            source_offset: 200 + u64::from(ordinal),
        };
    let uses = crate::test_support::with_decode_context(|ctx| {
        feature_sketch_named_point_block_uses(
            ctx,
            &[
                reference("miss", 0, 2, Some("block-9")),
                reference("hit", 1, 2, Some("block-11")),
                reference("unresolved", 2, 3, None),
            ],
            &[point],
        )
    })
    .unwrap();
    assert_eq!(uses.len(), 1);
    assert_eq!(uses[0].sketch_reference, "hit");
    assert_eq!(uses[0].reference_ordinal, 1);
    assert_eq!(uses[0].point_block_ordinal, 1);
    assert_eq!(uses[0].data_block, "block-11");
}

#[test]
fn sketch_preceding_named_point_uses_require_a_complete_unique_consecutive_lane() {
    let reference = |ordinal, declared_count, block: Option<&str>| FeatureSketchReference {
        id: format!("reference-{ordinal}"),
        operation_label: "nx:feature-history:operation-label#1-4".to_string(),
        position: crate::om::sketch_references::SketchReferencePosition::new(
            crate::om::sketch_references::SketchReferenceCount::from_count_byte(declared_count),
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
    let uses = crate::test_support::with_decode_context(|ctx| {
        feature_sketch_preceding_named_point_uses(
            ctx,
            &references,
            std::slice::from_ref(&preceding),
        )
    })
    .unwrap();
    assert_eq!(uses.len(), 1);
    assert_eq!(uses[0].first_sketch_reference, references[0].id);
    assert_eq!(uses[0].named_point, preceding.id);
    assert_eq!(uses[0].following_data_block, "nx:om-data-blocks-2:block#12");

    let ambiguous = point(
        "nx:offset-store:named-point#2-11",
        &["nx:om-data-blocks-2:block#11"],
    );
    assert!(crate::test_support::with_decode_context(|ctx| {
        feature_sketch_preceding_named_point_uses(ctx, &references, &[preceding.clone(), ambiguous])
    })
    .unwrap()
    .is_empty());
    let gap = point(
        "nx:offset-store:named-point#2-9",
        &["nx:om-data-blocks-2:block#9"],
    );
    let other_store = point(
        "nx:offset-store:named-point#3-11",
        &["nx:om-data-blocks-3:block#11"],
    );
    assert!(crate::test_support::with_decode_context(|ctx| {
        feature_sketch_preceding_named_point_uses(ctx, &references, &[gap, other_store])
    })
    .unwrap()
    .is_empty());

    let unresolved = [references[0].clone(), reference(1, 2, None)];
    assert!(crate::test_support::with_decode_context(|ctx| {
        feature_sketch_preceding_named_point_uses(
            ctx,
            &unresolved,
            std::slice::from_ref(&preceding),
        )
    })
    .unwrap()
    .is_empty());
    let noncontiguous = [
        references[0].clone(),
        reference(2, 3, Some("nx:om-data-blocks-2:block#13")),
    ];
    assert!(crate::test_support::with_decode_context(|ctx| {
        feature_sketch_preceding_named_point_uses(
            ctx,
            &noncontiguous,
            std::slice::from_ref(&preceding),
        )
    })
    .unwrap()
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
    let operation_label = "nx:feature-history:operation-label#1-4".to_string();
    let point = FeatureSketchPoint {
        id: "payload-point".to_string(),
        operation_label: operation_label.clone(),
        named_record: "named-record".to_string(),
        name: "Point1".to_string(),
        coordinates: cadmpeg_ir::units::FiniteVector::new([1.0, 2.0]).expect("finite coordinates"),
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

    let groups = sketch_point_groups(std::slice::from_ref(&point));
    let uses = crate::test_support::with_decode_context(|ctx| {
        feature_sketch_point_uses(
            ctx,
            &groups,
            std::slice::from_ref(&named_point),
            &[second_block_use.clone(), block_use.clone()],
        )
    })
    .unwrap();
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
    different.coordinates =
        cadmpeg_ir::units::FiniteVector::new([1.0, f64::from_bits(2.0_f64.to_bits() + 1)])
            .expect("finite coordinates");
    let different_groups = sketch_point_groups(std::slice::from_ref(&different));
    assert!(
        crate::test_support::with_decode_context(|ctx| feature_sketch_point_uses(
            ctx,
            &different_groups,
            std::slice::from_ref(&named_point),
            std::slice::from_ref(&block_use),
        ))
        .unwrap()
        .is_empty()
    );
    let mut duplicate = point.clone();
    duplicate.id = "payload-point-2".to_string();
    let duplicate_groups = sketch_point_groups(&[point.clone(), duplicate.clone()]);
    assert_eq!(duplicate_groups[0].points, [point.id.clone(), duplicate.id]);
    let uses = crate::test_support::with_decode_context(|ctx| {
        feature_sketch_point_uses(
            ctx,
            &duplicate_groups,
            std::slice::from_ref(&named_point),
            std::slice::from_ref(&block_use),
        )
    })
    .unwrap();
    assert_eq!(uses[0].sketch_point_group, duplicate_groups[0].id);
    let conflicting_groups = sketch_point_groups(&[point, different]);
    assert!(conflicting_groups.is_empty());
    assert!(
        crate::test_support::with_decode_context(|ctx| feature_sketch_point_uses(
            ctx,
            &conflicting_groups,
            &[named_point],
            &[block_use]
        ))
        .unwrap()
        .is_empty()
    );
}
