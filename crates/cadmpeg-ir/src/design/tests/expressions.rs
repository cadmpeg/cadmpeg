// SPDX-License-Identifier: Apache-2.0
//! Canonical expression text: the specification's examples, the token rules
//! it states, and every node variant through the `Neutral` wire form.

use super::fixtures::{self, parameter_ref};
use super::round_trip;
use crate::design::dimension::{ParameterRef, RefTarget};
use crate::design::expr::{
    self, AddOp, BendVariable, BinaryOp, Constant, Expression, Function, MulOp, Neutral,
    NeutralWire, Node, UnaryOp,
};
use crate::design::units::UnitSymbol;
use crate::scalar::NonNegativeReal;

/// "Every example of a neutral tree in this document, read with its operand
/// letters as references and `u` as a unit symbol, parses and prints to
/// itself." The left column is the specification's text; the right is that
/// text with letters as `$i` in order of first use, `u` as `mm`, and the
/// elided `…` as `14[mm]`.
const EXAMPLES: &[(&str, &str)] = &[
    ("$0 * 2 + 10[mm]", "$0 * 2 + 10[mm]"),
    ("10[deg] + 30[arcmin]", "10[deg] + 30[arcmin]"),
    ("if(c, a, b)", "if($0, $1, $2)"),
    ("atan2(y, x)", "atan2($0, $1)"),
    ("mod(a, b)", "mod($0, $1)"),
    ("a - trunc(a / b) * b", "$0 - trunc($0 / $1) * $1"),
    ("-2^2", "-2^2"),
    ("2^3^2", "2^3^2"),
    ("if(b, 1, 0)", "if($0, 1, 0)"),
    ("exp(1)", "exp(1)"),
    ("1[min]", "1[min]"),
    ("78.25[in/min]", "78.25[in/min]"),
    ("x^2", "$0^2"),
    ("x * 1[deg]", "$0 * 1[deg]"),
    ("x * 1[rad]", "$0 * 1[rad]"),
    ("atan(x) / 1[deg]", "atan($0) / 1[deg]"),
    ("if(y >= 0, abs(x), -abs(x))", "if($0 >= 0, abs($1), -abs($1))"),
    ("floor(x + 1e-9)", "floor($0 + 1e-9)"),
    ("ceil(x - 1e-9)", "ceil($0 - 1e-9)"),
    ("floor(x / 1[u] + 1e-9) * 1[u]", "floor($0 / 1[mm] + 1e-9) * 1[mm]"),
    ("ceil(x / 1[u] - 1e-9) * 1[u]", "ceil($0 / 1[mm] - 1e-9) * 1[mm]"),
    ("round(x / 1[mm]) * 1[mm]", "round($0 / 1[mm]) * 1[mm]"),
    ("floor(x / 1[mm]) * 1[mm]", "floor($0 / 1[mm]) * 1[mm]"),
    ("ceil(x / 1[mm]) * 1[mm]", "ceil($0 / 1[mm]) * 1[mm]"),
    ("trunc(x / 1[mm]) * 1[mm]", "trunc($0 / 1[mm]) * 1[mm]"),
    ("ceil(x / s) * s", "ceil($0 / $1) * $1"),
    ("floor(x / s) * s", "floor($0 / $1) * $1"),
    ("a^b", "$0^$1"),
    ("W * 1[mm]", "$0 * 1[mm]"),
    ("(exp(x) - exp(-x)) / 2", "(exp($0) - exp(-$0)) / 2"),
    ("(exp(x) + exp(-x)) / 2", "(exp($0) + exp(-$0)) / 2"),
    ("ln(x + sqrt(x^2 + 1))", "ln($0 + sqrt($0^2 + 1))"),
    ("ln(x + sqrt(x^2 - 1))", "ln($0 + sqrt($0^2 - 1))"),
    ("ln((1 + x) / (1 - x)) / 2", "ln((1 + $0) / (1 - $0)) / 2"),
    ("sqrt(a^2 + b^2)", "sqrt($0^2 + $1^2)"),
    ("sqrt(a^2 - b^2)", "sqrt($0^2 - $1^2)"),
    (
        "if(x >= 0, x^(1 / 3), -(-x)^(1 / 3))",
        "if($0 >= 0, $0^(1 / 3), -(-$0)^(1 / 3))",
    ),
    ("log10(x)", "log10($0)"),
    ("ln(x) / ln(10)", "ln($0) / ln(10)"),
    ("-$0", "-$0"),
    ("not $0", "not $0"),
    (
        "if($0 == \"A\", 10[mm], if($0 == \"B\", 12[mm], …))",
        "if($0 == \"A\", 10[mm], if($0 == \"B\", 12[mm], 14[mm]))",
    ),
    ("sign(0)", "sign(0)"),
    ("if(x > 0[u], 1, 0)", "if($0 > 0[mm], 1, 0)"),
    ("if(x >= 0[u], 1, 0)", "if($0 >= 0[mm], 1, 0)"),
    ("a - floor(a / b) * b", "$0 - floor($0 / $1) * $1"),
    ("$0 * 1[mm] - 2[mm]", "$0 * 1[mm] - 2[mm]"),
    ("10000[rev/min]", "10000[rev/min]"),
    ("$0 / ($1 * $2) * 1[rev]", "$0 / ($1 * $2) * 1[rev]"),
    (
        "if(trunc(n) == 0, \"custom\", if(trunc(n) == 1, \"edge\", if(trunc(n) == 2, \"normal\", \"\")))",
        "if(trunc($0) == 0, \"custom\", if(trunc($0) == 1, \"edge\", if(trunc($0) == 2, \"normal\", \"\")))",
    ),
];

fn refs(count: u32) -> Vec<ParameterRef> {
    (0..count).map(parameter_ref).collect()
}

fn wire(text: &str) -> NeutralWire<ParameterRef> {
    let count = (0..10u32)
        .filter(|index| text.contains(&format!("${index}")))
        .count();
    NeutralWire {
        text: text.to_owned(),
        refs: refs(u32::try_from(count).unwrap()),
        measures: Vec::new(),
    }
}

fn read(text: &str) -> Result<Node<ParameterRef>, expr::ExprError> {
    expr::read(&wire(text))
}

#[test]
fn every_specification_example_parses_and_prints_to_itself() {
    let failures = EXAMPLES
        .iter()
        .filter_map(|(spec, canon)| {
            read(canon)
                .err()
                .map(|error| format!("{spec:?} as {canon:?}: {error}"))
        })
        .collect::<Vec<_>>();
    assert!(failures.is_empty(), "{failures:#?}");
}

#[test]
fn the_unit_spellings_of_the_literals_bullet_read_as_units() {
    for unit in ["in/min", "mm/s^2", "m^2"] {
        let text = format!("1[{unit}]");
        read(&text).unwrap_or_else(|error| panic!("{text}: {error}"));
    }
}

#[test]
fn the_token_rules_of_the_specification_hold() {
    let exp = read("exp(1)").unwrap();
    assert!(
        matches!(
            exp,
            Node::Call {
                function: Function::Exp,
                ..
            }
        ),
        "`exp` is one word"
    );
    let min = read("1[min]").unwrap();
    assert!(matches!(&min, Node::Number { unit, .. } if unit.0[0].symbol == UnitSymbol::Min));
    assert!(matches!(read("e").unwrap(), Node::Constant(Constant::E)));
    assert!(matches!(
        read("tan(t)").unwrap(),
        Node::Call {
            function: Function::Tan,
            ..
        }
    ));
    for (word, variable) in [
        ("bendangle", BendVariable::Angle),
        ("bendradius", BendVariable::InsideRadius),
        ("bendthickness", BendVariable::Thickness),
    ] {
        assert!(
            matches!(read(word).unwrap(), Node::Bend(v) if v == variable),
            "`{word}` is a keyword"
        );
    }
    for malformed in [
        "xp(1)",
        "1[mi n]",
        "ex",
        "2e",
        "$0*2",
        "a ^ b",
        "$01",
        "1[mm*s^-2]",
        "(1)",
        "bend",
    ] {
        assert!(
            read(malformed).is_err(),
            "{malformed:?} is not canonical text"
        );
    }
}

#[test]
fn refs_are_stated_once_in_order_of_first_use() {
    let two = refs(2);
    let swapped = NeutralWire {
        text: "$1 + $0".into(),
        refs: two.clone(),
        measures: vec![],
    };
    assert!(expr::read(&swapped).is_err(), "out of first-use order");
    let unused = NeutralWire {
        text: "$0".into(),
        refs: two.clone(),
        measures: vec![],
    };
    assert!(expr::read(&unused).is_err(), "an unused ref");
    let repeated = NeutralWire {
        text: "$0 + $1".into(),
        refs: vec![two[0].clone(), two[0].clone()],
        measures: vec![],
    };
    assert!(expr::read(&repeated).is_err(), "a ref stated twice");
}

/// Every node variant, alone and as an operand of every operator, as
/// `Expression::Neutral`, written and read back through the text.
#[test]
fn every_node_variant_round_trips_as_neutral_text() {
    let leaves = fixtures::nodes();
    let mut trees = leaves.clone();
    for op in [UnaryOp::Negate, UnaryOp::Not] {
        trees.extend(leaves.iter().map(|operand| Node::Unary {
            op,
            operand: Box::new(operand.clone()),
        }));
    }
    for op in Operator::ALL {
        for left in &leaves {
            for right in [&leaves[9], &leaves[11], &leaves[5]] {
                trees.push(op.apply(left.clone(), right.clone()));
            }
        }
    }
    let failures = trees
        .into_iter()
        .filter_map(|node| {
            let expression = Expression::Neutral(Neutral(node.clone()));
            round_trip(&expression)
                .err()
                .map(|error| format!("{node:?}: {error}"))
        })
        .collect::<Vec<_>>();
    assert!(failures.is_empty(), "{failures:#?}");
}

/// Every two-operand operator of canonical text: `^` and the comparisons as
/// a `Binary`, and the four chain levels as one more operand of a run.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(super) enum Operator {
    Binary(BinaryOp),
    Add(AddOp),
    Mul(MulOp),
    And,
    Or,
}

impl Operator {
    pub(super) const ALL: [Self; 13] = [
        Self::Add(AddOp::Add),
        Self::Add(AddOp::Subtract),
        Self::Mul(MulOp::Multiply),
        Self::Mul(MulOp::Divide),
        Self::Binary(BinaryOp::Power),
        Self::Binary(BinaryOp::Equal),
        Self::Binary(BinaryOp::NotEqual),
        Self::Binary(BinaryOp::Less),
        Self::Binary(BinaryOp::LessEqual),
        Self::Binary(BinaryOp::Greater),
        Self::Binary(BinaryOp::GreaterEqual),
        Self::And,
        Self::Or,
    ];

    /// `left` and `right` joined by the operator, in the one tree the text
    /// of the two reads as.
    pub(super) fn apply<R>(self, left: Node<R>, right: Node<R>) -> Node<R> {
        match self {
            Self::Binary(op) => Node::binary(op, left, right),
            Self::Add(op) => Node::add(left, op, right),
            Self::Mul(op) => Node::mul(left, op, right),
            Self::And => Node::and(left, right),
            Self::Or => Node::or(left, right),
        }
    }
}

/// A fixed sequence of choices, so the trees are the same on every run.
pub(super) struct Choices(pub(super) u64);

impl Choices {
    pub(super) fn below(&mut self, count: usize) -> usize {
        self.0 = self
            .0
            .wrapping_mul(6_364_136_223_846_793_005)
            .wrapping_add(1_442_695_040_888_963_407);
        usize::try_from(self.0 >> 33).unwrap() % count
    }

    pub(super) fn pick<T: Clone>(&mut self, from: &[T]) -> T {
        from[self.below(from.len())].clone()
    }
}

/// A tree of every node variant, operator, and function, nested to `depth`,
/// whose calls take from zero to three arguments.
fn tree(choices: &mut Choices, leaves: &[Node<ParameterRef>], depth: u32) -> Node<ParameterRef> {
    if depth == 0 || choices.below(4) == 0 {
        return choices.pick(leaves);
    }
    let kind = choices.below(6);
    let mut operand = || Box::new(tree(choices, leaves, depth - 1));
    match kind {
        0 => Node::Unary {
            op: UnaryOp::Negate,
            operand: operand(),
        },
        1 => Node::Unary {
            op: UnaryOp::Not,
            operand: operand(),
        },
        2 | 3 => {
            let left = operand();
            let right = operand();
            choices.pick(&Operator::ALL).apply(*left, *right)
        }
        4 => {
            let count = choices.below(4);
            let args = (0..count)
                .map(|_| tree(choices, leaves, depth - 1))
                .collect();
            Node::Call {
                function: choices.pick(&Function::ALL),
                args,
            }
        }
        _ => {
            let (condition, then, otherwise) = (operand(), operand(), operand());
            Node::branch(*condition, *then, *otherwise)
        }
    }
}

fn holds_a_call_with_no_argument(node: &Node<ParameterRef>) -> bool {
    match node {
        Node::Call { args, .. } => {
            args.is_empty() || args.iter().any(holds_a_call_with_no_argument)
        }
        Node::Unary { .. }
        | Node::Binary { .. }
        | Node::Sum { .. }
        | Node::Product { .. }
        | Node::All(_)
        | Node::Any(_)
        | Node::If { .. } => node.operands().any(holds_a_call_with_no_argument),
        Node::Number { .. }
        | Node::Constant(_)
        | Node::Boolean(_)
        | Node::Text(_)
        | Node::Reference(_)
        | Node::Measure(_)
        | Node::LawParameter
        | Node::Bend(_) => false,
    }
}

/// Every tree the printer writes reads back as the same tree, and the one it
/// refuses holds a call with no argument, which the grammar cannot spell.
#[test]
fn every_printed_expression_reads_back() {
    let leaves = fixtures::nodes();
    let mut choices = Choices(0x00d1_f1c5);
    let (mut printed, mut refused) = (0, 0);
    for _ in 0..2_000 {
        let node = tree(&mut choices, &leaves, 5);
        match expr::print(&node) {
            Ok(wire) => {
                assert_eq!(expr::read(&wire).as_ref(), Ok(&node), "{}", wire.text);
                round_trip(&Neutral(node)).unwrap();
                printed += 1;
            }
            Err(expr::ExprError::Malformed(error)) => {
                assert!(holds_a_call_with_no_argument(&node), "{node:?}: {error}");
                refused += 1;
            }
            Err(error) => panic!("{node:?}: {error}"),
        }
    }
    assert!(
        printed > 1_000 && refused > 10,
        "{printed} printed, {refused} refused"
    );
}

/// The grammar calls a function with at least one argument, so the printer
/// refuses a call with none rather than write `f()`, which does not read.
#[test]
fn a_call_with_no_argument_is_refused_by_the_printer() {
    let call = Node::<ParameterRef>::Call {
        function: Function::Sign,
        args: Vec::new(),
    };
    assert!(matches!(
        expr::print(&call),
        Err(expr::ExprError::Malformed(error)) if error.contains("no argument")
    ));
    assert!(serde_json::to_string(&Neutral(call)).is_err());
    assert!(read("sign()").is_err());
}

/// A number has no sign of its own, so `Number.value` is a
/// `NonNegativeReal`, and a negative literal is `Unary { Negate }` over a
/// `Number`, which prints back to itself.
#[test]
fn a_negative_literal_is_a_negated_number() {
    assert!(NonNegativeReal::new(-2.25).is_none());
    let read_back = read("-2.25").unwrap();
    assert!(
        matches!(&read_back, Node::Unary { op: UnaryOp::Negate, operand }
        if matches!(operand.as_ref(), Node::Number { value, .. } if value.get() == 2.25))
    );
    round_trip(&Expression::Neutral(Neutral(read_back))).unwrap();
}

#[test]
fn numbers_print_in_the_ecmascript_form() {
    let cases = [
        (10.0, "10"),
        (0.375, "0.375"),
        (1e-9, "1e-9"),
        (1e21, "1e+21"),
        (123_456.5, "123456.5"),
        (1e-6, "0.000001"),
        (1.5e-7, "1.5e-7"),
        (2.5e20, "250000000000000000000"),
        (0.0, "0"),
        (999_999_999_999_999_900_000.0, "999999999999999900000"),
    ];
    for (value, text) in cases {
        assert_eq!(expr::format_number(value).unwrap(), text);
    }
    let below = f64::from_bits(1e-6f64.to_bits() - 1);
    let text = expr::format_number(below).unwrap();
    assert!(
        text.starts_with("9.") && text.ends_with("e-7"),
        "the double below 1e-6 is {text}"
    );
    assert_eq!(text.parse::<f64>().unwrap(), below);
    assert!(expr::format_number(-1.0).is_err());
    assert!(expr::format_number(f64::NAN).is_err());
}

/// The quantity exponents (length, mass, angle, time) of a unit symbol.
const fn symbol_quantity(symbol: UnitSymbol) -> [i32; 4] {
    match symbol {
        UnitSymbol::Nm
        | UnitSymbol::Um
        | UnitSymbol::Mm
        | UnitSymbol::Cm
        | UnitSymbol::M
        | UnitSymbol::Km
        | UnitSymbol::In
        | UnitSymbol::Ft
        | UnitSymbol::Yd
        | UnitSymbol::Mi => [1, 0, 0, 0],
        UnitSymbol::G | UnitSymbol::Kg | UnitSymbol::Lb => [0, 1, 0, 0],
        UnitSymbol::Rad
        | UnitSymbol::Deg
        | UnitSymbol::Grad
        | UnitSymbol::Arcmin
        | UnitSymbol::Arcsec
        | UnitSymbol::Rev => [0, 0, 1, 0],
        UnitSymbol::S | UnitSymbol::Min | UnitSymbol::H => [0, 0, 0, 1],
    }
}

/// The quantity of a product-and-quotient tree, by the `* /` row of the
/// semantics table.
fn quantity(node: &Node<ParameterRef>, refs: &[[i32; 4]]) -> [i32; 4] {
    let combine = |a: [i32; 4], b: [i32; 4], sign: i32| [0, 1, 2, 3].map(|i| a[i] + sign * b[i]);
    match node {
        Node::Number { unit, .. } => unit.0.iter().fold([0; 4], |q, factor| {
            combine(
                q,
                symbol_quantity(factor.symbol),
                i32::from(factor.power.get()),
            )
        }),
        Node::Reference(reference) => {
            let RefTarget::Parameter(id) = &reference.target else {
                panic!("a local reference")
            };
            let index: usize = id.as_str().rsplit('#').next().unwrap().parse().unwrap();
            refs[index]
        }
        Node::Product { first, rest } => {
            rest.iter()
                .fold(quantity(first, refs), |held, (op, operand)| {
                    let sign = if *op == MulOp::Multiply { 1 } else { -1 };
                    combine(held, quantity(operand, refs), sign)
                })
        }
        other => panic!("not a product or quotient: {other:?}"),
    }
}

/// `rev` is an angle, so a spindle speed in `rev/min` is an `AngularSpeed`,
/// and the feed-per-tooth tree, a feed over an angular speed and a flute
/// count, times `1[rev]`, is a `Length`.
#[test]
fn the_feed_per_tooth_example_type_checks() {
    let feed = [1, 0, 0, -1];
    let angular_speed = [0, 0, 1, -1];
    let count = [0, 0, 0, 0];
    let speed = read("10000[rev/min]").unwrap();
    assert_eq!(
        quantity(&speed, &[]),
        angular_speed,
        "rev/min is an angular speed"
    );
    let per_tooth = read("$0 / ($1 * $2) * 1[rev]").unwrap();
    assert_eq!(
        quantity(&per_tooth, &[feed, angular_speed, count]),
        [1, 0, 0, 0],
        "a length per tooth"
    );
}

/// "A chain is a run of one left-associative level of the grammar … held as
/// one node with its operands in source order", and "an `if` that is the
/// last operand of an `if` is a further case of it".
#[test]
fn a_run_of_one_level_is_one_node_and_an_if_in_last_place_is_a_further_case() {
    let leaf = |index| Node::Reference(parameter_ref(index));
    assert_eq!(
        read("$0 + $1 - $2").unwrap(),
        Node::Sum {
            first: Box::new(leaf(0)),
            rest: vec![(AddOp::Add, leaf(1)), (AddOp::Subtract, leaf(2))],
        }
    );
    assert_eq!(
        read("$0 * $1 / $2").unwrap(),
        Node::Product {
            first: Box::new(leaf(0)),
            rest: vec![(MulOp::Multiply, leaf(1)), (MulOp::Divide, leaf(2))],
        }
    );
    assert_eq!(
        read("$0 and $1 and $2").unwrap(),
        Node::All(vec![leaf(0), leaf(1), leaf(2)])
    );
    assert_eq!(
        read("$0 or $1 or $2").unwrap(),
        Node::Any(vec![leaf(0), leaf(1), leaf(2)])
    );
    assert_eq!(
        read("$0 - ($1 + $2)").unwrap(),
        Node::Sum {
            first: Box::new(leaf(0)),
            rest: vec![(
                AddOp::Subtract,
                Node::Sum {
                    first: Box::new(leaf(1)),
                    rest: vec![(AddOp::Add, leaf(2))],
                },
            )],
        }
    );
    assert!(read("($0 + $1) + $2").is_err());
    let ladder = Node::If {
        cases: vec![(leaf(0), leaf(1)), (leaf(2), leaf(3))],
        otherwise: Box::new(leaf(4)),
    };
    assert_eq!(read("if($0, $1, if($2, $3, $4))").unwrap(), ladder);
    assert_eq!(
        expr::print(&ladder).unwrap().text,
        "if($0, $1, if($2, $3, $4))"
    );
    assert_eq!(
        Node::branch(leaf(0), leaf(1), Node::branch(leaf(2), leaf(3), leaf(4))),
        ladder
    );
    let nested = read("if($0, if($1, $2, $3), $4)").unwrap();
    let Node::If { cases, otherwise } = &nested else {
        panic!("an if: {nested:?}");
    };
    assert_eq!(cases.len(), 1);
    assert_eq!(**otherwise, leaf(4));
}

/// "Its conditions are tested in order; the value of the first that is true
/// is evaluated, or `otherwise` where none is, and nothing else."
#[test]
fn a_ladder_evaluates_the_value_of_its_first_true_condition_only() {
    use crate::design::edit::{evaluate, EvalError, Value};

    let ladder = read("if($0, 1, if($1, 2, 3 / 0))").unwrap();
    let run = |first: bool, second: bool| {
        evaluate(&ladder, &mut |reference: &ParameterRef| {
            Some(Value::Boolean(if *reference == parameter_ref(0) {
                first
            } else {
                second
            }))
        })
    };
    let number = |value: f64| Value::Number {
        value,
        quantity: crate::design::units::Quantity::REAL,
    };
    assert_eq!(run(true, true), Ok(number(1.0)));
    assert_eq!(run(true, false), Ok(number(1.0)));
    assert_eq!(run(false, true), Ok(number(2.0)));
    assert_eq!(
        run(false, false),
        Err(EvalError::Domain("division by zero"))
    );
}
