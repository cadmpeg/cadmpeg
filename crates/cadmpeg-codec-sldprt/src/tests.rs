// SPDX-License-Identifier: Apache-2.0
//! Synthetic `.sldprt` byte-fixture tests.
#![allow(clippy::unwrap_used)]

#[test]
fn source_record_join_borrows_the_retained_source_image() {
    let payload = vec![0x5a; 4096];
    let payload_ptr = payload.as_ptr();
    let mut fidelity = cadmpeg_ir::SourceFidelity::default();
    fidelity.retained_records = vec![cadmpeg_ir::source_fidelity::RetainedSourceRecord {
        id: "sldprt:file:source-image#0".into(),
        stream: "source".into(),
        offset: 0,
        byte_len: payload.len() as u64,
        sha256: cadmpeg_ir::hash::sha256_hex(&payload),
        data: Some(payload),
    }];

    let records = crate::source_records(&cadmpeg_ir::examples::unit_cube(), &fidelity).unwrap();
    let retained = records[0].data.expect("retained source bytes");
    assert_eq!(retained.as_ptr(), payload_ptr);
}

#[path = "integration_tests.rs"]
mod integration_tests;
