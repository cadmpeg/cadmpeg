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
    let custom =
        property.dynamic.is_some() && !crate::property_policy::is_projection_metadata(property);
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::native::{DynamicPropertyMeta, PropertyFamily};
    use cadmpeg_ir::{Codec, DecodeOptions};
    use std::io::Cursor;

    #[test]
    fn projected_property_deletion_applies_to_every_group() {
        let decoded = crate::FcstdCodec
            .decode(
                &mut Cursor::new(crate::test_support::test_archive::CORE_DESIGN_PRODUCT),
                &DecodeOptions::default(),
            )
            .unwrap();
        let ir = decoded.ir();
        let owner = ir
            .model
            .features
            .iter()
            .find_map(|feature| feature.native_ref.clone())
            .unwrap();
        let mut property = PropertyRecord {
            id: "fcstd:native:property#test".into(),
            owner,
            name: "CustomScalar".into(),
            type_name: "App::PropertyFloat".into(),
            family: PropertyFamily::Scalar,
            status: None,
            dynamic: Some(DynamicPropertyMeta {
                group: "CADIR".into(),
                documentation: None,
                attributes: None,
                read_only: None,
                hidden: None,
            }),
            body: None,
            order: 0,
        };
        assert!(!property_held(ir, &property, &[]));
        property.dynamic.as_mut().unwrap().group = "Custom".into();
        assert!(!property_held(ir, &property, &[]));
        property.dynamic.as_mut().unwrap().group = "CADIR".into();
        property.name = "CadmpegFeatureOrdinal".into();
        assert!(property_held(ir, &property, &[]));
    }
}
