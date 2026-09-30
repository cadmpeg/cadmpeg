// SPDX-License-Identifier: Apache-2.0

use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};

use super::{
    feature_sketch_record_id_in_scan, model_sketch_id, owning_feature_definition_ref,
    section_owner_feature_id, sketch_constraint_id_admitted, sketch_entity_id_admitted,
    sketch_feature_id_admitted, sketch_native_ref_admitted, sketch_point_ref_admitted,
    sketch_section_curve_id_admitted, sketch_table_headers, typed_sketch_section_curve_id_admitted,
};

fn with_retained_limit<T>(limit: u64, run: impl FnOnce(&DecodeContext<'_>) -> T) -> T {
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_retained_bytes = limit;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("empty root");
    run(&ctx)
}

#[test]
fn sketch_entity_identity_refuses_before_retained_formatting() {
    let sketch = cadmpeg_ir::sketches::SketchId::mint("creo:model:sketch#40").expect("sketch ID");
    let expected = "creo:featdefs:sketch_entity#40:7";
    assert!(
        matches!(with_retained_limit(cadmpeg_core::decode::u64_from_index(expected.len()) - 1, |ctx| {
        sketch_entity_id_admitted(ctx, &sketch, 7)
    }), Err(cadmpeg_core::CodecError::ResourceLimit(resource))
        if resource.dimension == ResourceDimension::RetainedBytes
            && resource.operation == "creo sketch entity identity")
    );
    assert_eq!(
        with_retained_limit(cadmpeg_core::decode::u64_from_index(expected.len()), |ctx| {
            sketch_entity_id_admitted(ctx, &sketch, 7)
        })
        .expect("exact cap admits entity ID")
        .expect("valid entity ID")
        .as_str(),
        expected
    );
}

#[test]
fn sketch_point_reference_refuses_before_retained_formatting() {
    let sketch = cadmpeg_ir::sketches::SketchId::mint("creo:model:sketch#40").expect("sketch ID");
    let expected = "creo:featdefs:sketch#40:point#7";
    assert!(
        matches!(with_retained_limit(cadmpeg_core::decode::u64_from_index(expected.len()) - 1, |ctx| {
        sketch_point_ref_admitted(ctx, &sketch, 7)
    }), Err(cadmpeg_core::CodecError::ResourceLimit(resource))
        if resource.dimension == ResourceDimension::RetainedBytes
            && resource.operation == "creo sketch point reference")
    );
    assert_eq!(
        with_retained_limit(cadmpeg_core::decode::u64_from_index(expected.len()), |ctx| {
            sketch_point_ref_admitted(ctx, &sketch, 7)
        })
        .expect("exact retained cap admits point ref"),
        expected
    );
}

#[test]
fn section_curve_identity_and_reference_refuse_before_formatting() {
    let sketch = cadmpeg_ir::sketches::SketchId::mint("creo:model:sketch#40").expect("sketch ID");
    let expected = "creo:featdefs:section_curve#40:7";
    let limit = cadmpeg_core::decode::u64_from_index(expected.len()) - 1;
    for (operation, result) in [
        (
            "creo section curve reference",
            with_retained_limit(limit, |ctx| {
                sketch_section_curve_id_admitted(ctx, &sketch, 7).map(|text| text.len())
            }),
        ),
        (
            "creo section curve identity",
            with_retained_limit(limit, |ctx| {
                typed_sketch_section_curve_id_admitted(ctx, &sketch, 7)
                    .map(|id| id.map(|id| id.as_str().len()).unwrap_or_default())
            }),
        ),
    ] {
        assert!(
            matches!(result, Err(cadmpeg_core::CodecError::ResourceLimit(resource))
            if resource.dimension == ResourceDimension::RetainedBytes
                && resource.operation == operation)
        );
    }
    assert_eq!(
        with_retained_limit(cadmpeg_core::decode::u64_from_index(expected.len()), |ctx| {
            typed_sketch_section_curve_id_admitted(ctx, &sketch, 7)
        })
        .expect("exact cap admits curve ID")
        .expect("valid curve ID")
        .as_str(),
        expected
    );
}

#[test]
fn sketch_feature_identity_refuses_before_formatting() {
    let sketch = cadmpeg_ir::sketches::SketchId::mint("creo:model:sketch#40").expect("sketch ID");
    let expected = "creo:model:sketch_feature#40";
    assert!(
        matches!(with_retained_limit(cadmpeg_core::decode::u64_from_index(expected.len()) - 1, |ctx| {
        sketch_feature_id_admitted(ctx, &sketch)
    }), Err(cadmpeg_core::CodecError::ResourceLimit(resource))
        if resource.dimension == ResourceDimension::RetainedBytes
            && resource.operation == "creo sketch feature identity")
    );
    assert_eq!(
        with_retained_limit(cadmpeg_core::decode::u64_from_index(expected.len()), |ctx| {
            sketch_feature_id_admitted(ctx, &sketch)
        })
        .expect("exact cap admits feature ID")
        .expect("valid feature ID")
        .as_str(),
        expected
    );
}
use crate::decode::native_records::CreoSketchTableKind;
use crate::feature::definitions::{
    DefinitionIdentity, FeatureDefinition, FeatureTrimBucket, FeatureTrimEntityTable,
    FeatureTrimVertexTable,
};

fn definition() -> FeatureDefinition {
    FeatureDefinition {
        identity: DefinitionIdentity::Parsed {
            schema_id: None,
            owner_feature_id: None,
        },
        body: Vec::new(),
        parameter_frames: Vec::new(),
        outlines: Vec::new(),
        variables: None,
        segments: None,
        trim_entities: None,
        trim_vertices: None,
        order_table: None,
        section_3d: None,
        dimensions: None,
        relations: None,
        saved_section: None,
        offset: 0,
    }
}

#[test]
fn sketch_constraint_identity_and_native_ref_refuse_retained_bytes() {
    let sketch = cadmpeg_ir::sketches::SketchId::mint("creo:model:sketch#40").expect("sketch ID");
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_retained_bytes =
        cadmpeg_core::decode::u64_from_index("creo:featdefs:sketch_constraint#40:equation:offset:28".len()) - 1;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("empty root");
    let error = sketch_constraint_id_admitted(&ctx, &sketch, "equation:offset:28")
        .expect_err("constraint ID exceeds retained cap");
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(resource)
        if resource.dimension == ResourceDimension::RetainedBytes
            && resource.operation == "creo sketch constraint identity")
    );
    policy.limits.max_retained_bytes = cadmpeg_core::decode::u64_from_index("creo:featdefs:sketch#40".len()) - 1;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("empty root");
    let error =
        sketch_native_ref_admitted(&ctx, &sketch).expect_err("native ref exceeds retained cap");
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(resource)
        if resource.dimension == ResourceDimension::RetainedBytes
            && resource.operation == "creo sketch native reference")
    );
    let service = DecodePolicy::service();
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &service).expect("empty root");
    assert_eq!(
        sketch_constraint_id_admitted(&ctx, &sketch, "equation:offset:28")
            .expect("service constraint ID")
            .expect("valid constraint ID")
            .as_str(),
        "creo:featdefs:sketch_constraint#40:equation:offset:28"
    );
    assert_eq!(
        sketch_native_ref_admitted(&ctx, &sketch).expect("service native ref"),
        "creo:featdefs:sketch#40"
    );
}

#[test]
fn section_owner_feature_identity_refuses_before_formatting() {
    let scan = crate::container::scan_bytes_ok(Vec::new());
    let sketch = cadmpeg_ir::sketches::SketchId::mint("creo:model:sketch#917").expect("sketch ID");
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_retained_bytes = cadmpeg_core::decode::u64_from_index("creo:model:sketch_feature#917".len()) - 1;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("empty root");
    let error = section_owner_feature_id(&ctx, &scan, 917, &sketch)
        .expect_err("owner feature ID exceeds retained cap");
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(resource)
        if resource.dimension == ResourceDimension::RetainedBytes
            && resource.operation == "creo section owner feature identity")
    );
    let service = DecodePolicy::service();
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &service).expect("empty root");
    assert_eq!(
        section_owner_feature_id(&ctx, &scan, 917, &sketch)
            .expect("service owner ID")
            .expect("valid owner ID")
            .as_str(),
        "creo:model:sketch_feature#917"
    );
}

#[test]
fn model_sketch_identity_refuses_before_formatting_and_uniqueness_scan() {
    let mut scan = crate::container::scan_bytes_ok(Vec::new());
    let mut source = definition();
    source.offset = 9;
    scan.features.definitions.push(source);
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_work_units = 0;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("empty root");
    let error = model_sketch_id(&ctx, &scan, &scan.features.definitions[0])
        .expect_err("one definition needs one scan unit");
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(resource)
        if resource.dimension == ResourceDimension::WorkUnits
            && resource.operation == "creo model sketch identity uniqueness")
    );
    policy.limits.max_work_units = DecodePolicy::service().limits.max_work_units;
    policy.limits.max_retained_bytes = cadmpeg_core::decode::u64_from_index("creo:model:sketch#offset:9".len()) - 1;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("empty root");
    let error = model_sketch_id(&ctx, &scan, &scan.features.definitions[0])
        .expect_err("identity text exceeds retained cap");
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(resource)
        if resource.dimension == ResourceDimension::RetainedBytes
            && resource.operation == "creo model sketch identity")
    );
    let id = crate::decode::with_test_decode_ctx(|ctx| {
        model_sketch_id(ctx, &scan, &scan.features.definitions[0])
    })
    .expect("service identity admitted")
    .expect("valid sketch identity");
    assert_eq!(id.as_str(), "creo:model:sketch#offset:9");
}

#[test]
fn native_sketch_identity_refuses_before_formatting_and_uniqueness_scan() {
    let mut scan = crate::container::scan_bytes_ok(Vec::new());
    let mut source = definition();
    source.offset = 9;
    scan.features.definitions.push(source);
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_work_units = 0;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("empty root");
    let error = feature_sketch_record_id_in_scan(&ctx, &scan, &scan.features.definitions[0])
        .expect_err("one definition needs one scan unit");
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(resource)
        if resource.dimension == ResourceDimension::WorkUnits
            && resource.operation == "creo native sketch identity uniqueness")
    );
    policy.limits.max_work_units = DecodePolicy::service().limits.max_work_units;
    policy.limits.max_retained_bytes = cadmpeg_core::decode::u64_from_index("creo:featdefs:sketch#offset:9".len()) - 1;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("empty root");
    let error = feature_sketch_record_id_in_scan(&ctx, &scan, &scan.features.definitions[0])
        .expect_err("native identity exceeds retained cap");
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(resource)
        if resource.dimension == ResourceDimension::RetainedBytes
            && resource.operation == "creo native sketch identity")
    );
    assert_eq!(
        crate::decode::with_test_decode_ctx(|ctx| {
            feature_sketch_record_id_in_scan(ctx, &scan, &scan.features.definitions[0])
        })
        .expect("service native identity"),
        "creo:featdefs:sketch#offset:9"
    );
}

#[test]
fn owning_definition_lookup_keeps_unique_owner_rule() {
    let mut scan = crate::container::scan_bytes_ok(Vec::new());
    let mut owned = definition();
    owned.identity = DefinitionIdentity::Parsed {
        schema_id: std::num::NonZeroU32::new(17),
        owner_feature_id: Some(40),
    };
    scan.features.definitions.push(owned.clone());
    assert_eq!(
        crate::decode::with_test_decode_ctx(|ctx| owning_feature_definition_ref(ctx, &scan, 40))
            .expect("ID admitted")
            .as_deref(),
        Some("creo:featdefs:feature_definition#17")
    );
    scan.features.definitions.push(owned);
    assert_eq!(
        crate::decode::with_test_decode_ctx(|ctx| owning_feature_definition_ref(ctx, &scan, 40))
            .expect("lookup admitted"),
        None
    );
}

fn bucket() -> FeatureTrimBucket {
    FeatureTrimBucket {
        index: 0,
        declared_entry_count: 1,
        decoded_entry_count: Some(1),
        offset: 17,
    }
}

fn trim_entities(buckets: Vec<FeatureTrimBucket>) -> FeatureTrimEntityTable {
    FeatureTrimEntityTable {
        declared_count: Some(1),
        entity_ref: None,
        entry_ref: None,
        buckets,
        rows: Vec::new(),
        solved_external_ids: Vec::new(),
        offset: 11,
    }
}

fn trim_vertices(buckets: Vec<FeatureTrimBucket>) -> FeatureTrimVertexTable {
    FeatureTrimVertexTable {
        declared_count: Some(1),
        entity_ref: None,
        entry_ref: None,
        buckets,
        rows: Vec::new(),
        offset: 12,
    }
}

fn limited_headers(
    definition: &FeatureDefinition,
    max_collection_items: u64,
) -> cadmpeg_core::CodecError {
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_collection_items = max_collection_items;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("empty root");
    match sketch_table_headers(&ctx, definition) {
        Err(error) => error,
        Ok(_) => panic!("headers exceed the collection limit"),
    }
}

#[test]
fn sketch_headers_refuse_before_outer_row() {
    let mut definition = definition();
    definition.trim_entities = Some(trim_entities(Vec::new()));
    let error = limited_headers(&definition, 0);
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(resource)
        if resource.dimension == ResourceDimension::CollectionItems
            && resource.operation == "creo sketch table headers")
    );
}

#[test]
fn sketch_headers_refuse_before_trim_entity_bucket_rows() {
    let mut definition = definition();
    definition.trim_entities = Some(trim_entities(vec![bucket()]));
    let error = limited_headers(&definition, 0);
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(resource)
        if resource.dimension == ResourceDimension::CollectionItems
            && resource.operation == "creo sketch trim entity headers")
    );
}

#[test]
fn sketch_headers_refuse_before_trim_vertex_bucket_rows() {
    let mut definition = definition();
    definition.trim_vertices = Some(trim_vertices(vec![bucket()]));
    let error = limited_headers(&definition, 0);
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(resource)
        if resource.dimension == ResourceDimension::CollectionItems
            && resource.operation == "creo sketch trim vertex headers")
    );
}

#[test]
fn sketch_headers_keep_source_offset_order_and_bucket_values() {
    let mut definition = definition();
    definition.trim_entities = Some(trim_entities(vec![bucket()]));
    definition.trim_vertices = Some(trim_vertices(vec![bucket()]));
    let headers = crate::decode::with_test_decode_ctx(|ctx| sketch_table_headers(ctx, &definition))
        .expect("two headers fit the service limits");
    assert_eq!(headers.len(), 2);
    assert_eq!((headers[0].offset, headers[1].offset), (11, 12));
    for header in headers {
        let (CreoSketchTableKind::TrimEntities { buckets, .. }
        | CreoSketchTableKind::TrimVertices { buckets, .. }) = header.kind
        else {
            panic!("expected trim table header")
        };
        assert_eq!(buckets.len(), 1);
        assert_eq!(buckets[0].index, 0);
        assert_eq!(buckets[0].declared_entry_count, 1);
        assert_eq!(buckets[0].decoded_entry_count, Some(1));
        assert_eq!(buckets[0].offset, 17);
    }
}
