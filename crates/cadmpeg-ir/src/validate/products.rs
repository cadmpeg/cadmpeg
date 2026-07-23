// SPDX-License-Identifier: Apache-2.0
//! Product graph and placement validation.

use std::collections::{HashMap, HashSet};

use super::{Check, Finding, Severity};
use crate::document::CadIr;
use crate::products::ComponentReference;

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
    let occurrence_by_native = ir
        .model
        .occurrences
        .iter()
        .filter_map(|occurrence| {
            occurrence
                .native_ref
                .as_deref()
                .map(|native| (native, occurrence))
        })
        .collect::<HashMap<_, _>>();

    for component in &ir.model.components {
        let parent_valid = component.parent.as_ref().is_none_or(|parent| {
            components
                .get(parent.0.as_str())
                .is_some_and(|parent| parent.components.iter().any(|child| child == &component.id))
        });
        let expected_transform =
            component
                .parent
                .as_ref()
                .map_or(component.local_transform, |parent| {
                    components
                        .get(parent.0.as_str())
                        .map_or(component.local_transform, |parent| {
                            multiply(parent.resolved_transform, component.local_transform)
                        })
                });
        if !parent_valid
            || !finite_transform(&component.local_transform)
            || !finite_transform(&component.resolved_transform)
            || !same_transform(&expected_transform, &component.resolved_transform)
        {
            invalid(
                findings,
                &component.id.0,
                "invalid component parent or resolved transform",
            );
        }
        let mut children = HashSet::new();
        for child in &component.components {
            let valid = components
                .get(child.0.as_str())
                .is_some_and(|child| child.parent.as_ref() == Some(&component.id));
            if !valid || !children.insert(child.0.as_str()) {
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
            .chain(occurrence.prototype_transform.iter().flatten())
            .chain(occurrence.resolved_transform.iter().flatten())
            .chain(occurrence.scale.iter())
            .all(|value| value.is_finite());
        let container_transform =
            occurrence
                .parent
                .as_ref()
                .map_or(occurrence.local_transform, |parent| {
                    components
                        .get(parent.0.as_str())
                        .map_or(occurrence.local_transform, |parent| {
                            multiply(parent.resolved_transform, occurrence.local_transform)
                        })
                });
        let expected_transform = multiply(container_transform, occurrence.prototype_transform);
        if !valid_prototype
            || !valid_parent
            || !auxiliary_components
            || !finite
            || !same_transform(&expected_transform, &occurrence.resolved_transform)
        {
            invalid(
                findings,
                &occurrence.id.0,
                "invalid occurrence reference or transform",
            );
        }
        if occurrence_prototype_cycle(
            occurrence.id.0.as_str(),
            &occurrences,
            &components,
            &occurrence_by_native,
        ) {
            invalid(
                findings,
                &occurrence.id.0,
                "product occurrence prototype cycle",
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
            && (joint.offset_frames.is_empty() || joint.offset_frames.len() == expected)
            && joint.operands.iter().all(|operand| {
                let external_valid = operand.external_document.as_ref().is_none_or(|document| {
                    document.path.is_some() ^ document.document_id.is_some()
                });
                let resolution_valid = match &operand.external_document {
                    Some(_) => operand.component.is_none(),
                    None => operand.component.is_some(),
                };
                operand.object.is_some()
                    && resolution_valid
                    && operand
                        .component
                        .as_ref()
                        .is_none_or(|component| components.contains_key(component.0.as_str()))
                    && external_valid
            });
        let finite = joint
            .frames
            .iter()
            .flatten()
            .flatten()
            .chain(joint.offset_frames.iter().flatten().flatten())
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

fn occurrence_prototype_cycle<'a>(
    start: &str,
    occurrences: &HashMap<&'a str, &'a crate::products::Occurrence>,
    components: &HashMap<&'a str, &'a crate::products::Component>,
    occurrence_by_native: &HashMap<&'a str, &'a crate::products::Occurrence>,
) -> bool {
    let mut current = start;
    let mut seen = HashSet::from([start]);
    loop {
        let Some(occurrence) = occurrences.get(current) else {
            return false;
        };
        let ComponentReference::Local { component } = &occurrence.prototype else {
            return false;
        };
        let Some(target) = components
            .get(component.0.as_str())
            .and_then(|component| component.native_ref.as_deref())
            .and_then(|native| occurrence_by_native.get(native))
        else {
            return false;
        };
        current = target.id.0.as_str();
        if current == start {
            return true;
        }
        if !seen.insert(current) {
            return false;
        }
    }
}

fn finite_transform(transform: &[[f64; 4]; 4]) -> bool {
    transform.iter().flatten().all(|value| value.is_finite())
}

fn multiply(left: [[f64; 4]; 4], right: [[f64; 4]; 4]) -> [[f64; 4]; 4] {
    std::array::from_fn(|row| {
        std::array::from_fn(|column| {
            (0..4)
                .map(|index| left[row][index] * right[index][column])
                .sum()
        })
    })
}

fn same_transform(left: &[[f64; 4]; 4], right: &[[f64; 4]; 4]) -> bool {
    left.iter()
        .flatten()
        .zip(right.iter().flatten())
        .all(|(left, right)| {
            let scale = left.abs().max(right.abs()).max(1.0);
            (left - right).abs() <= scale * 1e-12
        })
}

fn component_cycle(start: &str, components: &HashMap<&str, &crate::products::Component>) -> bool {
    let mut seen = HashSet::from([start]);
    let mut stack = vec![start];
    while let Some(current) = stack.pop() {
        let Some(component) = components.get(current) else {
            continue;
        };
        for child in &component.components {
            if child.0 == start {
                return true;
            }
            if seen.insert(child.0.as_str()) {
                stack.push(child.0.as_str());
            }
        }
    }
    false
}

fn invalid(findings: &mut Vec<Finding>, entity: &str, message: &str) {
    findings.push(Finding {
        check: Check::ReferentialIntegrity,
        severity: Severity::Error,
        message: message.into(),
        entity: Some(entity.into()),
    });
}
