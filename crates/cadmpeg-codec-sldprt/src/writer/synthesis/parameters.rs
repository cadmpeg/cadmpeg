// SPDX-License-Identifier: Apache-2.0
//! The smallest unused parameter records excluded by active operation roles.

use std::collections::{BTreeSet, HashMap};

use cadmpeg_core::decode::DecodeContext;
use cadmpeg_core::CodecError;
use cadmpeg_ir::features::{
    edge_treatments::RadiusSpec, DesignParameter, Dimension, ExpressionToken, FeatureDefinition,
    FeatureOperation, FeatureSourceContent, ParameterValue, RibDraft, ScaleFactors,
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

pub(super) fn omit_unreferenced(
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
                .map(|owner| (parameter.id.as_str(), owner.as_str()))
        })
        .collect::<HashMap<_, _>>();
    let mut owned_positions = BTreeSet::new();
    for feature in &ir.model.features {
        for content in &feature.source_content {
            let FeatureSourceContent::Parameter(id) = content else {
                continue;
            };
            if parameter_owners.get(id.as_str()).copied() == Some(feature.id.as_str()) {
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
    let owners = ir
        .model
        .features
        .iter()
        .map(|feature| (&feature.id, feature.native_ref.as_deref()))
        .collect::<HashMap<_, _>>();
    for parameter in &ir.model.parameters {
        if !omitted.contains(parameter.id.as_str()) {
            continue;
        }
        losses.push(SldprtLossCode::WriterEntityOmitted.note(format!(
            "parameter {} ({}) omitted because its native control conflicts with the current operation on feature {}",
            parameter.id, parameter.name, parameter.owner.as_ref().map_or("", |owner| owner.as_str())
        )));
        if let Some(record_id) = parameter
            .owner
            .as_ref()
            .and_then(|owner| owners.get(owner))
            .copied()
            .flatten()
        {
            for record in native
                .feature_histories
                .iter_mut()
                .flat_map(|history| &mut history.features)
                .filter(|record| record.id == record_id)
            {
                record.parameters.remove(parameter.name.as_str());
                record.dimension_properties.remove(parameter.name.as_str());
                record.content.retain(|content| !matches!(content, FeatureContent::Dimension(name) if name == &parameter.name));
            }
        }
    }
    for feature in &mut ir.model.features {
        remove_positions(feature, &omitted)?;
    }
    ir.model
        .parameters
        .retain(|parameter| !omitted.contains(parameter.id.as_str()));
    Ok(())
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
