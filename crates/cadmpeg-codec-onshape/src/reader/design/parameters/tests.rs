// SPDX-License-Identifier: Apache-2.0
use std::collections::BTreeMap;

use cadmpeg_ir::design::expr::print;
use cadmpeg_ir::features::ParameterId;

use super::neutral;
use crate::reader::expression::Dimension;

fn text(rendered: &str, variables: &BTreeMap<&str, &ParameterId>) -> Option<String> {
    neutral(rendered, Dimension::Length, &|name| {
        variables.get(name).map(|id| (*id, Dimension::Length))
    })
    .or_else(|| neutral(rendered, Dimension::Angle, &|_| None))
    .map(|tree| print(&tree).unwrap().text)
}

#[test]
fn rendered_arithmetic_prints_as_its_canonical_tree() {
    let width = ParameterId::mint("onshape:model:parameter#width").unwrap();
    let variables = BTreeMap::from([("width", &width)]);
    for (rendered, canonical) in [
        ("20 mm", "20[mm]"),
        ("1.5 in", "1.5[in]"),
        ("(#width * 2)", "$0 * 2"),
        ("((#width * 2) + 3 mm)", "$0 * 2 + 3[mm]"),
        ("((1 mm - 2 mm) - 3 mm)", "1[mm] - 2[mm] - 3[mm]"),
        ("(1 mm - (2 mm - 3 mm))", "1[mm] - (2[mm] - 3[mm])"),
        ("(-(#width / 4))", "-($0 / 4)"),
        ("(30 deg + 0.5 rad)", "30[deg] + 0.5[rad]"),
    ] {
        assert_eq!(
            text(rendered, &variables).as_deref(),
            Some(canonical),
            "{rendered}"
        );
    }
}

#[test]
fn an_unknown_name_or_unit_has_no_tree() {
    let variables = BTreeMap::new();
    for rendered in ["width", "2 furlong", "(1 mm + 2 mm", "1 mm 2 mm"] {
        assert_eq!(text(rendered, &variables), None, "{rendered}");
    }
}
