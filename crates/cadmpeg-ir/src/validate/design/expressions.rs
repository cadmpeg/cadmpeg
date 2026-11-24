// SPDX-License-Identifier: Apache-2.0
//! Expressions and parameter references (§5.12, §5.13): every expression the
//! design root states, a program's statements, a sheet-metal rule's bend
//! allowances, and a process parameter's references.
//!
//! A `Neutral` tree reads only after its canonical text parses and prints
//! back to itself, and a design expression's references are design
//! references by type, so those two rules hold by construction.

use std::collections::{BTreeMap, BTreeSet};

use cadmpeg_core::decode::DecodeContext;
use cadmpeg_core::CodecError;

use super::evaluations::check_path_steps;
use super::parameters::{node_references, references};
use super::records::DesignRecords;
use super::walk::{Collect, ExpressionPlace, Walk};
use crate::design::configuration::ParameterOverride;
use crate::design::dimension::{ParamRef, ParameterRef, RefTarget};
use crate::design::edit::{check_types, kind_type, ExpressionSite, Type};
use crate::design::expr::{Expression, ExpressionToken, Measure, MeasureKind, MeasureTarget, Node};
use crate::design::feature::Feature;
use crate::design::parameter::{ParameterDefinition, ParameterOwner, ParameterValue};
use crate::design::program::{ProgramBody, Statement};
use crate::design::selection::{Anchor, Picks, StepRef};
use crate::design::units::ValueKind;
use crate::features::ParameterId;
use crate::process::groups::ProcessDefinition;
use crate::products::Storage;
use crate::report::check::Finding;

/// Where one expression occurs.
#[derive(Clone, Copy)]
pub(super) struct Site<'a> {
    /// The record that holds the expression.
    pub(super) entity: &'a str,
    /// The kind the root has, where the site states one.
    pub(super) kind: Option<&'a ValueKind>,
    /// Whether the expression is a feature-owned parameter's, whose measures
    /// may be anchored `Input`.
    pub(super) feature_owned: bool,
    /// The variables in scope.
    pub(super) variables: ExpressionSite,
}

pub(super) fn check_expressions(
    ctx: &DecodeContext<'_>,
    index: &DesignRecords<'_>,
    findings: &mut Vec<Finding>,
) -> Result<(), CodecError> {
    let design = &index.ir.design;
    for parameter in &design.parameters {
        if let ParameterDefinition::Expression(expression) = &parameter.definition {
            let site = Site {
                entity: parameter.id.as_str(),
                kind: Some(&parameter.kind),
                feature_owned: matches!(parameter.owner, Some(ParameterOwner::Feature(_))),
                variables: ExpressionSite::Parameter,
            };
            check_expression(index, site, expression, findings);
        }
    }
    for configuration in &design.configurations {
        check_overrides(
            index,
            configuration.id.as_str(),
            &configuration.overrides.parameters,
            findings,
        );
    }
    for arrangement in &design.arrangements {
        check_overrides(
            index,
            arrangement.id.as_str(),
            &arrangement.overrides.parameters,
            findings,
        );
    }
    for scope in &design.scopes {
        for occurrence in scope.occurrences.values() {
            check_overrides(
                index,
                scope.definition.as_str(),
                &occurrence.parameters,
                findings,
            );
        }
    }
    for program in &design.programs {
        let site = Site {
            entity: program.id.as_str(),
            kind: None,
            feature_owned: false,
            variables: ExpressionSite::Parameter,
        };
        match &program.body {
            ProgramBody::Neutral(statements) => check_statements(index, site, statements, findings),
            ProgramBody::Source { tokens, .. } => {
                check_tokens(site.entity, tokens, findings);
                for token in tokens {
                    if let ExpressionToken::Reference(reference) = token {
                        check_reference(index, site, reference, findings);
                    }
                }
            }
        }
    }
    for rule in &design.sheet_metal_rules {
        let site = Site {
            entity: rule.id.as_str(),
            kind: None,
            feature_owned: false,
            variables: ExpressionSite::BendAllowance,
        };
        let mut walked = Collect::new(ctx);
        rule.walk(&mut walked)?;
        for (_, expression) in walked.expressions {
            check_expression(index, site, expression, findings);
        }
    }
    for feature in &design.features {
        check_feature_expressions(ctx, index, feature, findings)?;
    }
    check_process_parameters(index, findings);
    check_cycles(index, findings);
    Ok(())
}

fn check_overrides(
    index: &DesignRecords<'_>,
    entity: &str,
    overrides: &BTreeMap<ParameterId, ParameterOverride>,
    findings: &mut Vec<Finding>,
) {
    for (parameter, stated) in overrides {
        let (Some(expression), Some(record)) =
            (&stated.expression, index.parameters.get(parameter.as_str()))
        else {
            continue;
        };
        let site = Site {
            entity,
            kind: Some(&record.kind),
            feature_owned: matches!(record.owner, Some(ParameterOwner::Feature(_))),
            variables: ExpressionSite::Parameter,
        };
        check_expression(index, site, expression, findings);
    }
}

/// Every rule of part 10 on one expression: its references, its tokens, its
/// type, and its measures.
pub(super) fn check_expression(
    index: &DesignRecords<'_>,
    site: Site<'_>,
    expression: &Expression,
    findings: &mut Vec<Finding>,
) {
    for reference in references(expression) {
        check_reference(index, site, reference, findings);
    }
    match expression {
        Expression::Neutral(neutral) => check_tree(index, site, &neutral.0, findings),
        Expression::Source { tokens, .. } => check_tokens(site.entity, tokens, findings),
    }
}

/// `Source` tokens hold at least one `Text` token. An empty token list, a
/// parameter whose value and text the decoder did not read, is left
/// unchecked (interim, gap 20261119-X3-1).
fn check_tokens<R>(entity: &str, tokens: &[ExpressionToken<R>], findings: &mut Vec<Finding>) {
    if !tokens.is_empty()
        && !tokens
            .iter()
            .any(|token| matches!(token, ExpressionToken::Text(_)))
    {
        super::error(
            findings,
            entity,
            format_args!("source expression holds no text token"),
        );
    }
}

/// The type of a design parameter reference: its parameter's kind, unknown
/// where the parameter is not in the document.
fn design_type(index: &DesignRecords<'_>, reference: &ParameterRef) -> Result<Type, String> {
    match &reference.target {
        RefTarget::Parameter(target) => match index.parameters.get(target.as_str()) {
            Some(record) => Type::of_kind(&record.kind).ok_or_else(|| {
                format!(
                    "{} has the kind {:?}, with no neutral value",
                    target.as_str(),
                    record.kind
                )
            }),
            None => Ok(Type::Unknown),
        },
        RefTarget::External { .. } => Ok(Type::Unknown),
    }
}

/// A design neutral tree type-checks, its root has the site's kind, and its
/// measures are anchored and sized as their kinds take.
fn check_tree(
    index: &DesignRecords<'_>,
    site: Site<'_>,
    root: &Node<ParameterRef>,
    findings: &mut Vec<Finding>,
) {
    check_typed_tree(
        site,
        root,
        &|reference| design_type(index, reference),
        findings,
    );
}

/// A neutral tree type-checks with `type_of` giving each reference's type,
/// its root has the site's kind (a number for a law), and its measures are
/// anchored and sized as their kinds take.
fn check_typed_tree<R>(
    site: Site<'_>,
    root: &Node<R>,
    type_of: &dyn Fn(&R) -> Result<Type, String>,
    findings: &mut Vec<Finding>,
) {
    match check_types(root, site.variables, type_of) {
        Err(reason) => super::error(
            findings,
            site.entity,
            format_args!("expression does not type-check: {reason}"),
        ),
        Ok(found) => {
            let root_kind = match (site.kind, site.variables) {
                (Some(kind), _) => kind_type(kind, found),
                (None, ExpressionSite::Law) => match found {
                    Type::Number(_) | Type::Unknown => Ok(()),
                    Type::Boolean | Type::Text => {
                        Err(format!("a law's value is a number, not {found:?}"))
                    }
                },
                (None, ExpressionSite::Parameter | ExpressionSite::BendAllowance) => Ok(()),
            };
            if let Err(reason) = root_kind {
                super::error(
                    findings,
                    site.entity,
                    format_args!("expression's root does not have the parameter's kind: {reason}"),
                );
            }
        }
    }
    for measure in measures(root) {
        check_measure(site, measure, findings);
    }
}

/// The measures of a neutral tree, walked without recursion.
pub(super) fn measures<R>(root: &Node<R>) -> Vec<&Measure> {
    let mut found = Vec::new();
    let mut pending = vec![root];
    while let Some(node) = pending.pop() {
        match node {
            Node::Measure(measure) => found.push(&**measure),
            Node::Unary { operand, .. } => pending.push(operand),
            Node::Binary { left, right, .. } => pending.extend([&**left, &**right]),
            Node::Call { args, .. } => pending.extend(args),
            Node::If(branches) => pending.extend(branches.iter()),
            Node::Number { .. }
            | Node::Constant(_)
            | Node::Boolean(_)
            | Node::Text(_)
            | Node::Reference(_)
            | Node::LawParameter
            | Node::Bend(_) => {}
        }
    }
    found
}

/// A measure's selection is anchored `Pinned`, `Current`, or, in a
/// feature-owned parameter's expression, `Input`; `Distance` and `Angle`
/// select two picks, the others one or more.
fn check_measure(site: Site<'_>, measure: &Measure, findings: &mut Vec<Finding>) {
    let entity = site.entity;
    let two = matches!(measure.kind, MeasureKind::Distance | MeasureKind::Angle);
    let selection = match &measure.of {
        MeasureTarget::Selection(selection) => selection,
        MeasureTarget::Model => {
            if two {
                super::error(
                    findings,
                    entity,
                    format_args!(
                        "a {:?} measure reads the whole model; it selects two picks",
                        measure.kind
                    ),
                );
            }
            return;
        }
    };
    match &selection.at {
        Anchor::Pinned(_) | Anchor::Current => {}
        Anchor::Input if site.feature_owned => {}
        Anchor::Input => super::error(
            findings,
            entity,
            format_args!(
                "a measure is anchored `input` outside a feature-owned parameter's expression"
            ),
        ),
        Anchor::Native(_) => super::error(
            findings,
            entity,
            format_args!("a measure's selection is anchored `native`"),
        ),
    }
    let count = match &selection.picks {
        Picks::Named(picks) => picks.as_slice().len(),
        Picks::Unread | Picks::Rule { .. } => return,
    };
    if two && count != 2 {
        super::error(
            findings,
            entity,
            format_args!(
                "a {:?} measure selects {count} picks; it selects two",
                measure.kind
            ),
        );
    } else if count == 0 {
        super::error(
            findings,
            entity,
            format_args!("a {:?} measure selects no pick", measure.kind),
        );
    }
}

/// A program's statements: each `Assign` value has its target's kind, and
/// each condition is Boolean.
fn check_statements(
    index: &DesignRecords<'_>,
    site: Site<'_>,
    statements: &[Statement],
    findings: &mut Vec<Finding>,
) {
    let mut pending: Vec<&Statement> = statements.iter().rev().collect();
    while let Some(statement) = pending.pop() {
        match statement {
            Statement::Assign { target, value } => {
                check_reference(index, site, target, findings);
                let kind = match &target.target {
                    RefTarget::Parameter(parameter) => index
                        .parameters
                        .get(parameter.as_str())
                        .map(|record| &record.kind),
                    RefTarget::External { .. } => None,
                };
                let site = Site { kind, ..site };
                check_neutral(index, site, &value.0, findings);
            }
            Statement::If {
                condition,
                then,
                otherwise,
            } => {
                let site = Site {
                    kind: Some(&ValueKind::Boolean),
                    ..site
                };
                check_neutral(index, site, &condition.0, findings);
                pending.extend(otherwise.iter().rev());
                pending.extend(then.iter().rev());
            }
        }
    }
}

fn check_neutral(
    index: &DesignRecords<'_>,
    site: Site<'_>,
    root: &Node<ParameterRef>,
    findings: &mut Vec<Finding>,
) {
    for reference in node_references(root) {
        check_reference(index, site, reference, findings);
    }
    check_tree(index, site, root, findings);
}

/// A reference resolves: a `Parameter` exists, an `External` definition
/// exists, and a `through` path names occurrences of the model and ends at a
/// prototype that is the target's scope.
pub(super) fn check_reference(
    index: &DesignRecords<'_>,
    site: Site<'_>,
    reference: &ParameterRef,
    findings: &mut Vec<Finding>,
) {
    let entity = site.entity;
    let target_scope = match &reference.target {
        RefTarget::Parameter(target) => match index.parameters.get(target.as_str()) {
            Some(record) => Some(record.scope.as_str()),
            None => {
                super::error(
                    findings,
                    entity,
                    format_args!(
                        "expression references {}, which is no parameter of the design",
                        target.as_str()
                    ),
                );
                None
            }
        },
        RefTarget::External { definition, .. } => {
            let definition = definition.as_str();
            match index.definitions.get(definition).map(|record| &record.storage) {
                None => super::error(
                    findings,
                    entity,
                    format_args!("expression references a parameter of {definition}, which is no product definition"),
                ),
                Some(Storage::External(_)) => {}
                Some(Storage::Embedded | Storage::File { .. }) => super::error(
                    findings,
                    entity,
                    format_args!("expression references a parameter of {definition} by name, but {definition} is no external definition"),
                ),
            }
            Some(definition)
        }
    };
    let Some(path) = &reference.through else {
        return;
    };
    check_path_steps(
        index,
        path,
        entity,
        "a parameter reference's `through`",
        findings,
    );
    let last = match path.0.last().map(|step| &step.occurrence) {
        Some(StepRef::Occurrence(occurrence)) => index.prototype(occurrence.as_str()),
        Some(StepRef::Native(_)) | None => None,
    };
    if let (Some(last), Some(target_scope)) = (last, target_scope) {
        if last != target_scope {
            super::error(
                findings,
                entity,
                format_args!(
                    "a parameter reference's `through` path ends at {last}, not at the target's scope {target_scope}"
                ),
            );
        }
    }
}

/// Every expression a feature's operation holds: its laws, its bend
/// allowances, and its pattern instances' seed expressions.
fn check_feature_expressions(
    ctx: &DecodeContext<'_>,
    index: &DesignRecords<'_>,
    feature: &Feature,
    findings: &mut Vec<Finding>,
) -> Result<(), CodecError> {
    let mut walked = Collect::new(ctx);
    feature.definition.operation.walk(&mut walked)?;
    let entity = feature.id.as_str();
    for (place, expression) in walked.expressions {
        let base = Site {
            entity,
            kind: None,
            feature_owned: false,
            variables: ExpressionSite::Parameter,
        };
        let site = match place {
            ExpressionPlace::Law => Site {
                variables: ExpressionSite::Law,
                ..base
            },
            ExpressionPlace::BendAllowance => Site {
                variables: ExpressionSite::BendAllowance,
                ..base
            },
            ExpressionPlace::Seed(parameter) => {
                let record = index.parameters.get(parameter);
                Site {
                    kind: record.map(|record| &record.kind),
                    feature_owned: record.is_some_and(|record| {
                        matches!(record.owner, Some(ParameterOwner::Feature(_)))
                    }),
                    ..base
                }
            }
        };
        check_expression(index, site, expression, findings);
    }
    Ok(())
}

/// A process parameter with no kind holds a `Number` and has a `Source`
/// expression, no neutral tree references it, a stated value follows a stated
/// kind, each reference of its expression resolves, and a neutral expression
/// type-checks with a root of the stated kind.
pub(super) fn check_process_parameters(index: &DesignRecords<'_>, findings: &mut Vec<Finding>) {
    let process = &index.ir.process;
    let kinds: BTreeMap<&str, Option<&ValueKind>> = process
        .parameters
        .iter()
        .map(|parameter| (parameter.id.as_str(), parameter.kind.as_ref()))
        .collect();
    let type_of = |reference: &ParamRef| match reference {
        ParamRef::Process(target) => match kinds.get(target.as_str()) {
            Some(Some(kind)) => Type::of_kind(kind).ok_or_else(|| {
                format!(
                    "{} has the kind {kind:?}, with no neutral value",
                    target.as_str()
                )
            }),
            Some(None) | None => Ok(Type::Unknown),
        },
        ParamRef::Design(design) => design_type(index, design),
    };
    for parameter in &process.parameters {
        let entity = parameter.id.as_str();
        if let Some(value) = &parameter.value {
            let follows = match (&parameter.kind, value) {
                (None | Some(ValueKind::Quantity(_)), ParameterValue::Number(_))
                | (Some(ValueKind::Integer), ParameterValue::Integer(_))
                | (Some(ValueKind::Boolean), ParameterValue::Boolean(_))
                | (Some(ValueKind::Text), ParameterValue::Text(_))
                | (Some(ValueKind::Native(_)), _) => true,
                (
                    None
                    | Some(
                        ValueKind::Quantity(_)
                        | ValueKind::Integer
                        | ValueKind::Boolean
                        | ValueKind::Text,
                    ),
                    _,
                ) => false,
            };
            if !follows {
                super::error(
                    findings,
                    entity,
                    format_args!(
                        "process parameter's value {value:?} does not follow its kind {:?}",
                        parameter.kind
                    ),
                );
            }
        }
        let ProcessDefinition::Expression(expression) = &parameter.definition else {
            continue;
        };
        let refs: Vec<&ParamRef> = match expression {
            Expression::Neutral(neutral) => {
                if parameter.kind.is_none() {
                    super::error(
                        findings,
                        entity,
                        format_args!(
                            "process parameter has no kind, so its expression is `source`"
                        ),
                    );
                }
                for reference in node_references(&neutral.0) {
                    if let ParamRef::Process(target) = reference {
                        if let Some(None) = kinds.get(target.as_str()) {
                            super::error(
                                findings,
                                entity,
                                format_args!(
                                    "neutral expression references {}, a process parameter with no kind",
                                    target.as_str()
                                ),
                            );
                        }
                    }
                }
                let site = Site {
                    entity,
                    kind: parameter.kind.as_ref(),
                    feature_owned: false,
                    variables: ExpressionSite::Parameter,
                };
                check_typed_tree(site, &neutral.0, &type_of, findings);
                node_references(&neutral.0)
            }
            Expression::Source { tokens, .. } => {
                check_tokens(entity, tokens, findings);
                tokens
                    .iter()
                    .filter_map(|token| match token {
                        ExpressionToken::Reference(reference) => Some(reference),
                        ExpressionToken::Text(_) => None,
                    })
                    .collect()
            }
        };
        for reference in refs {
            match reference {
                ParamRef::Process(target) if !kinds.contains_key(target.as_str()) => super::error(
                    findings,
                    entity,
                    format_args!(
                        "expression references {}, which is no process parameter",
                        target.as_str()
                    ),
                ),
                ParamRef::Process(_) => {}
                ParamRef::Design(design) => {
                    let site = Site {
                        entity,
                        kind: None,
                        feature_owned: false,
                        variables: ExpressionSite::Parameter,
                    };
                    check_reference(index, site, design, findings);
                }
            }
        }
    }
}

/// Parameter reference cycles through the parameters' own expressions are
/// Warnings (D6).
fn check_cycles(index: &DesignRecords<'_>, findings: &mut Vec<Finding>) {
    let mut graph: BTreeMap<&str, BTreeSet<&str>> = BTreeMap::new();
    for (&id, parameter) in &index.parameters {
        let targets = graph.entry(id).or_default();
        if let ParameterDefinition::Expression(expression) = &parameter.definition {
            for reference in references(expression) {
                if let RefTarget::Parameter(target) = &reference.target {
                    if let Some((&target, _)) = index.parameters.get_key_value(target.as_str()) {
                        targets.insert(target);
                    }
                }
            }
        }
    }
    for cycle in cycles(&graph) {
        super::warning(
            findings,
            cycle[0],
            format_args!(
                "parameters {} reference each other in a cycle",
                cycle.join(", ")
            ),
        );
    }
}

/// The strongly connected components of `graph` that hold a cycle, each
/// sorted, by Kosaraju's two passes, each walked without recursion.
pub(in crate::validate) fn cycles<'a>(
    graph: &BTreeMap<&'a str, BTreeSet<&'a str>>,
) -> Vec<Vec<&'a str>> {
    let mut order = Vec::with_capacity(graph.len());
    let mut visited = BTreeSet::new();
    for &start in graph.keys() {
        if !visited.insert(start) {
            continue;
        }
        let mut stack = vec![(start, graph[start].iter())];
        while let Some((node, next)) = stack.last_mut() {
            if let Some(&target) = next.next() {
                if graph.contains_key(target) && visited.insert(target) {
                    stack.push((target, graph[target].iter()));
                }
            } else {
                order.push(*node);
                stack.pop();
            }
        }
    }
    let mut reverse: BTreeMap<&str, Vec<&str>> = BTreeMap::new();
    for (&from, targets) in graph {
        for &to in targets {
            reverse.entry(to).or_default().push(from);
        }
    }
    let mut assigned = BTreeSet::new();
    let mut found = Vec::new();
    for &root in order.iter().rev() {
        if !assigned.insert(root) {
            continue;
        }
        let mut component = vec![root];
        let mut pending = vec![root];
        while let Some(node) = pending.pop() {
            for &from in reverse.get(node).into_iter().flatten() {
                if assigned.insert(from) {
                    component.push(from);
                    pending.push(from);
                }
            }
        }
        let cyclic = component.len() > 1 || graph[root].contains(root);
        if cyclic {
            component.sort_unstable();
            found.push(component);
        }
    }
    found
}
