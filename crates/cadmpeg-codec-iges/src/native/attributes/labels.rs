// SPDX-License-Identifier: Apache-2.0
//! Directory labels and carrier name properties have editable neutral homes.

use super::{
    project_fields, AttributeTarget, AttributeValue, BTreeMap, CadIr, CodecError, DecodeContext,
    IgesLossCode, LossNote, NativeProperty, NativePropertyValue,
};

fn carrier(target: &AttributeTarget) -> bool {
    matches!(
        target,
        AttributeTarget::Point(_) | AttributeTarget::Curve(_) | AttributeTarget::Surface(_)
    )
}

pub(super) fn project(
    ir: &mut CadIr,
    entities: &mut [super::super::NativeEntity<'_>],
    properties: &mut [NativeProperty],
    targets: &BTreeMap<String, Vec<AttributeTarget>>,
    ctx: &DecodeContext<'_>,
) -> Result<Vec<LossNote>, CodecError> {
    let mut losses = Vec::new();
    for property in properties.iter_mut().filter(|property| property.form == 15) {
        let owners: Vec<_> = property
            .owners
            .iter()
            .filter(|owner| {
                targets
                    .get(*owner)
                    .is_some_and(|targets| targets.iter().any(carrier))
            })
            .cloned()
            .collect();
        if owners.is_empty() {
            continue;
        }
        let NativePropertyValue::Name { value: Some(value) } = &property.value else {
            continue;
        };
        let Some(value) = crate::text::decoded(value) else {
            losses.push(
                IgesLossCode::NeutralProjectionUnavailable
                    .note(format!("{}: carrier name is not UTF-8", property.id)),
            );
            continue;
        };
        property.neutral_values = Some(project_fields(
            ir,
            &property.source_entity,
            &owners,
            "name",
            vec![(
                String::new(),
                vec![AttributeValue::String(value.to_owned())],
            )],
            targets,
            ctx,
        )?);
    }
    for entity in entities {
        let Some(assigned) = targets.get(&entity.id) else {
            continue;
        };
        let assigned: Vec<_> = assigned
            .iter()
            .filter(|target| carrier(target) || matches!(target, AttributeTarget::Body(_)))
            .cloned()
            .collect();
        if assigned.is_empty() {
            continue;
        }
        let Some(label) = crate::text::decoded(&entity.label) else {
            losses.push(
                IgesLossCode::NeutralProjectionUnavailable
                    .note(format!("{}: directory label is not UTF-8", entity.id)),
            );
            continue;
        };
        let label = label.trim_matches(' ');
        if label.is_empty() {
            continue;
        }
        // A full Name property may coexist with a distinct short directory
        // label. Preserve that independent label rather than overwrite either.
        let current: Vec<_> = ir
            .model
            .attributes
            .iter()
            .filter(|attribute| attribute.name == "name" && assigned.contains(&attribute.target))
            .collect();
        if !current.is_empty()
            && current
                .iter()
                .all(|attribute| attribute.values == [AttributeValue::String(label.to_owned())])
        {
            entity.neutral_label = current
                .iter()
                .map(|attribute| attribute.id.clone())
                .collect();
        } else {
            let name = if current.is_empty() && assigned.iter().all(carrier) {
                "name"
            } else {
                "iges.directory_label"
            };
            let label_targets = BTreeMap::from([(entity.id.clone(), assigned)]);
            let values = project_fields(
                ir,
                &entity.id,
                std::slice::from_ref(&entity.id),
                name,
                vec![(
                    String::new(),
                    vec![AttributeValue::String(label.to_owned())],
                )],
                &label_targets,
                ctx,
            )?;
            entity.neutral_label = values.into_values().flatten().collect();
        }
    }
    Ok(losses)
}
