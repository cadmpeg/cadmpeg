// SPDX-License-Identifier: Apache-2.0
//! Thread annotations and native face selections.

use super::*;
use crate::feature::{
    parse_face_collection, parse_face_item, project_thread, thread_face_collection,
    FeatureInventory, ProjectedSketches, FACE_COLLECTION_TYPE, FACE_ITEM_TYPE, THREAD_CLASS_ID,
};

type TestResult<T = ()> = Result<T, Box<dyn std::error::Error>>;

#[test]
fn face_selection_records_require_indices_and_exact_termination() -> TestResult {
    for version in [18, 19, 23, 28] {
        let stamps: &[i32] = match version {
            23 => &[-1],
            28 => &[-1, -1],
            _ => &[],
        };
        let mut collection = content_at(1, version, stamps);
        collection.extend_from_slice(&references(&[0x8000_0003]));
        let mut item = content_at(2, version, stamps);
        item.extend_from_slice(&references(&[0x8000_0001]));
        item.extend_from_slice(&(-1i32).to_le_bytes());
        parse(&collection, |ctx, source| -> TestResult {
            let value = parse_face_collection(ctx, source, version)?;
            assert!(matches!(
                value.kind,
                PmDcFeaturePropertyKind::References {
                    family: PmDcFeatureReferenceFamily::FaceCollection, items
                } if items.references() == [reference(3)]
            ));
            Ok(())
        })?;
        parse(&item, |ctx, source| -> TestResult {
            let value = parse_face_item(ctx, source, version)?;
            assert!(matches!(
                value.kind,
                PmDcFeaturePropertyKind::FaceItem { index_references }
                    if index_references.values() == [0x8000_0001]
            ));
            Ok(())
        })?;
        let mut invalid = item.clone();
        let terminal = invalid.len() - 4;
        invalid[terminal..].copy_from_slice(&0i32.to_le_bytes());
        let mut extra = item.clone();
        extra.push(0);
        let mut empty = content_at(2, version, stamps);
        empty.extend_from_slice(&references(&[]));
        empty.extend_from_slice(&(-1i32).to_le_bytes());
        for bytes in [&invalid[..], &extra[..], &empty[..], &item[..terminal]] {
            parse(bytes, |ctx, source| {
                assert!(parse_face_item(ctx, source, version).is_err());
            });
        }
        for type_id in [FACE_COLLECTION_TYPE, FACE_ITEM_TYPE] {
            let bytes = if type_id == FACE_COLLECTION_TYPE {
                &collection
            } else {
                &item
            };
            let parser = crate::feature::feature_property_parser(type_id)
                .ok_or("missing face-property parser")?;
            parse(bytes, |ctx, source| {
                assert!(parser(ctx, source, version).is_ok());
            });
        }
        let mut wrong_marker = item.clone();
        let list_start = content_at(2, version, stamps).len();
        wrong_marker[list_start..list_start + 2].copy_from_slice(&3u16.to_le_bytes());
        parse(&wrong_marker, |ctx, source| {
            assert!(parse_face_item(ctx, source, version).is_err());
        });
        collection.push(0);
        parse(&collection, |ctx, source| {
            assert!(parse_face_collection(ctx, source, version).is_err());
        });
    }
    Ok(())
}

fn thread_property(ordinal: u32, kind: PmDcFeaturePropertyKind) -> TestResult<PmDcFeatureProperty> {
    let type_id = match &kind {
        PmDcFeaturePropertyKind::FaceItem { .. } => FACE_ITEM_TYPE,
        PmDcFeaturePropertyKind::Boolean { .. } => BOOLEAN_TYPE,
        PmDcFeaturePropertyKind::EdgeItem { .. } => crate::feature::EDGE_ITEM_TYPE,
        PmDcFeaturePropertyKind::References {
            family: PmDcFeatureReferenceFamily::FaceCollection,
            ..
        } => FACE_COLLECTION_TYPE,
        PmDcFeaturePropertyKind::References {
            family: PmDcFeatureReferenceFamily::EdgeCollection,
            ..
        } => crate::feature::EDGE_COLLECTION_TYPE,
        PmDcFeaturePropertyKind::References {
            family: PmDcFeatureReferenceFamily::FeatureDimensions,
            ..
        } => crate::feature::FEATURE_DIMENSIONS_TYPE,
        PmDcFeaturePropertyKind::RdxVariable { .. } => crate::feature::RDX_VARIABLE_TYPE,
        _ => return Err("unsupported Thread fixture property".into()),
    };
    let property = test_property(ordinal, kind);
    Ok(Located::new(
        (*property).clone(),
        crate::record_identity::RecordTypeId::from_bytes(type_id),
        segment(),
        ordinal,
    ))
}

fn thread_fixture() -> TestResult<(PmDcFeature, PmDcFeatureLabel, Vec<PmDcFeatureProperty>)> {
    let source = test_feature(
        0,
        9,
        &[
            (0, 10),
            (1, 11),
            (2, 12),
            (3, 20),
            (4, 21),
            (5, 15),
            (6, 16),
            (7, 30),
            (8, 18),
        ],
    );
    let label = test_label(0, 1, THREAD_CLASS_ID, &[]);
    let mut properties = vec![
        thread_property(
            10,
            PmDcFeaturePropertyKind::References {
                family: PmDcFeatureReferenceFamily::FaceCollection,
                items: reference_list(&[20]),
            },
        )?,
        thread_property(
            19,
            PmDcFeaturePropertyKind::FaceItem {
                index_references: PmDcU32List::new(
                    2,
                    Some(crate::pmdc::PmDcListMetadata::U32([1, 0])),
                    vec![0x8000_0001],
                )
                .ok_or("invalid face-item fixture list")?,
            },
        )?,
        thread_property(
            15,
            PmDcFeaturePropertyKind::References {
                family: PmDcFeatureReferenceFamily::EdgeCollection,
                items: reference_list(&[]),
            },
        )?,
        thread_property(
            18,
            PmDcFeaturePropertyKind::References {
                family: PmDcFeatureReferenceFamily::FeatureDimensions,
                items: reference_list(&[21, 22]),
            },
        )?,
        thread_property(
            30,
            PmDcFeaturePropertyKind::RdxVariable {
                name: String::new(),
                name_value: 0,
                nominal_value: 0,
                model_value: 0,
            },
        )?,
    ];
    for (ordinal, value) in [(11, true), (12, false), (16, false)] {
        properties.push(thread_property(
            ordinal,
            PmDcFeaturePropertyKind::Boolean {
                name: String::new(),
                name_value: 0,
                value,
            },
        )?);
    }
    Ok((source, label, properties))
}

#[test]
fn thread_projects_native_faces_without_size_or_result_bodies() -> TestResult {
    let (source, label, properties) = thread_fixture()?;
    let design = crate::design::DesignInventory {
        parameters: vec![raw_parameter(20), raw_parameter(21)],
        expressions: Vec::new(),
        units: Vec::new(),
        parameter_tables: Vec::new(),
        model_roots: Vec::new(),
        brep_segments: Vec::new(),
        issues: Vec::new(),
    };
    let sketch = crate::sketch::SketchInventory {
        sketches: Vec::new(),
        entities: Vec::new(),
        transforms: Vec::new(),
        directions: Vec::new(),
        constraints: Vec::new(),
        issues: Vec::new(),
    };
    let expected_face = properties[0].id();
    let inventory = FeatureInventory {
        record_types: std::collections::HashMap::new(),
        features: vec![source],
        pattern_features: Vec::new(),
        terminators: Vec::new(),
        properties,
        labels: vec![label],
        entity_style_links: Vec::new(),
        kernel_entity_links: Vec::new(),
        work_planes: Vec::new(),
        issues: Vec::new(),
    };
    let arena = DecodeArena::new();
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &DecodePolicy::service())?;
    let projection = crate::feature::project(
        &ctx,
        &inventory,
        &design,
        &sketch,
        &[],
        &ProjectedSketches {
            sketches: &[],
            entities: &[],
            profile_bodies: &[],
        },
    )?;
    assert_eq!(projection.unresolved_features, 0);
    assert_eq!(projection.unresolved_states, 1);
    assert!(projection.result_topologies.is_empty());
    assert_eq!(projection.features.len(), 1);
    let feature = &projection.features[0];
    assert!(matches!(
        feature.evaluation.definition(),
        FeatureDefinition::Operation(FeatureOperation::CosmeticThread {
            face: cadmpeg_ir::features::FaceSelection::Native(native),
            diameter: None, extent: None,
        }) if *native == expected_face
    ));
    assert_eq!(feature.source_properties["property_1_boolean"], "true");
    assert_eq!(feature.source_properties["property_2_boolean"], "false");
    assert_eq!(feature.source_properties["property_6_boolean"], "false");
    Ok(())
}

#[test]
fn thread_refuses_open_selections_and_propagates_entity_limits() -> TestResult {
    let (source, label, properties) = thread_fixture()?;
    let parameters = [raw_parameter(20), raw_parameter(21)];
    let arena = DecodeArena::new();
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &DecodePolicy::service())?;
    {
        let index = test_projection_index(&properties, &parameters, &[], &[], &[], &[], &[], &[]);
        let mut policy = DecodePolicy::service();
        policy.limits.max_entities = 0;
        let (limited, _) = DecodeContext::from_root_bytes(&[], &arena, &policy)?;
        assert!(matches!(project_thread(&limited, &source, &label, &index),
            Some(Err(CodecError::ResourceLimit(limit)))
                if limit.operation == "project Inventor thread annotation"));
        let index =
            test_projection_index(&properties, &parameters[..1], &[], &[], &[], &[], &[], &[]);
        assert!(project_thread(&ctx, &source, &label, &index).is_none());
    }
    let mut wrong_kind = properties.clone();
    wrong_kind[1] = thread_property(
        19,
        PmDcFeaturePropertyKind::Boolean {
            name: String::new(),
            name_value: 0,
            value: true,
        },
    )?;
    let index = test_projection_index(&wrong_kind, &parameters, &[], &[], &[], &[], &[], &[]);
    assert!(project_thread(&ctx, &source, &label, &index).is_none());
    for items in [
        reference_list(&[]),
        reference_list(&[0]),
        reference_list(&[100]),
    ] {
        let mut candidate = properties.clone();
        candidate[0] = thread_property(
            10,
            PmDcFeaturePropertyKind::References {
                family: PmDcFeatureReferenceFamily::FaceCollection,
                items,
            },
        )?;
        let index = test_projection_index(&candidate, &parameters, &[], &[], &[], &[], &[], &[]);
        assert!(project_thread(&ctx, &source, &label, &index).is_none());
    }
    Ok(())
}

#[test]
fn thread_requires_type_two_lists_and_closed_edge_members() -> TestResult {
    let (source, _, properties) = thread_fixture()?;
    let admitted = |source: &PmDcFeature, properties: &[PmDcFeatureProperty]| {
        thread_face_collection(
            source,
            |reference| {
                let ordinal = reference.checked_sub(1)?;
                properties
                    .iter()
                    .find(|property| property.identity.record_ordinal == ordinal)
            },
            |reference| matches!(reference, 21 | 22),
        )
        .is_some()
    };
    assert!(admitted(&source, &properties));
    for position in [0, 1, 2, 3] {
        let mut candidate = properties.clone();
        let mut wire = serde_json::to_value(&candidate[position])?;
        let list = if position == 1 {
            "index_references"
        } else {
            "items"
        };
        wire["kind"][list]["marker"] = serde_json::json!(3);
        candidate[position] = serde_json::from_value(wire)?;
        assert!(!admitted(&source, &candidate));
    }
    let mut wire = serde_json::to_value(&source)?;
    wire["properties"]["marker"] = serde_json::json!(3);
    let wrong_marker = serde_json::from_value(wire)?;
    assert!(!admitted(&wrong_marker, &properties));
    for members in [&[0][..], &[100][..], &[20][..]] {
        let mut candidate = properties.clone();
        candidate[2] = thread_property(
            15,
            PmDcFeaturePropertyKind::References {
                family: PmDcFeatureReferenceFamily::EdgeCollection,
                items: reference_list(members),
            },
        )?;
        assert!(!admitted(&source, &candidate));
    }
    let mut candidate = properties.clone();
    candidate[2] = thread_property(
        15,
        PmDcFeaturePropertyKind::References {
            family: PmDcFeatureReferenceFamily::EdgeCollection,
            items: reference_list(&[41]),
        },
    )?;
    candidate.push(thread_property(
        40,
        PmDcFeaturePropertyKind::EdgeItem {
            index_references: PmDcU32List::new(2, None, Vec::new())
                .ok_or("invalid edge-item fixture list")?,
            index_reference_value: 0,
            value: 0,
        },
    )?);
    assert!(admitted(&source, &candidate));
    Ok(())
}
