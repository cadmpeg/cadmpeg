use super::*;

#[test]
fn feature_projection_without_labels_needs_no_collection_storage() {
    let inventory = super::FeatureInventory {
        features: vec![test_feature(0, 0, &[])],
        pattern_features: Vec::new(),
        terminators: Vec::new(),
        properties: Vec::new(),
        labels: Vec::new(),
        entity_style_links: Vec::new(),
        issues: Vec::new(),
    };
    let design = crate::design::DesignInventory {
        parameters: Vec::new(),
        expressions: Vec::new(),
        units: Vec::new(),
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
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_collection_items = 0;
    let (ctx, _) =
        DecodeContext::from_root_bytes(&[], &arena, &policy).expect("projection context");
    let projection = super::project(&ctx, &inventory, &design, &sketch, &[], &[])
        .expect("token check stores no entries");
    assert_eq!(projection.unresolved_features, 1);
    assert!(projection.features.is_empty());
    assert!(projection.result_topologies.is_empty());
    // No labels can produce a feature, so projection stores zero collection entries.
    assert!(matches!(ctx.charge_collection_items(1, "probe"),
        Err(CodecError::ResourceLimit(limit))
            if limit.dimension == ResourceDimension::CollectionItems && limit.used == 0));
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &DecodePolicy::service())
        .expect("service projection context");
    assert_eq!(
        super::project(&ctx, &inventory, &design, &sketch, &[], &[])
            .expect("feature projection")
            .unresolved_features,
        1
    );
}

#[test]
fn feature_parameter_index_refuses_at_first_insert_before_source_tail() {
    let inventory = FeatureInventory {
        features: vec![test_feature(0, 0, &[])],
        pattern_features: Vec::new(),
        terminators: Vec::new(),
        properties: vec![test_property(
            0,
            PmDcFeaturePropertyKind::Boolean {
                name: String::new(),
                name_value: 0,
                value: false,
            },
        )],
        labels: vec![test_label(0, 1, ClassId([0; 16]), &[])],
        entity_style_links: Vec::new(),
        issues: Vec::new(),
    };
    let design = crate::design::DesignInventory {
        parameters: Vec::new(),
        expressions: Vec::new(),
        units: Vec::new(),
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
    let first_raw = raw_parameter(1);
    let second_raw = raw_parameter(2);
    let parameters = [
        neutral_parameter(
            &first_raw,
            ParameterValue::Real(cadmpeg_ir::scalar::FiniteReal::ZERO),
        ),
        neutral_parameter(
            &second_raw,
            ParameterValue::Real(cadmpeg_ir::scalar::FiniteReal::ZERO),
        ),
    ];
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    // The property index is built before the neutral parameter map and takes
    // one slot. The first parameter entry needs a second slot, so it refuses
    // before the source tail is visited.
    policy.limits.max_collection_items = 1;
    policy.limits.max_work_units = u64::MAX;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy)
        .expect("feature parameter index context");
    let probe = RefusalProbe::arm(
        ResourceDimension::WorkUnits,
        "index Inventor feature parameter values",
        Some(2),
    );
    let Err(CodecError::ResourceLimit(limit)) =
        super::project(&ctx, &inventory, &design, &sketch, &parameters, &[])
    else {
        panic!("the first parameter map insertion must exceed the remaining slot");
    };
    drop(probe);
    assert_eq!(limit.dimension, ResourceDimension::CollectionItems);
    assert_eq!(limit.operation, "index Inventor feature parameter values");
    assert_eq!(limit.used, 1);
    assert_eq!(limit.additional, 1);
    assert!(matches!(ctx.finish_session(),
        Err(CodecError::ResourceLimit(sticky)) if sticky == limit));
}

#[test]
fn boolean_property_literal_slots_need_no_work_admission() {
    let source = test_feature(0, 0, &[]);
    let index = test_projection_index(&[], &[], &[], &[], &[], &[], &[], &[]);
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_work_units = 0;
    for slots in [&[20, 22][..], &[2, 3, 4, 5, 8][..], &[6, 9][..]] {
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("context");
        assert!(super::boolean_properties(&ctx, &source, slots, &index)
            .expect("fixed slot lists need no variable-work admission")
            .is_empty());
    }
}

#[test]
fn feature_result_fixed_record_uses_no_extra_collection_slot() {
    let source = test_feature(0, 1, &[(0, 1)]);
    let properties = [
        test_property(
            1,
            PmDcFeaturePropertyKind::References {
                family: PmDcFeatureReferenceFamily::ObjectCollection,
                items: reference_list(&[3]),
            },
        ),
        test_property(
            2,
            PmDcFeaturePropertyKind::SurfaceBody { body: reference(0) },
        ),
    ];
    let index = test_projection_index(&properties, &[], &[], &[], &[], &[], &[], &[]);
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    // One stored body member and one uniqueness-index slot use two slots.
    policy.limits.max_collection_items = 2;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("context");
    let (_, result) = super::feature_result(&ctx, &source, 0, &index)
        .expect("result candidate")
        .expect("fixed record adds no collection entry");
    assert_eq!(result.bodies().len(), 1);
    assert!(matches!(ctx.charge_collection_items(1, "probe"),
        Err(CodecError::ResourceLimit(limit)) if limit.used == 2));
}

#[test]
fn feature_result_refuses_entity_limit_before_creation() {
    let source = test_feature(0, 1, &[(0, 1)]);
    let properties = [
        test_property(
            1,
            PmDcFeaturePropertyKind::References {
                family: PmDcFeatureReferenceFamily::ObjectCollection,
                items: reference_list(&[3]),
            },
        ),
        test_property(
            2,
            PmDcFeaturePropertyKind::SurfaceBody { body: reference(0) },
        ),
    ];
    let index = test_projection_index(&properties, &[], &[], &[], &[], &[], &[], &[]);
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_entities = 0;
    let (ctx, _) =
        DecodeContext::from_root_bytes(&[], &arena, &policy).expect("projection context");
    assert!(matches!(
        super::feature_result(&ctx, &source, 0, &index),
        Some(Err(CodecError::ResourceLimit(limit)))
            if limit.dimension == ResourceDimension::Entities
                && limit.operation == "project Inventor feature result topology"
    ));
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &DecodePolicy::service())
        .expect("service projection context");
    assert!(matches!(
        super::feature_result(&ctx, &source, 0, &index),
        Some(Ok(_))
    ));
}

#[test]
fn duplicate_feature_result_members_skip_without_entity_charge() {
    let source = test_feature(0, 1, &[(0, 1)]);
    let properties = [
        test_property(
            1,
            PmDcFeaturePropertyKind::References {
                family: PmDcFeatureReferenceFamily::ObjectCollection,
                items: reference_list(&[3, 3]),
            },
        ),
        test_property(
            2,
            PmDcFeaturePropertyKind::SurfaceBody { body: reference(0) },
        ),
    ];
    let index = test_projection_index(&properties, &[], &[], &[], &[], &[], &[], &[]);
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_entities = 0;
    let (ctx, _) =
        DecodeContext::from_root_bytes(&[], &arena, &policy).expect("projection context");
    assert!(super::feature_result(&ctx, &source, 0, &index).is_none());
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &DecodePolicy::service())
        .expect("service projection context");
    assert!(super::feature_result(&ctx, &source, 0, &index).is_none());
}

#[test]
fn feature_result_refuses_collection_limit_before_distinct_precheck() {
    let source = test_feature(0, 1, &[(0, 1)]);
    let properties = [
        test_property(
            1,
            PmDcFeaturePropertyKind::References {
                family: PmDcFeatureReferenceFamily::ObjectCollection,
                items: reference_list(&[3, 3]),
            },
        ),
        test_property(
            2,
            PmDcFeaturePropertyKind::SurfaceBody { body: reference(0) },
        ),
    ];
    let index = test_projection_index(&properties, &[], &[], &[], &[], &[], &[], &[]);
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_collection_items = 2;
    let (ctx, _) =
        DecodeContext::from_root_bytes(&[], &arena, &policy).expect("projection context");
    assert!(matches!(
        super::feature_result(&ctx, &source, 0, &index),
        Some(Err(CodecError::ResourceLimit(limit)))
            if limit.dimension == ResourceDimension::CollectionItems
                && limit.operation == "precheck distinct Inventor feature result bodies"
    ));
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &DecodePolicy::service())
        .expect("service projection context");
    assert!(super::feature_result(&ctx, &source, 0, &index).is_none());
}

#[test]
fn feature_result_stops_at_first_missing_body_reference() {
    let run = |references: &[u32], max_work_units: u64, measure_work: bool| {
        let source = test_feature(0, 1, &[(0, 1)]);
        let properties = [test_property(
            1,
            PmDcFeaturePropertyKind::References {
                family: PmDcFeatureReferenceFamily::ObjectCollection,
                items: reference_list(references),
            },
        )];
        let index = test_projection_index(&properties, &[], &[], &[], &[], &[], &[], &[]);
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        policy.limits.max_work_units = max_work_units;
        let (ctx, _) =
            DecodeContext::from_root_bytes(&[], &arena, &policy).expect("projection context");
        let skipped = super::feature_result(&ctx, &source, 0, &index).is_none();
        let used = measure_work.then(|| {
            let refusal = ctx
                .charge_work_limit(max_work_units, "measure Inventor feature-result prefix")
                .expect_err("the probe exceeds the remaining work allowance");
            assert_eq!(refusal.dimension, ResourceDimension::WorkUnits);
            refusal.used
        });
        if !measure_work {
            ctx.finish_session()
                .expect("early result rejection leaves the session clean");
        }
        (skipped, used)
    };

    let (single_skipped, Some(prefix_work)) = run(&[0], 1_000_000, true) else {
        panic!("a null first body reference skips the candidate and measures its prefix");
    };
    assert!(single_skipped);
    let tail_len = usize::try_from(prefix_work.checked_add(1).expect("tail length fits"))
        .expect("tail length fits usize");
    let references = vec![0; tail_len];
    assert!(run(&references, prefix_work, false).0);
}

#[test]
fn feature_label_errors_do_not_retain_error_text() {
    let label = test_label(0, 1, EXTRUSION_CLASS_ID, &[]);
    for (name, class_id) in [("", "ab".repeat(16)), ("valid", "z".repeat(32))] {
        let wire = PmDcFeatureLabelPayloadWire {
            save_version_major: label.save_version_major,
            header: label.header.clone(),
            index: label.index,
            participants: label.participants.clone(),
            name: name.to_owned(),
            class_id,
        };
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        policy.limits.max_retained_bytes = 0;
        policy.limits.max_work_units = 0;
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("context");
        assert!(matches!(
            wire.into_record(&ctx),
            Err(CodecError::Malformed(_))
        ));
    }
}

#[test]
fn projected_feature_tags_retain_literals_without_variable_work() {
    let arena = DecodeArena::new();
    for tag in ["extrude", "fillet", "chamfer", "hole"] {
        let mut policy = DecodePolicy::service();
        policy.limits.max_collection_items = 0;
        policy.limits.max_entities = 1;
        policy.limits.max_work_units = 0;
        policy.limits.max_retained_bytes =
            cadmpeg_core::decode::u64_from_index(tag.len());
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("context");
        assert_eq!(
            super::admit_projected_feature(&ctx, tag).expect("retained tag"),
            tag
        );
        assert!(matches!(ctx.charge_work(1, "probe"),
        Err(CodecError::ResourceLimit(limit))
            if limit.dimension == ResourceDimension::WorkUnits && limit.used == 0));
    }

    let source = test_feature(0, 0, &[]);
    let native_id = "inventor:pmdc:feature#generated-0";
    // The caller retains the tag and one formatted native ID: 7 + ID length bytes.
    let retained_needed = "extrude".len() + native_id.len();
    let mut policy = DecodePolicy::service();
    policy.limits.max_collection_items = 0;
    policy.limits.max_entities = 1;
    for shortfall in [1, 0] {
        policy.limits.max_retained_bytes =
            u64::try_from(retained_needed - shortfall).expect("feature budget fits");
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("context");
        assert_eq!(
            super::admit_projected_feature(&ctx, "extrude").expect("charged tag copy"),
            "extrude"
        );
        let id = source.id(&ctx);
        if shortfall == 0 {
            assert_eq!(id.expect("single native ID charge"), native_id);
        } else {
            assert!(matches!(id, Err(CodecError::ResourceLimit(limit))
                if limit.dimension == ResourceDimension::RetainedBytes
                    && limit.operation == "retain Inventor PmDc record identity"));
        }
    }
}

#[test]
fn feature_projection_refuses_entity_limit_before_creation() {
    let source = test_feature(0, 0, &[]);
    let _label = test_label(0, 1, EXTRUSION_CLASS_ID, &[]);
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_entities = 0;
    let (ctx, _) =
        DecodeContext::from_root_bytes(&[], &arena, &policy).expect("projection context");
    assert!(matches!(
        super::admit_projected_feature(&ctx, "extrude"),
        Err(CodecError::ResourceLimit(limit))
            if limit.dimension == ResourceDimension::Entities
                && limit.operation == "project Inventor feature"
    ));
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &DecodePolicy::service())
        .expect("service projection context");
    assert!(super::admit_projected_feature(&ctx, "extrude").is_ok());
    assert_eq!(
        source.id(&ctx).expect("caller retains the native ID"),
        "inventor:pmdc:feature#generated-0"
    );
}

#[test]
fn closed_edge_items_stops_at_first_missing_reference() {
    let items = reference_list(&[0, 1, 2, 3]);
    let index = test_projection_index(&[], &[], &[], &[], &[], &[], &[], &[]);
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    // The null first reference stops validation after one source step.
    policy.limits.max_work_units = 1;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("context");
    assert!(
        !super::closed_edge_items(&ctx, "generated", &items, &index).expect("early rejection")
    );
    assert!(matches!(ctx.charge_work(1, "probe"),
        Err(CodecError::ResourceLimit(limit)) if limit.used == 1));
}

#[test]
fn fillet_set_projection_stops_at_first_missing_set_reference() {
    let run = |references: &[u32], max_work_units: u64, measure_work: bool| {
        let properties = [
            test_property(
                0,
                PmDcFeaturePropertyKind::Enumeration {
                    family: PmDcFeatureEnumFamily::Fillet,
                    type_value: 2,
                    value: 0,
                },
            ),
            test_property(
                1,
                PmDcFeaturePropertyKind::References {
                    family: PmDcFeatureReferenceFamily::FilletEdgeSets,
                    items: reference_list(references),
                },
            ),
        ];
        let source = test_feature(100, 12, &[(0, 1), (11, 0)]);
        let label = test_label(100, 7, FILLET_CLASS_ID, &[]);
        let index = test_projection_index(&properties, &[], &[], &[], &[], &[], &[], &[]);
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        policy.limits.max_work_units = max_work_units;
        let (ctx, _) =
            DecodeContext::from_root_bytes(&[], &arena, &policy).expect("projection context");
        let skipped = super::project_fillet(&ctx, &source, &label, &index).is_none();
        let used = measure_work.then(|| {
            let refusal = ctx
                .charge_work_limit(max_work_units, "measure Inventor fillet-set prefix")
                .expect_err("the probe exceeds the remaining work allowance");
            assert_eq!(refusal.dimension, ResourceDimension::WorkUnits);
            refusal.used
        });
        if !measure_work {
            ctx.finish_session()
                .expect("early fillet rejection leaves the session clean");
        }
        (skipped, used)
    };

    let (single_skipped, Some(prefix_work)) = run(&[0], 1_000_000, true) else {
        panic!("a null first fillet-set reference skips the candidate and measures its prefix");
    };
    assert!(single_skipped);
    let tail_len = usize::try_from(prefix_work.checked_add(1).expect("tail length fits"))
        .expect("tail length fits usize");
    let references = vec![0; tail_len];
    assert!(run(&references, prefix_work, false).0);
}

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
        vec![fillet_properties[8]
            .id(&ctx)
            .expect("service fixture record identity")]
    );

    let mut policy = DecodePolicy::service();
    // One fillet group, one body member and one uniqueness-index slot use three slots.
    policy.limits.max_collection_items = 3;
    let (limited, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("context");
    assert!(project_fillet(&limited, &fillet, &label, &index)
        .expect("candidate")
        .is_ok());

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
    let mut policy = DecodePolicy::service();
    // One body member and one uniqueness-index slot use two slots; the chamfer group is fixed.
    policy.limits.max_collection_items = 2;
    let (limited, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("context");
    assert!(project_chamfer(&limited, &chamfer, &label, &index)
        .expect("candidate")
        .is_ok());
    assert!(matches!(
        projected.evaluation.definition(),
        FeatureDefinition::Operation(FeatureOperation::Chamfer {
            groups,
            flip_direction: true
        }) if matches!(groups[0].spec, ChamferSpec::Distance { distance: actual_distance } if actual_distance.get() == 1.25)
    ));
}

#[test]
fn duplicate_extrusion_selections_skip_before_feature_entity_charge() {
    let mut policy = DecodePolicy::service();
    policy.limits.max_entities = 0;
    assert!(generated_extrusion(&[4, 4], policy).is_none());
}

#[test]
fn duplicate_extrusion_selection_stops_before_tail_precharge() {
    let mut measurement_policy = DecodePolicy::service();
    measurement_policy.limits.max_work_units = 1_000_000;
    let (projection, Some(prefix_work), _) =
        generated_extrusion_with_work(&[4, 4], measurement_policy, true)
    else {
        panic!("duplicate selections skip the candidate and measure their work");
    };
    assert!(projection.is_none());

    let tail_len = usize::try_from(prefix_work.checked_add(1).expect("tail length fits"))
        .expect("tail length fits usize");
    let selections = vec![4; tail_len];
    let mut policy = DecodePolicy::service();
    policy.limits.max_work_units = prefix_work;
    assert!(generated_extrusion(&selections, policy).is_none());
}

#[test]
fn extrusion_selection_resolution_stops_at_first_null_reference() {
    let mut policy = DecodePolicy::service();
    policy.limits.max_work_units = u64::MAX;
    let probe = RefusalProbe::arm(
        ResourceDimension::WorkUnits,
        "resolve Inventor extrusion selections",
        Some(3),
    );
    let (projection, _, session) =
        generated_extrusion_with_work(&[0, 4, 5], policy, false);
    drop(probe);
    assert!(projection.is_none());
    session.expect("stepwise selection rejection leaves the session clean");
}

#[test]
fn extrusion_projection_charges_only_stored_collection_members() {
    let mut policy = DecodePolicy::service();
    // Selection uniqueness, selected property, body member, body uniqueness
    // and planar selection uniqueness each use one slot: five in total.
    policy.limits.max_collection_items = 5;
    let (feature, result) = generated_extrusion(&[4], policy)
        .expect("candidate")
        .expect("five stored member and index slots");
    assert_eq!(feature.source_tag.as_deref(), Some("extrude"));
    assert_eq!(feature.name.as_deref(), Some("Feature 5"));
    assert_eq!(result.bodies().len(), 1);
}

#[test]
fn projects_generated_extrusion() {
    let (projected, _) = generated_extrusion(&[4], DecodePolicy::service())
        .expect("extrusion candidate")
        .expect("extrusion projection");
    assert!(matches!(
        projected.evaluation.definition(),
        FeatureDefinition::Operation(FeatureOperation::Extrude {
            direction: ExtrudeDirection::Explicit {
                vector: geometry_1,
                ..
            },
            extent: ExtrudeExtent::OneSided {
                side: ExtrudeSide {
                    termination: LinearTermination::Blind {
                        length: actual_length
                    },
                    draft: Some(actual_draft),
                    ..
                }
            },
            op: BooleanOp::NewBody,
            ..
        }) if ( actual_length.get() == 12.0 && actual_draft.get() == 0.1) && matches!(geometry_1.get(), Vector3 { z: -1.0, .. })
    ));
}

#[test]
fn projects_generated_hole() {
    let raw_parameters = (70..76).map(raw_parameter).collect::<Vec<_>>();
    let neutral_parameters = [
        ParameterValue::Length(Length::new(5.0).expect("finite length fixture")),
        ParameterValue::Length(Length::new(20.0).expect("finite length fixture")),
        ParameterValue::Length(Length::new(9.0).expect("finite length fixture")),
        ParameterValue::Length(Length::new(3.0).expect("finite length fixture")),
        ParameterValue::Angle(Angle::new(1.5).expect("finite angle fixture")),
        ParameterValue::Angle(Angle::new(2.0).expect("finite angle fixture")),
    ]
    .into_iter()
    .zip(&raw_parameters)
    .map(|(value, raw)| neutral_parameter(raw, value))
    .collect::<Vec<_>>();
    let transform = Located::new(
        crate::sketch::PmDcTransformPayload {
            save_version_major: 16,
            header: test_header(),
            prefix_present: false,
            matrix: crate::compact_matrix::CompactMatrix::try_from_rows(
                0,
                0,
                [
                    [1.0, 0.0, 0.0, 1.0],
                    [0.0, 1.0, 0.0, 2.0],
                    [0.0, 0.0, 1.0, 3.0],
                    [0.0, 0.0, 0.0, 1.0],
                ],
            )
            .expect("finite explicit matrix fixture"),
        },
        crate::record_identity::RecordTypeId::try_from(
            "184d8790d011f8d10008cabc0663dc09".to_owned(),
        )
        .expect("test GUID"),
        segment()
            .try_clone_for_decode(
                &cadmpeg_test_support::service_decode_context(),
                "Inventor located fixture token",
            )
            .expect("service fixture token"),
        60,
    );
    let direction = Located::new(
        crate::sketch::PmDcDirectionPayload {
            save_version_major: 16,
            header: test_header(),
            entity_flags: 0,
            parameter: cadmpeg_ir::scalar::FiniteReal::ZERO,
            extension: None,
            direction: [
                cadmpeg_ir::scalar::FiniteReal::ZERO,
                cadmpeg_ir::scalar::FiniteReal::ZERO,
                cadmpeg_ir::scalar::FiniteReal::ONE.negated(),
            ],
        },
        crate::record_identity::RecordTypeId::try_from(
            "40df52ced011d0d20008ccbc0663dc09".to_owned(),
        )
        .expect("test GUID"),
        segment()
            .try_clone_for_decode(
                &cadmpeg_test_support::service_decode_context(),
                "Inventor located fixture token",
            )
            .expect("service fixture token"),
        61,
    );
    let properties = vec![
        test_property(
            1,
            PmDcFeaturePropertyKind::Enumeration {
                family: PmDcFeatureEnumFamily::Hole,
                type_value: 3,
                value: 2,
            },
        ),
        test_property(
            2,
            PmDcFeaturePropertyKind::Enumeration {
                family: PmDcFeatureEnumFamily::Extent,
                type_value: 11,
                value: 5,
            },
        ),
        test_property(
            3,
            PmDcFeaturePropertyKind::Boolean {
                name: String::new(),
                name_value: 0,
                value: false,
            },
        ),
        test_property(
            4,
            PmDcFeaturePropertyKind::Placement {
                transform: reference(61),
                point: reference(90),
                value: reference(91),
            },
        ),
        test_property(
            5,
            PmDcFeaturePropertyKind::References {
                family: PmDcFeatureReferenceFamily::ObjectCollection,
                items: reference_list(&[7]),
            },
        ),
        test_property(
            6,
            PmDcFeaturePropertyKind::SurfaceBody {
                body: reference(30),
            },
        ),
    ];
    let feature = test_feature(
        100,
        25,
        &[
            (0, 1),
            (1, 70),
            (2, 71),
            (3, 72),
            (4, 73),
            (5, 74),
            (6, 75),
            (8, 60),
            (9, 2),
            (16, 61),
            (17, 3),
            (21, 4),
            (24, 5),
        ],
    );
    let label = test_label(100, 9, HOLE_CLASS_ID, &[]);
    let index = test_projection_index(
        &properties,
        &raw_parameters,
        &neutral_parameters,
        &[],
        &[],
        std::slice::from_ref(&direction),
        std::slice::from_ref(&transform),
        &[],
    );
    let arena = DecodeArena::new();
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &DecodePolicy::service())
        .expect("projection context");
    let (projected, _) = project_hole(&ctx, &feature, &label, &index)
        .expect("hole candidate")
        .expect("hole projection");
    let mut policy = DecodePolicy::service();
    // One body member and one uniqueness-index slot use two slots; the placement is fixed.
    policy.limits.max_collection_items = 2;
    let (limited, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("context");
    assert!(project_hole(&limited, &feature, &label, &index)
        .expect("candidate")
        .is_ok());
    assert!(matches!(
        projected.evaluation.definition(), FeatureDefinition::Operation(FeatureOperation::Hole {
            placements,
            shape,

            extent: Some(LinearTermination::ThroughAll {}),
            ..
        }) if matches!((shape.construction(), &shape.diameter(),), (cadmpeg_ir::features::holes::HoleConstruction::Form {
                kind: HoleKind::CounterboreDrilled {
                    diameter: actual_diameter,
                    depth: actual_depth,
                    drill_point_angle: actual_drill_point_angle
                },
                ..
            }, Some(actual_diameter_2),) if (matches!(
            placements.as_deref(),
            Some([HolePlacement::Directed {
                position: geometry_1,
                direction: geometry_2
            }])
         if matches!(geometry_1.get(), Point3 { x: 10.0, y: 20.0, z: 30.0 }) && matches!(geometry_2.get(), Vector3 { z: -1.0, .. }))) && actual_diameter.get() == 9.0 && actual_depth.get() == 3.0 && actual_drill_point_angle.get() == 2.0 && actual_diameter_2.get() == 5.0)));
}
