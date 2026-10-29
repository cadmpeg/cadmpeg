// SPDX-License-Identifier: Apache-2.0
//! Unused parameters that conflict with their operation move to the global owner.

use std::collections::{BTreeSet, HashMap};

use cadmpeg_core::decode::DecodeContext;
use cadmpeg_core::CodecError;
use cadmpeg_ir::features::{
    edge_treatments::RadiusSpec, DesignParameter, Dimension, ExpressionToken, FeatureDefinition,
    FeatureId, FeatureOperation, FeatureSourceContent, ParameterValue, RibDraft, ScaleFactors,
};
use cadmpeg_ir::report::loss::LossNote;
use cadmpeg_ir::schema::EntitySchema;
use cadmpeg_ir::CadIr;

use crate::history::classify::indexed_name;
use crate::loss::SldprtLossCode;
use crate::native::SldprtNative;
use crate::records::FeatureContent;

fn candidates(ir: &CadIr) -> BTreeSet<String> {
    let definitions = ir
        .model
        .features
        .iter()
        .map(|feature| (&feature.id, feature.evaluation.definition()))
        .collect::<HashMap<_, _>>();
    ir.model
        .parameters
        .iter()
        .filter(|parameter| {
            parameter
                .owner
                .as_ref()
                .and_then(|owner| definitions.get(owner))
                .is_some_and(|definition| conflicts(parameter, definition))
        })
        .map(|parameter| parameter.id.as_str().to_owned())
        .collect()
}

pub(in crate::writer) fn has_conflict(ir: &CadIr) -> bool {
    !candidates(ir).is_empty()
}

fn unbound<Q>(dimension: &Dimension<Q>, parameter: &DesignParameter) -> bool {
    !matches!(dimension, Dimension::Parameter(id) if id == &parameter.id)
}

fn conflicts(parameter: &DesignParameter, definition: &FeatureDefinition) -> bool {
    let name = parameter.name.as_str();
    match definition {
        FeatureDefinition::Operation(FeatureOperation::Scale { factors, .. }) => match factors {
            ScaleFactors::Uniform { factor } => {
                matches!(name, "ScaleX" | "ScaleY" | "ScaleZ")
                    || (name == "Factor" && unbound(factor, parameter))
            }
            ScaleFactors::PerAxis { factors } => {
                name == "Factor"
                    || ["ScaleX", "ScaleY", "ScaleZ"]
                        .into_iter()
                        .zip(factors)
                        .any(|(role, dimension)| name == role && unbound(dimension, parameter))
            }
            ScaleFactors::Unresolved {} => false,
        },
        FeatureDefinition::Operation(FeatureOperation::KnitSurface { gap_tolerance, .. }) => {
            name == "GapTolerance"
                && gap_tolerance
                    .as_ref()
                    .is_none_or(|dimension| unbound(dimension, parameter))
        }
        FeatureDefinition::Operation(FeatureOperation::Rib { construction, .. }) => {
            (name == "Draft"
                && match &construction.draft {
                    RibDraft::None => true,
                    RibDraft::Angle(dimension) => unbound(dimension, parameter),
                    RibDraft::Unresolved => false,
                })
                || (name == "Thickness"
                    && construction
                        .thickness
                        .as_ref()
                        .is_some_and(|dimension| unbound(dimension, parameter)))
        }
        FeatureDefinition::Operation(FeatureOperation::Fillet { groups }) => {
            match groups.as_slice() {
                [group] => match &group.radius {
                    RadiusSpec::Constant { radius } => {
                        indexed_name(name, "Position")
                            || indexed_name(name, "Radius")
                            || (name == "Radius" && unbound(radius, parameter))
                    }
                    RadiusSpec::Variable { points } => {
                        matches!(name, "Radius" | "D1")
                            || ["Radius", "Position"].into_iter().any(|prefix| {
                                indexed_name(name, prefix)
                                    && name
                                        .strip_prefix(prefix)
                                        .and_then(|suffix| suffix.parse::<usize>().ok())
                                        .is_some_and(|index| index >= points.as_slice().len())
                            })
                            || points.as_slice().iter().enumerate().any(|(index, point)| {
                                (name == format!("Radius{index}")
                                    && unbound(&point.radius, parameter))
                                    || (name == format!("Position{index}") && {
                                        let expected = ParameterValue::Real(point.parameter.into());
                                        parameter.expression.literal_value_as(&expected).as_ref()
                                            != Some(&expected)
                                    })
                            })
                    }
                    _ => false,
                },
                _ => false,
            }
        }
        _ => false,
    }
}

pub(super) fn move_unreferenced(
    ctx: &DecodeContext<'_>,
    ir: &mut CadIr,
    native: &mut SldprtNative,
    losses: &mut Vec<LossNote>,
) -> Result<(), CodecError> {
    let mut omitted = candidates(ir);
    if omitted.is_empty() {
        return Ok(());
    }
    let mut referenced =
        ir.model
            .parameters
            .iter()
            .filter(|parameter| {
                parameter.expression.tokens.iter().any(
                    |token| matches!(token, ExpressionToken::Parameter(id) if id == &parameter.id),
                )
            })
            .map(|parameter| parameter.id.as_str().to_owned())
            .collect::<BTreeSet<_>>();
    let parameter_owners = ir
        .model
        .parameters
        .iter()
        .filter(|parameter| omitted.contains(parameter.id.as_str()))
        .filter_map(|parameter| {
            parameter
                .owner
                .as_ref()
                .map(|owner| (parameter.id.as_str().to_owned(), owner.as_str().to_owned()))
        })
        .collect::<HashMap<_, _>>();
    let mut owned_positions = BTreeSet::new();
    for feature in &ir.model.features {
        for content in &feature.source_content {
            let FeatureSourceContent::Parameter(id) = content else {
                continue;
            };
            if parameter_owners.get(id.as_str()).map(String::as_str) == Some(feature.id.as_str()) {
                owned_positions.insert((feature.id.as_str(), id.as_str()));
            }
        }
    }
    ir.visit_neutral_references(ctx, &mut |owner, target| {
        if !omitted.contains(target) {
            return Ok(());
        }
        let owned_position = owned_positions.contains(&(owner, target));
        if !owned_position {
            referenced.insert(target.to_owned());
        }
        Ok(())
    })?;
    for feature in &ir.model.features {
        if feature.source_content.iter().any(|content| {
            matches!(content,
            FeatureSourceContent::Parameter(id) if omitted.contains(id.as_str()))
        }) {
            let mut owner = feature.clone();
            remove_positions(&mut owner, &omitted)?;
            owner.visit_references(ctx, &mut |target| {
                if omitted.contains(target) {
                    referenced.insert(target.to_owned());
                }
                Ok(())
            })?;
        }
    }
    omitted.retain(|id| !referenced.contains(id));
    if omitted.is_empty() {
        return Ok(());
    }
    let moved = move_to_global_owner(ir, &omitted, losses)?;
    let records = ir
        .model
        .features
        .iter()
        .filter_map(|feature| {
            feature
                .native_ref
                .as_deref()
                .map(|record| (feature.id.as_str(), record))
        })
        .collect::<HashMap<_, _>>();
    for parameter in &ir.model.parameters {
        if !omitted.contains(parameter.id.as_str()) {
            continue;
        }
        let former = parameter_owners
            .get(parameter.id.as_str())
            .map_or("", String::as_str);
        let name = match moved.get(parameter.id.as_str()) {
            Some(name) => name.as_str(),
            None => {
                losses.push(SldprtLossCode::WriterEntityOmitted.note(format!(
                    "parameter {} ({}) omitted because its native control conflicts with the current operation on feature {former} and no unique global parameter name can be formed",
                    parameter.id, parameter.name
                )));
                parameter.name.as_str()
            }
        };
        let Some(record_id) = records.get(former).copied() else {
            continue;
        };
        for record in native
            .feature_histories
            .iter_mut()
            .flat_map(|history| &mut history.features)
            .filter(|record| record.id == record_id)
        {
            record.parameters.remove(name);
            record.dimension_properties.remove(name);
            record.content.retain(
                |content| !matches!(content, FeatureContent::Dimension(held) if held == name),
            );
        }
    }
    for feature in &mut ir.model.features {
        remove_positions(feature, &omitted)?;
    }
    ir.model.parameters.retain(|parameter| {
        !omitted.contains(parameter.id.as_str()) || moved.contains_key(parameter.id.as_str())
    });
    Ok(())
}

/// Move each conflicting unused parameter to the generated global owner,
/// with a name that no other global parameter holds. Returns the former
/// name of each moved parameter, keyed by parameter id.
fn move_to_global_owner(
    ir: &mut CadIr,
    candidates: &BTreeSet<String>,
    losses: &mut Vec<LossNote>,
) -> Result<HashMap<String, String>, CodecError> {
    let existing = super::design::find_generated_global_owner(ir);
    let global_owners = crate::history::parameters::global_parameter_owners(&ir.model.features);
    let is_global = |owner: Option<&FeatureId>| {
        owner.is_none_or(|owner| existing.as_ref() == Some(owner) || global_owners.contains(owner))
    };
    let mut names = ir
        .model
        .parameters
        .iter()
        .filter(|parameter| {
            !candidates.contains(parameter.id.as_str()) && is_global(parameter.owner.as_ref())
        })
        .map(|parameter| parameter.name.clone())
        .collect::<BTreeSet<_>>();
    let mut next_ordinal = ir
        .model
        .parameters
        .iter()
        .filter(|parameter| {
            !candidates.contains(parameter.id.as_str())
                && (parameter.owner.is_none() || parameter.owner == existing)
        })
        .try_fold(0_u32, |next, parameter| {
            parameter
                .ordinal
                .checked_add(1)
                .map(|after| next.max(after))
        });
    let mut planned = Vec::new();
    for (index, parameter) in ir.model.parameters.iter().enumerate() {
        if !candidates.contains(parameter.id.as_str()) {
            continue;
        }
        let Some(former) = &parameter.owner else {
            continue;
        };
        let name = if names.contains(&parameter.name) {
            ir.model
                .features
                .iter()
                .find(|feature| &feature.id == former)
                .and_then(|feature| feature.name.as_deref())
                .map(|owner| format!("{}@{owner}", parameter.name))
                .filter(|name| !names.contains(name))
        } else {
            Some(parameter.name.clone())
        };
        let (Some(name), Some(ordinal)) = (name, next_ordinal) else {
            continue;
        };
        next_ordinal = ordinal.checked_add(1);
        names.insert(name.clone());
        planned.push((index, former.clone(), name, ordinal));
    }
    let mut moved = HashMap::new();
    if planned.is_empty() {
        return Ok(moved);
    }
    let global = super::design::generated_global_owner(ir)?;
    for (index, former, name, ordinal) in planned {
        let parameter = &mut ir.model.parameters[index];
        losses.push(SldprtLossCode::WriterParameterMovedToGlobal.note(format!(
            "parameter {} ({}) moved from feature {former} to the generated Global parameters owner {global} as {name}: its native control conflicts with the current operation and nothing references it",
            parameter.id, parameter.name
        )));
        moved.insert(
            parameter.id.as_str().to_owned(),
            std::mem::replace(&mut parameter.name, name),
        );
        parameter.owner = Some(global.clone());
        parameter.ordinal = ordinal;
    }
    Ok(moved)
}

fn remove_positions(
    feature: &mut cadmpeg_ir::features::Feature,
    removed: &BTreeSet<String>,
) -> Result<(), CodecError> {
    if feature.source_content.iter().any(|content| {
        matches!(content,
        FeatureSourceContent::Parameter(id) if removed.contains(id.as_str()))
    }) {
        feature.source_content = feature.source_content.iter().filter(|content| {
            !matches!(content, FeatureSourceContent::Parameter(id) if removed.contains(id.as_str()))
        }).cloned().collect::<Vec<_>>().try_into().map_err(CodecError::malformed)?;
    }
    Ok(())
}

#[cfg(test)]
mod tests;
