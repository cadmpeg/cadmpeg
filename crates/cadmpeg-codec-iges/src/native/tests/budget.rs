// SPDX-License-Identifier: Apache-2.0
#![allow(clippy::unwrap_used)]

use std::io::Cursor;

use crate::test_support::test_owned::{
    owned_test_file, owned_test_file_with_structures, OwnedTestEntity,
};
use crate::IgesCodec;
use cadmpeg_core::decode::{DecodePolicy, ResourceDimension};
use cadmpeg_ir::codec::{Codec, DecodeFailure, DecodeOptions};

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
        IgesCodec
            .decode(
                &mut Cursor::new(bytes),
                &DecodeOptions {
                    policy,
                    ..DecodeOptions::default()
                },
            )
            .map_err(|failure| match failure {
                DecodeFailure::Codec(error) => error,
                other => panic!("unexpected decode failure: {other:?}"),
            })
    });
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
        if limit.dimension == dimension && limit.operation == operation)
    );
}

// Parse and project with a separate bounded budget. Only native scratch storage
// participates in the searched peak, so parser peaks cannot hide this boundary.
fn assert_native_storage_boundary(bytes: &[u8], operation: &str) {
    use super::super::{NativeStoreInputs, ProductOccurrenceLimits, QuarantinedRecords};
    use cadmpeg_core::decode::{DecodeArena, DecodeContext};
    let scan = crate::test_support::scan(bytes).unwrap();
    let arena = DecodeArena::new();
    let (parse_ctx, _) =
        DecodeContext::from_root_bytes(bytes, &arena, &DecodePolicy::service()).unwrap();
    let (global, _, _global_storage) = crate::global::parse(&scan, &parse_ctx).unwrap();
    let (directory, quarantined_directory) =
        crate::directory::parse(&scan, global.global_table(), &parse_ctx)
            .unwrap();
    let assembly = crate::parameter::assemble_with_context(
        &scan,
        &directory,
        &quarantined_directory,
        &global,
        &parse_ctx,
    )
    .unwrap();
    let (references, _reference_storage) = crate::graph::build(&directory, &parse_ctx).unwrap();
    let mut ir = cadmpeg_ir::CadIr::empty();
    let projection = global
        .length_context()
        .map(|length| {
            crate::entities::geometry::project_geometry(
                &mut ir,
                &directory,
                &assembly.records,
                &assembly.trailing_pointer_analysis,
                &length,
                &parse_ctx,
            )
            .unwrap()
        })
        .unwrap_or_default();
    let error = cadmpeg_test_support::refusal::resource_limit_at(
        ResourceDimension::MaterializedBytes,
        operation,
        |cap| {
            let arena = DecodeArena::new();
            let mut policy = DecodePolicy::service();
            policy.limits.max_materialized_bytes = cap;
            let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
            super::super::store(
                &mut ir.clone(),
                NativeStoreInputs {
                    scan: &scan,
                    directory: &directory,
                    parameters: &assembly.records,
                    trailing_pointer_analysis: &assembly.trailing_pointer_analysis,
                    quarantine: QuarantinedRecords {
                        directory: &quarantined_directory,
                        parameters: &assembly.quarantined,
                    },
                    structure_admitted: Some(&projection),
                    sequences: &projection.sequences,
                    boundary_vertex_derivations: &projection.boundary_vertex_derivations,
                },
                &mut references.clone(),
                &global,
                ProductOccurrenceLimits::new(100_000, 64),
                &ctx,
            )
            .map(|_| ())
        },
    );
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
        if limit.dimension == ResourceDimension::MaterializedBytes && limit.operation == operation)
    );
}

#[test]
fn native_property_owner_index_refuses_work_slots_and_scoped_storage() {
    let bytes = owned_test_file(&[
        OwnedTestEntity {
            entity_type: 116,
            form: 0,
            label: "OWNER".into(),
            status: "00000000",
            parameters: "116,1,2,3,0,0,2,3,3;".into(),
        },
        OwnedTestEntity {
            entity_type: 406,
            form: 15,
            label: "NAME".into(),
            status: "00000200",
            parameters: "406,1,1HX;".into(),
        },
    ]);
    for (dimension, operation) in [
        (
            ResourceDimension::WorkUnits,
            "iges native property owner index scan",
        ),
        (
            ResourceDimension::CollectionItems,
            "iges native property owner index nodes",
        ),
        (
            ResourceDimension::CollectionItems,
            "iges native property owner index members",
        ),
        (
            ResourceDimension::MaterializedBytes,
            "iges native property owner index nodes",
        ),
    ] {
        assert_boundary(&bytes, dimension, operation);
    }
    let result = IgesCodec
        .decode(&mut Cursor::new(bytes), &DecodeOptions::default())
        .unwrap();
    let native = result.ir().native.namespace("iges").unwrap();
    for arena in ["product_properties", "properties"] {
        assert_eq!(
            native.arenas()[arena][0].fields()["owners"],
            serde_json::json!(["iges:entity:directory#1"])
        );
    }
}

#[test]
fn native_property_owners_preserve_directory_order_for_a_large_batch() {
    const OWNER_COUNT: usize = 1_200;
    let mut entities = Vec::new();
    for index in 0..OWNER_COUNT {
        entities.push(OwnedTestEntity {
            entity_type: 116,
            form: 0,
            label: "OWNER".into(),
            status: "00000000",
            parameters: format!("116,1,2,3,0,0,1,{};", 4 * index + 3),
        });
        entities.push(OwnedTestEntity {
            entity_type: 406,
            form: 15,
            label: "NAME".into(),
            status: "00000200",
            parameters: "406,1,1HX;".into(),
        });
    }
    let result = IgesCodec
        .decode(
            &mut Cursor::new(owned_test_file(&entities)),
            &DecodeOptions::default(),
        )
        .unwrap();
    let native = result.ir().native.namespace("iges").unwrap();
    for arena in ["product_properties", "properties"] {
        assert_eq!(native.arenas()[arena].len(), OWNER_COUNT);
        for record in &native.arenas()[arena] {
            let sequence = record.fields()["source_entity"]
                .as_str()
                .unwrap()
                .strip_prefix("iges:entity:directory#")
                .unwrap()
                .parse::<u32>()
                .unwrap();
            assert_eq!(
                record.fields()["owners"],
                serde_json::json!([format!("iges:entity:directory#{}", sequence - 2)])
            );
        }
    }
}

fn entity(entity_type: i64, form: i64, parameters: &str) -> OwnedTestEntity {
    OwnedTestEntity {
        entity_type,
        form,
        label: "NATIVE".into(),
        status: "00000200",
        parameters: parameters.into(),
    }
}

#[test]
fn native_variable_layouts_refuse_work_and_scoped_storage() {
    for (entity_type, form, parameters, scan, storage) in [
        (
            310,
            0,
            "310,1,1HA,0,1,1,65,0,0,1,0,1,2;",
            "iges native glyph layout scan",
            "iges native glyph layouts",
        ),
        (
            302,
            5001,
            "302,2,1,1,2,1,2,2,2,1,3;",
            "iges native class layout scan",
            "iges native class layouts",
        ),
        (
            322,
            1,
            "322,4HATTR,0,1,1,1,1,3HVAL;",
            "iges native attribute layout scan",
            "iges native attribute layouts",
        ),
        (
            406,
            11,
            "406,7,0,1,1,1,1,2,3;",
            "iges native tabular layout scan",
            "iges native tabular layouts",
        ),
        (
            148,
            0,
            "148,0,0,0,3,0,1,1,0,1,1,0,1,0,3,4,5,6;",
            "iges FEM element result layout scan",
            "iges FEM element result layouts",
        ),
    ] {
        let bytes = owned_test_file(&[entity(entity_type, form, parameters)]);
        assert_boundary(&bytes, ResourceDimension::WorkUnits, scan);
        assert_boundary(&bytes, ResourceDimension::MaterializedBytes, storage);
    }
}

#[test]
fn malformed_attribute_definition_keeps_resolved_prefix_references() {
    let bytes = owned_test_file(&[
        entity(322, 2, "322,4HPAIR,0,2,1,1,1,10,3,2,1,99,20;"),
        entity(312, 0, "312,1,1,1,0,0,0,0,0,0,0;"),
    ]);
    let result = IgesCodec
        .decode(&mut Cursor::new(bytes), &DecodeOptions::default())
        .unwrap();
    let native = result.ir().native.namespace("iges").unwrap();
    assert_eq!(
        native.arenas()["attribute_table_definitions"][0].fields()["attributes"],
        serde_json::json!([])
    );
    assert_eq!(
        native.arenas()["entities"][0].fields()["links"],
        serde_json::json!(["iges:entity:directory#3"])
    );
}

#[test]
fn native_attribute_width_cache_refuses_work_nodes_and_scoped_storage() {
    let bytes = owned_test_file_with_structures(
        &[
            entity(322, 0, "322,4HMETA,1,1,10,1,1;"),
            entity(422, 1, "422,1,4HITEM;"),
        ],
        &[(3, -1)],
    );
    for (dimension, operation) in [
        (
            ResourceDimension::WorkUnits,
            "iges native attribute definition width scan",
        ),
        (
            ResourceDimension::CollectionItems,
            "iges native attribute definition width nodes",
        ),
        (
            ResourceDimension::MaterializedBytes,
            "iges native attribute definition width nodes",
        ),
    ] {
        assert_boundary(&bytes, dimension, operation);
    }
}

#[test]
fn native_attribute_instances_share_a_large_zero_width_definition() {
    const ATTRIBUTE_COUNT: usize = 1_200;
    const INSTANCE_COUNT: usize = 1_200;
    let mut parameters = format!("322,4HMETA,1,{ATTRIBUTE_COUNT}");
    for _ in 0..ATTRIBUTE_COUNT {
        parameters.push_str(",10,1,0");
    }
    parameters.push(';');
    let mut entities = vec![entity(322, 0, &parameters)];
    let mut structures = Vec::new();
    for index in 0..INSTANCE_COUNT {
        entities.push(entity(422, 1, "422,1;"));
        structures.push((u32::try_from(2 * index + 3).unwrap(), -1));
    }
    let result = IgesCodec
        .decode(
            &mut Cursor::new(owned_test_file_with_structures(&entities, &structures)),
            &DecodeOptions::default(),
        )
        .unwrap();
    let native = result.ir().native.namespace("iges").unwrap();
    assert_eq!(
        native.arenas()["attribute_table_definitions"][0].fields()["attributes"]
            .as_array()
            .unwrap()
            .len(),
        ATTRIBUTE_COUNT
    );
    let instances = &native.arenas()["attribute_table_instances"];
    assert_eq!(instances.len(), INSTANCE_COUNT);
    for instance in instances {
        assert_eq!(
            instance.fields()["definition"],
            "iges:product:attribute-definition#D1"
        );
        assert_eq!(instance.fields()["rows"], serde_json::json!([]));
    }
}

#[test]
fn native_occurrence_traversal_refuses_work_and_scoped_storage() {
    let bytes = crate::test_support::test_drawing_and_trimming::nested_subfigure_file();
    for operation in [
        "iges occurrence root scan",
        "iges occurrence member scan",
        "iges occurrence cycle search",
        "iges native occurrence id path scan",
    ] {
        assert_boundary(&bytes, ResourceDimension::WorkUnits, operation);
    }
    for operation in [
        "iges occurrence definition map",
        "iges contained occurrence instances",
        "iges occurrence neutral link map nodes",
    ] {
        assert_boundary(&bytes, ResourceDimension::MaterializedBytes, operation);
    }
}

#[test]
fn native_occurrence_id_and_role_follow_the_same_path() {
    use super::super::NativeProductOccurrence;
    for (path, member, id, root, target) in [
        (
            &[1][..],
            None,
            "iges:product:occurrence#1",
            true,
            serde_json::Value::Null,
        ),
        (
            &[1, 3][..],
            None,
            "iges:product:occurrence#1/3",
            false,
            serde_json::Value::Null,
        ),
        (
            &[1, 3][..],
            Some(5),
            "iges:product:occurrence#1/3/D5",
            false,
            serde_json::json!("iges:entity:directory#5"),
        ),
    ] {
        crate::test_support::with_service_context(&[], |ctx| {
            let occurrence = NativeProductOccurrence::new(
                ctx,
                path,
                member,
                3,
                7,
                Vec::new(),
                ([[0.0; 4]; 3], [[0.0; 4]; 3]),
            )
            .unwrap();
            let wire = serde_json::to_value(occurrence).unwrap();
            assert_eq!(wire["id"], id);
            assert_eq!(wire["root"], root);
            assert_eq!(wire["member"], target);
        });
    }
}
