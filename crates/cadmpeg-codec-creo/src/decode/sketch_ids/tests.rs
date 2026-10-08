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

fn retained_refusal_at<T>(
    operation: &'static str,
    run: impl Fn(&DecodeContext<'_>) -> Result<T, cadmpeg_core::CodecError>,
) -> Result<T, cadmpeg_core::CodecError> {
    Err(crate::test_support::last_refusal_at(
        &[],
        ResourceDimension::RetainedBytes,
        operation,
        run,
    ))
}

#[test]
fn sketch_entity_identity_refuses_before_retained_formatting() {
    let sketch = cadmpeg_ir::sketches::SketchId::mint("creo:model:sketch#40").expect("sketch ID");
    let expected = "creo:featdefs:sketch_entity#40:7";
    assert!(
        matches!(retained_refusal_at("creo sketch entity identity", |ctx| {
        sketch_entity_id_admitted(ctx, &sketch, 7)
    }), Err(cadmpeg_core::CodecError::ResourceLimit(resource))
        if resource.dimension == ResourceDimension::RetainedBytes
            && resource.operation == "creo sketch entity identity")
    );
    assert_eq!(
        with_retained_limit(
            cadmpeg_core::decode::u64_from_index(expected.len()),
            |ctx| { sketch_entity_id_admitted(ctx, &sketch, 7) }
        )
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
        matches!(retained_refusal_at("creo sketch point reference", |ctx| {
        sketch_point_ref_admitted(ctx, &sketch, 7)
    }), Err(cadmpeg_core::CodecError::ResourceLimit(resource))
        if resource.dimension == ResourceDimension::RetainedBytes
            && resource.operation == "creo sketch point reference")
    );
    assert_eq!(
        with_retained_limit(
            cadmpeg_core::decode::u64_from_index(expected.len()),
            |ctx| { sketch_point_ref_admitted(ctx, &sketch, 7) }
        )
        .expect("exact retained cap admits point ref"),
        expected
    );
}

#[test]
fn section_curve_identity_and_reference_refuse_before_formatting() {
    let sketch = cadmpeg_ir::sketches::SketchId::mint("creo:model:sketch#40").expect("sketch ID");
    let expected = "creo:featdefs:section_curve#40:7";
    for (operation, result) in [
        (
            "creo section curve reference",
            retained_refusal_at("creo section curve reference", |ctx| {
                sketch_section_curve_id_admitted(ctx, &sketch, 7).map(|text| text.len())
            }),
        ),
        (
            "creo section curve identity",
            retained_refusal_at("creo section curve identity", |ctx| {
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
        with_retained_limit(
            cadmpeg_core::decode::u64_from_index(expected.len()),
            |ctx| { typed_sketch_section_curve_id_admitted(ctx, &sketch, 7) }
        )
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
        matches!(retained_refusal_at("creo sketch feature identity", |ctx| {
        sketch_feature_id_admitted(ctx, &sketch)
    }), Err(cadmpeg_core::CodecError::ResourceLimit(resource))
        if resource.dimension == ResourceDimension::RetainedBytes
            && resource.operation == "creo sketch feature identity")
    );
    assert_eq!(
        with_retained_limit(
            cadmpeg_core::decode::u64_from_index(expected.len()),
            |ctx| { sketch_feature_id_admitted(ctx, &sketch) }
        )
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
    for (operation, error) in [
        (
            "creo sketch constraint identity",
            retained_refusal_at("creo sketch constraint identity", |ctx| {
                sketch_constraint_id_admitted(ctx, &sketch, "equation:offset:28")
            })
            .expect_err("constraint ID exceeds retained cap"),
        ),
        (
            "creo sketch native reference",
            retained_refusal_at("creo sketch native reference", |ctx| {
                sketch_native_ref_admitted(ctx, &sketch)
            })
            .expect_err("native ref exceeds retained cap"),
        ),
    ] {
        assert!(
            matches!(error, cadmpeg_core::CodecError::ResourceLimit(resource)
            if resource.dimension == ResourceDimension::RetainedBytes
            && resource.operation == operation)
        );
    }
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
    let scan = crate::test_support::empty_container_scan();
    let sketch = cadmpeg_ir::sketches::SketchId::mint("creo:model:sketch#917").expect("sketch ID");
    let arena = DecodeArena::new();
    let error = retained_refusal_at("creo section owner feature identity", |ctx| {
        section_owner_feature_id(ctx, &scan, 917, &sketch)
    })
    .expect_err("owner feature ID exceeds retained cap");
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(resource)
        if resource.dimension == ResourceDimension::RetainedBytes && resource.operation == "creo section owner feature identity")
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
    let mut scan = crate::test_support::empty_container_scan();
    let mut source = definition();
    source.offset = 9;
    scan.features.definitions.push(source);
    for (dimension, operation) in [
        (
            ResourceDimension::WorkUnits,
            "creo model sketch identity uniqueness",
        ),
        (
            ResourceDimension::RetainedBytes,
            "creo model sketch identity",
        ),
    ] {
        let error = crate::test_support::last_refusal_at(&[], dimension, operation, |ctx| {
            model_sketch_id(ctx, &scan, &scan.features.definitions[0])
        });
        assert!(
            matches!(error, cadmpeg_core::CodecError::ResourceLimit(resource) if resource.dimension == dimension && resource.operation == operation)
        );
    }
    let id = crate::decode::with_test_decode_ctx(|ctx| {
        model_sketch_id(ctx, &scan, &scan.features.definitions[0])
    })
    .expect("service identity admitted")
    .expect("valid sketch identity");
    assert_eq!(id.as_str(), "creo:model:sketch#offset:9");
}

#[test]
fn native_sketch_identity_refuses_before_formatting_and_uniqueness_scan() {
    let mut scan = crate::test_support::empty_container_scan();
    let mut source = definition();
    source.offset = 9;
    scan.features.definitions.push(source);
    for (dimension, operation) in [
        (
            ResourceDimension::WorkUnits,
            "creo native sketch identity uniqueness",
        ),
        (
            ResourceDimension::RetainedBytes,
            "creo native sketch identity",
        ),
    ] {
        let error = crate::test_support::last_refusal_at(&[], dimension, operation, |ctx| {
            feature_sketch_record_id_in_scan(ctx, &scan, &scan.features.definitions[0])
        });
        assert!(
            matches!(error, cadmpeg_core::CodecError::ResourceLimit(resource) if resource.dimension == dimension && resource.operation == operation)
        );
    }
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
    let mut scan = crate::test_support::empty_container_scan();
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

#[test]
fn sketch_design_presence_is_free_and_preserves_original_refusal() {
    let mut definition = definition();
    definition.body = vec![0; 64];
    definition.trim_entities = Some(trim_entities(Vec::new()));
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_work_units = 0;
    policy.limits.max_materialized_bytes = 0;
    policy.limits.max_retained_bytes = 0;
    policy.limits.max_collection_items = 0;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("root");
    assert!(super::feature_definition_has_sketch_design(&ctx, &definition)
        .expect("present table avoids body scan"));
    let original = ctx.charge_work_limit(1, "prior sketch presence refusal")
        .expect_err("seed refusal");
    assert!(matches!(super::feature_definition_has_sketch_design(&ctx, &definition),
        Err(cadmpeg_core::CodecError::ResourceLimit(refusal)) if refusal == original));
    definition.trim_entities = None;
    assert!(matches!(super::feature_definition_has_sketch_design(&ctx, &definition),
        Err(cadmpeg_core::CodecError::ResourceLimit(refusal)) if refusal == original));
}

fn limited_headers(
    definition: &FeatureDefinition,
    operation: &'static str,
) -> cadmpeg_core::CodecError {
    crate::test_support::last_refusal_at(
        &[],
        ResourceDimension::CollectionItems,
        operation,
        |ctx| sketch_table_headers(ctx, definition),
    )
}

#[test]
fn sketch_headers_refuse_before_outer_row() {
    let mut definition = definition();
    definition.trim_entities = Some(trim_entities(Vec::new()));
    let error = limited_headers(&definition, "creo sketch table headers");
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
    let error = limited_headers(&definition, "creo sketch trim entity headers");
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
    let error = limited_headers(&definition, "creo sketch trim vertex headers");
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

#[test]
fn sketch_headers_preserve_offset_and_tie_order() {
    let mut definition = definition();
    definition.variables = Some(crate::feature::definitions::FeatureVariableTable {
        declared_count: 0,
        entity_ref: None,
        rows: Vec::new(),
        offset: 12,
    });
    definition.trim_entities = Some(trim_entities(vec![bucket()]));
    definition.trim_vertices = Some(trim_vertices(vec![bucket()]));
    let headers = crate::decode::with_test_decode_ctx(|ctx| sketch_table_headers(ctx, &definition))
        .expect("header ordering admission");
    assert_eq!(
        headers
            .iter()
            .map(|header| header.offset)
            .collect::<Vec<_>>(),
        vec![11, 12, 12]
    );
    assert!(matches!(
        headers[0].kind,
        CreoSketchTableKind::TrimEntities { .. }
    ));
    assert!(matches!(
        headers[1].kind,
        CreoSketchTableKind::Variables { .. }
    ));
    assert!(matches!(
        headers[2].kind,
        CreoSketchTableKind::TrimVertices { .. }
    ));
    definition.variables.as_mut().expect("variables").offset = 13;
    definition
        .trim_entities
        .as_mut()
        .expect("trim entities")
        .offset = 12;
    definition
        .trim_vertices
        .as_mut()
        .expect("trim vertices")
        .offset = 11;
    let headers = crate::decode::with_test_decode_ctx(|ctx| sketch_table_headers(ctx, &definition))
        .expect("reversed header ordering admission");
    assert_eq!(
        headers
            .iter()
            .map(|header| header.offset)
            .collect::<Vec<_>>(),
        vec![11, 12, 13]
    );
    assert!(matches!(
        headers[0].kind,
        CreoSketchTableKind::TrimVertices { .. }
    ));
    assert!(matches!(
        headers[1].kind,
        CreoSketchTableKind::TrimEntities { .. }
    ));
    assert!(matches!(
        headers[2].kind,
        CreoSketchTableKind::Variables { .. }
    ));
}
