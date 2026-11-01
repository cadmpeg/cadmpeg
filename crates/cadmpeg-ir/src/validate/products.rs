// SPDX-License-Identifier: Apache-2.0
//! Product graph and placement validation.

use crate::index::identities::BorrowedIdentities;
use cadmpeg_core::decode::DecodeContext;
use cadmpeg_core::CodecError;

use super::record_finding;
use crate::document::CadIr;
use crate::products::{
    JointOperands, OccurrenceParent, OperandContainer, PairedJointKind, PrototypeReference,
};
use crate::report::check::{Check, Finding};

pub(super) fn check_products(
    ctx: &DecodeContext<'_>,
    ir: &CadIr,
    findings: &mut Vec<Finding>,
) -> Result<(), CodecError> {
    let definitions = BorrowedIdentities::build(|add| {
        for definition in &ir.model.product_definitions {
            add(definition.id.as_str(), ());
        }
    });
    for (id, owner) in ir
        .model
        .features
        .iter()
        .map(|row| (row.id.as_str(), row.product_definition.as_ref()))
        .chain(
            ir.model
                .parameters
                .iter()
                .map(|row| (row.id.as_str(), row.product_definition.as_ref())),
        )
    {
        if let Some(owner) = owner {
            if !definitions.contains(owner.as_str()) {
                record_finding(
                    findings,
                    Check::ReferentialIntegrity,
                    crate::report::Severity::Error,
                    Some(id),
                    format_args!("invalid product definition owner"),
                );
            }
        }
    }
    for feature in &ir.model.features {
        if let Some(owner) = &feature.product_definition {
            if let Some(definition) = ir
                .model
                .product_definitions
                .iter()
                .find(|row| &row.id == owner)
            {
                if feature
                    .evaluation
                    .outputs()
                    .iter()
                    .any(|body| !definition.bodies.contains(body))
                {
                    record_finding(
                        findings,
                        Check::ReferentialIntegrity,
                        crate::report::Severity::Error,
                        Some(feature.id.as_str()),
                        format_args!("feature output is not owned by its product definition"),
                    );
                }
            }
        }
    }
    let occurrences = BorrowedIdentities::build(|add| {
        for occurrence in &ir.model.occurrences {
            add(occurrence.id.as_str(), ());
        }
    });
    let parameters = BorrowedIdentities::build(|add| {
        for parameter in &ir.model.parameters {
            add(parameter.id.as_str(), ());
        }
    });
    let joints = BorrowedIdentities::build(|add| {
        for joint in &ir.model.assembly_joints {
            add(joint.id.as_str(), ());
        }
    });
    let bodies = BorrowedIdentities::build(|add| {
        for body in &ir.model.bodies {
            add(body.id.as_str(), ());
        }
    });
    let sketches = BorrowedIdentities::build(|add| {
        for sketch in &ir.model.sketches {
            add(sketch.id.as_str(), ());
        }
    });
    let mut sketch_owners = std::collections::BTreeMap::new();
    for definition in &ir.model.product_definitions {
        for sketch in &definition.sketches {
            let message = if !sketches.contains(sketch.as_str()) {
                Some("invalid product sketch reference")
            } else if let Some(owner) = sketch_owners.insert(sketch, &definition.id) {
                Some(if owner == &definition.id {
                    "duplicate product sketch reference"
                } else {
                    "sketch has multiple product definitions"
                })
            } else {
                None
            };
            if let Some(message) = message {
                record_finding(
                    findings,
                    Check::ReferentialIntegrity,
                    crate::report::Severity::Error,
                    Some(definition.id.as_str()),
                    format_args!("{message}"),
                );
                break;
            }
        }
        for body in &definition.bodies {
            if !bodies.contains(body.as_str()) {
                record_finding(
                    findings,
                    Check::ReferentialIntegrity,
                    crate::report::Severity::Error,
                    Some(definition.id.as_str()),
                    format_args!("invalid product body reference"),
                );
                break;
            }
        }
    }
    if !crate::products::assembly_graph::validate(ctx, &ir.model.occurrences)? {
        record_finding(
            findings,
            Check::ReferentialIntegrity,
            crate::report::Severity::Error,
            Some("model:assembly"),
            format_args!("invalid occurrence parent graph"),
        );
    }
    let mut root_ordinals = std::collections::BTreeSet::new();
    let mut sibling_ordinals: BorrowedIdentities<'_, std::collections::BTreeSet<u32>> =
        BorrowedIdentities::new();
    for occurrence in &ir.model.occurrences {
        let valid_prototype = match &occurrence.prototype {
            PrototypeReference::Local { definition } => definitions.contains(definition.as_str()),
            PrototypeReference::External { .. } | PrototypeReference::Unresolved {} => true,
        };
        let valid_parent = match &occurrence.parent {
            OccurrenceParent::Root {} => true,
            OccurrenceParent::Occurrence { occurrence } => {
                occurrences.contains(occurrence.as_str())
            }
        };
        let ordinal_unique = match &occurrence.parent {
            OccurrenceParent::Root {} => root_ordinals.insert(occurrence.ordinal),
            OccurrenceParent::Occurrence { occurrence: parent } => {
                if let Some(orders) = sibling_ordinals.get_mut(parent.as_str()) {
                    orders.insert(occurrence.ordinal)
                } else {
                    let mut orders = std::collections::BTreeSet::new();
                    let unique = orders.insert(occurrence.ordinal);
                    sibling_ordinals.insert_unique(parent.as_str(), orders);
                    unique
                }
            }
        };
        let mut auxiliary_definitions = true;
        if let Some(link) = &occurrence.link {
            let element_valid = match link.element_component() {
                Some(definition) => definitions.contains(definition.as_str()),
                None => true,
            };
            let mut copy_targets_valid = true;
            if let Some(copy) = link.copy_on_change() {
                for reference in [copy.source.as_ref(), copy.group.as_ref()]
                    .into_iter()
                    .flatten()
                {
                    if let PrototypeReference::Local { definition } = reference {
                        if !definitions.contains(definition.as_str()) {
                            copy_targets_valid = false;
                            break;
                        }
                    }
                }
            }
            auxiliary_definitions = element_valid && copy_targets_valid;
        }
        if !valid_prototype || !valid_parent || !ordinal_unique || !auxiliary_definitions {
            record_finding(
                findings,
                Check::ReferentialIntegrity,
                crate::report::Severity::Error,
                Some(occurrence.id.as_str()),
                format_args!("invalid occurrence reference, ordinal, or affine transform"),
            );
        }
    }
    for joint in &ir.model.assembly_joints {
        // A kind that states its own sense leaves no room for another.
        if let (Some(kind_sense), Some(frame_sense)) = (joint.kind_sense(), joint.frame_sense()) {
            if kind_sense != frame_sense {
                record_finding(
                    findings,
                    Check::ReferentialIntegrity,
                    crate::report::Severity::Error,
                    Some(joint.id.as_str()),
                    format_args!("assembly joint frame sense contradicts its kind's sense"),
                );
            }
        }
        if let Some(parameter) = &joint.driving_parameter {
            if !parameters.contains(parameter.as_str()) {
                record_finding(
                    findings,
                    Check::ReferentialIntegrity,
                    crate::report::Severity::Error,
                    Some(joint.id.as_str()),
                    format_args!("invalid assembly joint driving parameter"),
                );
            }
        }
        if let JointOperands::Pair {
            kind:
                PairedJointKind::Gears { coupled_joints, .. }
                | PairedJointKind::RackPinion { coupled_joints, .. },
            ..
        } = joint.operands()
        {
            let resolved = coupled_joints
                .iter()
                .all(|coupled| coupled != &joint.id && joints.contains(coupled.as_str()));
            if !matches!(coupled_joints.len(), 0 | 2) || !resolved {
                record_finding(
                    findings,
                    Check::ReferentialIntegrity,
                    crate::report::Severity::Error,
                    Some(joint.id.as_str()),
                    format_args!("assembly joint couples other than two other joints"),
                );
            }
        }
        for connector in joint.connectors() {
            if let OperandContainer::Occurrence { occurrence, .. } = &connector.operand.container {
                if !occurrences.contains(occurrence.as_str()) {
                    record_finding(
                        findings,
                        Check::ReferentialIntegrity,
                        crate::report::Severity::Error,
                        Some(joint.id.as_str()),
                        format_args!("invalid assembly joint operands, frames, or limits"),
                    );
                    break;
                }
            }
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    mod admission;
    mod history_owners;
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

    /// A kind that states its own sense requires the frame sense to agree.
    #[test]
    fn a_frame_sense_must_agree_with_its_kinds_sense() {
        use crate::products::PlaneSense;
        let findings = |frame_sense| {
            let mut ir = CadIr::empty();
            let mut joint = AssemblyJoint::paired(
                JointId::mint("test:model:joint#plane").expect("identity grammar"),
                PairedJointKind::PlaneCoincidence {
                    sense: PlaneSense::Opposed,
                    offset: None,
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
            );
            assert!(joint.set_frame_sense(frame_sense));
            ir.model.assembly_joints.push(joint);
            let mut findings = Vec::new();
            check_products(
                &cadmpeg_test_support::service_decode_context(),
                &ir,
                &mut findings,
            )
            .unwrap();
            findings
        };
        assert!(findings(None).is_empty());
        assert!(findings(Some(PlaneSense::Opposed)).is_empty());
        let contradicted = findings(Some(PlaneSense::Aligned));
        assert_eq!(contradicted.len(), 1);
        assert_eq!(
            contradicted[0].message,
            "assembly joint frame sense contradicts its kind's sense"
        );
    }

    #[test]
    fn joint_operands_allow_document_root_and_reject_two_qualifiers() {
        let mut ir = CadIr::empty();
        ir.model.assembly_joints.push(AssemblyJoint::paired(
            JointId::mint("test:model:joint#root").expect("identity grammar"),
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
        check_products(
            &cadmpeg_test_support::service_decode_context(),
            &ir,
            &mut findings,
        )
        .unwrap();
        assert!(findings.is_empty(), "{findings:?}");

        let occurrence =
            OccurrenceId::mint("test:model:occurrence#placed").expect("valid identity");
        ir.model.occurrences.push(Occurrence {
            id: occurrence.clone(),
            prototype: PrototypeReference::Unresolved {},
            parent: OccurrenceParent::Root {},
            ordinal: 0,
            transform: Some(Transform::identity()),
            linked_prototype: None,
            name: None,
            visible: None,
            link: None,
            native_ref: None,
        });
        let mut wire = serde_json::to_value(&ir.model.assembly_joints[0]).expect("joint wire");
        wire["operands"]["connectors"][0]["operand"]["occurrence"] =
            serde_json::json!(occurrence.as_str());
        wire["operands"]["connectors"][0]["operand"]["external_document"] = serde_json::json!({
            "path": "external.f3d",
            "resolution": "unresolved"
        });
        assert!(serde_json::from_value::<AssemblyJoint>(wire).is_err());
    }

    #[test]
    fn a_gear_couples_two_other_existing_joints() {
        let mut ir = CadIr::empty();
        let gear = |coupled: Vec<JointId>| {
            AssemblyJoint::paired(
                JointId::mint("test:model:joint#gear").expect("identity grammar"),
                PairedJointKind::Gears {
                    distance: None,
                    distance2: None,
                    ratio: crate::scalar::PositiveReal::new(2.0),
                    coupled_joints: coupled,
                },
                [
                    JointConnector {
                        operand: JointOperand::root("/", Vec::new()),
                        frame: Transform::identity(),
                        detached: false,
                    },
                    JointConnector {
                        operand: JointOperand::root("/", Vec::new()),
                        frame: Transform::identity(),
                        detached: false,
                    },
                ],
                None,
            )
        };
        let hinge = |name: &str| {
            JointId::mint(format!("test:model:joint#{name}")).expect("identity grammar")
        };
        let findings = |ir: &CadIr| {
            let mut findings = Vec::new();
            check_products(
                &cadmpeg_test_support::service_decode_context(),
                ir,
                &mut findings,
            )
            .unwrap();
            findings.iter().any(|finding| {
                finding
                    .message
                    .contains("couples other than two other joints")
            })
        };
        ir.model
            .assembly_joints
            .push(gear(vec![hinge("a"), hinge("b")]));
        assert!(findings(&ir));
        for name in ["a", "b"] {
            let mut other = gear(Vec::new());
            other.id = hinge(name);
            ir.model.assembly_joints.push(other);
        }
        assert!(!findings(&ir));
        ir.model.assembly_joints[0] = gear(vec![hinge("a")]);
        assert!(findings(&ir));
    }

    #[test]
    fn a_joint_driving_parameter_must_name_a_design_parameter() {
        let mut ir = CadIr::empty();
        let mut joint = AssemblyJoint::paired(
            JointId::mint("test:model:joint#driven").expect("identity grammar"),
            PairedJointKind::AxisCoincidence {},
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
        );
        joint.driving_parameter = Some(
            crate::features::ParameterId::mint("test:model:parameter#absent")
                .expect("identity grammar"),
        );
        ir.model.assembly_joints.push(joint);
        let mut findings = Vec::new();
        check_products(
            &cadmpeg_test_support::service_decode_context(),
            &ir,
            &mut findings,
        )
        .unwrap();
        assert_eq!(findings.len(), 1, "{findings:?}");
        assert!(findings[0]
            .message
            .contains("invalid assembly joint driving parameter"));
    }
}
