//! Local admission at recursive formula expression boundaries.

use std::collections::BTreeMap;

#[test]
fn formula_local_unary_depth_returns_absence() {
    let bindings = BTreeMap::new();
    for (depth, accepted) in [(4, true), (129, false)] {
        let expression = format!("{}1", "+".repeat(depth));
        let result = super::super::evaluate_formula_expression(&expression, &bindings);
        assert_eq!(result.is_some(), accepted, "{depth}");
    }
}
