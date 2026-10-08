// SPDX-License-Identifier: Apache-2.0

use super::*;
use crate::test_support::test_dump as support;
use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};

#[test]
fn native_product_staging_is_materialized_and_serialized_output_is_retained() {
    let archive = crate::chunks::ArchiveVersion::V5;
    let definition = support::definition_record(
        archive,
        &support::v5_definition_payload(archive, 6, [0x51; 16], &[[0x62; 16], [0x62; 16]], false),
    );
    let scan = crate::container::scan_owned(support::document_with_definitions(
        "50",
        archive,
        &[definition],
        &[],
    ))
    .unwrap();
    for (dimension, operation) in [
        (
            ResourceDimension::MaterializedBytes,
            "Rhino definition member UUID text",
        ),
        (
            ResourceDimension::MaterializedBytes,
            "Rhino product definitions",
        ),
        (ResourceDimension::RetainedBytes, "serialize native record"),
    ] {
        cadmpeg_test_support::refusal::resource_limit_at(dimension, operation, |cap| {
            let arena = DecodeArena::new();
            let mut policy = DecodePolicy::service();
            if dimension == ResourceDimension::MaterializedBytes {
                policy.limits.max_materialized_bytes = cap;
            } else {
                policy.limits.max_retained_bytes = cap;
            }
            let (ctx, _) = DecodeContext::from_root_bytes(scan.data, &arena, &policy)?;
            install(&ctx, &scan, &mut CadIr::empty())
        });
    }
    let mut ir = CadIr::empty();
    install(
        &cadmpeg_test_support::service_decode_context(),
        &scan,
        &mut ir,
    )
    .unwrap();
    let definitions = &ir.native.namespace("rhino").unwrap().arenas()["product_definitions"];
    assert_eq!(definitions.len(), 1);
    let member = crate::wire::Uuid::from_wire([0x62; 16]).to_string();
    assert_eq!(
        definitions[0].field("member_object_ids"),
        Some(serde_json::json!([member, member]))
    );
}

#[test]
fn native_occurrence_and_external_staging_use_materialized_storage() {
    for (scan, operations, arena_name) in [
        (
            one_reference_scan(),
            ["Rhino product occurrences"],
            "product_occurrences",
        ),
        (
            one_linked_definition_scan(6),
            ["Rhino external references"],
            "external_references",
        ),
    ] {
        for operation in operations {
            cadmpeg_test_support::refusal::resource_limit_at(
                ResourceDimension::MaterializedBytes,
                operation,
                |cap| {
                    let arena = DecodeArena::new();
                    let mut policy = DecodePolicy::service();
                    policy.limits.max_materialized_bytes = cap;
                    let (ctx, _) = DecodeContext::from_root_bytes(scan.data, &arena, &policy)?;
                    install(&ctx, &scan, &mut CadIr::empty())
                },
            );
        }
        let mut ir = CadIr::empty();
        install(
            &cadmpeg_test_support::service_decode_context(),
            &scan,
            &mut ir,
        )
        .unwrap();
        assert_eq!(
            ir.native.namespace("rhino").unwrap().arenas()[arena_name].len(),
            1
        );
    }
}

#[test]
fn product_staging_borrows_source_names_and_paths_without_copying_them() {
    let scan = one_linked_definition_scan(7);
    for operation in [
        "Rhino product definition name",
        "Rhino product definition description",
        "Rhino product definition URL",
        "Rhino product definition URL tag",
        "Rhino external full path",
        "Rhino external relative path",
    ] {
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        policy.limits.max_work_units = u64::MAX;
        let (ctx, _) = DecodeContext::from_root_bytes(scan.data, &arena, &policy).unwrap();
        let _probe = cadmpeg_core::decode::refusal_probe::RefusalProbe::arm(
            ResourceDimension::WorkUnits,
            operation,
            None,
        );
        let mut ir = CadIr::empty();
        install(&ctx, &scan, &mut ir).expect("no staging text copies");
        let namespace = ir.native.namespace("rhino").unwrap();
        assert_eq!(
            namespace.arenas()["product_definitions"][0].field("name"),
            Some(serde_json::json!("v5 definition"))
        );
        assert_eq!(
            namespace.arenas()["external_references"][0].field("full_path"),
            Some(serde_json::json!("/full/source.3dm"))
        );
        assert_eq!(
            namespace.arenas()["external_references"][0].field("relative_path"),
            Some(serde_json::json!("source.3dm"))
        );
    }
}
