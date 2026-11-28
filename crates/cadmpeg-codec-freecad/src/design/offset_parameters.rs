// SPDX-License-Identifier: Apache-2.0
//! The parameters of the `AttachmentOffset` components of a sketch that an
//! expression binds.

use std::collections::{BTreeMap, HashMap, HashSet};

use cadmpeg_core::CodecError;
use cadmpeg_ir::features::parameters::{DesignParameter, ParameterValue};
use cadmpeg_ir::features::selections::DistinctMembers;
use cadmpeg_ir::features::{FeatureId, ParameterId};
use cadmpeg_ir::scalar::{Angle, Length};

use super::{design_identity_text, feature_id, is_sketch, property};
use crate::native::{ObjectRecord, PropertyRecord};

/// One parameter for each component of a sketch's `AttachmentOffset` that
/// its `ExpressionEngine` binds, named by the component's path below the
/// object (`AttachmentOffset.Base.x`), owned by the sketch's feature, and
/// holding the expression and the stored value: `Base.x`, `.y`, and `.z`
/// as lengths, and `Rotation.Yaw`, `.Pitch`, `.Roll`, and `.Angle` as
/// angles. `FreeCAD` evaluates a yaw, pitch, roll, or angle expression in
/// degrees and stores the axis and angle of the rotation (`PropertyGeo.cpp`).
pub(super) fn append(
    parameters: &mut Vec<DesignParameter>,
    objects: &[ObjectRecord],
    properties_by_owner: &HashMap<&str, Vec<&PropertyRecord>>,
) -> Result<(), CodecError> {
    let mut last_ordinals = HashMap::<FeatureId, u32>::new();
    let mut existing = HashSet::<(FeatureId, String)>::new();
    for parameter in parameters.iter() {
        let Some(owner) = parameter.owner.as_ref() else {
            continue;
        };
        last_ordinals
            .entry(owner.clone())
            .and_modify(|ordinal| *ordinal = (*ordinal).max(parameter.ordinal))
            .or_insert(parameter.ordinal);
        existing.insert((owner.clone(), parameter.name.clone()));
    }
    for object in objects.iter().filter(|object| is_sketch(&object.type_name)) {
        let owned = properties_by_owner
            .get(object.id().as_str())
            .map_or(&[][..], Vec::as_slice);
        let (Some(engine), Some(offset)) = (
            property(owned, "ExpressionEngine"),
            property(owned, "AttachmentOffset"),
        ) else {
            continue;
        };
        let Some(matrix) = crate::placement::placement_matrix_unreported(offset) else {
            continue;
        };
        let rows = matrix.rows();
        let (yaw, pitch, roll) = crate::placement::yaw_pitch_roll(rows);
        let angle = crate::placement::placement_axis_angle(offset).map(|(_, angle)| angle);
        let owner = feature_id(object)?;
        for value in engine
            .values()
            .iter()
            .filter(|value| value.tag == "Expression")
        {
            let (Some(path), Some(expression)) = (
                value.attributes.get("path"),
                value.attributes.get("expression"),
            ) else {
                continue;
            };
            let Some(component) = path.strip_prefix(".AttachmentOffset.") else {
                continue;
            };
            let value = match component {
                "Base.x" => Length::new(rows[0][3]).map(ParameterValue::Length),
                "Base.y" => Length::new(rows[1][3]).map(ParameterValue::Length),
                "Base.z" => Length::new(rows[2][3]).map(ParameterValue::Length),
                "Rotation.Yaw" => Angle::new(yaw).map(ParameterValue::Angle),
                "Rotation.Pitch" => Angle::new(pitch).map(ParameterValue::Angle),
                "Rotation.Roll" => Angle::new(roll).map(ParameterValue::Angle),
                "Rotation.Angle" => angle.and_then(Angle::new).map(ParameterValue::Angle),
                _ => None,
            };
            let Some(value) = value else {
                continue;
            };
            let name = format!("AttachmentOffset.{component}");
            if !existing.insert((owner.clone(), name.clone())) {
                continue;
            }
            let ordinal = last_ordinals
                .get(&owner)
                .map_or(Some(0), |ordinal| ordinal.checked_add(1))
                .ok_or_else(|| CodecError::malformed("FreeCAD parameter ordinal overflow"))?;
            last_ordinals.insert(owner.clone(), ordinal);
            parameters.push(DesignParameter {
                product_definition: None,
                id: ParameterId::mint(design_identity_text(
                    "parameter",
                    object,
                    format_args!(":{}", cadmpeg_ir::ids::IdentityKey::encode_segment(&name)),
                ))
                .map_err(CodecError::malformed)?,
                owner: Some(owner.clone()),
                ordinal,
                name,
                expression: expression.clone(),
                display: None,
                value: Some(value),
                dependencies: DistinctMembers::default(),
                properties: BTreeMap::from([(
                    cadmpeg_core::nonblank_literal!("expression_native_ref"),
                    engine.id.clone(),
                )]),
                pmi: None,
                native_ref: Some(offset.id.clone()),
            });
        }
    }
    Ok(())
}
