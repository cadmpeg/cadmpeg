// SPDX-License-Identifier: Apache-2.0
//! The depth bounds of §2.5: an expression tree reads to 64 levels, and the
//! nesting design types read to the document's 128 levels of JSON.

use cadmpeg_core::serde_erase::json;
use std::collections::BTreeMap;
use std::fmt::Debug;

use cadmpeg_core::decode::ResourceLimits;
use cadmpeg_core::CodecError;
use serde::de::DeserializeOwned;
use serde::Serialize;

use super::expressions::{Choices, Operator};
use super::fixtures::{self, container, feature, picked};
use crate::bounds::{text_depth, MAX_DOCUMENT_DEPTH, MAX_EXPRESSION_DEPTH};
use crate::design::construction::{
    kinds, AnyConstruction, Construction, ConstructionInput, InputReference,
};
use crate::design::dimension::ParameterRef;
use crate::design::expr::{AddOp, BinaryOp, Function, MulOp, Neutral, NeutralWire, Node};
use crate::design::names::TopoName;
use crate::design::read;
use crate::design::selection::{PickDomain, Picked};

fn read_expression(text: &str) -> Result<Neutral<ParameterRef>, CodecError> {
    read::from_str(text, &ResourceLimits::desktop())
}

fn wire(text: String) -> String {
    json::to_string(&NeutralWire::<ParameterRef> {
        text,
        refs: Vec::new(),
        measures: Vec::new(),
    })
    .unwrap()
}

/// Requires the read of `text` to be refused for an expression past its bound.
fn refused_past_the_expression_bound(text: &str) {
    let read = read_expression(text);
    assert!(
        matches!(&read, Err(CodecError::Malformed(message)) if message.contains("levels of an expression")),
        "{text}: {read:?}"
    );
}

/// `depth - 1` negations of a leaf: a tree `depth` levels deep.
fn negations(depth: usize) -> String {
    wire(format!("{}1", "-".repeat(depth - 1)))
}

#[test]
fn an_expression_reads_to_its_bound() {
    let at = read_expression(&negations(MAX_EXPRESSION_DEPTH)).unwrap();
    assert_eq!(at.0.depth(), MAX_EXPRESSION_DEPTH);
    refused_past_the_expression_bound(&negations(MAX_EXPRESSION_DEPTH + 1));
}

/// Flat text of `operands` operands joined by `mark`.
fn chained(operand: &str, mark: &str, operands: usize) -> String {
    wire(vec![operand; operands].join(&format!(" {mark} ")))
}

const CHAINS: [(&str, &str); 6] = [
    ("1", "+"),
    ("1", "-"),
    ("1", "*"),
    ("1", "/"),
    ("true", "and"),
    ("true", "or"),
];

/// Runs `test` on a thread whose stack is 256 KiB.
fn on_a_small_stack(test: impl FnOnce() + Send + 'static) {
    std::thread::Builder::new()
        .stack_size(256 << 10)
        .spawn(test)
        .unwrap()
        .join()
        .unwrap();
}

/// A chain is one node, so a chain of any length is two levels deep; a long
/// one reads, prints the same text, clones, compares, and drops on a small
/// stack.
#[test]
fn a_long_chain_is_two_levels_deep() {
    on_a_small_stack(|| {
        for (operand, mark) in CHAINS {
            let text = chained(operand, mark, 10_000);
            let tree = read_expression(&text).unwrap();
            assert_eq!(tree.0.depth(), 2);
            assert_eq!(json::to_string(&tree).unwrap(), text);
            let copy = tree.clone();
            assert_eq!(copy, tree);
        }
    });
}

/// `calls` calls nested down to `1`, each holding one operator of every
/// precedence: each call adds six levels to the tree.
fn mixed(calls: usize) -> String {
    let mut text = "1".to_owned();
    for _ in 0..calls {
        text = format!("sin(true or true and true == 1 + 1 * {text})");
    }
    wire(text)
}

/// The deepest mixed form reads, prints, clones, compares, and drops on a
/// small stack, and one call more is refused.
#[test]
fn a_mixed_expression_reads_to_the_bound_on_a_small_stack() {
    on_a_small_stack(|| {
        let calls = (MAX_EXPRESSION_DEPTH - 1) / 6;
        let at = mixed(calls);
        let tree = read_expression(&at).unwrap();
        assert_eq!(tree.0.depth(), 6 * calls + 1);
        assert_eq!(json::to_string(&tree).unwrap(), at);
        let copy = tree.clone();
        assert_eq!(copy, tree);
        drop(copy);
        refused_past_the_expression_bound(&mixed(calls + 1));
    });
}

/// The binary operators by precedence, from the tightest to the loosest.
const LADDER: [&[Operator]; 6] = [
    &[Operator::Binary(BinaryOp::Power)],
    &[Operator::Mul(MulOp::Multiply), Operator::Mul(MulOp::Divide)],
    &[Operator::Add(AddOp::Add), Operator::Add(AddOp::Subtract)],
    &[
        Operator::Binary(BinaryOp::Equal),
        Operator::Binary(BinaryOp::NotEqual),
        Operator::Binary(BinaryOp::Less),
        Operator::Binary(BinaryOp::LessEqual),
        Operator::Binary(BinaryOp::Greater),
        Operator::Binary(BinaryOp::GreaterEqual),
    ],
    &[Operator::And],
    &[Operator::Or],
];

/// Nests `inner` one level in a call or an `if`, at a random argument, then
/// applies, each with odds of three in four, one operator of every
/// precedence, from the tightest to the loosest, with the nested tree on a
/// random side and a leaf on the other.
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
        Node::branch(condition, then, otherwise)
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
        let leaf = choices.pick(leaves);
        node = if op == Operator::Binary(BinaryOp::Power) || choices.below(2) == 0 {
            op.apply(node, leaf)
        } else {
            op.apply(leaf, node)
        };
    }
    node
}

/// Generated trees of mixed precedence read back to the same tree up to the
/// bound on a small stack. A deeper tree writes, and its text is refused.
#[test]
fn mixed_trees_read_back_to_the_bound_on_a_small_stack() {
    on_a_small_stack(|| {
        let leaves: Vec<_> = fixtures::nodes()
            .into_iter()
            .filter(|node| node.depth() == 1)
            .collect();
        let mut choices = Choices(0x00d1_f1c3);
        let mut deepest = 0;
        for _ in 0..32 {
            let mut tree = choices.pick(&leaves);
            while tree.depth() <= MAX_EXPRESSION_DEPTH {
                deepest = deepest.max(tree.depth());
                let text = json::to_string(&Neutral(tree.clone())).unwrap();
                let back = read_expression(&text).unwrap();
                assert_eq!(back.0, tree, "{text}");
                tree = wrapped(&mut choices, &leaves, tree);
            }
            let text = json::to_string(&Neutral(tree)).unwrap();
            refused_past_the_expression_bound(&text);
        }
        assert!(
            deepest > MAX_EXPRESSION_DEPTH - 6,
            "the deepest tree is {deepest}"
        );
    });
}

/// The deepest of `nested` that the document's depth bound admits reads,
/// writes back its bytes, clones, and compares, and the next one is refused.
fn nests_to_the_document_bound<T>(nested: impl Fn(usize) -> String)
where
    T: Serialize + DeserializeOwned + Clone + PartialEq + Debug,
{
    let limits = ResourceLimits::desktop();
    let past = (1..=MAX_DOCUMENT_DEPTH + 1)
        .find(|&depth| text_depth(&nested(depth)) > MAX_DOCUMENT_DEPTH)
        .unwrap();
    let at = nested(past - 1);
    let value = read::from_str::<T>(&at, &limits).unwrap();
    assert_eq!(json::to_string(&value).unwrap(), at);
    assert_eq!(value.clone(), value);
    let refusal = read::from_str::<T>(&nested(past), &limits);
    assert!(
        matches!(&refusal, Err(CodecError::Malformed(message)) if message.contains("levels of a CADIR document")),
        "{refusal:?}"
    );
}

fn modified(depth: usize) -> String {
    let mut name = TopoName::Root {
        container: container(1),
    };
    for _ in 1..depth {
        name = TopoName::Modified {
            feature: feature(1),
            from: Box::new(name),
        };
    }
    json::to_string(&name).unwrap()
}

fn limited(depth: usize) -> String {
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
    json::to_string(&pick).unwrap()
}

fn built(depth: usize) -> String {
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
    json::to_string(&construction).unwrap()
}

#[test]
fn a_topo_name_nests_to_the_document_bound() {
    nests_to_the_document_bound::<TopoName>(modified);
}

#[test]
fn a_picked_nests_to_the_document_bound() {
    nests_to_the_document_bound::<Picked>(limited);
}

#[test]
fn a_construction_nests_to_the_document_bound() {
    nests_to_the_document_bound::<Construction<kinds::Plane>>(built);
}
