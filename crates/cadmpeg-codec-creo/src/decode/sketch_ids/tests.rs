// SPDX-License-Identifier: Apache-2.0

use super::{
    feature_sketch_record_id_in_scan, model_sketch_id, owning_feature_definition_ref,
    section_owner_feature_id, sketch_constraint_id, sketch_native_ref, sketch_table_headers,
};

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
fn sketch_constraint_identity_and_native_ref_name_the_sketch() {
    let sketch = cadmpeg_ir::sketches::SketchId::mint("creo:model:sketch#40").expect("sketch ID");
    assert_eq!(
        sketch_constraint_id(&sketch, "equation:offset:28")
            .expect("build the sketch constraint identity")
            .as_str(),
        "creo:featdefs:sketch_constraint#40:equation:offset:28"
    );
    assert_eq!(sketch_native_ref(&sketch), "creo:featdefs:sketch#40");
}

#[test]
fn section_owner_feature_identity_names_the_feature() {
    let scan = crate::test_support::empty_container_scan();
    let sketch = cadmpeg_ir::sketches::SketchId::mint("creo:model:sketch#917").expect("sketch ID");
    assert_eq!(
        section_owner_feature_id(&scan, 917, &sketch)
            .expect("build the section owner feature identity")
            .as_str(),
        "creo:model:sketch_feature#917"
    );
}

#[test]
fn model_sketch_identity_uses_the_unique_definition_offset() {
    let mut scan = crate::test_support::empty_container_scan();
    let mut source = definition();
    source.offset = 9;
    scan.features.definitions.push(source);
    let id = { model_sketch_id(&scan, &scan.features.definitions[0]) }
        .expect("build the model sketch identity");
    assert_eq!(id.as_str(), "creo:model:sketch#offset:9");
}

#[test]
fn native_sketch_identity_uses_the_unique_definition_offset() {
    let mut scan = crate::test_support::empty_container_scan();
    let mut source = definition();
    source.offset = 9;
    scan.features.definitions.push(source);
    assert_eq!(
        feature_sketch_record_id_in_scan(&scan, &scan.features.definitions[0]),
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
        owning_feature_definition_ref(&scan, 40).as_deref(),
        Some("creo:featdefs:feature_definition#17")
    );
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

#[test]
fn sketch_headers_keep_source_offset_order_and_bucket_values() {
    let mut definition = definition();
    definition.trim_entities = Some(trim_entities(vec![bucket()]));
    definition.trim_vertices = Some(trim_vertices(vec![bucket()]));
    let headers = sketch_table_headers(&definition).expect("two headers fit the service limits");
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
