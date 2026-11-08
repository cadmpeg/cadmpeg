// SPDX-License-Identifier: Apache-2.0
use serde_json::json;

use super::{
    collapsed_pointer, edit_targets, normalize_edit, scalar_edits, structural_name_findings,
};

#[test]
fn normalized_edits_require_the_named_loss_and_use_the_declared_value() {
    use crate::cross_closure::ReducedProbe;
    use cadmpeg_ir::codec::write::ReducedStorage;
    use cadmpeg_ir::report::loss::{LossKind, LossNote, LossTaxonomy};

    let pointer = "/model/tessellations/0/channels/3/flags";
    assert_eq!(
        collapsed_pointer(pointer),
        "model.tessellations[].channels[].flags"
    );
    let row = ReducedProbe {
        path: "model.tessellations[].channels[].flags",
        storage: ReducedStorage::Normalized {
            loss: "sldprt/writer.fields-omitted",
        },
        normalize: Some(|_| json!(2)),
    };
    let document = json!({"model":{"tessellations":[{"channels":[{}, {}, {}, {"flags":7}]}]}});
    let note = |message| {
        LossNote::new(
            LossKind::namespaced(
                cadmpeg_ir::loss_namespace!("sldprt"),
                "writer.fields-omitted",
                LossTaxonomy::AttributesNotTransferred,
            ),
            message,
        )
    };
    for losses in [vec![], vec![note("another record")]] {
        let mut candidate = document.clone();
        assert!(normalize_edit(&mut candidate, pointer, &row, "mesh#1", &losses).is_err());
        assert_eq!(candidate.pointer(pointer), Some(&json!(7)));
    }
    let mut candidate = document;
    normalize_edit(
        &mut candidate,
        pointer,
        &row,
        "mesh#1",
        &[note("mesh#1 flag normalized")],
    )
    .expect("declared loss permits normalization");
    assert_eq!(candidate.pointer(pointer), Some(&json!(2)));
}

#[test]
fn a_vector_row_normalizes_the_whole_array() {
    use crate::cross_closure::ReducedProbe;
    use cadmpeg_ir::codec::write::ReducedStorage;
    use cadmpeg_ir::report::loss::{LossKind, LossNote, LossTaxonomy};

    let pointer = "/model/views/0/axis/0";
    let row = ReducedProbe {
        path: "model.views[].axis[]",
        storage: ReducedStorage::Normalized {
            loss: "rhino/writer.axis-normalized",
        },
        normalize: Some(|axis| {
            let first = axis[0].as_f64().unwrap_or_default();
            let second = axis[1].as_f64().unwrap_or_default();
            let length = first.hypot(second);
            json!([first / length, second / length])
        }),
    };
    let note = LossNote::new(
        LossKind::namespaced(
            cadmpeg_ir::loss_namespace!("rhino"),
            "writer.axis-normalized",
            LossTaxonomy::AttributesNotTransferred,
        ),
        "view#1 axis normalized",
    );
    let mut candidate = json!({"model":{"views":[{"axis":[3.0, 4.0]}]}});
    normalize_edit(&mut candidate, pointer, &row, "view#1", &[note])
        .expect("declared loss permits normalization");
    assert_eq!(
        candidate.pointer("/model/views/0/axis"),
        Some(&json!([0.6, 0.8]))
    );
}

#[test]
fn dropped_native_list_member_does_not_make_the_owner_missing() {
    use crate::comparison::{unpaired_after_allowances, Allowances};
    let before = json!({
        "model": {"drawings": [{"id":"drawing", "items":[{"target":"point"},{"target":"native"}]}],
                  "points": [{"id":"point", "value":1}]},
        "native": {"step":{"drawing_targets":[{"id":"native", "source_type":"ANNOTATION_PLANE"}]}}
    });
    let after = json!({
        "model": {"drawings": [{"id":"written-drawing", "items":[{"target":"written-point"}]}],
                  "points": [{"id":"written-point", "value":1}]},
        "native": {"step":{"drawing_targets":[]}}
    });
    let allowances = Allowances::removed(std::collections::BTreeSet::from(["native".to_owned()]));
    let missing = unpaired_after_allowances(&before, &after, &allowances).expect("pair records");
    assert!(missing.contains("native"));
    assert!(!missing.contains("drawing"));
    let mut missing_owner = after.clone();
    missing_owner["model"]["drawings"] = json!([]);
    let missing =
        unpaired_after_allowances(&before, &missing_owner, &allowances).expect("pair records");
    assert!(missing.contains("drawing"));
}

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
fn record_order_is_an_ordinal_and_is_not_edited() {
    let view = json!({"id": "v", "order": 1, "visible": true, "states": [{"order": 0}]});
    assert_eq!(
        scalar_edits(&view, "view_presentations"),
        [
            ("/states/0/order".to_owned(), json!(1)),
            ("/visible".to_owned(), json!(false)),
        ]
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
