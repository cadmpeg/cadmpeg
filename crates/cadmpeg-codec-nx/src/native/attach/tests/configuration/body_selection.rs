use super::*;

#[test]
fn feature_body_selection_retains_complete_input_local_identities_atomically() {
    use cadmpeg_ir::features::BodySelection;
    use cadmpeg_ir::ids::BodyId;
    use std::collections::BTreeMap;

    let first = BodyId::mint("nx:s2:body#3".to_string()).expect("identity grammar");
    let roots = BTreeMap::from([(94, 94), (122, 122)]);
    assert_eq!(
        super::feature_body_selection(
            &[94, 122],
            &roots,
            &BTreeMap::new(),
            "nx:om-object-indices#94,122".to_string(),
        )
        .into_selection(),
        BodySelection::local(
            vec![
                "nx:om-body-object#94".to_string(),
                "nx:om-body-object#122".to_string(),
            ],
            "nx:om-object-indices#94,122".to_string()
        )
        .unwrap()
    );
    assert!(matches!(
        super::feature_body_selection(
            &[94, 123],
            &roots,
            &BTreeMap::new(),
            "nx:om-object-indices#94,123".to_string(),
        )
        .into_selection(),
        BodySelection::Native(_)
    ));
    let aliases = BTreeMap::from([(94, 94), (150, 94)]);
    assert_eq!(
        super::feature_body_selection(
            &[94, 150],
            &aliases,
            &BTreeMap::new(),
            "nx:om-object-indices#94,150".to_string(),
        )
        .into_selection(),
        BodySelection::local(
            vec!["nx:om-body-object#94".to_string()],
            "nx:om-object-indices#94,150".to_string()
        )
        .unwrap()
    );
    let bindings = BTreeMap::from([(94, vec![first.clone()])]);
    let segment_binding = |id: &str, stream_ordinal, body_object_index, alias| {
        crate::native::segments::SegmentBodyBinding {
            id: id.to_string(),
            stream_link: format!("stream-link#{stream_ordinal}"),
            stream_ordinal,
            stream_kind: crate::parasolid::StreamKind::Partition,
            body_object_index,
            body_alias_object_index: alias,
            stream_role: 0,
            source_offset: 0,
        }
    };
    let segment_bindings = [segment_binding("binding#0", 0, 94, 150)];
    assert_eq!(
        super::feature_body_selection(
            &[94],
            &roots,
            &bindings,
            "nx:om-object-index#94".to_string(),
        )
        .into_selection(),
        BodySelection::Resolved {
            bodies: vec![first.clone()],
            native: "nx:om-object-index#94".to_string(),
        }
    );
    assert_eq!(
        super::feature_body_outputs(94, &segment_bindings, &bindings),
        vec![first]
    );
    let ambiguous_body_bindings = BTreeMap::from([(
        94,
        vec![
            BodyId::mint("nx:s2:body#3".to_string()).expect("identity grammar"),
            BodyId::mint("nx:s2:body#4".to_string()).expect("identity grammar"),
        ],
    )]);
    assert!(
        super::feature_body_outputs(94, &segment_bindings, &ambiguous_body_bindings).is_empty()
    );
    assert!(super::feature_body_outputs(123, &segment_bindings, &bindings).is_empty());
    let ambiguous_bindings = [
        segment_binding("binding#0", 0, 94, 150),
        segment_binding("binding#1", 1, 94, 151),
    ];
    assert!(super::feature_body_outputs(94, &ambiguous_bindings, &bindings).is_empty());
}

#[test]
fn feature_body_selection_uses_complete_offset_store_proof_for_colliding_index() {
    use cadmpeg_ir::features::BodySelection;
    use std::collections::BTreeMap;

    let selection = super::feature_body_selection_with_offset_blocks(
        &[94],
        &BTreeMap::from([(94, 94)]),
        &BTreeMap::from([(94, "nx:om-data-blocks-3:block#94".to_string())]),
        &BTreeMap::new(),
        "nx:om-object-index#94".to_string(),
    );
    assert_eq!(
        selection.into_selection(),
        BodySelection::local(
            vec!["nx:om-data-blocks-3:block#94".to_string()],
            "nx:om-object-index#94".to_string()
        )
        .unwrap()
    );
}
#[test]
fn native_primary_body_references_retain_only_proven_body_namespaces() {
    use crate::native::features::{
        FeatureBodyDataBlockUse, FeatureBodyReference, FeatureBodySegmentUse, FeatureInputBlock,
    };
    use crate::native::om::{DataBlock, DataBlockRole};

    let reference = |id: &str, operation_label: &str, body_object_index| FeatureBodyReference {
        ordinal: None,
        id: id.to_string(),
        operation_label: operation_label.to_string(),
        body: crate::om::reference_index::FeatureReferenceToken::from_wire(
            body_object_index,
            &[body_object_index as u8],
        )
        .unwrap(),
        source_offset: 0,
    };
    let references = [
        reference("reference#segment", "operation#segment", 10),
        reference("reference#exact", "operation#exact", 99),
        reference("reference#missing", "operation#missing", 100),
        reference("reference#ambiguous", "operation#ambiguous", 101),
        reference("reference#duplicate-a", "operation#duplicate", 102),
        reference("reference#duplicate-b", "operation#duplicate", 103),
    ];
    let input = |id: &str, operation_label: &str, slot: u8, data_block: &str| FeatureInputBlock {
        id: id.to_string(),
        operation_label: operation_label.to_string(),
        input_slot: crate::om::header_references::HeaderSlot::try_from(slot).unwrap(),
        object: crate::om::reference_index::FeatureReferenceToken::from_wire(
            u32::from(slot),
            &[slot],
        )
        .unwrap(),
        data_block: data_block.to_string(),
        source_offset: 0,
    };
    let blocks = [
        DataBlock {
            id: "block#exact-input".to_string(),
            section_ordinal: 2,
            block_ordinal: 3,
            role: DataBlockRole::Column,
            section_offset: 0,
            byte_len: 0,
            sha256: crate::native::hex::Sha256Hex::digest(&[]),
            stable_identity: None,
            source_entry: String::new(),
            source_offset: 0,
        },
        DataBlock {
            id: "block#missing-input".to_string(),
            section_ordinal: 2,
            block_ordinal: 4,
            role: DataBlockRole::Column,
            section_offset: 0,
            byte_len: 0,
            sha256: crate::native::hex::Sha256Hex::digest(&[]),
            stable_identity: None,
            source_entry: String::new(),
            source_offset: 0,
        },
        DataBlock {
            id: "block#ambiguous-input-1".to_string(),
            section_ordinal: 2,
            block_ordinal: 5,
            role: DataBlockRole::Column,
            section_offset: 0,
            byte_len: 0,
            sha256: crate::native::hex::Sha256Hex::digest(&[]),
            stable_identity: None,
            source_entry: String::new(),
            source_offset: 0,
        },
        DataBlock {
            id: "block#ambiguous-input-2".to_string(),
            section_ordinal: 3,
            block_ordinal: 6,
            role: DataBlockRole::Column,
            section_offset: 0,
            byte_len: 0,
            sha256: crate::native::hex::Sha256Hex::digest(&[]),
            stable_identity: None,
            source_entry: String::new(),
            source_offset: 0,
        },
    ];
    let data_block_uses = [FeatureBodyDataBlockUse {
        id: "data-block-use#exact".to_string(),
        feature_body_reference: references[1].id.clone(),
        data_block: "block#exact-output".to_string(),
    }];
    let inputs = [
        input("input#exact", "operation#exact", 0, "block#exact-input"),
        input(
            "input#missing",
            "operation#missing",
            0,
            "block#missing-input",
        ),
        input(
            "input#ambiguous-1",
            "operation#ambiguous",
            0,
            "block#ambiguous-input-1",
        ),
        input(
            "input#ambiguous-2",
            "operation#ambiguous",
            1,
            "block#ambiguous-input-2",
        ),
    ];

    let native = super::native_primary_body_references(
        &references,
        &data_block_uses,
        &[FeatureBodySegmentUse {
            id: "segment-use#exact".to_string(),
            feature_body_reference: references[1].id.clone(),
            segment_body_binding: "binding#exact".to_string(),
        }],
        &inputs,
        &blocks,
    );
    assert_eq!(native.get("operation#segment"), Some(&10));
    assert_eq!(native.get("operation#exact"), Some(&99));
    assert!(!native.contains_key("operation#missing"));
    assert!(!native.contains_key("operation#ambiguous"));
    assert!(!native.contains_key("operation#duplicate"));
}

#[test]
fn segment_bound_bodies_form_the_exact_retained_history_input() {
    use cadmpeg_ir::features::{BodySelection, FeatureDefinition};
    use cadmpeg_ir::ids::{BodyId, RegionId};
    use cadmpeg_ir::topology::{Body, BodyKind};

    let mut ir = CadIr::empty();
    let bound = BodyId::mint("nx:s2:body#3".to_string()).expect("identity grammar");
    ir.model.bodies.extend([
        Body {
            id: bound.clone(),
            kind: BodyKind::Solid,
            regions: vec![
                RegionId::mint("test:model:entity#region-2".to_string()).expect("identity grammar")
            ],
            transform: None,
            name: None,
            color: None,
            visible: None,
        },
        Body {
            id: BodyId::mint("nx:s3:body#4".to_string()).expect("identity grammar"),
            kind: BodyKind::Solid,
            regions: vec![
                RegionId::mint("test:model:entity#region-3".to_string()).expect("identity grammar")
            ],
            transform: None,
            name: None,
            color: None,
            visible: None,
        },
    ]);
    let binding = crate::native::segments::SegmentBodyBinding {
        id: "nx:segment-body-bindings:binding#0".to_string(),
        stream_link: "nx:segment-stream-links:link#0".to_string(),
        stream_ordinal: 2,
        stream_kind: crate::parasolid::StreamKind::Partition,
        body_object_index: 10,
        body_alias_object_index: 11,
        stream_role: 19,
        source_offset: 100,
    };
    let mut annotations = AnnotationBuilder::new();
    let stream = annotations.stream("nx:container");

    let id = super::attach_initial_segment_bodies(&mut ir, &[binding], &mut annotations, &stream)
        .expect("one emitted body has an exact segment binding");

    assert_eq!(
        id,
        FeatureId::mint("nx:feature-history:feature#initial-bodies").expect("identity grammar")
    );
    assert_eq!(
        ir.model.features[0].evaluation.outputs(),
        std::slice::from_ref(&bound)
    );
    assert_eq!(
        *ir.model.features[0].evaluation.definition(),
        FeatureDefinition::BaseFeature {
            bodies: BodySelection::Resolved {
                bodies: vec![bound.clone()],
                native: "nx:segment-body-bindings".to_string(),
            },
        }
    );
    assert_eq!(
        crate::evaluation::evaluate_saved_body_census(&ir),
        crate::evaluation::BodyCensusEvaluation::Mismatch {
            rederived: vec![bound],
            saved: ir.model.bodies.iter().map(|body| body.id.clone()).collect(),
        }
    );
}

#[test]
fn body_write_does_not_materialize_missing_neutral_geometry() {
    let mut ir = CadIr::empty();
    let binding = crate::native::segments::SegmentBodyBinding {
        id: "nx:segment-body-bindings:binding#0".to_string(),
        stream_link: "nx:segment-stream-links:link#0".to_string(),
        stream_ordinal: 2,
        stream_kind: crate::parasolid::StreamKind::Plain,
        body_object_index: 10,
        body_alias_object_index: 11,
        stream_role: 5,
        source_offset: 100,
    };
    let mut annotations = AnnotationBuilder::new();
    let stream = annotations.stream("nx:container");

    assert!(
        super::attach_initial_segment_bodies(&mut ir, &[binding], &mut annotations, &stream,)
            .is_none()
    );
    assert!(ir.model.bodies.is_empty());
    assert!(ir.model.features.is_empty());
}

#[test]
fn nx_boolean_retains_disjoint_current_and_input_local_bodies() {
    use cadmpeg_ir::features::{BodySelection, BooleanKind, Feature, FeatureDefinition, FeatureId};
    use cadmpeg_ir::ids::BodyId;
    use std::collections::BTreeMap;

    let operation = crate::native::features::FeatureBooleanOperation {
        id: "boolean#0".to_string(),
        operation_label: "operation#0".to_string(),
        kind: crate::native::features::FeatureBooleanKind::Subtract,
        target: crate::test_support::native_references::boolean_reference(94, 0),
        tools: vec![crate::test_support::native_references::boolean_reference(
            122, 1,
        )],
        source_offset: 0,
    };
    let body = BodyId::mint("nx:s18:body#3".to_string()).expect("identity grammar");
    let definition = super::boolean_feature_definition(
        &operation,
        &BTreeMap::from([(94, 94), (122, 122)]),
        &BooleanOffsetStoreResolution::None,
        &BTreeMap::from([(94, vec![body.clone()])]),
    )
    .unwrap();

    assert_eq!(
        definition,
        FeatureDefinition::Combine {
            operands: cadmpeg_ir::features::CombineOperands::new(
                BodySelection::Resolved {
                    bodies: vec![body.clone()],
                    native: "nx:om-object-index#94".to_string(),
                },
                BodySelection::local(
                    vec!["nx:om-body-object#122".to_string()],
                    "nx:om-object-indices#122".to_string()
                )
                .unwrap()
            )
            .unwrap(),

            op: BooleanKind::Cut,
            keep_tools: false,
        }
    );
    let feature = Feature {
        id: FeatureId::mint("synthetic:test:id#feature".to_string()).expect("identity grammar"),
        ordinal: 0,
        name: None,
        suppressed: Some(false),
        dependencies: cadmpeg_ir::features::DistinctMembers::default(),
        source_properties: BTreeMap::new(),
        source_tag: None,
        source_text: None,
        source_content: cadmpeg_ir::features::FeatureContent::default(),

        evaluation: cadmpeg_ir::features::FeatureEvaluation::new(definition, vec![body]).unwrap(),
        native_ref: None,
    };
    assert!(!combine_definition_is_incomplete(&feature));
}

#[test]
fn nx_boolean_projects_unique_offset_store_body_blocks_as_local_bodies() {
    use cadmpeg_ir::features::{BodySelection, BooleanKind, FeatureDefinition};
    use std::collections::BTreeMap;

    let operation = crate::native::features::FeatureBooleanOperation {
        id: "boolean#offset".to_string(),
        operation_label: "operation#offset".to_string(),
        kind: crate::native::features::FeatureBooleanKind::Unite,
        target: crate::test_support::native_references::boolean_reference(401, 0),
        tools: vec![
            crate::test_support::native_references::boolean_reference(402, 1),
            crate::test_support::native_references::boolean_reference(403, 2),
        ],
        source_offset: 0,
    };
    let blocks = BTreeMap::from([
        (401, "nx:om-data-blocks-3:block#401".to_string()),
        (402, "nx:om-data-blocks-3:block#402".to_string()),
        (403, "nx:om-data-blocks-3:block#403".to_string()),
    ]);

    assert_eq!(
        super::boolean_feature_definition(
            &operation,
            &BTreeMap::new(),
            &BooleanOffsetStoreResolution::Complete(blocks.clone()),
            &BTreeMap::new(),
        )
        .unwrap(),
        FeatureDefinition::Combine {
            operands: cadmpeg_ir::features::CombineOperands::new(
                BodySelection::local(
                    vec!["nx:om-data-blocks-3:block#401".to_string()],
                    "nx:om-object-index#401".to_string()
                )
                .unwrap(),
                BodySelection::local(
                    vec![
                        "nx:om-data-blocks-3:block#402".to_string(),
                        "nx:om-data-blocks-3:block#403".to_string(),
                    ],
                    "nx:om-object-indices#402,403".to_string()
                )
                .unwrap()
            )
            .unwrap(),

            op: BooleanKind::Join,
            keep_tools: false,
        }
    );
}

#[test]
fn nx_boolean_writers_follow_selected_identity_namespace() {
    use cadmpeg_ir::features::{BodySelection, BooleanKind, FeatureDefinition, FeatureId};
    use std::collections::BTreeMap;

    let operation = crate::native::features::FeatureBooleanOperation {
        id: "boolean#writer-namespace".to_string(),
        operation_label: "nx:feature-history:operation-label#section-7".to_string(),
        kind: crate::native::features::FeatureBooleanKind::Unite,
        target: crate::test_support::native_references::boolean_reference(401, 0),
        tools: vec![crate::test_support::native_references::boolean_reference(
            402, 1,
        )],
        source_offset: 0,
    };
    let blocks = BTreeMap::from([
        (401, "nx:om-data-blocks-3:block#401".to_string()),
        (402, "nx:om-data-blocks-3:block#402".to_string()),
    ]);
    let definition = super::boolean_feature_definition(
        &operation,
        &BTreeMap::new(),
        &BooleanOffsetStoreResolution::Complete(blocks.clone()),
        &BTreeMap::new(),
    )
    .unwrap();
    let FeatureDefinition::Combine { operands, .. } = &definition else {
        panic!("Boolean definition");
    };
    let target = operands.target();
    let tools = operands.tools();

    let native_prior =
        FeatureId::mint("synthetic:test:id#native-prior".to_string()).expect("identity grammar");
    let offset_prior =
        FeatureId::mint("synthetic:test:id#offset-prior".to_string()).expect("identity grammar");
    let mut history = super::BodyWriterHistory::default();
    history.record_writer(Some(401), None, &[], &native_prior);
    history.record_writer(None, Some(&blocks[&401]), &[], &offset_prior);
    history.record_writer(None, Some(&blocks[&402]), &[], &offset_prior);

    assert_eq!(
        super::boolean_participant_writer(target, 401, Some(&blocks), &BTreeMap::new(), &history,),
        Some(&offset_prior)
    );
    assert_eq!(
        super::boolean_participant_writer(tools, 402, Some(&blocks), &BTreeMap::new(), &history,),
        Some(&offset_prior)
    );
    assert_eq!(
        super::boolean_target_writer(&definition, 401),
        (None, Some("nx:om-data-blocks-3:block#401"))
    );

    let native_definition = FeatureDefinition::Combine {
        operands: cadmpeg_ir::features::CombineOperands::new(
            BodySelection::Native("nx:om-object-index#401".to_string()),
            BodySelection::Native("nx:om-object-indices#402".to_string()),
        )
        .unwrap(),

        op: BooleanKind::Join,
        keep_tools: false,
    };
    assert_eq!(
        super::boolean_target_writer(&native_definition, 401),
        (Some(401), None)
    );
}

#[test]
fn nx_boolean_offset_store_resolution_requires_one_unique_store() {
    use crate::native::features::FeatureBooleanKind;
    use crate::native::om::{DataBlock, DataBlockRole};
    use std::collections::BTreeMap;

    let operation = crate::native::features::FeatureBooleanOperation {
        id: "boolean#offset-store".to_string(),
        operation_label: "nx:feature-history:operation-label#section-7".to_string(),
        kind: FeatureBooleanKind::Unite,
        target: crate::test_support::native_references::boolean_reference(401, 0),
        tools: vec![
            crate::test_support::native_references::boolean_reference(402, 1),
            crate::test_support::native_references::boolean_reference(403, 2),
        ],
        source_offset: 0,
    };
    let block = |section_ordinal, block_ordinal| DataBlock {
        id: format!("nx:om-data-blocks-{section_ordinal}:block#{block_ordinal}"),
        section_ordinal,
        block_ordinal,
        role: DataBlockRole::Column,
        section_offset: 0,
        byte_len: 0,
        sha256: crate::native::hex::Sha256Hex::digest(&[]),
        stable_identity: None,
        source_entry: String::new(),
        source_offset: 0,
    };
    let same_store = vec![block(3, 401), block(3, 402), block(3, 403)];
    assert_eq!(
        crate::native::segments::boolean_offset_store_resolution(&operation, &same_store),
        crate::native::segments::BooleanOffsetStoreResolution::Complete(BTreeMap::from([
            (401, "nx:om-data-blocks-3:block#401".to_string()),
            (402, "nx:om-data-blocks-3:block#402".to_string()),
            (403, "nx:om-data-blocks-3:block#403".to_string()),
        ]))
    );
    let mixed_store = vec![block(3, 401), block(4, 402), block(4, 403)];
    assert!(matches!(
        crate::native::segments::boolean_offset_store_resolution(&operation, &mixed_store),
        crate::native::segments::BooleanOffsetStoreResolution::Unresolved
    ));
    assert!(matches!(
        crate::native::segments::boolean_offset_store_resolution(&operation, &[]),
        crate::native::segments::BooleanOffsetStoreResolution::None
    ));
    let mut control = block(3, 0);
    control.role = DataBlockRole::Control;
    let control_operation = crate::native::features::FeatureBooleanOperation {
        target: crate::test_support::native_references::boolean_reference(
            0,
            operation.target.offset,
        ),
        tools: [401, 402]
            .into_iter()
            .zip(operation.tools.iter())
            .map(|(value, token)| {
                crate::test_support::native_references::boolean_reference(value, token.offset)
            })
            .collect(),
        ..operation.clone()
    };
    assert!(matches!(
        crate::native::segments::boolean_offset_store_resolution(
            &control_operation,
            &[control, block(3, 401), block(3, 402)],
        ),
        crate::native::segments::BooleanOffsetStoreResolution::Unresolved
    ));
}
