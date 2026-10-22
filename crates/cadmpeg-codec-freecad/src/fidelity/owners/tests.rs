// SPDX-License-Identifier: Apache-2.0
//! Closed owner fields, target existence, and reserved unknown handling.

use std::collections::BTreeMap;

use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy};
use cadmpeg_ir::{ids::Identity, native::NativeRecord, CadIr};
use serde_json::{json, Value};

use super::Owners;

fn row(id: &str, fields: Value) -> NativeRecord {
    NativeRecord::new(
        Identity::new(id).unwrap(),
        fields.as_object().unwrap().clone(),
    )
    .unwrap()
}

fn owners(ir: &CadIr) -> Result<BTreeMap<String, Option<String>>, cadmpeg_core::CodecError> {
    let arena = DecodeArena::new();
    let policy = DecodePolicy::service();
    let (ctx, _) = DecodeContext::from_root_bytes(b"source", &arena, &policy)?;
    let mut found = BTreeMap::new();
    ir.visit_native_provenance_with_owner_resolver(&ctx, &Owners, &mut |record| {
        found.insert(record.id().to_owned(), record.owner().map(str::to_owned));
        Ok(())
    })?;
    Ok(found)
}

#[test]
fn properties_use_the_admitted_owner_identity_and_not_the_persisted_object_name() {
    let object = "fcstd:native:object#Pad";
    let property = "fcstd:native:property#Pad:Length";
    let mut ir = CadIr::empty();
    let native = ir.native.namespace_mut("fcstd").arenas_mut();
    native.insert(
        "objects".into(),
        vec![row(object, json!({"name":"Pad", "owner":"ignored"}))],
    );
    native.insert(
        "properties".into(),
        vec![row(
            property,
            json!({"owner":object, "name":"Length", "family":"scalar", "value":"2 mm"}),
        )],
    );
    assert_eq!(
        owners(&ir).unwrap(),
        BTreeMap::from([
            (object.into(), None),
            (property.into(), Some(object.into())),
        ])
    );
    ir.native.namespace_mut("fcstd").arenas_mut().insert(
        "properties".into(),
        vec![row(property, json!({"owner":"Pad"}))],
    );
    assert!(
        owners(&ir).is_err(),
        "raw names are not admitted owner identities"
    );
}

#[test]
fn optional_gui_and_string_table_owners_remain_absent() {
    let mut ir = CadIr::empty();
    let native = ir.native.namespace_mut("fcstd").arenas_mut();
    native.insert(
        "gui_view_providers".into(),
        vec![row(
            "fcstd:native:view-provider#Orphan",
            json!({"object":null}),
        )],
    );
    native.insert(
        "string_tables".into(),
        vec![row(
            "fcstd:native:string-table#0",
            json!({"owner_property":null}),
        )],
    );
    assert_eq!(
        owners(&ir).unwrap(),
        BTreeMap::from([
            ("fcstd:native:view-provider#Orphan".into(), None),
            ("fcstd:native:string-table#0".into(), None),
        ])
    );
}

#[test]
fn a_new_arena_and_a_missing_required_owner_do_not_fall_back_to_identity_only() {
    let mut ir = CadIr::empty();
    ir.native.namespace_mut("fcstd").arenas_mut().insert(
        "vendor_records".into(),
        vec![row("fcstd:native:vendor#0", json!({}))],
    );
    assert!(owners(&ir)
        .unwrap_err()
        .to_string()
        .contains("has no typed source-owner rule"));
    ir.native.namespace_mut("fcstd").arenas_mut().clear();
    ir.native.namespace_mut("fcstd").arenas_mut().insert(
        "properties".into(),
        vec![row("fcstd:native:property#missing", json!({"owner":null}))],
    );
    assert!(owners(&ir)
        .unwrap_err()
        .to_string()
        .contains("invalid typed owner reference"));
}

#[test]
fn reserved_unknowns_use_the_shared_typed_links_without_an_invented_owner() {
    let mut ir = CadIr::empty();
    ir.native.namespace_mut("fcstd").arenas_mut().insert(
        "unknowns".into(),
        vec![row("fcstd:native:unknown#0", json!({"links":[]}))],
    );
    assert_eq!(
        owners(&ir).unwrap(),
        BTreeMap::from([("fcstd:native:unknown#0".into(), None),])
    );
}

#[test]
fn the_codec_arena_registry_has_an_explicit_owner_rule_for_every_arena() {
    let mut ir = CadIr::empty();
    for arena in crate::native::ARENAS {
        ir.native
            .namespace_mut("fcstd")
            .arenas_mut()
            .insert((*arena).into(), Vec::new());
    }
    assert!(owners(&ir).unwrap().is_empty());
    ir.native.namespace_mut("foreign").arenas_mut().insert(
        "objects".into(),
        vec![row("foreign:native:object#0", json!({}))],
    );
    assert!(owners(&ir)
        .unwrap_err()
        .to_string()
        .contains("typed owner resolution"));
}
