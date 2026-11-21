// SPDX-License-Identifier: Apache-2.0
//! Model-class records that reference the design class.

use super::{ArenaName, Model};
use crate::attributes::{AttributeTarget, AttributeValue, SourceAttribute};
use crate::declared_measures::DeclaredTarget;
use crate::design::expr::{Expression, ExpressionToken, Neutral, Node};
use crate::physics::solver::SolverValue;
use crate::pmi::PmiDefinition;
use crate::references::{InstanceTarget, ReferenceSelection};

/// The first model arena holding a record that references the design
/// class: an instance target's or a tessellation's state, a declared
/// measure of a state, a reference to a feature, an attribute on a
/// configuration, a feature, a sketch, or a parameter, or an attribute's
/// parameter value or expression.
pub(super) fn first(model: &Model) -> Option<ArenaName> {
    let arenas: [(&str, bool); 13] = [
        (
            "appearance_bindings",
            model
                .appearance_bindings
                .iter()
                .any(|binding| binding.target.reads_design()),
        ),
        (
            "attributes",
            model.attributes.iter().any(attribute_reads_design),
        ),
        (
            "pmi",
            model.pmi.iter().any(|annotation| {
                annotation.targets.iter().any(InstanceTarget::reads_design)
                    || pmi_definition_reads_design(&annotation.definition)
            }),
        ),
        (
            "presentation_layers",
            model
                .presentation_layers
                .iter()
                .any(|layer| layer.items.iter().any(InstanceTarget::reads_design)),
        ),
        (
            "mass_properties",
            model
                .mass_properties
                .iter()
                .any(|record| record.target.reads_design()),
        ),
        (
            "rigid_bodies",
            model
                .rigid_bodies
                .iter()
                .any(|record| record.target.reads_design()),
        ),
        (
            "colliders",
            model
                .colliders
                .iter()
                .any(|record| record.target.reads_design()),
        ),
        (
            "articulations",
            model
                .articulations
                .iter()
                .any(|record| record.root.reads_design()),
        ),
        (
            "solver_settings",
            model.solver_settings.iter().any(|record| {
                record.target.reads_design()
                    || record.values.values().any(|value| {
                        matches!(value, SolverValue::Targets { targets }
                            if targets.iter().any(InstanceTarget::reads_design))
                    })
            }),
        ),
        (
            "tessellations",
            model
                .tessellations
                .iter()
                .any(|tessellation| tessellation.state.is_some()),
        ),
        (
            "declared_measures",
            model
                .declared_measures
                .iter()
                .any(|measure| matches!(measure.target, DeclaredTarget::State(_))),
        ),
        (
            "drawings",
            model
                .drawings
                .iter()
                .any(|drawing| references_read_design(drawing.relationships.values())),
        ),
        (
            "semantic_annotations",
            model
                .semantic_annotations
                .iter()
                .any(|annotation| references_read_design(annotation.references.values())),
        ),
    ];
    arenas
        .into_iter()
        .find_map(|(name, reads)| reads.then(|| ArenaName::parse(name)).flatten())
}

/// Whether an attribute targets a design record or a state, or holds a
/// parameter value or an expression that reads a parameter or a measure.
fn attribute_reads_design(attribute: &SourceAttribute) -> bool {
    attribute.target.reads_design()
        || matches!(
            attribute.target.entity,
            AttributeTarget::Configuration(_)
                | AttributeTarget::Feature(_)
                | AttributeTarget::Sketch(_)
                | AttributeTarget::Parameter(_)
        )
        || attribute.values.iter().flatten().any(|value| match value {
            AttributeValue::Parameter(_) => true,
            AttributeValue::Expression(expression) => {
                expression_reads_design(&expression.expression)
            }
            _ => false,
        })
}

fn expression_reads_design(expression: &Expression) -> bool {
    match expression {
        Expression::Neutral(Neutral(root)) => {
            let mut pending = vec![root];
            while let Some(node) = pending.pop() {
                if matches!(node, Node::Reference(_) | Node::Measure(_)) {
                    return true;
                }
                pending.extend(node.operands());
            }
            false
        }
        Expression::Source { tokens, .. } => tokens
            .iter()
            .any(|token| matches!(token, ExpressionToken::Reference(_))),
    }
}

fn pmi_definition_reads_design(definition: &PmiDefinition) -> bool {
    match definition {
        PmiDefinition::DatumTarget { basis, .. } => basis.iter().any(InstanceTarget::reads_design),
        PmiDefinition::GeometricTolerance { zones, .. } => zones
            .iter()
            .flat_map(|zone| &zone.projections)
            .flat_map(crate::pmi::zones::ToleranceProjection::targets)
            .any(InstanceTarget::reads_design),
        PmiDefinition::Datum { .. }
        | PmiDefinition::DatumSystem { .. }
        | PmiDefinition::Dimension(_)
        | PmiDefinition::Presentation { .. } => false,
    }
}

fn references_read_design<'a>(groups: impl Iterator<Item = &'a Vec<ReferenceSelection>>) -> bool {
    groups
        .flatten()
        .any(|selection| selection.target.reads_design())
}
