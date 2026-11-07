// SPDX-License-Identifier: Apache-2.0
//! Declared geometry measures from saved mass-property caches.

use cadmpeg_core::decode::{u64_from_index, DecodeContext};
use cadmpeg_core::CodecError;
use cadmpeg_ir::attributes::{AttributeValue, SourceAttribute};
use cadmpeg_ir::declared_measures::{
    DeclaredMeasure, DeclaredMeasureProvenance, DeclaredQuantity, DeclaredTarget,
};
use cadmpeg_ir::ids::DeclaredMeasureId;
use cadmpeg_ir::scalar::{FiniteReal, NonNegativeReal};
use cadmpeg_ir::CadIr;

use crate::loss::SldprtLossCode;

const PROPERTY_PREFIX: &str = "SW-MassProp-Config-";
const METRES_CUBED_TO_MILLIMETRES_CUBED: f64 = 1_000_000_000.0;
const METRES_SQUARED_TO_MILLIMETRES_SQUARED: f64 = 1_000_000.0;
const SAVED_MASS_PROPERTY_FIELDS: usize = 14;
const SAVED_DECIMAL_PLACES: usize = 6;
const DECIMAL_HALF_UNIT: f64 = 0.5;
const VOLUME_FIELD: usize = 3;
const AREA_FIELD: usize = 4;

struct SavedQuantity {
    value: f64,
    source_error: f64,
}

struct SavedMeasures {
    volume: SavedQuantity,
    area: SavedQuantity,
}

pub(crate) fn append(
    ctx: &DecodeContext<'_>,
    ir: &mut CadIr,
    losses: &mut Vec<cadmpeg_ir::report::loss::LossNote>,
) -> Result<(), CodecError> {
    let has_mass_property = ir.model.attributes.iter().any(|attribute| {
        super::default_properties::is_attribute(attribute)
            && attribute.name.starts_with(PROPERTY_PREFIX)
    });
    if !has_mass_property {
        return Ok(());
    }

    let mut active_configurations = ir
        .model
        .configurations
        .iter()
        .filter(|configuration| configuration.active);
    let Some(configuration) = active_configurations.next() else {
        not_compared(losses, "the active configuration is unresolved");
        return Ok(());
    };
    if active_configurations.next().is_some() {
        not_compared(losses, "the active configuration is ambiguous");
        return Ok(());
    }
    let Some(configuration_index) = configuration.source_index else {
        not_compared(losses, "the active configuration has no source index");
        return Ok(());
    };
    let mut matching = None;
    let mut repeated = false;
    for attribute in ir.model.attributes.iter().filter(|attribute| {
        super::default_properties::is_attribute(attribute)
            && property_configuration_index(&attribute.name) == Some(configuration_index)
    }) {
        if matching.replace(attribute).is_some() {
            repeated = true;
            break;
        }
    }
    if repeated {
        not_compared(
            losses,
            "the active configuration has repeated mass-property caches",
        );
        return Ok(());
    }
    let Some(attribute) = matching else {
        not_compared(
            losses,
            "no mass-property cache matches the active configuration",
        );
        return Ok(());
    };

    let Some(bodies) = configuration
        .bodies
        .as_ref()
        .map(cadmpeg_ir::features::DistinctMembers::as_slice)
    else {
        not_compared(losses, "the active configuration body set is unresolved");
        return Ok(());
    };
    let [body_id] = bodies else {
        not_compared(
            losses,
            "the active configuration does not have exactly one body",
        );
        return Ok(());
    };
    if ir.model.bodies.len() != 1 || ir.model.bodies[0].id != *body_id {
        not_compared(
            losses,
            "the active configuration body does not resolve uniquely",
        );
        return Ok(());
    }
    if ir.model.bodies[0].visible == Some(false) {
        not_compared(losses, "the active configuration body is hidden");
        return Ok(());
    }

    let Some(saved) = saved_measures(attribute) else {
        not_compared(
            losses,
            "the active configuration cache has unsupported values",
        );
        return Ok(());
    };

    let target = DeclaredTarget::Bodies {
        bodies: vec![body_id.clone()],
    };
    let property_name = attribute.name.as_str();
    let native_ref = attribute.id.to_string();
    let volume_id = DeclaredMeasureId::mint(format!(
        "sldprt:data:declared_measure#saved-mass-property-config-{configuration_index}-volume"
    ))
    .map_err(CodecError::malformed)?;
    let area_id = DeclaredMeasureId::mint(format!(
        "sldprt:data:declared_measure#saved-mass-property-config-{configuration_index}-area"
    ))
    .map_err(CodecError::malformed)?;
    let volume_value = FiniteReal::new(saved.volume.value).ok_or_else(|| {
        CodecError::malformed("SolidWorks cached volume is not finite after unit conversion")
    })?;
    let volume_error = NonNegativeReal::new(saved.volume.source_error)
        .ok_or_else(|| CodecError::malformed("SolidWorks cached volume precision is not finite"))?;
    let area_value = FiniteReal::new(saved.area.value).ok_or_else(|| {
        CodecError::malformed("SolidWorks cached area is not finite after unit conversion")
    })?;
    let area_error = NonNegativeReal::new(saved.area.source_error)
        .ok_or_else(|| CodecError::malformed("SolidWorks cached area precision is not finite"))?;
    let volume_label = format!("Saved mass-property cache: {property_name} volume");
    let area_label = format!("Saved mass-property cache: {property_name} area");
    let native_ref_bytes = native_ref
        .len()
        .checked_mul(2)
        .ok_or_else(|| CodecError::malformed("SolidWorks native reference size overflow"))?;
    let measure_bytes = std::mem::size_of::<DeclaredMeasure>()
        .checked_mul(2)
        .ok_or_else(|| CodecError::malformed("SolidWorks declared measure size overflow"))?;
    let retained = [
        volume_id.as_str().len(),
        area_id.as_str().len(),
        native_ref_bytes,
        volume_label.len(),
        area_label.len(),
        measure_bytes,
    ]
    .into_iter()
    .try_fold(0usize, usize::checked_add)
    .ok_or_else(|| CodecError::malformed("SolidWorks mass-property allocation size overflow"))?;
    ctx.retain_bytes(
        u64_from_index(retained),
        "retain SolidWorks declared measures",
    )?;
    ctx.charge_entities(2, "admit SolidWorks declared measures")?;
    ir.model.declared_measures.extend([
        DeclaredMeasure::new(
            volume_id,
            target.clone(),
            DeclaredQuantity::Volume {
                value: volume_value,
            },
        )
        .with_provenance(DeclaredMeasureProvenance::SavedCache)
        .with_source_error(volume_error)
        .with_label(volume_label)
        .with_native_ref(native_ref.clone()),
        DeclaredMeasure::new(
            area_id,
            target,
            DeclaredQuantity::SolidArea { value: area_value },
        )
        .with_provenance(DeclaredMeasureProvenance::SavedCache)
        .with_source_error(area_error)
        .with_label(area_label)
        .with_native_ref(native_ref),
    ]);
    Ok(())
}

fn property_configuration_index(name: &str) -> Option<u32> {
    let digits = name.strip_prefix(PROPERTY_PREFIX)?;
    if digits.is_empty()
        || !digits.bytes().all(|byte| byte.is_ascii_digit())
        || (digits.len() > 1 && digits.starts_with('0'))
    {
        return None;
    }
    digits.parse().ok()
}

fn saved_measures(attribute: &SourceAttribute) -> Option<SavedMeasures> {
    let [AttributeValue::String(value)] = attribute.values.as_slice() else {
        return None;
    };
    let mut count = 0usize;
    let mut volume = None;
    let mut area = None;
    for field in value.split(',') {
        if count == SAVED_MASS_PROPERTY_FIELDS {
            return None;
        }
        let parsed = decimal_value_and_error(field)?;
        match count {
            VOLUME_FIELD => volume = Some(parsed),
            AREA_FIELD => area = Some(parsed),
            _ => {}
        }
        count += 1;
    }
    if count != SAVED_MASS_PROPERTY_FIELDS {
        return None;
    }
    let (volume, volume_error) = volume?;
    let (area, area_error) = area?;
    if volume <= 0.0 || area <= 0.0 {
        return None;
    }
    let volume = volume * METRES_CUBED_TO_MILLIMETRES_CUBED;
    let volume_error = volume_error * METRES_CUBED_TO_MILLIMETRES_CUBED;
    let area = area * METRES_SQUARED_TO_MILLIMETRES_SQUARED;
    let area_error = area_error * METRES_SQUARED_TO_MILLIMETRES_SQUARED;
    if [volume, volume_error, area, area_error]
        .into_iter()
        .any(|value| !value.is_finite())
        || volume_error == 0.0
        || area_error == 0.0
    {
        return None;
    }
    Some(SavedMeasures {
        volume: SavedQuantity {
            value: volume,
            source_error: volume_error,
        },
        area: SavedQuantity {
            value: area,
            source_error: area_error,
        },
    })
}

fn decimal_value_and_error(text: &str) -> Option<(f64, f64)> {
    let text = text.trim();
    let unsigned = text.strip_prefix('-').unwrap_or(text);
    let (whole, fractional) = unsigned.split_once('.')?;
    if whole.is_empty()
        || !whole.bytes().all(|byte| byte.is_ascii_digit())
        || fractional.len() != SAVED_DECIMAL_PLACES
        || fractional.bytes().any(|byte| !byte.is_ascii_digit())
    {
        return None;
    }
    let value = text.parse::<f64>().ok()?;
    let decimal_places = i32::try_from(fractional.len()).ok()?;
    let error = DECIMAL_HALF_UNIT * 10_f64.powi(-decimal_places);
    (value.is_finite() && error.is_finite() && error > 0.0).then_some((value, error))
}

fn not_compared(losses: &mut Vec<cadmpeg_ir::report::loss::LossNote>, reason: &str) {
    losses.push(
        SldprtLossCode::MetadataMassPropertiesNotCompared.note(format!(
            "SolidWorks saved mass properties are not compared: {reason}."
        )),
    );
}

#[cfg(test)]
mod tests {
    use super::{decimal_value_and_error, property_configuration_index, saved_measures};
    use cadmpeg_ir::attributes::{AttributeTarget, AttributeValue, SourceAttribute};
    use cadmpeg_ir::ids::AttributeId;

    fn assert_approx(actual: f64, expected: f64, tolerance: f64) {
        assert!(
            (actual - expected).abs() <= tolerance,
            "{actual} differs from {expected} by more than {tolerance}"
        );
    }

    fn attribute(name: &str, value: &str) -> SourceAttribute {
        SourceAttribute {
            id: AttributeId::mint("sldprt:metadata:default_property#1:18:SW-MassProp-Config-0")
                .expect("attribute id"),
            target: AttributeTarget::Document,
            name: name.to_owned(),
            values: vec![AttributeValue::String(value.to_owned())],
        }
    }

    #[test]
    fn saved_mass_property_precision_uses_each_serialized_decimal() {
        let parsed = decimal_value_and_error(" 0.003325 ").expect("decimal");
        assert_approx(parsed.0, 0.003_325, 1.0e-15);
        assert_approx(parsed.1, 0.000_000_5, 1.0e-18);
        assert!(decimal_value_and_error("0.25").is_none());
        assert!(decimal_value_and_error("0.2500000").is_none());
        assert!(decimal_value_and_error("+.250000").is_none());
        assert!(decimal_value_and_error(".250000").is_none());
        let property = attribute(
            "SW-MassProp-Config-0",
            "0.000000, 0.000000, 0.000000, 0.000123, 0.000234, 0.000345, 0.000456, 0.000567, 0.000678, 0.000789, 0.000890, 0.000901, 1.000000, 0.000000",
        );
        let measures = saved_measures(&property).expect("mass-property values");
        assert_approx(measures.volume.value, 123_000.0, 1.0e-9);
        assert_approx(measures.volume.source_error, 500.0, 1.0e-9);
        assert_approx(measures.area.value, 234.0, 1.0e-12);
        assert_approx(measures.area.source_error, 0.5, 1.0e-12);
    }

    #[test]
    fn saved_mass_property_parser_requires_the_serialized_field_count() {
        let too_many = ["1.000000"; 15].join(",");
        let property = attribute("SW-MassProp-Config-0", &too_many);
        assert!(saved_measures(&property).is_none());

        let property = attribute(
            "SW-MassProp-Config-0",
            "0.000000,0.000000,0.000000,1.000000,1.000000,1.000000,1.000000,1.000000,1.000000,1.000000,1.000000,1.000000,1.000000",
        );
        assert!(saved_measures(&property).is_none());
    }

    #[test]
    fn only_canonical_configuration_property_names_resolve() {
        assert_eq!(
            property_configuration_index("SW-MassProp-Config-0"),
            Some(0)
        );
        assert_eq!(
            property_configuration_index("SW-MassProp-Config-12"),
            Some(12)
        );
        assert_eq!(property_configuration_index("SW-MassProp-Config-"), None);
        assert_eq!(property_configuration_index("SW-MassProp-Config-00"), None);
        assert_eq!(property_configuration_index("SW-MassProp-Config-+0"), None);
        assert_eq!(
            property_configuration_index("SW-MassProp-Config-1-extra"),
            None
        );
    }
}
