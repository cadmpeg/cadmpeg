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
    BooleanOp, Dimension, DimensionValue, FeatureDefinition, FeatureId, FeatureOperation,
    ParameterId, ParameterValue, PrimitiveSolidKind, RadialDimension,
};

struct Scalar {
    kind: &'static str,
    value: f64,
    parameter: Option<ParameterId>,
    name: Option<String>,
    expression: Option<String>,
}

fn scalar<Q: DimensionValue>(
    target: &CadIr,
    feature: &FeatureId,
    kind: &'static str,
    ordinal: u32,
    dimension: &Dimension<Q>,
) -> Option<Scalar> {
    let ParameterValue::Length(value) = target.dimension_value(dimension)?.to_parameter_value()?
    else {
        return None;
    };
    let source_value = value.get() / 10.0;
    if value.get() != 0.0 && source_value == 0.0 {
        return None;
    }
    let parameter = match dimension {
        Dimension::Literal(_) => None,
        Dimension::Parameter(id) => Some(
            target
                .model
                .parameters
                .iter()
                .find(|parameter| &parameter.id == id)?,
        ),
    };
    if parameter.is_some_and(|parameter| {
        parameter.owner.as_ref() != Some(feature)
            || parameter.ordinal != ordinal
            || !parameter.driving
            || parameter.display.is_some()
            || parameter.pmi.is_some()
            || parameter.product_definition.is_some()
            || !parameter.properties.is_empty()
            || parameter.name.trim().is_empty()
    }) {
        return None;
    }
    let expression = match parameter {
        Some(parameter) => Some(parameter.expression.render_with(|id| {
            target
                .model
                .parameters
                .iter()
                .find(|candidate| &candidate.id == id)
                .map(|candidate| candidate.name.clone())
        })?),
        None => None,
    };
    Some(Scalar {
        kind,
        value: source_value,
        parameter: parameter.map(|parameter| parameter.id.clone()),
        name: parameter.map(|parameter| parameter.name.clone()),
        expression,
    })
}

pub(super) fn encode(
    target: &CadIr,
) -> Result<
    Option<(
        Vec<u8>,
        Vec<u8>,
        Vec<cadmpeg_ir::features::FeatureId>,
        Vec<ParameterId>,
        u32,
    )>,
    CodecError,
> {
    let mut primitives = Vec::new();
    for feature in &target.model.features {
        if feature.ordinal >= u64::from(u32::MAX)
            || feature.suppressed == Some(true)
            || !feature.dependencies.is_empty()
            || !feature.source_properties.is_empty()
            || feature.source_text.is_some()
            || !feature.source_content.is_empty()
            || feature.product_definition.is_some()
        {
            continue;
        }
        let (kind, values, op) = match feature.evaluation.definition() {
            FeatureDefinition::Operation(FeatureOperation::Primitive {
                placement: None,
                solid,
                op,
            }) => {
                let PrimitiveSolidKind::Cylinder {
                    radius: RadialDimension::Diameter(diameter),
                    height,
                    angle: Dimension::Literal(angle),
                } = solid.kind()
                else {
                    continue;
                };
                if *angle != cadmpeg_ir::scalar::Angle::FULL_TURN {
                    continue;
                }
                (
                    "CylinderPrimitive",
                    vec![
                        match scalar(target, &feature.id, "Height", 0, height) {
                            Some(value) => value,
                            None => continue,
                        },
                        match scalar(target, &feature.id, "Diameter", 1, diameter) {
                            Some(value) => value,
                            None => continue,
                        },
                    ],
                    op,
                )
            }
            FeatureDefinition::Operation(FeatureOperation::Block {
                dimensions: Some([length, width, height]),
                placement: Some(placement),
                op,
            }) => {
                let rows = placement.rows();
                if rows[0][0..3] != [1.0, 0.0, 0.0]
                    || rows[1][0..3] != [0.0, 1.0, 0.0]
                    || rows[2] != [0.0, 0.0, 1.0, 0.0]
                    || [rows[0][3], rows[1][3]]
                        .into_iter()
                        .any(|value| value != 0.0 && value / 10.0 == 0.0)
                {
                    continue;
                }
                (
                    "BoxPrimitive",
                    vec![
                        match scalar(target, &feature.id, "Length", 0, length) {
                            Some(value) => value,
                            None => continue,
                        },
                        match scalar(target, &feature.id, "Width", 1, width) {
                            Some(value) => value,
                            None => continue,
                        },
                        match scalar(target, &feature.id, "Height", 2, height) {
                            Some(value) => value,
                            None => continue,
                        },
                        Scalar {
                            kind: "OffsetX",
                            value: rows[0][3] / 10.0,
                            parameter: None,
                            name: None,
                            expression: None,
                        },
                        Scalar {
                            kind: "OffsetY",
                            value: rows[1][3] / 10.0,
                            parameter: None,
                            name: None,
                            expression: None,
                        },
                    ],
                    op,
                )
            }
            _ => continue,
        };
        let generated_name = format!("{} {}", kind, u64::from(feature.ordinal) + 1);
        if feature
            .name
            .as_ref()
            .is_some_and(|name| name != &generated_name)
            || feature.source_tag.as_deref().is_some_and(|tag| tag != kind)
        {
            continue;
        }
        let operation = match op {
            BooleanOp::Join => 1u32,
            BooleanOp::Cut => 2,
            BooleanOp::Intersect => 3,
            BooleanOp::NewBody => 4,
            BooleanOp::Unresolved => continue,
        };
        primitives.push((feature.ordinal, kind, values, operation, feature.id.clone()));
    }
    if primitives.is_empty() {
        return Ok(None);
    }
    primitives.sort_by_key(|primitive| primitive.0);
    let timeline = u32::try_from(primitives.len())
        .ok()
        .and_then(|value| value.checked_mul(16))
        .and_then(|value| value.checked_add(1))
        .ok_or_else(|| CodecError::InvalidInput("too many F3D features".into()))?;
    let collection_index = timeline.checked_add(1).ok_or_else(|| {
        CodecError::InvalidInput("F3D parameter collection index exceeds u32".into())
    })?;
    let mut out = Vec::new();
    let mut primary = Vec::new();
    let mut secondary = Vec::new();
    let mut scopes = Vec::new();
    let mut owners = Vec::new();
    let mut parameters = Vec::new();
    for (ordinal, (source_ordinal, kind, values, operation, _)) in primitives.iter().enumerate() {
        let ordinal = u32::try_from(ordinal)
            .map_err(|_| CodecError::InvalidInput("too many F3D feature records".into()))?;
        let base = ordinal
            .checked_mul(16)
            .and_then(|v| v.checked_add(1))
            .ok_or_else(|| CodecError::InvalidInput("too many F3D feature records".into()))?;
        scopes.push(u64::from(base));
        mark(&mut primary, base, out.len())?;
        header(&mut out, "257", base)?;
        out.extend_from_slice(&[0; 9]);
        out.extend_from_slice(&operation.to_le_bytes());
        out.push(0);
        out.push(1);
        out.extend_from_slice(
            &u32::try_from(values.len())
                .map_err(|_| CodecError::InvalidInput("too many F3D feature parameters".into()))?
                .to_le_bytes(),
        );
        for local in 0..values.len() {
            reference(
                &mut out,
                base + 1
                    + 3 * u32::try_from(local).map_err(|_| {
                        CodecError::InvalidInput("too many F3D feature parameters".into())
                    })?,
            );
        }
        out.extend_from_slice(&u32::MAX.to_le_bytes());
        native_lp_utf16(&mut out, kind)?;
        out.extend_from_slice(
            &source_ordinal
                .checked_add(1)
                .ok_or_else(|| {
                    CodecError::InvalidInput("F3D feature ordinal exceeds u32::MAX".into())
                })?
                .to_le_bytes(),
        );
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
        mark(&mut secondary, base, out.len())?;
        header(&mut out, "256", base)?;
        for (local, scalar) in values.iter().enumerate() {
            let local = u32::try_from(local)
                .map_err(|_| CodecError::InvalidInput("too many F3D feature parameters".into()))?;
            let value = scalar.value;
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
            mark(
                &mut secondary,
                owner_index,
                out.len() - crate::layout::indexed_design_record_header::LEN,
            )?;
            let parameter = DesignParameter::authored(DesignParameterContent {
                id: format!("f3d:generated:parameter#{parameter_index}"),
                class_tag: "259".to_owned().try_into().map_err(CodecError::Malformed)?,
                record_index: parameter_index,
                source_ordinal: local,
                source: DesignParameterSource::authored(
                    scalar.kind.into(),
                    Some(owner_index),
                    Some(DesignParameterDiscriminator::Code3),
                )
                .map_err(CodecError::Malformed)?,
                expression: scalar
                    .expression
                    .clone()
                    .unwrap_or_else(|| format!("{} cm", value)),
                unit: Some("cm".into()),
                name: scalar
                    .name
                    .clone()
                    .unwrap_or_else(|| format!("{}{base}", scalar.kind)),
                evaluated_value: value,
            })
            .map_err(CodecError::Malformed)?;
            mark(&mut primary, parameter_index, out.len())?;
            out.extend_from_slice(&super::parameters::encode_document_parameters(
                &[parameter],
                collection_index,
            )?);
        }
    }
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
    mark(&mut primary, collection_index, out.len())?;
    let parameter_indices = parameters
        .iter()
        .map(|value| {
            u32::try_from(*value)
                .map_err(|_| CodecError::InvalidInput("F3D parameter index exceeds u32".into()))
        })
        .collect::<Result<Vec<_>, _>>()?;
    super::parameters::encode_collection(&mut out, "261", collection_index, &parameter_indices)?;
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
        (
            "B0556F58-EE09-46A7-B18F-E296F48E5B13",
            Some("A7AEA631-985B-4DD1-8CE2-DE2C-14B54081"),
            3,
            "CommonData",
            vec![u64::from(collection_index)],
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
    let meta = encode_design_metastream(&registry, &primary, &secondary)?
        .ok_or_else(|| CodecError::Malformed("missing generated feature metadata".into()))?;
    let written_parameters = primitives
        .iter()
        .flat_map(|(_, _, values, _, _)| values.iter().filter_map(|value| value.parameter.clone()))
        .collect();
    let written = primitives.into_iter().map(|(_, _, _, _, id)| id).collect();
    Ok(Some((
        out,
        meta,
        written,
        written_parameters,
        collection_index,
    )))
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
