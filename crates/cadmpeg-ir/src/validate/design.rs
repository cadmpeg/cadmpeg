// SPDX-License-Identifier: Apache-2.0
//! Part 10 validation of the CADIR v11 design layer.
//!
//! The rules here are decided by reference lookup, counting, and in-IR
//! arithmetic over the design records, the current geometry, and the product
//! definitions. A rule about an operation's operands or names does not fire
//! on an `Unresolved` or `Native` operation.
//!
//! Every selection and construction of the design arenas is reached by a
//! typed walk of the record that holds it. A rule that reads a state checks
//! it only where the document stores it; an `Unread` selection, a `Native`
//! pick, and an operation whose operands are not stated are legal partial
//! statements that no rule about their operands checks.

mod caches;
mod configurations;
mod constructions;
mod containers;
mod dimensions;
pub(super) mod evaluations;
pub(super) mod expressions;
mod fit;
#[cfg(test)]
pub(super) mod fixture;
mod folds;
mod index;
mod joins;
mod linkage;
mod matching;
mod materials;
mod names;
mod ops;
mod overrides;
mod parameter_records;
#[cfg(test)]
mod parameter_tests;
mod parameters;
mod programs;
pub(super) mod records;
pub(super) mod resolved;
pub(super) mod selections;
mod states;
mod tolerances;
mod walk;

#[cfg(test)]
mod tests;

use cadmpeg_core::decode::DecodeContext;
use cadmpeg_core::CodecError;

use crate::design::feature::Feature;
use crate::document::CadIr;
use crate::ids::ProductDefinitionId;
use crate::index::ModelIndex;
use crate::report::{
    check::{Check, Finding},
    Severity,
};

use super::record_finding;
use index::DesignIndex;
use records::MemberKey;
use walk::{BoundDimension, Collect, ConstructionView, SelectionView, Walk};

/// The record that holds a walked selection or construction.
#[derive(Debug, Clone, Copy)]
enum Holder<'a> {
    /// A feature's definition and placement.
    Definition(&'a Feature),
    /// A sketch, sketch entity, sketch constraint, or parameter that a feature
    /// owns, by its identity.
    Owned { feature: &'a str, record: &'a str },
    /// Any other design record, with the scope it names where it names one.
    Record {
        record: &'a str,
        scope: Option<&'a ProductDefinitionId>,
    },
}

impl<'a> Holder<'a> {
    /// The holding record's identity.
    fn record(self) -> &'a str {
        match self {
            Self::Definition(feature) => feature.id.as_str(),
            Self::Owned { record, .. } | Self::Record { record, .. } => record,
        }
    }

    /// The feature whose definition holds the record, where one does.
    fn feature(self) -> Option<&'a str> {
        match self {
            Self::Definition(feature) => Some(feature.id.as_str()),
            Self::Owned { feature, .. } => Some(feature),
            Self::Record { .. } => None,
        }
    }
}

/// The selections, constructions, and bound dimensions one design record
/// holds.
struct Held<'a> {
    holder: Holder<'a>,
    selections: Vec<SelectionView<'a>>,
    constructions: Vec<ConstructionView<'a>>,
    dimensions: Vec<BoundDimension<'a>>,
}

/// Check the design layer of `ir` against part 10's scope-and-linkage,
/// container, position, evaluation, current-geometry, configuration,
/// parameter, expression, program, dimension, cache, name, selection, and
/// construction rules, and the rules of the feature vocabulary.
pub(super) fn check_design(
    ctx: &DecodeContext<'_>,
    ir: &CadIr,
    ids: &ModelIndex<'_>,
    findings: &mut Vec<Finding>,
) -> Result<(), CodecError> {
    if ir.design.is_empty() {
        linkage::check_scope(ir, findings);
        tolerances::check_definition_general_tolerances(ir, findings);
        if !ir.process.parameters.is_empty() {
            let records = records::DesignRecords::new(ir);
            expressions::check_process_parameters(&records, findings);
        }
        return Ok(());
    }
    let mut index = DesignIndex::new(ir);
    let records = &index.records;
    linkage::check_linkage(records, ids, findings);
    containers::check_containers(records, findings);
    evaluations::check_evaluations(records, ctx.policy().limits.max_state_chain_depth, findings);
    evaluations::check_current_geometry(records, findings);
    resolved::check_resolved_states(ctx, records, ids, findings)?;
    configurations::check_configurations(records, findings);
    ops::check_operations(records, findings);
    overrides::check(records, findings);
    parameter_records::check_parameter_records(records, findings);
    programs::check_programs(records, findings);
    expressions::check_expressions(ctx, records, findings)?;
    dimensions::check_dimensions(records, findings);
    tolerances::check_tolerances(records, findings);
    caches::check_caches(records, findings);

    let _depth = ctx.enter_nested("IR design validation")?;
    let held = collect(ctx, ir, &index)?;
    dimensions::check_bound_dimensions(&index.records, &held, findings);
    index.add_dependencies(&held);
    let states = states::States::new(ir);
    joins::check(&index, &states, findings);
    let matchers = matching::Matchers::new(&states);
    names::check(ctx, &index, &states, &held, findings)?;
    selections::check(ctx, &index, &matchers, &held, ids.native_view(), findings)?;
    constructions::check(&index, &held, findings);
    folds::check(&index, &matchers, &held, findings);
    materials::check(&index, &matchers, findings);
    Ok(())
}

/// Walks every design record that can hold a selection, a construction, or a
/// dimension.
fn collect<'a>(
    ctx: &DecodeContext<'_>,
    ir: &'a CadIr,
    index: &DesignIndex<'a>,
) -> Result<Vec<Held<'a>>, CodecError> {
    let design = &ir.design;
    let mut held = Vec::new();
    let mut add = |holder: Holder<'a>, record: &'a dyn Walk| -> Result<(), CodecError> {
        let mut into = Collect::new(ctx);
        record.walk(&mut into)?;
        if !into.selections.is_empty()
            || !into.constructions.is_empty()
            || !into.dimensions.is_empty()
        {
            held.push(Held {
                holder,
                selections: into.selections,
                constructions: into.constructions,
                dimensions: into.dimensions,
            });
        }
        Ok(())
    };
    let owned_by_sketch = |sketch: &'a str, record: &'a str| match index.sketches.get(sketch) {
        Some(sketch) => Holder::Owned {
            feature: sketch.owner.as_str(),
            record,
        },
        None => Holder::Record {
            record,
            scope: None,
        },
    };
    let owned_by_spatial =
        |sketch: &'a str, record: &'a str| match index.spatial_sketches.get(sketch) {
            Some(sketch) => Holder::Owned {
                feature: sketch.owner.as_str(),
                record,
            },
            None => Holder::Record {
                record,
                scope: None,
            },
        };
    for feature in &design.features {
        add(Holder::Definition(feature), feature)?;
    }
    for sketch in &design.sketches {
        let holder = Holder::Owned {
            feature: sketch.owner.as_str(),
            record: sketch.id.as_str(),
        };
        add(holder, sketch)?;
    }
    for sketch in &design.spatial_sketches {
        let holder = Holder::Owned {
            feature: sketch.owner.as_str(),
            record: sketch.id.as_str(),
        };
        add(holder, sketch)?;
    }
    for entity in &design.sketch_entities {
        add(
            owned_by_sketch(entity.sketch.as_str(), entity.id.as_str()),
            entity,
        )?;
    }
    for constraint in &design.sketch_constraints {
        add(
            owned_by_sketch(constraint.sketch.as_str(), constraint.id.as_str()),
            constraint,
        )?;
    }
    for entity in &design.spatial_sketch_entities {
        add(
            owned_by_spatial(entity.sketch.as_str(), entity.id.as_str()),
            entity,
        )?;
    }
    for constraint in &design.spatial_sketch_constraints {
        add(
            owned_by_spatial(constraint.sketch.as_str(), constraint.id.as_str()),
            constraint,
        )?;
    }
    for parameter in &design.parameters {
        let holder = match &parameter.owner {
            Some(crate::design::parameter::ParameterOwner::Feature(feature)) => Holder::Owned {
                feature: feature.as_str(),
                record: parameter.id.as_str(),
            },
            _ => Holder::Record {
                record: parameter.id.as_str(),
                scope: Some(&parameter.scope),
            },
        };
        add(holder, parameter)?;
    }
    for program in &design.programs {
        let holder = Holder::Record {
            record: program.id.as_str(),
            scope: Some(&program.scope),
        };
        add(holder, program)?;
    }
    for rule in &design.sheet_metal_rules {
        let holder = Holder::Record {
            record: rule.id.as_str(),
            scope: Some(&rule.scope),
        };
        add(holder, rule)?;
    }
    for configuration in &design.configurations {
        let holder = Holder::Record {
            record: configuration.id.as_str(),
            scope: Some(&configuration.scope),
        };
        add(holder, configuration)?;
    }
    for arrangement in &design.arrangements {
        let holder = Holder::Record {
            record: arrangement.id.as_str(),
            scope: Some(&arrangement.scope),
        };
        add(holder, arrangement)?;
    }
    for container in &design.containers {
        let id = container.id().as_str();
        let holder = Holder::Record {
            record: id,
            scope: index.scope_of(MemberKey::Container(id)),
        };
        add(holder, container)?;
    }
    for scope in &design.scopes {
        let holder = Holder::Record {
            record: scope.definition.as_str(),
            scope: Some(&scope.definition),
        };
        add(holder, scope)?;
    }
    Ok(held)
}

fn error(findings: &mut Vec<Finding>, entity: &str, message: std::fmt::Arguments<'_>) {
    record_finding(
        findings,
        Check::Design,
        Severity::Error,
        Some(entity),
        message,
    );
}

fn warning(findings: &mut Vec<Finding>, entity: &str, message: std::fmt::Arguments<'_>) {
    super::record_finding(
        findings,
        Check::Design,
        Severity::Warning,
        Some(entity),
        message,
    );
}
