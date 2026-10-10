// SPDX-License-Identifier: Apache-2.0

use super::*;
use super::super::{external_record, LinkSource};
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
            install(&ctx, &scan, &mut CadIr::empty()).map(drop)
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
                    install(&ctx, &scan, &mut CadIr::empty()).map(drop)
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
    let mut prior_staging_bytes = None;
    for length in [1, 4096] {
        let path = "p".repeat(length);
        let archive = crate::chunks::ArchiveVersion::V5;
        let payload = support::v5_definition_payload_with_paths(archive, 6, [0x51; 16], &[], true, &path, false);
        let record = support::definition_record(archive, &payload);
        let scan = crate::container::scan_owned(support::document_with_definitions("50", archive, &[record], &[])).unwrap();
        let error = cadmpeg_test_support::refusal::resource_limit_at(
            ResourceDimension::MaterializedBytes, "Rhino product definitions", |cap| {
                let arena = DecodeArena::new();
                let mut policy = DecodePolicy::service();
                policy.limits.max_materialized_bytes = cap;
                let (ctx, _) = DecodeContext::from_root_bytes(scan.data, &arena, &policy)?;
                install(&ctx, &scan, &mut CadIr::empty()).map(drop)
            });
        let cadmpeg_core::CodecError::ResourceLimit(limit) = error else { panic!("staging storage refusal"); };
        if let Some(prior) = prior_staging_bytes { assert_eq!(limit.used, prior); }
        prior_staging_bytes = Some(limit.used);
        let definition = &scan.definitions.definitions()[0];
        let LinkSource::LegacyFull(source_path) = &definition.link else { panic!("legacy full path"); };
        let ctx = cadmpeg_test_support::service_decode_context();
        let record = external_record(&ctx, definition.id(), &definition.link).unwrap().unwrap();
        assert_eq!(record.full_path.as_ptr(), source_path.as_str().as_ptr());
        assert_eq!(record.full_path.len(), path.len());
    }
    let scan = one_linked_definition_scan(7);
    let definition = &scan.definitions.definitions()[0];
    let LinkSource::Structured(reference) = &definition.link else { panic!("structured reference"); };
    let ctx = cadmpeg_test_support::service_decode_context();
    let record = external_record(&ctx, definition.id(), &definition.link).unwrap().unwrap();
    assert_eq!(record.full_path.as_ptr(), reference.full_path.as_ptr());
    assert_eq!(record.relative_path.as_ptr(), reference.relative_path.as_ptr());
    let mut ir = CadIr::empty();
    install(&ctx, &scan, &mut ir).expect("borrowed fields serialize");
    let namespace = ir.native.namespace("rhino").unwrap();
    assert_eq!(namespace.arenas()["product_definitions"][0].field("name"), Some(serde_json::json!("v5 definition")));
    assert_eq!(namespace.arenas()["external_references"][0].field("full_path"), Some(serde_json::json!("/full/source.3dm")));
    assert_eq!(namespace.arenas()["external_references"][0].field("relative_path"), Some(serde_json::json!("source.3dm")));
}


fn assert_product_first_visit_refusal(scan: &crate::container::Scan<'_>, operation: &'static str) {
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_work_units = 0;
    policy.limits.max_materialized_bytes = 0;
    policy.limits.max_retained_bytes = 0;
    policy.limits.max_collection_items = 0;
    let (ctx, _) = DecodeContext::from_root_bytes(scan.data, &arena, &policy).unwrap();
    let mut ir = CadIr::empty();
    let error = install(&ctx, scan, &mut ir).expect_err("only the first visit is refused");
    let cadmpeg_core::CodecError::ResourceLimit(refusal) = error else {
        panic!("product traversal resource refusal");
    };
    assert_eq!(refusal.dimension, ResourceDimension::WorkUnits);
    assert_eq!(refusal.operation, operation);
    assert_eq!(refusal.used, 0);
    assert_eq!(refusal.additional, 1);
    assert_eq!(ctx.resource_refusal(), Some(refusal));
    assert!(ir.native.namespace("rhino").is_none());
    assert!(matches!(ctx.finish_session(),
        Err(cadmpeg_core::CodecError::ResourceLimit(sticky)) if sticky == refusal));
}

#[test]
fn product_object_traversal_refuses_only_first_visit() {
    let archive = crate::chunks::ArchiveVersion::V5;
    let record = support::object_record_with_payload(
        archive, 0x1000, support::INSTANCE_REFERENCE_CLASS,
        &support::instance_reference_payload([0x51; 16], cadmpeg_ir::transform::Transform::identity().rows()),
    );
    let scan = support::scan_with_objects(&vec![record; 1024]);
    assert_eq!(scan.objects.len(), 1024);
    assert_product_first_visit_refusal(&scan, "Rhino install traversal");
}

#[test]
fn product_definition_traversal_refuses_only_first_visit() {
    let archive = crate::chunks::ArchiveVersion::V5;
    // Each definition must remain unique after source ambiguity resolution.
    let records: Vec<_> = (0_u16..1024).map(|index| {
        let mut id = [0x51; 16];
        id[..2].copy_from_slice(&index.to_be_bytes());
        support::definition_record(
            archive, &support::v5_definition_payload(archive, 6, id, &[], false),
        )
    }).collect();
    let scan = crate::container::scan_owned(support::document_with_definitions(
        "50", archive, &records, &[],
    )).expect("definition document is framed");
    assert!(scan.objects.is_empty());
    assert_eq!(scan.definitions.definitions().len(), 1024);
    assert_product_first_visit_refusal(&scan, "Rhino install borrowed traversal");
}
