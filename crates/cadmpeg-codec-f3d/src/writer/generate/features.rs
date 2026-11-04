// SPDX-License-Identifier: Apache-2.0
//! Author Design feature scopes and their scalar owners.

use super::presentation::{GeneratedDesignRegistry, GeneratedDesignType};
use super::records::{encode_design_metastream, native_lp_ascii, native_lp_utf16};
use crate::records::parameters::{
    DesignParameter, DesignParameterContent, DesignParameterDiscriminator, DesignParameterOwner,
    DesignParameterOwnerContent, DesignParameterSource,
};
use cadmpeg_core::CodecError;
use cadmpeg_ir::document::CadIr;
use cadmpeg_ir::features::{
    BooleanOp, FeatureDefinition, FeatureOperation, PrimitiveSolidKind, RadialDimension,
};

pub(super) fn encode(target: &CadIr) -> Result<Option<(Vec<u8>, Vec<u8>)>, CodecError> {
    let mut cylinders = Vec::new();
    for feature in &target.model.features {
        let FeatureDefinition::Operation(FeatureOperation::Primitive {
            placement: None,
            solid,
            op,
        }) = feature.evaluation.definition()
        else {
            continue;
        };
        let PrimitiveSolidKind::Cylinder {
            radius: RadialDimension::Diameter(diameter),
            height,
            angle,
        } = solid.kind()
        else {
            continue;
        };
        let (Some(diameter), Some(height), Some(angle)) = (
            target.dimension_value(diameter),
            target.dimension_value(height),
            target.dimension_value(angle),
        ) else {
            continue;
        };
        if angle != cadmpeg_ir::scalar::Angle::FULL_TURN {
            continue;
        }
        let operation = match op {
            BooleanOp::Join => 1u32,
            BooleanOp::Cut => 2,
            BooleanOp::Intersect => 3,
            BooleanOp::NewBody => 4,
            BooleanOp::Unresolved => continue,
        };
        cylinders.push((
            feature.ordinal,
            diameter.get() / 10.0,
            height.get() / 10.0,
            operation,
        ));
    }
    if cylinders.is_empty() {
        return Ok(None);
    }
    cylinders.sort_by_key(|cylinder| cylinder.0);
    let mut out = Vec::new();
    let mut primary = Vec::new();
    let mut scopes = Vec::new();
    let mut owners = Vec::new();
    let mut parameters = Vec::new();
    for (ordinal, (_, diameter, height, operation)) in cylinders.iter().enumerate() {
        let base = u32::try_from(ordinal)
            .ok()
            .and_then(|v| v.checked_mul(7))
            .and_then(|v| v.checked_add(1))
            .ok_or_else(|| CodecError::InvalidInput("too many F3D feature records".into()))?;
        scopes.push(u64::from(base));
        mark(&mut primary, base, out.len())?;
        header(&mut out, "257", base)?;
        out.extend_from_slice(&[0; 9]);
        out.extend_from_slice(&operation.to_le_bytes());
        out.push(0);
        out.push(1);
        out.extend_from_slice(&2u32.to_le_bytes());
        reference(&mut out, base + 1);
        reference(&mut out, base + 4);
        out.extend_from_slice(&u32::MAX.to_le_bytes());
        native_lp_utf16(&mut out, "CylinderPrimitive")?;
        out.extend_from_slice(&(u32::try_from(ordinal).unwrap_or(u32::MAX) + 1).to_le_bytes());
        out.extend_from_slice(&[0; 4]);
        out.extend_from_slice(&0u32.to_le_bytes());
        out.extend_from_slice(&[0; 7]);
        out.extend_from_slice(&[1, 15]);
        out.extend_from_slice(&1u64.to_le_bytes());
        out.extend_from_slice(&[0; 2]);
        out.extend_from_slice(&1u32.to_le_bytes());
        out.extend_from_slice(&252u32.to_le_bytes());
        out.extend_from_slice(&0f64.to_le_bytes());
        out.extend_from_slice(&252u32.to_le_bytes());
        out.extend_from_slice(&[1, 14]);
        out.extend_from_slice(&1u64.to_le_bytes());
        out.extend_from_slice(&[0, 1, 0, 0, 1, 13]);
        out.extend_from_slice(&1u64.to_le_bytes());
        out.extend_from_slice(&[0; 3]);
        header(&mut out, "256", base)?;
        for (local, value, kind) in [(0u32, *height, "Height"), (1, *diameter, "Diameter")] {
            let owner_index = base + 1 + 3 * local;
            let parameter_index = owner_index + 1;
            owners.push(u64::from(owner_index));
            parameters.push(u64::from(parameter_index));
            let owner = DesignParameterOwner::try_from(DesignParameterOwnerContent {
                id: format!("f3d:generated:owner#{owner_index}"),
                class_tag: "258".to_owned().try_into().map_err(CodecError::Malformed)?,
                record_index: owner_index,
                scope_record_index: Some(base),
                local_ordinal: Some(local),
                evaluated_value: value,
                parameter_record_index: parameter_index,
                owned_ordinal: local,
                variant: Some(0),
                companion_record_index: owner_index + 2,
            })
            .map_err(CodecError::Malformed)?;
            mark(&mut primary, owner_index, out.len())?;
            super::parameter_links::encode(&mut out, &[owner], &[])?;
            let parameter = DesignParameter::authored(DesignParameterContent {
                id: format!("f3d:generated:parameter#{parameter_index}"),
                class_tag: "259".to_owned().try_into().map_err(CodecError::Malformed)?,
                record_index: parameter_index,
                source_ordinal: local,
                source: DesignParameterSource::authored(
                    kind.into(),
                    Some(owner_index),
                    Some(DesignParameterDiscriminator::Code3),
                )
                .map_err(CodecError::Malformed)?,
                expression: format!("{} cm", value),
                unit: Some("cm".into()),
                name: format!("{kind}{base}"),
                evaluated_value: value,
            })
            .map_err(CodecError::Malformed)?;
            mark(&mut primary, parameter_index, out.len())?;
            out.extend_from_slice(&super::parameters::encode_document_parameters(&[
                parameter,
            ])?);
        }
    }
    let timeline = u32::try_from(cylinders.len())
        .ok()
        .and_then(|v| v.checked_mul(7))
        .and_then(|v| v.checked_add(1))
        .ok_or_else(|| CodecError::InvalidInput("too many F3D features".into()))?;
    mark(&mut primary, timeline, out.len())?;
    native_lp_ascii(&mut out, "260")?;
    out.extend_from_slice(&u64::from(timeline).to_le_bytes());
    native_lp_ascii(&mut out, "Timeline")?;
    out.extend_from_slice(&[0, 0, 1]);
    out.extend_from_slice(&scopes[0].to_le_bytes());
    out.extend_from_slice(&[0; 2]);
    out.extend_from_slice(
        &u32::try_from(scopes.len())
            .map_err(|_| CodecError::InvalidInput("too many F3D features".into()))?
            .to_le_bytes(),
    );
    for scope in &scopes {
        out.push(1);
        out.extend_from_slice(&scope.to_le_bytes());
        out.extend_from_slice(&[0; 2]);
    }
    let types = [
        (
            "CB844AB6-240D-4FC9-9C9F-3679DC896D6F",
            Some("98542EB9-A4F2-4137-A808-DBB5B3CD6159"),
            0,
            "CommonData",
            vec![],
        ),
        (
            "6AA4C36C-52A2-48D2-A062-69FE123B84D1",
            Some("2EDC2536-D95B-4469-8ACA-DC07124D9C43"),
            0,
            "Fusion",
            scopes,
        ),
        (
            "D91D429C-1BB5-4640-B49F-69AF14F463E9",
            Some("B24D5130-8751-4272-8143-D230D8757873"),
            0,
            "Fusion",
            owners,
        ),
        (
            "7A6A3D31-BE74-4E19-9115-20642944C2E4",
            Some("63D6AD42-466A-40B7-9CCD-EB21BC500EF6"),
            5,
            "CommonData",
            parameters,
        ),
        (
            "2F4C1849-1A5A-4F6C-A086-8DD445CBF94B",
            Some("98542EB9-A4F2-4137-A808-DBB5B3CD6159"),
            2,
            "Fusion",
            vec![u64::from(timeline)],
        ),
    ]
    .into_iter()
    .map(|(guid, base, version, module, entity_ids)| {
        Ok(GeneratedDesignType {
            type_guid: guid.to_owned().try_into().map_err(CodecError::Malformed)?,
            base_type_guid: base
                .map(|value| value.to_owned().try_into())
                .transpose()
                .map_err(CodecError::Malformed)?,
            version,
            module: module.into(),
            entity_ids,
        })
    })
    .collect::<Result<Vec<_>, CodecError>>()?;
    let registry = GeneratedDesignRegistry {
        types,
        body_map: None,
        browser_nodes: None,
    };
    let meta = encode_design_metastream(&registry, &primary)?
        .ok_or_else(|| CodecError::Malformed("missing generated feature metadata".into()))?;
    Ok(Some((out, meta)))
}

fn mark(
    records: &mut Vec<crate::metastream::RecordIndexEntry>,
    id: u32,
    offset: usize,
) -> Result<(), CodecError> {
    records.push(crate::metastream::RecordIndexEntry {
        entity_id: u64::from(id),
        bulk_offset: u64::try_from(offset)
            .map_err(|_| CodecError::InvalidInput("F3D stream too large".into()))?,
    });
    Ok(())
}
fn header(out: &mut Vec<u8>, tag: &str, id: u32) -> Result<(), CodecError> {
    native_lp_ascii(out, tag)?;
    out.extend_from_slice(&id.to_le_bytes());
    Ok(())
}
fn reference(out: &mut Vec<u8>, id: u32) {
    out.push(1);
    out.extend_from_slice(&id.to_le_bytes());
    out.extend_from_slice(&[0; 6]);
}
