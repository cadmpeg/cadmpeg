use super::*;

#[test]
fn feature_terminator_refuses_collection_limit_before_push() {
    let mut payload = content(0);
    payload.extend_from_slice(&0u32.to_le_bytes());
    assert_eq!(
        inventory_with_record(END_OF_FEATURES_TYPE, &payload, DecodePolicy::service())
            .expect("terminator is admitted")
            .terminators
            .len(),
        1
    );
    let mut policy = DecodePolicy::service();
    policy.limits.max_collection_items = 0;
    assert!(matches!(
        inventory_with_record(END_OF_FEATURES_TYPE, &payload, policy),
        Err(CodecError::ResourceLimit(limit))
            if limit.dimension == ResourceDimension::CollectionItems
                && limit.operation == "admit Inventor PmDc feature terminator record"
                && limit.used == 0
    ));
}

#[test]
fn feature_terminator_refuses_retained_limit_before_identity_copy() {
    let mut payload = content(0);
    payload.extend_from_slice(&0u32.to_le_bytes());
    let mut policy = DecodePolicy::service();
    policy.limits.max_retained_bytes = 31;
    assert!(matches!(
        inventory_with_record(END_OF_FEATURES_TYPE, &payload, policy),
        Err(CodecError::ResourceLimit(limit))
            if limit.dimension == ResourceDimension::RetainedBytes
                && limit.operation == "retain Inventor PmDc record type id"
                && limit.used == 0
    ));
    policy.limits.max_retained_bytes = 32;
    assert!(matches!(
        inventory_with_record(END_OF_FEATURES_TYPE, &payload, policy),
        Err(CodecError::ResourceLimit(limit))
            if limit.dimension == ResourceDimension::RetainedBytes
                && limit.operation == "retain Inventor PmDc record segment token"
                && limit.used == 32
    ));
}

#[test]
fn feature_parse_issue_refuses_collection_limit_before_push() {
    assert_eq!(
        inventory_with_record(END_OF_FEATURES_TYPE, &[], DecodePolicy::service())
            .expect("truncated terminator becomes an issue")
            .issues
            .len(),
        1
    );
    let mut policy = DecodePolicy::service();
    policy.limits.max_collection_items = 0;
    assert!(matches!(
        inventory_with_record(END_OF_FEATURES_TYPE, &[], policy),
        Err(CodecError::ResourceLimit(limit))
            if limit.dimension == ResourceDimension::CollectionItems
                && limit.operation == "admit Inventor PmDc feature issue"
                && limit.used == 0
    ));
}

#[test]
fn feature_parse_issue_refuses_entity_limit_before_push() {
    let mut policy = DecodePolicy::service();
    policy.limits.max_entities = 0;
    assert!(matches!(
        inventory_with_record(END_OF_FEATURES_TYPE, &[], policy),
        Err(CodecError::ResourceLimit(limit))
            if limit.dimension == ResourceDimension::Entities
                && limit.operation == "admit Inventor PmDc feature issue"
    ));
    assert_eq!(
        inventory_with_record(END_OF_FEATURES_TYPE, &[], DecodePolicy::service())
            .expect("service issue")
            .issues
            .len(),
        1
    );
}

#[test]
fn feature_parse_issue_copies_refuse_retained_limits_before_creation() {
    let admitted = inventory_with_record(END_OF_FEATURES_TYPE, &[], DecodePolicy::service())
        .expect("truncated terminator becomes an issue");
    let issue = &admitted.issues[0];
    let detail_len = issue.detail.len();
    let segment_token_len = issue.segment_token.as_str().len();
    // Construction retains the 32-byte type id, token, then detail.
    for (limit_bytes, operation, used) in [
        (32 - 1, "retain Inventor PmDc feature issue type id", 0),
        (
            32 + segment_token_len - 1,
            "retain Inventor PmDc feature issue segment token",
            32,
        ),
        (
            32 + segment_token_len + detail_len - 1,
            "retain Inventor PmDc feature issue detail",
            32 + segment_token_len,
        ),
    ] {
        let mut policy = DecodePolicy::service();
        policy.limits.max_retained_bytes = cadmpeg_core::decode::u64_from_index(limit_bytes);
        assert!(matches!(
            inventory_with_record(END_OF_FEATURES_TYPE, &[], policy),
            Err(CodecError::ResourceLimit(limit))
                if limit.dimension == ResourceDimension::RetainedBytes
                    && limit.operation == operation
                    && limit.used == cadmpeg_core::decode::u64_from_index(used)
        ));
    }
}

#[test]
fn located_label_admission_rejects_empty_name_and_wrong_class_width() {
    let label = test_label(0, 1, EXTRUSION_CLASS_ID, &[]);
    let valid = serde_json::to_value(&label).expect("valid label fixture");
    let admitted = decode_label(valid.clone()).expect("valid label fixture");
    assert_eq!(
        serde_json::to_value(admitted).expect("valid label fixture"),
        valid
    );
    for (field, value) in [
        ("name", String::new()),
        ("class_id", "a".repeat(31)),
        ("class_id", "a".repeat(33)),
    ] {
        let mut wire = valid.clone();
        wire[field] = serde_json::json!(value);
        assert!(decode_label(wire)
            .expect_err("invalid label")
            .contains(field));
    }
    let mut wire = valid;
    wire["class_id"] = serde_json::json!("z".repeat(32));
    assert!(decode_label(wire).is_err());
}

#[test]
fn parses_generated_feature_and_terminator() {
    let mut feature = content(7);
    feature.extend_from_slice(&(-1i32).to_le_bytes());
    feature.extend_from_slice(&42u32.to_le_bytes());
    feature.extend_from_slice(&2u16.to_le_bytes());
    feature.extend_from_slice(&0x3000u16.to_le_bytes());
    feature.extend_from_slice(&2u32.to_le_bytes());
    feature.extend_from_slice(&[0; 8]);
    feature.extend_from_slice(&0x8000_0004u32.to_le_bytes());
    feature.extend_from_slice(&5u32.to_le_bytes());
    feature.extend_from_slice(&9u32.to_le_bytes());
    let parsed = parse(&feature, |ctx, source| {
        parse_feature(ctx, source, 16).expect("feature")
    });
    assert_eq!(parsed.state, -1);
    assert_eq!(parsed.outline_value, 42);
    assert_eq!(parsed.properties.references().len(), 2);
    assert!(parsed.properties.references()[0].qualified());
    assert_eq!(parsed.value, 9);

    let mut terminator = content(8);
    terminator.extend_from_slice(&(-1i32).to_le_bytes());
    let parsed = parse(&terminator, |_, source| {
        parse_terminator(source, 16).expect("terminator")
    });
    assert_eq!(parsed.state, -1);
}

#[test]
fn feature_label_keeps_its_class_id_bytes_without_scoped_text() {
    let bytes = feature_label_bytes();
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_materialized_bytes = 0;
    let (ctx, source) =
        DecodeContext::from_root_bytes(&bytes, &arena, &policy).expect("label view");
    let label = parse_label(&ctx, source, 22).expect("label needs no scoped storage");
    assert_eq!(label.name.as_str(), "a");
    assert_eq!(label.class_id, ClassId([0xab; 16]));
}

#[test]
fn feature_record_forms_refuse_collection_limit_before_push() {
    let mut feature = content(0);
    feature.extend_from_slice(&[0; 8]);
    feature.extend(references(&[]));
    feature.extend_from_slice(&0u32.to_le_bytes());
    let mut property = content(0);
    property.extend(utf16("a"));
    property.extend_from_slice(&0u32.to_le_bytes());
    property.push(1);
    let mut entity_link = Vec::new();
    entity_link.extend_from_slice(&0u32.to_le_bytes());
    entity_link.extend_from_slice(&16u16.to_le_bytes());
    entity_link.extend_from_slice(&[0; 32]);
    let cases = [
        (
            FEATURE_TYPE,
            feature,
            0,
            "admit Inventor PmDc feature record",
        ),
        (
            RECTANGULAR_PATTERN_FEATURE_TYPE,
            pattern_feature_bytes(16, PmDcPatternFamily::Rectangular),
            28,
            "admit Inventor PmDc pattern feature record",
        ),
        (
            MIRROR_FEATURE_TYPE,
            pattern_feature_bytes(16, PmDcPatternFamily::Mirror),
            15,
            "admit Inventor PmDc pattern feature record",
        ),
        (
            FEATURE_LABEL_TYPE,
            feature_label_bytes(),
            0,
            "admit Inventor PmDc feature label record",
        ),
        (
            ENTITY_STYLE_LINK_TYPE,
            entity_link,
            0,
            "admit Inventor PmDc entity style link record",
        ),
        (
            BOOLEAN_TYPE,
            property,
            0,
            "admit Inventor PmDc feature property record",
        ),
    ];
    for (type_id, payload, parser_items, operation) in cases {
        let admitted = inventory_with_record(type_id, &payload, DecodePolicy::service())
            .expect("feature record is admitted");
        assert_eq!(
            admitted.features.len()
                + admitted.pattern_features.len()
                + admitted.labels.len()
                + admitted.entity_style_links.len()
                + admitted.properties.len(),
            1,
            "{operation}"
        );
        assert!(admitted.issues.is_empty());
        let mut policy = DecodePolicy::service();
        policy.limits.max_collection_items = parser_items;
        assert!(matches!(
            inventory_with_record(type_id, &payload, policy),
            Err(CodecError::ResourceLimit(limit))
                if limit.dimension == ResourceDimension::CollectionItems
                    && limit.operation == operation
                    && limit.used == parser_items
        ));
    }
}

#[test]
fn parses_generated_pattern_feature_branches() {
    for (version, family, slots, extensions) in [
        (16, PmDcPatternFamily::Rectangular, 26, 0),
        (21, PmDcPatternFamily::Rectangular, 32, 0),
        (16, PmDcPatternFamily::Mirror, 13, 0),
        (21, PmDcPatternFamily::Mirror, 13, 6),
    ] {
        let bytes = pattern_feature_bytes(version, family);
        let parsed = parse(&bytes, |ctx, source| {
            parse_pattern_feature(ctx, source, version, family).expect("pattern feature")
        });
        assert_eq!(parsed.family, family);
        assert_eq!(parsed.participants.references().len(), 2);
        assert_eq!(parsed.property_slots.len(), slots);
        assert_eq!(parsed.extension_values.len(), extensions);
    }
}

#[test]
fn pattern_feature_property_slots_refuse_collection_limit_before_allocation() {
    for (version, family, slots, extension_slots) in [
        (16, PmDcPatternFamily::Rectangular, 26, 0),
        (21, PmDcPatternFamily::Rectangular, 32, 0),
        (16, PmDcPatternFamily::Mirror, 13, 0),
        (21, PmDcPatternFamily::Mirror, 13, 6),
    ] {
        let bytes = pattern_feature_bytes(version, family);
        let mut policy = DecodePolicy::service();
        let collection_limit = 2 + slots + extension_slots - 1;
        // Two participants, property slots, and extension slots precede the final property push.
        policy.limits.max_collection_items = collection_limit;
        let arena = DecodeArena::new();
        let (ctx, source) = DecodeContext::from_root_bytes(&bytes, &arena, &policy)
            .expect("pattern feature view");
        assert!(matches!(
            parse_pattern_feature(&ctx, source, version, family),
            Err(CodecError::ResourceLimit(limit))
                if limit.dimension == ResourceDimension::CollectionItems
                    && limit.operation == "admit Inventor pattern feature property slots"
                    && limit.used == collection_limit
        ));
    }
}

#[test]
fn pattern_feature_extension_values_refuse_collection_limit_before_allocation() {
    let bytes = pattern_feature_bytes(21, PmDcPatternFamily::Mirror);
    let mut policy = DecodePolicy::service();
    // Two participants, eleven property slots and five extension pushes
    // use 2 + 11 + 5 = 18 slots before the sixth extension push.
    policy.limits.max_collection_items = 2 + 11 + 6 - 1;
    let arena = DecodeArena::new();
    let (ctx, source) =
        DecodeContext::from_root_bytes(&bytes, &arena, &policy).expect("pattern feature view");
    assert!(matches!(
        parse_pattern_feature(&ctx, source, 21, PmDcPatternFamily::Mirror),
        Err(CodecError::ResourceLimit(limit))
                if limit.dimension == ResourceDimension::CollectionItems
                && limit.operation == "admit Inventor pattern feature extension values"
                && limit.used == 18
    ));
    let (ctx, source) =
        DecodeContext::from_root_bytes(&bytes, &arena, &DecodePolicy::service())
            .expect("pattern feature view");
    assert_eq!(
        parse_pattern_feature(&ctx, source, 21, PmDcPatternFamily::Mirror)
            .expect("pattern feature is admitted")
            .extension_values
            .len(),
        6
    );
}

#[test]
fn parses_generated_feature_properties_and_label() {
    let mut enumeration = content(10);
    enumeration.extend_from_slice(&5i16.to_le_bytes());
    enumeration.extend_from_slice(&3u16.to_le_bytes());
    let parsed = parse(&enumeration, |ctx, source| {
        parse_part_operation(ctx, source, 16).expect("enumeration")
    });
    assert!(matches!(
        parsed.kind,
        PmDcFeaturePropertyKind::Enumeration {
            family: PmDcFeatureEnumFamily::PartOperation,
            type_value: 5,
            value: 3
        }
    ));

    let mut chamfer = content(10);
    chamfer.extend_from_slice(&2i16.to_le_bytes());
    chamfer.extend_from_slice(&0u16.to_le_bytes());
    chamfer.extend_from_slice(&0u32.to_le_bytes());
    let parsed = parse(&chamfer, |ctx, source| {
        parse_chamfer(ctx, source, 16).expect("chamfer enumeration")
    });
    assert!(matches!(
        parsed.kind,
        PmDcFeaturePropertyKind::Enumeration {
            family: PmDcFeatureEnumFamily::Chamfer,
            type_value: 2,
            value: 0
        }
    ));

    let mut fillet_selection = content(10);
    fillet_selection.extend_from_slice(&4u32.to_le_bytes());
    fillet_selection.extend_from_slice(&0u32.to_le_bytes());
    let parsed = parse(&fillet_selection, |ctx, source| {
        parse_fillet_edge_selection(ctx, source, 16).expect("fillet edge selection")
    });
    assert!(matches!(
        parsed.kind,
        PmDcFeaturePropertyKind::WideEnumeration {
            type_value: 4,
            value: 0
        }
    ));

    let mut boolean = content(11);
    boolean.extend_from_slice(&utf16("solid"));
    boolean.extend_from_slice(&7u32.to_le_bytes());
    boolean.push(1);
    let parsed = parse(&boolean, |ctx, source| {
        parse_boolean(ctx, source, 16).expect("Boolean")
    });
    assert!(matches!(
        parsed.kind,
        PmDcFeaturePropertyKind::Boolean { value: true, .. }
    ));

    let mut collection = content(12);
    collection.extend_from_slice(&references(&[0x8000_0004, 0x8000_0005]));
    let parsed = parse(&collection, |ctx, source| {
        parse_boundary_patch(ctx, source, 16).expect("boundary patch")
    });
    assert!(matches!(
        parsed.kind,
        PmDcFeaturePropertyKind::References { items, .. }
            if items.references().len() == 2
    ));

    let mut rdx = content(13);
    rdx.extend_from_slice(&utf16("RDxVar1"));
    rdx.extend_from_slice(&0u32.to_le_bytes());
    rdx.extend_from_slice(&2u32.to_le_bytes());
    rdx.extend_from_slice(&3u32.to_le_bytes());
    parse(&rdx, |ctx, source| {
        parse_rdx_variable(ctx, source, 16).expect("RDx variable")
    });

    let mut surface = content(14);
    surface.extend_from_slice(&0x8000_0006u32.to_le_bytes());
    parse(&surface, |ctx, source| {
        parse_surface_body(ctx, source, 16).expect("surface body")
    });

    let mut selection = content(15);
    selection.extend_from_slice(&0x8000_0007u32.to_le_bytes());
    selection.push(0);
    parse(&selection, |ctx, source| {
        parse_profile_selection(ctx, source, 16).expect("profile selection")
    });

    let mut entity_link = Vec::new();
    entity_link.extend_from_slice(&0u32.to_le_bytes());
    entity_link.extend_from_slice(&16u16.to_le_bytes());
    entity_link.extend_from_slice(&0u32.to_le_bytes());
    entity_link.extend_from_slice(&0u32.to_le_bytes());
    entity_link.extend_from_slice(&0x8000_0008u32.to_le_bytes());
    entity_link.extend_from_slice(&0u32.to_le_bytes());
    entity_link.extend_from_slice(&0x8000_0009u32.to_le_bytes());
    entity_link.extend_from_slice(&1u32.to_le_bytes());
    entity_link.extend_from_slice(&2u32.to_le_bytes());
    entity_link.extend_from_slice(&3u32.to_le_bytes());
    let parsed = parse(&entity_link, |_, source| {
        parse_entity_style_link(source, 16).expect("entity-style link")
    });
    assert_eq!(parsed.header.owner.index(), 8);
    assert_eq!(parsed.header.next.index(), 9);
    assert_eq!(parsed.associative_id, 2);

    let mut placement = content(17);
    placement.extend_from_slice(&0x8000_0009u32.to_le_bytes());
    placement.extend_from_slice(&0x8000_000au32.to_le_bytes());
    placement.extend_from_slice(&0x8000_000bu32.to_le_bytes());
    parse(&placement, |ctx, source| {
        parse_placement(ctx, source, 16).expect("placement")
    });

    let mut fillet_set = content(18);
    fillet_set.extend_from_slice(&0x8000_0010u32.to_le_bytes());
    fillet_set.extend_from_slice(&0x8000_0011u32.to_le_bytes());
    fillet_set.extend_from_slice(&0x8000_0012u32.to_le_bytes());
    fillet_set.extend_from_slice(&0x8000_0013u32.to_le_bytes());
    let parsed = parse(&fillet_set, |ctx, source| {
        parse_fillet_edge_set(ctx, source, 16).expect("fillet edge set")
    });
    assert!(matches!(
        parsed.kind,
        PmDcFeaturePropertyKind::FilletEdgeSet { radius, .. } if radius.index() == 17
    ));

    let mut edge_item = content(18);
    edge_item.extend_from_slice(&2u16.to_le_bytes());
    edge_item.extend_from_slice(&0x3000u16.to_le_bytes());
    edge_item.extend_from_slice(&1u32.to_le_bytes());
    edge_item.extend_from_slice(&[1u32.to_le_bytes(), 0u32.to_le_bytes()].concat());
    edge_item.extend_from_slice(&42u32.to_le_bytes());
    edge_item.extend_from_slice(&0i32.to_le_bytes());
    edge_item.extend_from_slice(&7u32.to_le_bytes());
    let parsed = parse(&edge_item, |ctx, source| {
        parse_edge_item(ctx, source, 16).expect("edge item")
    });
    assert!(matches!(
        parsed.kind,
        PmDcFeaturePropertyKind::EdgeItem {
            index_reference_value: 0,
            value: 7,
            ..
        }
    ));

    let mut label = Vec::new();
    label.extend_from_slice(&0u32.to_le_bytes());
    label.extend_from_slice(&18u16.to_le_bytes());
    label.extend_from_slice(&0u32.to_le_bytes());
    label.extend_from_slice(&0u32.to_le_bytes());
    label.extend_from_slice(&0x8000_000du32.to_le_bytes());
    label.extend_from_slice(&0u32.to_le_bytes());
    label.extend_from_slice(&0u32.to_le_bytes());
    label.extend_from_slice(&4u32.to_le_bytes());
    label.extend_from_slice(&references(&[0x8000_0013]));
    label.extend_from_slice(&utf16("Extrude1"));
    label.extend_from_slice(&[0xabu8; 16]);
    let parsed = parse(&label, |ctx, source| {
        parse_label(ctx, source, 16).expect("label")
    });
    assert_eq!(parsed.name.as_str(), "Extrude1");
    assert_eq!(parsed.participants.references().len(), 1);
    assert_eq!(parsed.class_id, ClassId([0xab; 16]));
}

#[test]
fn class_id_admits_only_the_canonical_lowercase_spelling() {
    let lower = "ab".repeat(16);
    let ctx = cadmpeg_test_support::service_decode_context();
    let class_id = ClassId::from_text(&lower).expect("lowercase class id");
    assert_eq!(
        class_id
            .into_text(&ctx, "retain Inventor class id test text")
            .expect("service class id text"),
        lower
    );
    let upper = "AB".repeat(16);
    assert!(ClassId::from_text(&upper).is_err());
}
