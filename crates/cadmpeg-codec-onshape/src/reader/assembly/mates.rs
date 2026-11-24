// SPDX-License-Identifier: Apache-2.0
//! Evaluated mate connector projection.

use super::{key, Structure};
use crate::error::Error;
use crate::loss::OnshapeLossCode;
use crate::reader::{id, optional_rows, rows, text, Mapping};
use cadmpeg_ir::products::{AssemblyJoint, JointConnector, JointOperand};
use cadmpeg_ir::products::{PairedJointKind, PlaneSense};
use cadmpeg_ir::transform::Transform;
use serde_json::Value;

pub(super) fn project(
    structure: &mut Structure<'_, '_>,
    assembly: &Value,
    path: &[String],
) -> Result<(), Error> {
    let features = match optional_rows(assembly, "features") {
        Ok(features) => features,
        Err(reason) => {
            structure
                .report
                .push_coded(&key(path), OnshapeLossCode::MateUnsupported, reason);
            return Ok(());
        }
    };
    for feature in features {
        structure.ctx.charge_entities(1, "Onshape assembly mate")?;
        let native = feature.get("id").and_then(Value::as_str).unwrap_or("mate");
        let mapped = mate(structure, feature, path);
        match mapped {
            Ok(joint) => {
                if structure
                    .model
                    .ir
                    .model
                    .assembly_joints
                    .iter()
                    .any(|existing| existing.id == joint.id)
                {
                    structure.report.push_coded(
                        native,
                        OnshapeLossCode::MateUnsupported,
                        "mate identity repeats; retained the first joint",
                    );
                } else {
                    structure.model.ir.model.assembly_joints.push(joint);
                }
            }
            Err(reason) => {
                structure
                    .report
                    .push_coded(native, OnshapeLossCode::MateUnsupported, reason);
            }
        }
    }
    Ok(())
}

fn mate(
    structure: &mut Structure<'_, '_>,
    feature: &Value,
    path: &[String],
) -> Mapping<AssemblyJoint> {
    let data = feature
        .get("featureData")
        .ok_or("mate featureData is missing")?;
    let kind = match text(data, "mateType")? {
        "FASTENED" => PairedJointKind::Fixed {
            angle: None,
            translation_offset: None,
            angular_limits: None,
            linear_limits: None,
        },
        "REVOLUTE" => PairedJointKind::Revolute {
            angle: None,
            angular_limits: None,
        },
        "SLIDER" => PairedJointKind::Slider {
            distance: None,
            translation_offset: None,
            linear_limits: None,
        },
        "CYLINDRICAL" => PairedJointKind::Cylindrical {
            angle: None,
            distance: None,
            angular_limits: None,
            linear_limits: None,
        },
        "PLANAR" => PairedJointKind::PlaneCoincidence {
            sense: PlaneSense::Aligned,
            offset: None,
        },
        "BALL" => PairedJointKind::Ball {},
        other => return Err(format!("mate type {other} retains its bundle member")),
    };
    let [first, second] = rows(data, "matedEntities")? else {
        return Err("mate requires two evaluated connectors".into());
    };
    let connectors = [
        connector(structure, first, path)?,
        connector(structure, second, path)?,
    ];
    let native = text(feature, "id")?;
    let mut joint = AssemblyJoint::paired(
        id("joint", &format!("{}:{native}", key(path)))
            .try_into()
            .map_err(|e| format!("{e}"))?,
        kind,
        connectors,
        None,
    );
    joint.suppressed = feature
        .get("suppressed")
        .and_then(Value::as_bool)
        .unwrap_or(false);
    structure.report.push_coded(native, OnshapeLossCode::MateLimitsUnresolved,
        "mate limits and feature parameter offsets retain their bundle members; evaluated connector frames establish the joint");
    Ok(joint)
}

fn connector(
    structure: &Structure<'_, '_>,
    entity: &Value,
    parent: &[String],
) -> Mapping<JointConnector> {
    let mut path = parent.to_vec();
    for component in rows(entity, "matedOccurrence")? {
        path.push(
            component
                .as_str()
                .filter(|s| !s.is_empty())
                .ok_or("mate occurrence component is invalid")?
                .to_owned(),
        );
    }
    let occurrence = structure
        .placements
        .get(&path)
        .ok_or("mate occurrence path is unresolved")?;
    let cs = entity
        .get("matedCS")
        .ok_or("evaluated mate coordinate system is missing")?;
    let vector = |field| -> Mapping<[f64; 3]> {
        let values = rows(cs, field)?;
        let [x, y, z] = values else {
            return Err(format!("{field} requires three values"));
        };
        let scalar = |v: &Value| {
            v.as_f64()
                .filter(|v| v.is_finite())
                .ok_or_else(|| format!("{field} requires finite values"))
        };
        Ok([scalar(x)?, scalar(y)?, scalar(z)?])
    };
    let [ox, oy, oz] = vector("origin")?;
    let [xx, xy, xz] = vector("xAxis")?;
    let [yx, yy, yz] = vector("yAxis")?;
    let [zx, zy, zz] = vector("zAxis")?;
    let frame = Transform::affine([
        [xx, yx, zx, ox * 1000.0],
        [xy, yy, zy, oy * 1000.0],
        [xz, yz, zz, oz * 1000.0],
    ])
    .ok_or("mate frame exceeds finite coordinates")?;
    if !frame.is_proper_rigid() {
        return Err("mate axes do not form a proper rigid frame".into());
    }
    Ok(JointConnector {
        operand: JointOperand::occurrence(
            occurrence.clone(),
            path.last().cloned().unwrap_or_default(),
            Vec::new(),
        ),
        frame,
        detached: false,
    })
}
