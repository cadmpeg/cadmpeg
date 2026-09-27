// SPDX-License-Identifier: Apache-2.0

use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};

use super::{owning_feature_definition_ref, sketch_table_headers};
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
fn owning_definition_lookup_keeps_unique_owner_rule() {
    let mut scan = crate::container::scan_bytes_ok(Vec::new());
    let mut owned = definition();
    owned.identity = DefinitionIdentity::Parsed {
        schema_id: std::num::NonZeroU32::new(17),
        owner_feature_id: Some(40),
    };
    scan.features.definitions.push(owned.clone());
    assert_eq!(owning_feature_definition_ref(&scan, 40).as_deref(),
        Some("creo:featdefs:feature_definition#17"));
    scan.features.definitions.push(owned);
    assert_eq!(owning_feature_definition_ref(&scan, 40), None);
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
    assert!(matches!(error, cadmpeg_core::CodecError::ResourceLimit(resource)
        if resource.dimension == ResourceDimension::CollectionItems
            && resource.operation == "creo sketch table headers"));
}

#[test]
fn sketch_headers_refuse_before_trim_entity_bucket_rows() {
    let mut definition = definition();
    definition.trim_entities = Some(trim_entities(vec![bucket()]));
    let error = limited_headers(&definition, 0);
    assert!(matches!(error, cadmpeg_core::CodecError::ResourceLimit(resource)
        if resource.dimension == ResourceDimension::CollectionItems
            && resource.operation == "creo sketch trim entity headers"));
}

#[test]
fn sketch_headers_refuse_before_trim_vertex_bucket_rows() {
    let mut definition = definition();
    definition.trim_vertices = Some(trim_vertices(vec![bucket()]));
    let error = limited_headers(&definition, 0);
    assert!(matches!(error, cadmpeg_core::CodecError::ResourceLimit(resource)
        if resource.dimension == ResourceDimension::CollectionItems
            && resource.operation == "creo sketch trim vertex headers"));
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
        let buckets = match header.kind {
            CreoSketchTableKind::TrimEntities { buckets, .. }
            | CreoSketchTableKind::TrimVertices { buckets, .. } => buckets,
            _ => panic!("expected trim table header"),
        };
        assert_eq!(buckets.len(), 1);
        assert_eq!(buckets[0].index, 0);
        assert_eq!(buckets[0].declared_entry_count, 1);
        assert_eq!(buckets[0].decoded_entry_count, Some(1));
        assert_eq!(buckets[0].offset, 17);
    }
}
