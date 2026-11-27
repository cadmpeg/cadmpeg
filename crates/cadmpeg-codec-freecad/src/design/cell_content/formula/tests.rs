// SPDX-License-Identifier: Apache-2.0
use std::collections::HashMap;

use cadmpeg_ir::design::units::Quantity;

use super::Class::{self, Boolean, Float, Int, Object, Refused, Text, Vector};
use super::{parse, Kind, Node, Operator, Origin, Path, Reference, Scope, Unparsed, Value};

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
    fn insert(&mut self, name: &str, classes: &[Class]) {
        self.paths.insert(name.to_owned(), Value::of(classes));
    }
}

impl Scope for Names {
    fn path(&mut self, path: &Path) -> Value {
        self.paths
            .get(&path.components.join("."))
            .cloned()
            .unwrap_or_else(Value::any)
    }

    fn range(&mut self, _from: &str, _to: &str) -> Option<Vec<Value>> {
        self.range.clone()
    }

    fn assume_integer(&mut self) {
        self.assumed = true;
    }
}

fn quantity(quantity: Quantity) -> Class {
    Class::quantity(quantity)
}

fn cells(classes: &[Class]) -> Vec<Value> {
    classes.iter().map(|class| Value::of(&[*class])).collect()
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
    assert_eq!(kind("1 > 2 ? 3 mm : 4"), REAL);
}

#[test]
fn a_comparison_and_a_truth_value_are_bools() {
    let mut scope = Names::default();
    scope.insert("Flag", &[Boolean]);
    scope.insert("A1", &[quantity(Quantity::LENGTH)]);
    scope.insert("Ratio", &[Float]);
    scope.insert("Part", &[Refused]);
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
        // A property the model does not resolve may hold `None`.
        "B9 == 1",
    ] {
        assert_eq!(kind_in(text, &mut scope), Kind::Unknown, "{text:?}");
    }
}

#[test]
fn text_times_a_number_repeats_only_for_an_int() {
    let mut scope = Names::default();
    scope.insert("Flag", &[Boolean]);
    scope.insert("Ratio", &[Float]);
    scope.insert("Count", &[Int]);
    scope.insert("Whole", &[Int]);
    for text in [
        "<<a>> * 2",
        "3 * <<a>>",
        "<<a>> * 2.0",
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
        "<<a>> * true",
        "<<a>> * sum(<<b>>)",
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
        "<<a>> * (1 + 2) % 2",
        // An average of no number is NaN.
        "<<a>> * average(<<b>>)",
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
        "<<a>> * (Cell > 1 ? 2 : Cell)",
    ] {
        let mut scope = Names::default();
        scope.insert("Cell", &[Int, Float]);
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
    scope.insert("A1", &[quantity(Quantity::LENGTH)]);
    scope.insert("Cell", &[Int, Float]);
    scope.insert("Whole", &[Int]);
    scope.insert("Flag", &[Boolean]);
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
        "pow(16 mm^4; 0.25) == 1",
        "pow(16 mm; -9) == 1",
        // A property the model does not resolve may hold no number, and a
        // power of a unit by an unstored number may raise.
        "round(Shape) == 1",
        "pow(A1; Cell) == 1",
        "pow(A1; Whole) == 1",
    ] {
        assert_eq!(kind_in(text, &mut scope), Kind::Unknown, "{text:?}");
    }
    // A Boolean exponent takes zero or one and fits in the unit exponent range.
    for text in ["cos(30 °) == 1", "pow(A1; Flag) == 1", "not(5 mm) == 1"] {
        assert_eq!(kind_in(text, &mut scope), Kind::Boolean, "{text:?}");
    }
}

#[test]
fn a_property_freecad_refuses_is_refused_by_a_comparison() {
    let mut scope = Names::default();
    scope.insert("Empty", &[Refused]);
    scope.insert("Ids", &[Refused]);
    scope.insert("Proxy", &[Refused]);
    for text in ["Empty == 1", "Ids != 1", "Proxy == 1", "-Proxy == 1"] {
        assert_eq!(kind_in(text, &mut scope), Kind::Unknown, "{text:?}");
    }
}

#[test]
fn a_conditional_with_a_refused_branch_is_refused_by_a_comparison() {
    let mut scope = Names::default();
    scope.insert("Flag", &[Boolean]);
    for text in [
        "(Flag ? None : 1) == 1",
        "(Flag ? 1 : tuple(1; 2)) != 1",
        "(Flag ? cos(5 mm) : 1) == 1",
        // A condition that raises raises.
        "(cos(5 mm) > 0 ? 1 : 2) == 1",
        "<<a>> < 1 ? 2 mm : 3 mm",
    ] {
        assert_eq!(kind_in(text, &mut scope), Kind::Unknown, "{text:?}");
    }
    assert_eq!(kind_in("(Flag ? 2 : 1) == 1", &mut scope), Kind::Boolean);
}

#[test]
fn an_aggregate_passes_over_a_value_that_is_no_number() {
    let mut scope = Names::default();
    scope.insert("Part", &[Refused]);
    scope.insert("Proxy", &[Refused]);
    scope.insert("Placement", &[Class::Placement]);
    scope.insert("Base", &[Vector]);
    for (text, expected) in [
        ("sum(Part; 2 mm)", LENGTH),
        ("sum(None; tuple(1; 2); 3 mm)", LENGTH),
        ("max(<<a>>; 2)", REAL),
        ("sum(Proxy; 2 mm)", LENGTH),
        ("sum(Placement; 2 mm)", LENGTH),
        ("sum(Base; list(1; 2); 2 mm)", LENGTH),
        ("sum(-Base; 2 mm)", LENGTH),
        ("count(Placement; 2 mm; 3 °)", REAL),
        // An argument of unknown kind, and one that raises.
        ("sum(Shape; 2 mm)", Kind::Unknown),
        ("sum(1 mm + 2 °; 3 mm)", Kind::Unknown),
        ("sum(-None; 3 mm)", Kind::Unknown),
        ("count(1 mm + 2 °)", Kind::Unknown),
    ] {
        assert_eq!(kind_in(text, &mut scope), expected, "{text:?}");
    }
    // A Boolean, a `None`, or a raising cell of a range raises.
    scope.range = Some(cells(&[quantity(Quantity::LENGTH), Refused]));
    for text in ["sum(A1:A2)", "sum(A1:A2) == 1", "count(A1:A2)"] {
        assert_eq!(kind_in(text, &mut scope), Kind::Unknown, "{text:?}");
    }
}

#[test]
fn stddev_of_fewer_than_two_values_raises() {
    let mut scope = Names::default();
    for (text, expected) in [
        ("stddev(2 mm)", Kind::Unknown),
        ("stddev(2 mm; <<a>>)", Kind::Unknown),
        ("stddev(2 mm; 3 mm)", LENGTH),
        ("stddev(2 mm^4; 3 mm^4) == 1", Kind::Unknown),
    ] {
        assert_eq!(kind_in(text, &mut scope), expected, "{text:?}");
    }
    scope.range = Some(cells(&[quantity(Quantity::LENGTH)]));
    assert_eq!(kind_in("stddev(A1:A1)", &mut scope), Kind::Unknown);
    scope.range = Some(cells(&[quantity(Quantity::LENGTH); 2]));
    assert_eq!(kind_in("stddev(A1:A2)", &mut scope), LENGTH);
}

#[test]
fn a_numeric_function_raises_on_an_object_that_is_no_number() {
    let mut scope = Names::default();
    scope.insert("Placement", &[Class::Placement]);
    scope.insert("Base", &[Vector]);
    for text in [
        "cos(Placement) == 1",
        "abs(Base) == 1",
        "pow(2; Placement) == 1",
        "cos(vector(1; 2; 3)) == 1",
        "round(list(1; 2)) == 1",
        "vangle(Placement; Base) == 1",
        "Placement < 1",
    ] {
        assert_eq!(kind_in(text, &mut scope), Kind::Unknown, "{text:?}");
    }
    for text in [
        "Placement == 1",
        "Base != Placement",
        "vangle(Base; Base) < 1 °",
    ] {
        assert_eq!(kind_in(text, &mut scope), Kind::Boolean, "{text:?}");
    }
    assert_eq!(kind_in("vangle(Base; vector(1; 0; 0))", &mut scope), ANGLE);
    assert_eq!(kind_in("vlinedist(Base; Base; Base)", &mut scope), LENGTH);
}

#[test]
fn a_cell_that_raises_holds_text() {
    let node = parse("cos(5 mm)").unwrap();
    let value = node.value(&mut Names::default());
    assert_eq!(value, Value::of(&[Class::Raises]));
    assert_eq!(value.stored(), Value::of(&[Text]));
}

#[test]
fn a_quantitys_remainder_keeps_its_unit() {
    let mut scope = Names::default();
    scope.insert("Flag", &[Boolean]);
    for (text, expected) in [
        ("cos(0) % 2 mm", REAL),
        ("7 mm % 2", LENGTH),
        ("7 mm % 2 °", LENGTH),
        ("7 % 2", REAL),
        // A plain number's remainder by a quantity object raises.
        ("7 % 2 mm", Kind::Unknown),
        ("2 % cos(0)", Kind::Unknown),
        ("Flag % 2 mm", Kind::Unknown),
        // A string formats its right operand; an operand that raises raises.
        ("<<%s>> % (1 mm + 2 °)", Kind::Unknown),
        ("<<%s>> % None", Kind::Text),
        // A plain number's power by a quantity object raises.
        ("2 ^ cos(0)", Kind::Unknown),
        ("cos(0) ^ 2", REAL),
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
    assert_eq!(kind("pow(4 mm^2; 0.5)"), LENGTH);
    assert_eq!(kind("pow(16 mm^4; 0.25)"), Kind::Unknown);
    // A unit exponent of 8 or more overflows.
    assert_eq!(kind("(2 mm^4)^2"), Kind::Unknown);
    assert_eq!(kind("pow(2 mm^4; 2)"), Kind::Unknown);
    assert_eq!(kind("mod(7 mm, 2)"), LENGTH);
    assert_eq!(
        kind("pow(2 mm; 3)"),
        Kind::Quantity(Quantity::of(3, 0, 0, 0))
    );
    assert_eq!(kind("hypot(3 mm; 4 mm)"), LENGTH);
    assert_eq!(kind("max(3 mm; 4 mm)"), LENGTH);
    assert_eq!(kind("max(3 mm; 4 °)"), Kind::Unknown);
    assert!(matches!(parse("unknownfn(3)"), Err(Unparsed::Syntax)));
}

#[test]
fn formula_results_require_a_proven_operation() {
    let mut scope = Names::default();
    scope.insert("Operand", &[Object]);
    scope
        .paths
        .insert("A1".into(), Value::numeric(Int, Some(2.0)));
    for text in [
        "(1 / 0) == 1",
        "(1 % 0) == 1",
        "<<%d>> % <<bad>>",
        "<<%s %s>> % 1",
        "<<%k>> % 1",
        "address(1; 1; 5)",
        "address(1; 0)",
        "address(1; 1; 1.5)",
        "list(cos(5 mm); 1)",
        "tuple(cos(5 mm))",
        "translationm(tuple(1; <<x>>; 3))",
        "create(<<vector>>; <<bad>>; 2; 3)",
    ] {
        assert_eq!(
            parse(text).unwrap().value(&mut scope).classes(),
            &[Class::Raises],
            "{text}"
        );
    }
    for text in [
        "cos(0; 0) == 1",
        "sum(no_such_function(1); 2 mm)",
        "tuple()",
        "address(1)",
        "Vars.K",
    ] {
        assert!(matches!(parse(text), Err(Unparsed::Syntax)), "{text}");
    }
    for (text, expected) in [
        ("2 > 1 ? 4 mm : 5", LENGTH),
        ("1 == 2 ? cos(5 mm) : 4 mm", LENGTH),
        ("pow(4 mm; A1)", AREA),
        ("<<x>> * average(A1; 4)", Kind::Text),
        ("pow(4 mm; count(A1; 4))", AREA),
        ("4 A / 2 A", REAL),
        ("4 V / 2 V", REAL),
        ("2 A * 3 Ohm / 2 V", REAL),
        ("sum(list(1; 2) + list(3); 2 mm)", LENGTH),
        ("sum(create(<<vector>>); 2 mm)", LENGTH),
        (
            "vangle(create(<<vector>>; 1; 2; 3); vector(1; 0; 0))",
            ANGLE,
        ),
        (
            "vangle(vector(1; 2; 3) + vector(1; 2; 3); vector(1; 0; 0))",
            ANGLE,
        ),
        ("(vector(1; 2; 3) * vector(4; 5; 6)) + 2", REAL),
        ("sum(translationm(vector(1; 2; 3)); 2 mm)", LENGTH),
        ("sum(translationm(tuple(1; 2; 3)); 2 mm)", LENGTH),
        ("parsequant(<<2 mm>>)", LENGTH),
        ("address(1; 1; 4)", Kind::Text),
        ("(Operand + 2) == 1", Kind::Unknown),
        ("Operand == 1", Kind::Unknown),
    ] {
        assert_eq!(kind_in(text, &mut scope), expected, "{text}");
        assert!(!scope.assumed, "{text}");
    }
}

#[test]
fn a_reference_takes_the_kind_of_what_it_names() {
    let mut scope = Names::default();
    scope.insert("A1", &[quantity(Quantity::LENGTH)]);
    scope.insert("Box.Length", &[quantity(Quantity::LENGTH)]);
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
        range: Some(cells(&[quantity(Quantity::LENGTH); 2])),
        ..Names::default()
    };
    assert_eq!(kind_in("sum(A1:A3)", &mut scope), LENGTH);
    // An aggregate stops at a text cell of a range.
    scope.range = Some(cells(&[
        quantity(Quantity::LENGTH),
        Text,
        quantity(Quantity::LENGTH),
    ]));
    assert_eq!(kind_in("sum(A1:A3)", &mut scope), Kind::Unknown);
    scope.range = Some(cells(&[
        quantity(Quantity::LENGTH),
        quantity(Quantity::AREA),
    ]));
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
