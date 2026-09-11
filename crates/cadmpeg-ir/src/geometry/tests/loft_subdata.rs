// SPDX-License-Identifier: Apache-2.0
//! Loft subdata names its form with a tag, and the table form cannot carry the
//! fixed type-211 code or restate its own dimensions.

use crate::geometry::{LoftSubdata, LoftSubdataRow};

fn row(parameters: [f64; 2], columns: Vec<[f64; 2]>) -> LoftSubdataRow {
    LoftSubdataRow {
        parameters,
        columns,
        extra: None,
    }
}

#[test]
fn loft_subdata_names_its_form_and_refuses_a_211_table() {
    let fixed = LoftSubdata::type_211([3, 4], [0.5, 1.5]);
    let wire = serde_json::to_value(&fixed).expect("serializes");
    assert_eq!(
        wire,
        serde_json::json!({
            "form": "type211",
            "dimensions": [3, 4],
            "row": [0.5, 1.5]
        })
    );
    assert_eq!(
        serde_json::from_value::<LoftSubdata>(wire).expect("round trip"),
        fixed
    );
    assert_eq!(fixed.type_code(), 211);
    assert_eq!(fixed.row_count(), 3);
    assert_eq!(fixed.column_count(), 4);

    let table = LoftSubdata::table(
        7,
        vec![
            row([0.0, 1.0], vec![[2.0, 3.0]]),
            row([4.0, 5.0], vec![[6.0, 7.0]]),
        ],
    )
    .expect("a table of one shared column width");
    let wire = serde_json::to_value(&table).expect("serializes");
    assert_eq!(wire["form"], "table");
    assert_eq!(wire["type_code"], serde_json::json!(7));
    assert!(wire.get("row_count").is_none());
    assert!(wire.get("column_count").is_none());
    assert_eq!(
        serde_json::from_value::<LoftSubdata>(wire.clone()).expect("round trip"),
        table
    );
    assert_eq!(table.row_count(), 2);
    assert_eq!(table.column_count(), 1);

    let mut coded = wire.clone();
    coded["type_code"] = serde_json::json!(211);
    let error = serde_json::from_value::<LoftSubdata>(coded)
        .err()
        .expect("211 is not a table type code")
        .to_string();
    assert!(error.contains("211"), "{error}");

    let mut restated = wire.clone();
    restated
        .as_object_mut()
        .expect("object")
        .insert("row_count".to_string(), serde_json::json!(2));
    let error = serde_json::from_value::<LoftSubdata>(restated)
        .err()
        .expect("row_count is not a wire field")
        .to_string();
    assert!(error.contains("row_count"), "{error}");

    let mut ragged = wire;
    ragged["rows"][1]["columns"] = serde_json::json!([[6.0, 7.0], [8.0, 9.0]]);
    let error = serde_json::from_value::<LoftSubdata>(ragged)
        .err()
        .expect("rows share one column width")
        .to_string();
    assert!(error.contains("column width"), "{error}");
}
