// SPDX-License-Identifier: Apache-2.0
//! Per-arena native dispositions include mixed record outcomes and opaque stubs.

use super::{NativeDisposition, NativeDispositions};
use crate::{ids::Identity, native::NativeRecord, CadIr};

fn record(id: &str) -> NativeRecord {
    NativeRecord::new(Identity::new(id).unwrap(), serde_json::Map::new()).unwrap()
}

fn document() -> CadIr {
    let mut ir = CadIr::empty();
    ir.native.namespace_mut("own").arenas_mut().insert(
        "settings".into(),
        vec![
            record("own:native:setting#one"),
            record("own:native:setting#two"),
        ],
    );
    ir.native.namespace_mut("foreign").arenas_mut().insert(
        "unknowns".into(),
        vec![record("foreign:native:unknown#one")],
    );
    ir
}

#[test]
fn partial_native_drops_charge_exact_counts_without_dropping_valid_siblings() {
    let ir = document();
    let mut dispositions = NativeDispositions::for_namespace(
        "own",
        ["settings", "colors"],
        NativeDisposition::Written,
    );
    dispositions.set_record(
        "own",
        "settings",
        "own:native:setting#two",
        NativeDisposition::Dropped,
    );
    dispositions.complete(&ir);
    assert_eq!(
        dispositions.record_disposition("own", "settings", "own:native:setting#one"),
        NativeDisposition::Written
    );
    let counts = dispositions.dropped_counts(&ir);
    assert_eq!(counts["own"]["settings"], 1);
    assert!(!counts.contains_key("foreign"));
    assert!(!dispositions.namespaces["foreign"]
        .arenas
        .contains_key("unknowns"));
    let losses = dispositions.omission_losses(&ir, "own");
    assert_eq!(losses.len(), 1);
    assert!(losses
        .iter()
        .all(|loss| loss.code.local_code() == "native.records-omitted"
            && loss.severity == crate::report::Severity::Info));
    assert_eq!(
        losses[0].message,
        "the own write omits 1 native record(s) from `own.settings`: own:native:setting#two"
    );
    assert!(!losses[0].message.contains("setting#one"));
}

#[test]
fn regenerated_records_and_cadir_written_namespaces_have_no_omission_loss() {
    let ir = document();
    let mut dispositions = NativeDispositions::written(&ir);
    dispositions.set("own", "settings", NativeDisposition::Regenerated);
    assert!(dispositions.dropped_counts(&ir).is_empty());
    assert!(dispositions.omission_losses(&ir, "cadir").is_empty());
}

#[test]
fn disposition_objects_reject_unknown_fields_and_duplicate_arena_keys() {
    let wire = serde_json::json!({"namespaces":{"own":{"arenas":{
        "settings":{"disposition":"written", "extra":true}
    }}}});
    assert!(serde_json::from_value::<NativeDispositions>(wire).is_err());
    let duplicate = r#"{"namespaces":{"own":{"arenas":{"settings":{"disposition":"written"},"settings":{"disposition":"dropped"}}}}}"#;
    assert!(serde_json::from_str::<NativeDispositions>(duplicate).is_err());
}

#[test]
fn unknown_stubs_are_not_typed_omissions_even_with_explicit_dropped_dispositions() {
    let mut ir = CadIr::empty();
    for namespace in ["own", "foreign"] {
        ir.native.namespace_mut(namespace).arenas_mut().insert(
            "unknowns".into(),
            vec![record(&format!("{namespace}:native:unknown#one"))],
        );
    }
    let mut dispositions = NativeDispositions::default();
    dispositions.complete(&ir);
    assert!(dispositions
        .namespaces
        .values()
        .all(|namespace| namespace.arenas.is_empty()));
    for namespace in ["own", "foreign"] {
        dispositions.set(namespace, "unknowns", NativeDisposition::Dropped);
    }
    assert!(dispositions.dropped_counts(&ir).is_empty());
    assert!(dispositions.omission_losses(&ir, "own").is_empty());
}

#[test]
fn typed_omissions_name_sorted_ids_and_exclude_written_and_opaque_siblings() {
    let mut ir = document();
    ir.native.namespace_mut("foreign").arenas_mut().insert(
        "settings".into(),
        vec![
            record("foreign:native:setting#z"),
            record("foreign:native:setting#keep"),
            record("foreign:native:setting#a"),
        ],
    );
    let mut dispositions = NativeDispositions::written(&ir);
    dispositions.set("foreign", "settings", NativeDisposition::Dropped);
    dispositions.set_record(
        "foreign",
        "settings",
        "foreign:native:setting#keep",
        NativeDisposition::Written,
    );
    dispositions.set("foreign", "unknowns", NativeDisposition::Dropped);
    dispositions.complete(&ir);
    assert_eq!(dispositions.dropped_counts(&ir)["foreign"]["settings"], 2);
    let losses = dispositions.omission_losses(&ir, "own");
    assert_eq!(losses.len(), 1);
    assert_eq!(losses[0].message, "the own write omits 2 native record(s) from `foreign.settings`: foreign:native:setting#a, foreign:native:setting#z");
}
