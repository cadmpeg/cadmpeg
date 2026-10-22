// SPDX-License-Identifier: Apache-2.0
//! Assembly connector frames, references, joint families, and limits.

use super::{bool_property, external, fresh_name, xlink};
use crate::loss::FreecadLossCode;
use crate::native::joint::JointRecord;
use crate::writer::xml::{self, Object};
use cadmpeg_core::CodecError;
use cadmpeg_ir::document::CadIr;
use cadmpeg_ir::products::{
    AssemblyGraph, JointLimits, JointOperand, JointOperands, OperandContainer, PairedJointKind,
};
use cadmpeg_ir::report::loss::LossNote;
use cadmpeg_ir::scalar::FiniteReal;
use cadmpeg_ir::transform::Transform;
use std::collections::{HashMap, HashSet};

fn frame(
    object: &mut Object,
    name: &str,
    transform: Transform,
    losses: &mut Vec<LossNote>,
) -> Result<(), CodecError> {
    object.set(
        name,
        super::placement(transform, &object.name, losses)?.replacen(
            "name=\"Placement\"",
            &format!("name=\"{name}\""),
            1,
        ),
    );
    Ok(())
}

fn reference(
    operand: &JointOperand,
    names: &HashMap<String, String>,
    losses: &mut Vec<LossNote>,
) -> Result<String, CodecError> {
    let (object, file) = match &operand.container {
        OperandContainer::External { external_document } => {
            (operand.object.clone(), external(external_document, losses))
        }
        OperandContainer::Root {} => (
            names
                .get(&operand.object)
                .cloned()
                .or_else(|| {
                    names
                        .values()
                        .find(|name| **name == operand.object)
                        .cloned()
                })
                .ok_or_else(|| {
                    CodecError::malformed(format_args!(
                        "joint operand {} has no output object",
                        operand.object
                    ))
                })?,
            String::new(),
        ),
        OperandContainer::Occurrence { occurrence, .. } => (
            names
                .get(occurrence.as_str())
                .cloned()
                .ok_or_else(|| CodecError::malformed("joint operand occurrence is absent"))?,
            String::new(),
        ),
    };
    let nested_subelements;
    let subelements = if let OperandContainer::Occurrence {
        occurrence_path, ..
    } = &operand.container
    {
        if occurrence_path.is_empty() {
            &operand.subelements
        } else {
            let mut prefix = occurrence_path.join(".");
            prefix.push('.');
            prefix.push_str(&operand.object);
            prefix.push('.');
            nested_subelements = if operand.subelements.is_empty() {
                vec![prefix]
            } else {
                operand
                    .subelements
                    .iter()
                    .map(|selection| format!("{prefix}{selection}"))
                    .collect()
            };
            &nested_subelements
        }
    } else {
        &operand.subelements
    };
    Ok(xlink(&object, &file, subelements))
}

fn scalar(object: &mut Object, name: &str, value: Option<FiniteReal>, angular: bool) {
    if let Some(value) = value {
        object.set(
            name,
            xml::scalar(
                name,
                if angular {
                    "App::PropertyAngle"
                } else {
                    "App::PropertyLength"
                },
                "Float",
                if angular {
                    value.get().to_degrees()
                } else {
                    value.get()
                },
            ),
        );
    }
}

fn limits(object: &mut Object, limits: Option<&JointLimits>, angular: bool) {
    let (minimum, maximum) = match limits {
        Some(JointLimits::Minimum { minimum }) => (Some(*minimum), None),
        Some(JointLimits::Maximum { maximum }) => (None, Some(*maximum)),
        Some(JointLimits::Range(range)) => (Some(range.minimum()), Some(range.maximum())),
        None => (None, None),
    };
    let (low, high, enable_low, enable_high) = if angular {
        ("AngleMin", "AngleMax", "EnableAngleMin", "EnableAngleMax")
    } else {
        (
            "LengthMin",
            "LengthMax",
            "EnableLengthMin",
            "EnableLengthMax",
        )
    };
    scalar(object, low, minimum, angular);
    scalar(object, high, maximum, angular);
    bool_property(object, enable_low, minimum.is_some());
    bool_property(object, enable_high, maximum.is_some());
}

fn family_label(kind: &PairedJointKind) -> Option<&str> {
    use PairedJointKind as K;
    Some(match kind {
        K::Fixed { .. } => "Fixed",
        K::Revolute { .. } => "Revolute",
        K::Slider { .. } => "Slider",
        K::Cylindrical { .. } => "Cylindrical",
        K::Ball {} => "Ball",
        K::Distance { .. } => "Distance",
        K::Parallel {} => "Parallel",
        K::Perpendicular {} => "Perpendicular",
        K::Angle { .. } => "Angle",
        K::RackPinion { .. } => "RackPinion",
        K::Screw { .. } => "Screw",
        K::Gears { .. } => "Gears",
        K::Belt { .. } => "Belt",
        K::Native { name, .. } => name,
        K::PlaneCoincidence { .. }
        | K::AxisCoincidence {}
        | K::PointOnAxis { .. }
        | K::Insert { .. }
        | K::Tangent { .. } => return None,
    })
}

fn family(object: &mut Object, kind: &PairedJointKind, label: &str, losses: &mut Vec<LossNote>) {
    use PairedJointKind as K;
    object.set("JointType",xml::property("JointType","App::PropertyEnumeration",&format!("<Integer value=\"0\" CustomEnum=\"true\"/><CustomEnumList count=\"1\">{}</CustomEnumList>",xml::element("Enum",&[("value",label.to_owned())]))));
    let angle = match kind {
        K::Fixed { angle, .. }
        | K::Revolute { angle, .. }
        | K::Cylindrical { angle, .. }
        | K::Angle { angle }
        | K::Native { angle, .. } => *angle,
        _ => None,
    };
    let distance = match kind {
        K::Slider { distance, .. }
        | K::Cylindrical { distance, .. }
        | K::Distance { distance }
        | K::RackPinion { distance, .. }
        | K::Screw { distance }
        | K::Gears { distance, .. }
        | K::Belt { distance, .. }
        | K::Native { distance, .. } => *distance,
        _ => None,
    };
    let distance2 = match kind {
        K::RackPinion { distance2, .. }
        | K::Gears { distance2, .. }
        | K::Belt { distance2, .. }
        | K::Native { distance2, .. } => *distance2,
        _ => None,
    };
    let angular_limits = match kind {
        K::Fixed { angular_limits, .. }
        | K::Revolute { angular_limits, .. }
        | K::Cylindrical { angular_limits, .. }
        | K::Native { angular_limits, .. } => angular_limits.as_ref(),
        _ => None,
    };
    let linear_limits = match kind {
        K::Fixed { linear_limits, .. }
        | K::Slider { linear_limits, .. }
        | K::Cylindrical { linear_limits, .. }
        | K::Native { linear_limits, .. } => linear_limits.as_ref(),
        _ => None,
    };
    scalar(object, "Angle", angle, true);
    scalar(object, "Distance", distance, false);
    scalar(object, "Distance2", distance2, false);
    limits(object, angular_limits, true);
    limits(object, linear_limits, false);
    if matches!(
        kind,
        K::Fixed {
            translation_offset: Some(_),
            ..
        } | K::Slider {
            translation_offset: Some(_),
            ..
        } | K::Native {
            translation_offset: Some(_),
            ..
        }
    ) {
        losses.push(FreecadLossCode::ExportFeatureNotTransferred.note(format!("joint {} retains connector frames and scalar offsets; its additional vector offset is omitted",object.name)));
    }
}

pub(super) fn write(
    ir: &CadIr,
    objects: &mut Vec<Object>,
    names: &mut HashMap<String, String>,
    losses: &mut Vec<LossNote>,
) -> Result<(), CodecError> {
    let graph = AssemblyGraph::new(&ir.model.occurrences).map_err(CodecError::malformed)?;
    let native = ir
        .native
        .namespace("fcstd")
        .map(|namespace| namespace.arena_as::<JointRecord>("joints"))
        .transpose()?
        .unwrap_or_default();
    let mut used: HashSet<_> = objects.iter().map(|object| object.name.clone()).collect();
    for joint in &ir.model.assembly_joints {
        if let Some(unknown) = joint.connectors().find_map(|connector| {
            let OperandContainer::Occurrence { occurrence, .. } = &connector.operand.container
            else {
                return None;
            };
            ir.model.occurrences.iter().find(|candidate| {
                candidate.id == *occurrence && graph.effective_transform(&candidate.id).is_none()
            })
        }) {
            losses.push(
                FreecadLossCode::ExportJointOperandNotTransferred.note(format!(
                    "joint {} omitted because occurrence {} has unknown placement",
                    joint.id, unknown.id
                )),
            );
            continue;
        }
        let label = match joint.operands() {
            JointOperands::Grounded { .. } => "",
            JointOperands::Pair { kind, .. } => {
                let Some(label) = family_label(kind) else {
                    losses.push(FreecadLossCode::ExportFeatureNotTransferred.note(format!(
                        "joint {} has no FreeCAD joint family for {kind:?}",
                        joint.id
                    )));
                    continue;
                };
                label
            }
        };
        let existing = joint
            .native_ref
            .as_ref()
            .and_then(|id| native.iter().find(|record| record.id() == id))
            .and_then(|record| names.get(record.object()))
            .cloned();
        let name =
            existing.unwrap_or_else(|| fresh_name(&format!("Joint{}", objects.len()), &mut used));
        if !objects.iter().any(|object| object.name == name) {
            objects.push(Object::new(
                name.clone(),
                "App::FeaturePython",
                joint.id.as_str(),
            ));
        }
        names.insert(joint.id.as_str().to_owned(), name.clone());
        let object = objects
            .iter_mut()
            .find(|object| object.name == name)
            .ok_or_else(|| CodecError::malformed("joint output is absent"))?;
        for key in [
            "JointType",
            "ObjectToGround",
            "Placement",
            "Placement1",
            "Placement2",
            "Offset1",
            "Offset2",
            "Reference1",
            "Reference2",
            "Angle",
            "Distance",
            "Distance2",
            "AngleMin",
            "AngleMax",
            "LengthMin",
            "LengthMax",
            "EnableAngleMin",
            "EnableAngleMax",
            "EnableLengthMin",
            "EnableLengthMax",
            "Detach1",
            "Detach2",
            "Suppressed",
        ] {
            object.properties.remove(key);
        }
        match joint.operands() {
            JointOperands::Grounded {
                connector,
                offset_frame,
            } => {
                let target = reference(&connector.operand, names, losses)?;
                // Grounded links have no subelement carrier in FreeCAD.
                if !connector.operand.subelements.is_empty()
                    || matches!(&connector.operand.container, OperandContainer::Occurrence {
                        occurrence_path, ..
                    } if !occurrence_path.is_empty())
                {
                    losses.push(FreecadLossCode::ExportFeatureNotTransferred.note(format!(
                        "grounded joint {} selects the containing object rather than nested objects or subelements",
                        joint.id
                    )));
                }
                let target_name = match &connector.operand.container {
                    OperandContainer::External { .. } => None,
                    _ => names.get(&connector.operand.object).or_else(|| {
                        if let OperandContainer::Occurrence { occurrence, .. } =
                            &connector.operand.container
                        {
                            names.get(occurrence.as_str())
                        } else {
                            None
                        }
                    }),
                };
                if let Some(name) = target_name {
                    object.set(
                        "ObjectToGround",
                        xml::property(
                            "ObjectToGround",
                            "App::PropertyLinkGlobal",
                            &xml::element("Link", &[("value", name.clone())]),
                        ),
                    );
                } else {
                    losses.push(
                        FreecadLossCode::ExportFeatureNotTransferred
                            .note(format!("external grounded joint {} is omitted", joint.id)),
                    );
                    object.properties.remove("ObjectToGround");
                    continue;
                }
                drop(target);
                let placement = if let Some(offset) = offset_frame {
                    losses.push(FreecadLossCode::ExportFeatureNotTransferred.note(format!(
                        "grounded joint {} attachment offset is composed into its frame",
                        joint.id
                    )));
                    connector
                        .frame
                        .compose(*offset)
                        .map_err(CodecError::malformed)?
                } else {
                    connector.frame
                };
                frame(object, "Placement", placement, losses)?;
                bool_property(object, "Detach1", connector.detached);
            }
            JointOperands::Pair {
                kind,
                connectors,
                offset_frames,
            } => {
                family(object, kind, label, losses);
                for (index, connector) in connectors.iter().enumerate() {
                    let suffix = index + 1;
                    let key = format!("Reference{suffix}");
                    object.set(
                        &key,
                        xml::property(
                            &key,
                            "App::PropertyXLinkSub",
                            &reference(&connector.operand, names, losses)?,
                        ),
                    );
                    frame(
                        object,
                        &format!("Placement{suffix}"),
                        connector.frame,
                        losses,
                    )?;
                    frame(
                        object,
                        &format!("Offset{suffix}"),
                        offset_frames.map_or(Transform::identity(), |frames| frames[index]),
                        losses,
                    )?;
                    bool_property(object, &format!("Detach{suffix}"), connector.detached);
                }
            }
        }
        bool_property(object, "Suppressed", joint.suppressed);
    }
    Ok(())
}

#[cfg(test)]
mod tests;
