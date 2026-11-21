use super::{scope_bytes, Notes};
use crate::container::ContainerScan;
use crate::design::decode::mesh_operations::combine::{CombineReference, MeshCombineScope};
use crate::design::decode::mesh_operations::{MeshOperationRecords, MESH_GROUP_GUID};
use crate::loss::F3dLossCode;
use crate::records::feature::scope::DesignParameterScope;
use cadmpeg_core::decode::DecodeContext;
use cadmpeg_core::text::NonBlankString;
use cadmpeg_core::CodecError;
use cadmpeg_ir::features::{
    BodySelection, BooleanKind, CombineDestination, CombineOperands, Feature, FeatureDefinition,
    FeatureOperation, NativeSelections, SelectionReference, UnresolvedFamily,
};
use cadmpeg_ir::report::loss::LossNote;
use std::collections::HashMap;

pub(in crate::decode) const SOURCE_KIND: &str = "MeshCombine";

fn native(scope: Option<&str>) -> BodySelection {
    scope.map_or(BodySelection::Unresolved, |scope| {
        BodySelection::Native(scope.to_owned())
    })
}

fn reference(notes: &mut Notes<'_>, key: String, value: CombineReference<'_>) {
    match value {
        CombineReference::Null => notes.property(key, "null"),
        CombineReference::Local(index) => notes.property(key, index),
        CombineReference::TypedLocal {
            target,
            inline_type,
        } => {
            notes.property(format!("{key}:inline-type"), inline_type);
            notes.property(key, target);
        }
        CombineReference::Other(bytes) => notes.property(key, format!("{bytes:02x?}").as_str()),
    }
}

fn selection(
    ctx: &DecodeContext<'_>,
    records: &MeshOperationRecords<'_>,
    references: &[CombineReference<'_>],
    complete: bool,
    role: &str,
    scope: &str,
    notes: &mut Notes<'_>,
) -> Result<BodySelection, CodecError> {
    let mut valid = complete && !references.is_empty();
    let mut bodies = Vec::new();
    for (ordinal, value) in references.iter().enumerate() {
        let prefix = format!("mesh-combine:{role}:{ordinal}");
        reference(notes, format!("{prefix}:group"), *value);
        let group = match *value {
            CombineReference::Local(group) => group,
            CombineReference::TypedLocal {
                target,
                inline_type,
            } if inline_type.eq_ignore_ascii_case(MESH_GROUP_GUID) => target,
            _ => {
                valid = false;
                continue;
            }
        };
        let captured = records.group_body_references(ctx, group)?;
        if let Some(count) = captured.declared_count {
            notes.property(format!("{prefix}:member-count"), count);
        }
        for (member, index) in captured.member_references.iter().enumerate() {
            if *index != 0 {
                notes.property(format!("{prefix}:member:{member}"), index);
            }
        }
        for (member, body) in captured.references.iter().enumerate() {
            if *body != 0 {
                notes.property(format!("{prefix}:body:{member}"), body);
                bodies.push(body.to_string());
            }
        }
        if let Some(error) = captured.framing {
            notes.property(format!("{prefix}:framing"), error.to_string().as_str());
            valid = false;
        }
        if captured.references.is_empty() || captured.references.contains(&0) {
            valid = false;
        }
    }
    if valid {
        match NativeSelections::try_from(bodies) {
            Ok(bodies) => {
                notes.losses.push(F3dLossCode::FeatureMeshBodySelectionAssumed.note(format!(
                    "Mesh combination {} interprets {role} member body references as input-local selections; member-tail controls remain unresolved.", notes.source
                )));
                let native = SelectionReference::try_from(scope.to_owned())
                    .map_err(|error| CodecError::malformed(error.to_string()))?;
                return Ok(BodySelection::Local { bodies, native });
            }
            Err(error) => notes.property(
                format!("mesh-combine:{role}:selection-error"),
                error.to_string().as_str(),
            ),
        }
    }
    notes.losses.push(F3dLossCode::FeatureMeshCombineSelectionUnresolved.note(format!(
        "Mesh combination {} retains the ordered {role} group facts while its complete distinct selection remains unresolved.", notes.source
    )));
    Ok(native(Some(scope)))
}

fn project(
    control: Option<u32>,
    target: BodySelection,
    tools: BodySelection,
    scope: Option<&str>,
    notes: &mut Notes<'_>,
) -> Result<FeatureOperation, CodecError> {
    let Some(control @ 0..=3) = control else {
        notes.losses.push(F3dLossCode::FeatureMeshCombineSelectorUnruled.note(format!(
            "Mesh combination {} retains operation selector {control:?}; its neutral operation is unresolved.", notes.source
        )));
        return Ok(FeatureOperation::Unresolved {
            family: UnresolvedFamily::Combine,
        });
    };
    let operands = match CombineOperands::new(target, tools) {
        Ok(operands) => operands,
        Err(error) => {
            notes.losses.push(F3dLossCode::FeatureMeshCombineSelectionUnresolved.note(format!(
                "Mesh combination {} retains its operand facts because selection admission failed: {error}.", notes.source
            )));
            CombineOperands::new(BodySelection::Unresolved, BodySelection::Unresolved)
                .map_err(CodecError::malformed)?
        }
    };
    if control == 3 {
        notes.losses.push(F3dLossCode::FeatureMeshCombineMergeAssumed.note(format!(
            "Mesh combination {} interprets selector 3 as a compound that preserves ordered target and tool structures.", notes.source
        )));
        notes.losses.push(F3dLossCode::FeatureMeshCombineKeepToolsUnresolved.note(format!(
            "Mesh combination {} retains its source controls while compound tool retention remains unresolved.", notes.source
        )));
        notes.losses.push(F3dLossCode::FeatureMeshCombineDestinationUnresolved.note(format!(
            "Mesh combination {} retains its source controls while the compound result component remains unresolved.", notes.source
        )));
        let (target, tools) = operands.into_parts();
        let members = match (target, tools) {
            (
                BodySelection::Local {
                    bodies: target,
                    native,
                },
                BodySelection::Local { bodies: tools, .. },
            ) => {
                match NativeSelections::try_from(
                    target.into_iter().chain(tools).collect::<Vec<_>>(),
                ) {
                    Ok(bodies) => BodySelection::Local { bodies, native },
                    Err(error) => {
                        notes.losses.push(F3dLossCode::FeatureMeshCombineSelectionUnresolved.note(format!(
                            "Mesh combination {} retains its grouped members because compound selection admission failed: {error}.", notes.source
                        )));
                        self::native(scope)
                    }
                }
            }
            _ => native(scope),
        };
        return Ok(FeatureOperation::Compound { members });
    }
    notes.losses.push(
        F3dLossCode::FeatureMeshCombineKeepToolsAssumedFalse.note(format!(
            "Mesh combination {} assumes its Boolean operation consumes the tool bodies.",
            notes.source
        )),
    );
    notes.losses.push(
        F3dLossCode::FeatureMeshCombineDestinationAssumedTargetComponent.note(format!(
            "Mesh combination {} assumes its Boolean result belongs to the target component.",
            notes.source
        )),
    );
    Ok(FeatureOperation::Combine {
        operands,
        op: match control {
            0 => BooleanKind::Join,
            1 => BooleanKind::Cut,
            _ => BooleanKind::Intersect,
        },
        keep_tools: false,
        destination: CombineDestination::TargetComponent,
    })
}

fn captured(
    ctx: &DecodeContext<'_>,
    records: &MeshOperationRecords<'_>,
    parsed: &MeshCombineScope<'_>,
    scope: &str,
    notes: &mut Notes<'_>,
) -> Result<FeatureOperation, CodecError> {
    notes.property("mesh-combine:version", parsed.version);
    if parsed.version == 1 && matches!(parsed.control, Some(0..=2)) {
        notes.losses.push(F3dLossCode::FeatureMeshCombineLegacyBooleanAssumed.note(format!(
            "Mesh combination {} applies the version-2 Boolean selector mapping to its version-1 control {:?}.", notes.source, parsed.control
        )));
    }
    if let Some(flag) = parsed.flag {
        notes.property("mesh-combine:flag", flag);
    }
    if let Some(word) = parsed.prefix_word {
        notes.property("mesh-combine:prefix-word", word);
        notes
            .losses
            .push(F3dLossCode::MeshScopePrefixUninterpreted.note(format!(
                "Mesh combination {} retains prefix word {word}.",
                notes.source
            )));
    }
    if let Some(control) = parsed.control {
        notes.property("mesh-combine:control", control);
    }
    if let Some(word) = parsed.extra_word {
        notes.property("mesh-combine:version-word", word);
    }
    if let Some(byte) = parsed.raw_byte {
        notes.property("mesh-combine:control-byte", byte);
    }
    if let Some(count) = parsed.tool_count {
        notes.property("mesh-combine:tool-count", count);
    }
    if let Some(value) = parsed.output_geometry {
        reference(notes, "mesh-combine:output-geometry".into(), value);
    }
    if let Some(value) = parsed.input_geometry {
        reference(notes, "mesh-combine:input-geometry".into(), value);
    }
    if let Some(word) = parsed.tail_word {
        notes.property("mesh-combine:tail-word", word);
    }
    if parsed.prefix.iter().any(|byte| *byte != 0) {
        notes.property(
            "mesh-combine:prefix-bytes",
            format!("{:02x?}", parsed.prefix).as_str(),
        );
    }
    if parsed.extra_word.is_some()
        || parsed.raw_byte.is_some()
        || parsed.tail_word.is_some_and(|word| word != 0)
        || parsed.prefix.iter().any(|byte| *byte != 0)
    {
        notes.losses.push(F3dLossCode::FeatureMeshCombineFieldsUninterpreted.note(format!(
            "Mesh combination {} retains its version word, control byte and readable fixed-field deviations as source properties.", notes.source
        )));
    }
    if let Some(error) = parsed.framing {
        notes.losses.push(F3dLossCode::FeatureMeshCombineTailPartial.note(format!(
            "Mesh combination {} retains all decoded controls and references before its unresolved {error}.", notes.source
        )));
    }
    let target = selection(
        ctx,
        records,
        parsed.target.as_slice(),
        parsed.target.is_some(),
        "target",
        scope,
        notes,
    )?;
    let complete = parsed
        .tool_count
        .and_then(|count| usize::try_from(count).ok())
        == Some(parsed.tools.len());
    let tools = selection(ctx, records, &parsed.tools, complete, "tool", scope, notes)?;
    notes.losses.push(F3dLossCode::FeatureMeshCombineSourceUnresolved.note(format!(
        "Mesh combination {} retains its source body and geometry references while historical mesh geometry remains unresolved.", notes.source
    )));
    project(parsed.control, target, tools, Some(scope), notes)
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
        if feature.source_tag.as_deref() != Some(SOURCE_KIND) {
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
                Some(records) => Some((records, records.combine_scope(ctx, scope.record_index)?)),
                None => None,
            }
        } else {
            None
        };
        let operation = match parsed {
            Some((records, Ok(parsed))) => captured(ctx, records, &parsed, &source, &mut notes)?,
            other => {
                let reason = match other {
                    Some((_, Err(error))) => error.0,
                    _ => "scope or metadata",
                };
                notes.losses.push(F3dLossCode::FeatureMeshCombineScopeUnparsed.note(format!(
                    "Mesh combination {source} retains its originating feature while its {reason} is unresolved."
                )));
                project(
                    None,
                    BodySelection::Unresolved,
                    BodySelection::Unresolved,
                    None,
                    &mut notes,
                )?
            }
        };
        for (key, value) in notes.properties {
            let key = NonBlankString::new(key)
                .ok_or_else(|| CodecError::malformed("blank mesh combination property key"))?;
            feature.source_properties.insert(key, value);
        }
        losses.append(&mut notes.losses);
        feature
            .evaluation
            .set_definition(FeatureDefinition::Operation(operation));
    }
    Ok(())
}

#[cfg(test)]
mod tests;
