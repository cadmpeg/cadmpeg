// SPDX-License-Identifier: Apache-2.0
//! Caches that in-IR arithmetic decides (part 10, Caches): a parameter's
//! value against its expression, and a coupling's ratio against its radii.
//! Each disagreement is a Warning.
//!
//! No value that depends on a measure is checked: the source evaluates a
//! measure with a kernel and stores it at its own precision (§5.13).

use std::collections::BTreeSet;

use super::parameters::references;
use super::records::DesignRecords;
use crate::design::dimension::{Dimension, ParameterRef, RefTarget};
use crate::design::edit::{cache_value, evaluate, Value};
use crate::design::expr::{Expression, Node};
use crate::design::feature::FeatureOperation;
use crate::design::ops::shared::{Coupling, JointKind};
use crate::design::parameter::{DesignParameter, ParameterDefinition};
use crate::design::program::{ProgramBody, Statement};
use crate::design::units::{Quantity, ValueKind};
use crate::report::check::Finding;

/// The relative tolerance of a value that is neither a length nor an angle.
const RELATIVE: f64 = 1e-9;

pub(super) fn check_caches(index: &DesignRecords<'_>, findings: &mut Vec<Finding>) {
    for parameter in &index.ir.design.parameters {
        check_parameter(index, parameter, findings);
    }
    for feature in &index.ir.design.features {
        if let FeatureOperation::Joint(joint) = &feature.definition.operation {
            if let JointKind::Coupling { coupling, .. } = &joint.kind {
                check_coupling(feature.id.as_str(), coupling, findings);
            }
        }
    }
}

/// A parameter's cached value against its neutral expression, where the
/// expression depends on no measure, in its own tree or through the
/// parameters it references; and an integer parameter's evaluated value is a
/// whole number.
fn check_parameter(
    index: &DesignRecords<'_>,
    parameter: &DesignParameter,
    findings: &mut Vec<Finding>,
) {
    let ParameterDefinition::Expression(Expression::Neutral(neutral)) = &parameter.definition
    else {
        return;
    };
    let id = parameter.id.as_str();
    let mut cache_of = |reference: &ParameterRef| {
        let RefTarget::Parameter(target) = &reference.target else {
            return None;
        };
        if reference.through.is_some() {
            return None;
        }
        let record = index.parameters.get(target.as_str())?;
        cache_value(&record.kind, record.value.as_ref()?)
    };
    let Ok(evaluated) = evaluate(&neutral.0, &mut cache_of) else {
        return;
    };
    if parameter.kind == ValueKind::Integer {
        if let Value::Number { value, .. } = evaluated {
            if value.fract() != 0.0 {
                super::warning(
                    findings,
                    id,
                    format_args!(
                        "integer parameter's expression evaluates to {value}, not a whole number"
                    ),
                );
            }
        }
    }
    let Some(cached) = parameter.value.as_ref() else {
        return;
    };
    let Some(cached) = cache_value(&parameter.kind, cached) else {
        return;
    };
    if depends_on_measure(index, &neutral.0) {
        return;
    }
    if !agree(index, &evaluated, &cached) {
        super::warning(
            findings,
            id,
            format_args!("parameter's cached value {cached:?} disagrees with its expression's value {evaluated:?}"),
        );
    }
}

/// Whether a tree reads a measure, in itself or through the expressions of
/// the parameters it references. An assigned parameter reads what its
/// program's conditions and its own assigned values read. A reference whose
/// value no in-IR arithmetic decides (a driven parameter, one a `Source`
/// program assigns, a parameter of a document that was not read, or one read
/// through an occurrence path) counts as one.
fn depends_on_measure(index: &DesignRecords<'_>, root: &Node<ParameterRef>) -> bool {
    let mut seen = BTreeSet::new();
    let mut pending = vec![root];
    while let Some(root) = pending.pop() {
        if !super::expressions::measures(root).is_empty() {
            return true;
        }
        for reference in super::parameters::node_references(root) {
            let RefTarget::Parameter(target) = &reference.target else {
                return true;
            };
            if reference.through.is_some() {
                return true;
            }
            if !seen.insert(target.as_str()) {
                continue;
            }
            let Some(record) = index.parameters.get(target.as_str()) else {
                return true;
            };
            match &record.definition {
                ParameterDefinition::Expression(Expression::Neutral(neutral)) => {
                    pending.push(&neutral.0);
                }
                ParameterDefinition::Expression(source @ Expression::Source { .. }) => {
                    if !references(source).is_empty() {
                        return true;
                    }
                }
                ParameterDefinition::Assigned(program) => {
                    let body = index
                        .ir
                        .design
                        .programs
                        .iter()
                        .find(|candidate| candidate.id == *program)
                        .map(|program| &program.body);
                    let Some(ProgramBody::Neutral(statements)) = body else {
                        return true;
                    };
                    pending.extend(program_reads(statements, target.as_str()));
                }
                ParameterDefinition::Driven => return true,
            }
        }
    }
    false
}

/// The trees a program reads to assign `parameter`: every condition, and
/// each value it assigns to `parameter`, walked without recursion.
fn program_reads<'a>(statements: &'a [Statement], parameter: &str) -> Vec<&'a Node<ParameterRef>> {
    let mut reads = Vec::new();
    let mut pending: Vec<&Statement> = statements.iter().collect();
    while let Some(statement) = pending.pop() {
        match statement {
            Statement::Assign { target, value } => {
                if matches!(&target.target, RefTarget::Parameter(id) if id.as_str() == parameter) {
                    reads.push(&value.0);
                }
            }
            Statement::If {
                condition,
                then,
                otherwise,
            } => {
                reads.push(&condition.0);
                pending.extend(then.iter().chain(otherwise));
            }
        }
    }
    reads
}

/// Whether two values agree: a length within the linear tolerance, an angle
/// within the angular tolerance, another number within a relative 1e-9, and
/// a Boolean or a text exactly.
fn agree(index: &DesignRecords<'_>, left: &Value, right: &Value) -> bool {
    match (left, right) {
        (
            Value::Number {
                value: a,
                quantity: q,
            },
            Value::Number {
                value: b,
                quantity: r,
            },
        ) => {
            if q != r {
                return false;
            }
            let tolerances = &index.ir.tolerances;
            let allowed = match *q {
                Quantity::LENGTH => tolerances.linear.get(),
                Quantity::ANGLE => tolerances.angular.get(),
                _ => RELATIVE * a.abs().max(b.abs()),
            };
            (a - b).abs() <= allowed
        }
        (Value::Boolean(a), Value::Boolean(b)) => a == b,
        (Value::Text(a), Value::Text(b)) => a == b,
        _ => false,
    }
}

/// A gear's or a belt's literal ratio against its literal radii: the ratio
/// is `r₀ / r₁`.
fn check_coupling(feature: &str, coupling: &Coupling, findings: &mut Vec<Finding>) {
    let (ratio, radii) = match coupling {
        Coupling::Gear { ratio, radii } | Coupling::Belt { ratio, radii } => (ratio, radii),
        Coupling::RackPinion { .. } | Coupling::Screw { .. } | Coupling::Native(_) => return,
    };
    let (
        Some(Dimension::Literal(ratio)),
        [Some(Dimension::Literal(first)), Some(Dimension::Literal(second))],
    ) = (ratio, radii)
    else {
        return;
    };
    if second.get() == 0.0 {
        return;
    }
    let expected = first.get() / second.get();
    let ratio = ratio.get();
    if (ratio - expected).abs() > RELATIVE * ratio.abs().max(expected.abs()) {
        super::warning(
            findings,
            feature,
            format_args!(
                "coupling's ratio {ratio} disagrees with its radii, whose ratio is {expected}"
            ),
        );
    }
}
