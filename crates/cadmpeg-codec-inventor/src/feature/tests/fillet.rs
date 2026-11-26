// SPDX-License-Identifier: Apache-2.0
//! Fillet projection from generated edge selections.

use super::*;

#[test]
fn projects_generated_fillet_and_chamfer() {
    let raw_radius = raw_parameter(20);
    let neutral_radius = neutral_parameter(
        &raw_radius,
        ParameterValue::Length(Length::new(2.5).expect("finite length fixture")),
    );
    let fillet_properties = vec![
        test_property(
            1,
            PmDcFeaturePropertyKind::Enumeration {
                family: PmDcFeatureEnumFamily::Fillet,
                type_value: 2,
                value: 0,
            },
        ),
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
                continuity: reference(7),
                option: None,
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
            6,
            PmDcFeaturePropertyKind::Boolean {
                name: String::new(),
                name_value: 0,
                value: false,
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
                .expect("test list metadata matches length"),
                index_reference_value: 0,
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
            PmDcFeaturePropertyKind::SurfaceBody {
                body: reference(30),
            },
        ),
    ];
    let fillet = test_feature(100, 16, &[(0, 2), (11, 1), (15, 8)]);
    let label = test_label(100, 7, FILLET_CLASS_ID, &[]);
    let index = test_projection_index(
        &fillet_properties,
        std::slice::from_ref(&raw_radius),
        std::slice::from_ref(&neutral_radius),
        &[],
        &[],
        &[],
        &[],
        &[],
    );
    let arena = DecodeArena::new();
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &DecodePolicy::service())
        .expect("projection context");
    let (projected, result) = project_fillet(&ctx, &fillet, &label, &index)
        .expect("fillet candidate")
        .expect("fillet projection");
    assert!(matches!(
        projected.evaluation.definition(),
        FeatureDefinition::Operation(FeatureOperation::Fillet { groups })
            if matches!(groups[0].radius, RadiusSpec::Constant { radius: actual_radius } if actual_radius.get() == 2.5)
    ));
    assert_eq!(
        result
            .bodies()
            .iter()
            .map(|id| id.as_str().to_owned())
            .collect::<Vec<_>>(),
        vec![fillet_properties[8].id()]
    );

    for members in [&[11][..], &[8, 11][..]] {
        let mut properties = fillet_properties.clone();
        properties[3] = test_property(
            4,
            PmDcFeaturePropertyKind::References {
                family: PmDcFeatureReferenceFamily::EdgeCollection,
                items: reference_list(members),
            },
        );
        properties.push(test_property(
            10,
            PmDcFeaturePropertyKind::EdgeItem {
                index_references: PmDcU32List::new(2, None, Vec::new()).expect("empty index list"),
                index_reference_value: -1,
                value: 0,
            },
        ));
        let index = test_projection_index(
            &properties,
            std::slice::from_ref(&raw_radius),
            std::slice::from_ref(&neutral_radius),
            &[],
            &[],
            &[],
            &[],
            &[],
        );
        for class in [FILLET_CLASS_ID, crate::feature::CORNER_ROUND_CLASS_ID] {
            let label = test_label(100, 7, class, &[]);
            let projected = project_fillet(&ctx, &fillet, &label, &index);
            // An empty item selects no edge: a collection of only empty items
            // adds no blend, and one that also lists an item keeps the fillet.
            assert_eq!(projected.is_some(), members.len() == 2);
            if let Some(projected) = projected {
                let (feature, _) = projected.expect("fillet projection");
                assert!(matches!(
                    feature.evaluation.definition(),
                    FeatureDefinition::Operation(FeatureOperation::Fillet { groups })
                        if groups.len() == 1
                ));
            }
        }
    }

    for (type_value, value, continuity, admitted) in [
        (0x0002_0004, 0, Some(false), true),
        (0x0002_0004, 0, Some(true), false),
        (0x0002_0004, 0, None, false),
        (0x0002_0004, 1, Some(false), false),
        (0x0001_0004, 0, Some(false), false),
        (4, 1, Some(false), false),
        (5, 0, Some(false), false),
    ] {
        let mut properties = fillet_properties.clone();
        properties[4] = test_property(
            5,
            PmDcFeaturePropertyKind::WideEnumeration { type_value, value },
        );
        properties[2] = test_property(
            3,
            PmDcFeaturePropertyKind::FilletEdgeSet {
                edges: reference(5),
                radius: reference(21),
                selection: reference(6),
                continuity: reference(if continuity.is_some() { 7 } else { 0 }),
                option: None,
            },
        );
        properties[5] = test_property(
            6,
            PmDcFeaturePropertyKind::Boolean {
                name: String::new(),
                name_value: 0,
                value: continuity.unwrap_or(false),
            },
        );
        let index = test_projection_index(
            &properties,
            std::slice::from_ref(&raw_radius),
            std::slice::from_ref(&neutral_radius),
            &[],
            &[],
            &[],
            &[],
            &[],
        );
        let projected = project_fillet(&ctx, &fillet, &label, &index);
        if admitted {
            let (feature, _) = projected
                .expect("native selector")
                .expect("fillet projection");
            assert!(matches!(feature.evaluation.definition(),
                FeatureDefinition::Operation(FeatureOperation::Fillet { groups })
                    if matches!(&groups[0].edges, cadmpeg_ir::features::selections::EdgeSelection::Native(id)
                        if id == &properties[2].id())
                    && matches!(groups[0].radius, RadiusSpec::Constant { radius } if radius.get() == 2.5)
            ));
        } else {
            assert!(projected.is_none());
        }
    }

    let mut named_properties = fillet_properties.clone();
    named_properties[6] = test_property(
        7,
        PmDcFeaturePropertyKind::EdgeItem {
            index_references: PmDcU32List::new(
                2,
                Some(crate::pmdc::PmDcListMetadata::U32([1, 0])),
                vec![42],
            )
            .expect("valid named feature fixture"),
            index_reference_value: -1,
            value: 0,
        },
    );
    let mut named_index = test_projection_index(
        &named_properties,
        std::slice::from_ref(&raw_radius),
        std::slice::from_ref(&neutral_radius),
        &[],
        &[],
        &[],
        &[],
        &[],
    );
    let generated_edge = cadmpeg_ir::features::selections::GeneratedEdgeRef {
        feature: crate::feature::history_feature_id(&test_feature(90, 0, &[]).identity)
            .expect("valid named feature fixture"),
        local_id: cadmpeg_ir::features::selections::SelectionReference::new(
            "inventor:nmx:INV_NMX_SWEEPGENERATED_TAG:3:0:18".into(),
        )
        .expect("valid named feature fixture"),
    };
    named_index.edge_names.insert(
        (
            named_properties[3]
                .identity
                .segment_token
                .as_str()
                .to_owned(),
            42,
        ),
        Some(generated_edge.clone()),
    );
    let (named, _) = project_fillet(&ctx, &fillet, &label, &named_index)
        .expect("named fillet candidate")
        .expect("named fillet projection");
    assert!(
        matches!(named.evaluation.definition(), FeatureDefinition::Operation(FeatureOperation::Fillet { groups })
        if matches!(&groups[0].edges, cadmpeg_ir::features::selections::EdgeSelection::Generated { edges, native }
            if edges.as_slice() == [generated_edge.clone()] && native.as_str() == named_properties[3].id()))
    );

    let raw_distance = raw_parameter(40);
    let neutral_distance = neutral_parameter(
        &raw_distance,
        ParameterValue::Length(Length::new(1.25).expect("finite length fixture")),
    );
    let chamfer_properties = vec![
        test_property(
            31,
            PmDcFeaturePropertyKind::References {
                family: PmDcFeatureReferenceFamily::EdgeCollection,
                items: reference_list(&[34]),
            },
        ),
        test_property(
            32,
            PmDcFeaturePropertyKind::Enumeration {
                family: PmDcFeatureEnumFamily::Chamfer,
                type_value: 2,
                value: 0,
            },
        ),
        test_property(
            33,
            PmDcFeaturePropertyKind::EdgeItem {
                index_references: PmDcU32List::new(
                    2,
                    Some(crate::pmdc::PmDcListMetadata::U32([1, 0])),
                    vec![17],
                )
                .expect("test list metadata matches length"),
                index_reference_value: -1,
                value: 0,
            },
        ),
        test_property(
            34,
            PmDcFeaturePropertyKind::Boolean {
                name: String::new(),
                name_value: 0,
                value: true,
            },
        ),
        test_property(
            35,
            PmDcFeaturePropertyKind::References {
                family: PmDcFeatureReferenceFamily::ObjectCollection,
                items: reference_list(&[37]),
            },
        ),
        test_property(
            36,
            PmDcFeaturePropertyKind::SurfaceBody {
                body: reference(30),
            },
        ),
    ];
    let chamfer = test_feature(101, 12, &[(0, 31), (2, 40), (4, 32), (5, 34), (11, 35)]);
    let label = test_label(101, 8, CHAMFER_CLASS_ID, &[]);
    let index = test_projection_index(
        &chamfer_properties,
        std::slice::from_ref(&raw_distance),
        std::slice::from_ref(&neutral_distance),
        &[],
        &[],
        &[],
        &[],
        &[],
    );
    let arena = DecodeArena::new();
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &DecodePolicy::service())
        .expect("projection context");
    let (projected, _) = project_chamfer(&ctx, &chamfer, &label, &index)
        .expect("chamfer candidate")
        .expect("chamfer projection");
    assert!(matches!(
        projected.evaluation.definition(),
        FeatureDefinition::Operation(FeatureOperation::Chamfer {
            groups,
            flip_direction: true
        }) if matches!(groups[0].spec, ChamferSpec::Distance { distance: actual_distance } if actual_distance.get() == 1.25)
    ));
    let mut named_index = index;
    named_index.edge_names.insert(
        (
            chamfer_properties[0]
                .identity
                .segment_token
                .as_str()
                .to_owned(),
            17,
        ),
        Some(generated_edge.clone()),
    );
    let (named, _) = project_chamfer(&ctx, &chamfer, &label, &named_index)
        .expect("named chamfer candidate")
        .expect("named chamfer projection");
    assert!(
        matches!(named.evaluation.definition(), FeatureDefinition::Operation(FeatureOperation::Chamfer { groups, .. })
        if matches!(&groups[0].edges, cadmpeg_ir::features::selections::EdgeSelection::Generated { edges, native }
            if edges.as_slice() == [generated_edge] && native.as_str() == chamfer_properties[0].id()))
    );
}
