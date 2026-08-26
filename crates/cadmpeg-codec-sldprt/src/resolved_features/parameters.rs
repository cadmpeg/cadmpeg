//! Design parameter enrichment and scalar synchronisation.

use super::scalars::feature_object_name;
use super::{NAME_MARKER, VALUE_ONLY_SCALAR_HEADER};
use crate::records::{
    FeatureInputLane, FeatureInputName, FeatureInputRelationFamily, FeatureInputScalar,
    FeatureInputScalarRole,
};
use std::collections::{BTreeMap, HashMap, HashSet};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ScalarUnit {
    Native,
    Length,
    Angle,
}

fn scalar_owned_by_feature(
    scalar: &FeatureInputScalar,
    feature: &str,
    start: u64,
    end: u64,
) -> bool {
    scalar.feature_ref.as_deref() == Some(feature)
        || (scalar.feature_ref.is_none() && scalar.offset > start && scalar.offset < end)
}

/// Add unambiguous `ResolvedFeatures` length parameters to a projection copy of history.
pub(crate) fn enrich_history_parameters<'a>(
    histories: &mut [crate::records::FeatureHistory],
    lanes: impl IntoIterator<Item = &'a FeatureInputLane>,
    replace_existing: bool,
) {
    let mut candidates = BTreeMap::<(usize, usize, String), Vec<(f64, ScalarUnit, bool)>>::new();
    for lane in lanes {
        let names_by_id = lane
            .names
            .iter()
            .map(|name| (name.id.as_str(), name))
            .collect::<HashMap<_, _>>();
        let relation_unit = |family| match family {
            FeatureInputRelationFamily::Angle => ScalarUnit::Angle,
            FeatureInputRelationFamily::LineLineDistance
            | FeatureInputRelationFamily::PointPointDistance
            | FeatureInputRelationFamily::PointLineDistance
            | FeatureInputRelationFamily::PointPointHorizontalDistance
            | FeatureInputRelationFamily::PointPointVerticalDistance
            | FeatureInputRelationFamily::CircleDiameter => ScalarUnit::Length,
        };
        let mut scalar_units = lane
            .scalars
            .iter()
            .filter_map(|scalar| {
                let name = names_by_id.get(scalar.name.as_str())?;
                let parameter_class = lane
                    .classes
                    .iter()
                    .filter(|class| class.offset < name.offset)
                    .max_by_key(|class| class.offset)?;
                if lane.names.iter().any(|intervening| {
                    intervening.offset > parameter_class.offset && intervening.offset < name.offset
                }) {
                    return None;
                }
                let unit = match parameter_class.name.as_str() {
                    "moLengthParameter_c" => ScalarUnit::Length,
                    "moAngleParameter_c" => ScalarUnit::Angle,
                    _ => return None,
                };
                Some((scalar.id.as_str(), unit))
            })
            .chain(
                lane.relation_bindings
                    .iter()
                    .map(|binding| (binding.scalar_ref.as_str(), relation_unit(binding.family))),
            )
            .collect::<HashMap<_, _>>();
        for relation in &lane.relation_instances {
            let unit = relation_unit(relation.family);
            for scalar in &relation.scalar_refs {
                scalar_units.insert(scalar.as_str(), unit);
            }
        }
        let mut starts = Vec::<(u64, usize, usize)>::new();
        for (history_index, history) in histories.iter().enumerate() {
            for (feature_index, feature) in history.features.iter().enumerate() {
                let Some(name) = feature_object_name(feature, lane) else {
                    continue;
                };
                starts.push((name.offset, history_index, feature_index));
            }
        }
        starts.sort_by_key(|start| start.0);
        for (index, &(start, history_index, feature_index)) in starts.iter().enumerate() {
            let end = starts.get(index + 1).map_or(u64::MAX, |next| next.0);
            let feature = &histories[history_index].features[feature_index];
            let mut owned = BTreeMap::<&str, Vec<&FeatureInputScalar>>::new();
            for scalar in lane
                .scalars
                .iter()
                .filter(|scalar| scalar_owned_by_feature(scalar, &feature.id, start, end))
            {
                let Some(name) = names_by_id.get(scalar.name.as_str()) else {
                    continue;
                };
                owned.entry(&name.value).or_default().push(scalar);
            }
            for (name, scalars) in owned {
                let driving = scalars
                    .iter()
                    .filter(|scalar| scalar.role == FeatureInputScalarRole::Driving)
                    .copied()
                    .collect::<Vec<_>>();
                let candidates_for_name = if driving.is_empty() {
                    scalars
                        .into_iter()
                        .filter(|scalar| scalar.role == FeatureInputScalarRole::Native)
                        .collect::<Vec<_>>()
                } else {
                    driving
                };
                if let [scalar] = candidates_for_name.as_slice() {
                    let value_only = names_by_id.get(scalar.name.as_str()).is_some_and(|name| {
                        value_only_scalar_offset(&lane.native_payload, name)
                            == usize::try_from(scalar.offset).ok()
                    });
                    let unit = scalar_units
                        .get(scalar.id.as_str())
                        .copied()
                        .or_else(|| scalar_unit_from_feature_parameter(feature, name))
                        .unwrap_or(ScalarUnit::Native);
                    if value_only {
                        continue;
                    }
                    candidates
                        .entry((history_index, feature_index, name.to_string()))
                        .or_default()
                        .push((scalar.value, unit, value_only));
                }
            }
        }
    }

    for ((history_index, feature_index, name), values) in candidates {
        let Some((&(first, unit, value_only), rest)) = values.split_first() else {
            continue;
        };
        if rest
            .iter()
            .any(|(value, candidate_unit, candidate_value_only)| {
                value.to_bits() != first.to_bits()
                    || *candidate_unit != unit
                    || *candidate_value_only != value_only
            })
        {
            continue;
        }
        let feature = &mut histories[history_index].features[feature_index];
        let source_dimension = feature.content.iter().any(|content| {
            matches!(content, crate::records::FeatureContent::Dimension(dimension) if dimension == &name)
        });
        if unit == ScalarUnit::Native && source_dimension && feature.parameters.contains_key(&name)
        {
            continue;
        }
        if unit == ScalarUnit::Native && value_only && feature.parameters.contains_key(&name) {
            continue;
        }
        if unit == ScalarUnit::Native
            && feature.parameters.get(&name).is_some_and(|expression| {
                !native_scalar_matches_discrete_parameter(feature, &name, expression, first)
            })
        {
            continue;
        }
        let expression = match unit {
            ScalarUnit::Native => crate::history::format_native_scalar(
                feature,
                &name,
                first,
                feature.parameters.get(&name).map(String::as_str),
            ),
            ScalarUnit::Length
                if feature.parameters.get(&name).is_some_and(|expression| {
                    crate::history::strip_diameter_modifier(expression).is_some()
                }) =>
            {
                crate::history::format_native_scalar(
                    feature,
                    &name,
                    first,
                    feature.parameters.get(&name).map(String::as_str),
                )
            }
            ScalarUnit::Length => crate::history::format_length_mm(first * 1000.0),
            ScalarUnit::Angle => crate::history::format_angle_rad(first),
        };
        if replace_existing {
            feature.parameters.insert(name, expression);
        } else {
            feature.parameters.entry(name).or_insert(expression);
        }
    }
}

/// Infer a length unit from the owning operation and its native display role.
/// Move Face stores `D1` as distance. A standard fillet placeholder such as
/// `R0` also identifies a radius; variable fillets use indexed radii instead.
fn scalar_unit_from_feature_parameter(
    feature: &crate::records::Feature,
    name: &str,
) -> Option<ScalarUnit> {
    let normalized_kind = feature
        .kind
        .bytes()
        .filter(u8::is_ascii_alphanumeric)
        .map(|byte| byte.to_ascii_lowercase())
        .collect::<Vec<_>>();
    if matches!(name, "D5" | "D6" | "D7") && normalized_kind.as_slice() == b"cutextrudethin" {
        return Some(ScalarUnit::Length);
    }
    if name == "D1"
        && crate::classification::classify(feature)
            == Some(crate::classification::FeatureClass::MoveFace)
        && feature.properties.get("Mode").is_some_and(|mode| {
            mode.eq_ignore_ascii_case("Offset") || mode.eq_ignore_ascii_case("Translate")
        })
    {
        return Some(ScalarUnit::Length);
    }
    let expression = feature.parameters.get(name)?;
    let source_sketch_dimension = crate::classification::classify(feature)
        == Some(crate::classification::FeatureClass::Sketch)
        && feature.content.iter().any(|content| {
            matches!(content, crate::records::FeatureContent::Dimension(dimension) if dimension == name)
        });
    if source_sketch_dimension {
        return if crate::history::parse_angle_rad(expression).is_some() {
            Some(ScalarUnit::Angle)
        } else {
            crate::history::parse_dimension_display_length(expression).map(|_| ScalarUnit::Length)
        };
    }
    if crate::history::fillet_radius_parameter_has_native_display(feature, name, expression) {
        return Some(ScalarUnit::Length);
    }
    None
}

pub(super) fn value_only_scalar_offset(payload: &[u8], name: &FeatureInputName) -> Option<usize> {
    let name_offset = usize::try_from(name.offset).ok()?;
    let header_offset = name_offset
        .checked_add(NAME_MARKER.len() + 1)?
        .checked_add(name.value.encode_utf16().count().checked_mul(2)?)?;
    (payload.get(header_offset..header_offset + VALUE_ONLY_SCALAR_HEADER.len())
        == Some(VALUE_ONLY_SCALAR_HEADER))
    .then_some(header_offset + VALUE_ONLY_SCALAR_HEADER.len())
}

pub(super) fn native_scalar_matches_discrete_parameter(
    feature: &crate::records::Feature,
    name: &str,
    expression: &str,
    value: f64,
) -> bool {
    match crate::history::parse_native_parameter_literal(feature, name, expression) {
        Some(cadmpeg_ir::features::ParameterValue::Integer(expected)) => {
            crate::history::exact_integer_f64(expected) == Some(value)
        }
        Some(cadmpeg_ir::features::ParameterValue::Boolean(expected)) => {
            value == if expected { 1.0 } else { 0.0 }
        }
        _ => true,
    }
}

pub(crate) fn sync_changed_feature_scalars(
    histories: &[crate::records::FeatureHistory],
    lanes: &mut [FeatureInputLane],
    changed: &HashSet<(String, String)>,
) -> Result<(), cadmpeg_core::CodecError> {
    use cadmpeg_ir::features::ParameterValue;

    for lane in lanes {
        let names_by_id = lane
            .names
            .iter()
            .map(|name| (name.id.as_str(), name.value.as_str()))
            .collect::<HashMap<_, _>>();
        let mut starts = histories
            .iter()
            .flat_map(|history| &history.features)
            .filter_map(|feature| {
                feature_object_name(feature, lane).map(|name| (name.offset, feature))
            })
            .collect::<Vec<_>>();
        starts.sort_by_key(|(offset, _)| *offset);
        let mut updates = Vec::<(usize, f64)>::new();
        for (index, &(start, feature)) in starts.iter().enumerate() {
            let end = starts
                .get(index + 1)
                .map_or(u64::MAX, |(offset, _)| *offset);
            for (name, expression) in &feature.parameters {
                if !changed.contains(&(feature.id.clone(), name.clone())) {
                    continue;
                }
                let candidates = lane
                    .scalars
                    .iter()
                    .enumerate()
                    .filter(|(_, scalar)| scalar_owned_by_feature(scalar, &feature.id, start, end))
                    .filter(|(_, scalar)| {
                        names_by_id.get(scalar.name.as_str()) == Some(&name.as_str())
                    })
                    .collect::<Vec<_>>();
                let driving = candidates
                    .iter()
                    .filter(|(_, scalar)| scalar.role == FeatureInputScalarRole::Driving)
                    .copied()
                    .collect::<Vec<_>>();
                let candidates = if driving.is_empty() {
                    candidates
                        .into_iter()
                        .filter(|(_, scalar)| scalar.role == FeatureInputScalarRole::Native)
                        .collect::<Vec<_>>()
                } else {
                    driving
                };
                let [(scalar_index, _)] = candidates.as_slice() else {
                    continue;
                };
                let value =
                    match crate::history::parse_native_parameter_literal(feature, name, expression)
                    {
                        Some(ParameterValue::Length(value)) => value.0 / 1000.0,
                        Some(ParameterValue::Angle(value)) => value.0,
                        Some(ParameterValue::Real(value)) => value,
                        _ => continue,
                    };
                updates.push((*scalar_index, value));
            }
        }
        for (scalar_index, value) in updates {
            let scalar = &mut lane.scalars[scalar_index];
            let offset = usize::try_from(scalar.offset).map_err(|_| {
                cadmpeg_core::CodecError::Malformed(
                    "SLDPRT scalar offset exceeds address space".into(),
                )
            })?;
            let bytes = lane
                .native_payload
                .get_mut(offset..offset + 8)
                .ok_or_else(|| {
                    cadmpeg_core::CodecError::malformed(format_args!(
                        "SLDPRT scalar {} lies outside its payload",
                        scalar.id
                    ))
                })?;
            bytes.copy_from_slice(&value.to_le_bytes());
            scalar.value = value;
        }
    }
    Ok(())
}

#[cfg(test)]
mod parameters_tests;
