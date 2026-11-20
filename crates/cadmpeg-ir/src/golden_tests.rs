// SPDX-License-Identifier: Apache-2.0
use std::path::Path;

#[test]
fn committed_schema_matches_the_derives() {
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("schema/cadir.schema.json");
    let schema = serde_json::to_value(crate::cadir_json_schema()).expect("the schema serializes");
    let actual = format!("{schema:#}\n");
    let committed = std::fs::read_to_string(&path)
        .unwrap_or_default()
        .replace("\r\n", "\n");
    if committed == actual {
        return;
    }
    if std::env::var_os("UPDATE_GOLDEN").is_some() {
        std::fs::write(&path, actual)
            .unwrap_or_else(|error| panic!("write {}: {error}", path.display()));
        return;
    }
    panic!(
        "{} differs from `cadir_json_schema()`; regenerate with \
         `UPDATE_GOLDEN=1 cargo test -p cadmpeg-ir --features schema --lib golden` and review the diff",
        path.display()
    );
}
