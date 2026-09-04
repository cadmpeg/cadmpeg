// SPDX-License-Identifier: Apache-2.0
//! Product graph and placement validation.

use std::collections::{HashMap, HashSet};

use crate::document::CadIr;
use crate::products::{AssemblyGraph, OccurrenceParent, OperandContainer, PrototypeReference};
use crate::report::{Check, Finding, Severity};

pub(super) fn check_products(ir: &CadIr, findings: &mut Vec<Finding>) {
    let definitions = ir
        .model
        .product_definitions
        .iter()
        .map(|definition| (definition.id.as_str(), definition))
        .collect::<HashMap<_, _>>();
    let occurrences = ir
        .model
        .occurrences
        .iter()
        .map(|occurrence| (occurrence.id.as_str(), occurrence))
        .collect::<HashMap<_, _>>();
    let bodies = ir
        .model
        .bodies
        .iter()
        .map(|body| body.id.as_str())
        .collect::<HashSet<_>>();

    for definition in &ir.model.product_definitions {
        if definition
            .bodies
            .iter()
            .any(|body| !bodies.contains(body.as_str()))
        {
            invalid(
                findings,
                definition.id.as_str(),
                "invalid product body reference",
            );
        }
    }

    if AssemblyGraph::new(&ir.model.occurrences).is_err() {
        invalid(
            findings,
            "model:assembly",
            "invalid occurrence parent graph",
        );
    }
    let mut sibling_ordinals = HashSet::new();
    for occurrence in &ir.model.occurrences {
        let valid_prototype = match &occurrence.prototype {
            PrototypeReference::Local { definition } => {
                definitions.contains_key(definition.as_str())
            }
            PrototypeReference::External { .. } => true,
            PrototypeReference::Unresolved => true,
        };
        let valid_parent = match &occurrence.parent {
            OccurrenceParent::Root => true,
            OccurrenceParent::Occurrence { occurrence } => {
                occurrences.contains_key(occurrence.as_str())
            }
        };
        let parent_key = match &occurrence.parent {
            OccurrenceParent::Root => None,
            OccurrenceParent::Occurrence { occurrence } => Some(occurrence.as_str()),
        };
        let ordinal_unique = sibling_ordinals.insert((parent_key, occurrence.ordinal));
        let auxiliary_definitions = occurrence.link.as_ref().is_none_or(|link| {
            [
                link.element_component.as_ref(),
                link.copy_on_change
                    .as_ref()
                    .and_then(|copy| copy.source.as_ref()),
                link.copy_on_change
                    .as_ref()
                    .and_then(|copy| copy.group.as_ref()),
            ]
            .into_iter()
            .flatten()
            .all(|definition| definitions.contains_key(definition.as_str()))
        });
        let affine = occurrence.scale.iter().all(|value| value.is_finite());
        if !valid_prototype || !valid_parent || !ordinal_unique || !auxiliary_definitions || !affine
        {
            invalid(
                findings,
                occurrence.id.as_str(),
                "invalid occurrence reference, ordinal, or affine transform",
            );
        }
    }

    for joint in &ir.model.assembly_joints {
        let operands_valid =
            joint
                .connectors()
                .all(|connector| match &connector.operand.container {
                    OperandContainer::Occurrence(occurrence) => {
                        occurrences.contains_key(occurrence.as_str())
                    }
                    OperandContainer::Root | OperandContainer::External(_) => true,
                });
        let finite = joint
            .angle()
            .into_iter()
            .chain(joint.translation_offset().into_iter().flatten())
            .chain(joint.distance())
            .chain(joint.distance2())
            .chain(
                joint
                    .angular_limits()
                    .into_iter()
                    .chain(joint.linear_limits())
                    .flat_map(|limits| [limits.minimum(), limits.maximum()])
                    .flatten(),
            )
            .all(f64::is_finite);
        let ordered = [joint.angular_limits(), joint.linear_limits()]
            .into_iter()
            .flatten()
            .all(|limits| match (limits.minimum(), limits.maximum()) {
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

fn invalid(findings: &mut Vec<Finding>, entity: &str, message: &str) {
    findings.push(Finding {
        check: Check::ReferentialIntegrity,
        severity: Severity::Error,
        message: message.into(),
        entity: Some(entity.into()),
    });
}

#[cfg(test)]
mod tests {
    use super::check_products;
    use crate::document::CadIr;
    use crate::ids::OccurrenceId;
    use crate::products::{
        AssemblyJoint, JointConnector, JointId, JointOperand, Occurrence, OccurrenceParent,
        PairedJointKind, PrototypeReference,
    };
    use crate::transform::Transform;

    fn root_operand(object: &str) -> JointOperand {
        JointOperand::root(object, Vec::new())
    }

    #[test]
    fn joint_operands_allow_document_root_and_reject_two_qualifiers() {
        let mut ir = CadIr::empty();
        ir.model.assembly_joints.push(AssemblyJoint::paired(
            JointId("test:model:joint#root".into()),
            PairedJointKind::Fixed {
                angle: None,
                translation_offset: None,
                angular_limits: None,
                linear_limits: None,
            },
            [
                JointConnector {
                    operand: root_operand("root:first"),
                    frame: Transform::identity(),
                    detached: false,
                },
                JointConnector {
                    operand: root_operand("root:second"),
                    frame: Transform::identity(),
                    detached: false,
                },
            ],
            None,
        ));

        let mut findings = Vec::new();
        check_products(&ir, &mut findings);
        assert!(findings.is_empty(), "{findings:?}");

        let occurrence = OccurrenceId("test:model:occurrence#placed".into());
        ir.model.occurrences.push(Occurrence {
            id: occurrence.clone(),
            prototype: PrototypeReference::Unresolved,
            parent: OccurrenceParent::Root,
            ordinal: 0,
            transform: Transform::identity(),
            linked_prototype: None,
            scale: [1.0; 3],
            name: None,
            visible: None,
            link: None,
            native_ref: None,
        });
        let mut wire = serde_json::to_value(&ir.model.assembly_joints[0]).expect("joint wire");
        wire["operands"][0]["occurrence"] = serde_json::json!(occurrence.0);
        wire["operands"][0]["external_document"] = serde_json::json!({
            "path": "external.f3d",
            "resolution": "unresolved"
        });
        assert!(serde_json::from_value::<AssemblyJoint>(wire).is_err());
    }
}
