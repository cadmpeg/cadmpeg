// SPDX-License-Identifier: Apache-2.0
//! Evaluated mate connector projection.

use super::{key, Structure};
use crate::error::Error;
use crate::loss::OnshapeLossCode;
use crate::reader::{id, optional_rows, rows, text, Mapping};
use cadmpeg_ir::products::{
    AssemblyJoint, JointConnector, JointOperand, PairedJointKind, PlaneSense,
};
use cadmpeg_ir::transform::Transform;
use serde_json::Value;

const MATE_AXIS_DOT_TOLERANCE: f64 = 1e-12;

pub(super) fn project(
    structure: &mut Structure<'_, '_>,
    assembly: &Value,
    path: &[String],
    suppressed: bool,
) -> Result<(), Error> {
    let mut notes = 0;
    if structure.mate_exhausted {
        return Ok(());
    }
    let features = match optional_rows(assembly, "features") {
        Ok(features) => features,
        Err(reason) => {
            structure.mate_loss(
                &mut notes,
                &key(path),
                OnshapeLossCode::MateUnsupported,
                reason,
            )?;
            return Ok(());
        }
    };
    for feature in features {
        if !structure.admit_mate_work()? {
            break;
        }
        let native = feature
            .get("id")
            .and_then(Value::as_str)
            .unwrap_or("feature");
        let feature_type = feature
            .get("featureType")
            .and_then(Value::as_str)
            .unwrap_or("unknown");
        if feature_type != "mate" {
            structure.mate_loss(
                &mut notes,
                native,
                OnshapeLossCode::AssemblyFeatureUnsupported,
                format!("assembly feature type {feature_type} retains its native response"),
            )?;
            continue;
        }
        match mate(structure, feature, path, suppressed) {
            Ok(mapped) => {
                if !structure.joint_ids.insert(mapped.joint.id.clone()) {
                    structure.mate_loss(
                        &mut notes,
                        native,
                        OnshapeLossCode::MateUnsupported,
                        "mate identity repeats; retained the first joint",
                    )?;
                    continue;
                }
                structure.model.ir.model.assembly_joints.push(mapped.joint);
                if mapped.nested {
                    structure.mate_loss(
                        &mut notes,
                        native,
                        OnshapeLossCode::MateFrameAssumed,
                        "nested matedCS is interpreted in the placed part coordinates; ONSHAPE-011",
                    )?;
                }
                if mapped.unresolved_sense {
                    structure.mate_loss(
                        &mut notes,
                        native,
                        OnshapeLossCode::MateFrameUnresolved,
                        "solved connector world frames leave the axis sense unstated",
                    )?;
                }
                let parameters = quantities(structure, assembly, feature, &mut notes)?;
                if !parameters.is_empty() {
                    structure.mate_loss(&mut notes, native, OnshapeLossCode::MateLimitsUnresolved,
                        format!("enabled or unclassified mate parameters retain their native response; ids: {}", parameters.join(", ")))?;
                }
            }
            Err((code, reason)) => structure.mate_loss(&mut notes, native, code, reason)?,
        }
    }
    Ok(())
}

struct MappedMate {
    joint: AssemblyJoint,
    nested: bool,
    unresolved_sense: bool,
}
struct Connector {
    joint: JointConnector,
    world: Option<Transform>,
    nested: bool,
}
type MateResult = Result<MappedMate, (OnshapeLossCode, String)>;
fn unsupported(reason: String) -> (OnshapeLossCode, String) {
    (OnshapeLossCode::MateUnsupported, reason)
}
fn mate(
    structure: &Structure<'_, '_>,
    feature: &Value,
    path: &[String],
    suppressed: bool,
) -> MateResult {
    let data = feature
        .get("featureData")
        .ok_or_else(|| unsupported("mate featureData is missing".into()))?;
    let [first, second] = rows(data, "matedEntities").map_err(unsupported)? else {
        return Err(unsupported("mate requires two evaluated connectors".into()));
    };
    let first = connector(structure, first, path).map_err(unsupported)?;
    let second = connector(structure, second, path).map_err(unsupported)?;
    let sense = sense(first.world, second.world);
    let kind = match text(data, "mateType").map_err(unsupported)? {
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
            sense: sense.ok_or_else(|| {
                (
                    OnshapeLossCode::MateFrameUnresolved,
                    "planar mate requires a resolved axis sense; retained its native response"
                        .into(),
                )
            })?,
            offset: None,
        },
        "BALL" => PairedJointKind::Ball {},
        other => {
            return Err(unsupported(format!(
                "mate type {other} retains its native response"
            )))
        }
    };
    let native = text(feature, "id").map_err(unsupported)?;
    let mut joint = AssemblyJoint::paired(
        id("joint", &format!("{}:{native}", key(path)))
            .try_into()
            .map_err(|e| unsupported(format!("{e}")))?,
        kind,
        [first.joint, second.joint],
        None,
    );
    // A paired joint always stores its frame sense.
    joint.set_frame_sense(sense);
    joint.suppressed = suppressed
        || feature
            .get("suppressed")
            .and_then(Value::as_bool)
            .unwrap_or(false);
    Ok(MappedMate {
        joint,
        nested: first.nested || second.nested,
        unresolved_sense: sense.is_none(),
    })
}
fn sense(first: Option<Transform>, second: Option<Transform>) -> Option<PlaneSense> {
    let first = first?.affine_rows();
    let second = second?.affine_rows();
    let dot = first
        .iter()
        .zip(second)
        .map(|(a, b)| a[2] * b[2])
        .sum::<f64>();
    if !dot.is_finite() {
        return None;
    }
    if dot > MATE_AXIS_DOT_TOLERANCE {
        Some(PlaneSense::Aligned)
    } else if dot < -MATE_AXIS_DOT_TOLERANCE {
        Some(PlaneSense::Opposed)
    } else {
        None
    }
}
fn connector(
    structure: &Structure<'_, '_>,
    entity: &Value,
    parent: &[String],
) -> Mapping<Connector> {
    let mut path = parent.to_vec();
    let components = rows(entity, "matedOccurrence")?;
    if parent.len() + components.len() > super::ASSEMBLY_DEPTH {
        return Err("mate occurrence path exceeds the supported depth".into());
    }
    for component in components {
        path.push(
            component
                .as_str()
                .filter(|s| !s.is_empty())
                .ok_or("mate occurrence component is invalid")?
                .to_owned(),
        );
    }
    let placement = structure
        .placements
        .get(&path)
        .ok_or("mate occurrence path is unresolved")?;
    let cs = entity
        .get("matedCS")
        .ok_or("evaluated mate coordinate system is missing")?;
    let vector = |field| -> Mapping<[f64; 3]> {
        let [x, y, z] = rows(cs, field)? else {
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
    let world = placement
        .world
        .map(|world| world.compose(frame))
        .transpose()
        .map_err(|reason| format!("solved connector world frame is invalid: {reason}"))?;
    Ok(Connector {
        joint: JointConnector {
            operand: JointOperand::occurrence(
                placement.id.clone(),
                path.last().cloned().unwrap_or_default(),
                Vec::new(),
            ),
            frame,
            detached: false,
        },
        world,
        nested: components.len() > 1,
    })
}

fn quantities(
    structure: &mut Structure<'_, '_>,
    assembly: &Value,
    feature: &Value,
    notes: &mut usize,
) -> Result<Vec<String>, Error> {
    let eid = assembly
        .get("elementId")
        .and_then(Value::as_str)
        .unwrap_or(structure.root_element);
    let native = feature.get("id").and_then(Value::as_str).unwrap_or("");
    let response = structure
        .index
        .native_features
        .get(&(eid.into(), native.into()))
        .copied();
    let mut parameters = Vec::new();
    for owner in [Some(feature), feature.get("featureData"), response]
        .into_iter()
        .flatten()
    {
        let Ok(rows) = optional_rows(owner, "parameters") else {
            structure.mate_loss(
                notes,
                native,
                OnshapeLossCode::AssemblyRecordUnsupported,
                "mate parameters require an array; retained the native response",
            )?;
            continue;
        };
        for parameter in rows {
            if !structure.admit_mate_work()? {
                return Ok(Vec::new());
            }
            let native = parameter
                .get("parameterId")
                .and_then(Value::as_str)
                .unwrap_or("unnamed parameter");
            parameters.push((
                native.to_owned(),
                parameter
                    .get("value")
                    .or_else(|| parameter.get("expression"))
                    .unwrap_or(&Value::Null),
            ));
        }
        if let Some(fields) = owner.as_object() {
            for (name, value) in fields {
                if !structure.admit_mate_work()? {
                    return Ok(Vec::new());
                }
                let lower = name.to_ascii_lowercase();
                if lower.contains("limit") || lower.contains("offset") {
                    parameters.push((name.clone(), value));
                }
            }
        }
    }
    let switches: Vec<Vec<String>> = parameters
        .iter()
        .filter(|(_, value)| value.as_bool() == Some(false))
        .filter_map(|(name, _)| switch_scope(name))
        .collect();
    let mut ids = std::collections::BTreeSet::new();
    for (name, value) in parameters {
        if matches!(
            name.as_str(),
            "mateType"
                | "mateConnectorQueryList"
                | "mateConnectors"
                | "entities"
                | "name"
                | "suppressed"
                | "primaryAxisAlignment"
                | "secondaryAxisAlignment"
                | "flipPrimaryAxis"
                | "flipSecondaryAxis"
        ) {
            continue;
        }
        let lower = name.to_ascii_lowercase();
        if lower.contains("limit") || lower.contains("offset") {
            let tokens = name_tokens(&name);
            if switches
                .iter()
                .any(|scope| scope.iter().all(|token| tokens.contains(token)))
            {
                continue;
            }
            if value.as_bool() == Some(false) {
                continue;
            }
            let zero_offset = lower.contains("offset")
                && (value.as_f64() == Some(0.0)
                    || value.as_str().is_some_and(|expression| {
                        let mut tokens = expression.split_whitespace();
                        tokens
                            .next()
                            .is_some_and(|value| value.parse::<f64>() == Ok(0.0))
                            && tokens.next().is_some_and(|unit| {
                                matches!(unit, "m" | "mm" | "cm" | "in" | "deg" | "rad")
                            })
                            && tokens.next().is_none()
                    }));
            if zero_offset {
                continue;
            }
        }
        ids.insert(name);
    }
    Ok(ids.into_iter().collect())
}

/// Lowercase words of a parameter name, split at separators and camel-case
/// boundaries, with a plural `s` removed.
fn name_tokens(name: &str) -> Vec<String> {
    let mut tokens = Vec::new();
    let mut word = String::new();
    let mut previous_lower = false;
    for character in name.chars() {
        let boundary = !character.is_alphanumeric() || (character.is_uppercase() && previous_lower);
        if boundary && !word.is_empty() {
            tokens.push(std::mem::take(&mut word));
        }
        if character.is_alphanumeric() {
            word.extend(character.to_lowercase());
        }
        previous_lower = character.is_lowercase();
    }
    if !word.is_empty() {
        tokens.push(word);
    }
    for token in &mut tokens {
        if token.len() > 3 && token.ends_with('s') {
            token.pop();
        }
    }
    tokens
}

/// The words an `enable` switch governs: its own name without the switch
/// word. A switch governs a limit or offset only when it names one.
fn switch_scope(name: &str) -> Option<Vec<String>> {
    let tokens = name_tokens(name);
    if !tokens.iter().any(|token| token.starts_with("enable")) {
        return None;
    }
    let scope: Vec<String> = tokens
        .into_iter()
        .filter(|token| !token.starts_with("enable"))
        .collect();
    scope
        .iter()
        .any(|token| matches!(token.as_str(), "limit" | "offset"))
        .then_some(scope)
}
