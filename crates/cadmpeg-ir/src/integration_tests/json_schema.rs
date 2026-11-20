// SPDX-License-Identifier: Apache-2.0
//! The committed JSON Schema files are the schemas that the `JsonSchema`
//! derives generate. `UPDATE_GOLDEN=1` rewrites them from the derives.

use std::path::Path;

#[test]
fn committed_json_schemas_equal_the_derived_schemas() {
    let update = std::env::var_os("UPDATE_GOLDEN").is_some();
    for (file, committed, derived) in [
        (
            "cadir.schema.json",
            crate::CADIR_JSON_SCHEMA,
            crate::cadir_json_schema(),
        ),
        (
            "decode-sidecar.schema.json",
            crate::DECODE_SIDECAR_JSON_SCHEMA,
            crate::decode_sidecar_json_schema(),
        ),
    ] {
        let derived = serde_json::to_string_pretty(&derived).unwrap() + "\n";
        let path = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("schema")
            .join(file);
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
            "{} differs from the derived schema at line {line}. Run \
             `UPDATE_GOLDEN=1 cargo test -p cadmpeg-ir --features schema --lib -- \
             committed_json_schemas` and commit the file.",
            path.display()
        );
    }
}
