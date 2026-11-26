// SPDX-License-Identifier: Apache-2.0
//! Metadata decode allocation boundaries.

use cadmpeg_core::decode::ResourceDimension;

fn decoded_unknown_limit_error(
    adjust: impl FnOnce(&mut cadmpeg_core::decode::DecodePolicy),
) -> cadmpeg_core::CodecError {
    let mut ir = cadmpeg_ir::CadIr::empty();
    ir.set_native_unknowns(
        &cadmpeg_core::decode::test_support::service_decode_context(),
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
            cadmpeg_ir::hash::digest::Sha256Digest::digest(&[]),
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
fn bounded_indexed_section_search_gives_up_its_entry_as_a_part() {
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
        untyped: std::cell::RefCell::default(),
    };
    let mut decoded = cadmpeg_ir::codec::Decoded {
        ir: cadmpeg_ir::CadIr::empty(),
        body: cadmpeg_ir::codec::DecodeBody {
            transfer: cadmpeg_ir::report::decode::DecodeTransfer::full(false),
            coverage: cadmpeg_ir::report::decode::Coverage::default(),
            losses: Vec::new(),
            notes: Vec::new(),
            transfer_ledger: cadmpeg_ir::report::decode::TransferLedger::default(),
        },
        source_fidelity: cadmpeg_ir::SourceFidelity::new(
            cadmpeg_ir::hash::digest::Sha256Digest::digest(file.as_slice()),
        ),
    };

    crate::test_support::with_decode_context(|ctx| {
        super::super::give_up_bounded_om_entries(ctx, &scan, &mut decoded)
    })
    .expect("the bounded entry is given up");

    let body = &decoded.body;
    let bounded = body
        .losses
        .iter()
        .filter(|loss| loss.code == crate::loss::NxLossCode::SourceScanBounded.kind())
        .collect::<Vec<_>>();
    assert_eq!(bounded.len(), 1);
    assert!(bounded[0].message.contains("/Root/UG_PART/UG_PART"));
    assert!(bounded[0]
        .message
        .contains(&crate::om::MAX_INDEXED_SECTION_REGISTRY_WORK.to_string()));
    let (offset, length) = scan.container.entries[0]
        .file_span()
        .expect("the bounded entry is a file");
    let given_up = body
        .losses
        .iter()
        .filter(|loss| loss.code == cadmpeg_ir::unknown::part::record_unreadable_kind())
        .collect::<Vec<_>>();
    assert_eq!(given_up.len(), 1);
    let prefix = format!(
        "part om-entry \"/Root/UG_PART/UG_PART\" stream=\"nx:container\" \
         offset={offset} length={length} record=nx:source:part#"
    );
    assert!(given_up[0].message.starts_with(&prefix), "{given_up:#?}");
    let stub = given_up[0]
        .message
        .split_whitespace()
        .find_map(|token| token.strip_prefix("record=")?.strip_suffix(':'))
        .expect("the charge names its stub");
    let stubs = decoded.ir.native_unknowns("nx").expect("the stub reads");
    assert!(stubs.iter().any(|unknown| unknown.id.as_str() == stub));
    let start = usize::try_from(offset).expect("offset fits");
    let end = start
        .checked_add(usize::try_from(length).expect("length fits"))
        .expect("the entry ends in the file");
    assert_eq!(
        decoded
            .source_fidelity
            .retained_record(stub)
            .and_then(|record| record.data()),
        Some(&file.as_slice()[start..end])
    );
}
