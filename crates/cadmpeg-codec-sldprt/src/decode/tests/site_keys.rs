// SPDX-License-Identifier: Apache-2.0
//! Container site-key identity tests.
#![allow(clippy::unwrap_used)]

use crate::container::{Block, CompoundStream};

#[test]
fn site_keys_use_outer_container_identity() {
    let first = Block {
        offset: 100,
        type_id: 0,
        comp_sz: 0,
        uncomp_sz: 0,
        section: Some("Contents/Config-0-Partition".into()),
        family: "parasolid",
        payload: Vec::new(),
        ps_streams: Vec::new(),
    };
    let second = Block {
        offset: 200,
        section: first.section.clone(),
        ..first.clone()
    };
    assert_ne!(
        super::super::BodyOrigin::Block(&first).site_key(),
        super::super::BodyOrigin::Block(&second).site_key()
    );

    let compound = CompoundStream {
        path: "Contents/Config-0-Partition".into(),
        directory_id: 300,
        start_sector: 0,
        payload: Vec::new(),
        decoded_payload: None,
        ps_streams: Vec::new(),
    };
    assert_eq!(
        super::super::BodyOrigin::Compound(&compound).site_key(),
        "compound@300"
    );
}
