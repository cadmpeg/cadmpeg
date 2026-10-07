// SPDX-License-Identifier: Apache-2.0
//! Product graph and placement validation.

use super::orders::Orders;
use crate::index::identities::BorrowedIdentities;
use cadmpeg_core::decode::DecodeContext;
use cadmpeg_core::CodecError;

use super::record_finding;
use crate::document::CadIr;
use crate::products::{OccurrenceParent, OperandContainer, PrototypeReference};
use crate::report::check::{Check, Finding};

pub(super) fn check_products(
    ctx: &DecodeContext<'_>,
    ir: &CadIr,
    findings: &mut Vec<Finding>,
) -> Result<(), CodecError> {
    let definitions = BorrowedIdentities::build(ctx, |add| {
        for definition in ctx.admit_iter(
            ir.model.product_definitions.as_slice(),
            "product definition identity source scan",
        )? {
            add(definition.id.as_str(), ())?;
        }
        Ok(())
    })?;
    let occurrences = BorrowedIdentities::build(ctx, |add| {
        for occurrence in ctx.admit_iter(
            ir.model.occurrences.as_slice(),
            "product occurrence identity source scan",
        )? {
            add(occurrence.id.as_str(), ())?;
        }
        Ok(())
    })?;
    let bodies = BorrowedIdentities::build(ctx, |add| {
        for body in ctx.admit_iter(
            ir.model.bodies.as_slice(),
            "product body identity source scan",
        )? {
            add(body.id.as_str(), ())?;
        }
        Ok(())
    })?;
    for definition in &ir.model.product_definitions {
        ctx.charge_work(1, "product definition row")?;
        for body in &definition.bodies {
            ctx.charge_work(1, "product body reference")?;
            if !bodies.contains(ctx, body.as_str())? {
                record_finding(
                    ctx,
                    findings,
                    Check::ReferentialIntegrity,
                    crate::report::Severity::Error,
                    Some(definition.id.as_str()),
                    format_args!("invalid product body reference"),
                )?;
                break;
            }
        }
    }
    if !crate::products::assembly_graph::validate(ctx, &ir.model.occurrences)? {
        record_finding(
            ctx,
            findings,
            Check::ReferentialIntegrity,
            crate::report::Severity::Error,
            Some("model:assembly"),
            format_args!("invalid occurrence parent graph"),
        )?;
    }
    let mut root_ordinals = Orders::new(ctx)?;
    let mut sibling_ordinals: BorrowedIdentities<'_, '_, Orders<'_, '_>> =
        BorrowedIdentities::build(ctx, |_| Ok(()))?;
    for occurrence in &ir.model.occurrences {
        ctx.charge_work(1, "product occurrence row")?;
        let valid_prototype = match &occurrence.prototype {
            PrototypeReference::Local { definition } => {
                definitions.contains(ctx, definition.as_str())?
            }
            PrototypeReference::External { .. } | PrototypeReference::Unresolved {} => true,
        };
        let valid_parent = match &occurrence.parent {
            OccurrenceParent::Root {} => true,
            OccurrenceParent::Occurrence { occurrence } => {
                occurrences.contains(ctx, occurrence.as_str())?
            }
        };
        let ordinal_unique = match &occurrence.parent {
            OccurrenceParent::Root {} => root_ordinals.insert(occurrence.ordinal)?,
            OccurrenceParent::Occurrence { occurrence: parent } => {
                if let Some(orders) = sibling_ordinals.get_mut(ctx, parent.as_str())? {
                    orders.insert(occurrence.ordinal)?
                } else {
                    let mut orders = Orders::new(ctx)?;
                    let unique = orders.insert(occurrence.ordinal)?;
                    sibling_ordinals.insert_unique(parent.as_str(), orders)?;
                    unique
                }
            }
        };
        let mut auxiliary_definitions = true;
        if let Some(link) = &occurrence.link {
            ctx.charge_work(1, "product occurrence link")?;
            let element_valid = match link.element_component() {
                Some(definition) => definitions.contains(ctx, definition.as_str())?,
                None => true,
            };
            let mut copy_targets_valid = true;
            if let Some(copy) = link.copy_on_change() {
                for reference in [copy.source.as_ref(), copy.group.as_ref()]
                    .into_iter()
                    .flatten()
                {
                    ctx.charge_work(1, "product copy target")?;
                    if let PrototypeReference::Local { definition } = reference {
                        if !definitions.contains(ctx, definition.as_str())? {
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
                ctx,
                findings,
                Check::ReferentialIntegrity,
                crate::report::Severity::Error,
                Some(occurrence.id.as_str()),
                format_args!("invalid occurrence reference, ordinal, or affine transform"),
            )?;
        }
    }
    for joint in &ir.model.assembly_joints {
        ctx.charge_work(1, "assembly joint row")?;
        for connector in joint.connectors() {
            ctx.charge_work(1, "assembly joint connector")?;
            if let OperandContainer::Occurrence { occurrence } = &connector.operand.container {
                if !occurrences.contains(ctx, occurrence.as_str())? {
                    record_finding(
                        ctx,
                        findings,
                        Check::ReferentialIntegrity,
                        crate::report::Severity::Error,
                        Some(joint.id.as_str()),
                        format_args!("invalid assembly joint operands, frames, or limits"),
                    )?;
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
            transform: Transform::identity(),
            linked_prototype: None,
            scale: [crate::scalar::FiniteReal::ONE; 3],
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
}
