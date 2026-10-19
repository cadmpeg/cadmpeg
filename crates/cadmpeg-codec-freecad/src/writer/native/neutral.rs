// SPDX-License-Identifier: Apache-2.0
//! Absence in a held neutral arena also owns absence in native persistence.

use crate::native::{ObjectRecord, PropertyRecord};
use cadmpeg_core::CodecError;
use cadmpeg_ir::CadIr;

pub(super) fn retained(
    ir: &CadIr,
    object: &ObjectRecord,
    properties: &[PropertyRecord],
) -> Result<bool, CodecError> {
    if !ir.scope.model {
        return Ok(true);
    }
    let identity = object.id().as_str();
    if matches!(
        object.type_name.as_str(),
        "Assembly::AssemblyObject"
            | "Assembly::AssemblyLink"
            | "App::Part"
            | "App::DocumentObjectGroup"
            | "App::LinkGroup"
    ) {
        return Ok(ir
            .model
            .product_definitions
            .iter()
            .any(|record| record.native_ref.as_deref() == Some(identity)));
    }
    if matches!(object.type_name.as_str(), "App::Link" | "App::LinkElement") {
        return Ok(ir
            .model
            .occurrences
            .iter()
            .any(|record| record.native_ref.as_deref() == Some(identity)));
    }
    if object.type_name.starts_with("TechDraw::") || object.type_name.starts_with("Drawing::") {
        // Drawings have a neutral form; this writer explicitly drops that arena.
        return Ok(false);
    }
    if properties.iter().any(|property| {
        property.owner == identity
            && matches!(property.name.as_str(), "JointType" | "ObjectToGround")
    }) {
        let records = ir
            .native
            .namespace("fcstd")
            .map(|namespace| namespace.arena_as::<crate::native::joint::JointRecord>("joints"))
            .transpose()?
            .unwrap_or_default();
        return Ok(records
            .iter()
            .filter(|record| record.object() == identity)
            .any(|record| {
                ir.model
                    .assembly_joints
                    .iter()
                    .any(|joint| joint.native_ref.as_deref() == Some(record.id()))
            }));
    }
    if object.type_name == "App::VarSet" && ir.scope.design {
        return Ok(ir.model.parameters.iter().any(|parameter| {
            parameter.native_ref.as_ref().is_some_and(|native| {
                properties
                    .iter()
                    .any(|property| property.id == *native && property.owner == identity)
            })
        }));
    }
    Ok(true)
}

/// Generic scalar custom properties are projected parameters, not native extras.
pub(super) fn property_held(
    ir: &CadIr,
    property: &PropertyRecord,
    objects: &[ObjectRecord],
) -> bool {
    if !ir.scope.design {
        return true;
    }
    let custom = matches!(&property.body,crate::native::PropertyBody::Persisted {dynamic:Some(dynamic),..} if dynamic.group!="CADIR");
    let variable = objects
        .iter()
        .any(|object| object.id().as_str() == property.owner && object.type_name == "App::VarSet");
    let feature_owner = ir
        .model
        .features
        .iter()
        .any(|feature| feature.native_ref.as_deref() == Some(property.owner.as_str()));
    if ((custom && feature_owner) || variable)
        && crate::persistence::scalar_value_tag(&property.type_name).is_some()
        && !matches!(
            property.name.as_str(),
            "Label" | "Label2" | "Visibility" | "Suppressed"
        )
    {
        return ir
            .model
            .parameters
            .iter()
            .any(|parameter| parameter.native_ref.as_deref() == Some(property.id.as_str()));
    }
    true
}
