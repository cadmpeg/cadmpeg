use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
use cadmpeg_core::CodecError;

fn sketch_block_error(policy: DecodePolicy) -> CodecError {
    let feature = |id: &str, source: u32, class: &str| super::Feature {
        id: id.into(),
        parent: "history".into(),
        xml_tag: "Feature".into(),
        tree_parent: None,
        source_id: super::FeatureSource::from_value(source),
        ordinal: source,
        name: id.into(),
        kind: String::new(),
        input_class: Some(class.into()),
        suppressed: false,
        parameters: Default::default(),
        dimension_properties: Default::default(),
        properties: Default::default(),
        text: None,
        content: Vec::new(),
    };
    let mut histories = [super::FeatureHistory {
        id: "history".into(),
        part_name: None,
        properties: Default::default(),
        content: Vec::new(),
        configurations: Vec::new(),
        features: vec![
            feature("instance", 25, "moSketchBlockInst_c"),
            feature("definition", 23, "moSketchBlockDef_c"),
        ],
    }];
    let name = |id: &str, source: u32, offset: u64| super::FeatureInputName {
        id: id.into(),
        parent: "lane".into(),
        ordinal: offset as u32,
        offset,
        object_id: super::ObjectId::from_value(source),
        value: id.into(),
    };
    let lane = super::FeatureInputLane {
        id: "lane".into(),
        configuration: None,
        native_payload: vec![0; 4],
        classes: Vec::new(),
        names: vec![name("instance-name", 25, 0), name("definition-name", 23, 1)],
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
    super::super::enrich_history_sketch_block_references(&ctx, &mut histories, &lanes).unwrap_err()
}

#[test]
fn sketch_block_enrichment_refuses_collection_limit() {
    let mut policy = DecodePolicy::service();
    policy.limits.max_collection_items = 0;
    let error = sketch_block_error(policy);
    assert!(matches!(error, CodecError::ResourceLimit(limit)
        if limit.dimension == ResourceDimension::CollectionItems
            && limit.operation == "index SLDPRT sketch block sources"));
}

#[test]
fn sketch_block_enrichment_refuses_retained_limit() {
    let mut policy = DecodePolicy::service();
    policy.limits.max_retained_bytes = 0;
    let error = sketch_block_error(policy);
    assert!(matches!(error, CodecError::ResourceLimit(limit)
        if limit.dimension == ResourceDimension::RetainedBytes
            && limit.operation == "retain SLDPRT sketch block definition"));
}

#[test]
fn sketch_block_enrichment_refuses_work_limit() {
    let mut policy = DecodePolicy::service();
    policy.limits.max_work_units = 0;
    let error = sketch_block_error(policy);
    assert!(matches!(error, CodecError::ResourceLimit(limit)
        if limit.dimension == ResourceDimension::WorkUnits
            && limit.operation == "scan SLDPRT sketch block features"));
}
