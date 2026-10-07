// SPDX-License-Identifier: Apache-2.0
#![allow(clippy::unwrap_used)]

use std::io::Cursor;

use cadmpeg_core::decode::{DecodePolicy, ResourceDimension};
use cadmpeg_ir::codec::{Codec, DecodeFailure, DecodeOptions};
use crate::test_support::test_owned::{owned_test_file, OwnedTestEntity};
use crate::IgesCodec;

fn assert_boundary(bytes: &[u8], dimension: ResourceDimension, operation: &str) {
    if dimension == ResourceDimension::MaterializedBytes {
        assert_native_storage_boundary(bytes, operation);
        return;
    }
    let error = cadmpeg_test_support::refusal::resource_limit_at(dimension, operation, |cap| {
        let mut policy = DecodePolicy::service();
        match dimension {
            ResourceDimension::WorkUnits => policy.limits.max_work_units = cap,
            ResourceDimension::CollectionItems => policy.limits.max_collection_items = cap,
            ResourceDimension::MaterializedBytes => policy.limits.max_materialized_bytes = cap,
            _ => panic!("test dimension"),
        }
        IgesCodec.decode(&mut Cursor::new(bytes), &DecodeOptions { policy, ..DecodeOptions::default() })
            .map_err(|failure| match failure {
                DecodeFailure::Codec(error) => error,
                other => panic!("unexpected decode failure: {other:?}"),
            })
    });
    assert!(matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
        if limit.dimension == dimension && limit.operation == operation));
}

// Parse and project with a separate bounded budget. Only native scratch storage
// participates in the searched peak, so parser peaks cannot hide this boundary.
fn assert_native_storage_boundary(bytes: &[u8], operation: &str) {
    use cadmpeg_core::decode::{DecodeArena, DecodeContext};
    use super::super::{NativeStoreInputs, ProductOccurrenceLimits, QuarantinedRecords};
    let scan = crate::test_support::scan(bytes).unwrap();
    let arena = DecodeArena::new();
    let (parse_ctx, _) = DecodeContext::from_root_bytes(bytes, &arena, &DecodePolicy::service()).unwrap();
    let (global, _) = crate::global::parse(&scan, &parse_ctx).unwrap();
    let (directory, quarantined_directory) = crate::directory::parse(&scan, global.global_table(&parse_ctx).unwrap(), &parse_ctx).unwrap();
    let assembly = crate::parameter::assemble_with_context(&scan, &directory, &quarantined_directory, &global, &parse_ctx).unwrap();
    let references = crate::graph::build(&directory, &parse_ctx).unwrap();
    let mut ir = cadmpeg_ir::CadIr::empty();
    let projection = global.length_context(&parse_ctx).unwrap().map(|length| {
        crate::entities::geometry::project_geometry(&mut ir, &directory, &assembly.records,
            &assembly.trailing_pointer_analysis, &length, &parse_ctx).unwrap()
    }).unwrap_or_default();
    let error = cadmpeg_test_support::refusal::resource_limit_at(ResourceDimension::MaterializedBytes, operation, |cap| {
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        policy.limits.max_materialized_bytes = cap;
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
        super::super::store(&mut ir.clone(), NativeStoreInputs {
            scan: &scan, directory: &directory, parameters: &assembly.records,
            trailing_pointer_analysis: &assembly.trailing_pointer_analysis,
            quarantine: QuarantinedRecords { directory: &quarantined_directory, parameters: &assembly.quarantined },
            structure_admitted: Some(&projection), sequences: &projection.sequences,
            boundary_vertex_derivations: &projection.boundary_vertex_derivations,
        }, &mut references.clone(), &global, ProductOccurrenceLimits::new(100_000, 64), &ctx).map(|_| ())
    });
    assert!(matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
        if limit.dimension == ResourceDimension::MaterializedBytes && limit.operation == operation));
}

#[test]
fn native_property_owner_index_refuses_work_slots_and_scoped_storage() {
    let bytes = owned_test_file(&[
        OwnedTestEntity { entity_type: 116, form: 0, label: "OWNER".into(), status: "00000000", parameters: "116,1,2,3,0,0,2,3,3;".into() },
        OwnedTestEntity { entity_type: 406, form: 15, label: "NAME".into(), status: "00000200", parameters: "406,1,1HX;".into() },
    ]);
    for (dimension, operation) in [
        (ResourceDimension::WorkUnits, "iges native property owner index scan"),
        (ResourceDimension::CollectionItems, "iges native property owner index nodes"),
        (ResourceDimension::CollectionItems, "iges native property owner index members"),
        (ResourceDimension::MaterializedBytes, "iges native property owner index nodes"),
    ] { assert_boundary(&bytes, dimension, operation); }
    let result = IgesCodec.decode(&mut Cursor::new(bytes), &DecodeOptions::default()).unwrap();
    let native = result.ir().native.namespace("iges").unwrap();
    for arena in ["product_properties", "properties"] {
        assert_eq!(native.arenas()[arena][0].fields()["owners"], serde_json::json!(["iges:entity:directory#1"]));
    }
}

#[test]
fn native_property_owners_preserve_directory_order_for_a_large_batch() {
    const OWNER_COUNT: usize = 1_200;
    let mut entities = Vec::new();
    for index in 0..OWNER_COUNT {
        entities.push(OwnedTestEntity { entity_type: 116, form: 0, label: "OWNER".into(), status: "00000000", parameters: format!("116,1,2,3,0,0,1,{};", 4 * index + 3) });
        entities.push(OwnedTestEntity { entity_type: 406, form: 15, label: "NAME".into(), status: "00000200", parameters: "406,1,1HX;".into() });
    }
    let result = IgesCodec.decode(&mut Cursor::new(owned_test_file(&entities)), &DecodeOptions::default()).unwrap();
    let native = result.ir().native.namespace("iges").unwrap();
    for arena in ["product_properties", "properties"] {
        assert_eq!(native.arenas()[arena].len(), OWNER_COUNT);
        for record in &native.arenas()[arena] {
            let sequence = record.fields()["source_entity"].as_str().unwrap()
                .strip_prefix("iges:entity:directory#").unwrap().parse::<u32>().unwrap();
            assert_eq!(record.fields()["owners"], serde_json::json!([format!("iges:entity:directory#{}", sequence - 2)]));
        }
    }
}
