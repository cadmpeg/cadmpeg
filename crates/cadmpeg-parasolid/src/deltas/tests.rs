// SPDX-License-Identifier: Apache-2.0
//! The former scanner/replay invariants are now checked at the owner and semantic boundaries.

#[test]
fn declaration_only_input_has_no_committed_instance() {
    let mut bytes = crate::test_support::test_pmarks::prefix(1);
    let at = bytes.len();
    bytes.extend(crate::test_support::owned_deltas::body(36).0);
    crate::test_support::with_decode_context(|_ctx| {
        let owner = crate::records::DeltasRecords::read(&bytes).unwrap();
        assert_eq!(owner.census().instance_count(), 0);
        assert_eq!(owner.definitions().len(), 2);
        assert_eq!(owner.suffix(), at..bytes.len());
    });
}
