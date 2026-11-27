// SPDX-License-Identifier: Apache-2.0
//! Dimensional annotations from neutral attributes.

use std::collections::BTreeMap;

use cadmpeg_core::decode::DecodeContext;
use cadmpeg_core::text::NonBlankString;
use cadmpeg_core::CodecError;
use cadmpeg_ir::design::dimension::Dimension;
use cadmpeg_ir::design::options::{NativeOption, NativeOptions};
use cadmpeg_ir::design::parameter::{Deviations, Tolerance};
use cadmpeg_ir::document::CadIr;
use cadmpeg_ir::ids::PmiId;
use cadmpeg_ir::pmi::{
    PmiAnnotation, PmiDefinition, PmiDimension, PmiDimensionKind as DimensionKind, PmiQuantity,
    PmiTarget, PmiValue,
};
use cadmpeg_ir::report::loss::LossNote;
use cadmpeg_ir::unknown::part::GivenUpRecord;
use cadmpeg_ir::{AnnotationBuilder, Exactness};

use crate::loss::CreoLossCode;

use super::attributes::{Attribute, Value};
use super::elements::Element;

const SYMMETRIC_LENGTH_LIMIT_ROUNDING_TOLERANCE_MM: f64 = 1.0e-9;
const SYMMETRIC_ANGLE_LIMIT_ROUNDING_TOLERANCE_RAD: f64 = 1.0e-12;
const DEVIATION_LIMIT_RELEASE: u32 = 4;
const ABSOLUTE_LIMIT_RELEASE: u32 = 10;

/// A tolerance as the attributes state it: deviations about the nominal, or
/// the lower and upper bounds.
#[derive(Clone, Copy, Debug, PartialEq)]
enum DimensionTolerance {
    PlusMinus { lower: PmiValue, upper: PmiValue },
    Limits { lower: PmiValue, upper: PmiValue },
}

/// A dimension admitted from its kind, nominal, and stated tolerance, with
/// the native values its annotation keeps.
struct Admitted {
    dimension: PmiDimension,
    native_options: NativeOptions,
    source_settings: NativeOptions,
}

/// Admit a dimension, refusing a nominal or tolerance whose quantity
/// disagrees with the kind and reversed bounds. Bounds about a nominal are
/// its deviations; bounds with no nominal stay native.
fn admit(
    kind: &DimensionKind,
    nominal: Option<PmiValue>,
    tolerance: Option<DimensionTolerance>,
    mode: Option<&[u8]>,
) -> Result<Admitted, String> {
    let expected = kind.quantity().or(nominal.map(|value| value.quantity));
    if nominal.is_some_and(|value| Some(value.quantity) != expected) {
        return Err("dimension nominal quantity disagrees with its kind".into());
    }
    let mut native_options = NativeOptions::new();
    let mut source_settings = NativeOptions::new();
    let stated = match tolerance {
        None => None,
        Some(
            DimensionTolerance::PlusMinus { lower, upper }
            | DimensionTolerance::Limits { lower, upper },
        ) if lower.quantity != upper.quantity => {
            return Err("dimension tolerance quantities disagree".into());
        }
        Some(
            DimensionTolerance::PlusMinus { lower, .. } | DimensionTolerance::Limits { lower, .. },
        ) if expected.is_some_and(|quantity| quantity != lower.quantity) => {
            return Err("dimension tolerance quantity disagrees with its kind or nominal".into());
        }
        Some(DimensionTolerance::Limits { lower, upper })
            if lower.value.get() > upper.value.get() =>
        {
            return Err("dimension lower limit exceeds its upper limit".into());
        }
        Some(DimensionTolerance::PlusMinus { lower, upper }) => {
            Some(Tolerance::Deviations(Deviations {
                lower: lower.value,
                upper: upper.value,
            }))
        }
        Some(DimensionTolerance::Limits { lower, upper }) => {
            let deviations = nominal.and_then(|nominal| {
                Deviations::from_limits(nominal.value.get(), lower.value.get(), upper.value.get())
            });
            if deviations.is_none() {
                for (name, limit) in [
                    (cadmpeg_ir::codec_name!("creo:lower_limit"), lower),
                    (cadmpeg_ir::codec_name!("creo:upper_limit"), upper),
                ] {
                    native_options.insert(name, measure_option(limit));
                }
            }
            deviations.map(Tolerance::Deviations)
        }
    };
    if stated.is_some() {
        if let Some(mode) = mode
            .and_then(|mode| std::str::from_utf8(mode).ok())
            .and_then(NonBlankString::new)
        {
            source_settings.insert(
                cadmpeg_ir::codec_name!("creo:tol_type"),
                NativeOption::Text(Dimension::Literal(mode)),
            );
        }
    }
    let mut dimension = PmiDimension::new(kind.clone());
    dimension.nominal = nominal.map(Dimension::Literal);
    dimension.tolerance = stated;
    Ok(Admitted {
        dimension,
        native_options,
        source_settings,
    })
}

fn measure_option(value: PmiValue) -> NativeOption {
    match value.quantity {
        PmiQuantity::Length => NativeOption::Length(Dimension::Literal(
            cadmpeg_ir::scalar::Length::from_assigned_real(value.value),
        )),
        PmiQuantity::Angle => NativeOption::Angle(Dimension::Literal(
            cadmpeg_ir::scalar::Angle::from_assigned_real(value.value),
        )),
        PmiQuantity::Ratio => NativeOption::Real(Dimension::Literal(value.value)),
    }
}

/// How `PTC_DIM_LOWER_LIMIT` and `PTC_DIM_UPPER_LIMIT` state a dimension's bounds.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum LimitForm {
    /// The attributes are the lower and upper bounds.
    Absolute,
    /// The attributes are deviations below and above the nominal.
    Deviation,
}

impl LimitForm {
    fn other(self) -> Self {
        match self {
            Self::Absolute => Self::Deviation,
            Self::Deviation => Self::Absolute,
        }
    }
}

#[derive(Default)]
struct DimensionFields<'a> {
    kind: Option<&'a [u8]>,
    nominal: Option<f64>,
    mode: Option<&'a [u8]>,
    tolerance: Option<f64>,
    upper_tolerance: Option<f64>,
    lower_tolerance: Option<f64>,
    upper_limit: Option<f64>,
    lower_limit: Option<f64>,
    angular_unit: Option<&'a [u8]>,
    duplicate: bool,
    invalid: bool,
    offset: Option<usize>,
}

struct ProjectedDimension {
    owner: u32,
    owner_type: u32,
    offset: usize,
    definition: Admitted,
    targets: Vec<PmiTarget>,
}

/// Source-wide inputs to dimension values.
pub(super) struct DimensionSource {
    /// Millimetres per model length unit, when the unit system is known.
    pub(super) length_scale_mm: Option<f64>,
    /// Major release of the saving product, from the `#Creo  TM` banner.
    pub(super) release_major: Option<u32>,
}

/// Annotation elements by section and the targets of their dimension slots.
pub(super) struct AnnotationIndex<'a> {
    pub(super) elements: &'a [(&'a str, Vec<Option<Element>>)],
    pub(super) dimension_targets: &'a BTreeMap<(&'a str, u32), super::targets::DimensionTarget>,
}

/// Returns the byte ranges of the dimension attribute rows that name no owner.
pub(super) fn transfer(
    ctx: &DecodeContext<'_>,
    attributes: &[Option<Attribute<'_>>],
    index: &AnnotationIndex<'_>,
    source: &DimensionSource,
    ir: &mut CadIr,
    annotations: &mut AnnotationBuilder,
    losses: &mut Vec<LossNote>,
) -> Result<Vec<(usize, usize)>, CodecError> {
    let mut ownerless = Vec::new();
    let mut fields_by_owner = BTreeMap::<(u32, u32), DimensionFields<'_>>::new();
    let mut unresolved = 0usize;
    let mut unformed = Vec::new();
    for attribute in attributes.iter().flatten() {
        if !dimension_field(attribute.name) {
            continue;
        }
        let (Some(owner), Some(owner_type)) = (attribute.owner, attribute.owner_type) else {
            unresolved += 1;
            ownerless.push((attribute.offset, attribute.end));
            continue;
        };
        add_attribute(
            fields_by_owner.entry((owner, owner_type)).or_default(),
            attribute,
        );
    }

    let mut elements_by_owner = BTreeMap::<(u32, u32), Vec<(&str, Element)>>::new();
    for (section, elements) in index.elements {
        for element in elements.iter().flatten() {
            if element.annotation_type == Some(8) {
                elements_by_owner
                    .entry((element.owner, element.owner_type))
                    .or_default()
                    .push((section, *element));
            }
        }
    }

    let mut projected = Vec::new();
    let mut length_dimensions = 0usize;
    let mut inferred_linear_kinds = 0usize;
    let mut assumed_length_kinds = 0usize;
    let mut surface_targets = 0usize;
    let mut edge_targets = 0usize;
    let mut conflicting = 0usize;
    let mut assumed_angular_units = 0usize;
    let mut assumed_deviation_limits = 0usize;
    let mut assumed_absolute_limits = 0usize;
    for (owner, fields) in fields_by_owner {
        let record = super::pmi_record(format_args!("{}:{}", owner.1, owner.0));
        let Some(kind) = fields.kind else {
            unresolved += 1;
            unformed.push(record);
            continue;
        };
        let Some(candidates) = elements_by_owner.get(&owner) else {
            unresolved += 1;
            unformed.push(record);
            continue;
        };
        // More than one annotation element claims the owner.
        if candidates.len() != 1 {
            unresolved += 1;
            unformed.push(record);
            continue;
        }
        if fields.duplicate {
            conflicting += 1;
        }
        if fields.invalid {
            unresolved += 1;
        }
        let (section, element) = candidates[0];
        let Some(kind_text) = std::str::from_utf8(kind)
            .ok()
            .filter(|text| !text.is_empty())
        else {
            unresolved += 1;
            unformed.push(record);
            continue;
        };
        let is_linear_kind_inference = kind_text == "Linear";
        let dimension_kind = dimension_kind(kind_text);
        let is_unknown_kind = matches!(dimension_kind, DimensionKind::Other(_));
        let quantity = if dimension_kind == DimensionKind::Angular {
            PmiQuantity::Angle
        } else {
            PmiQuantity::Length
        };
        if quantity == PmiQuantity::Angle
            && !matches!(
                fields.angular_unit,
                Some(unit) if unit == b"Degrees" || unit == b"Radians"
            )
        {
            assumed_angular_units += 1;
        }
        let length_scale = source.length_scale_mm.unwrap_or(1.0);
        let nominal = match fields.nominal {
            Some(value) => match value_for(value, quantity, length_scale) {
                Some(value) => Some(value),
                None => {
                    unresolved += 1;
                    unformed.push(record);
                    continue;
                }
            },
            None => None,
        };
        let (mut form, form_selected) = limit_form(source.release_major, &fields);
        let (mut dimension_tolerance, mut unresolved_tolerance) =
            tolerance(&fields, fields.nominal, form, quantity, length_scale);
        let admissible = |candidate: &Option<DimensionTolerance>| {
            candidate.is_some() && admit(&dimension_kind, nominal, *candidate, fields.mode).is_ok()
        };
        if !form_selected && reads_limits(&fields) && !admissible(&dimension_tolerance) {
            let other = form.other();
            let retry = tolerance(&fields, fields.nominal, other, quantity, length_scale);
            if admissible(&retry.0) {
                (dimension_tolerance, unresolved_tolerance) = retry;
                form = other;
            }
        }
        let assumed_form = !form_selected && dimension_tolerance.is_some() && reads_limits(&fields);
        unresolved += usize::from(unresolved_tolerance);
        let definition = match admit(&dimension_kind, nominal, dimension_tolerance, fields.mode) {
            Ok(definition) => {
                if assumed_form {
                    match form {
                        LimitForm::Deviation => assumed_deviation_limits += 1,
                        LimitForm::Absolute => assumed_absolute_limits += 1,
                    }
                }
                definition
            }
            Err(_) => {
                unresolved += 1;
                let Ok(definition) = admit(&dimension_kind, nominal, None, fields.mode) else {
                    unformed.push(record);
                    continue;
                };
                definition
            }
        };
        if quantity == PmiQuantity::Length {
            length_dimensions += 1;
        }
        let Some(offset) = fields.offset else {
            unresolved += 1;
            unformed.push(record);
            continue;
        };
        let target = element
            .annotation
            .and_then(|slot| index.dimension_targets.get(&(section, slot)));
        surface_targets += usize::from(target.is_some_and(|target| !target.from_edges));
        edge_targets += usize::from(target.is_some_and(|target| target.from_edges));
        let targets = target
            .map(|target| target.faces.clone())
            .unwrap_or_default();
        projected.push(ProjectedDimension {
            owner: element.owner,
            owner_type: element.owner_type,
            offset,
            definition,
            targets,
        });
        if is_linear_kind_inference {
            inferred_linear_kinds += 1;
        }
        if is_unknown_kind {
            assumed_length_kinds += 1;
        }
    }

    for dimension in &projected {
        let id = PmiId::try_from(format!(
            "creo:annotation:pmi#{}:{}",
            dimension.owner_type, dimension.owner
        ))
        .map_err(|_| CodecError::malformed("invalid Creo dimension identity"))?;
        ctx.charge_entities(1, "Creo dimensional annotation")?;
        super::super::native::annotate(
            annotations,
            &id,
            "NeuPrtSld",
            cadmpeg_core::decode::u64_from_index(dimension.offset),
            "dimensional attributes",
            Exactness::Derived,
        )?;
        let mut annotation = PmiAnnotation::new(
            id,
            dimension.targets.iter().cloned().map(Into::into).collect(),
            PmiDefinition::Dimension(dimension.definition.dimension.clone()),
        );
        annotation
            .native_options
            .clone_from(&dimension.definition.native_options);
        annotation
            .source_settings
            .clone_from(&dimension.definition.source_settings);
        ir.model.pmi.push(annotation);
    }

    if unresolved != 0 {
        losses.push(CreoLossCode::PmiRecordUnresolved.note(format!(
            "{unresolved} dimensional attribute owner(s) have an ambiguous join, invalid value, or unresolved tolerance mode."
        )));
    }
    let code = CreoLossCode::PmiRecordUnresolved.kind();
    for record in &unformed {
        losses.push(
            GivenUpRecord::new(
                cadmpeg_ir::ids::Identity::new(record)?,
                format_args!("{code}: the dimensional attributes form no PMI record"),
            )
            .loss(),
        );
    }
    if conflicting != 0 {
        losses.push(CreoLossCode::PmiFirstAttributeAssumed.note(format!(
            "{conflicting} dimension(s) use the first value of conflicting attributes."
        )));
    }
    let untargeted = projected
        .iter()
        .filter(|dimension| dimension.targets.is_empty())
        .count();
    if untargeted != 0 {
        losses.push(CreoLossCode::PmiAssociationsUnresolved.note(format!(
            "{untargeted} dimensional annotation(s) have unresolved geometric targets."
        )));
    }
    if !projected.is_empty() {
        losses.push(CreoLossCode::PmiAssociationsUnresolved.note(format!(
            "{} dimensional annotation(s) have unresolved display state.",
            projected.len()
        )));
        losses.push(CreoLossCode::PmiModifiersUnresolved.note(format!(
            "{} dimensional annotation(s) have unresolved modifiers.",
            projected.len()
        )));
    }
    if surface_targets != 0 {
        losses.push(CreoLossCode::PmiDimensionTargetInferred.note(format!(
            "{surface_targets} dimension(s) use the faces of their referenced surfaces, or the visible faces on a referenced datum cylinder, as targets."
        )));
    }
    if edge_targets != 0 {
        losses.push(CreoLossCode::PmiDimensionTargetInferred.note(format!(
            "{edge_targets} dimension(s) referencing two edges use, for each edge, its face that the other edge does not bound."
        )));
    }
    if inferred_linear_kinds != 0 {
        losses.push(CreoLossCode::PmiDimensionKindInferred.note(format!(
            "{inferred_linear_kinds} Creo linear dimension kind(s) use Location as the inferred semantic kind."
        )));
    }
    if assumed_length_kinds != 0 {
        losses.push(CreoLossCode::PmiDimensionKindInferred.note(format!(
            "{assumed_length_kinds} dimension(s) with an unknown kind use length values by assumption."
        )));
    }
    if assumed_deviation_limits + assumed_absolute_limits != 0 {
        losses.push(CreoLossCode::PmiDimensionLimitFormAssumed.note(format!(
            "{assumed_deviation_limits} dimension(s) read their limit attributes as deviations from the nominal and {assumed_absolute_limits} as absolute bounds; deviations are read when both values lie nearer zero than the nominal, or when only that form gives a valid tolerance."
        )));
    }
    if assumed_angular_units != 0 {
        losses.push(CreoLossCode::PmiAngularUnitAssumed.note(format!(
            "{assumed_angular_units} angular dimension(s) use stored radians by assumption."
        )));
    }
    if length_dimensions != 0 && source.length_scale_mm.is_none() {
        losses.push(
            CreoLossCode::PmiLengthUnitAssumed
                .note("Dimensional lengths use the default millimetre unit."),
        );
    }
    Ok(ownerless)
}

pub(super) fn dimension_field(name: &[u8]) -> bool {
    matches!(
        name,
        b"PTC_DIM_TYPE"
            | b"PTC_DIM_NOMINAL_VAL"
            | b"PTC_TOLERANCE_MODE"
            | b"PTC_DIM_TOL_VALUE"
            | b"PTC_DIM_UPPER_TOL_VALUE"
            | b"PTC_DIM_LOWER_TOL_VALUE"
            | b"PTC_DIM_UPPER_LIMIT"
            | b"PTC_DIM_LOWER_LIMIT"
            | b"PTC_DIM_ANGULARDIM_UNIT"
    )
}

fn add_attribute<'a>(fields: &mut DimensionFields<'a>, attribute: &Attribute<'a>) {
    match (attribute.name, attribute.value) {
        (b"PTC_DIM_TYPE", Value::Text(value)) => {
            fields.duplicate |= retain_first_text(&mut fields.kind, value);
            fields.offset.get_or_insert(attribute.offset);
        }
        (b"PTC_TOLERANCE_MODE", Value::Text(value)) => {
            fields.duplicate |= retain_first_text(&mut fields.mode, value);
        }
        (b"PTC_DIM_ANGULARDIM_UNIT", Value::Text(value)) => {
            fields.duplicate |= retain_first_text(&mut fields.angular_unit, value);
        }
        (b"PTC_DIM_NOMINAL_VAL", Value::Real(Some(value))) => {
            fields.duplicate |= retain_first_real(&mut fields.nominal, value);
        }
        (b"PTC_DIM_TOL_VALUE", Value::Real(Some(value))) => {
            fields.duplicate |= retain_first_real(&mut fields.tolerance, value);
        }
        (b"PTC_DIM_UPPER_TOL_VALUE", Value::Real(Some(value))) => {
            fields.duplicate |= retain_first_real(&mut fields.upper_tolerance, value);
        }
        (b"PTC_DIM_LOWER_TOL_VALUE", Value::Real(Some(value))) => {
            fields.duplicate |= retain_first_real(&mut fields.lower_tolerance, value);
        }
        (b"PTC_DIM_UPPER_LIMIT", Value::Real(Some(value))) => {
            fields.duplicate |= retain_first_real(&mut fields.upper_limit, value);
        }
        (b"PTC_DIM_LOWER_LIMIT", Value::Real(Some(value))) => {
            fields.duplicate |= retain_first_real(&mut fields.lower_limit, value);
        }
        _ => fields.invalid = true,
    }
}

fn retain_first_text<'a>(slot: &mut Option<&'a [u8]>, value: &'a [u8]) -> bool {
    match slot {
        Some(previous) => *previous != value,
        None => {
            *slot = Some(value);
            false
        }
    }
}

fn retain_first_real(slot: &mut Option<f64>, value: f64) -> bool {
    match slot {
        Some(previous) => previous.to_bits() != value.to_bits(),
        None => {
            *slot = Some(value);
            false
        }
    }
}

fn dimension_kind(source: &str) -> DimensionKind {
    match source {
        "Linear" => DimensionKind::Location,
        "Diameter" => DimensionKind::Diameter,
        "Radius" => DimensionKind::Radius,
        "Angle" => DimensionKind::Angular,
        _ => DimensionKind::Other(source.to_owned()),
    }
}

fn value_for(value: f64, quantity: PmiQuantity, length_scale: f64) -> Option<PmiValue> {
    let value = if quantity == PmiQuantity::Length {
        value * length_scale
    } else {
        value
    };
    PmiValue::new(value, quantity)
}

/// The limit form a dimension uses, and whether the saving release selects it.
fn limit_form(release_major: Option<u32>, fields: &DimensionFields<'_>) -> (LimitForm, bool) {
    match release_major {
        Some(DEVIATION_LIMIT_RELEASE) => (LimitForm::Deviation, true),
        Some(ABSOLUTE_LIMIT_RELEASE) => (LimitForm::Absolute, true),
        _ => {
            let nearer_zero = |value: f64, nominal: f64| (value - nominal).abs() > value.abs();
            let form = match (fields.nominal, fields.lower_limit, fields.upper_limit) {
                (Some(nominal), Some(lower), Some(upper))
                    if nearer_zero(lower, nominal) && nearer_zero(upper, nominal) =>
                {
                    LimitForm::Deviation
                }
                _ => LimitForm::Absolute,
            };
            (form, false)
        }
    }
}

/// Whether the dimension's tolerance mode takes its values from the limit attributes.
fn reads_limits(fields: &DimensionFields<'_>) -> bool {
    match fields.mode {
        Some(b"Limits") => true,
        Some(b"+- Symmetric") => fields.tolerance.is_none(),
        Some(b"Plus-Minus") => fields.upper_tolerance.is_none() || fields.lower_tolerance.is_none(),
        _ => false,
    }
}

/// The lower and upper bounds the limit attributes state, in source units.
fn absolute_limits(
    fields: &DimensionFields<'_>,
    nominal: Option<f64>,
    form: LimitForm,
) -> Option<(f64, f64)> {
    let lower = fields.lower_limit?;
    let upper = fields.upper_limit?;
    match form {
        LimitForm::Absolute => Some((lower, upper)),
        LimitForm::Deviation => nominal.map(|nominal| (nominal - lower, nominal + upper)),
    }
}

fn tolerance(
    fields: &DimensionFields<'_>,
    nominal: Option<f64>,
    form: LimitForm,
    quantity: PmiQuantity,
    length_scale: f64,
) -> (Option<DimensionTolerance>, bool) {
    let limits = absolute_limits(fields, nominal, form);
    match fields.mode {
        Some(b"Limits") => match limits {
            Some((lower, upper)) => match (
                value_for(lower, quantity, length_scale),
                value_for(upper, quantity, length_scale),
            ) {
                (Some(lower), Some(upper)) => {
                    (Some(DimensionTolerance::Limits { lower, upper }), false)
                }
                _ => (None, true),
            },
            _ => (None, true),
        },
        Some(b"+- Symmetric") => {
            let magnitude = fields.tolerance.or_else(|| {
                let nominal = nominal?;
                let (lower, upper) = limits?;
                let lower_deviation = (lower - nominal).abs();
                let upper_deviation = (upper - nominal).abs();
                let (lower_comparison, upper_comparison, rounding_tolerance) = match quantity {
                    PmiQuantity::Length => (
                        lower_deviation * length_scale,
                        upper_deviation * length_scale,
                        SYMMETRIC_LENGTH_LIMIT_ROUNDING_TOLERANCE_MM,
                    ),
                    PmiQuantity::Angle => (
                        lower_deviation,
                        upper_deviation,
                        SYMMETRIC_ANGLE_LIMIT_ROUNDING_TOLERANCE_RAD,
                    ),
                    PmiQuantity::Ratio => return None,
                };
                ((lower_comparison - upper_comparison).abs() <= rounding_tolerance)
                    .then_some(f64::midpoint(lower_deviation, upper_deviation))
            });
            match magnitude.and_then(|value| value_for(value.abs(), quantity, length_scale)) {
                Some(magnitude) => match PmiValue::new(-magnitude.value.get(), quantity) {
                    Some(negative) => (
                        Some(DimensionTolerance::PlusMinus {
                            lower: negative,
                            upper: magnitude,
                        }),
                        false,
                    ),
                    None => (None, true),
                },
                None => (None, true),
            }
        }
        Some(b"Plus-Minus") => {
            let (upper, lower) = match (fields.upper_tolerance, fields.lower_tolerance) {
                (Some(upper), Some(lower)) => (upper.abs(), lower.abs()),
                _ => match (nominal, limits) {
                    (Some(nominal), Some((lower, upper))) => {
                        ((upper - nominal).abs(), (lower - nominal).abs())
                    }
                    _ => return (None, true),
                },
            };
            match (
                value_for(-lower, quantity, length_scale),
                value_for(upper, quantity, length_scale),
            ) {
                (Some(lower), Some(upper)) => {
                    (Some(DimensionTolerance::PlusMinus { lower, upper }), false)
                }
                _ => (None, true),
            }
        }
        Some(b"Nominal") => (
            None,
            fields.tolerance.is_some()
                || fields.upper_tolerance.is_some()
                || fields.lower_tolerance.is_some()
                || fields.upper_limit.is_some()
                || fields.lower_limit.is_some(),
        ),
        Some(_) => (None, true),
        None => (
            None,
            fields.tolerance.is_some()
                || fields.upper_tolerance.is_some()
                || fields.lower_tolerance.is_some()
                || fields.upper_limit.is_some()
                || fields.lower_limit.is_some(),
        ),
    }
}

#[cfg(test)]
mod tests {
    use super::{
        add_attribute, dimension_kind, tolerance, transfer, AnnotationIndex, Attribute,
        DimensionFields, DimensionKind, DimensionSource, DimensionTolerance, Element, LimitForm,
        Value,
    };
    use crate::loss::CreoLossCode;
    use cadmpeg_ir::document::CadIr;
    use cadmpeg_ir::pmi::{PmiAnnotation, PmiDefinition, PmiQuantity, PmiValue};
    use cadmpeg_ir::report::loss::LossNote;
    use cadmpeg_ir::AnnotationBuilder;
    use std::collections::BTreeMap;
    use std::f64::consts::PI;

    const VALUE_TOLERANCE: f64 = 1.0e-12;
    const CTC_ANGLE_NOMINAL_RADIANS: f64 = 1.047_197_551_196_596_6;
    const CTC_ANGLE_TOLERANCE_RADIANS: f64 = 0.008_726_646_259_971_65;

    fn close(actual: f64, expected: f64) {
        assert!((actual - expected).abs() < VALUE_TOLERANCE);
    }

    fn numeric_dimension(
        kind: &str,
        fields: &DimensionFields<'static>,
    ) -> (
        Option<PmiValue>,
        Option<DimensionTolerance>,
        bool,
        DimensionKind,
    ) {
        let dimension_kind = dimension_kind(kind);
        let quantity = if dimension_kind == DimensionKind::Angular {
            PmiQuantity::Angle
        } else {
            PmiQuantity::Length
        };
        let raw_nominal = fields.nominal;
        let (tolerance, unresolved) =
            tolerance(fields, raw_nominal, LimitForm::Absolute, quantity, 1.0);
        let nominal =
            raw_nominal.map(|value| PmiValue::new(value, quantity).expect("finite nominal"));
        (nominal, tolerance, unresolved, dimension_kind)
    }

    #[test]
    fn limits_use_absolute_bounds_from_the_ctc_workbook() {
        let (nominal, tolerance, unresolved, kind) = numeric_dimension(
            "Diameter",
            &DimensionFields {
                nominal: Some(35.0),
                mode: Some(b"Limits"),
                upper_limit: Some(35.2),
                lower_limit: Some(34.8),
                ..DimensionFields::default()
            },
        );
        assert_eq!(kind, DimensionKind::Diameter);
        close(nominal.expect("nominal").value.get(), 35.0);
        let Some(DimensionTolerance::Limits { lower, upper }) = tolerance else {
            panic!("limit tolerance")
        };
        close(lower.value.get(), 34.8);
        close(upper.value.get(), 35.2);
        assert!(!unresolved);
    }

    #[test]
    fn symmetric_tolerance_uses_signed_deviations_from_the_ctc_workbook() {
        let (_, tolerance, unresolved, kind) = numeric_dimension(
            "Diameter",
            &DimensionFields {
                nominal: Some(25.0),
                mode: Some(b"+- Symmetric"),
                tolerance: Some(0.15),
                upper_limit: Some(25.15),
                lower_limit: Some(24.85),
                ..DimensionFields::default()
            },
        );
        assert_eq!(kind, DimensionKind::Diameter);
        let Some(DimensionTolerance::PlusMinus { lower, upper }) = tolerance else {
            panic!("plus-minus tolerance")
        };
        close(lower.value.get(), -0.15);
        close(upper.value.get(), 0.15);
        assert!(!unresolved);
    }

    #[test]
    fn plus_minus_fields_preserve_unequal_and_one_sided_signs() {
        for (nominal, upper_value, lower_value, expected_lower, expected_upper) in [
            (20.0, 0.05, 0.1, -0.1, 0.05),
            (20.0, 0.1, 0.05, -0.05, 0.1),
            (35.0, 0.2, 0.0, 0.0, 0.2),
            (35.0, 0.0, 0.2, -0.2, 0.0),
        ] {
            let (_, tolerance, unresolved, kind) = numeric_dimension(
                "Diameter",
                &DimensionFields {
                    nominal: Some(nominal),
                    mode: Some(b"Plus-Minus"),
                    upper_tolerance: Some(upper_value),
                    lower_tolerance: Some(lower_value),
                    ..DimensionFields::default()
                },
            );
            assert_eq!(kind, DimensionKind::Diameter);
            let Some(DimensionTolerance::PlusMinus { lower, upper }) = tolerance else {
                panic!("plus-minus tolerance")
            };
            close(lower.value.get(), expected_lower);
            close(upper.value.get(), expected_upper);
            assert!(!unresolved);
        }
    }

    #[test]
    fn angular_tolerance_values_are_radians_as_in_the_step_twin() {
        let (nominal, tolerance, unresolved, kind) = numeric_dimension(
            "Angle",
            &DimensionFields {
                nominal: Some(CTC_ANGLE_NOMINAL_RADIANS),
                mode: Some(b"+- Symmetric"),
                tolerance: Some(CTC_ANGLE_TOLERANCE_RADIANS),
                angular_unit: Some(b"Degrees"),
                ..DimensionFields::default()
            },
        );
        assert_eq!(kind, DimensionKind::Angular);
        close(nominal.expect("nominal").value.get(), PI / 3.0);
        let Some(DimensionTolerance::PlusMinus { lower, upper }) = tolerance else {
            panic!("angular plus-minus tolerance")
        };
        close(lower.value.get(), -PI / 360.0);
        close(upper.value.get(), PI / 360.0);
        assert!(!unresolved);
    }

    #[test]
    fn symmetric_limit_inference_allows_floating_point_rounding() {
        let (_, tolerance, unresolved, _) = numeric_dimension(
            "Diameter",
            &DimensionFields {
                nominal: Some(25.0),
                mode: Some(b"+- Symmetric"),
                upper_limit: Some(25.150_000_000_000_002),
                lower_limit: Some(24.85),
                ..DimensionFields::default()
            },
        );
        let Some(DimensionTolerance::PlusMinus { lower, upper }) = tolerance else {
            panic!("plus-minus tolerance")
        };
        close(lower.value.get(), -0.15);
        close(upper.value.get(), 0.15);
        assert!(!unresolved);
    }

    #[test]
    fn conflicting_auxiliary_values_keep_the_first_supported_nominal() {
        let mut fields = DimensionFields {
            nominal: Some(85.0),
            mode: Some(b"Nominal"),
            ..DimensionFields::default()
        };
        let first = Attribute {
            name: b"PTC_DIM_UPPER_TOL_VALUE",
            value: Value::Real(Some(0.01)),
            owner: Some(3886),
            owner_type: Some(3),
            unit: b"",
            offset: 1,
            end: 2,
        };
        let conflicting = Attribute {
            value: Value::Real(Some(0.02)),
            offset: 2,
            ..first
        };
        add_attribute(&mut fields, &first);
        add_attribute(&mut fields, &conflicting);
        close(fields.nominal.expect("preserved nominal"), 85.0);
        close(fields.upper_tolerance.expect("first tolerance"), 0.01);
        assert!(fields.duplicate);
        let (tolerance, unresolved) = tolerance(
            &fields,
            fields.nominal,
            LimitForm::Absolute,
            PmiQuantity::Length,
            1.0,
        );
        assert!(tolerance.is_none());
        assert!(unresolved);
    }

    fn transferred_radius(length_scale_mm: Option<f64>) -> (Vec<PmiAnnotation>, Vec<LossNote>) {
        let attribute = |name: &'static [u8], value, offset| Attribute {
            name,
            value,
            owner: Some(7),
            owner_type: Some(3),
            unit: b"",
            offset,
            end: offset + 1,
        };
        let attributes = [
            Some(attribute(b"PTC_DIM_TYPE", Value::Text(b"Radius"), 40)),
            Some(attribute(
                b"PTC_DIM_NOMINAL_VAL",
                Value::Real(Some(5.0)),
                60,
            )),
        ];
        let elements = [(
            "BasFullData",
            vec![Some(Element {
                owner: 7,
                owner_type: 3,
                annotation: None,
                annotation_type: Some(8),
            })],
        )];
        let dimension_targets = BTreeMap::new();
        let index = AnnotationIndex {
            elements: &elements,
            dimension_targets: &dimension_targets,
        };
        let mut ir = CadIr::empty();
        let mut losses = Vec::new();
        crate::decode::with_service_context(|ctx| {
            transfer(
                ctx,
                &attributes,
                &index,
                &DimensionSource {
                    length_scale_mm,
                    release_major: None,
                },
                &mut ir,
                &mut AnnotationBuilder::new(),
                &mut losses,
            )
        })
        .expect("radius dimension transfers");
        (ir.model.pmi, losses)
    }

    #[test]
    fn radius_dimension_type_decodes_as_a_length_radius() {
        for (length_scale_mm, expected_nominal_mm) in [(None, 5.0), (Some(25.4), 127.0)] {
            let (pmi, losses) = transferred_radius(length_scale_mm);
            assert_eq!(pmi.len(), 1);
            let PmiDefinition::Dimension(dimension) = &pmi[0].definition else {
                panic!("dimension definition")
            };
            assert_eq!(&dimension.dimension, &DimensionKind::Radius);
            let nominal = dimension.literal_nominal().expect("nominal");
            assert_eq!(nominal.quantity, PmiQuantity::Length);
            close(nominal.value.get(), expected_nominal_mm);
            assert!(losses
                .iter()
                .all(|loss| loss.code != CreoLossCode::PmiDimensionKindInferred.kind()));
        }
    }

    #[test]
    fn nominal_linear_dimensions_keep_the_value_without_an_inferred_tolerance() {
        for (source_nominal, expected_nominal) in [(85.0, 85.0), (49.992_187_5, 49.992_187_5)] {
            let (nominal, tolerance, unresolved, kind) = numeric_dimension(
                "Linear",
                &DimensionFields {
                    nominal: Some(source_nominal),
                    mode: Some(b"Nominal"),
                    upper_tolerance: Some(0.01),
                    lower_tolerance: Some(0.01),
                    upper_limit: Some(source_nominal + 0.01),
                    lower_limit: Some(source_nominal - 0.01),
                    ..DimensionFields::default()
                },
            );
            assert_eq!(kind, DimensionKind::Location);
            close(nominal.expect("nominal").value.get(), expected_nominal);
            assert!(tolerance.is_none());
            assert!(unresolved);
        }
    }

    fn transferred_tolerance(
        mode: &'static [u8],
        lower: f64,
        upper: f64,
        release_major: Option<u32>,
        tolerance_values: &[(&'static [u8], f64)],
    ) -> (Option<DimensionTolerance>, Vec<LossNote>) {
        let attribute = |name: &'static [u8], value, offset| Attribute {
            name,
            value,
            owner: Some(9),
            owner_type: Some(3),
            unit: b"",
            offset,
            end: offset + 1,
        };
        let mut attributes = vec![
            Some(attribute(b"PTC_DIM_TYPE", Value::Text(b"Diameter"), 10)),
            Some(attribute(
                b"PTC_DIM_NOMINAL_VAL",
                Value::Real(Some(35.0)),
                20,
            )),
            Some(attribute(b"PTC_TOLERANCE_MODE", Value::Text(mode), 30)),
            Some(attribute(
                b"PTC_DIM_LOWER_LIMIT",
                Value::Real(Some(lower)),
                40,
            )),
            Some(attribute(
                b"PTC_DIM_UPPER_LIMIT",
                Value::Real(Some(upper)),
                50,
            )),
        ];
        attributes.extend(
            tolerance_values
                .iter()
                .map(|(name, value)| Some(attribute(name, Value::Real(Some(*value)), 60))),
        );
        let elements = [(
            "BasFullData",
            vec![Some(Element {
                owner: 9,
                owner_type: 3,
                annotation: None,
                annotation_type: Some(8),
            })],
        )];
        let dimension_targets = BTreeMap::new();
        let index = AnnotationIndex {
            elements: &elements,
            dimension_targets: &dimension_targets,
        };
        let mut ir = CadIr::empty();
        let mut losses = Vec::new();
        crate::decode::with_service_context(|ctx| {
            transfer(
                ctx,
                &attributes,
                &index,
                &DimensionSource {
                    length_scale_mm: Some(1.0),
                    release_major,
                },
                &mut ir,
                &mut AnnotationBuilder::new(),
                &mut losses,
            )
        })
        .expect("dimension transfers");
        let [annotation] = ir.model.pmi.as_slice() else {
            panic!("one dimension")
        };
        let PmiDefinition::Dimension(dimension) = &annotation.definition else {
            panic!("dimension definition")
        };
        let limits = annotation.source_settings.get("creo:tol_type")
            == Some(&cadmpeg_ir::design::options::NativeOption::Text(
                cadmpeg_ir::design::dimension::Dimension::Literal(
                    cadmpeg_core::text::NonBlankString::new("Limits").expect("a mode"),
                ),
            ));
        let nominal = dimension
            .literal_nominal()
            .map_or(0.0, |value| value.value.get());
        let length = |value| PmiValue::new(value, PmiQuantity::Length).expect("finite length");
        let tolerance = match &dimension.tolerance {
            Some(cadmpeg_ir::design::parameter::Tolerance::Deviations(deviations)) if limits => {
                Some(DimensionTolerance::Limits {
                    lower: length(nominal + deviations.lower.get()),
                    upper: length(nominal + deviations.upper.get()),
                })
            }
            Some(cadmpeg_ir::design::parameter::Tolerance::Deviations(deviations)) => {
                Some(DimensionTolerance::PlusMinus {
                    lower: length(deviations.lower.get()),
                    upper: length(deviations.upper.get()),
                })
            }
            _ => None,
        };
        (tolerance, losses)
    }

    fn limit_form_losses(losses: &[LossNote]) -> usize {
        losses
            .iter()
            .filter(|loss| loss.code == CreoLossCode::PmiDimensionLimitFormAssumed.kind())
            .count()
    }

    #[test]
    fn creo_4_limits_are_deviations_and_creo_10_limits_are_bounds() {
        for (release, lower, upper) in [(4, 0.2, 0.2), (10, 34.8, 35.2)] {
            let (tolerance, losses) =
                transferred_tolerance(b"Limits", lower, upper, Some(release), &[]);
            let Some(DimensionTolerance::Limits { lower, upper }) = tolerance else {
                panic!("limit tolerance")
            };
            close(lower.value.get(), 34.8);
            close(upper.value.get(), 35.2);
            assert_eq!(limit_form_losses(&losses), 0);
        }
    }

    #[test]
    fn plus_minus_without_tolerance_values_reads_the_release_limit_form() {
        for (release, lower, upper) in [(4, 0.2, 0.2), (10, 34.8, 35.2)] {
            let (tolerance, losses) =
                transferred_tolerance(b"Plus-Minus", lower, upper, Some(release), &[]);
            let Some(DimensionTolerance::PlusMinus { lower, upper }) = tolerance else {
                panic!("plus-minus tolerance")
            };
            close(lower.value.get(), -0.2);
            close(upper.value.get(), 0.2);
            assert_eq!(limit_form_losses(&losses), 0);
        }
    }

    #[test]
    fn limits_without_a_selecting_release_are_deviations_only_nearer_zero_and_are_charged() {
        for (release, lower, upper, expected_lower, expected_upper) in [
            (None, 0.2, 0.2, 34.8, 35.2),
            (Some(7), 34.8, 35.2, 34.8, 35.2),
            (Some(3), 34.975, 34.991, 34.975, 34.991),
            (Some(12), 34.8, 35.2, 34.8, 35.2),
            (None, 35.1, 35.3, 35.1, 35.3),
        ] {
            let (tolerance, losses) = transferred_tolerance(b"Limits", lower, upper, release, &[]);
            let Some(DimensionTolerance::Limits { lower, upper }) = tolerance else {
                panic!("limit tolerance")
            };
            close(lower.value.get(), expected_lower);
            close(upper.value.get(), expected_upper);
            assert_eq!(limit_form_losses(&losses), 1);
        }
    }

    #[test]
    fn symmetric_mode_without_a_magnitude_reads_creo_4_deviations() {
        let (tolerance, losses) = transferred_tolerance(b"+- Symmetric", 0.2, 0.2, Some(4), &[]);
        let Some(DimensionTolerance::PlusMinus { lower, upper }) = tolerance else {
            panic!("plus-minus tolerance")
        };
        close(lower.value.get(), -0.2);
        close(upper.value.get(), 0.2);
        assert_eq!(limit_form_losses(&losses), 0);
    }

    #[test]
    fn unread_limits_charge_no_limit_form() {
        let (tolerance, losses) = transferred_tolerance(
            b"Plus-Minus",
            0.2,
            0.2,
            None,
            &[
                (b"PTC_DIM_UPPER_TOL_VALUE", 0.1),
                (b"PTC_DIM_LOWER_TOL_VALUE", 0.05),
            ],
        );
        let Some(DimensionTolerance::PlusMinus { lower, upper }) = tolerance else {
            panic!("plus-minus tolerance")
        };
        close(lower.value.get(), -0.05);
        close(upper.value.get(), 0.1);
        assert_eq!(limit_form_losses(&losses), 0);
    }

    #[test]
    fn a_selecting_release_overrides_the_nearer_zero_rule() {
        for (release, lower, upper, expected_lower, expected_upper) in
            [(4, 30.0, 40.0, 5.0, 75.0), (10, 0.2, 0.3, 0.2, 0.3)]
        {
            let (tolerance, losses) =
                transferred_tolerance(b"Limits", lower, upper, Some(release), &[]);
            let Some(DimensionTolerance::Limits { lower, upper }) = tolerance else {
                panic!("limit tolerance")
            };
            close(lower.value.get(), expected_lower);
            close(upper.value.get(), expected_upper);
            assert_eq!(limit_form_losses(&losses), 0);
        }
    }

    #[test]
    fn plus_minus_without_a_selecting_release_reads_nearer_zero_limits_as_deviations() {
        let (tolerance, losses) = transferred_tolerance(b"Plus-Minus", 0.2, 0.2, None, &[]);
        let Some(DimensionTolerance::PlusMinus { lower, upper }) = tolerance else {
            panic!("plus-minus tolerance")
        };
        close(lower.value.get(), -0.2);
        close(upper.value.get(), 0.2);
        assert_eq!(limit_form_losses(&losses), 1);
    }

    #[test]
    fn an_invalid_rule_reading_takes_the_other_limit_form() {
        let (tolerance, losses) = transferred_tolerance(b"Limits", -0.3, 0.2, None, &[]);
        let Some(DimensionTolerance::Limits { lower, upper }) = tolerance else {
            panic!("limit tolerance")
        };
        close(lower.value.get(), -0.3);
        close(upper.value.get(), 0.2);
        assert_eq!(limit_form_losses(&losses), 1);
    }
}
