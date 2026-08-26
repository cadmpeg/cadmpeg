use super::super::{evaluate_expression_graphs, Expression, ExpressionUnit};

#[test]
fn graph_scopes_equal_names_by_native_unit_label() {
    let expression =
        |id: &str, name: &str, unit: ExpressionUnit, formula: &str, value| Expression {
            id: id.into(),
            object_id: None,
            record: None,
            declaration: None,
            name: name.into(),
            parameter_index: None,
            qualifier: None,
            unit,
            expression: formula.into(),
            value,
            source_entry: "part".into(),
            source_table: "table".into(),
            source_offset: 0,
        };
    let mut expressions = vec![
        expression(
            "custom-p1",
            "p1",
            ExpressionUnit::Native("custom/unit".into()),
            "3",
            Some(3.0),
        ),
        expression(
            "custom-p2",
            "p2",
            ExpressionUnit::Native("custom/unit".into()),
            "p1 * 4",
            None,
        ),
        expression(
            "other-p1",
            "p1",
            ExpressionUnit::Native("other/unit".into()),
            "9",
            Some(9.0),
        ),
        expression(
            "other-p2",
            "p2",
            ExpressionUnit::Native("other/unit".into()),
            "p1 * 4",
            None,
        ),
        expression(
            "millimeter-p1",
            "p1",
            ExpressionUnit::Millimeter,
            "5",
            Some(5.0),
        ),
        expression(
            "millimeter-p2",
            "p2",
            ExpressionUnit::Millimeter,
            "p1 * 2",
            None,
        ),
    ];

    evaluate_expression_graphs(&mut expressions);

    assert_eq!(expressions[1].value, Some(12.0));
    assert_eq!(expressions[3].value, Some(36.0));
    assert_eq!(expressions[5].value, Some(10.0));
}
