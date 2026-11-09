// SPDX-License-Identifier: Apache-2.0
//! Regenerate semantic dimension payloads from the current document.

use std::collections::{BTreeMap, HashSet};

use cadmpeg_core::decode::u64_from_index;
use cadmpeg_ir::attributes::{AttributeTarget, AttributeValue};
use cadmpeg_ir::features::Dimension;
use cadmpeg_ir::pmi::{DimensionKind, PmiDefinition, PmiQuantity};
use cadmpeg_ir::report::loss::LossNote;
use cadmpeg_ir::CadIr;

use crate::loss::SldprtLossCode;
use crate::native::SldprtNative;

use super::database;

struct Record {
    guid: String,
    cad_text: String,
    subtype: String,
    value: f64,
    precision: i64,
    basic: bool,
    inspection: bool,
    reference_only: bool,
    text: Option<String>,
}

pub(crate) fn reduce(
    ir: &mut CadIr,
    losses: &mut Vec<LossNote>,
) -> Result<(), crate::writer::write_error::WriteError> {
    let native = ir
        .native
        .namespace("sldprt")
        .map(SldprtNative::load)
        .transpose()?;
    let supported = ir
        .model
        .pmi
        .iter()
        .filter_map(|annotation| {
            let PmiDefinition::Dimension(dimension) = &annotation.definition else {
                return None;
            };
            (supported_dimension(ir, dimension)
                || native_dimension_without_neutral_value(
                    native.as_ref(),
                    &annotation.id,
                    dimension,
                )
                .is_some())
            .then_some(annotation.id.clone())
        })
        .collect::<HashSet<_>>();
    let mut kept = Vec::new();
    let mut presented = HashSet::new();
    for mut annotation in std::mem::take(&mut ir.model.pmi) {
        let supported_record = match &mut annotation.definition {
            PmiDefinition::Dimension(dimension) => {
                if dimension.tolerance().is_some() {
                    losses.push(SldprtLossCode::WriterFieldsOmitted.note(format!(
                        "PMI {}: dimensional tolerance has no semantic database encoding",
                        annotation.id
                    )));
                    let replacement = cadmpeg_ir::pmi::PmiDimension::new(
                        dimension.kind().clone(),
                        dimension.nominal().cloned(),
                        None,
                    );
                    *dimension =
                        replacement.map_err(crate::writer::write_error::WriteError::unwritable)?;
                }
                supported.contains(&annotation.id)
            }
            PmiDefinition::Presentation {
                placement,
                semantics,
                ..
            } => {
                if placement.take().is_some() {
                    losses.push(SldprtLossCode::WriterFieldsOmitted.note(format!(
                        "PMI {}: presentation placement has no semantic database encoding",
                        annotation.id
                    )));
                }
                semantics.len() == 1
                    && supported.contains(&semantics[0])
                    && presented.insert(semantics[0].clone())
            }
            _ => false,
        };
        if supported_record {
            if !annotation.targets.is_empty()
                || annotation.visible.is_some()
                || annotation.name.is_some()
            {
                losses.push(SldprtLossCode::WriterFieldsOmitted.note(format!("PMI {}: name, visibility, and geometric attachment fields have no semantic database encoding", annotation.id)));
                annotation.targets.clear();
                annotation.visible = None;
                annotation.name = None;
            }
            kept.push(annotation);
        } else {
            losses.push(SldprtLossCode::WriterEntityOmitted.note(format!(
                "PMI {}: semantic family or unevaluated nominal has no semantic database encoding",
                annotation.id
            )));
        }
    }
    ir.model.pmi = kept;
    let kept = ir
        .model
        .pmi
        .iter()
        .map(|annotation| &annotation.id)
        .collect::<HashSet<_>>();
    for parameter in &mut ir.model.parameters {
        if parameter
            .pmi
            .as_ref()
            .is_some_and(|pmi| !kept.contains(pmi))
        {
            parameter.pmi = None;
        }
    }
    Ok(())
}

/// Whether a record without a neutral annotation is the source of a history
/// dimension: its `cadText` names a parameter of a uniquely named feature
/// whose Keywords record omits that dimension.
fn supplies_history_dimension(ir: &CadIr, record: &crate::records::PmiDimension) -> bool {
    let Ok(id) = super::annotation_id(record) else {
        return false;
    };
    if ir.model.pmi.iter().any(|annotation| annotation.id == id) {
        return false;
    }
    let Some((name, owner)) = record.cad_text.split_once('@') else {
        return false;
    };
    let mut owners = ir
        .model
        .features
        .iter()
        .filter(|feature| feature.name.as_deref() == Some(owner));
    let (Some(owner), None) = (owners.next(), owners.next()) else {
        return false;
    };
    ir.model
        .parameters
        .iter()
        .any(|parameter| parameter.owner.as_ref() == Some(&owner.id) && parameter.name == name)
}

fn native_dimension_without_neutral_value<'a>(
    native: Option<&'a SldprtNative>,
    id: &cadmpeg_ir::ids::PmiId,
    dimension: &cadmpeg_ir::pmi::PmiDimension,
) -> Option<&'a crate::records::PmiDimension> {
    let DimensionKind::Other(kind) = dimension.kind() else {
        return None;
    };
    if dimension.nominal().is_some() || kind == "count" {
        return None;
    }
    native?.pmi_dimensions.iter().find(|record| {
        record.subtype == *kind
            && super::annotation_id(record).is_ok_and(|candidate| candidate == *id)
    })
}

pub(crate) fn attribute_supported(attribute: &cadmpeg_ir::attributes::SourceAttribute) -> bool {
    if !matches!(attribute.target, AttributeTarget::Pmi(_)) {
        return false;
    }
    matches!(
        (attribute.name.as_str(), attribute.values.as_slice()),
        ("precision", [Some(AttributeValue::Integer(_))])
            | (
                "basic" | "inspection" | "reference_only",
                [Some(AttributeValue::Boolean(_))]
            )
    )
}

fn supported_dimension(ir: &CadIr, dimension: &cadmpeg_ir::pmi::PmiDimension) -> bool {
    let Some(value) = dimension.resolved_nominal(ir) else {
        return false;
    };
    match dimension.kind() {
        DimensionKind::Size
        | DimensionKind::Location
        | DimensionKind::Diameter
        | DimensionKind::Radius => value.quantity == PmiQuantity::Length,
        DimensionKind::Angular => value.quantity == PmiQuantity::Angle,
        DimensionKind::Other(kind) => {
            kind == "count"
                && value.quantity == PmiQuantity::Ratio
                && super::exact_count(value.value.get()).is_some()
                && dimension
                    .nominal()
                    .and_then(Dimension::parameter)
                    .is_some_and(|id| {
                        ir.model
                            .parameters
                            .iter()
                            .find(|parameter| &parameter.id == id)
                            .and_then(|parameter| parameter.owner.as_ref())
                            .and_then(|owner| {
                                ir.model
                                    .features
                                    .iter()
                                    .find(|feature| &feature.id == owner)
                            })
                            .is_some_and(|feature| {
                                matches!(
                                    feature.evaluation.definition(),
                                    cadmpeg_ir::features::FeatureDefinition::Operation(
                                        cadmpeg_ir::features::FeatureOperation::Pattern { .. }
                                    )
                                )
                            })
                    })
        }
    }
}

pub(crate) fn payload(
    ir: &CadIr,
    native: Option<&SldprtNative>,
) -> Result<Option<Vec<u8>>, crate::writer::write_error::WriteError> {
    let records = records(ir, native)?;
    if records.is_empty() {
        return Ok(None);
    }
    let entries = records
        .into_values()
        .map(|record| {
            let (value, _) = record_payload(&record)?;
            Ok((record.guid, value))
        })
        .collect::<Result<Vec<_>, crate::writer::write_error::WriteError>>()?;
    Ok(database::encode(&entries).map(Some)?)
}

/// The map and display-text offsets the authored database gives each
/// record, by GUID. A record whose value overflows its cell has none.
pub(crate) fn authored_offsets(
    ir: &CadIr,
    native: Option<&SldprtNative>,
) -> Result<BTreeMap<String, (u64, u64)>, crate::writer::write_error::WriteError> {
    let mut offsets = BTreeMap::new();
    for (index, record) in records(ir, native)?.into_values().enumerate() {
        let (value, text) = record_payload(&record)?;
        if let Some(map) = database::local_value_offset(index, record.guid.len(), value.len()) {
            offsets.insert(record.guid, (map, map + u64_from_index(text)));
        }
    }
    Ok(offsets)
}

fn records(
    ir: &CadIr,
    native: Option<&SldprtNative>,
) -> Result<BTreeMap<String, Record>, crate::writer::write_error::WriteError> {
    let mut records = BTreeMap::<String, Record>::new();
    for source in native.into_iter().flat_map(|native| &native.pmi_dimensions) {
        if ir.scope.model && !supplies_history_dimension(ir, source) {
            continue;
        }
        if source.item_count.get() != 1 {
            return Err(crate::writer::write_error::WriteError::Decline(format!(
                "SLDPRT PMI {} contains untyped additional items",
                source.id
            )));
        }
        records.insert(
            source.guid.clone(),
            Record {
                guid: source.guid.clone(),
                cad_text: source.cad_text.clone(),
                subtype: source.subtype.clone(),
                value: source.value.get(),
                precision: source.precision,
                basic: source.basic,
                inspection: source.inspection,
                reference_only: source.reference_only,
                text: source.display_text().map(str::to_owned),
            },
        );
    }
    for annotation in &ir.model.pmi {
        let PmiDefinition::Dimension(dimension) = &annotation.definition else {
            if matches!(&annotation.definition, PmiDefinition::Presentation { semantics, .. } if semantics.len() == 1)
            {
                continue;
            }
            return Err(crate::writer::write_error::WriteError::Decline(format!(
                "SLDPRT PMI {} has no semantic database encoding",
                annotation.id
            )));
        };
        if let Some(source) =
            native_dimension_without_neutral_value(native, &annotation.id, dimension)
        {
            let mut record = Record {
                guid: source.guid.clone(),
                cad_text: source.cad_text.clone(),
                subtype: source.subtype.clone(),
                value: source.value.get(),
                precision: 6,
                basic: false,
                inspection: false,
                reference_only: false,
                text: None,
            };
            apply_neutral_metadata(ir, &annotation.id, &mut record);
            records.insert(source.guid.clone(), record);
            continue;
        }
        if !supported_dimension(ir, dimension)
            || dimension.tolerance().is_some()
            || !annotation.targets.is_empty()
            || annotation.name.is_some()
            || annotation.visible.is_some()
        {
            return Err(crate::writer::write_error::WriteError::Decline(format!(
                "SLDPRT PMI {} requires semantic-field reduction",
                annotation.id
            )));
        }
        let value = dimension.resolved_nominal(ir).ok_or_else(|| {
            crate::writer::write_error::WriteError::Decline(format!(
                "SLDPRT PMI {} has no evaluated nominal",
                annotation.id
            ))
        })?;
        let subtype = match dimension.kind() {
            DimensionKind::Size => "Linear",
            DimensionKind::Location => "Ordinate",
            DimensionKind::Angular => "Angle",
            DimensionKind::Diameter => "Diameter",
            DimensionKind::Radius => "Radial",
            DimensionKind::Other(_) => "",
        };
        let source = native
            .into_iter()
            .flat_map(|native| &native.pmi_dimensions)
            .find(|source| super::annotation_id(source).is_ok_and(|id| id == annotation.id));
        let guid = source.map_or_else(
            || guid_for(annotation.id.as_str()),
            |source| source.guid.clone(),
        );
        let cad_text = dimension
            .nominal()
            .and_then(Dimension::parameter)
            .and_then(|id| {
                ir.model
                    .parameters
                    .iter()
                    .find(|parameter| parameter.id == *id)
            })
            .map_or_else(
                || annotation.id.as_str().to_owned(),
                |parameter| {
                    let owner = parameter
                        .owner
                        .as_ref()
                        .and_then(|id| ir.model.features.iter().find(|feature| feature.id == *id))
                        .and_then(|feature| feature.name.as_deref());
                    owner.map_or_else(
                        || parameter.name.clone(),
                        |owner| format!("{}@{owner}", parameter.name),
                    )
                },
            );
        let mut record = Record {
            guid,
            cad_text,
            subtype: subtype.into(),
            value: value.value.get()
                * if value.quantity == PmiQuantity::Length {
                    0.001
                } else {
                    1.0
                },
            precision: 6,
            basic: false,
            inspection: false,
            reference_only: false,
            text: None,
        };
        apply_neutral_metadata(ir, &annotation.id, &mut record);
        records.insert(record.guid.clone(), record);
    }
    Ok(records)
}

fn apply_neutral_metadata(ir: &CadIr, id: &cadmpeg_ir::ids::PmiId, record: &mut Record) {
    let attribute = |name: &str| {
        ir.model
            .attributes
            .iter()
            .find(|attribute| {
                attribute.target == AttributeTarget::Pmi(id.clone()) && attribute.name == name
            })
            .and_then(|attribute| attribute.values.first().and_then(Option::as_ref))
    };
    record.precision = match attribute("precision") {
        Some(AttributeValue::Integer(value)) => *value,
        _ => 6,
    };
    record.basic = matches!(attribute("basic"), Some(AttributeValue::Boolean(true)));
    record.inspection = matches!(attribute("inspection"), Some(AttributeValue::Boolean(true)));
    record.reference_only = matches!(
        attribute("reference_only"),
        Some(AttributeValue::Boolean(true))
    );
    record.text = ir
        .model
        .pmi
        .iter()
        .find_map(|presentation| match &presentation.definition {
            PmiDefinition::Presentation {
                text, semantics, ..
            } if semantics.as_slice() == [id.clone()] => text.clone(),
            _ => None,
        });
}

/// The record's `MessagePack` map and the offset of its display text in it.
fn record_payload(
    record: &Record,
) -> Result<(Vec<u8>, usize), crate::writer::write_error::WriteError> {
    let mut out = vec![0x87];
    string(&mut out, "annoType")?;
    out.push(1);
    string(&mut out, "cadText")?;
    string(&mut out, &record.cad_text)?;
    string(&mut out, "dimItems")?;
    out.extend_from_slice(&[0x91, 0x87]);
    string(&mut out, "class")?;
    string(&mut out, "DimSemData")?;
    string(&mut out, "dimSubType")?;
    string(&mut out, &record.subtype)?;
    string(&mut out, "isBasic")?;
    boolean(&mut out, record.basic);
    string(&mut out, "isInspection")?;
    boolean(&mut out, record.inspection);
    string(&mut out, "isReferenceOnly")?;
    boolean(&mut out, record.reference_only);
    string(&mut out, "valPrecision")?;
    out.push(0xd3);
    out.extend_from_slice(&record.precision.to_be_bytes());
    string(&mut out, "value")?;
    out.push(0xcb);
    out.extend_from_slice(&record.value.to_be_bytes());
    string(&mut out, "dimText")?;
    let text = record.text.as_deref().unwrap_or("");
    string(&mut out, text)?;
    let text_offset = out.len() - text.len();
    string(&mut out, "dimType")?;
    out.push(0);
    string(&mut out, "iDString")?;
    string(&mut out, &record.guid)?;
    string(&mut out, "reserved")?;
    out.push(0xc0);
    Ok((out, text_offset))
}

fn string(out: &mut Vec<u8>, value: &str) -> Result<(), crate::writer::write_error::WriteError> {
    // (O-capacity): MessagePack strings have at most a u32 byte length.
    let length = u32::try_from(value.len()).map_err(|_| {
        crate::writer::write_error::WriteError::Unwritable(
            "SLDPRT PMI text exceeds the MessagePack string capacity".into(),
        )
    })?;
    if let Ok(length) = u8::try_from(length) {
        if length < 32 {
            out.push(0xa0 | length);
        } else {
            out.extend_from_slice(&[0xd9, length]);
        }
    } else {
        out.push(0xdb);
        out.extend_from_slice(&length.to_be_bytes());
    }
    out.extend_from_slice(value.as_bytes());
    Ok(())
}

fn boolean(out: &mut Vec<u8>, value: bool) {
    out.push(if value { 0xc3 } else { 0xc2 });
}

fn guid_for(id: &str) -> String {
    let hash = cadmpeg_ir::hash::sha256_hex(id.as_bytes());
    format!(
        "{}-{}-{}-{}-{}",
        &hash[..8],
        &hash[8..12],
        &hash[12..16],
        &hash[16..20],
        &hash[20..32]
    )
}

pub(super) fn valid_guid(value: &str) -> bool {
    value.len() == 36
        && value.bytes().enumerate().all(|(index, byte)| {
            if [8, 13, 18, 23].contains(&index) {
                byte == b'-'
            } else {
                byte.is_ascii_hexdigit()
            }
        })
}

#[cfg(test)]
mod tests;
