use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
use cadmpeg_core::CodecError;

fn reference_plane_error(policy: DecodePolicy) -> CodecError {
    const CLASS: &[u8] = b"moConstraintMidPlaneRefplaneData_c";
    let class_offset = 16;
    let body = class_offset + super::CLASS_MARKER.len() + 2 + CLASS.len();
    let mut payload = cadmpeg_test_support::service_decode_context()
        .alloc_filled(body + 48 + 16, 0u8, "fixed reference-plane fixture")
        .expect("fixed fixture allocation");
    payload[class_offset..class_offset + super::CLASS_MARKER.len()]
        .copy_from_slice(super::CLASS_MARKER);
    payload[class_offset + super::CLASS_MARKER.len()..class_offset + super::CLASS_MARKER.len() + 2]
        .copy_from_slice(&u16::try_from(CLASS.len()).unwrap().to_le_bytes());
    payload[class_offset + super::CLASS_MARKER.len() + 2..body].copy_from_slice(CLASS);
    for (relative, value) in [
        (8, 1.0e-16_f64),
        (16, 0.145),
        (24, 0.0),
        (32, 0.0),
        (40, 1.0),
    ] {
        payload[body + relative..body + relative + 8].copy_from_slice(&value.to_le_bytes());
    }
    let feature = super::Feature {
        id: "plane".into(),
        parent: "history".into(),
        xml_tag: "Feature".into(),
        tree_parent: None,
        source_id: super::FeatureSource::from_value(2080),
        ordinal: 0,
        name: "MidPlane".into(),
        kind: "Plane".into(),
        input_class: None,
        suppressed: false,
        parameters: std::collections::BTreeMap::default(),
        dimension_properties: std::collections::BTreeMap::default(),
        properties: std::collections::BTreeMap::default(),
        text: None,
        content: Vec::new(),
    };
    let mut histories = [super::FeatureHistory {
        id: "history".into(),
        part_name: None,
        properties: std::collections::BTreeMap::default(),
        content: Vec::new(),
        configurations: Vec::new(),
        features: vec![feature],
    }];
    let lane = super::FeatureInputLane {
        id: "lane".into(),
        configuration: None,
        native_payload: payload,
        classes: Vec::new(),
        names: vec![super::FeatureInputName {
            id: "name".into(),
            parent: "lane".into(),
            ordinal: 0,
            offset: 0,
            object_id: super::ObjectId::from_value(2080),
            value: "MidPlane".into(),
        }],
        scalars: Vec::new(),
        relation_bindings: Vec::new(),
        relation_instances: Vec::new(),
        body_selections: Vec::new(),
        edge_selections: Vec::new(),
        surface_selections: Vec::new(),
        generated_surface_identities: Vec::new(),
        references: Vec::new(),
        sketch_entities: Vec::new(),
    };
    let lanes = [lane];
    let arena = DecodeArena::new();
    let (ctx, _) =
        DecodeContext::from_root_bytes(&lanes[0].native_payload, &arena, &policy).unwrap();
    super::super::enrich_history_reference_planes(&ctx, &mut histories, &lanes).unwrap_err()
}

#[test]
fn reference_plane_enrichment_refuses_collection_limit() {
    let error = cadmpeg_test_support::refusal::resource_limit_at(
        ResourceDimension::CollectionItems,
        "index SLDPRT reference plane sources",
        |cap| {
            let mut policy = DecodePolicy::service();
            policy.limits.max_collection_items = cap;
            Err::<(), CodecError>(reference_plane_error(policy))
        },
    );
    assert!(matches!(error, CodecError::ResourceLimit(limit)
        if limit.dimension == ResourceDimension::CollectionItems
            && limit.operation == "index SLDPRT reference plane sources"));
}

#[test]
fn reference_plane_enrichment_refuses_materialized_limit() {
    let error = cadmpeg_test_support::refusal::resource_limit_at(
        cadmpeg_core::decode::ResourceDimension::MaterializedBytes,
        "retain SLDPRT plane frame source",
        |cap| {
            let mut policy = DecodePolicy::service();
            policy.limits.max_materialized_bytes = cap;
            Err::<(), cadmpeg_core::CodecError>(reference_plane_error(policy))
        },
    );
    assert!(matches!(error, CodecError::ResourceLimit(limit)
        if limit.dimension == ResourceDimension::MaterializedBytes
            && limit.operation == "retain SLDPRT plane frame source"));
}

#[test]
fn reference_plane_enrichment_refuses_work_limit() {
    // Admit source indexes before refusing feature traversal.
    let error = cadmpeg_test_support::refusal::resource_limit_at(
        ResourceDimension::WorkUnits,
        "scan SLDPRT reference plane features",
        |cap| {
            let mut policy = DecodePolicy::service();
            policy.limits.max_work_units = cap;
            Err::<(), CodecError>(reference_plane_error(policy))
        },
    );
    assert!(matches!(error, CodecError::ResourceLimit(limit)
        if limit.dimension == ResourceDimension::WorkUnits
            && limit.operation == "scan SLDPRT reference plane features"));
}

#[test]
fn component_face_path_is_scoped_until_candidate_formatting() {
    let class = b"moCompFace_c";
    let body = super::CLASS_MARKER.len() + 2 + class.len();
    let marker = body + crate::layout::component_face_flagged_operation_prefix::COMPONENT_MARKER;
    let mut payload = vec![0; marker - 12];
    payload[..super::CLASS_MARKER.len()].copy_from_slice(super::CLASS_MARKER);
    payload[super::CLASS_MARKER.len()..super::CLASS_MARKER.len() + 2]
        .copy_from_slice(&u16::try_from(class.len()).unwrap().to_le_bytes());
    payload[super::CLASS_MARKER.len() + 2..body].copy_from_slice(class);
    payload[body..body + 2].copy_from_slice(&0x802b_u16.to_le_bytes());
    payload[body + 2..body + 6].copy_from_slice(&2u32.to_le_bytes());
    assert_eq!(
        crate::resolved_features::selections::selection_vector_tail(&mut payload, &[6]),
        marker
    );
    let mut histories = [super::FeatureHistory {
        id: "history".into(),
        part_name: None,
        properties: std::collections::BTreeMap::new(),
        content: Vec::new(),
        configurations: Vec::new(),
        features: vec![super::Feature {
            id: "plane".into(),
            parent: "history".into(),
            xml_tag: "Plane".into(),
            tree_parent: None,
            source_id: super::FeatureSource::from_value(42),
            ordinal: 0,
            name: "FacePlane".into(),
            kind: "Plane".into(),
            input_class: Some("moRefPlane_c".into()),
            suppressed: false,
            parameters: std::collections::BTreeMap::new(),
            dimension_properties: std::collections::BTreeMap::new(),
            properties: std::collections::BTreeMap::new(),
            text: None,
            content: Vec::new(),
        }],
    }];
    let lane = super::FeatureInputLane {
        id: "lane".into(),
        configuration: None,
        native_payload: payload,
        classes: Vec::new(),
        names: vec![super::FeatureInputName {
            id: "plane-name".into(),
            parent: "lane".into(),
            ordinal: 0,
            offset: 0,
            object_id: super::ObjectId::from_value(42),
            value: "FacePlane".into(),
        }],
        scalars: Vec::new(),
        relation_bindings: Vec::new(),
        relation_instances: Vec::new(),
        body_selections: Vec::new(),
        edge_selections: Vec::new(),
        surface_selections: Vec::new(),
        generated_surface_identities: Vec::new(),
        references: Vec::new(),
        sketch_entities: Vec::new(),
    };
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_retained_bytes = u64::MAX;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    let probe = cadmpeg_core::decode::refusal_probe::RefusalProbe::arm(
        ResourceDimension::RetainedBytes,
        "decode SLDPRT component face record",
        None,
    );
    super::super::enrich_history_reference_planes(&ctx, &mut histories, &[lane]).unwrap();
    drop(probe);
    assert_eq!(
        histories[0].features[0].properties["ReferenceFaceNative"],
        format!("sldprt:feature-input:surface-component-ids#lane:{marker}:6")
    );
}

#[test]
fn compact_plane_source_membership_is_read_once() {
    let mut payload = 7u32.to_le_bytes().to_vec();
    payload.extend([
        2, 0, 0, 0, 0, 5, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0x2d, 0x80, 0x2b, 0x80,
    ]);
    let known = std::collections::HashSet::from([7]);
    let references = std::collections::HashSet::new();
    let ctx = cadmpeg_test_support::service_decode_context();
    assert_eq!(
        super::super::offset_plane_reference_source(&ctx, &payload, &known, &references, None)
            .unwrap(),
        Some(7)
    );
    let error =
        crate::test_support::work_refusal_at("scan SLDPRT structured plane sources", |ctx| {
            super::super::offset_plane_reference_source(ctx, &payload, &known, &references, None)
        });
    assert!(matches!(error, CodecError::ResourceLimit(limit)
        if limit.operation == "scan SLDPRT structured plane sources"
            && limit.used == 7 && limit.additional == 1 && limit.limit == 7));
}
