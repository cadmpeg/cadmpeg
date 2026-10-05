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
fn reference_plane_enrichment_refuses_retained_limit() {
    let error = cadmpeg_test_support::refusal::resource_limit_at(
        cadmpeg_core::decode::ResourceDimension::RetainedBytes,
        "retain SLDPRT plane frame source",
        |cap| {
            let mut policy = DecodePolicy::service();
            policy.limits.max_retained_bytes = cap;
            Err::<(), cadmpeg_core::CodecError>(reference_plane_error(policy))
        },
    );
    assert!(matches!(error, CodecError::ResourceLimit(limit)
        if limit.dimension == ResourceDimension::RetainedBytes
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
