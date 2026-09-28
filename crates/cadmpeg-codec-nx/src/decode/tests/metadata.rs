// SPDX-License-Identifier: Apache-2.0
//! Metadata decode allocation boundaries.

use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};

#[test]
fn metadata_unknown_stream_slots_refuse_at_collection_limit() {
    let scan = crate::decode::Scan {
        container: crate::container::Container {
            data: Vec::new().into(),
            physical_size: 0,
            layout: crate::container::test_modern_layout(0x06),
            entries: Vec::new(),
            fastload_table: None,
            indexed_section_layouts: std::sync::OnceLock::new(),
            om_section_cache: std::sync::OnceLock::new(),
        },
        streams: vec![crate::parasolid::Stream {
            file_offset: 0,
            consumed: 0,
            inflated: Vec::new(),
            body: crate::parasolid::StreamBody::Parasolid {
                subtype: crate::parasolid::ParasolidSubtype::Partition,
                schema: None,
            },
        }],
    };
    let (dialects, _) =
        crate::test_support::with_decode_context(|ctx| crate::dialect::classify_layers(ctx, &scan))
            .unwrap()
            .into_report_parts();
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_collection_items = 0;
    let (ctx, root) =
        DecodeContext::from_root_bytes(&[], &arena, &policy).expect("empty root fits policy");
    let error = super::super::build_metadata_ir(&ctx, root, &scan, &dialects)
        .expect_err("one unknown stream needs one collection item");
    assert!(matches!(
        error,
        cadmpeg_core::CodecError::ResourceLimit(limit)
            if limit.dimension == ResourceDimension::CollectionItems
                && limit.operation == "nx metadata unknown streams"
    ));
}

#[test]
fn metadata_source_refuses_retained_attribute_limit() {
    let scan = crate::decode::Scan {
        container: crate::container::Container {
            data: Vec::new().into(),
            physical_size: 0,
            layout: crate::container::test_modern_layout(0x06),
            entries: Vec::new(),
            fastload_table: None,
            indexed_section_layouts: std::sync::OnceLock::new(),
            om_section_cache: std::sync::OnceLock::new(),
        },
        streams: Vec::new(),
    };
    let (dialects, _) = crate::test_support::with_decode_context(|ctx| {
        crate::dialect::classify_layers(ctx, &scan)
    })
    .expect("empty scan dialect")
    .into_report_parts();
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_retained_bytes = 0;
    let (ctx, root) = DecodeContext::from_root_bytes(&[], &arena, &policy)
        .expect("empty root fits policy");
    let error = super::super::build_metadata_ir(&ctx, root, &scan, &dialects)
        .expect_err("source attribute needs retained bytes");
    assert!(matches!(
        error,
        cadmpeg_core::CodecError::ResourceLimit(limit)
            if limit.dimension == ResourceDimension::RetainedBytes
    ));
}

#[test]
fn untransferred_stream_report_refuses_loss_code_retained_limit() {
    let scan = crate::decode::Scan {
        container: crate::container::Container {
            data: Vec::new().into(),
            physical_size: 0,
            layout: crate::container::test_modern_layout(0x06),
            entries: Vec::new(),
            fastload_table: None,
            indexed_section_layouts: std::sync::OnceLock::new(),
            om_section_cache: std::sync::OnceLock::new(),
        },
        streams: vec![crate::parasolid::Stream {
            file_offset: 0,
            consumed: 0,
            inflated: Vec::new(),
            body: crate::parasolid::StreamBody::Preview,
        }],
    };
    let mut body = cadmpeg_ir::codec::DecodeBody {
        transfer: cadmpeg_ir::report::decode::DecodeTransfer::full(false),
        coverage: cadmpeg_ir::report::decode::Coverage::default(),
        losses: Vec::new(),
        notes: Vec::new(),
        transfer_ledger: cadmpeg_ir::report::decode::TransferLedger::default(),
    };
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_retained_bytes = 0;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy)
        .expect("empty root fits policy");
    let error = super::super::report_untransferred_streams(
        &ctx,
        &scan,
        &mut body,
        crate::native::TypedNative::Available,
    )
    .expect_err("loss code retained refusal");
    assert!(matches!(
        error,
        cadmpeg_core::CodecError::ResourceLimit(limit)
            if limit.dimension == ResourceDimension::RetainedBytes
                && limit.operation == "nx loss code text"
    ));
}

#[test]
fn untransferred_stream_report_refuses_loss_collection_limit() {
    let scan = crate::decode::Scan {
        container: crate::container::Container {
            data: Vec::new().into(),
            physical_size: 0,
            layout: crate::container::test_modern_layout(0x06),
            entries: Vec::new(),
            fastload_table: None,
            indexed_section_layouts: std::sync::OnceLock::new(),
            om_section_cache: std::sync::OnceLock::new(),
        },
        streams: vec![crate::parasolid::Stream {
            file_offset: 0,
            consumed: 0,
            inflated: Vec::new(),
            body: crate::parasolid::StreamBody::Preview,
        }],
    };
    let mut body = cadmpeg_ir::codec::DecodeBody {
        transfer: cadmpeg_ir::report::decode::DecodeTransfer::full(false),
        coverage: cadmpeg_ir::report::decode::Coverage::default(),
        losses: Vec::new(),
        notes: Vec::new(),
        transfer_ledger: cadmpeg_ir::report::decode::TransferLedger::default(),
    };
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_collection_items = 0;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy)
        .expect("empty root fits policy");
    let error = super::super::report_untransferred_streams(
        &ctx,
        &scan,
        &mut body,
        crate::native::TypedNative::Available,
    )
    .expect_err("one loss needs one collection item");
    assert!(matches!(
        error,
        cadmpeg_core::CodecError::ResourceLimit(limit)
            if limit.dimension == ResourceDimension::CollectionItems
    ));
}

#[test]
fn container_body_refuses_loss_code_retained_limit() {
    let scan = crate::decode::Scan {
        container: crate::container::Container {
            data: Vec::new().into(),
            physical_size: 0,
            layout: crate::container::test_modern_layout(0x06),
            entries: Vec::new(),
            fastload_table: None,
            indexed_section_layouts: std::sync::OnceLock::new(),
            om_section_cache: std::sync::OnceLock::new(),
        },
        streams: Vec::new(),
    };
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_retained_bytes = 0;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy)
        .expect("empty root fits policy");
    let error = super::super::build_container_body(&ctx, &scan, Vec::new(), Vec::new())
        .expect_err("loss code needs retained bytes");
    assert!(matches!(
        error,
        cadmpeg_core::CodecError::ResourceLimit(limit)
            if limit.dimension == ResourceDimension::RetainedBytes
                && limit.operation == "nx loss code text"
    ));
}

#[test]
fn container_body_refuses_loss_collection_limit() {
    let scan = crate::decode::Scan {
        container: crate::container::Container {
            data: Vec::new().into(),
            physical_size: 0,
            layout: crate::container::test_modern_layout(0x06),
            entries: Vec::new(),
            fastload_table: None,
            indexed_section_layouts: std::sync::OnceLock::new(),
            om_section_cache: std::sync::OnceLock::new(),
        },
        streams: Vec::new(),
    };
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_collection_items = 0;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy)
        .expect("empty root fits policy");
    let error = super::super::build_container_body(&ctx, &scan, Vec::new(), Vec::new())
        .expect_err("one loss needs one collection item");
    assert!(matches!(
        error,
        cadmpeg_core::CodecError::ResourceLimit(limit)
            if limit.dimension == ResourceDimension::CollectionItems
    ));
}

fn decoded_unknown_limit_error(policy: &DecodePolicy) -> cadmpeg_core::CodecError {
    let mut ir = cadmpeg_ir::CadIr::empty();
    ir.set_native_unknowns(
        "nx",
        &[cadmpeg_ir::NativeUnknownRecord {
            id: cadmpeg_ir::ids::UnknownId::mint("test:model:entity#prior")
                .expect("identity grammar"),
            links: vec![
                cadmpeg_ir::ids::Identity::new("test:model:entity#target".to_string())
                    .expect("identity grammar"),
            ],
        }],
    )
    .expect("prior unknown record");
    let unknown = cadmpeg_ir::UnknownRecord::retained(
        cadmpeg_ir::ids::UnknownId::mint("test:model:entity#incoming")
            .expect("identity grammar"),
        0,
        vec![1],
        Vec::new(),
    );
    let body = cadmpeg_ir::codec::DecodeBody {
        transfer: cadmpeg_ir::report::decode::DecodeTransfer::full(false),
        coverage: cadmpeg_ir::report::decode::Coverage::default(),
        losses: Vec::new(),
        notes: Vec::new(),
        transfer_ledger: cadmpeg_ir::report::decode::TransferLedger::default(),
    };
    let arena = DecodeArena::new();
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, policy)
        .expect("empty root fits policy");
    match super::super::decoded(
        &ctx,
        ir,
        body,
        cadmpeg_ir::Annotations::default(),
        vec![unknown],
        &mut 0,
    ) {
        Err(error) => error,
        Ok(_) => panic!("unknown attachment must refuse the low limit"),
    }
}

#[test]
fn decoded_route_refuses_collection_limit() {
    let mut policy = DecodePolicy::service();
    policy.limits.max_collection_items = 0;
    assert!(matches!(
        decoded_unknown_limit_error(&policy),
        cadmpeg_core::CodecError::ResourceLimit(limit)
            if limit.dimension == ResourceDimension::CollectionItems
    ));
}

#[test]
fn decoded_route_refuses_retained_limit() {
    let mut policy = DecodePolicy::service();
    policy.limits.max_retained_bytes = 0;
    assert!(matches!(
        decoded_unknown_limit_error(&policy),
        cadmpeg_core::CodecError::ResourceLimit(limit)
            if limit.dimension == ResourceDimension::RetainedBytes
    ));
}

#[test]
fn decoded_route_refuses_nesting_limit() {
    let mut policy = DecodePolicy::service();
    policy.limits.max_recursion_depth = 0;
    assert!(matches!(
        decoded_unknown_limit_error(&policy),
        cadmpeg_core::CodecError::ResourceLimit(limit)
            if limit.dimension == ResourceDimension::RecursionDepth
    ));
}

#[test]
fn decoded_route_refuses_work_limit() {
    let mut policy = DecodePolicy::service();
    policy.limits.max_work_units = 0;
    assert!(matches!(
        decoded_unknown_limit_error(&policy),
        cadmpeg_core::CodecError::ResourceLimit(limit)
            if limit.dimension == ResourceDimension::WorkUnits
    ));
}
