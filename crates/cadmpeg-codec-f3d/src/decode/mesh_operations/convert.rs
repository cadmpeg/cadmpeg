use super::{scope_bytes, Notes};
use crate::container::ContainerScan;
use crate::design::decode::mesh_operations::{MeshOperationFraming, MeshOperationRecords};
use crate::loss::F3dLossCode;
use crate::records::feature::scope::DesignParameterScope;
use cadmpeg_core::decode::DecodeContext;
use cadmpeg_core::text::NonBlankString;
use cadmpeg_core::CodecError;
use cadmpeg_ir::features::mesh::{MeshConversionMethod, MeshSelection};
use cadmpeg_ir::features::{
    Feature, FeatureDefinition, FeatureOperation, NativeSelections, SelectionReference,
};
use cadmpeg_ir::report::loss::LossNote;
use std::collections::HashMap;

fn source_selection(
    ctx: &DecodeContext<'_>,
    records: &MeshOperationRecords<'_>,
    ordinal: usize,
    wrapper: u32,
    notes: &mut Notes<'_>,
) -> Result<MeshSelection, CodecError> {
    notes.property(format!("mesh-conversion-input:{ordinal}"), wrapper);
    let group = records.conversion_input_group(wrapper);
    let bodies = match group {
        Ok(group) => {
            notes.property(format!("mesh-conversion-group:{ordinal}"), group);
            let captured = records.group_body_references(ctx, group)?;
            if let Some(count) = captured.declared_count {
                notes.property(format!("mesh-conversion-member-count:{ordinal}"), count);
            }
            for (member, body) in captured.references.iter().enumerate() {
                if *body != 0 {
                    notes.property(format!("mesh-conversion-body:{ordinal}:{member}"), body);
                }
            }
            match captured.framing {
                Some(error) => Err(error),
                None => Ok(captured.references),
            }
        }
        Err(error) => Err(error),
    };
    let members = match bodies {
        Ok(bodies) => {
            match NativeSelections::try_from(bodies.iter().map(u32::to_string).collect::<Vec<_>>())
            {
                Ok(members) => {
                    notes.losses.push(F3dLossCode::FeatureMeshBodySelectionAssumed.note(format!(
                        "Mesh conversion {} reads input {ordinal} member body references as input-local body selections; member-tail selections remain unresolved.", notes.source
                    )));
                    Some(members)
                }
                Err(error) => {
                    notes.losses.push(F3dLossCode::FeatureMeshConversionSourceUnresolved.note(format!(
                        "Mesh conversion {} retains input {ordinal} body records {bodies:?} because their ordered selection is unresolved: {error}.", notes.source
                    )));
                    None
                }
            }
        }
        Err(error) => {
            notes.losses.push(F3dLossCode::FeatureMeshConversionSourceUnresolved.note(format!(
                "Mesh conversion {} retains input {ordinal} wrapper {wrapper}; its {error} is unresolved.", notes.source
            )));
            None
        }
    };
    notes.losses.push(F3dLossCode::FeatureMeshConversionSourceUnresolved.note(format!(
        "Mesh conversion {} retains input {ordinal} in its wrapper context; the historical source tessellation remains unresolved.", notes.source
    )));
    Ok(MeshSelection::Native {
        native: SelectionReference::try_from(notes.source.to_owned())
            .map_err(CodecError::malformed)?,
        members,
    })
}

pub(in crate::decode) fn bind_features(
    ctx: &DecodeContext<'_>,
    scan: &ContainerScan<'_>,
    features: &mut [Feature],
    scopes: &[DesignParameterScope],
    losses: &mut Vec<LossNote>,
) -> Result<(), CodecError> {
    let mut records = HashMap::<String, Option<MeshOperationRecords<'_>>>::new();
    for feature in features {
        if feature.source_tag.as_deref() != Some("Convert Mesh") {
            continue;
        }
        let mut candidates = scopes
            .iter()
            .filter(|scope| feature.native_ref.as_deref() == Some(scope.id.as_str()));
        let scope = candidates.next().filter(|_| candidates.next().is_none());
        let source = scope.map_or_else(|| feature.id.to_string(), |scope| scope.id.clone());
        let mut notes = Notes {
            source: &source,
            properties: Vec::new(),
            losses: Vec::new(),
        };
        if let FeatureDefinition::Operation(FeatureOperation::Native { parameters, .. }) =
            feature.evaluation.definition()
        {
            for (name, expression) in parameters {
                notes.property(format!("parameter:{name}"), expression);
            }
        }
        let parsed = if let Some((scope, (entry, bytes))) =
            scope.and_then(|scope| Some((scope, scope_bytes(scan, scope)?)))
        {
            let indexed = match records.entry(entry) {
                std::collections::hash_map::Entry::Occupied(entry) => entry.into_mut(),
                std::collections::hash_map::Entry::Vacant(entry) => {
                    let meta = match crate::design::decode::meta::metadata_for_bulk_stream(
                        scan,
                        entry.key(),
                    ) {
                        Ok(meta) => meta,
                        Err(error @ CodecError::ResourceLimit(_)) => return Err(error),
                        Err(_) => None,
                    };
                    entry.insert(meta.map(|meta| MeshOperationRecords::index(bytes, meta)))
                }
            };
            match indexed.as_ref() {
                Some(records) => {
                    Some((records, records.conversion_scope(ctx, scope.record_index)?))
                }
                None => None,
            }
        } else {
            None
        };
        let mut sources = Vec::new();
        let mut method = None;
        match parsed {
            Some((records, Ok(parsed))) => {
                if let Some(control) = parsed.control {
                    notes.property("mesh-conversion-control", control);
                }
                if let Some(count) = parsed.input_count {
                    notes.property("mesh-conversion-input-count", count);
                }
                method = match parsed.control {
                    Some(1) => Some(MeshConversionMethod::Prismatic),
                    Some(3) => Some(MeshConversionMethod::Faceted),
                    _ => None,
                };
                if let Some(method) = method {
                    notes.losses.push(F3dLossCode::FeatureMeshConversionMethodAssumed.note(format!(
                        "Mesh conversion {source} reads its retained conversion control as the {method:?} method."
                    )));
                } else {
                    notes.losses.push(F3dLossCode::FeatureMeshConversionMethodUnresolved.note(format!(
                        "Mesh conversion {source} retains its readable control word in source properties; its conversion method remains unresolved."
                    )));
                }
                if let Some(word) = parsed.prefix_word {
                    notes.property("mesh-scope-prefix-word", word);
                    notes.losses.push(F3dLossCode::MeshScopePrefixUninterpreted.note(format!(
                        "Mesh conversion {source} retains scope prefix word {word} in its source properties."
                    )));
                }
                if let Some(word) = parsed.tail_control {
                    notes.property("mesh-conversion-envelope-word", word);
                }
                if let Some(error) = parsed.tail_error {
                    notes.losses.push(F3dLossCode::FeatureMeshConversionTailPartial.note(format!(
                        "Mesh conversion {source} retains the controls and references before its unresolved {error}."
                    )));
                } else {
                    notes.losses.push(F3dLossCode::FeatureMeshConversionEnvelopeAssumed.note(format!(
                        "Mesh conversion {source} reads its output count with the unique two-byte or six-byte control envelope that ends at the following scope GUID prefix."
                    )));
                }
                for (field, value) in &parsed.deviations {
                    notes.property(format!("mesh-field:{field}"), value);
                }
                if !parsed.deviations.is_empty() {
                    notes.losses.push(F3dLossCode::FeatureMeshOperationFieldsUninterpreted.note(format!(
                        "Mesh conversion {source} retains uninterpreted field values {:?} in its source properties.", parsed.deviations
                    )));
                }
                for (ordinal, wrapper) in parsed.input_wrappers.into_iter().enumerate() {
                    sources.push(source_selection(
                        ctx, records, ordinal, wrapper, &mut notes,
                    )?);
                }
                for (ordinal, wrapper) in parsed.output_wrappers.into_iter().enumerate() {
                    notes.property(format!("mesh-conversion-output:{ordinal}"), wrapper);
                    let reason = match records.conversion_output(wrapper) {
                        Ok(output) => {
                            notes.property(
                                format!("mesh-conversion-output-guid:{ordinal}"),
                                &output.guid,
                            );
                            if let Some(control) = output.control {
                                notes.property(
                                    format!("mesh-conversion-output-control:{ordinal}"),
                                    control,
                                );
                            }
                            if let Some(body) = output.body {
                                notes.property(
                                    format!("mesh-conversion-output-body:{ordinal}"),
                                    body,
                                );
                            }
                            match output.tail_error {
                                Some(error) => format!("its {error} is unresolved"),
                                None => "its body record has no resolved neutral result identity"
                                    .to_owned(),
                            }
                        }
                        Err(error) => format!("its {error} is unresolved"),
                    };
                    notes.losses.push(F3dLossCode::FeatureMeshConversionOutputUnresolved.note(format!(
                        "Mesh conversion {source} retains output wrapper {wrapper} in position {ordinal} because {reason}."
                    )));
                }
            }
            other => {
                let reason = match other {
                    Some((_, Err(error))) => error,
                    _ => MeshOperationFraming("conversion scope or metadata"),
                };
                notes.losses.push(F3dLossCode::FeatureMeshConversionScopeUnparsed.note(format!(
                    "Mesh conversion {source} retains its originating feature because its {reason} is unresolved."
                )));
            }
        }
        if sources.is_empty() {
            sources.push(MeshSelection::Unresolved {
                feature: feature.id.clone(),
            });
            notes.losses.push(F3dLossCode::FeatureMeshConversionSourceUnresolved.note(format!(
                "Mesh conversion {source} retains its originating feature as its unresolved mesh source."
            )));
        }
        for (key, value) in notes.properties {
            let key = NonBlankString::new(key)
                .ok_or_else(|| CodecError::malformed("blank mesh conversion property key"))?;
            feature.source_properties.insert(key, value);
        }
        losses.append(&mut notes.losses);
        feature
            .evaluation
            .set_definition(FeatureDefinition::Operation(
                FeatureOperation::MeshConvert { sources, method },
            ));
    }
    Ok(())
}

#[cfg(test)]
mod tests;
