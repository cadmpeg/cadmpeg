// SPDX-License-Identifier: Apache-2.0
//! Model-class records that reference the design class, and the model
//! without those references (§2.1).

use super::{ArenaName, Model};
use crate::attributes::{AttributeTarget, AttributeValue, SourceAttribute};
use crate::declared_measures::DeclaredTarget;
use crate::physics::solver::{SolverTarget, SolverValue};
use crate::pmi::PmiDefinition;
use crate::references::{InstanceTarget, ReferenceSelection, ReferenceTarget};
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
/// measure of a state, a reference to a feature, a solver target that names
/// a joint feature, an attribute on a
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
                solver_target_reads_design(&record.target)
                    || record.values.values().any(|value| {
                        matches!(value, SolverValue::Targets { targets }
                            if targets.iter().any(solver_target_reads_design))
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

/// Whether a solver target states a design state or names a joint feature.
fn solver_target_reads_design(target: &InstanceTarget<SolverTarget>) -> bool {
    target.reads_design() || matches!(target.entity, SolverTarget::Joint { .. })
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

/// What leaving the design class out of a document does to one model
/// record (§2.1).
#[derive(Clone, Copy, PartialEq, Eq)]
enum Fate {
    /// The record references no design record.
    Untouched,
    /// The record is kept without its references into the design class.
    Kept,
    /// The record is left out: its own target is a design record.
    LeftOut,
}

/// The model without its references into the design class (§2.1): each such
/// reference is absent, and each record whose target is a design record (an
/// attribute on a configuration, a feature, a sketch, or a parameter, or a
/// solver setting on a joint) stays for the caller to leave out. Returns
/// the ids of the records it kept without a reference and the records it
/// leaves out.
pub(super) fn leave_out(model: &mut Model) -> (Vec<String>, Vec<(ArenaName, String)>) {
    let mut kept = Vec::new();
    let mut left_out = Vec::new();
    let mut each = |name: &str, fates: Vec<(String, Fate)>| {
        let Some(arena) = ArenaName::parse(name) else {
            return;
        };
        for (id, fate) in fates {
            match fate {
                Fate::Untouched => {}
                Fate::Kept => kept.push(id),
                Fate::LeftOut => left_out.push((arena, id)),
            }
        }
    };
    each(
        "appearance_bindings",
        fates(&mut model.appearance_bindings, |binding| {
            kept_if(target(&mut binding.target))
        }),
    );
    each("attributes", fates(&mut model.attributes, attribute));
    each(
        "pmi",
        fates(&mut model.pmi, |annotation| {
            let listed = targets(&mut annotation.targets);
            kept_if(pmi_definition(&mut annotation.definition) | listed)
        }),
    );
    each(
        "presentation_layers",
        fates(&mut model.presentation_layers, |layer| {
            kept_if(targets(&mut layer.items))
        }),
    );
    each(
        "mass_properties",
        fates(&mut model.mass_properties, |record| {
            kept_if(target(&mut record.target))
        }),
    );
    each(
        "rigid_bodies",
        fates(&mut model.rigid_bodies, |record| {
            kept_if(target(&mut record.target))
        }),
    );
    each(
        "colliders",
        fates(&mut model.colliders, |record| {
            kept_if(target(&mut record.target))
        }),
    );
    each(
        "articulations",
        fates(&mut model.articulations, |record| {
            kept_if(target(&mut record.root))
        }),
    );
    each(
        "solver_settings",
        fates(&mut model.solver_settings, |record| {
            if matches!(record.target.entity, SolverTarget::Joint { .. }) {
                return Fate::LeftOut;
            }
            let mut changed = target(&mut record.target);
            for value in record.values.values_mut() {
                if let SolverValue::Targets { targets: listed } = value {
                    let before = listed.len();
                    listed.retain(|listed| !matches!(listed.entity, SolverTarget::Joint { .. }));
                    changed |= listed.len() != before;
                    changed |= targets(listed);
                }
            }
            kept_if(changed)
        }),
    );
    each(
        "product_definitions",
        fates(&mut model.product_definitions, |definition| {
            let mut changed = false;
            for representation in &mut definition.representations {
                changed |= !representation.features.is_empty();
                representation.features.clear();
            }
            kept_if(changed)
        }),
    );
    each(
        "tessellations",
        fates(&mut model.tessellations, |tessellation| {
            kept_if(tessellation.state.take().is_some())
        }),
    );
    each(
        "declared_measures",
        fates(&mut model.declared_measures, |measure| {
            let state = matches!(measure.target, DeclaredTarget::State(_));
            if state {
                measure.target = DeclaredTarget::Unresolved;
            }
            kept_if(state)
        }),
    );
    each(
        "drawings",
        fates(&mut model.drawings, |drawing| {
            kept_if(references(drawing.relationships.values_mut()))
        }),
    );
    each(
        "semantic_annotations",
        fates(&mut model.semantic_annotations, |annotation| {
            kept_if(references(annotation.references.values_mut()))
        }),
    );
    (kept, left_out)
}

/// The fate of each of `records` under `leave`, with its id.
fn fates<T: ArenaRecord>(
    records: &mut [T],
    mut leave: impl FnMut(&mut T) -> Fate,
) -> Vec<(String, Fate)> {
    records
        .iter_mut()
        .map(|record| {
            let fate = leave(record);
            (record.record_identity().to_owned(), fate)
        })
        .collect()
}

const fn kept_if(changed: bool) -> Fate {
    if changed {
        Fate::Kept
    } else {
        Fate::Untouched
    }
}

/// States `target` without its state and without the configurations of
/// this document in its path, and returns whether it held one.
fn target<E>(target: &mut InstanceTarget<E>) -> bool {
    let mut changed = target.state.take().is_some();
    for step in target.path.iter_mut().flat_map(|path| path.0.iter_mut()) {
        if matches!(
            step.configuration,
            Some(crate::design::refs::ConfigurationRef::Local(_))
        ) {
            step.configuration = None;
            changed = true;
        }
    }
    changed
}

fn targets<E>(listed: &mut [InstanceTarget<E>]) -> bool {
    listed
        .iter_mut()
        .fold(false, |changed, listed| target(listed) | changed)
}

/// An attribute on a design record is left out; another loses its state and
/// its parameter values, each stated as no value at its position.
fn attribute(attribute: &mut SourceAttribute) -> Fate {
    if matches!(
        attribute.target.entity,
        AttributeTarget::Configuration(_)
            | AttributeTarget::Feature(_)
            | AttributeTarget::Sketch(_)
            | AttributeTarget::Parameter(_)
    ) {
        return Fate::LeftOut;
    }
    let mut changed = target(&mut attribute.target);
    for value in &mut attribute.values {
        if matches!(value, Some(AttributeValue::Parameter(_))) {
            *value = None;
            changed = true;
        }
    }
    kept_if(changed)
}

fn pmi_definition(definition: &mut PmiDefinition) -> bool {
    match definition {
        PmiDefinition::DatumTarget { basis, .. } => targets(basis),
        PmiDefinition::GeometricTolerance { zones, .. } => zones
            .iter_mut()
            .flat_map(|zone| &mut zone.projections)
            .fold(false, |changed, projection| {
                let boundaries = targets(&mut projection.boundaries);
                target(&mut projection.end) | boundaries | changed
            }),
        PmiDefinition::Datum { .. }
        | PmiDefinition::DatumSystem { .. }
        | PmiDefinition::Dimension(_)
        | PmiDefinition::Presentation { .. } => false,
    }
}

/// States each reference without its state, and a reference to a feature as
/// unresolved.
fn references<'a>(groups: impl Iterator<Item = &'a mut Vec<ReferenceSelection>>) -> bool {
    groups.flatten().fold(false, |changed, selection| {
        let cleared = match &mut selection.target {
            ReferenceTarget::Entity(entity) => target(entity),
            ReferenceTarget::Feature(_) => true,
            ReferenceTarget::Null
            | ReferenceTarget::Drawing { .. }
            | ReferenceTarget::Annotation(_)
            | ReferenceTarget::Pmi(_)
            | ReferenceTarget::External { .. }
            | ReferenceTarget::Unresolved => false,
        };
        if matches!(selection.target, ReferenceTarget::Feature(_)) {
            selection.target = ReferenceTarget::Unresolved;
        }
        cleared | changed
    })
}
