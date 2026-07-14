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
            ComponentReference::External { document, .. } => {
                document.path.is_some() ^ document.document_id.is_some()
            }
            ComponentReference::Unresolved => true,
        };
        let valid_parent = occurrence
            .parent
            .as_ref()
            .is_none_or(|parent| components.contains_key(parent.0.as_str()));
        let auxiliary_components = [
            occurrence.element_component.as_ref(),
            occurrence.copy_on_change_source.as_ref(),
            occurrence.copy_on_change_group.as_ref(),
        ]
        .into_iter()
        .flatten()
        .all(|component| components.contains_key(component.0.as_str()));
        let finite = occurrence
            .local_transform
            .iter()
            .flatten()
            .chain(occurrence.resolved_transform.iter().flatten())
            .chain(occurrence.scale.iter())
            .all(|value| value.is_finite());
        if !valid_prototype || !valid_parent || !auxiliary_components || !finite {
            invalid(
                findings,
                &occurrence.id.0,
                "invalid occurrence reference or transform",
            );
        }
    }

    for joint in &ir.model.assembly_joints {
        let expected = if joint.kind == crate::products::JointKind::Grounded {
            1
        } else {
            2
        };
        let operands_valid = joint.operands.len() == expected
            && joint.frames.len() == expected
            && joint.operands.iter().all(|operand| {
                let external_valid = operand.external_document.as_ref().is_none_or(|document| {
                    document.path.is_some() ^ document.document_id.is_some()
                });
                operand
                    .component
                    .as_ref()
                    .is_none_or(|component| components.contains_key(component.0.as_str()))
                    && (operand.external_document.is_some() || operand.object.is_some())
                    && external_valid
            });
        let finite = joint
            .frames
            .iter()
            .flatten()
            .flatten()
            .copied()
            .chain(joint.angle)
            .chain(joint.distance)
            .chain(joint.distance2)
            .chain(
                joint
                    .angular_limits
                    .iter()
                    .chain(joint.linear_limits.iter())
                    .flat_map(|limits| [limits.minimum, limits.maximum])
                    .flatten(),
            )
            .all(f64::is_finite);
        let ordered = [joint.angular_limits.as_ref(), joint.linear_limits.as_ref()]
            .into_iter()
            .flatten()
            .all(|limits| match (limits.minimum, limits.maximum) {
                (Some(minimum), Some(maximum)) => minimum <= maximum,
                _ => true,
            });
        if !operands_valid || !finite || !ordered {
            invalid(
                findings,
                &joint.id.0,
                "invalid assembly joint operands, frames, or limits",
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
