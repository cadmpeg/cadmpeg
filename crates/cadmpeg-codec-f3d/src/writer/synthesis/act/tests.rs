// SPDX-License-Identifier: Apache-2.0
//! Omission of a canonical binding does not remove independent ACT entities.

use cadmpeg_ir::codec::{Codec, DecodeOptions};
use std::io::Cursor;

#[test]
fn omitted_appearance_carrier_keeps_registered_native_only_sibling() {
    let source = crate::test_support::zip_test::f3d_with_smbh_and_protein(
        &crate::test_support::smbh_geometry_test::synthetic_geometry_smbh(),
    );
    let decoded = crate::F3dCodec
        .decode(&mut Cursor::new(source), &DecodeOptions::default())
        .unwrap();
    let mut ir = decoded.ir().clone();
    let mut native = crate::test_support::native_test::f3d_native(&ir);
    let original = &native.act_entities[0];
    let original_id = original.id().clone();
    let original_index = original.record_index();
    let sibling_id = format!("{}#9000", original.id().rsplit_once('#').unwrap().0);
    let mut sibling = serde_json::to_value(original).unwrap();
    sibling["id"] = serde_json::json!(sibling_id);
    sibling["record_index"] = serde_json::json!(9000);
    sibling["entity_id"] = serde_json::json!("0_777");
    sibling["channels"] = serde_json::json!({
        "Independent": "01234567-89ab-cdef-0123-456789abcdef",
    });
    native
        .act_entities
        .push(serde_json::from_value(sibling).unwrap());
    let registration = native
        .act_types
        .iter_mut()
        .find(|record| {
            record
                .entities
                .values()
                .any(|value| *value == u64::from(original_index))
        })
        .unwrap();
    let mut entities = registration.entities.values().copied().collect::<Vec<_>>();
    entities.push(9000);
    registration.set_entities(crate::records::identity::ReferenceRun::unlocated(entities));
    let guids = native.act_guids.clone();
    ir.model.appearance_bindings.clear();
    let mut losses = Vec::new();
    super::reconcile(&ir, &mut native, &mut losses).unwrap();
    assert_eq!(native.act_entities.len(), 1);
    assert_eq!(native.act_entities[0].id(), &sibling_id);
    assert_eq!(
        native.act_entities[0].channel_group().channels()["Independent"]
            .value
            .as_str(),
        "01234567-89ab-cdef-0123-456789abcdef"
    );
    assert_eq!(native.act_guids, guids);
    assert!(losses
        .iter()
        .any(|loss| loss.message.contains(&original_id)));
}
