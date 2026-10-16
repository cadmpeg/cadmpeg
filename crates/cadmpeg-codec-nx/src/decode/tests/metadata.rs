// SPDX-License-Identifier: Apache-2.0
//! Metadata decode allocation boundaries.

use cadmpeg_core::decode::ResourceDimension;

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
fn bounded_indexed_section_search_reports_a_scan_loss() {
    let file = crate::test_support::test_prt::prt_with_named_payloads(&[(
        "/Root/UG_PART/UG_PART",
        crate::test_support::test_om::indexed_om_section(),
    )]);
    let container = crate::container::scan_bytes(file.as_slice()).unwrap();
    container
        .indexed_section_layouts
        .set(crate::container::IndexedSectionCache::Borrowed {
            sections: Vec::new(),
            blocks: std::collections::BTreeMap::new(),
            bounded_entries: vec![0],
        })
        .expect("the cache is empty");
    let scan = super::super::Scan {
        container,
        streams: Vec::new(),
    };
    let mut body = cadmpeg_ir::codec::DecodeBody {
        transfer: cadmpeg_ir::report::decode::DecodeTransfer::full(false),
        coverage: cadmpeg_ir::report::decode::Coverage::default(),
        losses: Vec::new(),
        notes: Vec::new(),
        transfer_ledger: cadmpeg_ir::report::decode::TransferLedger::default(),
    };

    super::super::report_untransferred_streams(
        &scan,
        &mut body,
        crate::native::TypedNative::Available,
    );

    let bounded = body
        .losses
        .iter()
        .filter(|loss| loss.code == crate::loss::NxLossCode::SourceScanBounded.kind())
        .collect::<Vec<_>>();
    assert_eq!(bounded.len(), 1);
    assert!(bounded[0].message.contains("/Root/UG_PART/UG_PART"));
}
