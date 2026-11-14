// SPDX-License-Identifier: Apache-2.0
//! Parse Form cage references and bind committed neutral cages.

use crate::container::ContainerScan;
use crate::design::decode::sketch::{next_indexed_record_offset, IndexedRecordOffsets};
use crate::ids::{self, native_stream};
use crate::layout::compact_selection_envelope_prefix as selection_prefix;
use crate::layout::{
    form_class_325_cage_entry, form_class_325_cage_table, form_class_328_cage_group,
    form_class_328_metadata_group, form_class_328_reference_entry, form_class_328_scope,
    form_class_350_member_owner_tail, form_serializer_frame_132,
};
use crate::records::feature::scope::DesignParameterScope;
use cadmpeg_core::decode::{bounded_len, DecodeContext, View};
use cadmpeg_core::CodecError;
use std::collections::{HashMap, HashSet};

use crate::loss::F3dLossCode;
use cadmpeg_ir::features::form::{
    FormCageBinding, FormCageObservation, FormConstruction, FormInputPath, FormInputs,
    FormKnownOrder, FormListRole, FormMember, FormMembership, FormNativeList, FormNativeMember,
    FormObservationIndex, FormOrder, FormResource, FormResourceAvailability,
};
use cadmpeg_ir::features::{Feature, FeatureDefinition, FeatureOperation, NonEmptyMembers};
use cadmpeg_ir::ids::NativeId;
use cadmpeg_ir::report::loss::LossNote;
use cadmpeg_ir::subd::SubdSurface;

pub(super) fn unresolved(
    scope: &DesignParameterScope,
    parameters: &[(u32, &crate::records::parameters::DesignParameter)],
) -> Result<FeatureDefinition, CodecError> {
    let mut inputs = form_inputs(scope)?;
    inputs.parameters = super::scope_parameter_expressions(scope, parameters)?;
    FormConstruction::try_new(
        inputs,
        Vec::new(),
        FormConstruction::default().membership().clone(),
    )
    .map(|construction| FeatureDefinition::Operation(FeatureOperation::Form(construction)))
    .map_err(CodecError::malformed)
}

fn record_id(
    scope: &DesignParameterScope,
    record: impl std::fmt::Display,
) -> Result<NativeId, CodecError> {
    let stream = native_stream(&scope.id).unwrap_or(ids::DEFAULT_STREAM);
    NativeId::mint(format!("{stream}:design-record#{record}")).map_err(CodecError::malformed)
}

fn input_path<T: Copy + std::fmt::Display>(
    scope: &DesignParameterScope,
    records: &[T],
) -> Result<FormInputPath, CodecError> {
    let references =
        std::iter::once(NativeId::mint(scope.id.clone()).map_err(CodecError::malformed))
            .chain(records.iter().map(|&record| record_id(scope, record)))
            .collect::<Result<Vec<_>, _>>()?;
    Ok(FormInputPath {
        references: NonEmptyMembers::try_from(references).map_err(CodecError::malformed)?,
    })
}

fn form_inputs(scope: &DesignParameterScope) -> Result<FormInputs, CodecError> {
    if NativeId::mint(scope.id.clone()).is_err() {
        return Ok(FormInputs::default());
    }
    Ok(FormInputs {
        references: scope
            .reference_members()
            .values()
            .map(|&record| record_id(scope, record))
            .collect::<Result<_, _>>()?,
        ..FormInputs::default()
    })
}

fn note(losses: &mut Vec<LossNote>, code: F3dLossCode, scope: &str, reason: &str) {
    let note = code.note(format!("Form {scope}: {reason}."));
    if !losses.contains(&note) {
        losses.push(note);
    }
}

pub(crate) fn project_forms(
    ctx: &DecodeContext<'_>,
    scan: &ContainerScan,
    scopes: &[DesignParameterScope],
    features: &mut [Feature],
    cages: &[SubdSurface],
    losses: &mut Vec<LossNote>,
) -> Result<(), CodecError> {
    if let Some(limit) = ctx.resource_refusal() {
        return Err(CodecError::ResourceLimit(limit));
    }
    let mut indices = HashMap::new();
    for feature in features {
        let mut retained = match feature.evaluation.definition() {
            FeatureDefinition::Operation(FeatureOperation::Form(construction)) => {
                construction.clone()
            }
            FeatureDefinition::Operation(FeatureOperation::Native { kind, parameters })
                if kind.as_str() == "Form" =>
            {
                FormConstruction::try_new(
                    FormInputs {
                        parameters: parameters.clone(),
                        ..FormInputs::default()
                    },
                    Vec::new(),
                    FormConstruction::default().membership().clone(),
                )
                .map_err(CodecError::malformed)?
            }
            _ => continue,
        };
        let scope = scopes.iter().find(|scope| {
            NativeId::mint(scope.id.clone()).is_ok()
                && scope.kind() == crate::records::feature::scope::DesignFeatureKind::Form
                && match feature.native_ref.as_deref() {
                    Some(native_ref) => native_ref == scope.id.as_str(),
                    None => feature.id == crate::ids::neutral_feature_id(scope),
                }
        });
        if let Some(scope) = scope {
            let mut inputs = retained.inputs().clone();
            if inputs.references.is_empty() {
                inputs.references = form_inputs(scope)?.references;
            }
            retained = FormConstruction::try_new(
                inputs,
                retained.observations().to_vec(),
                retained.membership().clone(),
            )
            .map_err(CodecError::malformed)?;
        }
        let entry = scope
            .and_then(|scope| native_stream(&scope.id))
            .and_then(|stream| {
                scan.design_stream_entry_for_scope(
                    cadmpeg_core::container::ContainerRole::Bulkstream,
                    stream,
                )
            });
        if let (Some(scope), Some(entry), Some(bytes)) = (
            scope,
            entry,
            entry.and_then(|entry| scan.entry_view(&entry.name)),
        ) {
            let records = indices
                .entry(entry.name.as_str())
                .or_insert_with(|| IndexedRecordOffsets::build(bytes.window()));
            let mut inputs = form_inputs(scope)?;
            inputs.parameters = retained.inputs().parameters.clone();
            let mut facts = FormFacts {
                inputs,
                observations: Vec::new(),
            };
            let source = FormSource {
                bytes: bytes.window(),
                records,
                scope,
                cages,
                stream: &entry.name,
            };
            let membership = collect_membership(ctx, scan, source, scopes, &mut facts, losses)?;
            let construction =
                FormConstruction::try_new(facts.inputs, facts.observations, membership)
                    .map_err(CodecError::malformed)?;
            retained = retain_knowledge(retained, construction)?;
            if !retained.membership_is_complete() {
                note(
                    losses,
                    F3dLossCode::FeatureFormMembershipUnresolved,
                    &scope.id,
                    "cage membership or identity joins remain incomplete",
                );
            }
        } else {
            note(
                losses,
                F3dLossCode::FeatureFormSourceUnavailable,
                feature.id.as_str(),
                "source scope or Design payload is unavailable",
            );
        }
        feature
            .evaluation
            .set_definition(FeatureDefinition::Operation(FeatureOperation::Form(
                retained,
            )));
    }
    Ok(())
}

fn retain_knowledge(
    old: FormConstruction,
    new: FormConstruction,
) -> Result<FormConstruction, CodecError> {
    let old_members = match old.membership() {
        FormMembership::Complete { members, .. } => members.len(),
        FormMembership::Partial { members, .. } => members.len(),
    };
    let new_members = match new.membership() {
        FormMembership::Complete { members, .. } => members.len(),
        FormMembership::Partial { members, .. } => members.len(),
    };
    let keep_old = !new.membership_is_complete()
        && (old.membership_is_complete() || old_members > new_members);
    let (base, extra) = if keep_old { (old, new) } else { (new, old) };
    let mut facts = FormFacts {
        inputs: base.inputs().clone(),
        observations: base.observations().to_vec(),
    };
    if facts.inputs.parameters.is_empty() {
        facts.inputs.parameters = extra.inputs().parameters.clone();
    }
    if facts.inputs.references.is_empty() {
        facts
            .inputs
            .references
            .clone_from(&extra.inputs().references);
    }
    for list in &extra.inputs().lists {
        if !facts.inputs.lists.contains(list) {
            facts.inputs.lists.push(list.clone());
        }
    }
    for observation in extra.observations() {
        facts.observe(observation.clone());
    }
    FormConstruction::try_new(facts.inputs, facts.observations, base.membership().clone())
        .map_err(CodecError::malformed)
}

struct FormFacts {
    inputs: FormInputs,
    observations: Vec<FormCageObservation>,
}
impl FormFacts {
    fn observe(&mut self, observation: FormCageObservation) -> FormObservationIndex {
        if let Some(index) =
            self.observations
                .iter()
                .position(|old| match (&old.binding, &observation.binding) {
                    (
                        FormCageBinding::Decoded { cage: a, .. },
                        FormCageBinding::Decoded { cage: b, .. },
                    ) => a == b,
                    (FormCageBinding::Unresolved {}, FormCageBinding::Unresolved {}) => {
                        old.paths == observation.paths
                    }
                    _ => old.binding == observation.binding,
                })
        {
            if let (
                FormCageBinding::Decoded { resource: old, .. },
                FormCageBinding::Decoded { resource: new, .. },
            ) = (&mut self.observations[index].binding, &observation.binding)
            {
                if old.is_none() {
                    old.clone_from(new);
                }
            }
            for path in observation.paths {
                if !self.observations[index].paths.contains(&path) {
                    self.observations[index].paths.push(path);
                }
            }
            return FormObservationIndex(index);
        }
        let index = self.observations.len();
        self.observations.push(observation);
        FormObservationIndex(index)
    }
    fn list(
        &mut self,
        scope: &DesignParameterScope,
        source: u32,
        role: FormListRole,
        members: &[Option<u64>],
    ) -> Result<(), CodecError> {
        let list = FormNativeList {
            source: input_path(scope, &[source])?,
            role,
            declared_count: u32::try_from(members.len()).map_err(CodecError::malformed)?,
            members: members
                .iter()
                .enumerate()
                .filter_map(|(ordinal, &record)| record.map(|record| (ordinal, record)))
                .map(|(ordinal, record)| {
                    Ok(FormNativeMember {
                        ordinal: u32::try_from(ordinal).map_err(CodecError::malformed)?,
                        reference: record_id(scope, record)?,
                    })
                })
                .collect::<Result<_, CodecError>>()?,
        };
        if !self.inputs.lists.contains(&list) {
            self.inputs.lists.push(list);
        }
        Ok(())
    }
    fn tracking(
        &mut self,
        ctx: &DecodeContext<'_>,
        source: FormSource<'_>,
        types: &FormRecordTypes,
    ) -> Result<(), CodecError> {
        for &group in source.scope.reference_members().values() {
            if types.selection_groups.get(&group) != Some(&0) {
                continue;
            }
            if let Some(members) = form_tracking_members(
                ctx,
                source.bytes,
                source.records,
                group,
                source.scope.record_index,
            )? {
                self.list(
                    source.scope,
                    group,
                    FormListRole::TrackingSelection,
                    &members,
                )?;
            }
        }
        Ok(())
    }
    fn surface(
        &mut self,
        scan: &ContainerScan,
        source: FormSource<'_>,
        path: FormInputPath,
        surface: Option<u64>,
        serializers: &FormCageSerializers,
        losses: &mut Vec<LossNote>,
    ) -> Result<Vec<FormObservationIndex>, CodecError> {
        let FormSource { scope, cages, .. } = source;
        let entries = surface.and_then(|surface| serializers.entries.get(&surface));
        let Some(entries) = entries else {
            return Ok(vec![self.observe(FormCageObservation {
                paths: vec![path],
                binding: FormCageBinding::Unresolved {},
            })]);
        };
        if entries.len() > 1 {
            note(
                losses,
                F3dLossCode::FeatureFormCageBindingAmbiguous,
                &scope.id,
                "surface has multiple serializers",
            );
        }
        let mut alternatives = Vec::new();
        for serializer in entries {
            let mut path = path.clone();
            path.references.push(record_id(scope, serializer.record)?);
            let entry = scan
                .design_asset_folder()
                .map(|folder| format!("{folder}/TSplines.BlobParts/{}", serializer.name));
            let resource = FormResource {
                source: record_id(scope, serializer.record)?,
                entry: cadmpeg_core::text::NonBlankString::new(
                    entry.as_ref().unwrap_or(&serializer.name).clone(),
                )
                .ok_or_else(|| CodecError::malformed("Form serializer entry is blank"))?,
            };
            let matches = cages
                .iter()
                .filter(|cage| {
                    entry.as_ref().is_some_and(|entry| {
                        scan.has_entry(entry)
                            && cage.source_object.as_ref().is_some_and(|source| {
                                source.format == cadmpeg_ir::CodecFormat::F3d
                                    && source.object_id.as_str() == entry
                            })
                    })
                })
                .collect::<Vec<_>>();
            if matches.len() == 1 {
                alternatives.push(self.observe(FormCageObservation {
                    paths: vec![path.clone()],
                    binding: FormCageBinding::Decoded {
                        cage: matches[0].id.clone(),
                        resource: Some(resource),
                    },
                }));
            } else {
                if matches.len() > 1 {
                    note(
                        losses,
                        F3dLossCode::FeatureFormCageBindingAmbiguous,
                        &scope.id,
                        "resource has multiple neutral cage associations",
                    );
                }
                let availability = match entry {
                    None => FormResourceAvailability::Unknown,
                    Some(_) if matches.len() > 1 => FormResourceAvailability::Ambiguous,
                    Some(entry) if scan.has_entry(&entry) => {
                        FormResourceAvailability::ActiveUndecoded
                    }
                    Some(_) => FormResourceAvailability::Inactive,
                };
                alternatives.push(self.observe(FormCageObservation {
                    paths: vec![path.clone()],
                    binding: FormCageBinding::Undecoded {
                        resource,
                        availability,
                    },
                }));
                for cage in matches {
                    self.observe(FormCageObservation {
                        paths: vec![path.clone()],
                        binding: FormCageBinding::Decoded {
                            cage: cage.id.clone(),
                            resource: None,
                        },
                    });
                }
            }
        }
        Ok(alternatives)
    }
    fn membership(
        &self,
        slots: Vec<Vec<FormObservationIndex>>,
        cardinality: Option<u32>,
        order: FormOrder,
        certainty: FormKnownOrder,
        closed: bool,
    ) -> Result<FormMembership, CodecError> {
        let mut members = Vec::new();
        let mut seen = HashSet::new();
        let mut complete = closed && (cardinality == Some(0) || !slots.is_empty());
        for (ordinal, alternatives) in slots.into_iter().enumerate() {
            let [observation] = alternatives.as_slice() else {
                complete = false;
                continue;
            };
            if matches!(
                self.observations[observation.0].binding,
                FormCageBinding::Undecoded {
                    availability: FormResourceAvailability::Inactive,
                    ..
                }
            ) {
                complete = false;
                continue;
            }
            if !seen.insert(*observation) {
                complete = false;
                continue;
            }
            complete &= matches!(
                self.observations[observation.0].binding,
                FormCageBinding::Decoded { .. }
            );
            members.push(FormMember {
                observation: *observation,
                ordinal: if cardinality.is_some() {
                    Some(u32::try_from(ordinal).map_err(CodecError::malformed)?)
                } else {
                    None
                },
            });
        }
        if complete && order != FormOrder::Unknown {
            Ok(FormMembership::Complete {
                members: members
                    .into_iter()
                    .map(|member| member.observation)
                    .collect(),
                order: if order == FormOrder::Source {
                    FormKnownOrder::Source
                } else {
                    FormKnownOrder::Assumed
                },
                certainty,
            })
        } else {
            Ok(FormMembership::Partial {
                cardinality,
                order,
                members,
            })
        }
    }
}

fn active_slots(
    facts: &FormFacts,
    slots: Vec<Vec<FormObservationIndex>>,
) -> Vec<Vec<FormObservationIndex>> {
    slots
        .into_iter()
        .filter(|slots| {
            !slots.iter().all(|index| {
                matches!(
                    facts.observations[index.0].binding,
                    FormCageBinding::Undecoded {
                        availability: FormResourceAvailability::Inactive,
                        ..
                    }
                )
            })
        })
        .collect()
}

#[derive(Clone, Copy)]
struct FormSource<'a> {
    bytes: &'a [u8],
    records: &'a IndexedRecordOffsets,
    scope: &'a DesignParameterScope,
    cages: &'a [SubdSurface],
    stream: &'a str,
}

fn collect_membership(
    ctx: &DecodeContext<'_>,
    scan: &ContainerScan,
    source: FormSource<'_>,
    scopes: &[DesignParameterScope],
    facts: &mut FormFacts,
    losses: &mut Vec<LossNote>,
) -> Result<FormMembership, CodecError> {
    let FormSource {
        bytes,
        records,
        scope,
        stream,
        ..
    } = source;
    let serializers = form_cage_serializers(ctx, bytes, records)?;
    let types = match crate::design::decode::meta::metadata_for_bulk_stream(scan, stream) {
        Ok(Some(meta)) => Some(FormRecordTypes::of(&meta)),
        Ok(None) | Err(CodecError::Malformed(_)) => None,
        Err(error) => return Err(error),
    };
    if let Some(types) = &types {
        facts.tracking(ctx, source, types)?;
    }
    if scope.class_tag.as_str() == "325" {
        if let Some(objects) = form_class_325_cage_objects(
            bytes,
            records,
            scope.record_index,
            scope.reference_members().values().copied(),
        )? {
            facts.list(
                scope,
                scope.record_index,
                FormListRole::CageCatalog,
                &objects
                    .iter()
                    .map(|&(object, _)| Some(u64::from(object)))
                    .collect::<Vec<_>>(),
            )?;
            let slots = objects
                .into_iter()
                .map(|(object, companion)| {
                    let surface = form_class_325_cage_surface(bytes, records, object);
                    let mut path = vec![object];
                    path.extend(surface);
                    let alternatives = facts.surface(
                        scan,
                        source,
                        input_path(scope, &path)?,
                        surface.map(u64::from),
                        &serializers,
                        losses,
                    )?;
                    let companion_path = input_path(scope, &[companion])?;
                    for index in &alternatives {
                        if !facts.observations[index.0].paths.contains(&companion_path) {
                            facts.observations[index.0]
                                .paths
                                .push(companion_path.clone());
                        }
                    }
                    Ok::<_, CodecError>(alternatives)
                })
                .collect::<Result<Vec<_>, _>>()?;
            let slots = active_slots(facts, slots);
            return facts.membership(slots, None, FormOrder::Source, FormKnownOrder::Source, true);
        }
    }
    if scope.class_tag.as_str() == "328"
        && scopes
            .iter()
            .filter(|scope| scope.kind() == crate::records::feature::scope::DesignFeatureKind::Form)
            .count()
            == 1
        && form_class_328_envelope(bytes, records, scope)
    {
        let slots = serializers
            .ordered
            .iter()
            .map(|&surface| {
                facts.surface(
                    scan,
                    source,
                    input_path(scope, &[surface])?,
                    Some(surface),
                    &serializers,
                    losses,
                )
            })
            .collect::<Result<_, _>>()?;
        let slots = active_slots(facts, slots);
        note(
            losses,
            F3dLossCode::FeatureFormLegacySerializerMembershipAssumed,
            &scope.id,
            "sole-scope serializers establish cage membership",
        );
        note(
            losses,
            F3dLossCode::FeatureFormLegacySerializerOrderAssumed,
            &scope.id,
            "serializer frame order establishes cage order",
        );
        return facts.membership(
            slots,
            None,
            FormOrder::Assumed,
            FormKnownOrder::Assumed,
            true,
        );
    }
    let lists = form_cage_lists(
        ctx,
        bytes,
        records,
        scope
            .reference_members()
            .values()
            .copied()
            .filter(|record| {
                types
                    .as_ref()
                    .is_none_or(|types| !types.selection_groups.contains_key(record))
            }),
        scope.record_index,
    )?;
    let mut ordinary = None;
    for list in &lists {
        let objects = &list.objects;
        facts.list(scope, list.record, FormListRole::CageObjects, objects)?;
        let slots = objects
            .iter()
            .map(|&object| {
                let Some(object) = object else {
                    return Ok(Vec::new());
                };
                let mut path = vec![u64::from(list.record), object];
                let joined = u32::try_from(object)
                    .ok()
                    .and_then(|object| form_cage_surface(bytes, records, object, &mut path));
                let surface = joined.map(|(_, surface)| surface);
                let alternatives = facts.surface(
                    scan,
                    source,
                    input_path(scope, &path)?,
                    surface,
                    &serializers,
                    losses,
                )?;
                if let Some((owner, _)) = joined {
                    let mut branch = path[..path.len() - 1].to_vec();
                    branch.push(owner);
                    let owner_path = input_path(scope, &branch)?;
                    for index in &alternatives {
                        if !facts.observations[index.0].paths.contains(&owner_path) {
                            facts.observations[index.0].paths.push(owner_path.clone());
                        }
                    }
                    if owner != u64::from(scope.record_index) {
                        return Ok(Vec::new());
                    }
                }
                Ok::<_, CodecError>(alternatives)
            })
            .collect::<Result<_, _>>()?;
        let membership = facts.membership(
            slots,
            Some(u32::try_from(objects.len()).map_err(CodecError::malformed)?),
            FormOrder::Source,
            FormKnownOrder::Source,
            lists.len() == 1,
        )?;
        if matches!(membership, FormMembership::Complete { .. }) {
            return Ok(membership);
        }
        if lists.len() == 1 {
            ordinary = Some(membership);
        }
    }
    let Some(types) = types else {
        note(
            losses,
            F3dLossCode::FeatureFormDefinitionUnavailable,
            &scope.id,
            "construction metadata is unavailable",
        );
        return Ok(ordinary.unwrap_or_else(|| FormConstruction::default().membership().clone()));
    };
    body_membership(ctx, scan, source, &types, facts, ordinary, losses)
}

fn body_membership(
    ctx: &DecodeContext<'_>,
    scan: &ContainerScan,
    source: FormSource<'_>,
    types: &FormRecordTypes,
    facts: &mut FormFacts,
    ordinary: Option<FormMembership>,
    losses: &mut Vec<LossNote>,
) -> Result<FormMembership, CodecError> {
    let FormSource {
        bytes,
        records,
        scope,
        ..
    } = source;
    let serializers = form_typed_serializers(ctx, bytes, records, types)?;
    let mut slots = Vec::new();
    let mut closed = !types.unsupported_bodies;
    if types.unsupported_bodies {
        note(
            losses,
            F3dLossCode::FeatureFormBodyRegistrationUnsupported,
            &scope.id,
            "a body registration or indexed identity is unsupported",
        );
    }
    for &body in &types.bodies {
        let alternatives = form_body_references(bytes, records, body);
        if alternatives.is_empty() {
            closed = false;
            continue;
        }
        let owned = alternatives
            .iter()
            .any(|references| references[0] == Some(u64::from(scope.record_index)));
        let body_id = record_id(scope, body)?;
        let linked = facts.observations.iter().any(|observation| {
            observation
                .paths
                .iter()
                .any(|path| path.references.contains(&body_id))
        });
        if !owned && !linked {
            if alternatives
                .iter()
                .any(|references| references[0].is_none())
            {
                closed = false;
            }
            continue;
        }
        let unique = alternatives.len() == 1;
        if !unique {
            note(
                losses,
                F3dLossCode::FeatureFormCageBindingAmbiguous,
                &scope.id,
                "body has multiple reference runs",
            );
            closed = false;
        }
        let mut selected = Vec::new();
        for references in alternatives {
            if references.iter().any(Option::is_none) {
                note(
                    losses,
                    F3dLossCode::FeatureFormBodyReferencesPartialAssumed,
                    &scope.id,
                    "retains local references from an incomplete body envelope",
                );
            }
            let mut path = vec![u64::from(body)];
            path.extend(references[2]);
            let observed = facts.surface(
                scan,
                source,
                input_path(scope, &path)?,
                references[2],
                &serializers,
                losses,
            )?;
            for index in &observed {
                for target in references
                    .iter()
                    .enumerate()
                    .filter_map(|(ordinal, &target)| (ordinal != 2).then_some(target).flatten())
                {
                    let path = input_path(scope, &[u64::from(body), target])?;
                    if !facts.observations[index.0].paths.contains(&path) {
                        facts.observations[index.0].paths.push(path);
                    }
                }
            }
            let valid = references[0] == Some(u64::from(scope.record_index))
                && references[1].is_some_and(|target| types.body_containers.contains(&target))
                && references[2].is_some_and(|target| types.surfaces.contains(&target))
                && references[3].is_some_and(|target| types.scene_nodes.contains(&target));
            if unique && valid {
                selected = observed;
            } else {
                closed = false;
            }
        }
        slots.push(selected);
    }
    if slots.is_empty() {
        return Ok(ordinary.unwrap_or_else(|| FormConstruction::default().membership().clone()));
    }
    if slots.len() > 1 {
        note(
            losses,
            F3dLossCode::FeatureFormCageOrderAssumed,
            &scope.id,
            "body-record index establishes cage order",
        );
    }
    let cardinality = if closed {
        Some(u32::try_from(slots.len()).map_err(CodecError::malformed)?)
    } else {
        None
    };
    let order = if slots.len() > 1 {
        FormOrder::Assumed
    } else if closed {
        FormOrder::Source
    } else {
        FormOrder::Unknown
    };
    facts.membership(slots, cardinality, order, FormKnownOrder::Source, closed)
}

fn form_tracking_members(
    ctx: &DecodeContext<'_>,
    bytes: &[u8],
    records: &IndexedRecordOffsets,
    group: u32,
    owner: u32,
) -> Result<Option<Vec<Option<u64>>>, CodecError> {
    let parsed = (|| {
        let (start, paired) = one_indexed_frame(records, group)?;
        let frame = bytes.get(start..paired)?;
        if frame.get(selection_prefix::ZERO_RUN_10..selection_prefix::OWNER_MARKER)
            != Some(&[0; 10])
        {
            return None;
        }
        let short_owner = frame.get(selection_prefix::OWNER_MARKER)
            == Some(&selection_prefix::OWNER_MARKER_VALUE)
            && View::u64_le_at(frame, selection_prefix::OWNER_SCOPE_RECORD_INDEX)
                == Some(u64::from(owner))
            && frame.get(selection_prefix::OWNER_ZERO_TAIL..selection_prefix::MEMBER_COUNT)
                == Some(&[0; 2]);
        let mut at = selection_prefix::MEMBER_COUNT;
        if !short_owner {
            at = selection_prefix::OWNER_MARKER;
            if crate::bytes::take_reference(frame, &mut at)?.local()?.0 != u64::from(owner) {
                return None;
            }
        }
        let count = View::u32_le_at(frame, at)?;
        at = at.checked_add(4)?;
        let count_usize = bounded_len(u64::from(count), 11, frame.len().checked_sub(at)?)?;
        let fixed_slots = frame
            .len()
            .checked_sub(at + 11 * count_usize)
            .is_some_and(|tail| matches!(tail, 52 | 53));
        Some((frame, count_usize, at, fixed_slots))
    })();
    let Some((frame, count_usize, mut at, fixed_slots)) = parsed else {
        return Ok(None);
    };
    let mut members = ctx.alloc_filled(count_usize, None, "Form tracking members")?;
    for member in &mut members {
        if fixed_slots {
            let end = at + 11;
            let mut cursor = at;
            *member = crate::bytes::take_reference(&frame[..end], &mut cursor)
                .filter(|_| cursor == end)
                .and_then(|reference| reference.local().map(|(target, _)| target));
            at = end;
            continue;
        }
        let Some(reference) = crate::bytes::take_reference(frame, &mut at) else {
            break;
        };
        *member = reference.local().map(|(target, _)| target);
    }
    Ok(Some(members))
}

const FORM_BODY_TYPE_GUID: &str = "FA1F0081-C9A1-4E14-8F25-D12DD868ED90";
const FORM_BODY_TYPE_VERSIONS: [u32; 3] = [9, 10, 11];
const FORM_SERIALIZER_TYPE_GUID: &str = "830A2A2B-0AA9-4D6A-ACA1-F7A2B2A06571";
const FORM_SURFACE_TYPE_GUID: &str = "22709494-9149-4A70-A829-7D748A7B5DB1";
const FORM_SELECTION_GROUP_TYPE_GUID: &str = "E079B079-016A-43C2-ACC7-9627B32A8960";
const FORM_BODY_REFERENCE_COUNT: usize = 4;

#[derive(Default)]
struct FormRecordTypes {
    bodies: Vec<u32>,
    unsupported_bodies: bool,
    serializers: Vec<u32>,
    surfaces: HashSet<u64>,
    selection_groups: HashMap<u32, u32>,
    body_containers: HashSet<u64>,
    scene_nodes: HashSet<u64>,
}

impl FormRecordTypes {
    fn of(meta: &crate::metastream::MetaStream) -> Self {
        use crate::design::presentation::{BODY_SCENE_NODE_TYPE_GUID, BREP_CONTAINER_TYPE_GUID};
        let mut types = Self::default();
        for design_type in &meta.types {
            let guid = design_type.type_guid.as_str();
            let version = design_type.version;
            let entities = design_type.entities.values().copied();
            if guid.eq_ignore_ascii_case(FORM_BODY_TYPE_GUID) {
                if FORM_BODY_TYPE_VERSIONS.contains(&version)
                    && design_type.module == crate::records::entity_header::DESIGN_MODULE_FUSION
                    && design_type.base_type_guid.value().is_some_and(|base| {
                        base.as_str()
                            .eq_ignore_ascii_case("CB844AB6-240D-4FC9-9C9F-3679DC896D6F")
                    })
                {
                    for entity in entities {
                        match u32::try_from(entity) {
                            Ok(entity) => types.bodies.push(entity),
                            Err(_) => types.unsupported_bodies = true,
                        }
                    }
                } else if entities.count() != 0 {
                    types.unsupported_bodies = true;
                }
            } else if guid.eq_ignore_ascii_case(FORM_SERIALIZER_TYPE_GUID) && version == 0 {
                types
                    .serializers
                    .extend(entities.filter_map(|entity| u32::try_from(entity).ok()));
            } else if guid.eq_ignore_ascii_case(FORM_SURFACE_TYPE_GUID) {
                types.surfaces.extend(entities);
            } else if guid.eq_ignore_ascii_case(FORM_SELECTION_GROUP_TYPE_GUID) {
                for entity in entities.filter_map(|entity| u32::try_from(entity).ok()) {
                    types
                        .selection_groups
                        .entry(entity)
                        .and_modify(|observed| *observed = (*observed).max(version))
                        .or_insert(version);
                }
            } else if guid.eq_ignore_ascii_case(BREP_CONTAINER_TYPE_GUID) {
                types.body_containers.extend(entities);
            } else if guid.eq_ignore_ascii_case(BODY_SCENE_NODE_TYPE_GUID) {
                types.scene_nodes.extend(entities);
            }
        }
        types.bodies.sort_unstable();
        types.bodies.dedup();
        types.serializers.sort_unstable();
        types.serializers.dedup();
        types
    }
}

fn form_body_references(
    bytes: &[u8],
    records: &IndexedRecordOffsets,
    body: u32,
) -> Vec<[Option<u64>; FORM_BODY_REFERENCE_COUNT]> {
    let Some((start, paired)) = one_indexed_frame(records, body) else {
        return Vec::new();
    };
    let Some(frame) = bytes.get(start..paired) else {
        return Vec::new();
    };
    let mut full = Vec::new();
    let mut partial = Vec::new();
    for at in 0..frame.len() {
        let Some(mut cursor) =
            crate::design::presentation::physical_material_envelope_end(frame, at)
        else {
            continue;
        };
        let mut references = [None; FORM_BODY_REFERENCE_COUNT];
        for reference in &mut references {
            let Some(parsed) = crate::bytes::take_reference(frame, &mut cursor) else {
                break;
            };
            *reference = parsed.local().map(|(target, _)| target);
        }
        if references.iter().all(Option::is_some) {
            full.push(references);
        } else if references.iter().any(Option::is_some) {
            partial.push(references);
        }
    }
    if full.is_empty() {
        partial
    } else {
        full
    }
}

fn form_class_328_envelope(
    bytes: &[u8],
    records: &IndexedRecordOffsets,
    scope: &DesignParameterScope,
) -> bool {
    let Some((scope_start, scope_paired)) = one_indexed_frame(records, scope.record_index) else {
        return false;
    };
    if scope_paired.checked_sub(scope_start) != Some(form_class_328_scope::LEN)
        || bytes.get(scope_start + 4..scope_start + 7) != Some(b"328")
        || bytes.get(scope_paired + 4..scope_paired + 7) != Some(b"267")
    {
        return false;
    }
    if scope.reference_members().len() != 2 {
        return false;
    }
    let Some(group_record) = scope
        .reference_members()
        .values()
        .copied()
        .find(|record_index| {
            records.frames(*record_index).any(|(start, paired)| {
                bytes.get(start + 4..start + 7) == Some(b"417")
                    && bytes.get(paired + 4..paired + 7) == Some(b"267")
            })
        })
    else {
        return false;
    };
    let Some(metadata_record) = scope
        .reference_members()
        .values()
        .copied()
        .find(|record_index| {
            *record_index != group_record
                && records.frames(*record_index).any(|(start, paired)| {
                    bytes.get(start + 4..start + 7) == Some(b"341")
                        && bytes.get(paired + 4..paired + 7) == Some(b"267")
                })
        })
    else {
        return false;
    };
    let Some((group_start, group_paired)) = records.frames(group_record).find(|(start, paired)| {
        bytes.get(*start + 4..*start + 7) == Some(b"417")
            && bytes.get(*paired + 4..*paired + 7) == Some(b"267")
    }) else {
        return false;
    };
    if group_paired.checked_sub(group_start) != Some(form_class_328_cage_group::LEN)
        || bytes.get(
            group_start + form_class_328_cage_group::ZERO_RUN_14
                ..group_start + form_class_328_cage_group::OWNER_MARKER,
        ) != Some(&[0; 14])
        || bytes.get(group_start + form_class_328_cage_group::OWNER_MARKER)
            != Some(&form_class_328_cage_group::OWNER_MARKER_VALUE)
        || View::u64_le_at(
            bytes,
            group_start + form_class_328_cage_group::OWNER_SCOPE_RECORD_INDEX,
        ) != Some(u64::from(scope.record_index))
        || bytes.get(
            group_start + form_class_328_cage_group::ZERO_RUN_14_AFTER_OWNER
                ..group_start + form_class_328_cage_group::MEMBER_COUNT,
        ) != Some(&[0; 14])
        || View::u32_le_at(bytes, group_start + form_class_328_cage_group::MEMBER_COUNT)
            != Some(form_class_328_cage_group::MEMBER_COUNT_VALUE)
        || View::u32_le_at(bytes, group_start + form_class_328_cage_group::TERMINAL_U32)
            != Some(form_class_328_cage_group::TERMINAL_U32_VALUE)
        || bytes.get(group_start + form_class_328_cage_group::FIRST_TAIL_MARKER)
            != Some(&form_class_328_cage_group::FIRST_TAIL_MARKER_VALUE)
        || bytes.get(
            group_start + form_class_328_cage_group::FIRST_TAIL_ZERO_RUN
                ..group_start + form_class_328_cage_group::SECOND_TAIL_MARKER,
        ) != Some(&[0; 4])
        || bytes.get(group_start + form_class_328_cage_group::SECOND_TAIL_MARKER)
            != Some(&form_class_328_cage_group::SECOND_TAIL_MARKER_VALUE)
        || bytes.get(
            group_start + form_class_328_cage_group::SECOND_TAIL_ZERO_RUN
                ..group_start + form_class_328_cage_group::FINAL_SCOPE_MARKER,
        ) != Some(&[0; 3])
        || bytes.get(group_start + form_class_328_cage_group::FINAL_SCOPE_MARKER)
            != Some(&form_class_328_cage_group::FINAL_SCOPE_MARKER_VALUE)
        || View::u64_le_at(
            bytes,
            group_start + form_class_328_cage_group::FINAL_SCOPE_RECORD_INDEX,
        ) != Some(u64::from(scope.record_index))
        || bytes.get(
            group_start + form_class_328_cage_group::PAIRED_ZERO_TAIL
                ..group_start + form_class_328_cage_group::LEN,
        ) != Some(&[0; 2])
    {
        return false;
    }
    let Some(group_class_272) = View::u64_le_at(
        bytes,
        group_start + form_class_328_cage_group::FIRST_TAIL_RECORD_INDEX,
    )
    .and_then(|value| u32::try_from(value).ok()) else {
        return false;
    };
    let Some(group_class_404) = View::u64_le_at(
        bytes,
        group_start + form_class_328_cage_group::SECOND_TAIL_RECORD_INDEX,
    )
    .and_then(|value| u32::try_from(value).ok()) else {
        return false;
    };
    if !unique_record_has_class(bytes, records, group_class_272, b"272")
        || !unique_record_has_class(bytes, records, group_class_404, b"404")
    {
        return false;
    }
    let mut members = [None; 4];
    for ordinal in 0..members.len() {
        let at = group_start
            + form_class_328_cage_group::MEMBER_ENTRIES
            + ordinal * form_class_328_reference_entry::LEN;
        if bytes.get(at) != Some(&form_class_328_reference_entry::MARKER_VALUE)
            || bytes.get(
                at + form_class_328_reference_entry::ZERO_TAIL
                    ..at + form_class_328_reference_entry::LEN,
            ) != Some(&[0; 2])
        {
            return false;
        }
        let Some(member) =
            View::u64_le_at(bytes, at + form_class_328_reference_entry::RECORD_INDEX)
                .and_then(|value| u32::try_from(value).ok())
        else {
            return false;
        };
        if members[..ordinal].contains(&Some(member)) {
            return false;
        }
        members[ordinal] = Some(member);
        let Some((member_start, member_paired)) = one_indexed_frame(records, member) else {
            return false;
        };
        if bytes.get(member_start + 4..member_start + 7) != Some(b"350")
            || bytes.get(member_paired + 4..member_paired + 7) != Some(b"351")
            || member_paired < member_start + form_class_350_member_owner_tail::LEN
            || bytes.get(
                member_paired - form_class_350_member_owner_tail::LEN
                    + form_class_350_member_owner_tail::OWNER_MARKER,
            ) != Some(&form_class_350_member_owner_tail::OWNER_MARKER_VALUE)
            || View::u64_le_at(
                bytes,
                member_paired - form_class_350_member_owner_tail::LEN
                    + form_class_350_member_owner_tail::OWNER_GROUP_RECORD_INDEX,
            ) != Some(u64::from(group_record))
            || bytes.get(
                member_paired - form_class_350_member_owner_tail::LEN
                    + form_class_350_member_owner_tail::ZERO_RUN_3
                    ..member_paired - form_class_350_member_owner_tail::LEN
                        + form_class_350_member_owner_tail::PAIRED_MARKER,
            ) != Some(&[0; 3])
            || bytes.get(
                member_paired - form_class_350_member_owner_tail::LEN
                    + form_class_350_member_owner_tail::PAIRED_MARKER,
            ) != Some(&form_class_350_member_owner_tail::PAIRED_MARKER_VALUE)
        {
            return false;
        }
    }
    let Some((metadata_start, metadata_paired)) =
        records.frames(metadata_record).find(|(start, paired)| {
            bytes.get(*start + 4..*start + 7) == Some(b"341")
                && bytes.get(*paired + 4..*paired + 7) == Some(b"267")
        })
    else {
        return false;
    };
    if metadata_paired.checked_sub(metadata_start) != Some(form_class_328_metadata_group::LEN)
        || bytes.get(
            metadata_start + form_class_328_metadata_group::ZERO_RUN_10
                ..metadata_start + form_class_328_metadata_group::OWNER_MARKER,
        ) != Some(&[0; 10])
        || bytes.get(metadata_start + form_class_328_metadata_group::OWNER_MARKER)
            != Some(&form_class_328_metadata_group::OWNER_MARKER_VALUE)
        || View::u64_le_at(
            bytes,
            metadata_start + form_class_328_metadata_group::OWNER_SCOPE_RECORD_INDEX,
        ) != Some(u64::from(scope.record_index))
        || bytes.get(
            metadata_start + form_class_328_metadata_group::ZERO_RUN_2
                ..metadata_start + form_class_328_metadata_group::MEMBER_COUNT,
        ) != Some(&[0; 2])
        || View::u32_le_at(
            bytes,
            metadata_start + form_class_328_metadata_group::MEMBER_COUNT,
        ) != Some(form_class_328_metadata_group::MEMBER_COUNT_VALUE)
        || View::u32_le_at(
            bytes,
            metadata_start + form_class_328_metadata_group::TAIL_U32,
        ) != Some(form_class_328_metadata_group::TAIL_U32_VALUE)
        || View::f64_le_at(
            bytes,
            metadata_start + form_class_328_metadata_group::TAIL_SCALAR,
        )
        .is_none_or(|value| !value.is_finite())
        || bytes.get(metadata_start + form_class_328_metadata_group::FIRST_TAIL_MARKER)
            != Some(&form_class_328_metadata_group::FIRST_TAIL_MARKER_VALUE)
        || bytes.get(
            metadata_start + form_class_328_metadata_group::FIRST_TAIL_ZERO_RUN
                ..metadata_start + form_class_328_metadata_group::SECOND_TAIL_MARKER,
        ) != Some(&[0; 4])
        || bytes.get(metadata_start + form_class_328_metadata_group::SECOND_TAIL_MARKER)
            != Some(&form_class_328_metadata_group::SECOND_TAIL_MARKER_VALUE)
        || bytes.get(
            metadata_start + form_class_328_metadata_group::SECOND_TAIL_ZERO_RUN
                ..metadata_start + form_class_328_metadata_group::FINAL_SCOPE_MARKER,
        ) != Some(&[0; 3])
        || bytes.get(metadata_start + form_class_328_metadata_group::FINAL_SCOPE_MARKER)
            != Some(&form_class_328_metadata_group::FINAL_SCOPE_MARKER_VALUE)
        || View::u64_le_at(
            bytes,
            metadata_start + form_class_328_metadata_group::FINAL_SCOPE_RECORD_INDEX,
        ) != Some(u64::from(scope.record_index))
        || bytes.get(
            metadata_start + form_class_328_metadata_group::PAIRED_ZERO_TAIL
                ..metadata_start + form_class_328_metadata_group::LEN,
        ) != Some(&[0; 2])
    {
        return false;
    }
    let Some(metadata_class_259) = View::u64_le_at(
        bytes,
        metadata_start + form_class_328_metadata_group::FIRST_TAIL_RECORD_INDEX,
    )
    .and_then(|value| u32::try_from(value).ok()) else {
        return false;
    };
    let Some(metadata_class_404) = View::u64_le_at(
        bytes,
        metadata_start + form_class_328_metadata_group::SECOND_TAIL_RECORD_INDEX,
    )
    .and_then(|value| u32::try_from(value).ok()) else {
        return false;
    };
    if !unique_record_has_class(bytes, records, metadata_class_259, b"259")
        || !unique_record_has_class(bytes, records, metadata_class_404, b"404")
    {
        return false;
    }
    let mut metadata_members = [None; 19];
    for ordinal in 0..metadata_members.len() {
        let at = metadata_start
            + form_class_328_metadata_group::MEMBER_ENTRIES
            + ordinal * form_class_328_reference_entry::LEN;
        if bytes.get(at) != Some(&form_class_328_reference_entry::MARKER_VALUE)
            || bytes.get(
                at + form_class_328_reference_entry::ZERO_TAIL
                    ..at + form_class_328_reference_entry::LEN,
            ) != Some(&[0; 2])
        {
            return false;
        }
        let Some(member) =
            View::u64_le_at(bytes, at + form_class_328_reference_entry::RECORD_INDEX)
                .and_then(|value| u32::try_from(value).ok())
        else {
            return false;
        };
        if metadata_members[..ordinal].contains(&Some(member))
            || records.offsets(member).len() != 1
            || records
                .offsets(member)
                .first()
                .and_then(|offset| bytes.get(*offset + 4..*offset + 7))
                != Some(b"320")
        {
            return false;
        }
        metadata_members[ordinal] = Some(member);
    }
    true
}

fn unique_record_has_class(
    bytes: &[u8],
    records: &IndexedRecordOffsets,
    record_index: u32,
    class: &[u8],
) -> bool {
    let [offset] = records.offsets(record_index) else {
        return false;
    };
    bytes.get(*offset + 4..*offset + 7) == Some(class)
}

fn one_indexed_frame(records: &IndexedRecordOffsets, record_index: u32) -> Option<(usize, usize)> {
    let mut frames = records.frames(record_index);
    let frame = frames.next()?;
    frames.next().is_none().then_some(frame)
}

fn form_class_325_cage_objects(
    bytes: &[u8],
    records: &IndexedRecordOffsets,
    scope_record_index: u32,
    mut owner_record_indices: impl Iterator<Item = u32>,
) -> Result<Option<Vec<(u32, u32)>>, CodecError> {
    let parsed = (|| {
        const CAGE_COUNT: usize = 32;
        const TYPE_DISCRIMINATOR_FIRST: u32 = 307;

        let mut frames = records.frames(scope_record_index);
        let (start, paired) = frames.next()?;
        if frames.next().is_some() {
            return None;
        }
        if bytes.get(start + 4..start + 7) != Some(b"325")
            || bytes.get(paired + 4..paired + 7) != Some(b"258")
            || paired.checked_sub(start)? != form_class_325_cage_table::LEN
            || bytes.get(
                start + form_class_325_cage_table::ZERO_RUN_9
                    ..start + form_class_325_cage_table::LIST_MARKER,
            )? != [0; 9]
            || bytes.get(start + form_class_325_cage_table::LIST_MARKER) != Some(&1)
            || bytes.get(
                start + form_class_325_cage_table::ZERO_RUN_5
                    ..start + form_class_325_cage_table::OWNER_MARKER,
            )? != [0; 5]
            || bytes.get(start + form_class_325_cage_table::OWNER_MARKER) != Some(&1)
            || bytes.get(
                start + form_class_325_cage_table::ZERO_RUN_2
                    ..start + form_class_325_cage_table::CAGE_COUNT,
            )? != [0; 2]
        {
            return None;
        }
        let owner_record = u32::try_from(View::u64_le_at(
            bytes,
            start + form_class_325_cage_table::OWNER_RESULT_RECORD_INDEX,
        )?)
        .ok()?;
        let [owner_at, ..] = records.offsets(owner_record) else {
            return None;
        };
        if !owner_record_indices.any(|index| index == owner_record)
            || bytes.get(*owner_at + 4..*owner_at + 7) != Some(b"407")
        {
            return None;
        }
        let count = bounded_len(
            u64::from(View::u32_le_at(
                bytes,
                start + form_class_325_cage_table::CAGE_COUNT,
            )?),
            form_class_325_cage_entry::LEN,
            paired.checked_sub(start + form_class_325_cage_table::CAGE_ENTRIES)?,
        )?;
        if count != CAGE_COUNT {
            return None;
        }
        let mut objects = Vec::new();
        let mut seen_type_discriminators = [false; CAGE_COUNT];
        for ordinal in 0..count {
            let entry = start
                .checked_add(form_class_325_cage_table::CAGE_ENTRIES)?
                .checked_add(form_class_325_cage_entry::LEN.checked_mul(ordinal)?)?;
            if bytes.get(entry + form_class_325_cage_entry::CAGE_OBJECT_MARKER) != Some(&1)
                || bytes.get(
                    entry + form_class_325_cage_entry::CAGE_OBJECT_ZERO
                        ..entry + form_class_325_cage_entry::TYPE_DISCRIMINATOR,
                )? != [0, 0]
                || bytes.get(entry + form_class_325_cage_entry::COMPANION_MARKER) != Some(&1)
                || bytes.get(
                    entry + form_class_325_cage_entry::COMPANION_ZERO
                        ..entry + form_class_325_cage_entry::LEN,
                )? != [0, 0]
            {
                return None;
            }
            let type_discriminator = u32::try_from(View::u64_le_at(
                bytes,
                entry + form_class_325_cage_entry::TYPE_DISCRIMINATOR,
            )?)
            .ok()?;
            let type_slot =
                usize::try_from(type_discriminator.checked_sub(TYPE_DISCRIMINATOR_FIRST)?).ok()?;
            if type_slot >= CAGE_COUNT || seen_type_discriminators[type_slot] {
                return None;
            }
            seen_type_discriminators[type_slot] = true;
            let object = u32::try_from(View::u64_le_at(
                bytes,
                entry + form_class_325_cage_entry::CAGE_OBJECT_RECORD_INDEX,
            )?)
            .ok()?;
            let companion = u32::try_from(View::u64_le_at(
                bytes,
                entry + form_class_325_cage_entry::COMPANION_RECORD_INDEX,
            )?)
            .ok()?;
            let mut object_frames = records
                .frames(object)
                .filter(|(_, paired)| bytes.get(paired + 4..paired + 7) == Some(b"258"));
            let (object_at, _) = object_frames.next()?;
            if object_frames.next().is_some() {
                return None;
            }
            let [companion_at, ..] = records.offsets(companion) else {
                return None;
            };
            if bytes.get(object_at + 4..object_at + 7) != Some(b"289")
                || bytes.get(*companion_at + 4..*companion_at + 7) != Some(b"273")
            {
                return None;
            }
            objects.push((object, companion));
        }
        Some(Ok(objects))
    })();
    parsed.transpose()
}

fn form_class_325_cage_surface(
    bytes: &[u8],
    records: &IndexedRecordOffsets,
    object_record: u32,
) -> Option<u32> {
    let mut frames = records
        .frames(object_record)
        .filter(|(_, paired)| bytes.get(paired + 4..paired + 7) == Some(b"258"));
    let (start, paired) = frames.next()?;
    if frames.next().is_some() {
        return None;
    }
    if bytes.get(start + 4..start + 7) != Some(b"289") {
        return None;
    }
    let mut surface = None;
    for at in start.checked_add(11)?..paired {
        if bytes.get(at) != Some(&1) {
            continue;
        }
        let target = View::u32_le_at(bytes, at + 1)?;
        let [target_at] = records.offsets(target) else {
            continue;
        };
        if bytes.get(target_at + 4..target_at + 7) == Some(b"310")
            && surface.replace(target).is_some()
        {
            return None;
        }
    }
    surface
}

fn form_cage_objects(
    ctx: &DecodeContext<'_>,
    bytes: &[u8],
    records: &IndexedRecordOffsets,
    record_index: u32,
    scope_record_index: u32,
) -> Result<Option<Vec<Option<u64>>>, CodecError> {
    let parsed = (|| -> Option<(usize, usize)> {
        let mut frames = records.frames(record_index).filter(|(_, paired)| {
            matches!(bytes.get(paired + 4..paired + 7), Some(b"258" | b"264"))
        });
        let (offset, paired) = frames.next()?;
        if frames.next().is_some() {
            return None;
        }
        if View::u64_le_at(bytes, offset + 7)? != u64::from(record_index)
            || bytes.get(offset + 15..offset + 21)? != [0; 6]
            || bytes.get(offset + 21) != Some(&1)
            || View::u64_le_at(bytes, offset + 22)? != u64::from(scope_record_index)
            || bytes.get(offset + 30..offset + 32)? != [0, 0]
        {
            return None;
        }
        let count = usize::try_from(View::u32_le_at(bytes, offset + 32)?).ok()?;
        if paired.checked_sub(offset)? != 88usize.checked_add(11usize.checked_mul(count)?)? {
            return None;
        }
        Some((count, offset.checked_add(36)?))
    })();
    let Some((count, mut cursor)) = parsed else {
        return Ok(None);
    };
    let mut objects = ctx.alloc_filled(count, None, "Form cage-list slots")?;
    for object in &mut objects {
        if bytes.get(cursor) == Some(&1) && bytes.get(cursor + 9..cursor + 11) == Some(&[0, 0]) {
            *object = View::u64_le_at(bytes, cursor + 1);
        }
        cursor += 11;
    }
    Ok(Some(objects))
}

struct FormCageList {
    record: u32,
    objects: Vec<Option<u64>>,
}

fn form_cage_lists(
    ctx: &DecodeContext<'_>,
    bytes: &[u8],
    records: &IndexedRecordOffsets,
    references: impl Iterator<Item = u32>,
    scope_record_index: u32,
) -> Result<Vec<FormCageList>, CodecError> {
    let mut lists = Vec::new();
    for record in references {
        if let Some(objects) = form_cage_objects(ctx, bytes, records, record, scope_record_index)? {
            lists.push(FormCageList { record, objects });
        }
    }
    Ok(lists)
}

fn form_cage_surface(
    bytes: &[u8],
    records: &IndexedRecordOffsets,
    object_record: u32,
    path: &mut Vec<u64>,
) -> Option<(u64, u64)> {
    let [object_at] = records.offsets(object_record) else {
        return None;
    };
    if bytes.get(object_at + 4..object_at + 7) != Some(b"301")
        || next_indexed_record_offset(bytes, object_at + 1)? != object_at + 200
        || bytes.get(object_at + 189) != Some(&1)
    {
        return None;
    }
    let first_wrapper = View::u64_le_at(bytes, object_at + 190)?;
    path.push(first_wrapper);
    let [first_at] = records.offsets(u32::try_from(first_wrapper).ok()?) else {
        return None;
    };
    if bytes.get(first_at + 4..first_at + 7) != Some(b"373")
        || next_indexed_record_offset(bytes, first_at + 1)? != first_at + 33
        || bytes.get(first_at + 11..first_at + 21)? != [0; 10]
        || bytes.get(first_at + 21) != Some(&1)
        || bytes.get(first_at + 30..first_at + 33)? != [0; 3]
    {
        return None;
    }
    let second_wrapper = View::u64_le_at(bytes, first_at + 22)?;
    path.push(second_wrapper);
    let [second_at] = records.offsets(u32::try_from(second_wrapper).ok()?) else {
        return None;
    };
    if bytes.get(second_at + 4..second_at + 7) != Some(b"362")
        || next_indexed_record_offset(bytes, second_at + 1)? != second_at + 29
        || bytes.get(second_at + 11..second_at + 21)? != [0; 10]
    {
        return None;
    }
    let carrier = View::u64_le_at(bytes, second_at + 21)?;
    path.push(carrier);
    let mut carrier_frames = records.frames(u32::try_from(carrier).ok()?);
    let (carrier_at, carrier_paired) = carrier_frames.next()?;
    if carrier_frames.next().is_some() {
        return None;
    }
    if carrier_paired.checked_sub(carrier_at)? != 665
        || bytes.get(carrier_at + 4..carrier_at + 7) != Some(b"457")
        || bytes.get(carrier_paired + 4..carrier_paired + 7) != Some(b"264")
        || bytes.get(carrier_at + 317) != Some(&1)
        || bytes.get(carrier_at + 339) != Some(&1)
    {
        return None;
    }
    let surface = View::u64_le_at(bytes, carrier_at + 340)?;
    path.push(surface);
    Some((View::u64_le_at(bytes, carrier_at + 318)?, surface))
}

struct FormSerializer {
    record: u32,
    name: String,
}
struct FormCageSerializers {
    ordered: Vec<u64>,
    entries: HashMap<u64, Vec<FormSerializer>>,
}

impl FormCageSerializers {
    #[cfg(test)]
    fn entry_name(&self, surface: u64) -> Option<&str> {
        let entries = self.entries.get(&surface)?;
        (entries.len() == 1).then(|| entries[0].name.as_str())
    }
}

fn form_cage_serializers(
    ctx: &DecodeContext<'_>,
    bytes: &[u8],
    records: &IndexedRecordOffsets,
) -> Result<FormCageSerializers, CodecError> {
    let mut offsets = Vec::new();
    for (_, record_offsets) in records.records() {
        for offset in record_offsets {
            offsets.push(*offset);
        }
    }
    offsets.sort_unstable_by(Ord::cmp);
    let mut frames = Vec::new();
    for offset in offsets {
        let frame = match bytes.get(offset + 4..offset + 7) {
            Some(b"335") => FormSerializerFrame::PairedClass331,
            Some(b"315" | b"349" | b"360" | b"431" | b"446") => FormSerializerFrame::Ordinary,
            _ => continue,
        };
        frames.push((offset, frame));
    }
    collect_form_cage_serializers(ctx, bytes, records, frames)
}

fn form_typed_serializers(
    ctx: &DecodeContext<'_>,
    bytes: &[u8],
    records: &IndexedRecordOffsets,
    types: &FormRecordTypes,
) -> Result<FormCageSerializers, CodecError> {
    let mut offsets = types
        .serializers
        .iter()
        .filter_map(|serializer| records.offsets(*serializer).first().copied())
        .collect::<Vec<_>>();
    offsets.sort_unstable();
    collect_form_cage_serializers(
        ctx,
        bytes,
        records,
        offsets
            .into_iter()
            .map(|offset| (offset, FormSerializerFrame::Ordinary)),
    )
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum FormSerializerFrame {
    Ordinary,
    PairedClass331,
}

fn collect_form_cage_serializers(
    ctx: &DecodeContext<'_>,
    bytes: &[u8],
    records: &IndexedRecordOffsets,
    frames: impl IntoIterator<Item = (usize, FormSerializerFrame)>,
) -> Result<FormCageSerializers, CodecError> {
    let mut ordered = Vec::new();
    let mut entries: HashMap<u64, Vec<FormSerializer>> = HashMap::new();
    for (offset, frame) in frames {
        let Some((surface, entry_name)) =
            form_serializer_entry(ctx, bytes, records, offset, frame)?
        else {
            continue;
        };
        let Some(record) = View::u32_le_at(bytes, offset + 7) else {
            continue;
        };
        if !entries.contains_key(&surface) {
            ordered.push(surface);
        }
        entries.entry(surface).or_default().push(FormSerializer {
            record,
            name: entry_name,
        });
    }
    Ok(FormCageSerializers { ordered, entries })
}

fn form_serializer_entry(
    ctx: &DecodeContext<'_>,
    bytes: &[u8],
    records: &IndexedRecordOffsets,
    offset: usize,
    frame: FormSerializerFrame,
) -> Result<Option<(u64, String)>, CodecError> {
    let is_class_335 = frame == FormSerializerFrame::PairedClass331;
    if bytes.get(
        offset + form_serializer_frame_132::ZERO_RUN_10
            ..offset + form_serializer_frame_132::ENTRY_NAME_LENGTH,
    ) != Some(&[0; 10])
    {
        return Ok(None);
    }
    let Some(next) = next_indexed_record_offset(bytes, offset + 1) else {
        return Ok(None);
    };
    if next != offset + form_serializer_frame_132::LEN
        || (is_class_335 && bytes.get(next + 4..next + 7) != Some(b"331"))
    {
        return Ok(None);
    }
    let name_at = offset + form_serializer_frame_132::ENTRY_NAME_LENGTH;
    let Some(name_units) = View::u32_le_at(bytes, name_at)
        .filter(|count| (1..=256).contains(count))
        .and_then(|count| usize::try_from(count).ok())
    else {
        return Ok(None);
    };
    let Some(after_name) = name_at.checked_add(4).and_then(|start| {
        name_units
            .checked_mul(2)
            .and_then(|length| start.checked_add(length))
    }) else {
        return Ok(None);
    };
    let Some(raw_name) = bytes.get(name_at + 4..after_name) else {
        return Ok(None);
    };
    let entry_name = match ctx.utf16le_text(
        raw_name,
        name_units,
        false,
        "f3d form serializer name materialization",
    ) {
        Ok(text) => text,
        Err(CodecError::Malformed(_)) => return Ok(None),
        Err(error) => return Err(error),
    };
    if !entry_name.starts_with("TSpline.")
        || !std::path::Path::new(&entry_name)
            .extension()
            .is_some_and(|extension| extension.eq_ignore_ascii_case("tsm"))
        || bytes.get(after_name) != Some(&1)
        || bytes.get(after_name + 9..after_name + 11) != Some(&[0, 0])
    {
        return Ok(None);
    }
    let Some(surface) = View::u64_le_at(bytes, after_name + 1) else {
        return Ok(None);
    };
    if is_class_335
        && (bytes
            .get(after_name + 11..offset + form_serializer_frame_132::LEN)
            .is_none_or(|tail| tail.iter().any(|byte| *byte != 0))
            || !u32::try_from(surface).ok().is_some_and(|surface| {
                records.offsets(surface).iter().any(|surface_offset| {
                    bytes.get(*surface_offset + 4..*surface_offset + 7) == Some(b"358")
                })
            }))
    {
        return Ok(None);
    }
    if !is_class_335 && after_name + 11 != offset + form_serializer_frame_132::LEN {
        return Ok(None);
    }
    Ok(Some((surface, entry_name)))
}

#[cfg(test)]
mod tests;
