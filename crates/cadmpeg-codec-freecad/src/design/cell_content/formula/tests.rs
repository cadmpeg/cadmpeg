// SPDX-License-Identifier: Apache-2.0
use std::collections::HashMap;

use cadmpeg_ir::design::units::Quantity;

use super::{
    parse, Kind, Node, Operator, Origin, Path, Python, Reference, Scope, Unparsed, Value,
    MAX_NESTING, MAX_TOKENS,
};

const REAL: Kind = Kind::Quantity(Quantity::REAL);
const LENGTH: Kind = Kind::Quantity(Quantity::LENGTH);
const ANGLE: Kind = Kind::Quantity(Quantity::ANGLE);
const AREA: Kind = Kind::Quantity(Quantity::AREA);

/// Properties by their components joined with `.`, one range, and whether a
/// formula took a number of unstated value as an `int`.
#[derive(Default)]
struct Names {
    paths: HashMap<String, Value>,
    range: Option<Vec<Value>>,
    assumed: bool,
}

impl Names {
    fn insert(&mut self, name: &str, kind: Kind, python: Python) {
        self.paths.insert(name.to_owned(), Value::new(kind, python));
    }
}

impl Scope for Names {
    fn path(&mut self, path: &Path) -> Value {
        self.paths
            .get(&path.components.join("."))
            .copied()
            .unwrap_or(Value::new(Kind::Unknown, Python::Object))
    }

    fn range(&mut self, _from: &str, _to: &str) -> Option<Vec<Value>> {
        self.range.clone()
    }

    fn assume_integer(&mut self) {
        self.assumed = true;
    }
}

fn quantities(kinds: &[Kind]) -> Vec<Value> {
    kinds
        .iter()
        .map(|kind| Value::new(*kind, Python::Object))
        .collect()
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
    scope.insert("Flag", Kind::Boolean, Python::Int);
    scope.insert("A1", LENGTH, Python::Object);
    scope.insert("Ratio", REAL, Python::Float);
    scope.insert("Part", Kind::Unknown, Python::Refused);
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
        "B9 == 1",
        "A1 == <<a>>",
        "cos(0) != Ratio",
    ] {
        assert_eq!(kind_in(text, &mut scope), Kind::Boolean, "{text:?}");
    }
    // A bool is an int in arithmetic.
    assert_eq!(kind_in("Flag + 1", &mut scope), REAL);
    assert_eq!(kind_in("-True", &mut scope), REAL);
    assert_eq!(kind_in("Flag * 2 mm", &mut scope), LENGTH);
    // Python raises where the operands do not compare, and `FreeCAD` refuses
    // `None`, a tuple, and an operand that raises.
    for text in [
        "<<a>> < 1",
        "1 mm < 2 °",
        "A1 > cos(0)",
        "None == 1",
        "Part != 1",
        "(1 mm + 2 °) == 1",
        "-<<a>> == 1",
    ] {
        assert_eq!(kind_in(text, &mut scope), Kind::Unknown, "{text:?}");
    }
}

#[test]
fn text_times_a_number_repeats_only_for_an_int() {
    let mut scope = Names::default();
    scope.insert("Flag", Kind::Boolean, Python::Int);
    scope.insert("Ratio", REAL, Python::Float);
    scope.insert("Count", REAL, Python::Int);
    scope.insert("Whole", REAL, Python::Int);
    for text in [
        "<<a>> * 2",
        "3 * <<a>>",
        "<<a>> * 2.0",
        "<<a>> * (1 + 2) % 2",
        "<<a>> * -2",
        "<<a>> * True",
        "<<a>> * Flag",
        "<<a>> * (1 > 2)",
        "<<a>> * count(1; 2)",
        "<<a>> * href(2)",
        "<<a>> * Count",
        "Whole * <<a>>",
        "<<a>> * sum(2; 3; <<b>>)",
        "<<a>> * 2^3",
    ] {
        assert_eq!(kind_in(text, &mut scope), Kind::Text, "{text:?}");
        assert!(!scope.assumed, "{text:?}");
    }
    for text in [
        "<<a>> * 1.5",
        "<<a>> * pi",
        "<<a>> * (4 / 2)",
        "<<a>> * Ratio",
        "<<a>> * (Count + Ratio)",
        "<<a>> * abs(2)",
        "<<a>> * 2 mm",
        "<<a>> * 2^-1",
    ] {
        assert_eq!(kind_in(text, &mut scope), Kind::Unknown, "{text:?}");
    }
    assert!(!scope.assumed);
}

#[test]
fn text_times_a_number_of_unstated_value_takes_it_as_an_int() {
    for text in [
        "<<a>> * Cell",
        "<<a>> * average(Cell; 3)",
        "<<a>> * max(Cell; 2)",
        "<<a>> * 2^Cell",
        "<<a>> * (Cell > 1 ? 2 : Cell)",
    ] {
        let mut scope = Names::default();
        scope.insert("Cell", REAL, Python::Unsettled);
        assert_eq!(kind_in(text, &mut scope), Kind::Text, "{text:?}");
        assert!(scope.assumed, "{text:?}");
    }
}

#[test]
fn an_aggregate_of_constants_is_its_value() {
    let mut scope = Names::default();
    // `average(2; 3)` is 2.5, a `float`, and Python refuses text times a
    // `float`.
    assert_eq!(kind_in("<<a>> * average(2; 3)", &mut scope), Kind::Unknown);
    for text in [
        "<<a>> * average(2; 4)",
        "<<a>> * sum(1.5; 1.5)",
        "<<a>> * max(1; 2.0)",
    ] {
        assert_eq!(kind_in(text, &mut scope), Kind::Text, "{text:?}");
    }
    assert!(!scope.assumed);
}

#[test]
fn a_tuple_is_refused_by_a_comparison() {
    for text in ["tuple(1; 2) == 1", "1 != tuple(1; 2)"] {
        assert_eq!(kind(text), Kind::Unknown, "{text:?}");
    }
    assert_eq!(kind("list(1; 2) == 1"), Kind::Boolean);
}

#[test]
fn a_function_that_raises_is_refused_by_a_comparison() {
    let mut scope = Names::default();
    scope.insert("A1", LENGTH, Python::Object);
    scope.insert("Cell", REAL, Python::Unsettled);
    for text in [
        "cos(5 mm) == 1",
        "sqrt(2 mm) != 1",
        "acos(1 mm) == 1",
        "cos(<<a>>) == 1",
        "abs(<<a>>) == 1",
        "atan2(1 mm; 2 °) == 1",
        "pow(2; 1 mm) == 1",
        "not(None) == 1",
        "hypot(3 mm; 4 °) == 1",
    ] {
        assert_eq!(kind_in(text, &mut scope), Kind::Unknown, "{text:?}");
    }
    // A function of a value of unknown kind, and a power of a quantity by a
    // number the file does not store, evaluate.
    for text in [
        "cos(30 °) == 1",
        "round(Shape) == 1",
        "pow(A1; Cell) == 1",
        "not(5 mm) == 1",
    ] {
        assert_eq!(kind_in(text, &mut scope), Kind::Boolean, "{text:?}");
    }
}

#[test]
fn a_property_freecad_refuses_is_refused_by_a_comparison() {
    let mut scope = Names::default();
    scope.insert("Empty", Kind::Unknown, Python::Refused);
    scope.insert("Ids", Kind::Unknown, Python::Refused);
    scope.insert("Proxy", Kind::Unknown, Python::Opaque);
    for text in ["Empty == 1", "Ids != 1", "Proxy == 1", "-Proxy == 1"] {
        assert_eq!(kind_in(text, &mut scope), Kind::Unknown, "{text:?}");
    }
}

#[test]
fn a_conditional_with_a_refused_branch_is_refused_by_a_comparison() {
    let mut scope = Names::default();
    scope.insert("Flag", Kind::Boolean, Python::Int);
    for text in [
        "(Flag ? None : 1) == 1",
        "(Flag ? 1 : tuple(1; 2)) != 1",
        "(Flag ? cos(5 mm) : 1) == 1",
    ] {
        assert_eq!(kind_in(text, &mut scope), Kind::Unknown, "{text:?}");
    }
    assert_eq!(kind_in("(Flag ? 2 : 1) == 1", &mut scope), Kind::Boolean);
}

#[test]
fn an_aggregate_passes_over_a_value_that_is_no_number() {
    let mut scope = Names::default();
    scope.insert("Part", Kind::Unknown, Python::Refused);
    scope.insert("Proxy", Kind::Unknown, Python::Opaque);
    for (text, expected) in [
        ("sum(Part; 2 mm)", LENGTH),
        ("sum(None; tuple(1; 2); 3 mm)", LENGTH),
        ("max(<<a>>; 2)", REAL),
        // An argument of unknown kind, and one that raises.
        ("sum(Shape; 2 mm)", Kind::Unknown),
        ("sum(Proxy; 2 mm)", Kind::Unknown),
        ("sum(1 mm + 2 °; 3 mm)", Kind::Unknown),
        ("sum(-None; 3 mm)", Kind::Unknown),
    ] {
        assert_eq!(kind_in(text, &mut scope), expected, "{text:?}");
    }
    // A Boolean, a `None`, or a raising cell of a range raises.
    scope.range = Some(vec![
        Value::new(LENGTH, Python::Object),
        Value::new(Kind::Unknown, Python::Refused),
    ]);
    assert_eq!(kind_in("sum(A1:A2)", &mut scope), Kind::Unknown);
    assert_eq!(kind_in("sum(A1:A2) == 1", &mut scope), Kind::Unknown);
}

#[test]
fn a_quantitys_remainder_keeps_its_unit() {
    let mut scope = Names::default();
    scope.insert("Flag", Kind::Boolean, Python::Int);
    for (text, expected) in [
        ("cos(0) % 2 mm", REAL),
        ("7 mm % 2", LENGTH),
        ("7 mm % 2 °", LENGTH),
        ("7 % 2", REAL),
        // A plain number's remainder by a quantity object raises.
        ("7 % 2 mm", Kind::Unknown),
        ("2 % cos(0)", Kind::Unknown),
        ("Flag % 2 mm", Kind::Unknown),
    ] {
        assert_eq!(kind_in(text, &mut scope), expected, "{text:?}");
    }
    assert_eq!(kind_in("(2 % cos(0)) == 1", &mut scope), Kind::Unknown);
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
    scope.insert("A1", LENGTH, Python::Object);
    scope.insert("Box.Length", LENGTH, Python::Object);
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
        range: Some(quantities(&[LENGTH, LENGTH])),
        ..Names::default()
    };
    assert_eq!(kind_in("sum(A1:A3)", &mut scope), LENGTH);
    // An aggregate stops at a text cell of a range.
    scope.range = Some(quantities(&[LENGTH, Kind::Text, LENGTH]));
    assert_eq!(kind_in("sum(A1:A3)", &mut scope), Kind::Unknown);
    scope.range = Some(quantities(&[LENGTH, AREA]));
    assert_eq!(kind_in("sum(A1:A3)", &mut scope), Kind::Unknown);
    let node = parse("sum(A1:A3) + B1").unwrap();
    let references = node.references();
    assert!(matches!(references[0], Reference::Range(range) if range.endpoints == ["A1", "A3"]));
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

#[test]
fn formula_limits_cover_unit_groups_and_token_collection() {
    let groups = "(".repeat(MAX_NESTING);
    let closes = ")".repeat(MAX_NESTING);
    assert_eq!(
        parse(&format!("1 {groups}mm{closes}")).err(),
        Some(Unparsed::Limit)
    );
    let long = format!("{}1", "1+".repeat(MAX_TOKENS / 2));
    assert_eq!(parse(&long).err(), Some(Unparsed::Limit));
}
