//! Design parameter enrichment and scalar synchronisation.

use super::scalars::FeatureObjectNames;
use super::{NAME_MARKER, VALUE_ONLY_SCALAR_HEADER};
use crate::records::{
    FeatureInputClass, FeatureInputLane, FeatureInputName, FeatureInputRelationFamily,
    FeatureInputScalar, FeatureInputScalarRole,
};
use std::collections::{BTreeMap, HashMap};

/// Resolve a dimensional binding through the current canonical parameter slice.
pub(crate) fn dimension_value<Q: cadmpeg_ir::features::DimensionValue>(
    dimension: &cadmpeg_ir::features::Dimension<Q>,
    parameters: &[cadmpeg_ir::features::DesignParameter],
) -> Option<Q> {
    match dimension {
        cadmpeg_ir::features::Dimension::Literal(value) => Some(value.clone()),
        cadmpeg_ir::features::Dimension::Parameter(id) => parameters
            .iter()
            .find(|parameter| parameter.id == *id)
            .and_then(Q::resolve_parameter),
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ScalarUnit {
    Native,
    Length,
    Angle,
}

/// A lane's scalars by owner. A scalar naming a feature belongs to it; an
/// unreferenced scalar belongs to the object whose open span holds its offset.
struct ScalarOwners<'a> {
    referenced: HashMap<&'a str, Vec<usize>>,
    unreferenced: Vec<(u32, usize)>,
}

impl<'a> ScalarOwners<'a> {
    fn new(scalars: &'a [FeatureInputScalar]) -> Self {
        let mut referenced = HashMap::<_, Vec<_>>::new();
        let mut unreferenced = Vec::new();
        for (index, scalar) in scalars.iter().enumerate() {
            match scalar.feature_ref.as_deref() {
                Some(feature) => referenced.entry(feature).or_default().push(index),
                None => unreferenced.push((scalar.sequence, index)),
            }
        }
        unreferenced.sort_unstable();
        Self {
            referenced,
            unreferenced,
        }
    }

    /// Indices, in scalar order, of the scalars `feature` owns over the span
    /// after `start` and before `end`.
    fn owned(&self, feature: &str, start: u32, end: Option<u32>) -> Vec<usize> {
        let first = self
            .unreferenced
            .partition_point(|&(offset, _)| offset <= start);
        let last = end.map_or(self.unreferenced.len(), |end| {
            self.unreferenced
                .partition_point(|&(offset, _)| offset < end)
        });
        let mut owned = self.referenced.get(feature).cloned().unwrap_or_default();
        owned.extend(
            self.unreferenced
                .get(first..last)
                .into_iter()
                .flatten()
                .map(|&(_, index)| index),
        );
        owned.sort_unstable();
        owned
    }
}

/// A lane's class declarations and name records in offset order.
struct DeclaredParameters<'a> {
    classes: Vec<&'a FeatureInputClass>,
    name_offsets: Vec<u32>,
}

impl<'a> DeclaredParameters<'a> {
    fn new(lane: &'a FeatureInputLane) -> Self {
        let mut classes = lane.classes.iter().collect::<Vec<_>>();
        classes.sort_by_key(|class| class.sequence);
        let mut name_offsets = lane
            .names
            .iter()
            .map(|name| name.sequence)
            .collect::<Vec<_>>();
        name_offsets.sort_unstable();
        Self {
            classes,
            name_offsets,
        }
    }

    /// The last class declared before `name`, the later of two at one
    /// offset, when no other name lies between them.
    fn class_of(&self, name: &FeatureInputName) -> Option<&'a FeatureInputClass> {
        let before = self
            .classes
            .partition_point(|class| class.sequence < name.sequence);
        let class = *self.classes.get(before.checked_sub(1)?)?;
        let after_class = self
            .name_offsets
            .partition_point(|&offset| offset <= class.sequence);
        let before_name = self
            .name_offsets
            .partition_point(|&offset| offset < name.sequence);
        (after_class >= before_name).then_some(class)
    }
}

/// Add unambiguous `ResolvedFeatures` length parameters to a projection copy of history.
pub(crate) fn enrich_history_parameters<'a>(
    histories: &mut [crate::records::FeatureHistory],
    lanes: impl IntoIterator<Item = &'a FeatureInputLane>,
    replace_existing: bool,
) {
    let mut candidates = BTreeMap::<(usize, usize, String), Vec<(f64, ScalarUnit)>>::new();
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
        let declared = DeclaredParameters::new(lane);
        let units = lane
            .scalars
            .iter()
            .filter_map(|scalar| {
                let name = names_by_id.get(scalar.name.as_str())?;
                let parameter_class = declared.class_of(name)?;
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
            );
        let mut scalar_units = units.collect::<HashMap<_, _>>();
        for relation in &lane.relation_instances {
            let unit = relation_unit(relation.family);
            for scalar in relation.scalar_refs() {
                scalar_units.insert(scalar.as_str(), unit);
            }
        }
        let object_names = FeatureObjectNames::new(&lane.names);
        let mut starts = Vec::<(u32, usize, usize)>::new();
        for (history_index, history) in histories.iter().enumerate() {
            for (feature_index, feature) in history.features.iter().enumerate() {
                let Some(name) = object_names.of(feature) else {
                    continue;
                };
                starts.push((name.sequence, history_index, feature_index));
            }
        }
        starts.sort_by_key(|left| left.0);
        let owners = ScalarOwners::new(&lane.scalars);
        let metadata_ids = super::bindings::history_metadata_ids(histories);
        let mut object_starts = histories
            .iter()
            .flat_map(|history| &history.features)
            .filter(|feature| !metadata_ids.contains(feature.id.as_str()))
            .filter_map(|feature| {
                let offset = usize::try_from(object_names.of(feature)?.offset?).ok()?;
                Some((offset, feature))
            })
            .collect::<Vec<_>>();
        object_starts.sort_unstable_by_key(|(offset, _)| *offset);
        let mut pattern_values = Vec::new();
        for (index, &(start, history_index, feature_index)) in starts.iter().enumerate() {
            let end = starts.get(index + 1).map(|next| next.0);
            let feature = &histories[history_index].features[feature_index];
            if feature.input_class.as_deref() == Some("moLPattern_c") {
                let object_start = object_names
                    .of(feature)
                    .and_then(|name| name.offset)
                    .and_then(|offset| usize::try_from(offset).ok());
                let object_end = object_starts
                    .iter()
                    .position(|(_, candidate)| candidate.id == feature.id)
                    .map_or(lane.native_payload.len(), |position| {
                        super::bindings::pattern_object_end(
                            object_starts.iter().skip(position + 1).copied(),
                            lane.native_payload.len(),
                        )
                    });
                if let Some(dimensions) = object_start.and_then(|start| {
                    super::axes::typed_linear_pattern_dimensions(feature, lane, start, object_end)
                }) {
                    for (name, default, length) in [
                        (
                            dimensions.spacing_name,
                            crate::history::literals::format_length_mm(dimensions.spacing.into()),
                            true,
                        ),
                        (dimensions.count_name, dimensions.count.to_string(), false),
                    ] {
                        let cache = feature
                            .dimension_properties
                            .get(name)
                            .and_then(|properties| properties.get("Value"));
                        let typed = match cache {
                            None => Some(default),
                            Some(cache) if length => {
                                crate::history::literals::parse_positive_dimension_length_mm(cache)
                                    .map(|value| {
                                        crate::history::literals::format_length_mm(value.into())
                                    })
                            }
                            Some(cache) => cache
                                .trim()
                                .parse::<u32>()
                                .ok()
                                .filter(|count| *count > 0)
                                .map(|count| count.to_string()),
                        };
                        if let Some(typed) = typed {
                            pattern_values.push((
                                history_index,
                                feature_index,
                                name.to_owned(),
                                typed,
                            ));
                        }
                    }
                }
            }
            let mut owned = BTreeMap::<&str, Vec<&FeatureInputScalar>>::new();
            for scalar in owners
                .owned(&feature.id, start, end)
                .into_iter()
                .filter_map(|index| lane.scalars.get(index))
            {
                let Some(name) = names_by_id.get(scalar.name.as_str()) else {
                    continue;
                };
                owned.entry(name.value.as_str()).or_default().push(scalar);
            }
            for (name, scalars) in owned {
                let driving = scalars
                    .iter()
                    .copied()
                    .filter(|scalar| scalar.role == FeatureInputScalarRole::Driving)
                    .collect::<Vec<_>>();
                let candidates_for_name = if driving.is_empty() {
                    scalars
                        .into_iter()
                        .filter(|scalar| scalar.role == FeatureInputScalarRole::Native)
                        .collect()
                } else {
                    driving
                };
                if let [scalar] = candidates_for_name.as_slice() {
                    let value_only = names_by_id.get(scalar.name.as_str()).is_some_and(|name| {
                        value_only_scalar_offset(&lane.native_payload, name).is_some_and(
                            |value_offset| {
                                scalar
                                    .offset
                                    .and_then(|offset| usize::try_from(offset).ok())
                                    == Some(value_offset)
                            },
                        )
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
                        .entry((history_index, feature_index, name.to_owned()))
                        .or_default()
                        .push((scalar.value.get(), unit));
                }
            }
        }
        for (history, feature, name, value) in pattern_values {
            histories[history].features[feature]
                .dimension_properties
                .entry(name)
                .or_default()
                .insert(cadmpeg_core::nonblank_literal!("Value"), value);
        }
    }

    for ((history_index, feature_index, name), values) in candidates {
        let Some((&(first, unit), rest)) = values.split_first() else {
            continue;
        };
        if rest.iter().any(|(value, candidate_unit)| {
            value.to_bits() != first.to_bits() || *candidate_unit != unit
        }) {
            continue;
        }
        let feature = &mut histories[history_index].features[feature_index];
        let source_dimension = feature.content.iter().any(|content| {
            matches!(content, crate::records::FeatureContent::Dimension(dimension) if dimension == &name)
        });
        if unit == ScalarUnit::Native
            && source_dimension
            && feature.parameters.contains_key(name.as_str())
        {
            continue;
        }
        if unit == ScalarUnit::Native
            && feature
                .parameters
                .get(name.as_str())
                .is_some_and(|expression| {
                    !native_scalar_matches_discrete_parameter(feature, &name, expression, first)
                })
        {
            continue;
        }
        let expression = match unit {
            ScalarUnit::Native => crate::history::parameters::format_native_scalar(
                feature,
                &name,
                first,
                feature.parameters.get(name.as_str()).map(String::as_str),
            ),
            ScalarUnit::Length
                if feature
                    .parameters
                    .get(name.as_str())
                    .is_some_and(|expression| {
                        crate::history::literals::strip_diameter_modifier(expression).is_some()
                    }) =>
            {
                crate::history::parameters::format_native_scalar(
                    feature,
                    &name,
                    first,
                    feature.parameters.get(name.as_str()).map(String::as_str),
                )
            }
            ScalarUnit::Length => cadmpeg_ir::scalar::Length::new(first * 1000.0)
                .map(crate::history::literals::format_length_mm),
            ScalarUnit::Angle => cadmpeg_ir::scalar::Angle::new(first)
                .map(crate::history::literals::format_angle_rad),
        };
        let Some(expression) = expression else {
            continue;
        };
        let Some(name) = cadmpeg_core::text::NonBlankString::new(name) else {
            continue;
        };
        if replace_existing {
            if let Some(properties) = feature.dimension_properties.get_mut(name.as_str()) {
                if properties.contains_key("Value") {
                    properties.insert(cadmpeg_core::nonblank_literal!("Value"), expression.clone());
                }
            }
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
    if matches!(name, "D5" | "D6" | "D7")
        && crate::history::classify::matches_alnum_ascii(&feature.kind, b"cutextrudethin")
    {
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
        return if crate::history::literals::parse_angle_rad(expression).is_some() {
            Some(ScalarUnit::Angle)
        } else {
            crate::history::literals::parse_dimension_display_length(expression)
                .map(|_| ScalarUnit::Length)
        };
    }
    if crate::history::project::modify::fillet_radius_parameter_has_native_display(
        feature, name, expression,
    ) {
        return Some(ScalarUnit::Length);
    }
    None
}

pub(super) fn value_only_scalar_offset(payload: &[u8], name: &FeatureInputName) -> Option<usize> {
    let name_offset = name
        .offset
        .and_then(|offset| usize::try_from(offset).ok())?;
    let header_offset = name_offset
        .checked_add(NAME_MARKER.len() + 1)?
        .checked_add(name.value.encode_utf16().count().checked_mul(2)?)?;
    (payload.get(header_offset..header_offset + VALUE_ONLY_SCALAR_HEADER.len())
        == Some(VALUE_ONLY_SCALAR_HEADER))
    .then_some(header_offset + VALUE_ONLY_SCALAR_HEADER.len())
}

fn native_scalar_matches_discrete_parameter(
    feature: &crate::records::Feature,
    name: &str,
    expression: &str,
    value: f64,
) -> bool {
    match crate::history::parameters::parse_native_parameter_literal(feature, name, expression) {
        Some(cadmpeg_ir::features::ParameterValue::Integer(expected)) => {
            crate::history::parameters::eval::exact_integer_f64(expected) == Some(value)
        }
        Some(cadmpeg_ir::features::ParameterValue::Boolean(expected)) => {
            value == if expected { 1.0 } else { 0.0 }
        }
        _ => true,
    }
}

pub(crate) fn sync_feature_scalars(
    histories: &[crate::records::FeatureHistory],
    lanes: &mut [FeatureInputLane],
) -> Result<(), cadmpeg_core::CodecError> {
    use cadmpeg_ir::features::ParameterValue;

    for lane in lanes {
        let names_by_id = lane
            .names
            .iter()
            .map(|name| (name.id.as_str(), name.value.as_str()))
            .collect::<HashMap<_, _>>();
        let object_names = FeatureObjectNames::new(&lane.names);
        let mut starts = histories
            .iter()
            .flat_map(|history| &history.features)
            .filter_map(|feature| {
                object_names
                    .of(feature)
                    .map(|name| (name.sequence, feature))
            })
            .collect::<Vec<_>>();
        starts.sort_by_key(|(left, _)| *left);
        let owners = ScalarOwners::new(&lane.scalars);
        let mut updates = Vec::<(usize, f64)>::new();
        for (index, &(start, feature)) in starts.iter().enumerate() {
            let end = starts.get(index + 1).map(|(offset, _)| *offset);
            let mut owned = None;
            for (name, expression) in &feature.parameters {
                let candidates = owned
                    .get_or_insert_with(|| owners.owned(&feature.id, start, end))
                    .iter()
                    .filter_map(|&index| Some((index, lane.scalars.get(index)?)))
                    .filter(|(_, scalar)| {
                        names_by_id.get(scalar.name.as_str()) == Some(&name.as_str())
                    })
                    .collect::<Vec<_>>();
                let value = match crate::history::parameters::parse_native_parameter_literal(
                    feature,
                    name.as_str(),
                    expression,
                ) {
                    Some(ParameterValue::Length(value)) => value.get() / 1000.0,
                    Some(ParameterValue::Angle(value)) => value.get(),
                    Some(ParameterValue::Real(value)) => value.get(),
                    _ => continue,
                };
                updates.extend(
                    candidates
                        .into_iter()
                        .map(|(scalar_index, _)| (scalar_index, value)),
                );
            }
        }
        for (scalar_index, value) in updates {
            let scalar = &mut lane.scalars[scalar_index];
            let checked = cadmpeg_ir::scalar::FiniteReal::new(value).ok_or_else(|| {
                cadmpeg_core::CodecError::malformed("SLDPRT scalar update is non-finite")
            })?;
            if !lane.native_payload.is_empty() {
                let offset = scalar
                    .offset
                    .and_then(|offset| usize::try_from(offset).ok())
                    .ok_or_else(|| {
                        cadmpeg_core::CodecError::malformed(
                            "SLDPRT scalar offset exceeds address space",
                        )
                    })?;
                let end = offset.checked_add(8).ok_or_else(|| {
                    cadmpeg_core::CodecError::malformed("SLDPRT scalar extent overflows")
                })?;
                let bytes = lane.native_payload.get_mut(offset..end).ok_or_else(|| {
                    cadmpeg_core::CodecError::malformed(format_args!(
                        "SLDPRT scalar {} lies outside its payload",
                        scalar.id
                    ))
                })?;
                bytes.copy_from_slice(&value.to_le_bytes());
            }
            scalar.value = checked;
        }
    }
    Ok(())
}

#[cfg(test)]
mod parameters_tests;
