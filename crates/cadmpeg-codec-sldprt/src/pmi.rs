// SPDX-License-Identifier: Apache-2.0
//! Semantic dimension records stored in `PMISemanticDataDB`.

use std::collections::{BTreeMap, HashMap, HashSet};

use cadmpeg_core::decode::View;
use cadmpeg_ir::annotations::Annotations;
use cadmpeg_ir::report::LossNote;
use cadmpeg_ir::Exactness;
use rmp::Marker;

use crate::container::ContainerScan;
use crate::loss::SldprtLossCode;
use crate::records::PmiDimension;

fn exact_count(value: f64) -> Option<i64> {
    let count = value as i64;
    (count >= 0 && count as f64 == value).then_some(count)
}

fn dimension_subtype(
    record: &PmiDimension,
    empty_subtype_is_count: bool,
) -> cadmpeg_ir::features::PmiDimensionSubtype {
    use cadmpeg_ir::features::PmiDimensionSubtype;

    match record.subtype.as_str() {
        "Linear" => PmiDimensionSubtype::Linear,
        "Angle" => PmiDimensionSubtype::Angle,
        "Diameter" => PmiDimensionSubtype::Diameter,
        "Radial" => PmiDimensionSubtype::Radial,
        "Ordinate" => PmiDimensionSubtype::Ordinate,
        "" if empty_subtype_is_count && exact_count(record.value).is_some() => {
            PmiDimensionSubtype::Count
        }
        other => PmiDimensionSubtype::Native(other.to_string()),
    }
}

fn neutral_parameter_is_count(
    feature: &cadmpeg_ir::features::Feature,
    name: &str,
    value: Option<&cadmpeg_ir::features::ParameterValue>,
) -> bool {
    use cadmpeg_ir::features::{FeatureDefinition, ParameterValue, PatternKind};

    matches!(value, Some(ParameterValue::Integer(_)))
        || (matches!(name, "D1" | "D2")
            && matches!(
                &feature.definition,
                FeatureDefinition::Pattern {
                    pattern: PatternKind::Linear { .. } | PatternKind::LinearOffsets { .. },
                    ..
                }
            ))
}

#[cfg(test)]
mod tests;

/// Return whether two retained records encode the same semantic dimension.
///
/// Record identity and byte locations are intentionally excluded. `SolidWorks`
/// can retain multiple GUID records for one owner-qualified dimension. Every
/// editable semantic field must agree before those records are aliases.
pub(crate) fn equivalent_dimensions(left: &PmiDimension, right: &PmiDimension) -> bool {
    left.cad_text == right.cad_text
        && left.item_count == right.item_count
        && left.subtype == right.subtype
        && left.value.to_bits() == right.value.to_bits()
        && left.precision == right.precision
        && left.display_text == right.display_text
        && left.basic == right.basic
        && left.inspection == right.inspection
        && left.reference_only == right.reference_only
}

/// Return one deterministic representative for each owner-qualified dimension
/// whose retained records all agree semantically.
pub(crate) fn agreed_dimension_records(records: &[PmiDimension]) -> Vec<&PmiDimension> {
    let mut groups = BTreeMap::<&str, Vec<&PmiDimension>>::new();
    for record in records {
        groups
            .entry(record.cad_text.as_str())
            .or_default()
            .push(record);
    }

    let mut representatives = groups
        .into_values()
        .filter_map(|mut group| {
            group.sort_unstable_by(|left, right| left.id.cmp(&right.id));
            let canonical = *group.first()?;
            (canonical.item_count == 1
                && group.iter().all(|record| {
                    record.item_count == 1 && equivalent_dimensions(canonical, record)
                }))
            .then_some(canonical)
        })
        .collect::<Vec<_>>();
    representatives.sort_unstable_by(|left, right| left.id.cmp(&right.id));
    representatives
}

/// Count native semantic dimensions not represented by a bound record or one
/// of its semantically identical retained aliases.
pub(crate) fn unbound_dimension_count(
    records: &[PmiDimension],
    bound_ids: &HashSet<&str>,
) -> usize {
    let bound = records
        .iter()
        .filter(|record| bound_ids.contains(record.id.as_str()))
        .collect::<Vec<_>>();
    records
        .iter()
        .filter(|record| {
            !bound_ids.contains(record.id.as_str())
                && !bound
                    .iter()
                    .any(|candidate| equivalent_dimensions(record, candidate))
        })
        .count()
}

/// Add uniquely owner-qualified PMI dimensions to a projection copy of history.
pub(crate) fn enrich_history_parameters(
    histories: &mut [crate::records::FeatureHistory],
    records: &[PmiDimension],
) {
    enrich_history_parameters_with_features(histories, records, &[]);
}

/// Add uniquely owner-qualified PMI dimensions with neutral owner context.
pub(crate) fn enrich_history_parameters_with_features(
    histories: &mut [crate::records::FeatureHistory],
    records: &[PmiDimension],
    neutral_features: &[cadmpeg_ir::features::Feature],
) {
    let mut owners = BTreeMap::<String, Vec<(usize, usize)>>::new();
    for (history_index, history) in histories.iter().enumerate() {
        for (feature_index, feature) in history.features.iter().enumerate() {
            owners
                .entry(feature.name.clone())
                .or_default()
                .push((history_index, feature_index));
        }
    }
    for record in agreed_dimension_records(records) {
        let Some((name, owner_name)) = record.cad_text.split_once('@') else {
            continue;
        };
        let Some([(history_index, feature_index)]) = owners.get(owner_name).map(Vec::as_slice)
        else {
            continue;
        };
        let millimetres = record.value * 1000.0;
        let feature = &histories[*history_index].features[*feature_index];
        let empty_subtype_is_count = feature.parameters.get(name).is_some_and(|expression| {
            matches!(
                crate::history::parse_native_parameter_literal(feature, name, expression),
                Some(cadmpeg_ir::features::ParameterValue::Integer(_))
            )
        }) || neutral_features.iter().any(|neutral| {
            neutral.native_ref.as_deref() == Some(feature.id.as_str())
                && neutral_parameter_is_count(neutral, name, None)
        });
        let expression = match dimension_subtype(record, empty_subtype_is_count) {
            cadmpeg_ir::features::PmiDimensionSubtype::Linear
            | cadmpeg_ir::features::PmiDimensionSubtype::Ordinate => {
                format!("{millimetres}mm")
            }
            cadmpeg_ir::features::PmiDimensionSubtype::Angle => record.value.to_string(),
            cadmpeg_ir::features::PmiDimensionSubtype::Diameter => {
                format!("<MOD-DIAM>{millimetres}mm")
            }
            cadmpeg_ir::features::PmiDimensionSubtype::Radial => {
                format!("R{millimetres}mm")
            }
            cadmpeg_ir::features::PmiDimensionSubtype::Count => match exact_count(record.value) {
                Some(count) => count.to_string(),
                None => continue,
            },
            cadmpeg_ir::features::PmiDimensionSubtype::Native(_) => continue,
        };
        histories[*history_index].features[*feature_index]
            .parameters
            .entry(name.to_string())
            .or_insert(expression);
    }
}

pub(crate) fn patch_payload(
    ir: &cadmpeg_ir::CadIr,
    block_id: &str,
    payload: &mut [u8],
) -> Result<(), cadmpeg_core::CodecError> {
    use cadmpeg_ir::features::{ParameterValue, PmiDimensionSubtype};

    let Some(namespace) = ir.native.namespace("sldprt") else {
        return Ok(());
    };
    let native = crate::native::SldprtNative::load(namespace).map_err(|error| {
        cadmpeg_core::CodecError::malformed(format_args!("invalid SLDPRT native PMI: {error}"))
    })?;
    let records_by_id = native
        .pmi_dimensions
        .iter()
        .map(|record| (record.id.as_str(), record))
        .collect::<HashMap<_, _>>();
    for record in native
        .pmi_dimensions
        .iter()
        .filter(|record| record.parent == block_id)
    {
        if record.item_count != 1 {
            continue;
        }
        let mut parameters = ir.model.parameters.iter().filter(|parameter| {
            parameter.pmi.as_ref().is_some_and(|pmi| {
                pmi.native_ref == record.id
                    || records_by_id
                        .get(pmi.native_ref.as_str())
                        .is_some_and(|bound| equivalent_dimensions(record, bound))
            })
        });
        let Some(parameter) = parameters.next() else {
            continue;
        };
        if parameters.next().is_some() {
            return Err(cadmpeg_core::CodecError::malformed(format_args!(
                "multiple parameters reference PMI record {}",
                record.id
            )));
        }
        let semantic = parameter.pmi.as_ref().expect("filtered above");
        let empty_subtype_is_count = semantic.subtype == PmiDimensionSubtype::Count;
        let subtype = dimension_subtype(record, empty_subtype_is_count);
        if semantic.subtype != subtype {
            return Err(cadmpeg_core::CodecError::NotImplemented(format!(
                "SLDPRT PMI record {} changes dimension subtype",
                record.id
            )));
        }
        let native_value = match (&subtype, &parameter.value) {
            (PmiDimensionSubtype::Angle, Some(ParameterValue::Angle(angle))) => angle.0,
            (
                PmiDimensionSubtype::Linear
                | PmiDimensionSubtype::Diameter
                | PmiDimensionSubtype::Radial
                | PmiDimensionSubtype::Ordinate,
                Some(ParameterValue::Length(length)),
            ) => length.0 / 1000.0,
            (PmiDimensionSubtype::Count, Some(ParameterValue::Integer(count))) => *count as f64,
            _ => {
                return Err(cadmpeg_core::CodecError::NotImplemented(format!(
                    "SLDPRT PMI record {} has a value incompatible with its dimension subtype",
                    record.id
                )));
            }
        };
        patch_bytes(
            payload,
            record.value_offset,
            &native_value.to_be_bytes(),
            &record.id,
        )?;
        let precision = u8::try_from(semantic.precision)
            .ok()
            .filter(|value| *value < 128)
            .ok_or_else(|| {
                cadmpeg_core::CodecError::NotImplemented(format!(
                    "SLDPRT PMI record {} requires fixint precision",
                    record.id
                ))
            })?;
        patch_bytes(payload, record.precision_offset, &[precision], &record.id)?;
        for (offset, value) in [
            (record.basic_offset, semantic.basic),
            (record.inspection_offset, semantic.inspection),
            (record.reference_only_offset, semantic.reference_only),
        ] {
            patch_bytes(
                payload,
                offset,
                &[if value { 0xc3 } else { 0xc2 }],
                &record.id,
            )?;
        }
        if semantic.display_text != record.display_text {
            let (Some(offset), Some(text), Some(previous)) = (
                record.display_text_offset,
                semantic.display_text.as_deref(),
                record.display_text.as_deref(),
            ) else {
                return Err(cadmpeg_core::CodecError::NotImplemented(format!(
                    "SLDPRT PMI record {} changes optional display text",
                    record.id
                )));
            };
            if text.len() != previous.len() {
                return Err(cadmpeg_core::CodecError::NotImplemented(format!(
                    "SLDPRT PMI record {} changes display-text width",
                    record.id
                )));
            }
            patch_bytes(payload, offset, text.as_bytes(), &record.id)?;
        }
    }
    Ok(())
}

fn patch_bytes(
    payload: &mut [u8],
    offset: u64,
    bytes: &[u8],
    record: &str,
) -> Result<(), cadmpeg_core::CodecError> {
    let start = usize::try_from(offset).map_err(|_| {
        cadmpeg_core::CodecError::malformed(format_args!(
            "SLDPRT PMI record {record} exceeds address space"
        ))
    })?;
    let end = start.checked_add(bytes.len()).ok_or_else(|| {
        cadmpeg_core::CodecError::malformed(format_args!(
            "SLDPRT PMI record {record} offset overflows"
        ))
    })?;
    payload
        .get_mut(start..end)
        .ok_or_else(|| {
            cadmpeg_core::CodecError::malformed(format_args!(
                "SLDPRT PMI record {record} lies outside its block"
            ))
        })?
        .copy_from_slice(bytes);
    Ok(())
}

pub(crate) fn apply_to_parameters(
    parameters: &mut Vec<cadmpeg_ir::features::DesignParameter>,
    features: &[cadmpeg_ir::features::Feature],
    records: &[PmiDimension],
) {
    use cadmpeg_ir::features::{
        DesignParameter, DimensionDisplay, Length, ParameterId, ParameterPmi, ParameterValue,
        PmiDimensionSubtype,
    };

    let mut feature_names = BTreeMap::<&str, Vec<&cadmpeg_ir::features::Feature>>::new();
    for feature in features {
        if let Some(name) = feature.name.as_deref() {
            feature_names.entry(name).or_default().push(feature);
        }
    }
    for record in agreed_dimension_records(records) {
        let Some((name, owner_name)) = record.cad_text.split_once('@') else {
            continue;
        };
        let Some([owner]) = feature_names.get(owner_name).map(Vec::as_slice) else {
            continue;
        };
        let existing_parameter = parameters.iter().position(|parameter| {
            parameter.owner.as_ref() == Some(&owner.id) && parameter.name == name
        });
        let empty_subtype_is_count = neutral_parameter_is_count(
            owner,
            name,
            existing_parameter.and_then(|index| parameters[index].value.as_ref()),
        );
        let subtype = dimension_subtype(record, empty_subtype_is_count);
        let millimetres = record.value * 1000.0;
        let (expression, display, value) = match subtype {
            PmiDimensionSubtype::Linear => (
                format!("{millimetres}mm"),
                None,
                Some(ParameterValue::Length(Length(millimetres))),
            ),
            PmiDimensionSubtype::Angle => (
                record.value.to_string(),
                None,
                Some(ParameterValue::Angle(cadmpeg_ir::features::Angle(
                    record.value,
                ))),
            ),
            PmiDimensionSubtype::Diameter => (
                format!("<MOD-DIAM>{millimetres}mm"),
                Some(DimensionDisplay::Diameter),
                Some(ParameterValue::Length(Length(millimetres))),
            ),
            PmiDimensionSubtype::Radial => (
                format!("R{millimetres}mm"),
                Some(DimensionDisplay::Radius),
                Some(ParameterValue::Length(Length(millimetres))),
            ),
            PmiDimensionSubtype::Ordinate => (
                format!("{millimetres}mm"),
                None,
                Some(ParameterValue::Length(Length(millimetres))),
            ),
            PmiDimensionSubtype::Count => {
                let Some(count) = exact_count(record.value) else {
                    continue;
                };
                (
                    count.to_string(),
                    None,
                    Some(ParameterValue::Integer(count)),
                )
            }
            PmiDimensionSubtype::Native(_) => (record.value.to_string(), None, None),
        };
        let semantic = ParameterPmi {
            subtype,
            precision: record.precision,
            display_text: record.display_text.clone(),
            basic: record.basic,
            inspection: record.inspection,
            reference_only: record.reference_only,
            native_ref: record.id.clone(),
        };
        if let Some(parameter) = existing_parameter.map(|index| &mut parameters[index]) {
            // Keywords is the authoritative design value when it already
            // supplied this parameter. PMI still contributes its semantic
            // annotation and source identity.
            parameter.pmi = Some(semantic);
            continue;
        }
        let ordinal = parameters
            .iter()
            .filter(|parameter| parameter.owner.as_ref() == Some(&owner.id))
            .map(|parameter| parameter.ordinal)
            .max()
            .map_or(0, |ordinal| ordinal.saturating_add(1));
        parameters.push(DesignParameter {
            id: ParameterId(format!("sldprt:model:parameter#pmi:{}", record.guid)),
            owner: Some(owner.id.clone()),
            ordinal,
            name: name.to_string(),
            expression,
            display,
            value,
            dependencies: Vec::new(),
            properties: BTreeMap::new(),
            pmi: Some(semantic),
            native_ref: None,
        });
    }
}

/// One `MessagePack` value with absolute source spans for in-place patching.
#[derive(Debug, Clone)]
struct SpannedValue {
    kind: ValueKind,
    /// Absolute offset of this value's marker byte.
    start: usize,
    /// Absolute offset of the writable scalar payload (kind-dependent).
    data_offset: usize,
}

#[derive(Debug, Clone)]
enum ValueKind {
    Bool(bool),
    Int(i64),
    Float(f64),
    String(String),
    Array(Vec<SpannedValue>),
    Map(BTreeMap<String, SpannedValue>),
    Nil,
    /// Invalid-UTF-8 strings, `bin` / `ext`, and out-of-range integers: cursor
    /// advanced, content opaque.
    Opaque,
}

/// Extract semantic dimensions from `PMISemanticDataDB` sections.
///
/// Parse failures on GUID-prefixed `MessagePack` maps emit
/// [`SldprtLossCode::PmiSemanticRecordMalformed`] instead of shrinking the
/// document silently. New losses are additive under sidecar v1.
pub(crate) fn dimensions(
    scan: &ContainerScan,
    annotations: &mut Annotations,
    losses: &mut Vec<LossNote>,
) -> Vec<PmiDimension> {
    let mut records = Vec::new();
    let mut seen = HashSet::<String>::new();
    for source in scan.sections() {
        let Some(section) = source.name() else {
            continue;
        };
        if !section.eq_ignore_ascii_case("Contents/PMISemanticDataDB") {
            continue;
        }
        collect_dimensions(
            source.payload(),
            section,
            &source.native_id(),
            annotations,
            losses,
            &mut records,
            &mut seen,
        );
    }
    records.sort_by(|left, right| left.id.cmp(&right.id));
    records
}

/// Parse PMI records from a raw `PMISemanticDataDB` payload.
///
/// Used by focused tests and the `sldprt_pmi` fuzz target. Parent/section are
/// placeholders; production decode supplies real block identities.
pub(crate) fn parse_payload(payload: &[u8], losses: &mut Vec<LossNote>) -> Vec<PmiDimension> {
    let mut annotations = Annotations::default();
    let mut records = Vec::new();
    let mut seen = HashSet::<String>::new();
    collect_dimensions(
        payload,
        "Contents/PMISemanticDataDB",
        "sldprt:block#pmi-payload",
        &mut annotations,
        losses,
        &mut records,
        &mut seen,
    );
    records.sort_by(|left, right| left.id.cmp(&right.id));
    records
}

fn collect_dimensions(
    payload: &[u8],
    section: &str,
    parent: &str,
    annotations: &mut Annotations,
    losses: &mut Vec<LossNote>,
    records: &mut Vec<PmiDimension>,
    seen: &mut HashSet<String>,
) {
    for (guid, offset) in candidate_maps(payload) {
        if !seen.insert(guid.clone()) {
            continue;
        }
        match extract_dimension(payload, offset, &guid) {
            Ok(Some(partial)) => {
                let id = format!("sldprt:pmi:dimension#{guid}");
                crate::annotations::note(
                    annotations,
                    id.clone(),
                    section,
                    offset as u64,
                    "messagepack_dim_sem_data",
                    Exactness::ByteExact,
                );
                records.push(PmiDimension {
                    id,
                    parent: parent.to_string(),
                    offset: offset as u64,
                    guid,
                    cad_text: partial.cad_text,
                    item_count: partial.item_count,
                    subtype: partial.subtype,
                    value: partial.value,
                    value_offset: partial.value_offset,
                    precision: partial.precision,
                    precision_offset: partial.precision_offset,
                    display_text: partial.display_text,
                    display_text_offset: partial.display_text_offset,
                    basic: partial.basic,
                    basic_offset: partial.basic_offset,
                    inspection: partial.inspection,
                    inspection_offset: partial.inspection_offset,
                    reference_only: partial.reference_only,
                    reference_only_offset: partial.reference_only_offset,
                });
            }
            Ok(None) => {}
            Err(message) => {
                losses.push(SldprtLossCode::PmiSemanticRecordMalformed.note(format!(
                    "PMISemanticDataDB map at offset {offset} (guid {guid}) {message}"
                )));
            }
        }
    }
}

struct PartialDimension {
    cad_text: String,
    item_count: u32,
    subtype: String,
    value: f64,
    value_offset: u64,
    precision: i64,
    precision_offset: u64,
    display_text: Option<String>,
    display_text_offset: Option<u64>,
    basic: bool,
    basic_offset: u64,
    inspection: bool,
    inspection_offset: u64,
    reference_only: bool,
    reference_only_offset: u64,
}

/// `Ok(None)` — not a PMI dimension map. `Err` — PMI candidate that failed.
fn extract_dimension(
    payload: &[u8],
    offset: usize,
    guid: &str,
) -> Result<Option<PartialDimension>, String> {
    let _ = guid;
    let mut cursor = offset;
    let Some(outer_value) = parse_value(payload, &mut cursor, 0) else {
        // Only attribute a loss when the window still names the PMI keys; a
        // bare GUID before an unrelated fixmap is common in UnQLite payloads.
        return if looks_like_pmi_map(payload, offset) {
            Err("failed to parse MessagePack map".into())
        } else {
            Ok(None)
        };
    };
    let ValueKind::Map(outer) = outer_value.kind else {
        return Ok(None);
    };
    let has_cad_text = outer.contains_key("cadText");
    let has_dim_items = outer.contains_key("dimItems");
    if !has_cad_text && !has_dim_items {
        return Ok(None);
    }
    if !has_cad_text || !has_dim_items {
        return Err("map is missing cadText or dimItems".into());
    }
    let Some(cad_text) = string_field(&outer, "cadText") else {
        return Err("cadText is not a string".into());
    };
    let Some(items_value) = outer.get("dimItems") else {
        return Err("dimItems missing after key check".into());
    };
    let ValueKind::Array(items) = &items_value.kind else {
        return Err("dimItems is not an array".into());
    };
    let Ok(item_count) = u32::try_from(items.len()) else {
        return Err("dimItems length exceeds u32".into());
    };
    let Some(item_value) = items.first() else {
        return Err("dimItems is empty".into());
    };
    let ValueKind::Map(item) = &item_value.kind else {
        return Err("first dimItems element is not a map".into());
    };
    if string_field(item, "class") != Some("DimSemData") {
        return Err("first dimItems element is not DimSemData".into());
    }
    let value_field = item
        .get("value")
        .ok_or_else(|| "DimSemData lacks value".to_string())?;
    if payload.get(value_field.start) != Some(&Marker::F64.to_u8()) {
        return Err("value is not an f64 (0xcb) MessagePack float".into());
    }
    let value = float_from(value_field).ok_or_else(|| "value is not a finite float".to_string())?;
    let precision_field = item
        .get("valPrecision")
        .ok_or_else(|| "DimSemData lacks valPrecision".to_string())?;
    let basic_field = item
        .get("isBasic")
        .ok_or_else(|| "DimSemData lacks isBasic".to_string())?;
    let inspection_field = item
        .get("isInspection")
        .ok_or_else(|| "DimSemData lacks isInspection".to_string())?;
    let reference_field = item
        .get("isReferenceOnly")
        .ok_or_else(|| "DimSemData lacks isReferenceOnly".to_string())?;
    Ok(Some(PartialDimension {
        cad_text: cad_text.to_string(),
        item_count,
        subtype: string_field(item, "dimSubType")
            .unwrap_or_default()
            .to_string(),
        value,
        value_offset: value_field.data_offset as u64,
        precision: int_from(precision_field).unwrap_or_default(),
        precision_offset: precision_field.data_offset as u64,
        display_text: string_field(&outer, "dimText").map(str::to_string),
        display_text_offset: outer.get("dimText").map(|field| field.data_offset as u64),
        basic: bool_from(basic_field).unwrap_or(false),
        basic_offset: basic_field.data_offset as u64,
        inspection: bool_from(inspection_field).unwrap_or(false),
        inspection_offset: inspection_field.data_offset as u64,
        reference_only: bool_from(reference_field).unwrap_or(false),
        reference_only_offset: reference_field.data_offset as u64,
    }))
}

/// True when a short window after `offset` still encodes the PMI map keys.
///
/// Used only to decide whether a failed parse is an attributed PMI loss or an
/// unrelated GUID/map collision. It is not the field locator.
fn looks_like_pmi_map(payload: &[u8], offset: usize) -> bool {
    let end = offset.saturating_add(1024).min(payload.len());
    let window = payload.get(offset..end).unwrap_or(&[]);
    contains_fixstr_key(window, "cadText") && contains_fixstr_key(window, "dimItems")
}

fn contains_fixstr_key(window: &[u8], key: &str) -> bool {
    if key.len() >= 32 {
        return false;
    }
    let mut encoded = Vec::with_capacity(key.len() + 1);
    encoded.push(0xa0 | key.len() as u8);
    encoded.extend_from_slice(key.as_bytes());
    window
        .windows(encoded.len())
        .any(|candidate| candidate == encoded)
}

/// Locate GUID-prefixed `MessagePack` maps. Key order and map length do not matter.
fn candidate_maps(payload: &[u8]) -> Vec<(String, usize)> {
    let mut out = Vec::new();
    for (offset, marker) in payload.iter().copied().enumerate() {
        if !matches!(
            Marker::from_u8(marker),
            Marker::FixMap(_) | Marker::Map16 | Marker::Map32
        ) {
            continue;
        }
        if let Some(guid) = guid_before(payload, offset) {
            out.push((guid, offset));
        }
    }
    out
}

fn guid_before(payload: &[u8], offset: usize) -> Option<String> {
    let start = offset.checked_sub(36)?;
    let guid = std::str::from_utf8(payload.get(start..offset)?).ok()?;
    let bytes = guid.as_bytes();
    (bytes.get(8) == Some(&b'-')
        && bytes.get(13) == Some(&b'-')
        && bytes.get(18) == Some(&b'-')
        && bytes.get(23) == Some(&b'-')
        && bytes
            .iter()
            .enumerate()
            .all(|(index, byte)| [8, 13, 18, 23].contains(&index) || byte.is_ascii_hexdigit()))
    .then(|| guid.to_ascii_lowercase())
}

fn parse_value(bytes: &[u8], cursor: &mut usize, depth: usize) -> Option<SpannedValue> {
    if depth > 16 {
        return None;
    }
    let start = *cursor;
    let marker = Marker::from_u8(take_u8(bytes, cursor)?);
    match marker {
        Marker::FixPos(value) => Some(SpannedValue {
            kind: ValueKind::Int(i64::from(value)),
            start,
            data_offset: start,
        }),
        Marker::FixNeg(value) => Some(SpannedValue {
            kind: ValueKind::Int(i64::from(value)),
            start,
            data_offset: start,
        }),
        Marker::FixMap(len) => parse_map(bytes, cursor, usize::from(len), depth, start),
        Marker::FixArray(len) => parse_array(bytes, cursor, usize::from(len), depth, start),
        Marker::FixStr(len) => parse_string(bytes, cursor, usize::from(len), start),
        Marker::Null => Some(SpannedValue {
            kind: ValueKind::Nil,
            start,
            data_offset: start,
        }),
        Marker::False => Some(SpannedValue {
            kind: ValueKind::Bool(false),
            start,
            data_offset: start,
        }),
        Marker::True => Some(SpannedValue {
            kind: ValueKind::Bool(true),
            start,
            data_offset: start,
        }),
        Marker::Bin8 => {
            let len = usize::from(take_u8(bytes, cursor)?);
            skip_bytes(bytes, cursor, len)?;
            Some(opaque(start))
        }
        Marker::Bin16 => {
            let len = usize::from(take_u16(bytes, cursor)?);
            skip_bytes(bytes, cursor, len)?;
            Some(opaque(start))
        }
        Marker::Bin32 => {
            let len = usize::try_from(take_u32(bytes, cursor)?).ok()?;
            skip_bytes(bytes, cursor, len)?;
            Some(opaque(start))
        }
        Marker::Ext8 => {
            let len = usize::from(take_u8(bytes, cursor)?);
            let _typeid = take_u8(bytes, cursor)?;
            skip_bytes(bytes, cursor, len)?;
            Some(opaque(start))
        }
        Marker::Ext16 => {
            let len = usize::from(take_u16(bytes, cursor)?);
            let _typeid = take_u8(bytes, cursor)?;
            skip_bytes(bytes, cursor, len)?;
            Some(opaque(start))
        }
        Marker::Ext32 => {
            let len = usize::try_from(take_u32(bytes, cursor)?).ok()?;
            let _typeid = take_u8(bytes, cursor)?;
            skip_bytes(bytes, cursor, len)?;
            Some(opaque(start))
        }
        Marker::F32 => {
            let bits = take_u32(bytes, cursor)?;
            Some(SpannedValue {
                kind: ValueKind::Float(f64::from(f32::from_bits(bits))),
                start,
                data_offset: start + 1,
            })
        }
        Marker::F64 => {
            let bits = take_u64(bytes, cursor)?;
            Some(SpannedValue {
                kind: ValueKind::Float(f64::from_bits(bits)),
                start,
                data_offset: start + 1,
            })
        }
        Marker::U8 => Some(SpannedValue {
            kind: ValueKind::Int(i64::from(take_u8(bytes, cursor)?)),
            start,
            data_offset: start + 1,
        }),
        Marker::U16 => Some(SpannedValue {
            kind: ValueKind::Int(i64::from(take_u16(bytes, cursor)?)),
            start,
            data_offset: start + 1,
        }),
        Marker::U32 => Some(SpannedValue {
            kind: ValueKind::Int(i64::from(take_u32(bytes, cursor)?)),
            start,
            data_offset: start + 1,
        }),
        Marker::U64 => {
            let value = take_u64(bytes, cursor)?;
            Some(SpannedValue {
                kind: i64::try_from(value).map_or(ValueKind::Opaque, ValueKind::Int),
                start,
                data_offset: start + 1,
            })
        }
        Marker::I8 => Some(SpannedValue {
            kind: ValueKind::Int(i64::from(take_u8(bytes, cursor)? as i8)),
            start,
            data_offset: start + 1,
        }),
        Marker::I16 => Some(SpannedValue {
            kind: ValueKind::Int(i64::from(take_u16(bytes, cursor)? as i16)),
            start,
            data_offset: start + 1,
        }),
        Marker::I32 => Some(SpannedValue {
            kind: ValueKind::Int(i64::from(take_u32(bytes, cursor)? as i32)),
            start,
            data_offset: start + 1,
        }),
        Marker::I64 => Some(SpannedValue {
            kind: ValueKind::Int(take_u64(bytes, cursor)? as i64),
            start,
            data_offset: start + 1,
        }),
        Marker::FixExt1 => {
            let _typeid = take_u8(bytes, cursor)?;
            skip_bytes(bytes, cursor, 1)?;
            Some(opaque(start))
        }
        Marker::FixExt2 => {
            let _typeid = take_u8(bytes, cursor)?;
            skip_bytes(bytes, cursor, 2)?;
            Some(opaque(start))
        }
        Marker::FixExt4 => {
            let _typeid = take_u8(bytes, cursor)?;
            skip_bytes(bytes, cursor, 4)?;
            Some(opaque(start))
        }
        Marker::FixExt8 => {
            let _typeid = take_u8(bytes, cursor)?;
            skip_bytes(bytes, cursor, 8)?;
            Some(opaque(start))
        }
        Marker::FixExt16 => {
            let _typeid = take_u8(bytes, cursor)?;
            skip_bytes(bytes, cursor, 16)?;
            Some(opaque(start))
        }
        Marker::Str8 => {
            let len = usize::from(take_u8(bytes, cursor)?);
            parse_string(bytes, cursor, len, start)
        }
        Marker::Str16 => {
            let len = usize::from(take_u16(bytes, cursor)?);
            parse_string(bytes, cursor, len, start)
        }
        Marker::Str32 => {
            let len = usize::try_from(take_u32(bytes, cursor)?).ok()?;
            parse_string(bytes, cursor, len, start)
        }
        Marker::Array16 => {
            let len = usize::from(take_u16(bytes, cursor)?);
            parse_array(bytes, cursor, len, depth, start)
        }
        Marker::Array32 => {
            let len = usize::try_from(take_u32(bytes, cursor)?).ok()?;
            parse_array(bytes, cursor, len, depth, start)
        }
        Marker::Map16 => {
            let len = usize::from(take_u16(bytes, cursor)?);
            parse_map(bytes, cursor, len, depth, start)
        }
        Marker::Map32 => {
            let len = usize::try_from(take_u32(bytes, cursor)?).ok()?;
            parse_map(bytes, cursor, len, depth, start)
        }
        Marker::Reserved => None,
    }
}

fn opaque(start: usize) -> SpannedValue {
    SpannedValue {
        kind: ValueKind::Opaque,
        start,
        data_offset: start,
    }
}

fn parse_map(
    bytes: &[u8],
    cursor: &mut usize,
    len: usize,
    depth: usize,
    start: usize,
) -> Option<SpannedValue> {
    let remaining = bytes.len().saturating_sub(*cursor);
    // Each entry is at least a one-byte key marker and a one-byte value marker.
    let len = cadmpeg_core::decode::bounded_len(len as u64, 2, remaining)?;
    let mut values = BTreeMap::new();
    for _ in 0..len {
        let key_value = parse_value(bytes, cursor, depth + 1)?;
        let ValueKind::String(key) = key_value.kind else {
            return None;
        };
        values.insert(key, parse_value(bytes, cursor, depth + 1)?);
    }
    Some(SpannedValue {
        kind: ValueKind::Map(values),
        start,
        data_offset: start,
    })
}

fn parse_array(
    bytes: &[u8],
    cursor: &mut usize,
    len: usize,
    depth: usize,
    start: usize,
) -> Option<SpannedValue> {
    // Every element encodes as at least one marker byte, so a length exceeding
    // the unread input cannot be satisfied and is rejected before allocating.
    let remaining = bytes.len().saturating_sub(*cursor);
    let len = cadmpeg_core::decode::bounded_len(len as u64, 1, remaining)?;
    let mut values = Vec::with_capacity(len);
    for _ in 0..len {
        values.push(parse_value(bytes, cursor, depth + 1)?);
    }
    Some(SpannedValue {
        kind: ValueKind::Array(values),
        start,
        data_offset: start,
    })
}

fn parse_string(
    bytes: &[u8],
    cursor: &mut usize,
    len: usize,
    start: usize,
) -> Option<SpannedValue> {
    let data_offset = *cursor;
    let end = cursor.checked_add(len)?;
    let value = bytes.get(*cursor..end)?;
    let kind = std::str::from_utf8(value).map_or(ValueKind::Opaque, |value| {
        ValueKind::String(value.to_string())
    });
    *cursor = end;
    Some(SpannedValue {
        kind,
        start,
        data_offset,
    })
}

fn skip_bytes(bytes: &[u8], cursor: &mut usize, len: usize) -> Option<()> {
    let end = cursor.checked_add(len)?;
    let _ = bytes.get(*cursor..end)?;
    *cursor = end;
    Some(())
}

fn take_u8(bytes: &[u8], cursor: &mut usize) -> Option<u8> {
    let value = *bytes.get(*cursor)?;
    *cursor += 1;
    Some(value)
}

fn take_u16(bytes: &[u8], cursor: &mut usize) -> Option<u16> {
    let mut view = View::over_retained(bytes);
    view.seek(*cursor)?;
    let value = view.u16_be()?;
    *cursor = view.position();
    Some(value)
}

fn take_u32(bytes: &[u8], cursor: &mut usize) -> Option<u32> {
    let mut view = View::over_retained(bytes);
    view.seek(*cursor)?;
    let value = view.u32_be()?;
    *cursor = view.position();
    Some(value)
}

fn take_u64(bytes: &[u8], cursor: &mut usize) -> Option<u64> {
    let mut view = View::over_retained(bytes);
    view.seek(*cursor)?;
    let value = view.u64_be()?;
    *cursor = view.position();
    Some(value)
}

fn string_field<'a>(map: &'a BTreeMap<String, SpannedValue>, key: &str) -> Option<&'a str> {
    match &map.get(key)?.kind {
        ValueKind::String(value) => Some(value),
        _ => None,
    }
}

fn bool_from(value: &SpannedValue) -> Option<bool> {
    match value.kind {
        ValueKind::Bool(value) => Some(value),
        _ => None,
    }
}

fn int_from(value: &SpannedValue) -> Option<i64> {
    match value.kind {
        ValueKind::Int(value) => Some(value),
        _ => None,
    }
}

fn float_from(value: &SpannedValue) -> Option<f64> {
    match value.kind {
        ValueKind::Float(value) if value.is_finite() => Some(value),
        ValueKind::Int(value) => Some(value as f64),
        _ => None,
    }
}
