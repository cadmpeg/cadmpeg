// SPDX-License-Identifier: Apache-2.0
//! Each nesting ceiling admits a value nested to it and refuses one level
//! past it with `ResourceLimit`.

use std::collections::BTreeMap;
use std::fmt::Debug;

use cadmpeg_core::decode::{ResourceDimension, ResourceFailure, ResourceLimits};
use cadmpeg_core::CodecError;
use serde::de::DeserializeOwned;
use serde::Serialize;

use super::expressions::Choices;
use super::fixtures::{self, container, feature, picked};
use crate::design::construction::{
    kinds, AnyConstruction, Construction, ConstructionInput, InputReference,
};
use crate::design::dimension::ParameterRef;
use crate::design::expr::{self, BinaryOp, ExprError, Function, Neutral, NeutralWire, Node};
use crate::design::names::{NameEntry, TopoName};
use crate::design::read::{self, Ceiling};
use crate::design::selection::{PickDomain, Picked};

fn policies() -> [ResourceLimits; 2] {
    [ResourceLimits::desktop(), ResourceLimits::service()]
}

/// Requires `at` to read and `past` to be refused with the ceiling's
/// `ResourceLimit`.
fn admits_and_refuses<T: Serialize + DeserializeOwned + Debug>(
    ceiling: Ceiling,
    limits: &ResourceLimits,
    at: &str,
    past: &str,
) {
    let limit = ceiling.of(limits);
    if let Err(error) = read::from_str::<T>(at, limits) {
        panic!("{ceiling:?} at its ceiling of {limit} is refused: {error}");
    }
    match read::from_str::<T>(past, limits) {
        Err(CodecError::ResourceLimit(refusal)) => {
            assert_eq!(refusal.dimension, ResourceDimension::RecursionDepth);
            assert_eq!(refusal.reason, ResourceFailure::BudgetExceeded);
            assert_eq!(refusal.limit, limit);
            assert_eq!(refusal.used, limit);
            assert_eq!(refusal.additional, 1);
        }
        other => panic!("{ceiling:?} one past its ceiling of {limit} gave {other:?}"),
    }
}

fn negations(depth: u64) -> String {
    let wire = NeutralWire::<ParameterRef> {
        text: format!("{}1", "-".repeat(usize::try_from(depth - 1).unwrap())),
        refs: Vec::new(),
        measures: Vec::new(),
    };
    serde_json::to_string(&wire).unwrap()
}

fn modified(depth: u64) -> String {
    let mut name = TopoName::Root {
        container: container(1),
    };
    for _ in 1..depth {
        name = TopoName::Modified {
            feature: feature(1),
            from: Box::new(name),
        };
    }
    serde_json::to_string(&name).unwrap()
}

fn limited(depth: u64) -> String {
    let mut pick = picked(1);
    for _ in 1..depth {
        pick = Picked {
            domain: Some(PickDomain::Limits {
                start: Some(Box::new(pick)),
                end: None,
            }),
            ..picked(2)
        };
    }
    serde_json::to_string(&pick).unwrap()
}

fn built(depth: u64) -> String {
    let empty = || Construction::<kinds::Plane> {
        inputs: Vec::new(),
        after: Vec::new(),
        solution: None,
        mode: None,
        resolved: None,
        resolved_in: BTreeMap::new(),
    };
    let mut construction = empty();
    for _ in 1..depth {
        construction = Construction {
            inputs: vec![ConstructionInput {
                role: None,
                reference: Some(InputReference::Built(Box::new(AnyConstruction::Plane(
                    construction,
                )))),
                value: None,
                reversed: None,
            }],
            ..empty()
        };
    }
    serde_json::to_string(&construction).unwrap()
}

#[test]
fn expression_text_nests_to_its_ceiling() {
    for limits in policies() {
        let limit = Ceiling::Expression.of(&limits);
        admits_and_refuses::<Neutral<ParameterRef>>(
            Ceiling::Expression,
            &limits,
            &negations(limit),
            &negations(limit + 1),
        );
    }
}

/// Flat text of `operands` operands joined by `mark`: one level of text whose
/// tree is a left-deep chain.
fn chained(operand: &str, mark: &str, operands: u64) -> String {
    let text = vec![operand; usize::try_from(operands).unwrap()].join(&format!(" {mark} "));
    serde_json::to_string(&NeutralWire::<ParameterRef> {
        text,
        refs: Vec::new(),
        measures: Vec::new(),
    })
    .unwrap()
}

const CHAINS: [(&str, &str); 6] = [
    ("1", "+"),
    ("1", "-"),
    ("1", "*"),
    ("1", "/"),
    ("true", "and"),
    ("true", "or"),
];

/// Each operand of a chain past its second nests one level, so a chain of
/// `limit + 1` operands reaches the ceiling and one more is refused.
#[test]
fn a_long_chain_nests_to_the_expression_ceiling() {
    for limits in policies() {
        let limit = Ceiling::Expression.of(&limits);
        for (operand, mark) in CHAINS {
            admits_and_refuses::<Neutral<ParameterRef>>(
                Ceiling::Expression,
                &limits,
                &chained(operand, mark, limit + 1),
                &chained(operand, mark, limit + 2),
            );
        }
    }
}

/// Runs `test` on a thread whose stack is 256 KiB.
fn on_a_small_stack(test: impl FnOnce() + Send + 'static) {
    std::thread::Builder::new()
        .stack_size(256 << 10)
        .spawn(test)
        .unwrap()
        .join()
        .unwrap();
}

/// A chain far past the ceiling is refused with `ResourceLimit` before its
/// tree is built, on a small stack.
#[test]
fn a_chain_far_past_the_ceiling_is_refused_without_overflow() {
    on_a_small_stack(|| {
        for (operand, mark) in CHAINS {
            let text = chained(operand, mark, 10_000);
            let read = read::from_str::<Neutral<ParameterRef>>(&text, &ResourceLimits::desktop());
            assert!(
                matches!(read, Err(CodecError::ResourceLimit(_))),
                "{mark}: {read:?}"
            );
        }
    });
}

/// The depth of a tree, counting its root, walked without recursion.
fn depth(node: &Node<ParameterRef>) -> u64 {
    let mut deepest = 0;
    let mut work = vec![(node, 1)];
    while let Some((node, at)) = work.pop() {
        deepest = deepest.max(at);
        match node {
            Node::Unary { operand, .. } => work.push((operand, at + 1)),
            Node::Binary { left, right, .. } => {
                work.push((left, at + 1));
                work.push((right, at + 1));
            }
            Node::Call { args, .. } => work.extend(args.iter().map(|arg| (arg, at + 1))),
            Node::If(branches) => work.extend(branches.iter().map(|branch| (branch, at + 1))),
            Node::Number { .. }
            | Node::Constant(_)
            | Node::Boolean(_)
            | Node::Text(_)
            | Node::Reference(_)
            | Node::Measure(_)
            | Node::LawParameter
            | Node::Bend(_) => {}
        }
    }
    deepest
}

/// `calls` calls nested down to `1`, each holding one operator of every
/// precedence a two-operand chain does not charge. Each call nests one level
/// and six tree nodes.
fn mixed(calls: u64) -> String {
    let mut text = "1".to_owned();
    for _ in 0..calls {
        text = format!("sin(true or true and true == 1 + 1 * {text})");
    }
    serde_json::to_string(&NeutralWire::<ParameterRef> {
        text,
        refs: Vec::new(),
        measures: Vec::new(),
    })
    .unwrap()
}

/// The mixed form at the ceiling is a tree six times as deep as the ceiling.
/// On a small stack it reads, prints, clones, compares, and drops, and one
/// call more is refused.
#[test]
fn a_mixed_expression_at_the_ceiling_reads_and_prints_on_a_small_stack() {
    on_a_small_stack(|| {
        for limits in policies() {
            let limit = Ceiling::Expression.of(&limits);
            let at = mixed(limit - 1);
            admits_and_refuses::<Neutral<ParameterRef>>(
                Ceiling::Expression,
                &limits,
                &at,
                &mixed(limit),
            );
            let tree = read::from_str::<Neutral<ParameterRef>>(&at, &limits).unwrap();
            assert_eq!(depth(&tree.0), 6 * (limit - 1) + 1);
            assert_eq!(serde_json::to_string(&tree).unwrap(), at);
            let copy = tree.clone();
            assert_eq!(copy, tree);
            drop(copy);
        }
    });
}

/// The binary operators by precedence, from the tightest to the loosest.
const LADDER: [&[BinaryOp]; 6] = [
    &[BinaryOp::Power],
    &[BinaryOp::Multiply, BinaryOp::Divide],
    &[BinaryOp::Add, BinaryOp::Subtract],
    &[
        BinaryOp::Equal,
        BinaryOp::NotEqual,
        BinaryOp::Less,
        BinaryOp::LessEqual,
        BinaryOp::Greater,
        BinaryOp::GreaterEqual,
    ],
    &[BinaryOp::And],
    &[BinaryOp::Or],
];

/// Nests `inner` one level in a call or an `if`, at a random argument, then
/// applies, each with odds of three in four, one operator of every
/// precedence a two-operand chain does not charge, from the tightest to the
/// loosest, with the nested tree on a random side and a leaf on the other.
fn wrapped(
    choices: &mut Choices,
    leaves: &[Node<ParameterRef>],
    inner: Node<ParameterRef>,
) -> Node<ParameterRef> {
    let count = 1 + choices.below(3);
    let at = choices.below(count);
    let mut args: Vec<_> = (0..count).map(|_| choices.pick(leaves)).collect();
    args[at] = inner;
    let mut node = if choices.below(4) == 0 {
        args.resize_with(3, || choices.pick(leaves));
        let [condition, then, otherwise] = <[_; 3]>::try_from(args).unwrap();
        Node::If(Box::new([condition, then, otherwise]))
    } else {
        Node::Call {
            function: choices.pick(&Function::ALL),
            args,
        }
    };
    for ops in LADDER {
        if choices.below(4) == 0 {
            continue;
        }
        let op = choices.pick(ops);
        let leaf = Box::new(choices.pick(leaves));
        let nested = Box::new(node);
        node = if op == BinaryOp::Power || choices.below(2) == 0 {
            Node::Binary {
                op,
                left: nested,
                right: leaf,
            }
        } else {
            Node::Binary {
                op,
                left: leaf,
                right: nested,
            }
        };
    }
    node
}

/// Generated trees of mixed precedence nested to the ceiling print and read
/// back to the same tree on a small stack, and one level more is refused.
#[test]
fn mixed_trees_at_the_ceiling_print_and_read_back_on_a_small_stack() {
    on_a_small_stack(|| {
        let leaves: Vec<_> = fixtures::nodes()
            .into_iter()
            .filter(|node| depth(node) == 1)
            .collect();
        let mut choices = Choices(0x00d1_f1c3);
        for limits in policies() {
            let limit = Ceiling::Expression.of(&limits);
            let mut deepest = 0;
            for _ in 0..32 {
                let mut tree = choices.pick(&leaves);
                for _ in 1..limit {
                    tree = wrapped(&mut choices, &leaves, tree);
                }
                deepest = deepest.max(depth(&tree));
                let text = serde_json::to_string(&Neutral(tree.clone())).unwrap();
                let back = read::from_str::<Neutral<ParameterRef>>(&text, &limits).unwrap();
                assert_eq!(back.0, tree, "{text}");
                let past = wrapped(&mut choices, &leaves, tree);
                let refusal = match expr::print(&past) {
                    Err(ExprError::ResourceLimit(refusal)) => refusal,
                    Ok(wire) => {
                        let text = serde_json::to_string(&wire).unwrap();
                        match read::from_str::<Neutral<ParameterRef>>(&text, &limits) {
                            Err(CodecError::ResourceLimit(refusal)) => refusal,
                            other => panic!("{text} one level past the ceiling gave {other:?}"),
                        }
                    }
                    Err(error) => panic!("{error}"),
                };
                assert_eq!(refusal.limit, limit);
            }
            assert!(deepest > 5 * limit, "the deepest tree is {deepest}");
        }
    });
}

/// The printer charges the levels the parser charges, so it writes a chain at
/// the ceiling and refuses one past it, without recursing down its spine.
#[test]
fn the_printer_refuses_a_chain_past_the_ceiling() {
    let chain = |operands: u64| {
        let one = || Node::<ParameterRef>::Boolean(true);
        let mut node = one();
        for _ in 1..operands {
            node = Node::Binary {
                op: BinaryOp::And,
                left: Box::new(node),
                right: Box::new(one()),
            };
        }
        node
    };
    let limit = Ceiling::Expression.of(&ResourceLimits::default());
    let at = expr::print(&chain(limit + 1)).unwrap();
    assert_eq!(
        at.text,
        vec!["true"; usize::try_from(limit + 1).unwrap()].join(" and ")
    );
    assert!(matches!(
        expr::print(&chain(limit + 2)),
        Err(ExprError::ResourceLimit(refusal)) if refusal.limit == limit
    ));
    assert!(serde_json::to_string(&Neutral(chain(limit + 2))).is_err());
}

#[test]
fn a_topo_name_nests_to_its_ceiling() {
    for limits in policies() {
        let limit = Ceiling::Name.of(&limits);
        admits_and_refuses::<TopoName>(
            Ceiling::Name,
            &limits,
            &modified(limit),
            &modified(limit + 1),
        );
    }
}

#[test]
fn a_picked_nests_to_its_ceiling() {
    for limits in policies() {
        let limit = Ceiling::Pick.of(&limits);
        admits_and_refuses::<Picked>(Ceiling::Pick, &limits, &limited(limit), &limited(limit + 1));
    }
}

#[test]
fn a_construction_nests_to_its_ceiling() {
    for limits in policies() {
        let limit = Ceiling::Construction.of(&limits);
        admits_and_refuses::<Construction<kinds::Plane>>(
            Ceiling::Construction,
            &limits,
            &built(limit),
            &built(limit + 1),
        );
    }
}

/// A ceiling refusal inside a larger value is still `ResourceLimit`, and a
/// read after it starts from depth zero.
#[test]
fn a_nested_refusal_surfaces_and_does_not_leak_into_the_next_read() {
    let limits = ResourceLimits::service();
    let limit = Ceiling::Name.of(&limits);
    let entry = format!("{{\"name\":{}}}", modified(limit + 1));
    assert!(matches!(
        read::from_str::<NameEntry>(&entry, &limits),
        Err(CodecError::ResourceLimit(_))
    ));
    read::from_str::<TopoName>(&modified(limit), &limits).unwrap();
    let malformed = read::from_str::<TopoName>("{\"kind\":\"root\"}", &limits);
    assert!(
        matches!(malformed, Err(CodecError::Malformed { .. })),
        "{malformed:?}"
    );
}

/// The ceilings sit below the JSON reader's own nesting limit, so each is
/// reached first.
#[test]
fn the_desktop_ceilings_are_reached_below_the_json_limit() {
    let limits = ResourceLimits::desktop();
    let past = |ceiling: Ceiling| ceiling.of(&limits) + 1;
    assert!(matches!(
        read::from_str::<TopoName>(&modified(past(Ceiling::Name)), &limits),
        Err(CodecError::ResourceLimit(_))
    ));
    assert!(matches!(
        read::from_str::<Picked>(&limited(past(Ceiling::Pick)), &limits),
        Err(CodecError::ResourceLimit(_))
    ));
    assert!(matches!(
        read::from_str::<Construction<kinds::Plane>>(&built(past(Ceiling::Construction)), &limits),
        Err(CodecError::ResourceLimit(_))
    ));
}
