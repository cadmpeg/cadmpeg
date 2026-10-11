// SPDX-License-Identifier: Apache-2.0
//! Initial container-report loss storage and fixed-work controls.

use cadmpeg_core::decode::refusal_probe::RefusalProbe;
use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
use cadmpeg_core::CodecError;
use cadmpeg_ir::report::loss::LossNote;

use crate::container::{ContainerScan, SolidWorksEnvelopeScan};
use crate::loss::SldprtLossCode;

const OPERATION: &str = "retain SLDPRT container report losses";

fn empty_scan() -> ContainerScan<'static> {
    ContainerScan {
        source_image: &[],
        version: 0,
        blocks: Vec::new(),
        directory: Vec::new(),
        cache_cells: Vec::new(),
        compound_streams: Vec::new(),
        solidworks: SolidWorksEnvelopeScan::default(),
    }
}

fn initial_loss_storage_refuses(dimension: ResourceDimension) {
    let scan = empty_scan();
    let fixture_arena = DecodeArena::new();
    let (fixture_ctx, _) =
        DecodeContext::from_root_bytes(&[], &fixture_arena, &DecodePolicy::service())
            .expect("fixture context");
    let classification = crate::dialect::classify_layers(&fixture_ctx, &scan)
        .expect("classify empty source in its own session");
    let mut policy = DecodePolicy::service();
    // The source has no rows. Exact growth allocates three complete LossNote slots;
    // their message backing is separate from this vector-shell invariant.
    let additional = match dimension {
        ResourceDimension::RetainedBytes => {
            let bytes = cadmpeg_core::decode::u64_from_index(3 * std::mem::size_of::<LossNote>());
            policy.limits.max_retained_bytes = bytes - 1;
            bytes
        }
        ResourceDimension::CollectionItems => {
            policy.limits.max_collection_items = 2;
            3
        }
        _ => panic!("unsupported loss-storage dimension"),
    };
    let arena = DecodeArena::new();
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("report context");
    let error =
        super::super::build_container_report(&ctx, &scan, &classification, None, Vec::new())
            .expect_err("the initial three-slot admission refuses");
    let CodecError::ResourceLimit(limit) = error else {
        panic!("expected initial loss-storage refusal");
    };
    assert_eq!(limit.dimension, dimension);
    assert_eq!(limit.operation, OPERATION);
    assert_eq!(limit.used, 0);
    assert_eq!(limit.additional, additional);
    assert_eq!(limit.limit, additional - 1);
    assert_eq!(ctx.resource_refusal().as_ref(), Some(&limit));
    assert!(
        matches!(ctx.finish_session(), Err(CodecError::ResourceLimit(first)) if first == limit)
    );
}

#[test]
fn container_report_initial_loss_slots_refuse_one_byte_below_need() {
    initial_loss_storage_refuses(ResourceDimension::RetainedBytes);
}

#[test]
fn container_report_initial_loss_slots_refuse_one_item_below_need() {
    initial_loss_storage_refuses(ResourceDimension::CollectionItems);
}

#[test]
fn container_report_initial_loss_slots_preserve_order_without_fixed_work() {
    let scan = empty_scan();
    let fixture_arena = DecodeArena::new();
    let (fixture_ctx, _) =
        DecodeContext::from_root_bytes(&[], &fixture_arena, &DecodePolicy::service())
            .expect("fixture context");
    let classification = crate::dialect::classify_layers(&fixture_ctx, &scan)
        .expect("classify empty source in its own session");
    let mut policy = DecodePolicy::service();
    policy.limits.max_work_units = u64::MAX;
    let arena = DecodeArena::new();
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("report context");
    let probe = RefusalProbe::arm(ResourceDimension::WorkUnits, OPERATION, None);
    let report =
        super::super::build_container_report(&ctx, &scan, &classification, None, Vec::new())
            .expect("fixed initial slots have no input-sized work");
    drop(probe);
    assert_eq!(
        report.losses[0].code,
        SldprtLossCode::GeometryParasolidNotTransferred.kind()
    );
    assert_eq!(
        report.losses[1].code,
        SldprtLossCode::TopologyGraphNotTransferred.kind()
    );
    assert_eq!(
        report.losses[2].code,
        SldprtLossCode::MaterialMetadataNotTransferred.kind()
    );
    assert_eq!(
        report.losses[3].code,
        SldprtLossCode::ContainerNoParasolidStream.kind()
    );
    assert_eq!(
        report.losses[0].message,
        "Parasolid B-rep geometry was not transferred: no partition/deltas stream resolved \
         into a topology graph. 0 payload source(s) were enumerated, \
         0 carrying Parasolid streams."
    );
    assert_eq!(
        report.losses[1].message,
        "B-rep topology graph (body/region/shell/face/loop/coedge/edge/vertex) was not built \
         for this file."
    );
    assert_eq!(
        report.losses[2].message,
        "Body-bound appearances and tessellation were not transferred because no body graph \
         exists."
    );
    assert!(ctx.resource_refusal().is_none());
    ctx.finish_session().expect("unrefused report session");
}
