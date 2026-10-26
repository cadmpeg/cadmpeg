// SPDX-License-Identifier: Apache-2.0
use super::*;
use crate::feature::{plate_result, plate_side, FeatureFamily, PLATE_SIDE_TYPE};
use cadmpeg_ir::features::SheetMetalThicknessSide;

fn enumeration(
    family: PmDcFeatureEnumFamily,
    type_value: i16,
    value: u16,
) -> PmDcFeaturePropertyKind {
    PmDcFeaturePropertyKind::Enumeration {
        family,
        type_value,
        value,
    }
}

#[test]
fn plate_side_enumeration_uses_the_versioned_content_header_and_exact_end() {
    for version in [16, 23, 25, 28, 29, 31] {
        let stamps: &[i32] = match version {
            23 => &[-1],
            25.. => &[-1, -1],
            _ => &[],
        };
        let mut bytes = content_at(12, version, stamps);
        bytes.extend_from_slice(&2_i16.to_le_bytes());
        bytes.extend_from_slice(&0_u16.to_le_bytes());
        let parsed = parse(&bytes, |ctx, view| {
            crate::feature::feature_property_parser(PLATE_SIDE_TYPE).expect("side parser")(
                ctx, view, version,
            )
            .expect("side enumeration")
        });
        assert_eq!(
            parsed.kind,
            enumeration(PmDcFeatureEnumFamily::PlateSide, 2, 0)
        );
        bytes.push(0);
        parse(&bytes, |ctx, view| {
            assert!(crate::feature::parse_plate_side(ctx, view, version).is_err());
        });
    }
}

#[test]
fn plate_sides_require_the_boolean_or_proven_enumeration_value() {
    let source = test_feature(100, 17, &[(2, 1)]);
    for (kind, expected) in [
        (
            PmDcFeaturePropertyKind::Boolean {
                name: String::new(),
                name_value: 0,
                value: false,
            },
            Some(SheetMetalThicknessSide::Forward),
        ),
        (
            PmDcFeaturePropertyKind::Boolean {
                name: String::new(),
                name_value: 0,
                value: true,
            },
            Some(SheetMetalThicknessSide::Reverse),
        ),
        (
            enumeration(PmDcFeatureEnumFamily::PlateSide, 2, 0),
            Some(SheetMetalThicknessSide::Forward),
        ),
        (enumeration(PmDcFeatureEnumFamily::PlateSide, 2, 1), None),
        (enumeration(PmDcFeatureEnumFamily::PlateSide, 3, 0), None),
        (enumeration(PmDcFeatureEnumFamily::Auxiliary, 2, 0), None),
    ] {
        let properties = [test_property(1, kind)];
        let index = test_projection_index(&properties, &[], &[], &[], &[], &[], &[], &[]);
        assert_eq!(plate_side(&source, &index), expected);
    }
}

#[test]
fn plate_result_collection_requires_new_body_and_no_legacy_body() {
    let arena = DecodeArena::new();
    let (ctx, _) =
        DecodeContext::from_root_bytes(&[], &arena, &DecodePolicy::service()).expect("context");
    for operation in [1, 2, 3, 4] {
        let properties = [
            test_property(
                1,
                PmDcFeaturePropertyKind::References {
                    family: PmDcFeatureReferenceFamily::ObjectCollection,
                    items: reference_list(&[4]),
                },
            ),
            test_property(
                2,
                enumeration(PmDcFeatureEnumFamily::PartOperation, 5, operation),
            ),
            test_property(
                3,
                PmDcFeaturePropertyKind::SurfaceBody { body: reference(0) },
            ),
        ];
        let index = test_projection_index(&properties, &[], &[], &[], &[], &[], &[], &[]);
        let source = test_feature(100, 17, &[(15, 1), (16, 2)]);
        assert_eq!(FeatureFamily::Plate.output_slot(&source), 15);
        let projected = plate_result(&ctx, &source, &index);
        if operation == 1 {
            let (_, result) = projected.expect("new body").expect("result");
            assert_eq!(result.bodies()[0].as_str(), properties[2].id());
            assert_eq!(
                result.native_ref.as_deref(),
                Some(properties[0].id().as_str())
            );
        } else {
            assert!(projected.is_none());
        }
        let mixed = test_feature(100, 17, &[(6, 3), (15, 1), (16, 2)]);
        assert!(plate_result(&ctx, &mixed, &index).is_none());
        for count in [15, 17] {
            let legacy = test_feature(100, count, &[(6, 3)]);
            assert_eq!(FeatureFamily::Plate.output_slot(&legacy), 6);
            let (_, result) = plate_result(&ctx, &legacy, &index)
                .expect("legacy body")
                .expect("result");
            assert_eq!(result.bodies()[0].as_str(), properties[2].id());
        }
    }
}

#[test]
fn corner_round_without_continuity_uses_its_exact_radius_and_edges() {
    let radius = raw_parameter(20);
    let neutral = neutral_parameter(
        &radius,
        ParameterValue::Length(Length::new(3.0).expect("radius")),
    );
    let properties = [
        test_property(1, enumeration(PmDcFeatureEnumFamily::Fillet, 2, 0)),
        test_property(
            2,
            PmDcFeaturePropertyKind::References {
                family: PmDcFeatureReferenceFamily::FilletEdgeSets,
                items: reference_list(&[4]),
            },
        ),
        test_property(
            3,
            PmDcFeaturePropertyKind::FilletEdgeSet {
                edges: reference(5),
                radius: reference(21),
                selection: reference(6),
                continuity: reference(0),
                option: Some(reference(0)),
            },
        ),
        test_property(
            4,
            PmDcFeaturePropertyKind::References {
                family: PmDcFeatureReferenceFamily::EdgeCollection,
                items: reference_list(&[8]),
            },
        ),
        test_property(
            5,
            PmDcFeaturePropertyKind::WideEnumeration {
                type_value: 4,
                value: 0,
            },
        ),
        test_property(
            7,
            PmDcFeaturePropertyKind::EdgeItem {
                index_references: PmDcU32List::new(
                    2,
                    Some(crate::pmdc::PmDcListMetadata::U32([1, 0])),
                    vec![42],
                )
                .expect("index list"),
                index_reference_value: -1,
                value: 0,
            },
        ),
        test_property(
            8,
            PmDcFeaturePropertyKind::References {
                family: PmDcFeatureReferenceFamily::ObjectCollection,
                items: reference_list(&[10]),
            },
        ),
        test_property(
            9,
            PmDcFeaturePropertyKind::SurfaceBody { body: reference(0) },
        ),
    ];
    let index = test_projection_index(
        &properties,
        std::slice::from_ref(&radius),
        std::slice::from_ref(&neutral),
        &[],
        &[],
        &[],
        &[],
        &[],
    );
    let source = test_feature(100, 16, &[(0, 2), (11, 1), (15, 8)]);
    let arena = DecodeArena::new();
    let (ctx, _) =
        DecodeContext::from_root_bytes(&[], &arena, &DecodePolicy::service()).expect("context");
    let label = test_label(100, 2, crate::feature::CORNER_ROUND_CLASS_ID, &[]);
    let (feature, _) = project_fillet(&ctx, &source, &label, &index)
        .expect("corner round")
        .expect("projection");
    assert!(
        matches!(feature.evaluation.definition(), FeatureDefinition::Operation(FeatureOperation::Fillet { groups }) if groups.len() == 1 && matches!(groups[0].radius, RadiusSpec::Constant { radius } if radius.get() == 3.0))
    );
    let label = test_label(100, 2, FILLET_CLASS_ID, &[]);
    assert!(project_fillet(&ctx, &source, &label, &index).is_none());
}

#[test]
fn each_bend_attachment_slot_keeps_the_plate_native() {
    let arena = DecodeArena::new();
    let (ctx, _) =
        DecodeContext::from_root_bytes(&[], &arena, &DecodePolicy::service()).expect("context");
    let label = test_label(100, 1, crate::feature::PLATE_CLASS_ID, &[]);
    let index = test_projection_index(&[], &[], &[], &[], &[], &[], &[], &[]);
    for slot in [4, 9, 10, 11, 12, 13] {
        let source = test_feature(100, 17, &[(slot, 7)]);
        assert!(crate::feature::project_plate(&ctx, &source, &label, &index).is_none());
    }
}

#[test]
fn unresolved_controls_count_native_attached_plates_and_separate_bends() {
    let arena = DecodeArena::new();
    let (ctx, _) =
        DecodeContext::from_root_bytes(&[], &arena, &DecodePolicy::service()).expect("context");
    let mut inventory = crate::feature::FeatureInventory {
        edge_inputs: crate::feature::edge_selection::EdgeInventory::default(),
        record_types: std::collections::HashMap::default(),
        features: vec![
            test_feature(100, 17, &[(4, 7)]),
            test_feature(200, 11, &[]),
            test_feature(300, 17, &[]),
        ],
        pattern_features: vec![],
        terminators: vec![],
        properties: vec![],
        labels: vec![
            test_label(100, 1, crate::feature::PLATE_CLASS_ID, &[]),
            test_label(
                200,
                2,
                crate::feature::ClassId::try_from("f258f74cd3114f24600047a99dccefb0".to_owned())
                    .expect("bend class"),
                &[],
            ),
            test_label(300, 3, crate::feature::PLATE_CLASS_ID, &[]),
        ],
        entity_style_links: vec![],
        kernel_entity_links: vec![],
        work_planes: vec![],
        issues: vec![],
        ..Default::default()
    };
    let features: Vec<_> = inventory
        .features
        .iter()
        .zip(&inventory.labels)
        .map(|(feature, label)| {
            crate::feature::native_feature(&ctx, &feature.identity, Some(label), feature.id())
                .expect("native history feature")
        })
        .collect();
    assert_eq!(
        crate::feature::sheet_metal::native_kind_counts(&inventory, &features),
        (1, 1)
    );
    assert_eq!(
        crate::feature::sheet_metal::native_kind_counts(&inventory, &[]),
        (0, 0)
    );
    inventory.labels.push(inventory.labels[0].clone());
    assert_eq!(
        crate::feature::sheet_metal::native_kind_counts(&inventory, &features),
        (0, 1)
    );
}
