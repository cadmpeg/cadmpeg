use super::scope_bytes;
use crate::container::ContainerScan;
use crate::design::decode::mesh_operations::separate::{
    Run, SeparateScope, Slot, SlotValue, SourceDetails,
};
use crate::design::decode::mesh_operations::{MeshOperationFraming, MeshOperationRecords};
use crate::layout::mesh_separate_common_envelope as envelope;
use crate::loss::F3dLossCode;
use crate::records::feature::scope::DesignParameterScope;
use cadmpeg_core::decode::{u64_from_index, DecodeContext, ResourceDimension, ResourceLimit, View};
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
use std::collections::{BTreeMap, HashMap, HashSet};
use std::fmt::{self, Write};

struct Notes<'a, 'ctx> {
    ctx: &'a DecodeContext<'ctx>,
    source: &'a str,
    existing: &'a BTreeMap<NonBlankString, String>,
    properties: Vec<(NonBlankString, String)>,
    keys: HashSet<String>,
    losses: Vec<LossNote>,
}

struct Length(usize);

impl fmt::Write for Length {
    fn write_str(&mut self, text: &str) -> fmt::Result {
        self.0 = self.0.checked_add(text.len()).ok_or(fmt::Error)?;
        Ok(())
    }
}

fn formatted_len(args: fmt::Arguments<'_>) -> Result<usize, CodecError> {
    let mut length = Length(0);
    fmt::write(&mut length, args)
        .map_err(|_| CodecError::malformed("Separate formatted extent overflow"))?;
    Ok(length.0)
}

fn allocation_error(ctx: &DecodeContext<'_>, size: usize) -> CodecError {
    ResourceLimit::allocation_failed(
        ResourceDimension::RetainedBytes,
        ctx.policy().limits.max_retained_bytes,
        u64_from_index(size),
        "F3D Separate staging allocation",
    )
    .into()
}

fn formatted(
    ctx: &DecodeContext<'_>,
    args: fmt::Arguments<'_>,
    size: usize,
) -> Result<String, CodecError> {
    let mut text = String::new();
    text.try_reserve_exact(size)
        .map_err(|_| allocation_error(ctx, size))?;
    text.write_fmt(args)
        .map_err(|_| CodecError::malformed("Separate formatting"))?;
    Ok(text)
}

impl Notes<'_, '_> {
    fn property(
        &mut self,
        key: fmt::Arguments<'_>,
        value: fmt::Arguments<'_>,
    ) -> Result<(), CodecError> {
        let key_len = formatted_len(key)?;
        let value_len = formatted_len(value)?;
        self.ctx
            .retain_bytes(u64_from_index(key_len), "F3D Separate property key")?;
        self.ctx
            .retain_bytes(u64_from_index(value_len), "F3D Separate property value")?;
        let mut key = formatted(self.ctx, key, key_len)?;
        let base_len = key.len();
        if self.contains(&key) {
            let limit = self
                .existing
                .len()
                .checked_add(self.properties.len())
                .ok_or_else(|| CodecError::malformed("Separate property namespace overflow"))?;
            for ordinal in 0..=limit {
                key.truncate(base_len);
                let suffix_len = formatted_len(format_args!(":mesh-separate:{ordinal}"))?;
                key.try_reserve(suffix_len)
                    .map_err(|_| allocation_error(self.ctx, suffix_len))?;
                write!(&mut key, ":mesh-separate:{ordinal}")
                    .map_err(|_| CodecError::malformed("Separate property key"))?;
                if !self.contains(&key) {
                    self.ctx
                        .retain_bytes(u64_from_index(suffix_len), "F3D Separate property key")?;
                    break;
                }
            }
        }
        let index_key = formatted(self.ctx, format_args!("{key}"), key.len())?;
        self.keys
            .try_reserve(1)
            .map_err(|_| allocation_error(self.ctx, 1))?;
        let key = NonBlankString::new(key)
            .ok_or_else(|| CodecError::malformed("blank Separate property key"))?;
        let value = formatted(self.ctx, value, value_len)?;
        self.properties
            .try_reserve(1)
            .map_err(|_| allocation_error(self.ctx, 1))?;
        self.keys.insert(index_key);
        self.properties.push((key, value));
        Ok(())
    }

    fn contains(&self, key: &str) -> bool {
        self.existing.contains_key(key) || self.keys.contains(key)
    }

    fn loss(&mut self, code: F3dLossCode, message: fmt::Arguments<'_>) -> Result<(), CodecError> {
        let len = formatted_len(message)?;
        self.ctx
            .retain_bytes(u64_from_index(len), "F3D Separate loss message")?;
        let message = formatted(self.ctx, message, len)?;
        self.losses
            .try_reserve(1)
            .map_err(|_| allocation_error(self.ctx, 1))?;
        self.losses.push(code.note(message));
        Ok(())
    }
}

struct Identifier<'a>(&'a [u8]);

impl Identifier<'_> {
    fn characters(&self) -> impl Iterator<Item = Result<char, std::char::DecodeUtf16Error>> + '_ {
        let mut view = View::over_retained(self.0);
        char::decode_utf16(std::iter::from_fn(move || view.u16_le()))
    }
}

impl fmt::Display for Identifier<'_> {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        for character in self.characters() {
            formatter.write_char(character.map_err(|_| fmt::Error)?)?;
        }
        Ok(())
    }
}

fn raw(notes: &mut Notes<'_, '_>, key: impl fmt::Display, bytes: &[u8]) -> Result<(), CodecError> {
    notes.property(
        format_args!("{key}"),
        format_args!("{}", NativeBytes::new(bytes)),
    )
}

fn slot(notes: &mut Notes<'_, '_>, key: &str, value: Slot<'_>) -> Result<(), CodecError> {
    let source_id = notes.source;
    notes.property(
        format_args!("{key}:offset"),
        format_args!("{}", value.offset),
    )?;
    raw(notes, format!("{key}:raw"), value.raw)?;
    let error = match value.value {
        SlotValue::Null => {
            notes.property(format_args!("{key}:state"), format_args!("null"))?;
            None
        }
        SlotValue::Present {
            target,
            inline_type,
            registration,
        } => {
            notes.property(format_args!("{key}:state"), format_args!("present"))?;
            notes.property(format_args!("{key}:target"), format_args!("{target}"))?;
            if let Some(guid) = inline_type {
                notes.property(format_args!("{key}:inline-type"), format_args!("{guid}"))?;
            }
            match registration {
                Ok(ordinal) => {
                    notes.property(
                        format_args!("{key}:registration"),
                        format_args!("{ordinal}"),
                    )?;
                    None
                }
                Err(error) => Some(error),
            }
        }
        SlotValue::Invalid(error) => {
            notes.property(format_args!("{key}:state"), format_args!("invalid"))?;
            Some(error)
        }
        SlotValue::Unread(error) => {
            notes.property(format_args!("{key}:state"), format_args!("unread"))?;
            Some(error)
        }
    };
    if let Some(error) = error {
        notes.property(format_args!("{key}:error"), format_args!("{error}"))?;
        notes.loss(
            F3dLossCode::FeatureMeshSeparateReferenceUnresolved,
            format_args!(
                "Mesh separation {} retains {key} at {} with unresolved {error}.",
                source_id, value.offset
            ),
        )?;
    }
    Ok(())
}

fn run(notes: &mut Notes<'_, '_>, key: &str, value: &Run<'_>) -> Result<(), CodecError> {
    if let Some(count) = value.declared {
        notes.property(format_args!("{key}:count"), format_args!("{count}"))?;
    }
    for (ordinal, value) in value.slots.iter().enumerate() {
        slot(notes, &format!("{key}:{ordinal}"), *value)?;
    }
    if let Some((ordinal, offset, count)) = value.unread {
        notes.property(format_args!("{key}:unread-from"), format_args!("{ordinal}"))?;
        notes.property(
            format_args!("{key}:unread-offset"),
            format_args!("{offset}"),
        )?;
        notes.property(format_args!("{key}:unread-count"), format_args!("{count}"))?;
    }
    if let Some(error) = value.framing {
        notes.property(format_args!("{key}:framing"), format_args!("{error}"))?;
    }
    Ok(())
}

fn settings(
    parsed: &SeparateScope<'_>,
    notes: &mut Notes<'_, '_>,
) -> Result<MeshSeparation, CodecError> {
    let source_id = notes.source;
    let mut separation = MeshSeparation::default();
    if let Some(selector) = parsed.selector {
        notes.property(
            format_args!("mesh-separate-selector"),
            format_args!("{selector}"),
        )?;
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
        notes.loss(
            code,
            format_args!(
                "Mesh separation {} provisionally reads selector {selector} as method {:?}.",
                source_id, separation.method
            ),
        )?;
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
            notes.loss(code, format_args!("Mesh separation {} provisionally reads selector {selector} as group scope {:?}; selected group identities remain unresolved.", source_id, separation.groups))?;
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
            notes.property(
                format_args!("mesh-separate-lane-word:{ordinal}"),
                format_args!("{value}"),
            )?;
            notes.loss(carrier, format_args!("Mesh separation {source_id} provisionally assigns raw lane word {ordinal} to this independent body control."))?;
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
            notes.loss(code, format_args!("Mesh separation {source_id} provisionally reads lane word {ordinal} value {value} with 0=false and 1=true; neutral value is {target:?}."))?;
        }
    }
    Ok(separation)
}

fn details(
    notes: &mut Notes<'_, '_>,
    key: &str,
    value: &SourceDetails<'_>,
) -> Result<(), CodecError> {
    let source_id = notes.source;
    raw(notes, format!("{key}:raw"), value.frame.window())?;
    run(notes, &format!("{key}:members"), &value.run)?;
    for (ordinal, reference) in value.references.iter().enumerate() {
        slot(notes, &format!("{key}:reference:{ordinal}"), *reference)?;
    }
    for (ordinal, word) in value.words.iter().enumerate() {
        notes.property(format_args!("{key}:word:{ordinal}"), format_args!("{word}"))?;
    }
    for (ordinal, identifier) in value.identifiers.iter().enumerate() {
        raw(notes, format!("{key}:identifier:{ordinal}:raw"), identifier)?;
        let text = Identifier(identifier);
        if text.characters().all(|character| character.is_ok()) {
            notes.property(
                format_args!("{key}:identifier:{ordinal}"),
                format_args!("{text}"),
            )?;
        } else {
            notes.loss(F3dLossCode::FeatureMeshSeparateSourceFieldsUninterpreted, format_args!("Mesh separation {source_id} retains invalid UTF-16 identifier {key}:{ordinal} as bytes."))?;
        }
    }
    if let Some(role) = value.role {
        notes.property(format_args!("{key}:role"), format_args!("{role}"))?;
    }
    if let Some(error) = value.framing {
        notes.property(format_args!("{key}:framing"), format_args!("{error}"))?;
        notes.loss(
            F3dLossCode::FeatureMeshSeparateSourceFieldsUninterpreted,
            format_args!(
                "Mesh separation {source_id} retains the readable {key} fields before {error}."
            ),
        )?;
    }
    Ok(())
}

fn project_source(
    ctx: &DecodeContext<'_>,
    records: &MeshOperationRecords<'_>,
    parsed: &SeparateScope<'_>,
    notes: &mut Notes<'_, '_>,
) -> Result<Option<NativeSelections>, CodecError> {
    let source_id = notes.source;
    let Some(group) = parsed.source.and_then(Slot::admitted) else {
        return Ok(None);
    };
    let source = match records.separate_source(ctx, group)? {
        Ok(source) => source,
        Err(error) => {
            notes.property(
                format_args!("mesh-separate-source-error"),
                format_args!("{error}"),
            )?;
            return Ok(None);
        }
    };
    details(notes, "mesh-separate-group", &source.group)?;
    if let Some(member) = parsed.common.slots.get(1).and_then(|slot| slot.target()) {
        if source.group.run.framing.is_none()
            && !source
                .group
                .run
                .slots
                .iter()
                .any(|slot| slot.target() == Some(member))
        {
            notes.loss(F3dLossCode::FeatureMeshSeparateReferenceMismatch, format_args!("Mesh separation {source_id} retains common-table member {member} outside its source-group member run."))?;
        }
    }
    for (ordinal, member) in source.members.iter().enumerate() {
        match member {
            Some(Ok(member)) => details(notes, &format!("mesh-separate-member:{ordinal}"), member)?,
            Some(Err(error)) => notes.property(
                format_args!("mesh-separate-member:{ordinal}:error"),
                format_args!("{error}"),
            )?,
            None => (),
        }
    }
    notes.loss(F3dLossCode::FeatureMeshSeparateSourceFieldsUninterpreted, format_args!("Mesh separation {source_id} retains group and member auxiliary references, native identifiers, words and roles in source properties."))?;
    if let Some(error) = source.membership.framing {
        notes.property(
            format_args!("mesh-separate-membership-framing"),
            format_args!("{error}"),
        )?;
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
            notes.loss(F3dLossCode::FeatureMeshSeparateSourceMembershipAssumed, format_args!("Mesh separation {source_id} provisionally interprets its complete ordered member body references as input-local mesh membership."))?;
            Ok(Some(members))
        }
        Err(error) => {
            notes.property(
                format_args!("mesh-separate-membership-error"),
                format_args!("{error}"),
            )?;
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
            ctx,
            source: &source,
            existing: &feature.source_properties,
            properties: Vec::new(),
            keys: HashSet::new(),
            losses: Vec::new(),
        };
        let definition = match feature.evaluation.definition() {
            FeatureDefinition::Operation(operation)
            | FeatureDefinition::PostProcess { operation, .. } => operation,
        };
        if let FeatureOperation::Native { parameters, .. } = definition {
            for (name, expression) in parameters {
                notes.property(
                    format_args!("parameter:{name}"),
                    format_args!("{expression}"),
                )?;
            }
        }
        let parsed = if let Some((scope, (entry, bytes))) =
            scope.and_then(|scope| Some((scope, scope_bytes(scan, scope)?)))
        {
            notes.property(
                format_args!("mesh-separate-scope-record"),
                format_args!("{}", scope.record_index),
            )?;
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
                raw(&mut notes, "mesh-separate-scope-raw", parsed.frame.window())?;
                if let Some(flag) = parsed.flag {
                    notes.property(format_args!("mesh-separate-flag"), format_args!("{flag}"))?;
                }
                if parsed.flag == Some(1) {
                    notes.loss(F3dLossCode::FeatureMeshSeparatePrefixAssumed, format_args!("Mesh separation {source} provisionally applies the flagged mesh-family prefix form."))?;
                }
                if let Some(word) = parsed.prefix_word {
                    notes.property(
                        format_args!("mesh-separate-prefix-word"),
                        format_args!("{word}"),
                    )?;
                    notes.loss(F3dLossCode::MeshScopePrefixUninterpreted, format_args!("Mesh separation {source} retains prefix word {word} with unresolved meaning."))?;
                }
                run(&mut notes, "mesh-separate-output", &parsed.outputs)?;
                if parsed.outputs.declared.is_some() {
                    notes.loss(F3dLossCode::FeatureMeshSeparateOutputRoleAssumed, format_args!("Mesh separation {source} provisionally reads the counted reference run as native output bodies in stored order."))?;
                    notes.loss(F3dLossCode::FeatureMeshSeparateOutputUnresolved, format_args!("Mesh separation {source} retains native output slots; neutral result ownership and identities remain unresolved."))?;
                }
                raw(&mut notes, "mesh-separate-lane", parsed.lane)?;
                separation = settings(&parsed, &mut notes)?;
                if let Some(value) = parsed.source {
                    slot(&mut notes, "mesh-separate-source", value)?;
                }
                if let Some(separator) = parsed.separator {
                    notes.property(
                        format_args!("mesh-separate-separator"),
                        format_args!("{separator}"),
                    )?;
                }
                raw(&mut notes, "mesh-separate-envelope", parsed.envelope)?;
                if parsed.envelope.len() == envelope::LEN {
                    let guid = ctx.utf16le_text(
                        &parsed.envelope[envelope::GUID_UTF16..envelope::ZEROS],
                        (envelope::ZEROS - envelope::GUID_UTF16) / 2,
                        false,
                        "F3D Separate envelope GUID",
                    );
                    let valid_guid = match guid {
                        Ok(guid) => {
                            notes.property(
                                format_args!("mesh-separate-envelope-guid"),
                                format_args!("{}", guid.as_str()),
                            )?;
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
                        notes.loss(F3dLossCode::FeatureMeshOperationFieldsUninterpreted, format_args!("Mesh separation {source} retains an unrecognized common envelope value."))?;
                    }
                }
                run(&mut notes, "mesh-separate-common", &parsed.common)?;
                if let Some(error) = parsed.framing {
                    notes.loss(F3dLossCode::FeatureMeshSeparateTailPartial, format_args!("Mesh separation {source} retains all fields before its unresolved {error}."))?;
                }
                if parsed.separator.is_some_and(|byte| byte != 0) {
                    notes.loss(
                        F3dLossCode::FeatureMeshOperationFieldsUninterpreted,
                        format_args!(
                            "Mesh separation {source} retains source separator {:?}.",
                            parsed.separator
                        ),
                    )?;
                }
                if parsed.common.declared.is_some() {
                    notes.loss(F3dLossCode::FeatureMeshSeparateReferenceRolesAssumed, format_args!("Mesh separation {source} provisionally reads its first common-table slot as the repeated source group and later slots as associated native records."))?;
                }
                if let (Some(group), Some(repeated)) = (
                    parsed.source.and_then(Slot::target),
                    parsed.common.slots.first().and_then(|slot| slot.target()),
                ) {
                    if group != repeated {
                        notes.loss(F3dLossCode::FeatureMeshSeparateReferenceMismatch, format_args!("Mesh separation {source} retains source group {group} and common-table group {repeated}."))?;
                    }
                }
                members = project_source(ctx, records, &parsed, &mut notes)?;
                if members.is_none() {
                    notes.loss(F3dLossCode::FeatureMeshSeparateMembershipUnresolved, format_args!("Mesh separation {source} retains partial source facts because its complete ordered distinct native membership is unresolved."))?;
                }
            }
            other => {
                let reason = match other {
                    Some((_, Err(error))) => error,
                    _ => MeshOperationFraming("Separate scope or metadata"),
                };
                notes.loss(F3dLossCode::FeatureMeshSeparateScopeUnparsed, format_args!("Mesh separation {source} retains its actual source anchor because its {reason} is unresolved."))?;
            }
        }
        notes.loss(F3dLossCode::FeatureMeshSeparateSourceUnresolved, format_args!("Mesh separation {source} retains ordered native source facts; historical tessellation and face-group geometry remain unresolved."))?;
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
        let Notes {
            properties,
            losses: mut staged_losses,
            ..
        } = notes;
        losses
            .try_reserve(staged_losses.len())
            .map_err(|_| allocation_error(ctx, staged_losses.len()))?;
        for (key, value) in properties {
            feature.source_properties.insert(key, value);
        }
        losses.append(&mut staged_losses);
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
