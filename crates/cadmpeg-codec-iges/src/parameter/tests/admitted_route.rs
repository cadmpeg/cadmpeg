// SPDX-License-Identifier: Apache-2.0
use std::collections::BTreeSet;

use crate::test_support::test_owned::{owned_test_file, OwnedTestEntity};

fn two_points() -> Vec<u8> {
    owned_test_file(&[
        OwnedTestEntity {
            entity_type: 116,
            form: 0,
            label: "FIRST".into(),
            status: "00000000",
            parameters: "116,1,2,3,0;".into(),
        },
        OwnedTestEntity {
            entity_type: 116,
            form: 0,
            label: "SECOND".into(),
            status: "00000000",
            parameters: "116,4,5,6,0;".into(),
        },
    ])
}

#[test]
fn each_parameter_record_takes_its_own_route() {
    let bytes = two_points();
    let arena = cadmpeg_core::decode::DecodeArena::new();
    let (ctx, _) = cadmpeg_core::decode::DecodeContext::from_root_bytes(
        &bytes,
        &arena,
        &cadmpeg_core::decode::DecodePolicy::default(),
    )
    .unwrap();
    let scan = crate::card::scan(&bytes).unwrap();
    let (global, _) = crate::global::parse(&scan).unwrap();
    let (directory, _) = crate::directory::parse(&scan, global.global_table(), &ctx).unwrap();
    let legacy = super::super::assemble(&scan, &directory, [], &global).unwrap();
    let [first, second] = legacy.records.as_slice() else {
        panic!("expected two Parameter records");
    };
    let mut admitted = second.clone();
    admitted.comment = b"admitted".to_vec();

    // #222: one record that is not admitted is assembled from its cards;
    // the admitted record keeps its admission.
    let unadmitted = BTreeSet::from([first.directory_sequence]);
    let assembly = super::super::assemble_with_admitted(
        &scan,
        &directory,
        [],
        super::super::AdmittedRecords {
            records: vec![admitted.clone()],
            unadmitted: &unadmitted,
        },
        &global,
    )
    .unwrap();
    assert_eq!(assembly.records, [first.clone(), admitted.clone()]);
    assert_eq!(
        assembly.trailing_pointer_analysis,
        legacy.trailing_pointer_analysis
    );
    assert!(assembly.quarantined.is_empty());

    // An entry with no Parameter record is neither admitted nor assembled.
    let assembly = super::super::assemble_with_admitted(
        &scan,
        &directory,
        [],
        super::super::AdmittedRecords {
            records: vec![admitted.clone()],
            unadmitted: &BTreeSet::new(),
        },
        &global,
    )
    .unwrap();
    assert_eq!(assembly.records, [admitted]);
    assert!(assembly.quarantined.is_empty());
}
