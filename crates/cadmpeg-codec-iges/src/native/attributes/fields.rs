// SPDX-License-Identifier: Apache-2.0
//! The semantic fields of each admitted Type 406 form.

use super::{AttributeValue, Fields, NativePropertyValue};
use crate::parameter::TokenValue;
use cadmpeg_ir::native::bytes::NativeBytes;

struct Builder(Fields);

impl Builder {
    fn integer(&mut self, name: &str, value: Option<i64>) {
        if let Some(value) = value {
            self.0
                .push((name.to_owned(), vec![AttributeValue::Integer(value)]));
        }
    }
    fn real(&mut self, name: &str, value: Option<f64>) -> Result<(), &'static str> {
        if let Some(value) = value {
            self.0.push((
                name.to_owned(),
                vec![AttributeValue::float(value).ok_or("property number is not finite")?],
            ));
        }
        Ok(())
    }
    fn boolean(&mut self, name: &str, value: Option<bool>) {
        if let Some(value) = value {
            self.0
                .push((name.to_owned(), vec![AttributeValue::Boolean(value)]));
        }
    }
    fn text(
        &mut self,
        name: &str,
        value: Option<&NativeBytes<Vec<u8>>>,
    ) -> Result<(), &'static str> {
        if let Some(value) = value {
            self.0.push((
                name.to_owned(),
                vec![AttributeValue::String(
                    crate::text::decoded(value)
                        .ok_or("property text is not UTF-8")?
                        .to_owned(),
                )],
            ));
        }
        Ok(())
    }
}

pub(super) fn property(value: &NativePropertyValue) -> Result<Fields, &'static str> {
    use NativePropertyValue::*;
    let mut fields = Builder(Vec::new());
    macro_rules! integers { ($($field:ident),+ $(,)?) => {{ $(fields.integer(stringify!($field), *$field);)+ }}; }
    macro_rules! reals { ($($field:ident),+ $(,)?) => {{ $(fields.real(stringify!($field), *$field)?;)+ }}; }
    macro_rules! booleans { ($($field:ident),+ $(,)?) => {{ $(fields.boolean(stringify!($field), *$field);)+ }}; }
    macro_rules! texts { ($($field:ident),+ $(,)?) => {{ $(fields.text(stringify!($field), $field.as_ref())?;)+ }}; }
    match value {
        RegionRestriction {
            electrical_vias,
            electrical_components,
            electrical_circuitry,
        } => integers!(electrical_vias, electrical_components, electrical_circuitry),
        LevelFunction {
            function_code,
            description,
        } => {
            integers!(function_code);
            texts!(description);
        }
        RegionFill {
            fill_code,
            obsolete_pointer,
        } => integers!(fill_code, obsolete_pointer),
        LineWidening {
            width,
            cornering,
            extension_flag,
            justification,
            extension,
        } => {
            reals!(width, extension);
            integers!(cornering, extension_flag, justification);
        }
        DrilledHole {
            drill_diameter,
            finished_diameter,
            plated,
            lower_layer,
            upper_layer,
        } => {
            reals!(drill_diameter, finished_diameter);
            integers!(plated, lower_layer, upper_layer);
        }
        ReferenceDesignator { value } | PinNumber { value } | Name { value } => texts!(value),
        PartNumber {
            generic,
            military,
            vendor,
            internal,
        } => texts!(generic, military, vendor, internal),
        Hierarchy {
            line_font,
            view,
            level,
            blank,
            line_weight,
            color,
        } => integers!(line_font, view, level, blank, line_weight, color),
        TabularData {
            property_type,
            declared_dependent_count,
            independent_variables,
            dependent_values,
        } => {
            integers!(property_type, declared_dependent_count);
            for (index, variable) in independent_variables.iter().enumerate() {
                fields.integer(
                    &format!("independent_variables.{index}.variable_type"),
                    variable.variable_type,
                );
                fields.integer(
                    &format!("independent_variables.{index}.declared_value_count"),
                    variable.declared_value_count,
                );
                for (offset, value) in variable.values.iter().enumerate() {
                    fields.real(
                        &format!("independent_variables.{index}.values.{offset}"),
                        *value,
                    )?;
                }
            }
            for (index, value) in dependent_values.iter().enumerate() {
                fields.real(&format!("dependent_values.{index}"), *value)?;
            }
        }
        ExternalReferenceFileList { names } => {
            for (index, value) in names.iter().enumerate() {
                fields.text(&format!("names.{index}"), value.as_ref())?;
            }
        }
        NominalSize {
            size,
            name,
            standard,
        } => {
            reals!(size);
            texts!(name, standard);
        }
        FlowLineSpecification { values } => {
            for (index, value) in values.iter().enumerate() {
                fields.text(&format!("values.{index}"), value.as_ref())?;
            }
        }
        IntercharacterSpacing { percent } => reals!(percent),
        LineFont { pattern_code } => integers!(pattern_code),
        Highlight { highlighted } => booleans!(highlighted),
        Pick { pickable } => booleans!(pickable),
        UniformRectangularGrid {
            finite,
            lines,
            weighted,
            origin,
            spacing,
            counts,
        } => {
            booleans!(finite, lines, weighted);
            for (index, value) in origin.iter().enumerate() {
                fields.real(&format!("origin.{index}"), *value)?;
            }
            for (index, value) in spacing.iter().enumerate() {
                fields.real(&format!("spacing.{index}"), *value)?;
            }
            for (index, value) in counts.iter().enumerate() {
                fields.integer(&format!("counts.{index}"), *value);
            }
        }
        AssociativityGroupType {
            associativity_type,
            name,
        } => {
            integers!(associativity_type);
            texts!(name);
        }
        LevelToLepLayerMap { definitions } => {
            for (index, definition) in definitions.iter().enumerate() {
                fields.integer(
                    &format!("definitions.{index}.exchange_level"),
                    definition.exchange_level,
                );
                fields.text(
                    &format!("definitions.{index}.native_identifier"),
                    definition.native_identifier.as_ref(),
                )?;
                fields.integer(
                    &format!("definitions.{index}.physical_layer"),
                    definition.physical_layer,
                );
                fields.text(
                    &format!("definitions.{index}.functional_identifier"),
                    definition.functional_identifier.as_ref(),
                )?;
            }
        }
        LepArtworkStackup {
            identification,
            levels,
        } => {
            texts!(identification);
            for (index, value) in levels.iter().enumerate() {
                fields.integer(&format!("levels.{index}"), *value);
            }
        }
        LepDrilledHole {
            drill_diameter,
            finished_diameter,
            function_code,
        } => {
            reals!(drill_diameter, finished_diameter);
            integers!(function_code);
        }
        GenericData { name: _, values } => {
            let values = values
                .iter()
                .filter_map(|value| match typed_token(&value.value, value.data_type) {
                    Ok(Some(value)) => Some(Ok(value)),
                    Ok(None) => None,
                    Err(reason) => Some(Err(reason)),
                })
                .collect::<Result<Vec<_>, _>>()?;
            fields.0.push((String::new(), values));
        }

        DimensionUnits {
            secondary_position,
            units_indicator,
            character_set,
            suffix,
            fraction_flag,
            precision,
        } => {
            integers!(
                secondary_position,
                units_indicator,
                character_set,
                fraction_flag,
                precision
            );
            texts!(suffix);
        }
        DimensionTolerance {
            secondary_flag,
            tolerance_type,
            placement,
            upper,
            lower,
            suppress_plus,
            fraction_flag,
            precision,
        } => {
            integers!(
                secondary_flag,
                tolerance_type,
                placement,
                fraction_flag,
                precision
            );
            reals!(upper, lower);
            booleans!(suppress_plus);
        }
        DimensionDisplayData {
            dimension_type,
            label_position,
            declared_character_set: _,
            character_set,
            label,
            decimal_symbol,
            declared_witness_line_angle: _,
            witness_line_angle,
            text_alignment,
            text_level,
            text_placement,
            arrow_orientation,
            initial_value,
            supplemental_notes,
        } => {
            // The declared lanes are omission metadata for the effective lanes,
            // not second neutral copies of the same character set and angle.
            integers!(
                dimension_type,
                label_position,
                character_set,
                decimal_symbol,
                text_alignment,
                text_level,
                text_placement,
                arrow_orientation
            );
            texts!(label);
            reals!(witness_line_angle, initial_value);
            for (index, note) in supplemental_notes.iter().enumerate() {
                fields.integer(
                    &format!("supplemental_notes.{index}.position"),
                    note.position,
                );
                fields.integer(
                    &format!("supplemental_notes.{index}.first_text"),
                    note.first_text,
                );
                fields.integer(
                    &format!("supplemental_notes.{index}.last_text"),
                    note.last_text,
                );
            }
        }
        BasicDimension { corners } => {
            for (index, corner) in corners.iter().enumerate() {
                for (coordinate, value) in corner.iter().enumerate() {
                    fields.real(&format!("corners.{index}.{coordinate}"), *value)?;
                }
            }
        }
        DrawingSheetApproval {
            name,
            organization,
            date,
        } => texts!(name, organization, date),
        DrawingSheetId {
            sheet_number,
            revision,
        } => {
            integers!(sheet_number);
            texts!(revision);
        }
        Underscore { ranges } | Overscore { ranges } => {
            for (index, range) in ranges.iter().enumerate() {
                fields.integer(&format!("ranges.{index}.text_index"), range.text_index);
                fields.integer(
                    &format!("ranges.{index}.first_character"),
                    range.first_character,
                );
                fields.integer(
                    &format!("ranges.{index}.last_character"),
                    range.last_character,
                );
            }
        }
        Closure { u, v } => integers!(u, v),
    }
    Ok(fields.0)
}

// Generic Data's NAME is the neutral attribute name, rather than another copy
// of that name inside an unrelated source-field map.
pub(super) fn name(value: &NativePropertyValue) -> Result<String, &'static str> {
    match value {
        NativePropertyValue::GenericData { name, .. } => name
            .as_ref()
            .and_then(crate::text::decoded)
            .filter(|name| !name.trim().is_empty())
            .map(str::to_owned)
            .ok_or("generic attribute has no readable name"),
        _ => Ok(format!("iges.property.{}", super::property_kind(value))),
    }
}

pub(super) fn typed_token(
    value: &TokenValue,
    data_type: Option<i64>,
) -> Result<Option<AttributeValue>, &'static str> {
    match (data_type, value) {
        (Some(0 | 5), TokenValue::Omitted) => Ok(None),
        (Some(1), TokenValue::Integer(value)) => Ok(Some(AttributeValue::Integer(*value))),
        (Some(2), TokenValue::Real(value)) => Ok(Some(AttributeValue::Float(*value))),
        (Some(2), TokenValue::Integer(value)) => Ok(Some(AttributeValue::Float(
            cadmpeg_ir::scalar::FiniteReal::new(
                cadmpeg_core::convert::f64_from_i64(*value)
                    .ok_or("attribute real integer loses precision")?,
            )
            .ok_or("attribute number is not finite")?,
        ))),
        (Some(3), TokenValue::String(bytes)) => Ok(Some(AttributeValue::String(
            std::str::from_utf8(bytes.as_ref())
                .map_err(|_| "attribute text is not UTF-8")?
                .to_owned(),
        ))),
        (Some(4), TokenValue::Integer(value)) => {
            let sequence = u32::try_from(*value)
                .ok()
                .filter(|sequence| sequence % 2 == 1)
                .ok_or("attribute entity pointer has no directory identity")?;
            Ok(Some(AttributeValue::Reference(super::source(sequence))))
        }
        (Some(6), TokenValue::Integer(0)) => Ok(Some(AttributeValue::Boolean(false))),
        (Some(6), TokenValue::Integer(1)) => Ok(Some(AttributeValue::Boolean(true))),
        (_, TokenValue::Omitted) => Ok(None),
        _ => Err("attribute value does not match its declared data type"),
    }
}
