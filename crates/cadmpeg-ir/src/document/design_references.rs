// SPDX-License-Identifier: Apache-2.0
//! Model-class records that reference the design class.

use super::{ArenaName, Model};
use crate::attributes::{AttributeTarget, AttributeValue, SourceAttribute};
use crate::declared_measures::DeclaredTarget;
use crate::physics::solver::SolverValue;
use crate::pmi::PmiDefinition;
use crate::references::{InstanceTarget, ReferenceSelection};
use crate::schema::ArenaRecord;

/// The first model arena holding a record that references the design
/// class.
pub(super) fn first(model: &Model) -> Option<ArenaName> {
    holders(model)
        .into_iter()
        .find_map(|(arena, ids)| (!ids.is_empty()).then_some(arena))
}

/// Each model arena with the ids of its records that reference the design
/// class: an instance target's or a tessellation's state, a declared
/// measure of a state, a reference to a feature, an attribute on a
/// configuration, a feature, a sketch, or a parameter, or an attribute's
/// parameter value or expression.
pub(super) fn holders(model: &Model) -> Vec<(ArenaName, Vec<&str>)> {
    let arenas: [(&str, Vec<&str>); 14] = [
        (
            "appearance_bindings",
            reading(&model.appearance_bindings, |binding| {
                binding.target.reads_design()
            }),
        ),
        (
            "attributes",
            reading(&model.attributes, attribute_reads_design),
        ),
        (
            "pmi",
            reading(&model.pmi, |annotation| {
                annotation.targets.iter().any(InstanceTarget::reads_design)
                    || pmi_definition_reads_design(&annotation.definition)
            }),
        ),
        (
            "presentation_layers",
            reading(&model.presentation_layers, |layer| {
                layer.items.iter().any(InstanceTarget::reads_design)
            }),
        ),
        (
            "mass_properties",
            reading(&model.mass_properties, |record| {
                record.target.reads_design()
            }),
        ),
        (
            "rigid_bodies",
            reading(&model.rigid_bodies, |record| record.target.reads_design()),
        ),
        (
            "colliders",
            reading(&model.colliders, |record| record.target.reads_design()),
        ),
        (
            "articulations",
            reading(&model.articulations, |record| record.root.reads_design()),
        ),
        (
            "solver_settings",
            reading(&model.solver_settings, |record| {
                record.target.reads_design()
                    || record.values.values().any(|value| {
                        matches!(value, SolverValue::Targets { targets }
                            if targets.iter().any(InstanceTarget::reads_design))
                    })
            }),
        ),
        (
            "product_definitions",
            reading(&model.product_definitions, |definition| {
                definition
                    .representations
                    .iter()
                    .any(|representation| !representation.features.is_empty())
            }),
        ),
        (
            "tessellations",
            reading(&model.tessellations, |tessellation| {
                tessellation.state.is_some()
            }),
        ),
        (
            "declared_measures",
            reading(&model.declared_measures, |measure| {
                matches!(measure.target, DeclaredTarget::State(_))
            }),
        ),
        (
            "drawings",
            reading(&model.drawings, |drawing| {
                references_read_design(drawing.relationships.values())
            }),
        ),
        (
            "semantic_annotations",
            reading(&model.semantic_annotations, |annotation| {
                references_read_design(annotation.references.values())
            }),
        ),
    ];
    arenas
        .into_iter()
        .filter_map(|(name, ids)| Some((ArenaName::parse(name)?, ids)))
        .collect()
}

/// The ids of the `records` for which `reads` holds.
fn reading<T: ArenaRecord>(records: &[T], reads: impl Fn(&T) -> bool) -> Vec<&str> {
    records
        .iter()
        .filter(|record| reads(*record))
        .map(ArenaRecord::record_identity)
        .collect()
}

/// Whether an attribute targets a design record or a state, or holds a
/// parameter value.
fn attribute_reads_design(attribute: &SourceAttribute) -> bool {
    attribute.target.reads_design()
        || matches!(
            attribute.target.entity,
            AttributeTarget::Configuration(_)
                | AttributeTarget::Feature(_)
                | AttributeTarget::Sketch(_)
                | AttributeTarget::Parameter(_)
        )
        || attribute
            .values
            .iter()
            .flatten()
            .any(|value| matches!(value, AttributeValue::Parameter(_)))
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
