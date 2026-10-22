// SPDX-License-Identifier: Apache-2.0
//! Semantic dimension records stored in `PMISemanticDataDB`.

use std::collections::{BTreeMap, HashSet};

use cadmpeg_core::convert::{f64_from_i64, truncate_f64_to_i64};
use cadmpeg_core::decode::{u64_from_index, DecodeContext, View, WorkBudget};
use cadmpeg_core::CodecError;
use cadmpeg_ir::annotations::Annotations;
use cadmpeg_ir::report::loss::LossNote;
use cadmpeg_ir::scalar::FiniteReal;
use cadmpeg_ir::Exactness;
use rmp::Marker;

use crate::container::ContainerScan;
use crate::loss::SldprtLossCode;
use crate::records::PmiDimension;

mod database;
#[cfg(test)]
mod patch_slots;
pub(crate) mod write;
#[cfg(test)]
use patch_slots::{BooleanPatchSlot, FloatPatchSlot, IntegerPatchSlot};

fn exact_count(value: f64) -> Option<i64> {
    const MAX_EXCLUSIVE_COUNT: f64 = 9_223_372_036_854_775_808.0;
    if !(0.0..MAX_EXCLUSIVE_COUNT).contains(&value) {
        return None;
    }
    let count = truncate_f64_to_i64(value)?;
    (value.fract() == 0.0).then_some(count)
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum DimensionSubtype {
    Linear,
    Angle,
    Diameter,
    Radial,
    Ordinate,
    Count,
    Native(String),
}

fn dimension_subtype(record: &PmiDimension, empty_subtype_is_count: bool) -> DimensionSubtype {
    match record.subtype.as_str() {
        "Linear" => DimensionSubtype::Linear,
        "Angle" => DimensionSubtype::Angle,
        "Diameter" => DimensionSubtype::Diameter,
        "Radial" => DimensionSubtype::Radial,
        "Ordinate" => DimensionSubtype::Ordinate,
        "" if empty_subtype_is_count && exact_count(record.value.get()).is_some() => {
            DimensionSubtype::Count
        }
        other => DimensionSubtype::Native(other.to_owned()),
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
            DimensionSubtype::Linear | DimensionSubtype::Ordinate => {
                format!("{millimetres}mm")
            }
            DimensionSubtype::Angle => record.value.get().to_string(),
            DimensionSubtype::Diameter => {
                format!("<MOD-DIAM>{millimetres}mm")
            }
            DimensionSubtype::Radial => {
                format!("R{millimetres}mm")
            }
            DimensionSubtype::Count => match exact_count(record.value.get()) {
                Some(count) => count.to_string(),
                None => continue,
            },
            DimensionSubtype::Native(_) => continue,
        };
        let feature = &mut histories[*history_index].features[*feature_index];
        if feature.parameters.contains_key(name) {
            continue;
        }
        let Some(name) = cadmpeg_core::text::NonBlankString::new(name.to_owned()) else {
            continue;
        };
        // This temporary parameter comes solely from the agreed PMI record.
        // Its reference-only state supplies the driving role; an actual
        // Keywords dimension was handled above and keeps its own Driven fact.
        feature
            .dimension_properties
            .entry(name.as_str().to_owned())
            .or_default()
            .insert(
                cadmpeg_core::nonblank_literal!("Driven"),
                record.reference_only.to_string(),
            );
        feature.parameters.insert(name, expression);
    }
}

/// Stable neutral identity of one semantic source occurrence.
pub(crate) fn annotation_id(record: &PmiDimension) -> Result<cadmpeg_ir::ids::PmiId, CodecError> {
    cadmpeg_ir::ids::PmiId::mint(format!("sldprt:model:pmi#{}", record.guid))
        .map_err(CodecError::malformed)
}

#[cfg(test)]
pub(crate) fn patch_payload(
    ir: &cadmpeg_ir::CadIr,
    block_id: &str,
    payload: &mut [u8],
) -> Result<(), CodecError> {
    use cadmpeg_ir::{
        attributes::{AttributeTarget, AttributeValue},
        pmi::{PmiDefinition, PmiQuantity},
    };
    let Some(namespace) = ir.native.namespace("sldprt") else {
        return Ok(());
    };
    let native = crate::native::SldprtNative::load(namespace)?;
    for record in native
        .pmi_dimensions
        .iter()
        .filter(|record| record.parent == block_id)
    {
        let id = annotation_id(record)?;
        let annotation = ir.model.pmi.iter().find(|annotation| annotation.id == id);
        let (value, precision, basic, inspection, reference_only, text) = if ir.scope.model {
            let Some(annotation) = annotation else {
                return Err(CodecError::UnsupportedContent(format!(
                    "SLDPRT PMI deletion of {id} requires synthesis"
                )));
            };
            let PmiDefinition::Dimension(dimension) = &annotation.definition else {
                return Err(CodecError::UnsupportedContent(format!(
                    "SLDPRT PMI occurrence {id} changed semantic family"
                )));
            };
            let value = dimension
                .resolved_nominal(ir)
                .map(|value| match value.quantity {
                    PmiQuantity::Length => value.value.get() * 0.001,
                    PmiQuantity::Angle | PmiQuantity::Ratio => value.value.get(),
                })
                .ok_or_else(|| {
                    CodecError::UnsupportedContent(format!(
                        "SLDPRT PMI occurrence {id} has no evaluated nominal"
                    ))
                })?;
            let attribute = |name: &str| {
                ir.model
                    .attributes
                    .iter()
                    .find(|attribute| {
                        attribute.target == AttributeTarget::Pmi(id.clone())
                            && attribute.name == name
                    })
                    .and_then(|attribute| attribute.values.first())
            };
            let precision = match attribute("precision") {
                Some(AttributeValue::Integer(value)) => *value,
                _ => 6,
            };
            let flag = |name| matches!(attribute(name), Some(AttributeValue::Boolean(true)));
            let text = ir
                .model
                .pmi
                .iter()
                .find_map(|annotation| match &annotation.definition {
                    PmiDefinition::Presentation {
                        text, semantics, ..
                    } if semantics.contains(&id) => text.clone(),
                    _ => None,
                });
            (
                value,
                precision,
                flag("basic"),
                flag("inspection"),
                flag("reference_only"),
                text,
            )
        } else {
            (
                record.value.get(),
                record.precision,
                record.basic,
                record.inspection,
                record.reference_only,
                record.display_text().map(str::to_owned),
            )
        };
        FloatPatchSlot::read(
            payload,
            record
                .offset
                .ok_or_else(|| CodecError::malformed("PMI patch lacks source map position"))?,
        )?
        .write(value);
        IntegerPatchSlot::read(
            payload,
            record
                .offset
                .ok_or_else(|| CodecError::malformed("PMI patch lacks source map position"))?,
            "valPrecision",
        )?
        .write(precision)
        .map_err(CodecError::malformed)?;
        for (field, value) in [
            ("isBasic", basic),
            ("isInspection", inspection),
            ("isReferenceOnly", reference_only),
        ] {
            BooleanPatchSlot::read(
                payload,
                record
                    .offset
                    .ok_or_else(|| CodecError::malformed("PMI patch lacks source map position"))?,
                field,
            )?
            .write(value);
        }
        match (record.display_text.as_ref(), text.as_deref()) {
            (Some((previous, offset)), Some(text)) if previous.len() == text.len() => patch_bytes(
                payload,
                offset.ok_or_else(|| {
                    CodecError::malformed("PMI patch lacks source display-text position")
                })?,
                text.as_bytes(),
                &record.id,
            )?,
            (None, None) => {}
            _ => {
                return Err(CodecError::UnsupportedContent(format!(
                    "SLDPRT PMI occurrence {id} requires a different text layout"
                )))
            }
        }
    }
    Ok(())
}

#[cfg(test)]
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

#[derive(Debug)]
pub(crate) struct ProjectedDimensions {
    pub(crate) annotations: Vec<cadmpeg_ir::pmi::PmiAnnotation>,
    pub(crate) attributes: Vec<cadmpeg_ir::attributes::SourceAttribute>,
    pub(crate) losses: Vec<LossNote>,
}

pub(crate) fn apply_to_parameters(
    parameters: &mut Vec<cadmpeg_ir::features::DesignParameter>,
    features: &[cadmpeg_ir::features::Feature],
    records: &[PmiDimension],
) -> Result<ProjectedDimensions, CodecError> {
    use cadmpeg_ir::{
        attributes::{AttributeTarget, AttributeValue, SourceAttribute},
        features::{
            DesignParameter, Dimension, DimensionDisplay, Expression, ParameterId, ParameterValue,
        },
        ids::{AttributeId, PmiId},
        pmi::{
            DimensionKind, PmiAnnotation, PmiDefinition, PmiDimension as NeutralDimension,
            PmiQuantity, PmiValue,
        },
    };
    let mut result = ProjectedDimensions {
        annotations: Vec::new(),
        attributes: Vec::new(),
        losses: Vec::new(),
    };
    let mut owners = BTreeMap::<&str, Vec<&cadmpeg_ir::features::Feature>>::new();
    for feature in features {
        if let Some(name) = feature.name.as_deref() {
            owners.entry(name).or_default().push(feature);
        }
    }
    let agreed = agreed_dimension_records(records)
        .into_iter()
        .map(|record| record.cad_text.as_str())
        .collect::<HashSet<_>>();
    let mut ordered = records.iter().collect::<Vec<_>>();
    ordered.sort_unstable_by(|left, right| left.id.cmp(&right.id));
    for record in ordered {
        let owner = record.cad_text.split_once('@').and_then(|(name, owner)| {
            let [owner] = owners.get(owner)?.as_slice() else {
                return None;
            };
            Some((name, *owner))
        });
        let existing = owner.and_then(|(name, owner)| {
            parameters.iter().position(|parameter| {
                parameter.owner.as_ref() == Some(&owner.id) && parameter.name == name
            })
        });
        let count = owner.is_some_and(|(name, owner)| {
            neutral_parameter_is_count(
                owner,
                name,
                existing.and_then(|index| parameters[index].value.as_ref()),
            )
        });
        let subtype = dimension_subtype(record, count);
        let (kind, source_value, display) = match &subtype {
            DimensionSubtype::Linear => (
                DimensionKind::Size,
                PmiValue::new(record.value.get() * 1000.0, PmiQuantity::Length),
                None,
            ),
            DimensionSubtype::Ordinate => (
                DimensionKind::Location,
                PmiValue::new(record.value.get() * 1000.0, PmiQuantity::Length),
                None,
            ),
            DimensionSubtype::Angle => (
                DimensionKind::Angular,
                PmiValue::new(record.value.get(), PmiQuantity::Angle),
                None,
            ),
            DimensionSubtype::Diameter => (
                DimensionKind::Diameter,
                PmiValue::new(record.value.get() * 1000.0, PmiQuantity::Length),
                Some(DimensionDisplay::Diameter),
            ),
            DimensionSubtype::Radial => (
                DimensionKind::Radius,
                PmiValue::new(record.value.get() * 1000.0, PmiQuantity::Length),
                Some(DimensionDisplay::Radius),
            ),
            DimensionSubtype::Count => (
                DimensionKind::Other("count".into()),
                PmiValue::new(record.value.get(), PmiQuantity::Ratio),
                None,
            ),
            DimensionSubtype::Native(kind) => {
                result.losses.push(SldprtLossCode::PmiDimensionUnbound.note(format!("PMI record {} has source subtype {kind:?} without a known neutral quantity; its typed native value is retained", record.id)));
                (DimensionKind::Other(kind.clone()), None, None)
            }
        };
        let id = annotation_id(record)?;
        let parameter = match (existing, owner, source_value) {
            (Some(index), _, _) => Some(index),
            (None, Some((name, owner)), Some(value))
                if record.item_count.get() == 1 && agreed.contains(record.cad_text.as_str()) =>
            {
                let ordinal = parameters
                    .iter()
                    .filter(|parameter| parameter.owner.as_ref() == Some(&owner.id))
                    .map(|parameter| parameter.ordinal)
                    .max()
                    .map_or(Some(0), |ordinal| ordinal.checked_add(1))
                    .ok_or_else(|| {
                        CodecError::malformed("SLDPRT PMI parameter ordinal overflows")
                    })?;
                let cache = match subtype {
                    DimensionSubtype::Count => {
                        exact_count(value.value.get()).map(ParameterValue::Integer)
                    }
                    _ => cadmpeg_ir::features::DimensionValue::to_parameter_value(&value),
                };
                let expression = cache.as_ref().map_or_else(
                    || Expression::text(record.value.get().to_string()),
                    |value| {
                        Expression::text(crate::history::literals::format_parameter_value(value))
                    },
                );
                let index = parameters.len();
                parameters.push(DesignParameter {
                    id: ParameterId::compose(
                        &cadmpeg_ir::identity_namespace!("sldprt", "model", "parameter"),
                        cadmpeg_ir::ids::IdentityKey::try_new(format!("pmi:{}", record.guid))
                            .map_err(CodecError::malformed)?,
                    ),
                    owner: Some(owner.id.clone()),
                    ordinal,
                    name: name.to_owned(),
                    expression,
                    driving: !record.reference_only,
                    display,
                    value: cache,
                    properties: BTreeMap::new(),
                    pmi: None,
                    native_ref: None,
                });
                Some(index)
            }
            _ => None,
        };
        let nominal = match parameter {
            Some(index) if source_value.is_some() => {
                if parameters[index].pmi.is_none() {
                    parameters[index].pmi = Some(id.clone());
                }
                if let (Some(source), Some(current)) =
                    (source_value, parameters[index].value.as_ref())
                {
                    use cadmpeg_ir::features::DimensionValue;
                    if let Some(current) = PmiValue::from_parameter_value(current) {
                        if source != current {
                            result.losses.push(SldprtLossCode::PmiDimensionUnbound.note(format!("PMI record {} nominal differs from its canonical design parameter {}; the annotation binds that parameter", record.id, parameters[index].id)));
                        }
                    }
                }
                Some(Dimension::Parameter(parameters[index].id.clone()))
            }
            _ => source_value.map(Dimension::Literal),
        };
        result.annotations.push(PmiAnnotation {
            native_ref: None,
            id: id.clone(),
            name: None,
            visible: None,
            targets: Vec::new(),
            definition: PmiDefinition::Dimension(
                NeutralDimension::new(kind, nominal, None).map_err(CodecError::malformed)?,
            ),
        });
        if let Some(text) = record.display_text().filter(|text| !text.is_empty()) {
            result.annotations.push(PmiAnnotation {
                native_ref: None,
                id: PmiId::mint(format!("sldprt:model:pmi#presentation:{}", record.guid))
                    .map_err(CodecError::malformed)?,
                name: None,
                visible: None,
                targets: Vec::new(),
                definition: PmiDefinition::Presentation {
                    text: Some(text.to_owned()),
                    placement: None,
                    semantics: vec![id.clone()],
                },
            });
        }
        for (name, value) in [
            ("precision", AttributeValue::Integer(record.precision)),
            ("basic", AttributeValue::Boolean(record.basic)),
            ("inspection", AttributeValue::Boolean(record.inspection)),
            (
                "reference_only",
                AttributeValue::Boolean(record.reference_only),
            ),
        ] {
            result.attributes.push(SourceAttribute {
                native_ref: None,
                id: AttributeId::mint(format!("sldprt:model:attribute#pmi:{}:{name}", record.guid))
                    .map_err(CodecError::malformed)?,
                target: AttributeTarget::Pmi(id.clone()),
                name: name.to_owned(),
                values: vec![value],
            });
        }
    }
    Ok(result)
}

/// Parse work one `PMISemanticDataDB` section may spend per payload byte. One
/// unit is one `MessagePack` value or one byte of a validated string. The
/// candidates left when the section runs out are skipped and counted in one
/// loss.
const PMI_PARSE_WORK_PER_PAYLOAD_BYTE: usize = 16;

/// The part of a candidate map the dimension reader looks at. Other parts are
/// validated as strictly as kept ones but not stored, so one parse holds at
/// most one record's fields.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Shape {
    /// Nothing is read: strings stay unvalidated and every value is opaque
    /// except plain scalars.
    Skip,
    /// Scalars and strings are kept; containers are validated and opaque.
    Scalar,
    /// The candidate map: keeps `cadText`, `dimText` and `dimItems`.
    Record,
    /// `dimItems`: keeps its length and its first element as an item map.
    Items,
    /// The first `DimSemData` item: keeps the fields the record states.
    Item,
}

impl Shape {
    /// Shape of the value under `key` in a map of this shape, or `None` when
    /// the reader never looks at that key.
    fn field(self, key: &str) -> Option<Self> {
        match (self, key) {
            (Self::Record, "dimItems") => Some(Self::Items),
            (Self::Record, "cadText" | "dimText")
            | (
                Self::Item,
                "class" | "value" | "valPrecision" | "isBasic" | "isInspection" | "isReferenceOnly"
                | "dimSubType",
            ) => Some(Self::Scalar),
            _ => None,
        }
    }
}

/// What one parse keeps and the section budget it spends.
#[derive(Clone, Copy)]
struct Parse<'w> {
    shape: Shape,
    work: &'w WorkBudget,
}

impl Parse<'_> {
    fn with(self, shape: Shape) -> Self {
        Self { shape, ..self }
    }
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
    /// The length of `dimItems` and its first element.
    Array {
        len: usize,
        first: Option<Box<SpannedValue<'a>>>,
    },
    Map(BTreeMap<&'a str, SpannedValue<'a>>),
    Opaque,
}

/// Extract semantic dimensions from `PMISemanticDataDB` sections.
///
/// Parse failures on GUID-prefixed `MessagePack` maps emit
/// [`SldprtLossCode::PmiSemanticRecordMalformed`] instead of shrinking the
/// document silently. Each section spends at most
/// [`PMI_PARSE_WORK_PER_PAYLOAD_BYTE`] per payload byte; candidates left when
/// it runs out emit one [`SldprtLossCode::PmiSemanticParseCapped`].
pub(crate) fn dimensions(
    ctx: &DecodeContext<'_>,
    scan: &ContainerScan,
    annotations: &mut Annotations,
    losses: &mut Vec<LossNote>,
    untyped_sections: &mut std::collections::BTreeSet<String>,
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
                untyped_sections: Some(untyped_sections),
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
            untyped_sections: None,
        },
    )?;
    records.sort_by(|left, right| left.id.cmp(&right.id));
    Ok(records)
}

struct DimensionOutput<'a> {
    records: &'a mut Vec<PmiDimension>,
    seen: &'a mut HashSet<String>,
    untyped_sections: Option<&'a mut std::collections::BTreeSet<String>>,
}

fn collect_dimensions(
    ctx: &DecodeContext<'_>,
    payload: &[u8],
    stream: &cadmpeg_ir::StreamName,
    parent: &str,
    annotations: &mut Annotations,
    losses: &mut Vec<LossNote>,
    mut output: DimensionOutput<'_>,
) -> Result<(), CodecError> {
    let images = match database::read::images(ctx, payload) {
        Ok(Some(images)) => images,
        Ok(None) => {
            return collect_dimension_maps(
                ctx,
                payload,
                stream,
                parent,
                annotations,
                losses,
                output,
            )
        }
        Err(error @ CodecError::ResourceLimit(_)) => return Err(error),
        Err(error) => {
            if let Some(sections) = output.untyped_sections.as_deref_mut() {
                sections.insert(parent.to_owned());
            }
            losses.push(
                SldprtLossCode::PmiSemanticRecordMalformed
                    .note(format!("PMISemanticDataDB section {parent}: {error}")),
            );
            // Retain independent readable maps when its database graph failed.
            return collect_dimension_maps(
                ctx,
                payload,
                stream,
                parent,
                annotations,
                losses,
                output,
            );
        }
    };
    for image in images {
        if !write::valid_guid(&image.key) {
            if let Some(sections) = output.untyped_sections.as_deref_mut() {
                sections.insert(parent.to_owned());
            }
            losses.push(SldprtLossCode::PmiSemanticRecordUntyped.note(format!(
                "PMISemanticDataDB key {:?} has no typed semantic representation",
                image.key
            )));
            continue;
        }
        let first = output.records.len();
        let mut encoded = image.key.as_bytes().to_vec();
        encoded.extend_from_slice(&image.value);
        collect_dimension_maps(
            ctx,
            &encoded,
            stream,
            parent,
            &mut Annotations::default(),
            losses,
            DimensionOutput {
                records: output.records,
                seen: output.seen,
                untyped_sections: output.untyped_sections.as_deref_mut(),
            },
        )?;
        for record in &mut output.records[first..] {
            let source = |offset: u64| {
                offset
                    .checked_sub(36)
                    .map(|offset| image.source_offset(offset))
            };
            record.offset = record.offset.and_then(source);
            record.value_offset = record.value_offset.and_then(source);
            record.precision_offset = record.precision_offset.and_then(source);
            record.basic_offset = record.basic_offset.and_then(source);
            record.inspection_offset = record.inspection_offset.and_then(source);
            record.reference_only_offset = record.reference_only_offset.and_then(source);
            if let Some((_, offset)) = &mut record.display_text {
                *offset = offset.and_then(source);
            }
            crate::annotations::note(
                annotations,
                &record.id,
                stream,
                record.offset.ok_or_else(|| {
                    CodecError::malformed("source PMI dimension lacks its parsed map position")
                })?,
                "messagepack_dim_sem_data",
                Exactness::ByteExact,
            )?;
        }
    }
    Ok(())
}

fn collect_dimension_maps(
    ctx: &DecodeContext<'_>,
    payload: &[u8],
    stream: &cadmpeg_ir::StreamName,
    parent: &str,
    annotations: &mut Annotations,
    losses: &mut Vec<LossNote>,
    output: DimensionOutput<'_>,
) -> Result<(), CodecError> {
    let DimensionOutput {
        records,
        seen,
        mut untyped_sections,
    } = output;
    let mut candidates = candidate_maps(payload);
    let Some(work_limit) = payload.len().checked_mul(PMI_PARSE_WORK_PER_PAYLOAD_BYTE) else {
        let skipped = candidates.count();
        if skipped > 0 {
            if let Some(sections) = untyped_sections.as_deref_mut() {
                sections.insert(parent.to_owned());
            }
            losses.push(parse_capped_loss(parent, payload.len(), 0, skipped));
        }
        return Ok(());
    };
    let work = WorkBudget::new(work_limit);
    while let Some((guid, offset)) = candidates.next() {
        let normalized = guid.to_ascii_lowercase();
        if seen.contains(&normalized) {
            continue;
        }
        match extract_dimension(ctx, payload, offset, &normalized, parent, &work) {
            Ok(Some(record)) => {
                if record.item_count.get() != 1 {
                    if let Some(sections) = untyped_sections.as_deref_mut() {
                        sections.insert(parent.to_owned());
                    }
                    losses.push(SldprtLossCode::PmiSemanticRecordUntyped.note(format!(
                        "PMI record {} contains {} additional semantic dimension item(s) without typed payloads",
                        record.id, record.item_count.get() - 1
                    )));
                }
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
            Ok(None) => {
                if let Some(sections) = untyped_sections.as_deref_mut() {
                    sections.insert(parent.to_owned());
                }
                losses.push(SldprtLossCode::PmiSemanticRecordUntyped.note(format!(
                    "PMISemanticDataDB map at offset {offset} (guid {normalized}) has no supported typed semantic record"
                )));
            }
            Err(PmiParseError::Malformed(message)) => {
                if let Some(sections) = untyped_sections.as_deref_mut() {
                    sections.insert(parent.to_owned());
                }
                losses.push(SldprtLossCode::PmiSemanticRecordMalformed.note(format!(
                    "PMISemanticDataDB map at offset {offset} (guid {normalized}) {message}"
                )));
            }
            Err(PmiParseError::Capped) => {
                if let Some(sections) = untyped_sections.as_deref_mut() {
                    sections.insert(parent.to_owned());
                }
                let skipped = candidates.count() + 1;
                losses.push(parse_capped_loss(parent, payload.len(), offset, skipped));
                return Ok(());
            }
            Err(PmiParseError::Resource(error)) => return Err(error),
        }
        seen.insert(normalized);
    }
    Ok(())
}

fn parse_capped_loss(parent: &str, payload_len: usize, offset: usize, skipped: usize) -> LossNote {
    SldprtLossCode::PmiSemanticParseCapped.note(format!(
        "PMISemanticDataDB section {parent}: {skipped} candidate map(s) from offset {offset} \
         were not parsed because the section reached the local work cap \
         PMI_PARSE_WORK_PER_PAYLOAD_BYTE ({PMI_PARSE_WORK_PER_PAYLOAD_BYTE}) times its \
         {payload_len}-byte payload."
    ))
}

enum PmiParseError {
    Malformed(String),
    /// The section work budget ran out inside this candidate.
    Capped,
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
    work: &WorkBudget,
) -> Result<Option<PmiDimension>, PmiParseError> {
    let mut cursor = offset;
    let parse = Parse {
        shape: Shape::Record,
        work,
    };
    let Some(outer_value) = parse_value(ctx, payload, &mut cursor, 0, parse)? else {
        if work.exhausted() {
            return Err(PmiParseError::Capped);
        }
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
    let ValueKind::Array { len, first } = &items_value.kind else {
        return Err("dimItems is not an array".into());
    };
    let Ok(item_count) = u32::try_from(*len) else {
        return Err("dimItems length exceeds u32".into());
    };
    let Some(item_count) = std::num::NonZeroU32::new(item_count) else {
        return Err("dimItems is empty".into());
    };
    let Some(item_value) = first.as_deref() else {
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
        }) => Some(((*text).to_owned(), Some(u64_from_index(*data_offset)))),
        _ => None,
    };
    Ok(Some(PmiDimension {
        id,
        parent: parent.to_owned(),
        offset: Some(u64_from_index(offset)),
        guid: guid.to_owned(),
        cad_text: cad_text.to_owned(),
        item_count,
        subtype: string_field(item, "dimSubType")
            .unwrap_or_default()
            .to_owned(),
        value,
        value_offset: Some(u64_from_index(value_field.data_offset)),
        precision: int_from(precision_field)
            .ok_or_else(|| "valPrecision is not an integer".to_string())?,
        precision_offset: Some(u64_from_index(precision_field.data_offset)),
        display_text,
        basic: bool_from(basic_field).ok_or_else(|| "basic is not a boolean".to_string())?,
        basic_offset: Some(u64_from_index(basic_field.data_offset)),
        inspection: bool_from(inspection_field)
            .ok_or_else(|| "inspection is not a boolean".to_string())?,
        inspection_offset: Some(u64_from_index(inspection_field.data_offset)),
        reference_only: bool_from(reference_field)
            .ok_or_else(|| "reference_only is not a boolean".to_string())?,
        reference_only_offset: Some(u64_from_index(reference_field.data_offset)),
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

/// `Ok(None)` rejects the value, including when `parse.work` runs out; the
/// budget then reads exhausted.
fn parse_value<'a>(
    ctx: &DecodeContext<'_>,
    bytes: &'a [u8],
    cursor: &mut usize,
    depth: usize,
    parse: Parse<'_>,
) -> Result<Option<SpannedValue<'a>>, CodecError> {
    let _depth = ctx.enter_nested("parse SLDPRT PMI MessagePack")?;
    // The nesting cap rejects this one MessagePack value.
    if depth > 16 {
        return Ok(None);
    }
    if !parse.work.charge() {
        return Ok(None);
    }
    let start = *cursor;
    let Some(marker) = take_u8(bytes, cursor).map(Marker::from_u8) else {
        return Ok(None);
    };
    let value = match marker {
        Marker::FixMap(len) => {
            parse_map(ctx, bytes, cursor, usize::from(len), depth, start, parse)?
        }
        Marker::FixArray(len) => {
            parse_array(ctx, bytes, cursor, usize::from(len), depth, start, parse)?
        }
        Marker::FixStr(len) => parse_string(bytes, cursor, usize::from(len), start, parse),
        Marker::Str8 => match take_u8(bytes, cursor) {
            Some(len) => parse_string(bytes, cursor, usize::from(len), start, parse),
            None => None,
        },
        Marker::Str16 => match take_u16(bytes, cursor) {
            Some(len) => parse_string(bytes, cursor, usize::from(len), start, parse),
            None => None,
        },
        Marker::Str32 => match take_u32(bytes, cursor).and_then(|len| usize::try_from(len).ok()) {
            Some(len) => parse_string(bytes, cursor, len, start, parse),
            None => None,
        },
        Marker::Array16 => match take_u16(bytes, cursor) {
            Some(len) => parse_array(ctx, bytes, cursor, usize::from(len), depth, start, parse)?,
            None => None,
        },
        Marker::Array32 => {
            match take_u32(bytes, cursor).and_then(|len| usize::try_from(len).ok()) {
                Some(len) => parse_array(ctx, bytes, cursor, len, depth, start, parse)?,
                None => None,
            }
        }
        Marker::Map16 => match take_u16(bytes, cursor) {
            Some(len) => parse_map(ctx, bytes, cursor, usize::from(len), depth, start, parse)?,
            None => None,
        },
        Marker::Map32 => match take_u32(bytes, cursor).and_then(|len| usize::try_from(len).ok()) {
            Some(len) => parse_map(ctx, bytes, cursor, len, depth, start, parse)?,
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

/// Keys are always validated; only maps of a keeping shape store their read
/// fields.
fn parse_map<'a>(
    ctx: &DecodeContext<'_>,
    bytes: &'a [u8],
    cursor: &mut usize,
    len: usize,
    depth: usize,
    start: usize,
    parse: Parse<'_>,
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
        let Some(key_value) =
            parse_value(ctx, bytes, cursor, next_depth, parse.with(Shape::Scalar))?
        else {
            return Ok(None);
        };
        let ValueKind::String(key) = key_value.kind else {
            return Ok(None);
        };
        let field = parse.shape.field(key);
        let value_parse = parse.with(field.unwrap_or(Shape::Skip));
        let Some(value) = parse_value(ctx, bytes, cursor, next_depth, value_parse)? else {
            return Ok(None);
        };
        if field.is_some() {
            values.insert(key, value);
        }
    }
    let kind = match parse.shape {
        Shape::Record | Shape::Item => ValueKind::Map(values),
        Shape::Skip | Shape::Scalar | Shape::Items => ValueKind::Opaque,
    };
    Ok(Some(SpannedValue {
        kind,
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
    parse: Parse<'_>,
) -> Result<Option<SpannedValue<'a>>, CodecError> {
    // Every element encodes as at least one marker byte, so a length exceeding
    // the unread input cannot be satisfied and is rejected before allocating.
    let Some(remaining) = bytes.len().checked_sub(*cursor) else {
        return Ok(None);
    };
    let Some(len) = cadmpeg_core::decode::bounded_len(u64_from_index(len), 1, remaining) else {
        return Ok(None);
    };
    let items = parse.shape == Shape::Items;
    let mut first = None;
    let next_depth = depth + 1;
    for index in 0..len {
        let keep = items && index == 0;
        let shape = if keep { Shape::Item } else { Shape::Skip };
        let Some(value) = parse_value(ctx, bytes, cursor, next_depth, parse.with(shape))? else {
            return Ok(None);
        };
        if keep {
            first = Some(Box::new(value));
        }
    }
    let kind = if items {
        ValueKind::Array { len, first }
    } else {
        ValueKind::Opaque
    };
    Ok(Some(SpannedValue {
        kind,
        start,
        data_offset: start,
    }))
}

/// A skipped string is not validated; any other string charges its length.
fn parse_string<'a>(
    bytes: &'a [u8],
    cursor: &mut usize,
    len: usize,
    start: usize,
    parse: Parse<'_>,
) -> Option<SpannedValue<'a>> {
    let data_offset = *cursor;
    let end = cursor.checked_add(len)?;
    let value = bytes.get(*cursor..end)?;
    let kind = if parse.shape == Shape::Skip {
        ValueKind::Opaque
    } else {
        if !parse.work.charge_by(len) {
            return None;
        }
        std::str::from_utf8(value).map_or(ValueKind::Opaque, ValueKind::String)
    };
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
