// SPDX-License-Identifier: Apache-2.0
use serde_json::json;

use super::{edit_targets, scalar_edits, structural_name_findings};

#[test]
fn the_first_record_of_each_arena_naming_the_writer_namespace_is_edited() {
    let document = json!({
        "model": {
            "features": [{"id": "a"}, {"id": "b", "native_ref": "x:n#1"}, {"id": "c", "native_ref": "x:n#2"}],
            "points": [{"id": "p", "native_ref": "y:n#1"}],
        },
        "native": {"x": {"rows": [{"id": "x:n#1"}, {"id": "x:n#2"}]}, "y": {"rows": [{"id": "y:n#1"}]}},
    });
    assert_eq!(edit_targets(&document, "x"), [("features".to_owned(), 1)]);
}

#[test]
fn scalar_leaves_are_edited_and_projected_keys_are_not() {
    let record = json!({
        "id": "a", "ordinal": 3, "native_ref": "x", "properties": {"k": "v"},
        "name": "Pad", "depth": 2.5, "count": 4, "flag": true, "path": {"a~b": ["s"]},
    });
    let edits = scalar_edits(&record, "features");
    let expected = vec![
        ("/count".to_owned(), json!(5)),
        ("/depth".to_owned(), json!(3.5)),
        ("/flag".to_owned(), json!(false)),
        ("/name".to_owned(), json!("Padx")),
        ("/path/a~0b/0".to_owned(), json!("sx")),
    ];
    assert_eq!(edits, expected);
    let drawing = json!({"id": "d", "parameters": {"Scale": 1.0}, "name": "Sheet"});
    assert_eq!(
        scalar_edits(&drawing, "drawings"),
        [("/name".to_owned(), json!("Sheetx"))]
    );
}

#[test]
fn structural_names_must_not_name_the_application() {
    let document = json!({
        "source": {"application": "cadmpeg"},
        "model": {"attributes": [{"id": "a", "name": "CadmpegId"}, {"id": "b", "name": "Label"}]},
        "native": {"fcstd": {"objects": [{"id": "o", "type": "App::CadirMarker"}, {"id": "p", "type": "Part::Box"}]}},
    });
    let members = ["Document.xml".to_owned(), "cadmpeg.json".to_owned()];
    assert_eq!(structural_name_findings(&document, &members).len(), 3);
}
