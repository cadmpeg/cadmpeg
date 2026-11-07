// SPDX-License-Identifier: Apache-2.0
//! Dimensional annotations from neutral attributes.

use std::collections::BTreeMap;

use cadmpeg_core::decode::DecodeContext;
use cadmpeg_core::CodecError;
use cadmpeg_ir::document::CadIr;
use cadmpeg_ir::ids::PmiId;
use cadmpeg_ir::pmi::{
    DimensionKind, DimensionTolerance, PmiAnnotation, PmiDefinition, PmiDimension, PmiQuantity,
    PmiValue,
};
use cadmpeg_ir::report::loss::LossNote;
use cadmpeg_ir::{AnnotationBuilder, Exactness};

use crate::loss::CreoLossCode;

use super::attributes::{Attribute, Value};
use super::elements::Element;

const SYMMETRIC_LENGTH_LIMIT_ROUNDING_TOLERANCE_MM: f64 = 1.0e-9;
const SYMMETRIC_ANGLE_LIMIT_ROUNDING_TOLERANCE_RAD: f64 = 1.0e-12;

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
    kind_ambiguous: bool,
    nominal_ambiguous: bool,
    mode_ambiguous: bool,
    symmetric_ambiguous: bool,
    upper_tolerance_ambiguous: bool,
    lower_tolerance_ambiguous: bool,
    upper_limit_ambiguous: bool,
    lower_limit_ambiguous: bool,
    conflicting: bool,
    unit_unresolved: bool,
    invalid: bool,
    offset: Option<usize>,
}

impl DimensionFields<'_> {
    fn tolerance_inputs_ambiguous(&self) -> bool {
        self.mode_ambiguous
            || self.symmetric_ambiguous
            || self.upper_tolerance_ambiguous
            || self.lower_tolerance_ambiguous
            || self.upper_limit_ambiguous
            || self.lower_limit_ambiguous
    }

    fn used_tolerance_inputs_ambiguous(&self) -> bool {
        if self.mode_ambiguous {
            return true;
        }
        let limits_ambiguous = self.upper_limit_ambiguous || self.lower_limit_ambiguous;
        match self.mode {
            Some(b"Limits") => limits_ambiguous,
            Some(b"+- Symmetric") => {
                self.symmetric_ambiguous
                    || (self.tolerance.is_none() && (self.nominal_ambiguous || limits_ambiguous))
            }
            Some(b"Plus-Minus") => {
                self.upper_tolerance_ambiguous
                    || self.lower_tolerance_ambiguous
                    || ((self.upper_tolerance.is_none() || self.lower_tolerance.is_none())
                        && (self.nominal_ambiguous || limits_ambiguous))
            }
            _ => false,
        }
    }
}

struct ProjectedDimension {
    owner: u32,
    owner_type: u32,
    offset: usize,
    definition: PmiDimension,
}

pub(super) fn transfer(
    ctx: &DecodeContext<'_>,
    attributes: &[Option<Attribute<'_>>],
    element_groups: &[Vec<Option<Element>>],
    length_scale_mm: Option<f64>,
    ir: &mut CadIr,
    annotations: &mut AnnotationBuilder,
    losses: &mut Vec<LossNote>,
) -> Result<(), CodecError> {
    let mut fields_by_owner = BTreeMap::<(u32, u32), DimensionFields<'_>>::new();
    let mut unresolved = 0usize;
    for attribute in attributes.iter().flatten() {
        if !dimension_field(attribute.name) {
            continue;
        }
        let (Some(owner), Some(owner_type)) = (attribute.owner, attribute.owner_type) else {
            unresolved += 1;
            continue;
        };
        add_attribute(
            fields_by_owner.entry((owner, owner_type)).or_default(),
            attribute,
        );
    }

    let mut elements_by_owner = BTreeMap::<(u32, u32), Vec<Element>>::new();
    for elements in element_groups {
        for element in elements.iter().flatten() {
            if element.annotation_type == Some(8) {
                elements_by_owner
                    .entry((element.owner, element.owner_type))
                    .or_default()
                    .push(*element);
            }
        }
    }

    let mut projected = Vec::new();
    let mut inferred_linear_kinds = 0usize;
    let mut conflicting = 0usize;
    let mut unresolved_units = 0usize;
    for (owner, fields) in fields_by_owner {
        conflicting += usize::from(fields.conflicting);
        unresolved_units += usize::from(fields.unit_unresolved);
        if fields.kind_ambiguous {
            unresolved += 1;
            continue;
        }
        let Some(kind) = fields.kind else {
            unresolved += 1;
            continue;
        };
        let Some(candidates) = elements_by_owner.get(&owner) else {
            unresolved += 1;
            continue;
        };
        if candidates.len() != 1 {
            unresolved += 1;
            continue;
        }
        if fields.invalid {
            unresolved += 1;
        }
        let element = candidates[0];
        let Some(kind_text) = std::str::from_utf8(kind)
            .ok()
            .filter(|text| !text.is_empty())
        else {
            unresolved += 1;
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
        if quantity == PmiQuantity::Length && length_scale_mm.is_none() && !fields.unit_unresolved {
            unresolved_units += usize::from(
                fields.nominal.is_some()
                    || fields.tolerance.is_some()
                    || fields.upper_tolerance.is_some()
                    || fields.lower_tolerance.is_some()
                    || fields.upper_limit.is_some()
                    || fields.lower_limit.is_some(),
            );
        }
        let units_known =
            !is_unknown_kind && (quantity == PmiQuantity::Angle || length_scale_mm.is_some());
        if !units_known || fields.nominal_ambiguous || fields.tolerance_inputs_ambiguous() {
            unresolved += 1;
        }
        let length_scale = length_scale_mm.unwrap_or(1.0);
        let nominal = match fields
            .nominal
            .filter(|_| units_known && !fields.nominal_ambiguous)
        {
            Some(value) => match value_for(value, quantity, length_scale) {
                Some(value) => Some(value),
                None => {
                    unresolved += 1;
                    continue;
                }
            },
            None => None,
        };
        let (tolerance, unresolved_tolerance) =
            if units_known && !fields.used_tolerance_inputs_ambiguous() {
                tolerance(
                    &fields,
                    (!fields.nominal_ambiguous)
                        .then_some(fields.nominal)
                        .flatten(),
                    quantity,
                    length_scale,
                )
            } else {
                (None, true)
            };
        unresolved += usize::from(unresolved_tolerance);
        let definition = match PmiDimension::new(dimension_kind.clone(), nominal, tolerance) {
            Ok(definition) => definition,
            Err(_) => {
                unresolved += 1;
                let Ok(definition) = PmiDimension::new(dimension_kind, nominal, None) else {
                    continue;
                };
                definition
            }
        };
        let Some(offset) = fields.offset else {
            unresolved += 1;
            continue;
        };
        projected.push(ProjectedDimension {
            owner: element.owner,
            owner_type: element.owner_type,
            offset,
            definition,
        });
        if is_linear_kind_inference {
            inferred_linear_kinds += 1;
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
        ir.model.pmi.push(PmiAnnotation {
            id,
            name: None,
            visible: None,
            targets: Vec::new(),
            definition: PmiDefinition::Dimension(dimension.definition.clone()),
        });
    }

    if unresolved != 0 {
        losses.push(CreoLossCode::PmiRecordUnresolved.note(format!(
            "{unresolved} dimensional value(s) or annotation-element joins are unresolved."
        )));
    }
    if conflicting != 0 {
        losses.push(CreoLossCode::PmiAttributeConflict.note(format!(
            "{conflicting} dimension(s) have conflicting attributes; affected values were withheld."
        )));
    }
    if unresolved_units != 0 {
        losses.push(CreoLossCode::PmiUnitUnresolved.note(format!(
            "{unresolved_units} dimension(s) have unresolved units; affected values were withheld."
        )));
    }
    if !projected.is_empty() {
        losses.push(CreoLossCode::PmiAssociationsUnresolved.note(format!(
            "{} dimensional annotation(s) have unresolved geometric targets and display state.",
            projected.len()
        )));
        losses.push(CreoLossCode::PmiModifiersUnresolved.note(format!(
            "{} dimensional annotation(s) have unresolved modifiers.",
            projected.len()
        )));
    }
    if inferred_linear_kinds != 0 {
        losses.push(CreoLossCode::PmiDimensionKindInferred.note(format!(
            "{inferred_linear_kinds} Creo linear dimension kind(s) use Location as the inferred semantic kind."
        )));
    }
    Ok(())
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
            let conflict = retain_first_text(&mut fields.kind, value);
            fields.kind_ambiguous |= conflict;
            fields.conflicting |= conflict;
            fields.offset.get_or_insert(attribute.offset);
        }
        (b"PTC_TOLERANCE_MODE", Value::Text(value)) => {
            let conflict = retain_first_text(&mut fields.mode, value);
            fields.mode_ambiguous |= conflict;
            fields.conflicting |= conflict;
        }
        (b"PTC_DIM_ANGULARDIM_UNIT", Value::Text(_)) => {}
        (b"PTC_DIM_NOMINAL_VAL", Value::Real(Some(value))) => {
            fields.nominal_ambiguous |= retain_known_real(
                &mut fields.nominal,
                value,
                attribute.unit,
                &mut fields.conflicting,
                &mut fields.unit_unresolved,
            );
        }
        (b"PTC_DIM_TOL_VALUE", Value::Real(Some(value))) => {
            fields.symmetric_ambiguous |= retain_known_real(
                &mut fields.tolerance,
                value,
                attribute.unit,
                &mut fields.conflicting,
                &mut fields.unit_unresolved,
            );
        }
        (b"PTC_DIM_UPPER_TOL_VALUE", Value::Real(Some(value))) => {
            fields.upper_tolerance_ambiguous |= retain_known_real(
                &mut fields.upper_tolerance,
                value,
                attribute.unit,
                &mut fields.conflicting,
                &mut fields.unit_unresolved,
            );
        }
        (b"PTC_DIM_LOWER_TOL_VALUE", Value::Real(Some(value))) => {
            fields.lower_tolerance_ambiguous |= retain_known_real(
                &mut fields.lower_tolerance,
                value,
                attribute.unit,
                &mut fields.conflicting,
                &mut fields.unit_unresolved,
            );
        }
        (b"PTC_DIM_UPPER_LIMIT", Value::Real(Some(value))) => {
            fields.upper_limit_ambiguous |= retain_known_real(
                &mut fields.upper_limit,
                value,
                attribute.unit,
                &mut fields.conflicting,
                &mut fields.unit_unresolved,
            );
        }
        (b"PTC_DIM_LOWER_LIMIT", Value::Real(Some(value))) => {
            fields.lower_limit_ambiguous |= retain_known_real(
                &mut fields.lower_limit,
                value,
                attribute.unit,
                &mut fields.conflicting,
                &mut fields.unit_unresolved,
            );
        }
        _ => {
            fields.invalid = true;
            match attribute.name {
                b"PTC_DIM_TYPE" => fields.kind_ambiguous = true,
                b"PTC_DIM_NOMINAL_VAL" => fields.nominal_ambiguous = true,
                b"PTC_TOLERANCE_MODE" => fields.mode_ambiguous = true,
                b"PTC_DIM_TOL_VALUE" => fields.symmetric_ambiguous = true,
                b"PTC_DIM_UPPER_TOL_VALUE" => fields.upper_tolerance_ambiguous = true,
                b"PTC_DIM_LOWER_TOL_VALUE" => fields.lower_tolerance_ambiguous = true,
                b"PTC_DIM_UPPER_LIMIT" => fields.upper_limit_ambiguous = true,
                b"PTC_DIM_LOWER_LIMIT" => fields.lower_limit_ambiguous = true,
                _ => {}
            }
        }
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

fn retain_known_real(
    slot: &mut Option<f64>,
    value: f64,
    unit: &[u8],
    conflicting: &mut bool,
    unit_unresolved: &mut bool,
) -> bool {
    let conflict = retain_first_real(slot, value);
    *conflicting |= conflict;
    *unit_unresolved |= !unit.is_empty();
    conflict || !unit.is_empty()
}

fn dimension_kind(source: &str) -> DimensionKind {
    match source {
        "Linear" => DimensionKind::Location,
        "Diameter" => DimensionKind::Diameter,
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

fn tolerance(
    fields: &DimensionFields<'_>,
    nominal: Option<f64>,
    quantity: PmiQuantity,
    length_scale: f64,
) -> (Option<DimensionTolerance>, bool) {
    match fields.mode {
        Some(b"Limits") => match (fields.lower_limit, fields.upper_limit) {
            (Some(lower), Some(upper)) => match (
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
                let lower = fields.lower_limit?;
                let upper = fields.upper_limit?;
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
                _ => match (nominal, fields.lower_limit, fields.upper_limit) {
                    (Some(nominal), Some(lower), Some(upper)) => {
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
    use super::{add_attribute, dimension_kind, tolerance, Attribute, DimensionFields, Value};
    use cadmpeg_ir::pmi::{DimensionKind, DimensionTolerance, PmiQuantity, PmiValue};
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
        let (tolerance, unresolved) = tolerance(fields, raw_nominal, quantity, 1.0);
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
        assert!(fields.upper_tolerance_ambiguous);
        let (tolerance, unresolved) = tolerance(&fields, fields.nominal, PmiQuantity::Length, 1.0);
        assert!(tolerance.is_none());
        assert!(unresolved);
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
}
