// SPDX-License-Identifier: Apache-2.0
//! Writer-domain synthetic tests.
#![allow(clippy::unwrap_used)]
#![allow(
    clippy::cloned_ref_to_slice_refs,
    clippy::default_trait_access,
    clippy::if_not_else,
    clippy::needless_pass_by_value,
    clippy::range_plus_one,
    clippy::semicolon_if_nothing_returned,
    clippy::trivially_copy_pass_by_ref
)]

use std::io::Cursor;

use cadmpeg_ir::codec::{Codec, DecodeOptions, Encoder};

use crate::test_support::*;
use crate::F3dCodec;

#[test]
fn generated_source_less_rejects_act_without_segment_metadata() {
    use crate::records::ActEntity;

    let mut source_less = cadmpeg_ir::examples::unit_cube();
    let mut native = f3d_native_mut(&mut source_less);
    native.act_entities = vec![ActEntity {
        id: "generated:act-entity#0".into(),
        record_index: 7,
        table_record_index_offset: None,
        channel_record_index_offset: None,
        entity_id: "0_985".into(),
        table_entity_id_offset: None,
        channel_entity_id_offset: None,
        in_table: true,
        channel_class_tag: None,
        channels: Default::default(),
        channel_guid_offsets: Default::default(),
    }];
    drop(native);
    let error = F3dCodec
        .plan(cadmpeg_ir::codec::EncodeInput {
            ir: &source_less,
            fidelity: None,
        })
        .and_then(|plan| plan.write_to(&mut Vec::new()))
        .expect_err("ACT generation without its record registry must fail atomically");
    assert!(error
        .to_string()
        .contains("requires a retained MetaStream record registry"));
}

#[test]
fn generated_f3d_rejects_act_binding_divergence() {
    let source = f3d_with_smbh_and_protein(&synthetic_geometry_smbh());
    let decoded = F3dCodec
        .decode(&mut Cursor::new(&source), &DecodeOptions::default())
        .expect("generated ACT decode");
    let (mut edited, _, fidelity) = decoded.into_parts();
    update_f3d_native(&mut edited, |native| {
        native.act_entities[0].channels.insert(
            "Appearance".into(),
            "dddddddd-1111-2222-3333-eeeeeeeeeeee".into(),
        );
    });

    let error = F3dCodec
        .write_preserved_with_source_fidelity(&edited, &fidelity, &mut Vec::new())
        .expect_err("divergent ACT and appearance binding must fail");
    assert!(matches!(error, cadmpeg_core::CodecError::NotImplemented(_)));
}

#[test]
fn generated_f3d_rejects_act_record_index_edit_without_metastream_edit() {
    let source = f3d_with_smbh_and_protein(&synthetic_geometry_smbh());
    let decoded = F3dCodec
        .decode(&mut Cursor::new(&source), &DecodeOptions::default())
        .expect("generated ACT decode");
    let (mut edited, _, fidelity) = decoded.into_parts();
    update_f3d_native(&mut edited, |native| {
        native.act_root_components[0].record_index += 1;
    });

    let error = F3dCodec
        .write_preserved_with_source_fidelity(&edited, &fidelity, &mut Vec::new())
        .expect_err("an ACT record-index edit without its MetaStream index must fail");
    assert!(matches!(
        error,
        cadmpeg_core::CodecError::NotImplemented(message)
            if message.contains("ACT root edit changes fields")
    ));
}
