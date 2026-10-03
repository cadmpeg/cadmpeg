// SPDX-License-Identifier: Apache-2.0
//! Metadata decode allocation boundaries.

use cadmpeg_core::decode::ResourceDimension;

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

    crate::test_support::with_decode_context_over(
        &[],
        |policy| {
            policy.limits.max_collection_items = 0;
        },
        |ctx| {
            let root = cadmpeg_core::decode::View::over_retained(&[]);

            let error = super::super::build_metadata_ir(ctx, root, &scan, &dialects)
                .expect_err("one unknown stream needs one collection item");
            assert!(matches!(
                error,
                cadmpeg_core::CodecError::ResourceLimit(limit)
                    if limit.dimension == ResourceDimension::CollectionItems
                        && limit.operation == "nx metadata unknown streams"
            ));
        },
    );
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
    let (dialects, _) =
        crate::test_support::with_decode_context(|ctx| crate::dialect::classify_layers(ctx, &scan))
            .expect("empty scan dialect")
            .into_report_parts();

    crate::test_support::with_decode_context_over(
        &[],
        |policy| {
            policy.limits.max_retained_bytes = 0;
        },
        |ctx| {
            let root = cadmpeg_core::decode::View::over_retained(&[]);

            let error = super::super::build_metadata_ir(ctx, root, &scan, &dialects)
                .expect_err("source attribute needs retained bytes");
            assert!(matches!(
                error,
                cadmpeg_core::CodecError::ResourceLimit(limit)
                    if limit.dimension == ResourceDimension::RetainedBytes
            ));
        },
    );
}

#[test]
fn metadata_unknown_stream_refuses_work_limit() {
    let bytes = crate::test_support::test_prt::prt_with_partition(
        &crate::test_support::test_streams::topology_partition_stream(),
    );

    crate::test_support::with_decode_context_over(
        &bytes,
        |_| {},
        |scan_ctx| {
            let scan_root = cadmpeg_core::decode::View::over_retained(&bytes);

            let scan = crate::decode::scan(scan_ctx, scan_root).expect("valid topology container");
            let (dialects, _) = crate::dialect::classify_layers(scan_ctx, &scan)
                .expect("classified topology input")
                .into_report_parts();

            crate::test_support::with_decode_context_over(
                &bytes,
                |policy| {
                    policy.limits.max_work_units = 0;
                },
                |ctx| {
                    let root = cadmpeg_core::decode::View::over_retained(&bytes);

                    let error = super::super::build_metadata_ir(ctx, root, &scan, &dialects)
                        .expect_err("one unknown stream needs digest work");
                    assert!(matches!(
                        error,
                        cadmpeg_core::CodecError::ResourceLimit(limit)
                            if limit.dimension == ResourceDimension::WorkUnits
                    ));
                },
            );
        },
    );
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

    crate::test_support::with_decode_context_over(
        &[],
        |policy| {
            policy.limits.max_retained_bytes = 0;
        },
        |ctx| {
            let error = super::super::report_untransferred_streams(
                ctx,
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
        },
    );
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

    crate::test_support::with_decode_context_over(
        &[],
        |policy| {
            policy.limits.max_collection_items = 0;
        },
        |ctx| {
            let error = super::super::report_untransferred_streams(
                ctx,
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
        },
    );
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

    crate::test_support::with_decode_context_over(
        &[],
        |policy| {
            policy.limits.max_retained_bytes = 0;
        },
        |ctx| {
            let error = super::super::build_container_body(ctx, &scan, Vec::new(), Vec::new())
                .expect_err("loss code needs retained bytes");
            assert!(matches!(
                error,
                cadmpeg_core::CodecError::ResourceLimit(limit)
                    if limit.dimension == ResourceDimension::RetainedBytes
                        && limit.operation == "nx loss code text"
            ));
        },
    );
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

    crate::test_support::with_decode_context_over(
        &[],
        |policy| {
            policy.limits.max_collection_items = 0;
        },
        |ctx| {
            let error = super::super::build_container_body(ctx, &scan, Vec::new(), Vec::new())
                .expect_err("one loss needs one collection item");
            assert!(matches!(
                error,
                cadmpeg_core::CodecError::ResourceLimit(limit)
                    if limit.dimension == ResourceDimension::CollectionItems
            ));
        },
    );
}

fn decoded_unknown_limit_error(
    adjust: impl FnOnce(&mut cadmpeg_core::decode::DecodePolicy),
) -> cadmpeg_core::CodecError {
    let mut ir = cadmpeg_ir::CadIr::empty();
    ir.set_native_unknowns(
        &cadmpeg_test_support::service_decode_context(),
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
        cadmpeg_ir::ids::UnknownId::mint("test:model:entity#incoming").expect("identity grammar"),
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

    crate::test_support::with_decode_context_over(&[], adjust, |ctx| {
        match super::super::decoded(
            ctx,
            ir,
            body,
            cadmpeg_ir::Annotations::default(),
            vec![unknown],
            &mut 0,
        ) {
            Err(error) => error,
            Ok(_) => panic!("unknown attachment must refuse the low limit"),
        }
    })
}

#[test]
fn decoded_route_refuses_collection_limit() {
    let adjust_policy = |policy: &mut cadmpeg_core::decode::DecodePolicy| {
        policy.limits.max_collection_items = 0;
    };
    assert!(matches!(
        decoded_unknown_limit_error(adjust_policy),
        cadmpeg_core::CodecError::ResourceLimit(limit)
            if limit.dimension == ResourceDimension::CollectionItems
    ));
}

#[test]
fn decoded_route_refuses_retained_limit() {
    let adjust_policy = |policy: &mut cadmpeg_core::decode::DecodePolicy| {
        policy.limits.max_retained_bytes = 0;
    };
    assert!(matches!(
        decoded_unknown_limit_error(adjust_policy),
        cadmpeg_core::CodecError::ResourceLimit(limit)
            if limit.dimension == ResourceDimension::RetainedBytes
    ));
}

#[test]
fn decoded_route_refuses_nesting_limit() {
    let adjust_policy = |policy: &mut cadmpeg_core::decode::DecodePolicy| {
        policy.limits.max_recursion_depth = 0;
    };
    assert!(matches!(
        decoded_unknown_limit_error(adjust_policy),
        cadmpeg_core::CodecError::ResourceLimit(limit)
            if limit.dimension == ResourceDimension::RecursionDepth
    ));
}

#[test]
fn decoded_route_refuses_work_limit() {
    let adjust_policy = |policy: &mut cadmpeg_core::decode::DecodePolicy| {
        policy.limits.max_work_units = 0;
    };
    assert!(matches!(
        decoded_unknown_limit_error(adjust_policy),
        cadmpeg_core::CodecError::ResourceLimit(limit)
            if limit.dimension == ResourceDimension::WorkUnits
    ));
}
