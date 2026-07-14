// SPDX-License-Identifier: Apache-2.0
//! Product graph and placement validation.

use std::collections::{HashMap, HashSet};

use crate::document::CadIr;
use crate::products::ComponentReference;
use crate::report::{Check, Finding, Severity};

pub(super) fn check_products(ir: &CadIr, findings: &mut Vec<Finding>) {
    let components = ir
        .model
        .components
        .iter()
        .map(|component| (component.id.0.as_str(), component))
        .collect::<HashMap<_, _>>();
    let occurrences = ir
        .model
        .occurrences
        .iter()
        .map(|occurrence| (occurrence.id.0.as_str(), occurrence))
        .collect::<HashMap<_, _>>();

    for component in &ir.model.components {
        let mut children = HashSet::new();
        for child in &component.components {
            if !components.contains_key(child.0.as_str()) || !children.insert(child.0.as_str()) {
                invalid(
                    findings,
                    &component.id.0,
                    "invalid or repeated component child",
                );
            }
        }
        let mut uses = HashSet::new();
        for occurrence in &component.occurrences {
            let valid = occurrences
                .get(occurrence.0.as_str())
                .is_some_and(|occurrence| occurrence.parent.as_ref() == Some(&component.id));
            if !valid || !uses.insert(occurrence.0.as_str()) {
                invalid(
                    findings,
                    &component.id.0,
                    "invalid or repeated occurrence child",
                );
            }
        }
        if component_cycle(&component.id.0, &components) {
            invalid(findings, &component.id.0, "product component cycle");
        }
    }

    for occurrence in &ir.model.occurrences {
        let valid_prototype = match &occurrence.prototype {
            ComponentReference::Local { component } => {
                components.contains_key(component.0.as_str())
            }
            ComponentReference::External { document, .. } => !document.is_empty(),
            ComponentReference::Unresolved => true,
        };
        let valid_parent = occurrence
            .parent
            .as_ref()
            .is_none_or(|parent| components.contains_key(parent.0.as_str()));
        let finite = occurrence
            .local_transform
            .iter()
            .flatten()
            .chain(occurrence.resolved_transform.iter().flatten())
            .chain(occurrence.scale.iter())
            .all(|value| value.is_finite());
        if !valid_prototype || !valid_parent || !finite {
            invalid(
                findings,
                &occurrence.id.0,
                "invalid occurrence reference or transform",
            );
        }
    }
}

fn component_cycle(start: &str, components: &HashMap<&str, &crate::products::Component>) -> bool {
    fn visit<'a>(
        current: &'a str,
        start: &str,
        components: &HashMap<&'a str, &'a crate::products::Component>,
        seen: &mut HashSet<&'a str>,
    ) -> bool {
        components.get(current).is_some_and(|component| {
            component.components.iter().any(|child| {
                child.0 == start
                    || (seen.insert(child.0.as_str())
                        && visit(child.0.as_str(), start, components, seen))
            })
        })
    }
    visit(start, start, components, &mut HashSet::from([start]))
}

fn invalid(findings: &mut Vec<Finding>, entity: &str, message: &str) {
    findings.push(Finding {
        check: Check::ReferentialIntegrity,
        severity: Severity::Error,
        message: message.into(),
        entity: Some(entity.into()),
    });
}
