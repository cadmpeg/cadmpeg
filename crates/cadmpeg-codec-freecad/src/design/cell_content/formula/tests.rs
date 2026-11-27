// SPDX-License-Identifier: Apache-2.0
use std::collections::HashMap;

use cadmpeg_ir::design::units::Quantity;

use super::{parse, Kind, Node, Operator, Origin, Path, Reference, Scope, Unparsed};

const REAL: Kind = Kind::Quantity(Quantity::REAL);
const LENGTH: Kind = Kind::Quantity(Quantity::LENGTH);
const ANGLE: Kind = Kind::Quantity(Quantity::ANGLE);
const AREA: Kind = Kind::Quantity(Quantity::AREA);

/// Properties by their components joined with `.`, and one range.
#[derive(Default)]
struct Names {
    paths: HashMap<String, Kind>,
    range: Option<Vec<Kind>>,
}

impl Scope for Names {
    fn path(&mut self, path: &Path) -> Kind {
        self.paths
            .get(&path.components.join("."))
            .copied()
            .unwrap_or(Kind::Unknown)
    }

    fn range(&mut self, _from: &str, _to: &str) -> Option<Vec<Kind>> {
        self.range.clone()
    }
}

fn kind_in(text: &str, scope: &mut Names) -> Kind {
    parse(text)
        .unwrap_or_else(|unparsed| panic!("{text:?}: {unparsed:?}"))
        .kind(scope)
}

fn kind(text: &str) -> Kind {
    kind_in(text, &mut Names::default())
}

fn path(text: &str) -> Path {
    match parse(text) {
        Ok(Node::Path(path)) => path,
        other => panic!("{text:?} is no path: {other:?}"),
    }
}

#[test]
fn numbers_and_units_take_their_quantity() {
    assert_eq!(kind("2*3"), REAL);
    assert_eq!(kind("5 mm"), LENGTH);
    assert_eq!(kind("1,5 mm"), LENGTH);
    assert_eq!(kind("mm"), LENGTH);
    assert_eq!(kind("5 mm^2"), AREA);
    assert_eq!(kind("5 mm * 2 mm"), AREA);
    assert_eq!(kind("6 / mm"), Kind::Quantity(Quantity::of(-1, 0, 0, 0)));
    assert_eq!(kind("30 °"), ANGLE);
    assert_eq!(kind("1' 6\""), LENGTH);
    assert_eq!(kind("pi"), REAL);
}

#[test]
fn a_unit_of_another_dimension_is_unknown() {
    assert_eq!(kind("5 V"), Kind::Unknown);
}

#[test]
fn operators_follow_the_unit_rules() {
    assert_eq!(kind("1 mm + 2 mm"), LENGTH);
    assert_eq!(kind("1 mm + 2 °"), Kind::Unknown);
    assert_eq!(kind("(5 mm)^2"), AREA);
    assert_eq!(kind("(5 mm)^0.5"), Kind::Unknown);
    assert_eq!(kind("2^0.5"), REAL);
    assert_eq!(kind("7 mm % 2"), LENGTH);
    assert_eq!(kind("7 % 2 mm"), Kind::Unknown);
    assert_eq!(kind("1 mm < 2 mm"), Kind::Boolean);
    assert_eq!(kind("1 > 2 ? 3 mm : 4 mm"), LENGTH);
    assert_eq!(kind("1 > 2 ? 3 mm : 4"), Kind::Unknown);
}

#[test]
fn a_comparison_and_a_truth_value_are_bools() {
    let mut scope = Names::default();
    scope.paths.insert("Flag".to_owned(), Kind::Boolean);
    scope.paths.insert("A1".to_owned(), LENGTH);
    scope.paths.insert("Ratio".to_owned(), REAL);
    for text in [
        "True",
        "False",
        "Flag",
        "<<a>> == 1",
        "1 mm != 2 °",
        "<<a>> < <<b>>",
        "Flag >= 1 mm",
        "A1 > 2",
        "A1 > Ratio",
        "A1 <= sum(2; 3)",
        "Flag ? True : 1 > 2",
    ] {
        assert_eq!(kind_in(text, &mut scope), Kind::Boolean, "{text:?}");
    }
    // A bool is an int in arithmetic.
    assert_eq!(kind_in("Flag + 1", &mut scope), REAL);
    assert_eq!(kind_in("-True", &mut scope), REAL);
    assert_eq!(kind_in("Flag * 2 mm", &mut scope), LENGTH);
    // Python raises where the operands do not compare, and an operand of no
    // known kind leaves the result undetermined.
    for text in [
        "<<a>> < 1",
        "1 mm < 2 °",
        "A1 > cos(0)",
        "B9 == 1",
        "None == 1",
    ] {
        assert_eq!(kind_in(text, &mut scope), Kind::Unknown, "{text:?}");
    }
}

#[test]
fn text_times_a_number_repeats_only_for_an_int() {
    let mut scope = Names::default();
    scope.paths.insert("Flag".to_owned(), Kind::Boolean);
    scope.paths.insert("Ratio".to_owned(), REAL);
    for text in [
        "<<a>> * 2",
        "3 * <<a>>",
        "<<a>> * (1 + 2) % 2",
        "<<a>> * -2",
        "<<a>> * True",
        "<<a>> * Flag",
        "<<a>> * (1 > 2)",
        "<<a>> * count(1; 2)",
        "<<a>> * href(2)",
    ] {
        assert_eq!(kind_in(text, &mut scope), Kind::Text, "{text:?}");
    }
    for text in [
        "<<a>> * 1.5",
        "<<a>> * pi",
        "<<a>> * (4 / 2)",
        "<<a>> * Ratio",
        "<<a>> * abs(2)",
        "<<a>> * 2 mm",
    ] {
        assert_eq!(kind_in(text, &mut scope), Kind::Unknown, "{text:?}");
    }
}

#[test]
fn a_sign_binds_tighter_than_a_power() {
    let Ok(Node::Binary {
        op: Operator::Power,
        left,
        ..
    }) = parse("-2^2")
    else {
        panic!("-2^2 is no power");
    };
    assert!(matches!(*left, Node::Negate(_)));
}

#[test]
fn strings_are_text() {
    assert_eq!(kind("<<w>>"), Kind::Text);
    assert_eq!(kind("<<a>> + <<b>>"), Kind::Text);
    assert_eq!(kind("<<%s mm>> % 3"), Kind::Text);
    assert_eq!(kind("str(5 mm)"), Kind::Text);
}

#[test]
fn functions_follow_the_unit_rules() {
    assert_eq!(kind("abs(-5 mm)"), LENGTH);
    assert_eq!(kind("trunc(5.5)"), REAL);
    assert_eq!(kind("tan(30 °)"), REAL);
    assert_eq!(kind("tan(30 mm)"), Kind::Unknown);
    assert_eq!(kind("atan(1)"), ANGLE);
    assert_eq!(kind("atan2(1 mm; 2 mm)"), ANGLE);
    assert_eq!(kind("sqrt(4 mm^2)"), LENGTH);
    assert_eq!(kind("sqrt(4 mm)"), Kind::Unknown);
    assert_eq!(kind("mod(7 mm, 2)"), LENGTH);
    assert_eq!(
        kind("pow(2 mm; 3)"),
        Kind::Quantity(Quantity::of(3, 0, 0, 0))
    );
    assert_eq!(kind("hypot(3 mm; 4 mm)"), LENGTH);
    assert_eq!(kind("max(3 mm; 4 mm)"), LENGTH);
    assert_eq!(kind("max(3 mm; 4 °)"), Kind::Unknown);
    assert_eq!(kind("unknownfn(3)"), Kind::Unknown);
}

#[test]
fn a_reference_takes_the_kind_of_what_it_names() {
    let mut scope = Names::default();
    scope.paths.insert("A1".to_owned(), LENGTH);
    scope.paths.insert("Box.Length".to_owned(), LENGTH);
    assert_eq!(kind_in("abs(A1)", &mut scope), LENGTH);
    assert_eq!(kind_in("A1^2", &mut scope), AREA);
    assert_eq!(kind_in("str(A1)", &mut scope), Kind::Text);
    assert_eq!(kind_in(".A1 * 2", &mut scope), LENGTH);
    assert_eq!(kind_in("Box.Length / A1", &mut scope), REAL);
    assert_eq!(kind_in("B1 + 1", &mut scope), Kind::Unknown);
}

#[test]
fn a_range_takes_the_quantity_its_cells_share() {
    let mut scope = Names {
        range: Some(vec![LENGTH, LENGTH]),
        ..Names::default()
    };
    assert_eq!(kind_in("sum(A1:A3)", &mut scope), LENGTH);
    // An aggregate stops at a text cell of a range.
    scope.range = Some(vec![LENGTH, Kind::Text, LENGTH]);
    assert_eq!(kind_in("sum(A1:A3)", &mut scope), Kind::Unknown);
    scope.range = Some(vec![LENGTH, AREA]);
    assert_eq!(kind_in("sum(A1:A3)", &mut scope), Kind::Unknown);
    let node = parse("sum(A1:A3) + B1").unwrap();
    let references = node.references();
    assert!(matches!(references[0], Reference::Range("A1", "A3")));
    assert!(matches!(references[1], Reference::Path(path) if path.components == ["B1"]));
}

#[test]
fn a_path_records_its_object_and_property() {
    let own = path(".A1");
    assert_eq!(own.origin, Origin::Own);
    assert_eq!(own.components, ["A1"]);

    let other = path("Sheet.w");
    assert_eq!(other.origin, Origin::Named);
    assert_eq!(other.components, ["Sheet", "w"]);

    let labelled = path("<<My sheet\\>>>.w");
    assert_eq!(labelled.origin, Origin::Label);
    assert_eq!(labelled.components, ["My sheet", "w"]);

    assert_eq!(path("Doc#Sheet.w").origin, Origin::Outside);
    assert_eq!(path("Doc#<<My sheet>>.w").origin, Origin::Outside);
    assert_eq!(path("Sheet.<<Sub.>>.w").origin, Origin::Outside);
    assert_eq!(path(".<<Sub.>>.w").origin, Origin::Outside);
    assert!(path("Sheet.w[0]").indexed);
    assert_eq!(
        path("Box.Placement.Base.x").components,
        ["Box", "Placement", "Base", "x"]
    );
    assert_eq!(path("$A$1").components, ["$A$1"]);
}

#[test]
fn a_path_spans_its_text() {
    let text = "2 * Sheet.w + 1";
    let node = parse(text).unwrap();
    let [Reference::Path(path)] = node.references()[..] else {
        panic!("one path");
    };
    assert_eq!(&text[path.span.clone()], "Sheet.w");
}

#[test]
fn text_the_parser_refuses_is_unparsed() {
    for text in ["2 +", "(2", "a b", "$", "2 mm mm", "f()", "1 ? 2"] {
        assert_eq!(parse(text).err(), Some(Unparsed::Syntax), "{text:?}");
    }
    let deep = format!("{}1{}", "(".repeat(100), ")".repeat(100));
    assert_eq!(parse(&deep).err(), Some(Unparsed::Limit));
}
