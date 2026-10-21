// SPDX-License-Identifier: Apache-2.0
//! Source-spelling and distinct-edge views for tokenized parameter assertions.

use cadmpeg_ir::features::{DesignParameter, DistinctMembers, ParameterId};
use cadmpeg_ir::CadIr;

pub(crate) fn dependencies(parameter: &DesignParameter) -> DistinctMembers<ParameterId> {
    parameter.expression.dependencies().cloned().collect()
}

pub(crate) fn expression_text(ir: &CadIr, parameter: &DesignParameter) -> String {
    if let Some(text) = parameter.expression.literal_text() {
        return text;
    }
    let native =
        crate::native::CatiaNative::load(ir.native.namespace("catia").expect("CATIA namespace"))
            .expect("typed CATIA namespace");
    let mut names = std::collections::BTreeMap::new();
    for entity in &native.entity_records {
        let dependencies = if let Some(formula) = entity.formula_relation().filter(|formula| {
            formula.output_entity.reference.entity() == parameter.native_ref.as_deref()
        }) {
            Some(formula.parameter_dependencies.as_slice())
        } else {
            entity
                .relation_program_instance()
                .filter(|program| {
                    program.output_entity().and_then(|target| target.entity())
                        == parameter.native_ref.as_deref()
                })
                .map(|program| program.parameter_dependencies.as_slice())
        };
        for dependency in dependencies.into_iter().flatten() {
            let [target] = dependency.candidates.as_slice() else {
                continue;
            };
            if let Some(target) = target.entity().and_then(|target| {
                ir.model
                    .parameters
                    .iter()
                    .find(|candidate| candidate.native_ref.as_deref() == Some(target))
            }) {
                names.insert(target.id.clone(), dependency.symbol.clone());
            }
        }
    }
    parameter
        .expression
        .render_with(|id| {
            names.get(id).cloned().or_else(|| {
                ir.model
                    .parameters
                    .iter()
                    .find(|parameter| &parameter.id == id)
                    .map(|parameter| parameter.name.clone())
            })
        })
        .expect("all expression references resolve")
}
