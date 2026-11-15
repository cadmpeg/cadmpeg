use super::{scope_bytes, Notes};
use crate::container::ContainerScan;
use crate::design::decode::mesh_operations::separate::{
    Run, SeparateScope, Slot, SlotValue, SourceDetails,
};
use crate::design::decode::mesh_operations::{MeshOperationFraming, MeshOperationRecords};
use crate::layout::mesh_separate_common_envelope as envelope;
use crate::loss::F3dLossCode;
use crate::records::feature::scope::DesignParameterScope;
use cadmpeg_core::decode::{u64_from_index, DecodeContext, View};
use cadmpeg_core::text::NonBlankString;
use cadmpeg_core::CodecError;
use cadmpeg_ir::features::mesh::{
    MeshModification, MeshSelection, MeshSeparation, MeshSeparationGroups, MeshSeparationMethod,
};
use cadmpeg_ir::features::{
    Feature, FeatureDefinition, FeatureOperation, NativeSelections, SelectionReference,
};
use cadmpeg_ir::native::bytes::NativeBytes;
use cadmpeg_ir::report::loss::LossNote;
use std::collections::HashMap;

fn raw(
    ctx: &DecodeContext<'_>,
    notes: &mut Notes<'_>,
    key: impl Into<String>,
    bytes: &[u8],
) -> Result<(), CodecError> {
    let size = bytes
        .len()
        .checked_mul(2)
        .ok_or_else(|| CodecError::malformed("Separate provenance extent overflow"))?;
    ctx.retain_bytes(u64_from_index(size), "F3D Separate byte provenance")?;
    notes.property(key, NativeBytes::new(bytes));
    Ok(())
}

fn slot(
    ctx: &DecodeContext<'_>,
    notes: &mut Notes<'_>,
    key: &str,
    value: Slot<'_>,
) -> Result<(), CodecError> {
    notes.property(format!("{key}:offset"), value.offset);
    raw(ctx, notes, format!("{key}:raw"), value.raw)?;
    let error = match value.value {
        SlotValue::Null => {
            notes.property(format!("{key}:state"), "null");
            None
        }
        SlotValue::Present {
            target,
            inline_type,
            registration,
        } => {
            notes.property(format!("{key}:state"), "present");
            notes.property(format!("{key}:target"), target);
            if let Some(guid) = inline_type {
                notes.property(format!("{key}:inline-type"), guid);
            }
            match registration {
                Ok(ordinal) => {
                    notes.property(format!("{key}:registration"), ordinal);
                    None
                }
                Err(error) => Some(error),
            }
        }
        SlotValue::Invalid(error) => {
            notes.property(format!("{key}:state"), "invalid");
            Some(error)
        }
        SlotValue::Unread(error) => {
            notes.property(format!("{key}:state"), "unread");
            Some(error)
        }
    };
    if let Some(error) = error {
        notes.property(format!("{key}:error"), error.to_string().as_str());
        notes.losses.push(
            F3dLossCode::FeatureMeshSeparateReferenceUnresolved.note(format!(
                "Mesh separation {} retains {key} at {} with unresolved {error}.",
                notes.source, value.offset
            )),
        );
    }
    Ok(())
}

fn run(
    ctx: &DecodeContext<'_>,
    notes: &mut Notes<'_>,
    key: &str,
    value: &Run<'_>,
) -> Result<(), CodecError> {
    if let Some(count) = value.declared {
        notes.property(format!("{key}:count"), count);
    }
    for (ordinal, value) in value.slots.iter().enumerate() {
        slot(ctx, notes, &format!("{key}:{ordinal}"), *value)?;
    }
    if let Some((ordinal, offset, count)) = value.unread {
        notes.property(format!("{key}:unread-from"), ordinal);
        notes.property(format!("{key}:unread-offset"), offset);
        notes.property(format!("{key}:unread-count"), count);
    }
    if let Some(error) = value.framing {
        notes.property(format!("{key}:framing"), error.to_string().as_str());
    }
    Ok(())
}

fn settings(parsed: &SeparateScope<'_>, notes: &mut Notes<'_>) -> MeshSeparation {
    let mut separation = MeshSeparation::default();
    if let Some(selector) = parsed.selector {
        notes.property("mesh-separate-selector", selector);
        let method_loss = match selector {
            0 => {
                separation.method = Some(MeshSeparationMethod::Shells);
                Some(F3dLossCode::FeatureMeshSeparateShellsMethodAssumed)
            }
            0x0001_0000 | 0x0002_0000 => {
                separation.method = Some(MeshSeparationMethod::FaceGroups);
                Some(F3dLossCode::FeatureMeshSeparateFaceGroupsMethodAssumed)
            }
            _ => None,
        };
        let code = method_loss.unwrap_or(F3dLossCode::FeatureMeshSeparateSelectorUnruled);
        notes.losses.push(code.note(format!(
            "Mesh separation {} provisionally reads selector {selector} as method {:?}.",
            notes.source, separation.method
        )));
        let group_loss = match selector {
            0x0001_0000 => {
                separation.groups = Some(MeshSeparationGroups::All {});
                Some(F3dLossCode::FeatureMeshSeparateAllGroupsAssumed)
            }
            0x0002_0000 => {
                separation.groups = Some(MeshSeparationGroups::Selected { members: None });
                Some(F3dLossCode::FeatureMeshSeparateSelectedGroupsAssumed)
            }
            _ => None,
        };
        if let Some(code) = group_loss {
            notes.losses.push(code.note(format!("Mesh separation {} provisionally reads selector {selector} as group scope {:?}; selected group identities remain unresolved.", notes.source, separation.groups)));
        }
    }
    let mut lane = View::over_retained(parsed.lane);
    for (ordinal, (target, carrier, polarity)) in [
        (
            &mut separation.multiple_bodies,
            F3dLossCode::FeatureMeshSeparateMultipleBodiesCarrierAssumed,
            F3dLossCode::FeatureMeshSeparateMultipleBodiesPolarityAssumed,
        ),
        (
            &mut separation.keep_body,
            F3dLossCode::FeatureMeshSeparateKeepBodyCarrierAssumed,
            F3dLossCode::FeatureMeshSeparateKeepBodyPolarityAssumed,
        ),
    ]
    .into_iter()
    .enumerate()
    {
        if let Some(value) = lane.u32_le() {
            notes.property(format!("mesh-separate-lane-word:{ordinal}"), value);
            notes.losses.push(carrier.note(format!("Mesh separation {} provisionally assigns raw lane word {ordinal} to this independent body control.", notes.source)));
            *target = match value {
                0 => Some(false),
                1 => Some(true),
                _ => None,
            };
            let code = if target.is_some() {
                polarity
            } else {
                F3dLossCode::FeatureMeshSeparateSelectorUnruled
            };
            notes.losses.push(code.note(format!("Mesh separation {} provisionally reads lane word {ordinal} value {value} with 0=false and 1=true; neutral value is {target:?}.", notes.source)));
        }
    }
    separation
}

fn details(
    ctx: &DecodeContext<'_>,
    notes: &mut Notes<'_>,
    key: &str,
    value: &SourceDetails<'_>,
) -> Result<(), CodecError> {
    raw(ctx, notes, format!("{key}:raw"), value.frame.window())?;
    run(ctx, notes, &format!("{key}:members"), &value.run)?;
    for (ordinal, reference) in value.references.iter().enumerate() {
        slot(
            ctx,
            notes,
            &format!("{key}:reference:{ordinal}"),
            *reference,
        )?;
    }
    for (ordinal, word) in value.words.iter().enumerate() {
        notes.property(format!("{key}:word:{ordinal}"), word);
    }
    for (ordinal, identifier) in value.identifiers.iter().enumerate() {
        raw(
            ctx,
            notes,
            format!("{key}:identifier:{ordinal}:raw"),
            identifier,
        )?;
        match ctx.utf16le_text(identifier, identifier.len() / 2, false, "F3D Separate native identifier") {
            Ok(text) => {
                let key = format!("{key}:identifier:{ordinal}");
                ctx.retain_bytes(u64_from_index(text.len()), "F3D Separate identifier text")?;
                notes.property(key, text.as_str());
            },
            Err(error @ CodecError::ResourceLimit(_)) => return Err(error),
            Err(_) => notes.losses.push(F3dLossCode::FeatureMeshSeparateSourceFieldsUninterpreted.note(format!("Mesh separation {} retains invalid UTF-16 identifier {key}:{ordinal} as bytes.", notes.source))),
        }
    }
    if let Some(role) = value.role {
        notes.property(format!("{key}:role"), role);
    }
    if let Some(error) = value.framing {
        notes.property(format!("{key}:framing"), error.to_string().as_str());
        notes.losses.push(
            F3dLossCode::FeatureMeshSeparateSourceFieldsUninterpreted.note(format!(
                "Mesh separation {} retains the readable {key} fields before {error}.",
                notes.source
            )),
        );
    }
    Ok(())
}

fn project_source(
    ctx: &DecodeContext<'_>,
    records: &MeshOperationRecords<'_>,
    parsed: &SeparateScope<'_>,
    notes: &mut Notes<'_>,
) -> Result<Option<NativeSelections>, CodecError> {
    let Some(group) = parsed.source.and_then(Slot::admitted) else {
        return Ok(None);
    };
    let source = match records.separate_source(ctx, group)? {
        Ok(source) => source,
        Err(error) => {
            notes.property("mesh-separate-source-error", error.to_string().as_str());
            return Ok(None);
        }
    };
    details(ctx, notes, "mesh-separate-group", &source.group)?;
    if let Some(member) = parsed.common.slots.get(1).and_then(|slot| slot.target()) {
        if source.group.run.framing.is_none()
            && !source
                .group
                .run
                .slots
                .iter()
                .any(|slot| slot.target() == Some(member))
        {
            notes.losses.push(F3dLossCode::FeatureMeshSeparateReferenceMismatch.note(format!("Mesh separation {} retains common-table member {member} outside its source-group member run.", notes.source)));
        }
    }
    for (ordinal, member) in source.members.iter().enumerate() {
        match member {
            Some(Ok(member)) => details(
                ctx,
                notes,
                &format!("mesh-separate-member:{ordinal}"),
                member,
            )?,
            Some(Err(error)) => notes.property(
                format!("mesh-separate-member:{ordinal}:error"),
                error.to_string().as_str(),
            ),
            None => (),
        }
    }
    notes.losses.push(F3dLossCode::FeatureMeshSeparateSourceFieldsUninterpreted.note(format!("Mesh separation {} retains group and member auxiliary references, native identifiers, words and roles in source properties.", notes.source)));
    if let Some(error) = source.membership.framing {
        notes.property(
            "mesh-separate-membership-framing",
            error.to_string().as_str(),
        );
        return Ok(None);
    }
    let values = source
        .membership
        .references
        .iter()
        .map(u32::to_string)
        .collect::<Vec<_>>();
    match NativeSelections::try_from(values) {
        Ok(members) => {
            notes.losses.push(F3dLossCode::FeatureMeshSeparateSourceMembershipAssumed.note(format!("Mesh separation {} provisionally interprets its complete ordered member body references as input-local mesh membership.", notes.source)));
            Ok(Some(members))
        }
        Err(error) => {
            notes.property("mesh-separate-membership-error", error.to_string().as_str());
            Ok(None)
        }
    }
}

pub(in crate::decode) fn bind_features(
    ctx: &DecodeContext<'_>,
    scan: &ContainerScan<'_>,
    features: &mut [Feature],
    scopes: &[DesignParameterScope],
    losses: &mut Vec<LossNote>,
) -> Result<(), CodecError> {
    let mut cache = HashMap::<String, Option<MeshOperationRecords<'_>>>::new();
    for feature in features {
        if feature.source_tag.as_deref() != Some("MeshSeparate")
            || matches!(
                feature.evaluation.definition().operation(),
                FeatureOperation::MeshModify {
                    modification: MeshModification::Separate { .. },
                    ..
                }
            )
        {
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
        let definition = match feature.evaluation.definition() {
            FeatureDefinition::Operation(operation)
            | FeatureDefinition::PostProcess { operation, .. } => operation,
        };
        if let FeatureOperation::Native { parameters, .. } = definition {
            for (name, expression) in parameters {
                let key = format!("parameter:{name}");
                ctx.retain_bytes(
                    u64_from_index(expression.len()),
                    "F3D Separate parameter text",
                )?;
                notes.property(key, expression);
            }
        }
        let parsed = if let Some((scope, (entry, bytes))) =
            scope.and_then(|scope| Some((scope, scope_bytes(scan, scope)?)))
        {
            notes.property("mesh-separate-scope-record", scope.record_index);
            let indexed = match cache.entry(entry) {
                std::collections::hash_map::Entry::Occupied(entry) => entry.into_mut(),
                std::collections::hash_map::Entry::Vacant(entry) => {
                    let meta_name = entry
                        .key()
                        .strip_suffix("BulkStream.dat")
                        .map(|prefix| format!("{prefix}MetaStream.dat"));
                    let unique = scan
                        .entries
                        .iter()
                        .filter(|candidate| candidate.name == *entry.key())
                        .count()
                        == 1
                        && meta_name.is_some_and(|name| {
                            scan.entries
                                .iter()
                                .filter(|candidate| candidate.name == name)
                                .count()
                                == 1
                        });
                    let meta = if unique {
                        match crate::design::decode::meta::metadata_for_bulk_stream(
                            scan,
                            entry.key(),
                        ) {
                            Ok(meta) => meta,
                            Err(error @ CodecError::ResourceLimit(_)) => return Err(error),
                            Err(_) => None,
                        }
                    } else {
                        None
                    };
                    entry.insert(meta.map(|meta| MeshOperationRecords::index(bytes, meta)))
                }
            };
            match indexed.as_ref() {
                Some(records) => Some((records, records.separate_scope(ctx, scope.record_index)?)),
                None => None,
            }
        } else {
            None
        };
        let mut separation = MeshSeparation::default();
        let mut members = None;
        match parsed {
            Some((records, Ok(parsed))) => {
                raw(
                    ctx,
                    &mut notes,
                    "mesh-separate-scope-raw",
                    parsed.frame.window(),
                )?;
                if let Some(flag) = parsed.flag {
                    notes.property("mesh-separate-flag", flag);
                }
                if parsed.flag == Some(1) {
                    notes.losses.push(F3dLossCode::FeatureMeshSeparatePrefixAssumed.note(format!("Mesh separation {source} provisionally applies the flagged mesh-family prefix form.")));
                }
                if let Some(word) = parsed.prefix_word {
                    notes.property("mesh-separate-prefix-word", word);
                    notes.losses.push(F3dLossCode::MeshScopePrefixUninterpreted.note(format!("Mesh separation {source} retains prefix word {word} with unresolved meaning.")));
                }
                run(ctx, &mut notes, "mesh-separate-output", &parsed.outputs)?;
                if parsed.outputs.declared.is_some() {
                    notes.losses.push(F3dLossCode::FeatureMeshSeparateOutputRoleAssumed.note(format!("Mesh separation {source} provisionally reads the counted reference run as native output bodies in stored order.")));
                    notes.losses.push(F3dLossCode::FeatureMeshSeparateOutputUnresolved.note(format!("Mesh separation {source} retains native output slots; neutral result ownership and identities remain unresolved.")));
                }
                raw(ctx, &mut notes, "mesh-separate-lane", parsed.lane)?;
                separation = settings(&parsed, &mut notes);
                if let Some(value) = parsed.source {
                    slot(ctx, &mut notes, "mesh-separate-source", value)?;
                }
                if let Some(separator) = parsed.separator {
                    notes.property("mesh-separate-separator", separator);
                }
                raw(ctx, &mut notes, "mesh-separate-envelope", parsed.envelope)?;
                if parsed.envelope.len() == envelope::LEN {
                    let guid = ctx.utf16le_text(
                        &parsed.envelope[envelope::GUID_UTF16..envelope::ZEROS],
                        (envelope::ZEROS - envelope::GUID_UTF16) / 2,
                        false,
                        "F3D Separate envelope GUID",
                    );
                    let valid_guid = match guid {
                        Ok(guid) => {
                            notes.property("mesh-separate-envelope-guid", guid.as_str());
                            crate::bytes::is_guid_hyphenated(&guid)
                        }
                        Err(error @ CodecError::ResourceLimit(_)) => return Err(error),
                        Err(_) => false,
                    };
                    if !valid_guid
                        || parsed.envelope[envelope::ZEROS..]
                            .iter()
                            .any(|byte| *byte != 0)
                    {
                        notes.losses.push(F3dLossCode::FeatureMeshOperationFieldsUninterpreted.note(format!("Mesh separation {source} retains an unrecognized common envelope value.")));
                    }
                }
                run(ctx, &mut notes, "mesh-separate-common", &parsed.common)?;
                if let Some(error) = parsed.framing {
                    notes.losses.push(F3dLossCode::FeatureMeshSeparateTailPartial.note(format!("Mesh separation {source} retains all fields before its unresolved {error}.")));
                }
                if parsed.separator.is_some_and(|byte| byte != 0) {
                    notes
                        .losses
                        .push(
                            F3dLossCode::FeatureMeshOperationFieldsUninterpreted.note(format!(
                                "Mesh separation {source} retains source separator {:?}.",
                                parsed.separator
                            )),
                        );
                }
                if parsed.common.declared.is_some() {
                    notes.losses.push(F3dLossCode::FeatureMeshSeparateReferenceRolesAssumed.note(format!("Mesh separation {source} provisionally reads its first common-table slot as the repeated source group and later slots as associated native records.")));
                }
                if let (Some(group), Some(repeated)) = (
                    parsed.source.and_then(Slot::target),
                    parsed.common.slots.first().and_then(|slot| slot.target()),
                ) {
                    if group != repeated {
                        notes.losses.push(F3dLossCode::FeatureMeshSeparateReferenceMismatch.note(format!("Mesh separation {source} retains source group {group} and common-table group {repeated}.")));
                    }
                }
                members = project_source(ctx, records, &parsed, &mut notes)?;
                if members.is_none() {
                    notes.losses.push(F3dLossCode::FeatureMeshSeparateMembershipUnresolved.note(format!("Mesh separation {source} retains partial source facts because its complete ordered distinct native membership is unresolved.")));
                }
            }
            other => {
                let reason = match other {
                    Some((_, Err(error))) => error,
                    _ => MeshOperationFraming("Separate scope or metadata"),
                };
                notes.losses.push(F3dLossCode::FeatureMeshSeparateScopeUnparsed.note(format!("Mesh separation {source} retains its actual source anchor because its {reason} is unresolved.")));
            }
        }
        notes.losses.push(F3dLossCode::FeatureMeshSeparateSourceUnresolved.note(format!("Mesh separation {source} retains ordered native source facts; historical tessellation and face-group geometry remain unresolved.")));
        let source_selection = if scope.is_some() {
            MeshSelection::Native {
                native: SelectionReference::try_from(source.clone())
                    .map_err(CodecError::malformed)?,
                members,
            }
        } else {
            MeshSelection::Unresolved {
                feature: feature.id.clone(),
            }
        };
        for (key, value) in notes.properties {
            let original = NonBlankString::new(key.clone())
                .ok_or_else(|| CodecError::malformed("blank Separate property key"))?;
            let available = if feature.source_properties.contains_key(&original) {
                let mut available = None;
                for ordinal in 0..=feature.source_properties.len() {
                    let candidate =
                        NonBlankString::new(format!("{key}:mesh-separate:{ordinal}"))
                            .ok_or_else(|| CodecError::malformed("blank Separate property key"))?;
                    if !feature.source_properties.contains_key(&candidate) {
                        available = Some(candidate);
                        break;
                    }
                }
                available
                    .ok_or_else(|| CodecError::malformed("Separate property namespace exhausted"))?
            } else {
                original
            };
            ctx.retain_bytes(
                u64_from_index(available.as_str().len()),
                "F3D Separate property key",
            )?;
            feature.source_properties.insert(available, value);
        }
        losses.append(&mut notes.losses);
        let mut definition = feature.evaluation.definition().clone();
        let operation = match &mut definition {
            FeatureDefinition::Operation(operation)
            | FeatureDefinition::PostProcess { operation, .. } => operation,
        };
        *operation = FeatureOperation::MeshModify {
            source: source_selection,
            modification: MeshModification::Separate { separation },
        };
        feature.evaluation.set_definition(definition);
    }
    Ok(())
}

#[cfg(test)]
mod tests;
