// SPDX-License-Identifier: Apache-2.0
#![allow(clippy::unwrap_used)]
//! The committed JSON Schema files are the schemas that the `JsonSchema`
//! derives generate. Under the `schema-order` feature, which lists each
//! object's `properties` in declared order, the test compares their text and
//! `UPDATE_GOLDEN=1` rewrites them; under `schema` alone it compares their
//! content.
//!
//! An integration test links the library built without `cfg(test)`, so the
//! `schema-order` build compiles no unit test of the crate.

use std::path::Path;

use cadmpeg_core::serde_erase::json;
use serde_json::Value;

const REGENERATE: &str =
    "UPDATE_GOLDEN=1 cargo test -p cadmpeg-ir --features schema-order --test committed_schema";

#[test]
fn committed_json_schemas_equal_the_derived_schemas() {
    let update = std::env::var_os("UPDATE_GOLDEN").is_some();
    for (file, committed, derived) in [
        (
            "cadir.schema.json",
            cadmpeg_ir::CADIR_JSON_SCHEMA,
            cadmpeg_ir::cadir_json_schema(),
        ),
        (
            "decode-sidecar.schema.json",
            cadmpeg_ir::DECODE_SIDECAR_JSON_SCHEMA,
            cadmpeg_ir::decode_sidecar_json_schema(),
        ),
    ] {
        let path = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("schema")
            .join(file);
        if !cfg!(feature = "schema-order") {
            let committed: Value = serde_json::from_str(committed).unwrap();
            assert!(
                committed == json::to_value(&derived).unwrap(),
                "{} differs from the derived schema. Run `{REGENERATE}` and commit the file.",
                path.display()
            );
            continue;
        }
        let derived = json::to_string_pretty(&derived).unwrap() + "\n";
        if update {
            std::fs::write(&path, &derived).unwrap();
            continue;
        }
        let line = committed
            .lines()
            .zip(derived.lines())
            .position(|(committed, derived)| committed != derived)
            .unwrap_or_else(|| committed.lines().count().min(derived.lines().count()))
            + 1;
        assert!(
            committed == derived,
            "{} differs from the derived schema at line {line}. Run `{REGENERATE}` and commit \
             the file.",
            path.display()
        );
    }
}
