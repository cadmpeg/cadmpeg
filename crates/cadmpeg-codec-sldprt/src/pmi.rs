// SPDX-License-Identifier: Apache-2.0
//! Semantic dimension records stored in `PMISemanticDataDB`.

use std::collections::{BTreeMap, HashMap, HashSet};

use cadmpeg_core::convert::{f64_from_i64, truncate_f64_to_i64};
use cadmpeg_core::decode::{u64_from_index, DecodeContext, View};
use cadmpeg_core::CodecError;
use cadmpeg_ir::annotations::Annotations;
use cadmpeg_ir::report::loss::LossNote;
use cadmpeg_ir::scalar::FiniteReal;
use cadmpeg_ir::Exactness;
use rmp::Marker;

use crate::container::ContainerScan;
use crate::loss::SldprtLossCode;
use crate::records::PmiDimension;

mod patch_slots;
use patch_slots::{BooleanPatchSlot, FloatPatchSlot, IntegerPatchSlot};

fn exact_count(value: f64) -> Option<i64> {
    const MAX_EXCLUSIVE_COUNT: f64 = 9_223_372_036_854_775_808.0;
    if !(0.0..MAX_EXCLUSIVE_COUNT).contains(&value) {
        return None;
    }
    let count = truncate_f64_to_i64(value)?;
    (value.fract() == 0.0).then_some(count)
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
        "" if empty_subtype_is_count && exact_count(record.value.get()).is_some() => {
            PmiDimensionSubtype::Count
        }
        other => PmiDimensionSubtype::Native(other.to_owned()),
    }
}

fn neutral_parameter_is_count(
    feature: &cadmpeg_ir::features::Feature,
    name: &str,
    value: Option<&cadmpeg_ir::features::ParameterValue>,
) -> bool {
    use cadmpeg_ir::features::{
        patterns::PatternTransform, FeatureDefinition, FeatureOperation, ParameterValue,
    };

    matches!(value, Some(ParameterValue::Integer(_)))
        || (matches!(name, "D1" | "D2")
            && matches!(
                feature.evaluation.definition(),
                FeatureDefinition::Operation(FeatureOperation::Pattern {
                    pattern: admitted_pattern,
                    ..
                }) if matches!(admitted_pattern.definition(), PatternTransform::Linear { .. } | PatternTransform::LinearOffsets { .. })
            ))
}

#[cfg(test)]
mod tests;

/// Return whether two retained records encode the same semantic dimension.
///
/// Record identity and byte locations are intentionally excluded. `SolidWorks`
/// can retain multiple GUID records for one owner-qualified dimension. Every
/// editable semantic field must agree before those records are aliases.
fn equivalent_dimensions(left: &PmiDimension, right: &PmiDimension) -> bool {
    left.cad_text == right.cad_text
        && left.item_count == right.item_count
        && left.subtype == right.subtype
        && left.value.get().to_bits() == right.value.get().to_bits()
        && left.precision == right.precision
        && left.display_text() == right.display_text()
        && left.basic == right.basic
        && left.inspection == right.inspection
        && left.reference_only == right.reference_only
}

/// Return one deterministic representative for each owner-qualified dimension
/// whose retained records all agree semantically.
fn agreed_dimension_records(records: &[PmiDimension]) -> Vec<&PmiDimension> {
    let mut groups = BTreeMap::<&str, Vec<&PmiDimension>>::new();
    for record in records {
        groups
            .entry(record.cad_text.as_str())
            .or_default()
            .push(record);
    }
    let mut representatives = Vec::new();
    for mut group in groups.into_values() {
        group.sort_unstable_by(|left, right| left.id.cmp(&right.id));
        let Some(&canonical) = group.first() else {
            continue;
        };
        if canonical.item_count.get() == 1
            && group.iter().all(|record| {
                record.item_count.get() == 1 && equivalent_dimensions(canonical, record)
            })
        {
            representatives.push(canonical);
        }
    }
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
        .filter(|record| !bound_ids.contains(record.id.as_str()))
        .filter(|record| {
            !bound
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
        let millimetres = record.value.get() * 1000.0;
        let feature = &histories[*history_index].features[*feature_index];
        let empty_subtype_is_count = feature.parameters.get(name).is_some_and(|expression| {
            matches!(
                crate::history::parameters::parse_native_parameter_literal(
                    feature, name, expression
                ),
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
            cadmpeg_ir::features::PmiDimensionSubtype::Angle => record.value.get().to_string(),
            cadmpeg_ir::features::PmiDimensionSubtype::Diameter => {
                format!("<MOD-DIAM>{millimetres}mm")
            }
            cadmpeg_ir::features::PmiDimensionSubtype::Radial => {
                format!("R{millimetres}mm")
            }
            cadmpeg_ir::features::PmiDimensionSubtype::Count => {
                match exact_count(record.value.get()) {
                    Some(count) => count.to_string(),
                    None => continue,
                }
            }
            cadmpeg_ir::features::PmiDimensionSubtype::Native(_) => continue,
        };
        let parameters = &mut histories[*history_index].features[*feature_index].parameters;
        if parameters.contains_key(name) {
            continue;
        }
        let Some(name) = cadmpeg_core::text::NonBlankString::new(name.to_owned()) else {
            continue;
        };
        parameters.insert(name, expression);
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
    let native =
        crate::native::SldprtNative::load(namespace).map_err(cadmpeg_core::CodecError::from)?;
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
        if record.item_count.get() != 1 {
            continue;
        }
        let mut parameters = ir.model.parameters.iter().filter_map(|parameter| {
            let semantic = parameter.pmi.as_ref()?;
            (semantic.native_ref == record.id
                || records_by_id
                    .get(semantic.native_ref.as_str())
                    .is_some_and(|bound| equivalent_dimensions(record, bound)))
            .then_some((parameter, semantic))
        });
        let Some((parameter, semantic)) = parameters.next() else {
            continue;
        };
        if parameters.next().is_some() {
            return Err(cadmpeg_core::CodecError::malformed(format_args!(
                "multiple parameters reference PMI record {}",
                record.id
            )));
        }
        let empty_subtype_is_count = semantic.subtype == PmiDimensionSubtype::Count;
        let subtype = dimension_subtype(record, empty_subtype_is_count);
        if semantic.subtype != subtype {
            return Err(cadmpeg_core::CodecError::NotImplemented(format!(
                "SLDPRT PMI record {} changes dimension subtype",
                record.id
            )));
        }
        let native_value = match (&subtype, &parameter.value) {
            (PmiDimensionSubtype::Angle, Some(ParameterValue::Angle(angle))) => angle.get(),
            (
                PmiDimensionSubtype::Linear
                | PmiDimensionSubtype::Diameter
                | PmiDimensionSubtype::Radial
                | PmiDimensionSubtype::Ordinate,
                Some(ParameterValue::Length(length)),
            ) => length.get() / 1000.0,
            (PmiDimensionSubtype::Count, Some(ParameterValue::Integer(count))) => {
                f64_from_i64(*count).ok_or_else(|| {
                    cadmpeg_core::CodecError::NotImplemented(format!(
                        "SLDPRT PMI record {} has a count beyond exact float range",
                        record.id
                    ))
                })?
            }
            _ => {
                return Err(cadmpeg_core::CodecError::NotImplemented(format!(
                    "SLDPRT PMI record {} has a value incompatible with its dimension subtype",
                    record.id
                )));
            }
        };
        FloatPatchSlot::read(payload, record.offset)?.write(native_value);
        IntegerPatchSlot::read(payload, record.offset, "valPrecision")?
            .write(semantic.precision)
            .map_err(cadmpeg_core::CodecError::malformed)?;
        for (field, value) in [
            ("isBasic", semantic.basic),
            ("isInspection", semantic.inspection),
            ("isReferenceOnly", semantic.reference_only),
        ] {
            BooleanPatchSlot::read(payload, record.offset, field)?.write(value);
        }
        if semantic.display_text.as_deref() != record.display_text() {
            let (Some((previous, offset)), Some(text)) = (
                record.display_text.as_ref(),
                semantic.display_text.as_deref(),
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
            patch_bytes(payload, *offset, text.as_bytes(), &record.id)?;
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
) -> Result<(), cadmpeg_core::CodecError> {
    use cadmpeg_ir::{
        features::{
            DesignParameter, DimensionDisplay, ParameterId, ParameterPmi, ParameterValue,
            PmiDimensionSubtype,
        },
        scalar::Length,
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
        let millimetres = record.value.get() * 1000.0;
        let (expression, display, value) = match subtype {
            PmiDimensionSubtype::Linear => (
                format!("{millimetres}mm"),
                None,
                Some(ParameterValue::Length(
                    Length::new(millimetres).ok_or_else(|| {
                        cadmpeg_core::CodecError::Malformed(
                            "SolidWorks projected length must be finite".into(),
                        )
                    })?,
                )),
            ),
            PmiDimensionSubtype::Angle => (
                record.value.get().to_string(),
                None,
                Some(ParameterValue::Angle(
                    cadmpeg_ir::scalar::Angle::from_assigned_real(record.value),
                )),
            ),
            PmiDimensionSubtype::Diameter => (
                format!("<MOD-DIAM>{millimetres}mm"),
                Some(DimensionDisplay::Diameter),
                Some(ParameterValue::Length(
                    Length::new(millimetres).ok_or_else(|| {
                        cadmpeg_core::CodecError::Malformed(
                            "SolidWorks projected length must be finite".into(),
                        )
                    })?,
                )),
            ),
            PmiDimensionSubtype::Radial => (
                format!("R{millimetres}mm"),
                Some(DimensionDisplay::Radius),
                Some(ParameterValue::Length(
                    Length::new(millimetres).ok_or_else(|| {
                        cadmpeg_core::CodecError::Malformed(
                            "SolidWorks projected length must be finite".into(),
                        )
                    })?,
                )),
            ),
            PmiDimensionSubtype::Ordinate => (
                format!("{millimetres}mm"),
                None,
                Some(ParameterValue::Length(
                    Length::new(millimetres).ok_or_else(|| {
                        cadmpeg_core::CodecError::Malformed(
                            "SolidWorks projected length must be finite".into(),
                        )
                    })?,
                )),
            ),
            PmiDimensionSubtype::Count => {
                let Some(count) = exact_count(record.value.get()) else {
                    continue;
                };
                (
                    count.to_string(),
                    None,
                    Some(ParameterValue::Integer(count)),
                )
            }
            PmiDimensionSubtype::Native(_) => (record.value.get().to_string(), None, None),
        };
        let semantic = ParameterPmi {
            subtype,
            precision: record.precision,
            display_text: record.display_text().map(str::to_owned),
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
        let ordinal = match parameters
            .iter()
            .filter(|parameter| parameter.owner.as_ref() == Some(&owner.id))
            .map(|parameter| parameter.ordinal)
            .max()
        {
            Some(ordinal) => ordinal.checked_add(1).ok_or_else(|| {
                cadmpeg_core::CodecError::Malformed("SLDPRT PMI parameter ordinal overflows".into())
            })?,
            None => 0,
        };
        parameters.push(DesignParameter {
            id: ParameterId::compose(
                &cadmpeg_ir::identity_namespace!("sldprt", "model", "parameter"),
                cadmpeg_ir::identity_key!("pmi:").then(
                    cadmpeg_ir::ids::IdentityKey::try_new(record.guid.clone()).map_err(
                        |error| {
                            cadmpeg_core::CodecError::malformed(format_args!(
                                "SLDPRT PMI record guid is not identity key text: {error}"
                            ))
                        },
                    )?,
                ),
            ),
            owner: Some(
                cadmpeg_ir::features::FeatureId::mint(owner.id.as_str().to_owned())
                    .map_err(cadmpeg_core::CodecError::malformed)?,
            ),
            ordinal,
            name: name.to_owned(),
            expression,
            display,
            value,
            dependencies: cadmpeg_ir::features::DistinctMembers::default(),
            properties: BTreeMap::new(),
            pmi: Some(semantic),
            native_ref: None,
        });
    }
    Ok(())
}

/// One `MessagePack` value with absolute source spans for in-place patching.
#[derive(Debug)]
struct SpannedValue<'a> {
    kind: ValueKind<'a>,
    /// Absolute offset of this value's marker byte.
    start: usize,
    /// Absolute offset of the writable scalar payload (kind-dependent).
    data_offset: usize,
}

#[derive(Debug)]
enum ValueKind<'a> {
    Bool(bool),
    Int(i64),
    Float(f64),
    String(&'a str),
    Array(Vec<SpannedValue<'a>>),
    Map(BTreeMap<&'a str, SpannedValue<'a>>),
    Opaque,
}

/// Extract semantic dimensions from `PMISemanticDataDB` sections.
///
/// Parse failures on GUID-prefixed `MessagePack` maps emit
/// [`SldprtLossCode::PmiSemanticRecordMalformed`] instead of shrinking the
/// document silently.
pub(crate) fn dimensions(
    ctx: &DecodeContext<'_>,
    scan: &ContainerScan,
    annotations: &mut Annotations,
    losses: &mut Vec<LossNote>,
) -> Result<Vec<PmiDimension>, CodecError> {
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
            ctx,
            source.payload(),
            source.source_stream(),
            source.native_id().as_str(),
            annotations,
            losses,
            DimensionOutput {
                records: &mut records,
                seen: &mut seen,
            },
        )?;
    }
    records.sort_by(|left, right| left.id.cmp(&right.id));
    Ok(records)
}

/// Parse PMI records from a raw `PMISemanticDataDB` payload.
///
/// Used by focused tests and the `sldprt_pmi` fuzz target. Parent/section are
/// placeholders; production decode supplies real block identities.
pub(crate) fn parse_payload(
    ctx: &DecodeContext<'_>,
    payload: &[u8],
    losses: &mut Vec<LossNote>,
) -> Result<Vec<PmiDimension>, CodecError> {
    let mut annotations = Annotations::default();
    let mut records = Vec::new();
    let mut seen = HashSet::<String>::new();
    let stream = cadmpeg_ir::stream_name!("Contents/PMISemanticDataDB");
    collect_dimensions(
        ctx,
        payload,
        &stream,
        "sldprt:block#pmi-payload",
        &mut annotations,
        losses,
        DimensionOutput {
            records: &mut records,
            seen: &mut seen,
        },
    )?;
    records.sort_by(|left, right| left.id.cmp(&right.id));
    Ok(records)
}

struct DimensionOutput<'a> {
    records: &'a mut Vec<PmiDimension>,
    seen: &'a mut HashSet<String>,
}

fn collect_dimensions(
    ctx: &DecodeContext<'_>,
    payload: &[u8],
    stream: &cadmpeg_ir::StreamName,
    parent: &str,
    annotations: &mut Annotations,
    losses: &mut Vec<LossNote>,
    output: DimensionOutput<'_>,
) -> Result<(), CodecError> {
    let DimensionOutput { records, seen } = output;
    for (guid, offset) in candidate_maps(payload) {
        let normalized = guid.to_ascii_lowercase();
        if seen.contains(&normalized) {
            continue;
        }
        match extract_dimension(ctx, payload, offset, &normalized, parent) {
            Ok(Some(record)) => {
                crate::annotations::note(
                    annotations,
                    record.id.clone(),
                    stream,
                    u64_from_index(offset),
                    "messagepack_dim_sem_data",
                    Exactness::ByteExact,
                )?;
                records.push(record);
            }
            Ok(None) => {}
            Err(PmiParseError::Malformed(message)) => {
                losses.push(SldprtLossCode::PmiSemanticRecordMalformed.note(format!(
                    "PMISemanticDataDB map at offset {offset} (guid {normalized}) {message}"
                )));
            }
            Err(PmiParseError::Resource(error)) => return Err(error),
        }
        seen.insert(normalized);
    }
    Ok(())
}

enum PmiParseError {
    Malformed(String),
    Resource(CodecError),
}

impl From<&str> for PmiParseError {
    fn from(message: &str) -> Self {
        Self::Malformed(message.to_owned())
    }
}

impl From<String> for PmiParseError {
    fn from(message: String) -> Self {
        Self::Malformed(message)
    }
}

impl From<CodecError> for PmiParseError {
    fn from(error: CodecError) -> Self {
        Self::Resource(error)
    }
}

/// `Ok(None)` means the map is not a PMI dimension.
/// Malformed candidates carry a loss; resource refusals end the decode.
fn extract_dimension(
    ctx: &DecodeContext<'_>,
    payload: &[u8],
    offset: usize,
    guid: &str,
    parent: &str,
) -> Result<Option<PmiDimension>, PmiParseError> {
    let mut cursor = offset;
    let Some(outer_value) = parse_value(ctx, payload, &mut cursor, 0)? else {
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
    let Some(item_count) = std::num::NonZeroU32::new(item_count) else {
        return Err("dimItems is empty".into());
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
    let id = format!("sldprt:pmi:dimension#{guid}");
    let display_text = match outer.get("dimText") {
        Some(SpannedValue {
            kind: ValueKind::String(text),
            data_offset,
            ..
        }) => Some(((*text).to_owned(), u64_from_index(*data_offset))),
        _ => None,
    };
    Ok(Some(PmiDimension {
        id,
        parent: parent.to_owned(),
        offset: u64_from_index(offset),
        guid: guid.to_owned(),
        cad_text: cad_text.to_owned(),
        item_count,
        subtype: string_field(item, "dimSubType")
            .unwrap_or_default()
            .to_owned(),
        value,
        value_offset: u64_from_index(value_field.data_offset),
        precision: int_from(precision_field)
            .ok_or_else(|| "valPrecision is not an integer".to_string())?,
        precision_offset: u64_from_index(precision_field.data_offset),
        display_text,
        basic: bool_from(basic_field).ok_or_else(|| "basic is not a boolean".to_string())?,
        basic_offset: u64_from_index(basic_field.data_offset),
        inspection: bool_from(inspection_field)
            .ok_or_else(|| "inspection is not a boolean".to_string())?,
        inspection_offset: u64_from_index(inspection_field.data_offset),
        reference_only: bool_from(reference_field)
            .ok_or_else(|| "reference_only is not a boolean".to_string())?,
        reference_only_offset: u64_from_index(reference_field.data_offset),
    }))
}

/// True when a short window after `offset` still encodes the PMI map keys.
///
/// Used only to decide whether a failed parse is an attributed PMI loss or an
/// unrelated GUID/map collision. It is not the field locator.
fn looks_like_pmi_map(payload: &[u8], offset: usize) -> bool {
    let Some(end) = offset.checked_add(1024) else {
        return false;
    };
    let end = end.min(payload.len());
    let window = payload.get(offset..end).unwrap_or(&[]);
    contains_fixstr_key(window, "cadText") && contains_fixstr_key(window, "dimItems")
}

fn contains_fixstr_key(window: &[u8], key: &str) -> bool {
    if key.len() >= 32 {
        return false;
    }
    let Ok(key_len) = u8::try_from(key.len()) else {
        return false;
    };
    let mut encoded = Vec::with_capacity(key.len() + 1);
    encoded.push(0xa0 | key_len);
    encoded.extend_from_slice(key.as_bytes());
    window
        .windows(encoded.len())
        .any(|candidate| candidate == encoded)
}

/// Locate GUID-prefixed `MessagePack` maps. Key order and map length do not matter.
fn candidate_maps(payload: &[u8]) -> impl Iterator<Item = (&str, usize)> {
    payload
        .iter()
        .copied()
        .enumerate()
        .filter_map(|(offset, marker)| {
            if matches!(
                Marker::from_u8(marker),
                Marker::FixMap(_) | Marker::Map16 | Marker::Map32
            ) {
                guid_before(payload, offset).map(|guid| (guid, offset))
            } else {
                None
            }
        })
}

fn guid_before(payload: &[u8], offset: usize) -> Option<&str> {
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
    .then_some(guid)
}

fn parse_value<'a>(
    ctx: &DecodeContext<'_>,
    bytes: &'a [u8],
    cursor: &mut usize,
    depth: usize,
) -> Result<Option<SpannedValue<'a>>, CodecError> {
    let _depth = ctx.enter_nested("parse SLDPRT PMI MessagePack")?;
    // The nesting cap rejects this one MessagePack value.
    if depth > 16 {
        return Ok(None);
    }
    let start = *cursor;
    let Some(marker) = take_u8(bytes, cursor).map(Marker::from_u8) else {
        return Ok(None);
    };
    let value = match marker {
        Marker::FixMap(len) => parse_map(ctx, bytes, cursor, usize::from(len), depth, start)?,
        Marker::FixArray(len) => parse_array(ctx, bytes, cursor, usize::from(len), depth, start)?,
        Marker::FixStr(len) => parse_string(bytes, cursor, usize::from(len), start),
        Marker::Str8 => match take_u8(bytes, cursor) {
            Some(len) => parse_string(bytes, cursor, usize::from(len), start),
            None => None,
        },
        Marker::Str16 => match take_u16(bytes, cursor) {
            Some(len) => parse_string(bytes, cursor, usize::from(len), start),
            None => None,
        },
        Marker::Str32 => match take_u32(bytes, cursor).and_then(|len| usize::try_from(len).ok()) {
            Some(len) => parse_string(bytes, cursor, len, start),
            None => None,
        },
        Marker::Array16 => match take_u16(bytes, cursor) {
            Some(len) => parse_array(ctx, bytes, cursor, usize::from(len), depth, start)?,
            None => None,
        },
        Marker::Array32 => {
            match take_u32(bytes, cursor).and_then(|len| usize::try_from(len).ok()) {
                Some(len) => parse_array(ctx, bytes, cursor, len, depth, start)?,
                None => None,
            }
        }
        Marker::Map16 => match take_u16(bytes, cursor) {
            Some(len) => parse_map(ctx, bytes, cursor, usize::from(len), depth, start)?,
            None => None,
        },
        Marker::Map32 => match take_u32(bytes, cursor).and_then(|len| usize::try_from(len).ok()) {
            Some(len) => parse_map(ctx, bytes, cursor, len, depth, start)?,
            None => None,
        },
        _ => parse_scalar(bytes, cursor, marker, start),
    };
    Ok(value)
}

fn parse_scalar<'a>(
    bytes: &'a [u8],
    cursor: &mut usize,
    marker: Marker,
    start: usize,
) -> Option<SpannedValue<'a>> {
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
        Marker::FixMap(_) | Marker::FixArray(_) | Marker::FixStr(_) => None,
        Marker::Null => Some(opaque(start)),
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
            kind: ValueKind::Int(i64::from(take_u8(bytes, cursor)?.cast_signed())),
            start,
            data_offset: start + 1,
        }),
        Marker::I16 => Some(SpannedValue {
            kind: ValueKind::Int(i64::from(take_u16(bytes, cursor)?.cast_signed())),
            start,
            data_offset: start + 1,
        }),
        Marker::I32 => Some(SpannedValue {
            kind: ValueKind::Int(i64::from(take_u32(bytes, cursor)?.cast_signed())),
            start,
            data_offset: start + 1,
        }),
        Marker::I64 => Some(SpannedValue {
            kind: ValueKind::Int(take_u64(bytes, cursor)?.cast_signed()),
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
        Marker::Str8
        | Marker::Str16
        | Marker::Str32
        | Marker::Array16
        | Marker::Array32
        | Marker::Map16
        | Marker::Map32 => None,
        Marker::Reserved => None,
    }
}

fn opaque(start: usize) -> SpannedValue<'static> {
    SpannedValue {
        kind: ValueKind::Opaque,
        start,
        data_offset: start,
    }
}

fn parse_map<'a>(
    ctx: &DecodeContext<'_>,
    bytes: &'a [u8],
    cursor: &mut usize,
    len: usize,
    depth: usize,
    start: usize,
) -> Result<Option<SpannedValue<'a>>, CodecError> {
    let Some(remaining) = bytes.len().checked_sub(*cursor) else {
        return Ok(None);
    };
    // Each entry is at least a one-byte key marker and a one-byte value marker.
    let Some(len) = cadmpeg_core::decode::bounded_len(u64_from_index(len), 2, remaining) else {
        return Ok(None);
    };
    let mut values = BTreeMap::new();
    let next_depth = depth + 1;
    for _ in 0..len {
        let Some(key_value) = parse_value(ctx, bytes, cursor, next_depth)? else {
            return Ok(None);
        };
        let ValueKind::String(key) = key_value.kind else {
            return Ok(None);
        };
        let Some(value) = parse_value(ctx, bytes, cursor, next_depth)? else {
            return Ok(None);
        };
        values.insert(key, value);
    }
    Ok(Some(SpannedValue {
        kind: ValueKind::Map(values),
        start,
        data_offset: start,
    }))
}

fn parse_array<'a>(
    ctx: &DecodeContext<'_>,
    bytes: &'a [u8],
    cursor: &mut usize,
    len: usize,
    depth: usize,
    start: usize,
) -> Result<Option<SpannedValue<'a>>, CodecError> {
    // Every element encodes as at least one marker byte, so a length exceeding
    // the unread input cannot be satisfied and is rejected before allocating.
    let Some(remaining) = bytes.len().checked_sub(*cursor) else {
        return Ok(None);
    };
    let Some(len) = cadmpeg_core::decode::bounded_len(u64_from_index(len), 1, remaining) else {
        return Ok(None);
    };
    let mut values = Vec::new();
    let next_depth = depth + 1;
    for _ in 0..len {
        let Some(value) = parse_value(ctx, bytes, cursor, next_depth)? else {
            return Ok(None);
        };
        values.push(value);
    }
    Ok(Some(SpannedValue {
        kind: ValueKind::Array(values),
        start,
        data_offset: start,
    }))
}

fn parse_string<'a>(
    bytes: &'a [u8],
    cursor: &mut usize,
    len: usize,
    start: usize,
) -> Option<SpannedValue<'a>> {
    let data_offset = *cursor;
    let end = cursor.checked_add(len)?;
    let value = bytes.get(*cursor..end)?;
    let kind = std::str::from_utf8(value).map_or(ValueKind::Opaque, ValueKind::String);
    *cursor = end;
    Some(SpannedValue {
        kind,
        start,
        data_offset,
    })
}

fn skip_bytes(bytes: &[u8], cursor: &mut usize, len: usize) -> Option<()> {
    let end = cursor.checked_add(len)?;
    // discarded-value: the skipped range is proven to lie in the record; ? states the refusal and the slice has no reader
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

fn string_field<'a>(map: &'a BTreeMap<&str, SpannedValue<'_>>, key: &str) -> Option<&'a str> {
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

fn float_from(value: &SpannedValue) -> Option<FiniteReal> {
    match value.kind {
        ValueKind::Float(value) => FiniteReal::new(value),
        ValueKind::Int(value) => FiniteReal::new(f64_from_i64(value)?),
        _ => None,
    }
}
