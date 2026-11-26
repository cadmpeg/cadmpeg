// SPDX-License-Identifier: Apache-2.0
//! A design written past a bound, or whose scope is given up, keeps no
//! record whose scope or container is gone (§3.3, §3.4, §5.1, §5.2).

use std::collections::{BTreeMap, BTreeSet};

use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy};

use crate::bounds::MAX_EXPRESSION_DEPTH;
use crate::codec::write::target::TargetRequest;
use crate::codec::write::{CadirEncoder, EncodeInput, Encoder};
use crate::design::configuration::ParameterOverride;
use crate::design::expr::{Expression, Neutral, Node, UnaryOp};
use crate::design::scope::{DesignScope, OccurrenceDesign};
use crate::design::units::ValueKind;
use crate::features::ParameterId;
use crate::give_up::{drop_records, Arena, Seed};
use crate::ids::{OccurrenceId, ProductDefinitionId};
use crate::products::{Occurrence, ProductDefinition, ProductDefinitionKind, Storage};
use crate::report::Severity;
use crate::validate::design::fixture::{document, id, parameter};
use crate::validate::validate_neutral;
use crate::{CadIr, DecodeResult};

/// Each Error finding of `ir`, by rule and record.
fn errors(ir: &CadIr) -> BTreeSet<(String, Option<String>)> {
    validate_neutral(ir, Vec::new())
        .expect("validation runs")
        .findings
        .into_iter()
        .filter(|finding| finding.severity >= Severity::Error)
        .map(|finding| (finding.rule.as_str().to_owned(), finding.entity))
        .collect()
}

/// `depth - 1` negations of `true`: a tree `depth` levels deep.
fn negations(depth: usize) -> Expression {
    Expression::Neutral(Neutral((1..depth).fold(
        Node::Boolean(true),
        |operand, _| Node::Unary {
            op: UnaryOp::Not,
            operand: Box::new(operand),
        },
    )))
}

/// The design fixture, whose part also places `bolt`, a part whose scope
/// holds parameter `length`. The part's scope overrides `length` in the
/// placed bolt with `expression`.
fn placing_a_bolt(expression: Expression) -> CadIr {
    let mut ir = document();
    let part: ProductDefinitionId = id("definition", "part");
    let bolt: ProductDefinitionId = id("definition", "bolt");
    let occurrence: OccurrenceId = id("occurrence", "bolt");
    ir.model.product_definitions.push(ProductDefinition {
        kind: Some(ProductDefinitionKind::Part),
        ..ProductDefinition::new(bolt.clone(), Storage::Embedded)
    });
    ir.model
        .product_definitions
        .iter_mut()
        .find(|definition| definition.id == part)
        .expect("the fixture's part")
        .occurrences
        .push(occurrence.clone());
    ir.model
        .occurrences
        .push(Occurrence::new(occurrence.clone(), bolt.clone(), None));
    let mut length = parameter("length", ValueKind::Boolean);
    length.scope = bolt.clone();
    let length_id: ParameterId = length.id.clone();
    ir.design.parameters.push(length);
    ir.design.scopes.push(DesignScope {
        definition: bolt,
        roots: Vec::new(),
        active_configuration: None,
        active_arrangements: BTreeSet::new(),
        occurrences: BTreeMap::new(),
        current: Vec::new(),
        unproduced: BTreeSet::new(),
    });
    ir.design
        .scopes
        .iter_mut()
        .find(|scope| scope.definition == part)
        .expect("the fixture's scope")
        .occurrences
        .insert(
            occurrence,
            OccurrenceDesign {
                configuration: None,
                arrangements: Vec::new(),
                grounded: None,
                flexible: None,
                parameters: BTreeMap::from([(
                    length_id,
                    ParameterOverride {
                        expression: Some(expression),
                        value: None,
                    },
                )]),
            },
        );
    ir.finalize();
    ir
}

/// A scope can pass only the bound of an expression. The CADIR writer
/// states such an expression `Source` and keeps the scope, so each record
/// of the scope keeps its scope and container.
#[test]
fn the_cadir_writer_keeps_a_scope_that_holds_an_expression_past_its_bound() {
    let ir = placing_a_bolt(negations(MAX_EXPRESSION_DEPTH + 1));
    let part: ProductDefinitionId = id("definition", "part");
    let before = errors(&ir);

    let plan = CadirEncoder
        .plan(EncodeInput::new(&ir, None), TargetRequest::Unnamed)
        .expect("an expression past the bound does not refuse the write");
    let unwritable = plan
        .report()
        .losses
        .iter()
        .filter(|loss| loss.code.local_code() == "value.unwritable")
        .collect::<Vec<_>>();
    let [loss] = unwritable.as_slice() else {
        panic!("one loss for the one restated scope: {unwritable:?}");
    };
    assert!(
        loss.message.contains(part.as_str()) && loss.message.contains("design.scopes"),
        "{}",
        loss.message
    );
    let mut encoded = Vec::new();
    plan.write_to(&mut encoded).expect("the write completes");
    let written = CadIr::from_json(std::str::from_utf8(&encoded).expect("UTF-8"))
        .expect("the written document reads");

    assert_eq!(written.design.scopes.len(), ir.design.scopes.len());
    assert_eq!(written.design.containers, ir.design.containers);
    assert_eq!(written.design.features, ir.design.features);
    assert_eq!(written.design.configurations, ir.design.configurations);
    assert_eq!(written.design.evaluations, ir.design.evaluations);
    let scope = written
        .design
        .scopes
        .iter()
        .find(|scope| scope.definition == part)
        .expect("the part's scope stays");
    let overrides: Vec<_> = scope
        .occurrences
        .values()
        .flat_map(|occurrence| occurrence.parameters.values())
        .collect();
    let [only] = overrides.as_slice() else {
        panic!("one override: {overrides:?}");
    };
    assert!(
        matches!(&only.expression, Some(Expression::Source { tokens, .. }) if tokens.is_empty()),
        "{:?}",
        only.expression
    );
    let after = errors(&written);
    assert!(
        after.is_subset(&before),
        "new errors: {:?}",
        after.difference(&before).collect::<Vec<_>>()
    );
}

/// The decode wrapper states `Source` an expression past its bound as the
/// CADIR writer does, and keeps the scope that holds it (§3.3).
#[test]
fn the_decode_wrapper_keeps_a_scope_that_holds_an_expression_past_its_bound() {
    use crate::codec::{DecodeBody, Decoded, FormatId};
    use crate::report::decode::DecodeTransfer;
    use crate::report::strictness::Strictness;
    use crate::source_fidelity::SourceFidelity;

    let ir = placing_a_bolt(negations(MAX_EXPRESSION_DEPTH + 1));
    let part: ProductDefinitionId = id("definition", "part");
    let arena = DecodeArena::new();
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &DecodePolicy::default())
        .expect("a decode context");
    let decoded = Decoded {
        ir: ir.clone(),
        body: DecodeBody::new(DecodeTransfer::full(true)),
        source_fidelity: SourceFidelity::new(crate::hash::digest::Sha256Digest::digest(
            b"test source",
        )),
    };
    let result =
        DecodeResult::from_decoded(&ctx, decoded, FormatId::new("test"), &Strictness::default())
            .expect("an expression past the bound does not refuse the decode");

    let undecoded = result
        .report()
        .losses
        .iter()
        .filter(|loss| loss.code == crate::unknown::part::record_undecoded_kind())
        .collect::<Vec<_>>();
    let [loss] = undecoded.as_slice() else {
        panic!("one loss for the one restated scope: {undecoded:?}");
    };
    assert_eq!(loss.severity, Severity::Warning);
    assert!(
        loss.message.contains(part.as_str()) && loss.message.contains("design.scopes"),
        "{}",
        loss.message
    );
    assert!(result.source_fidelity().content_digest.is_some());
    let decoded = result.ir();
    assert_eq!(decoded.design.scopes.len(), ir.design.scopes.len());
    assert_eq!(decoded.design.containers, ir.design.containers);
    assert_eq!(decoded.design.features, ir.design.features);
    let scope = decoded
        .design
        .scopes
        .iter()
        .find(|scope| scope.definition == part)
        .expect("the part's scope stays");
    let overrides: Vec<_> = scope
        .occurrences
        .values()
        .flat_map(|occurrence| occurrence.parameters.values())
        .collect();
    let [only] = overrides.as_slice() else {
        panic!("one override: {overrides:?}");
    };
    assert!(
        matches!(&only.expression, Some(Expression::Source { tokens, .. }) if tokens.is_empty()),
        "{:?}",
        only.expression
    );
}

/// A scope with no partial form, given up, takes its root containers, each
/// member of those containers, and each record that names the scope.
#[test]
fn a_dropped_scope_takes_its_containers_their_members_and_its_records() {
    let mut ir = document();
    let before = errors(&ir);
    let part: ProductDefinitionId = id("definition", "part");
    let arena = DecodeArena::new();
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &DecodePolicy::default())
        .expect("a decode context");
    let removed = drop_records(
        &ctx,
        &mut ir,
        vec![Seed {
            arena: Arena::home("design", "scopes").expect("a registered arena"),
            id: part.as_str().to_owned(),
        }],
    )
    .expect("the removal completes");

    let [(_, released)] = removed.as_slice() else {
        panic!("one removal: {removed:?}");
    };
    let dropped: BTreeSet<&str> = released.dropped.iter().map(String::as_str).collect();
    let expected: BTreeSet<String> = [
        id::<String>("container", "timeline"),
        id("feature", "extrude"),
        id("feature", "fillet"),
        id("configuration", "base"),
        id("configuration", "large"),
        id("evaluation", "after-extrude"),
        id("evaluation", "large-after-fillet"),
    ]
    .into();
    assert_eq!(
        dropped,
        expected.iter().map(String::as_str).collect::<BTreeSet<_>>()
    );
    let design = &ir.design;
    assert!(design.scopes.is_empty());
    assert!(design.containers.is_empty());
    assert!(design.features.is_empty());
    assert!(design.configurations.is_empty());
    assert!(design.evaluations.is_empty());
    assert!(ir
        .model
        .product_definitions
        .iter()
        .any(|definition| definition.id == part));
    let after = errors(&ir);
    assert!(
        after.is_subset(&before),
        "new errors: {:?}",
        after.difference(&before).collect::<Vec<_>>()
    );
}
