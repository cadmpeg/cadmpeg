// SPDX-License-Identifier: Apache-2.0
//! Product graph and placement validation.

use crate::index::identities::BorrowedIdentities;
use cadmpeg_core::decode::DecodeContext;
use cadmpeg_core::CodecError;

use std::collections::{BTreeMap, BTreeSet};

use super::{record_finding, record_related_finding};

pub(in crate::validate) mod decode_set;
pub(in crate::validate) mod definitions;
use crate::document::CadIr;
use crate::products::{JointOperands, OperandContainer, PairedJointKind};
use crate::report::check::{Check, Finding};
use crate::report::Severity;

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
    decode_set::check_component_records(ir, findings);
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
                    crate::rule!(
                        "products.invalid-product-definition-owner",
                        "invalid product definition owner"
                    ),
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
                        crate::rule!(
                            "products.feature-output-not-owned-product-definition",
                            "feature output is not owned by its product definition"
                        ),
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
        for body in &ir.geometry.bodies {
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
                    crate::rule!("products.sketch-owning-definition", "{message}"),
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
                    crate::rule!(
                        "products.invalid-product-body-reference",
                        "invalid product body reference"
                    ),
                );
                break;
            }
        }
    }
    let mut owners: BorrowedIdentities<'_, &str> = BorrowedIdentities::new();
    for definition in &ir.model.product_definitions {
        for occurrence in &definition.occurrences {
            let message = if !occurrences.contains(occurrence.as_str()) {
                Some("invalid product occurrence reference")
            } else if owners.get(occurrence.as_str()).is_some() {
                Some("occurrence has more than one owning definition")
            } else {
                owners.insert_unique(occurrence.as_str(), definition.id.as_str());
                None
            };
            if let Some(message) = message {
                record_finding(
                    findings,
                    Check::ReferentialIntegrity,
                    crate::report::Severity::Error,
                    Some(definition.id.as_str()),
                    crate::rule!("products.occurrence-owning-definition", "{message}"),
                );
                break;
            }
        }
    }
    for occurrence in &ir.model.occurrences {
        let message = if !definitions.contains(occurrence.prototype.as_str()) {
            Some("invalid occurrence prototype")
        } else if owners.get(occurrence.id.as_str()).is_none() {
            Some("occurrence has no owning definition")
        } else {
            None
        };
        if let Some(message) = message {
            record_finding(
                findings,
                Check::ReferentialIntegrity,
                crate::report::Severity::Error,
                Some(occurrence.id.as_str()),
                crate::rule!("products.occurrence-prototype-and-owner", "{message}"),
            );
        }
    }
    check_root_definition(ir, findings);
    check_body_owners(ir, findings);
    if let Some(definition) = placement_cycle(ctx, ir)? {
        record_finding(
            findings,
            Check::ReferentialIntegrity,
            crate::report::Severity::Error,
            Some(definition),
            crate::rule!(
                "products.product-definition-places-itself",
                "product definition is the prototype of an occurrence it owns, directly or transitively"
            ),
        );
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
                    crate::rule!(
                        "products.assembly-joint-frame-sense-contradicts-kind",
                        "assembly joint frame sense contradicts its kind's sense"
                    ),
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
                    crate::rule!(
                        "products.invalid-assembly-joint-driving-parameter",
                        "invalid assembly joint driving parameter"
                    ),
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
                    crate::rule!(
                        "products.assembly-joint-couples-other-two-other",
                        "assembly joint couples other than two other joints"
                    ),
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
                        crate::rule!(
                            "products.invalid-assembly-joint-operands-frames-limits",
                            "invalid assembly joint operands, frames, or limits"
                        ),
                    );
                    break;
                }
            }
        }
    }
    definitions::check(ir, findings);
    Ok(())
}

/// Part 10: where `model` is in scope and the document is not a fragment, a
/// document that holds a model record holds exactly one definition that is
/// `File` with `source: None`, the root definition, and no occurrence places
/// it (§8.1). A fragment's root can lie outside it. The counts are not in the
/// decoder admission sets, which judge candidate parts of a document.
fn check_root_definition(ir: &CadIr, findings: &mut Vec<Finding>) {
    let roots = ir
        .model
        .product_definitions
        .iter()
        .filter(|definition| definition.is_root())
        .collect::<Vec<_>>();
    match roots.split_first() {
        None if !ir.fragment
            && ir.scope().holds(cadmpeg_core::decode::ArenaClass::Model)
            && ir
                .populated_arena(cadmpeg_core::decode::ArenaClass::Model)
                .is_some() =>
        {
            record_finding(
                findings,
                Check::Counts,
                crate::report::Severity::Error,
                None,
                crate::rule!(
                    "products.document-states-no-root-definition",
                    "document states no root definition"
                ),
            );
        }
        None => {}
        Some((first, extras)) => {
            for extra in extras {
                super::record_related_finding(
                    findings,
                    Check::Counts,
                    crate::report::Severity::Error,
                    extra.id.as_str(),
                    &[first.id.as_str()],
                    crate::rule!(
                        "products.document-states-more-one-root-definition",
                        "document states more than one root definition"
                    ),
                );
            }
        }
    }
    for occurrence in &ir.model.occurrences {
        if let Some(root) = roots.iter().find(|root| root.id == occurrence.prototype) {
            super::record_related_finding(
                findings,
                Check::ReferentialIntegrity,
                crate::report::Severity::Error,
                root.id.as_str(),
                &[occurrence.id.as_str()],
                crate::rule!(
                    "products.occurrence-places-root-definition",
                    "an occurrence places the root definition"
                ),
            );
        }
    }
}

/// Part 10: every body of `CadIr.geometry` is listed by exactly one
/// definition's `bodies` and by no `instance_bodies` entry, or by
/// `instance_bodies` entries and by no definition's `bodies` (§8.1). The rule
/// holds where the root rule holds, since a body the file holds outside every
/// other definition is the root definition's.
fn check_body_owners(ir: &CadIr, findings: &mut Vec<Finding>) {
    if ir.fragment
        || !ir.scope().holds(cadmpeg_core::decode::ArenaClass::Model)
        || ir
            .populated_arena(cadmpeg_core::decode::ArenaClass::Model)
            .is_none()
    {
        return;
    }
    let mut listing: BTreeMap<&str, BTreeSet<&str>> = BTreeMap::new();
    let mut instanced: BTreeSet<&str> = BTreeSet::new();
    for definition in &ir.model.product_definitions {
        for body in &definition.bodies {
            listing
                .entry(body.as_str())
                .or_default()
                .insert(definition.id.as_str());
        }
        for entry in &definition.instance_bodies {
            instanced.extend(entry.bodies.iter().map(crate::ids::BodyId::as_str));
        }
    }
    for body in &ir.geometry.bodies {
        let id = body.id.as_str();
        let owners: Vec<&str> = listing
            .get(id)
            .map(|owners| owners.iter().copied().collect())
            .unwrap_or_default();
        if owners.len() > 1 {
            record_related_finding(
                findings,
                Check::ReferentialIntegrity,
                Severity::Error,
                id,
                &owners,
                crate::rule!(
                    "products.body-listed-several-definitions",
                    "body is listed by {} definitions' `bodies`; one lists it",
                    owners.len()
                ),
            );
        } else if owners.len() == 1 && instanced.contains(id) {
            let owner = owners[0];
            record_related_finding(
                findings,
                Check::ReferentialIntegrity,
                Severity::Error,
                id,
                &[owner],
                crate::rule!(
                    "products.body-listed-definition-and-instance-bodies",
                    "body is listed by definition {owner}'s `bodies` and by an `instance_bodies` entry; a body an entry shows is listed by no definition's `bodies`"
                ),
            );
        } else if owners.is_empty() && !instanced.contains(id) {
            record_finding(
                findings,
                Check::ReferentialIntegrity,
                Severity::Error,
                Some(id),
                crate::rule!(
                    "products.body-listed-no-definition",
                    "body is listed by no definition's `bodies` and by no `instance_bodies` entry"
                ),
            );
        }
    }
}

/// A definition that places itself through its occurrences, if any.
///
/// Definitions whose occurrences place only definitions already cleared are
/// cleared in turn; whatever is left is on or behind a cycle, and walking
/// that many placements through what is left ends on one.
fn placement_cycle<'ir>(
    ctx: &DecodeContext<'_>,
    ir: &'ir CadIr,
) -> Result<Option<&'ir str>, CodecError> {
    let definitions = &ir.model.product_definitions;
    let position = BorrowedIdentities::build(|add| {
        for (index, definition) in definitions.iter().enumerate() {
            add(definition.id.as_str(), index);
        }
    });
    let prototype = BorrowedIdentities::build(|add| {
        for occurrence in &ir.model.occurrences {
            add(occurrence.id.as_str(), occurrence.prototype.as_str());
        }
    });
    let mut pending = ctx.alloc_filled(definitions.len(), 0_usize, "product placement order")?;
    let mut placed_by: Vec<Vec<usize>> = definitions.iter().map(|_| Vec::new()).collect();
    let mut places: Vec<Vec<usize>> = definitions.iter().map(|_| Vec::new()).collect();
    for (owner, definition) in definitions.iter().enumerate() {
        for occurrence in &definition.occurrences {
            let Some(placed) = prototype
                .get(occurrence.as_str())
                .and_then(|prototype| position.get(prototype))
            else {
                continue;
            };
            pending[owner] += 1;
            placed_by[*placed].push(owner);
            places[owner].push(*placed);
        }
    }
    let mut cleared: Vec<usize> = (0..definitions.len())
        .filter(|&index| pending[index] == 0)
        .collect();
    while let Some(index) = cleared.pop() {
        for &owner in &placed_by[index] {
            pending[owner] -= 1;
            if pending[owner] == 0 {
                cleared.push(owner);
            }
        }
    }
    let Some(mut index) = pending.iter().position(|&count| count > 0) else {
        return Ok(None);
    };
    for _ in 0..definitions.len() {
        let Some(&next) = places[index].iter().find(|&&placed| pending[placed] > 0) else {
            break;
        };
        index = next;
    }
    Ok(Some(definitions[index].id.as_str()))
}

#[cfg(test)]
mod tests {
    mod admission;
    mod body_owners;
    mod history_owners;
    mod root;
    use super::check_products;
    use crate::document::CadIr;
    use crate::ids::OccurrenceId;
    use crate::products::{
        AssemblyJoint, JointConnector, JointId, JointOperand, Occurrence, PairedJointKind,
    };
    use crate::transform::Transform;
    use cadmpeg_core::serde_erase::json;

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
            crate::examples::add_document_root(&mut ir);
            let mut findings = Vec::new();
            check_products(
                &cadmpeg_core::decode::test_support::service_decode_context(),
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

        crate::examples::add_document_root(&mut ir);
        let mut findings = Vec::new();
        check_products(
            &cadmpeg_core::decode::test_support::service_decode_context(),
            &ir,
            &mut findings,
        )
        .unwrap();
        assert!(findings.is_empty(), "{findings:?}");

        let occurrence =
            OccurrenceId::mint("test:model:occurrence#placed").expect("valid identity");
        ir.model.occurrences.push(Occurrence::new(
            occurrence.clone(),
            "test:model:product#placed".try_into().unwrap(),
            Some(Transform::identity()),
        ));
        let mut wire = json::to_value(&ir.model.assembly_joints[0]).expect("joint wire");
        wire["operands"]["value"]["connectors"][0]["operand"]["occurrence"] =
            cadmpeg_core::json!(occurrence.as_str());
        wire["operands"]["value"]["connectors"][0]["operand"]["external_document"] = cadmpeg_core::json!({
            "path": "external.f3d"
        });
        assert!(json::from_value::<AssemblyJoint>(wire).is_err());
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
                    ratio: crate::scalar::NonZeroReal::new(-2.0),
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
                &cadmpeg_core::decode::test_support::service_decode_context(),
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
        crate::examples::add_document_root(&mut ir);
        let mut findings = Vec::new();
        check_products(
            &cadmpeg_core::decode::test_support::service_decode_context(),
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
