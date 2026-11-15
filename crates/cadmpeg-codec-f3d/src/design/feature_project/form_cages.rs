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
    FormCageBinding, FormCageObservation, FormConstruction, FormExternalVersion, FormInputPath,
    FormInputs, FormKnownOrder, FormListRole, FormMember, FormMembership, FormNativeList,
    FormNativeMember, FormNativeReference, FormNativeReferenceTarget, FormObservationIndex,
    FormOrder, FormPositionEvidence, FormReferenceRole, FormResource, FormResourceAvailability,
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
            retained = retain_knowledge(&retained, &construction)?;
            if membership_evidence_conflicts(retained.inputs()) {
                note(
                    losses,
                    F3dLossCode::FeatureFormMembershipConflict,
                    &scope.id,
                    "source membership counts or positions cannot be reconciled",
                );
            }
            if retained.observations().iter().any(|observation| {
                matches!(
                    &observation.binding,
                    FormCageBinding::Undecoded {
                        availability: FormResourceAvailability::Ambiguous,
                        ..
                    }
                )
            }) {
                note(
                    losses,
                    F3dLossCode::FeatureFormCageBindingAmbiguous,
                    &scope.id,
                    "resource association remains ambiguous",
                );
            }
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
    old: &FormConstruction,
    new: &FormConstruction,
) -> Result<FormConstruction, CodecError> {
    let mut facts = FormFacts {
        inputs: old.inputs().clone(),
        observations: old.observations().to_vec(),
    };
    if facts.inputs.parameters.is_empty() {
        facts.inputs.parameters = new.inputs().parameters.clone();
    }
    if facts.inputs.references.is_empty() {
        facts.inputs.references.clone_from(&new.inputs().references);
    }
    for list in &new.inputs().lists {
        if !facts.inputs.lists.contains(list) {
            facts.inputs.lists.push(list.clone());
        }
    }
    for reference in &new.inputs().reference_evidence {
        if !facts.inputs.reference_evidence.contains(reference) {
            facts.inputs.reference_evidence.push(reference.clone());
        }
    }
    let mapping = new
        .observations()
        .iter()
        .map(|observation| facts.observe(observation.clone()))
        .collect::<Vec<_>>();
    for evidence in &new.inputs().position_evidence {
        let mut evidence = evidence.clone();
        evidence.observation = mapping[evidence.observation.0];
        if !facts.inputs.position_evidence.contains(&evidence) {
            facts.inputs.position_evidence.push(evidence);
        }
    }
    for &count in &new.inputs().cardinality_evidence {
        if !facts.inputs.cardinality_evidence.contains(&count) {
            facts.inputs.cardinality_evidence.push(count);
        }
    }
    for resource in &new.inputs().resource_evidence {
        if !facts.inputs.resource_evidence.contains(resource) {
            facts.inputs.resource_evidence.push(resource.clone());
        }
    }
    let resource_conflict = facts.normalize_resource_conflicts();
    let new_membership = remap_membership(new.membership().clone(), &mapping);
    let old_empty = old.observations().is_empty()
        && old.inputs().lists.is_empty()
        && old.inputs().reference_evidence.is_empty()
        && old.inputs().cardinality_evidence.is_empty()
        && membership_unknown(old.membership());
    let new_empty = new.observations().is_empty()
        && new.inputs().lists.is_empty()
        && new.inputs().reference_evidence.is_empty()
        && new.inputs().cardinality_evidence.is_empty()
        && membership_unknown(new.membership());
    let mut membership = if old_empty {
        new_membership
    } else if new_empty {
        old.membership().clone()
    } else {
        facts.reconcile(old.membership().clone(), new_membership)
    };
    if resource_conflict && matches!(membership, FormMembership::Complete { .. }) {
        let (cardinality, order, members) = membership_parts(membership);
        membership = FormMembership::Partial {
            cardinality,
            order,
            members,
        };
    }
    FormConstruction::try_new(facts.inputs, facts.observations, membership)
        .map_err(CodecError::malformed)
}

fn membership_unknown(membership: &FormMembership) -> bool {
    matches!(membership, FormMembership::Partial { cardinality: None, order: FormOrder::Unknown, members } if members.is_empty())
}

fn membership_evidence_conflicts(inputs: &FormInputs) -> bool {
    if inputs.cardinality_evidence.len() > 1 {
        return true;
    }
    let mut positions = HashMap::new();
    let mut members = HashMap::new();
    for evidence in &inputs.position_evidence {
        if positions
            .insert(evidence.ordinal, evidence.observation)
            .is_some_and(|old| old != evidence.observation)
            || members
                .insert(evidence.observation, evidence.ordinal)
                .is_some_and(|old| old != evidence.ordinal)
        {
            return true;
        }
    }
    false
}

fn remap_membership(
    mut membership: FormMembership,
    mapping: &[FormObservationIndex],
) -> FormMembership {
    match &mut membership {
        FormMembership::Complete { members, .. } => {
            for member in members {
                *member = mapping[member.0];
            }
        }
        FormMembership::Partial { members, .. } => {
            for member in members {
                member.observation = mapping[member.observation.0];
            }
        }
    }
    membership
}

fn membership_parts(membership: FormMembership) -> (Option<u32>, FormOrder, Vec<FormMember>) {
    match membership {
        FormMembership::Complete { members, order, .. } => {
            let cardinality = u32::try_from(members.len()).ok();
            let order = match order {
                FormKnownOrder::Source => FormOrder::Source,
                FormKnownOrder::Assumed => FormOrder::Assumed,
            };
            let members = members
                .into_iter()
                .enumerate()
                .map(|(ordinal, observation)| FormMember {
                    observation,
                    ordinal: u32::try_from(ordinal).ok(),
                })
                .collect();
            (cardinality, order, members)
        }
        FormMembership::Partial {
            cardinality,
            order,
            members,
        } => (cardinality, order, members),
    }
}

fn merge_memberships(left: FormMembership, right: FormMembership) -> FormMembership {
    if left == right {
        return left;
    }
    let (left_count, left_order, mut members) = membership_parts(left);
    let (right_count, right_order, more) = membership_parts(right);
    let cardinality = match (left_count, right_count) {
        (Some(a), Some(b)) if a != b => None,
        (Some(a), _) | (_, Some(a)) => Some(a),
        _ => None,
    };
    let mut conflicting = HashSet::new();
    for member in more {
        if let Some(existing) = members
            .iter_mut()
            .find(|existing| existing.observation == member.observation)
        {
            if existing.ordinal != member.ordinal {
                existing.ordinal = None;
                conflicting.insert(existing.observation);
            }
        } else {
            members.push(member);
        }
    }
    for index in 0..members.len() {
        if members[index].ordinal.is_some_and(|ordinal| {
            members
                .iter()
                .enumerate()
                .any(|(other, member)| other != index && member.ordinal == Some(ordinal))
        }) {
            conflicting.insert(members[index].observation);
        }
    }
    for member in &mut members {
        if conflicting.contains(&member.observation)
            || cardinality
                .is_some_and(|count| member.ordinal.is_some_and(|ordinal| ordinal >= count))
        {
            member.ordinal = None;
        }
    }
    let cardinality =
        cardinality.filter(|count| members.len() <= cadmpeg_core::decode::index_from_u32(*count));
    members.sort_by_key(|member| {
        (
            member.ordinal.is_none(),
            member.ordinal,
            member.observation.0,
        )
    });
    FormMembership::Partial {
        cardinality,
        order: if conflicting.is_empty() && left_order == right_order {
            left_order
        } else {
            FormOrder::Unknown
        },
        members,
    }
}

struct FormFacts {
    inputs: FormInputs,
    observations: Vec<FormCageObservation>,
}

struct FormSurface {
    path: FormInputPath,
    record: Option<u64>,
}

impl FormFacts {
    fn capture_membership(&mut self, membership: &FormMembership) {
        let (count, _, members) = membership_parts(membership.clone());
        if let Some(count) = count {
            if !self.inputs.cardinality_evidence.contains(&count) {
                self.inputs.cardinality_evidence.push(count);
            }
        }
        for member in members {
            let Some(ordinal) = member.ordinal else {
                continue;
            };
            let observation = &self.observations[member.observation.0];
            let sources = self
                .inputs
                .lists
                .iter()
                .filter(|list| {
                    list.members.iter().any(|native| {
                        native.ordinal == ordinal
                            && observation.paths.iter().any(|path| {
                                path.references
                                    .starts_with(list.source.references.as_slice())
                                    && path.references.contains(&native.reference)
                            })
                    })
                })
                .map(|list| Some(list.source.clone()))
                .collect::<Vec<_>>();
            for source in if sources.is_empty() {
                vec![None]
            } else {
                sources
            } {
                let evidence = FormPositionEvidence {
                    source,
                    observation: member.observation,
                    ordinal,
                };
                if !self.inputs.position_evidence.contains(&evidence) {
                    self.inputs.position_evidence.push(evidence);
                }
            }
        }
        self.inputs.cardinality_evidence.sort_unstable();
        self.inputs.position_evidence.sort_by(|a, b| {
            a.ordinal
                .cmp(&b.ordinal)
                .then(a.observation.0.cmp(&b.observation.0))
                .then(
                    a.source
                        .as_ref()
                        .map(|path| path.references.as_slice())
                        .cmp(&b.source.as_ref().map(|path| path.references.as_slice())),
                )
        });
    }
    fn reconcile(&mut self, left: FormMembership, right: FormMembership) -> FormMembership {
        if left != right {
            self.capture_membership(&left);
            self.capture_membership(&right);
        }
        let mut membership = merge_memberships(left, right);
        if self.inputs.cardinality_evidence.len() > 1 {
            if let FormMembership::Partial { cardinality, .. } = &mut membership {
                *cardinality = None;
            }
        }
        membership
    }
    fn reference(
        &mut self,
        ctx: &DecodeContext<'_>,
        scope: &DesignParameterScope,
        source: u32,
        slot: (FormReferenceRole, u32, Option<usize>),
        reference: &crate::bytes::Reference<String, String>,
        resource: Option<FormResource>,
    ) -> Result<(), CodecError> {
        use crate::bytes::Reference;
        let (inline_type_guid, target) = match reference {
            Reference::Null => return Ok(()),
            Reference::Local {
                target,
                inline_type_guid,
            } => (
                inline_type_guid.clone(),
                FormNativeReferenceTarget::Local {
                    target: record_id(scope, target)?,
                },
            ),
            Reference::CrossSegment {
                target,
                inline_type_guid,
                segment,
            } => (
                inline_type_guid.clone(),
                FormNativeReferenceTarget::Segment {
                    target: *target,
                    segment: *segment,
                },
            ),
            Reference::CrossDocument {
                target,
                inline_type_guid,
                segment,
                asset_guid,
                target_type_guid,
                link_name,
                version,
            } => (
                inline_type_guid.clone(),
                FormNativeReferenceTarget::Document {
                    target: *target,
                    segment: *segment,
                    asset_guid: asset_guid.clone(),
                    target_type_guid: target_type_guid.clone(),
                    link_name: link_name.clone(),
                    version: version.as_ref().map(|(key, urn)| FormExternalVersion {
                        key: key.clone(),
                        urn: urn.clone(),
                    }),
                },
            ),
        };
        let evidence = FormNativeReference {
            source: record_id(scope, source)?,
            role: slot.0,
            ordinal: slot.1,
            run: slot.2,
            inline_type_guid,
            target,
            resource,
        };
        if !self.inputs.reference_evidence.contains(&evidence) {
            let mut lengths = [0; 6];
            lengths[0] = evidence.inline_type_guid.as_deref().map_or(0, str::len);
            if let FormNativeReferenceTarget::Document {
                asset_guid,
                target_type_guid,
                link_name,
                version,
                ..
            } = &evidence.target
            {
                lengths[1] = asset_guid.len();
                lengths[2] = target_type_guid.len();
                lengths[3] = link_name.len();
                if let Some(version) = version {
                    lengths[4] = version.key.len();
                    lengths[5] = version.urn.len();
                }
            }
            let text_bytes = lengths
                .into_iter()
                .try_fold(0usize, usize::checked_add)
                .ok_or_else(|| CodecError::malformed("Form reference text length overflows"))?;
            ctx.retain_bytes(
                cadmpeg_core::decode::u64_from_index(text_bytes),
                "Form reference text",
            )?;
            self.inputs.reference_evidence.push(evidence);
        }
        Ok(())
    }
    fn remember_resource(&mut self, resource: FormResource) {
        if !self.inputs.resource_evidence.contains(&resource) {
            self.inputs.resource_evidence.push(resource);
            self.inputs
                .resource_evidence
                .sort_by(|a, b| a.entry.cmp(&b.entry).then(a.source.cmp(&b.source)));
        }
    }
    fn normalize_resource_conflicts(&mut self) -> bool {
        let mut groups: HashMap<cadmpeg_core::text::NonBlankString, Vec<usize>> = HashMap::new();
        for (index, observation) in self.observations.iter().enumerate() {
            if let FormCageBinding::Decoded {
                resource: Some(resource),
                ..
            } = &observation.binding
            {
                groups
                    .entry(resource.entry.clone())
                    .or_default()
                    .push(index);
            }
        }
        let mut conflicts = groups
            .into_iter()
            .filter(|(_, indices)| indices.len() > 1)
            .collect::<Vec<_>>();
        conflicts.sort_by(|a, b| a.0.cmp(&b.0));
        let found = !conflicts.is_empty();
        for (_, indices) in conflicts {
            let mut resources = Vec::new();
            let mut paths = Vec::new();
            for index in indices {
                let observation = &mut self.observations[index];
                if let FormCageBinding::Decoded { resource, .. } = &mut observation.binding {
                    if let Some(resource) = resource.take() {
                        resources.push(resource);
                    }
                }
                for path in &observation.paths {
                    if !paths.contains(path) {
                        paths.push(path.clone());
                    }
                }
            }
            paths.sort_by(|a, b| a.references.as_slice().cmp(b.references.as_slice()));
            if let Some(resource) = resources.first().cloned() {
                for item in resources {
                    self.remember_resource(item);
                }
                self.observe(FormCageObservation {
                    paths,
                    binding: FormCageBinding::Undecoded {
                        resource,
                        availability: FormResourceAvailability::Ambiguous,
                    },
                });
            }
        }
        found
    }
    fn observe(&mut self, mut observation: FormCageObservation) -> FormObservationIndex {
        if let FormCageBinding::Decoded {
            resource: Some(resource),
            ..
        } = &observation.binding
        {
            if self.observations.iter().any(|old| matches!(&old.binding, FormCageBinding::Undecoded { resource: existing, availability: FormResourceAvailability::Ambiguous } if existing.entry == resource.entry)) {
                self.remember_resource(resource.clone());
                if let FormCageBinding::Decoded { resource, .. } = &mut observation.binding { *resource = None; }
            }
        }
        if let FormCageBinding::Undecoded {
            resource,
            availability: FormResourceAvailability::Ambiguous,
        } = &observation.binding
        {
            let mut displaced = Vec::new();
            for old in &mut self.observations {
                if let FormCageBinding::Decoded {
                    resource: old_resource,
                    ..
                } = &mut old.binding
                {
                    if old_resource
                        .as_ref()
                        .is_some_and(|old| old.entry == resource.entry)
                    {
                        if let Some(old) = old_resource.take() {
                            displaced.push(old);
                        }
                    }
                }
            }
            for resource in displaced {
                self.remember_resource(resource);
            }
        }
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
                    (
                        FormCageBinding::Decoded {
                            resource: Some(a), ..
                        },
                        FormCageBinding::Undecoded { resource: b, .. },
                    )
                    | (
                        FormCageBinding::Undecoded { resource: a, .. },
                        FormCageBinding::Decoded {
                            resource: Some(b), ..
                        },
                    ) => a.entry == b.entry,
                    (
                        FormCageBinding::Undecoded { resource: a, .. },
                        FormCageBinding::Undecoded { resource: b, .. },
                    ) => a.entry == b.entry,
                    _ => old.binding == observation.binding,
                })
        {
            let old_resource = match &self.observations[index].binding {
                FormCageBinding::Decoded { resource, .. } => resource.clone(),
                FormCageBinding::Undecoded { resource, .. } => Some(resource.clone()),
                FormCageBinding::Unresolved {} => None,
            };
            let new_resource = match &observation.binding {
                FormCageBinding::Decoded { resource, .. } => resource.clone(),
                FormCageBinding::Undecoded { resource, .. } => Some(resource.clone()),
                FormCageBinding::Unresolved {} => None,
            };
            if let (Some(old), Some(new)) = (&old_resource, &new_resource) {
                if old != new {
                    for resource in [old, new] {
                        self.remember_resource(resource.clone());
                    }
                }
            }
            if matches!(
                self.observations[index].binding,
                FormCageBinding::Undecoded { .. }
            ) && matches!(observation.binding, FormCageBinding::Decoded { .. })
            {
                self.observations[index].binding = observation.binding.clone();
            }
            if let (
                FormCageBinding::Undecoded {
                    availability: old, ..
                },
                FormCageBinding::Undecoded {
                    availability: new, ..
                },
            ) = (&mut self.observations[index].binding, &observation.binding)
            {
                *old = match (*old, *new) {
                    (a, b) if a == b => a,
                    (FormResourceAvailability::Unknown, value)
                    | (value, FormResourceAvailability::Unknown) => value,
                    _ => FormResourceAvailability::Ambiguous,
                };
            }
            if let (
                FormCageBinding::Decoded { resource: old, .. },
                FormCageBinding::Decoded { resource: new, .. },
            ) = (&mut self.observations[index].binding, &observation.binding)
            {
                if old.is_none()
                    || matches!((&*old, new), (Some(a), Some(b)) if (&b.entry, &b.source) < (&a.entry, &a.source))
                {
                    old.clone_from(new);
                }
            }
            for path in observation.paths {
                if !self.observations[index].paths.contains(&path) {
                    self.observations[index].paths.push(path);
                }
            }
            self.observations[index]
                .paths
                .sort_by(|a, b| a.references.as_slice().cmp(b.references.as_slice()));
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
        losses: &mut Vec<LossNote>,
    ) -> Result<(), CodecError> {
        for &group in source.scope.reference_members().values() {
            if types.selection_groups.get(&group) != Some(&0) {
                continue;
            }
            if let Some(run) = form_tracking_members(
                ctx,
                source.bytes,
                source.records,
                group,
                source.scope.record_index,
            )? {
                if let Some(owner) = &run.owner {
                    self.reference(
                        ctx,
                        source.scope,
                        group,
                        (FormReferenceRole::TrackingOwner, 0, None),
                        owner,
                        None,
                    )?;
                    match owner.local() {
                        None if matches!(owner, FormReference::Null) => note(
                            losses,
                            F3dLossCode::FeatureFormTrackingOwnerConflict,
                            &source.scope.id,
                            "a tracking group has a null owner",
                        ),
                        None => note(
                            losses,
                            F3dLossCode::FeatureFormForeignReferenceUnresolved,
                            &source.scope.id,
                            "a tracking owner names another namespace",
                        ),
                        Some((target, inline)) => {
                            if target != u64::from(source.scope.record_index) {
                                note(
                                    losses,
                                    F3dLossCode::FeatureFormTrackingOwnerConflict,
                                    &source.scope.id,
                                    "a tracking group names a different owner",
                                );
                            }
                            if inline.is_some_and(|guid| !types.type_matches(target, guid)) {
                                note(
                                    losses,
                                    F3dLossCode::FeatureFormReferenceTypeConflict,
                                    &source.scope.id,
                                    "a tracking owner type conflicts with its target registration",
                                );
                            }
                        }
                    }
                }
                let members = run.members;
                let mut local =
                    ctx.alloc_filled(members.len(), None, "Form tracking local targets")?;
                for (ordinal, reference) in members.iter().enumerate() {
                    let Some(reference) = reference else {
                        continue;
                    };
                    self.reference(
                        ctx,
                        source.scope,
                        group,
                        (
                            FormReferenceRole::TrackingMember,
                            u32::try_from(ordinal).map_err(CodecError::malformed)?,
                            None,
                        ),
                        reference,
                        None,
                    )?;
                    let target = match reference.local() {
                        None => {
                            note(
                                losses,
                                F3dLossCode::FeatureFormForeignReferenceUnresolved,
                                &source.scope.id,
                                "a tracking member names another namespace",
                            );
                            None
                        }
                        Some((target, inline)) => {
                            let valid = inline.is_none_or(|guid| {
                                guid.eq_ignore_ascii_case("C46D3EEB-40E1-43FA-83DC-9CB204335417")
                                    && types.type_matches(target, guid)
                            });
                            if !valid {
                                note(losses, F3dLossCode::FeatureFormReferenceTypeConflict, &source.scope.id, "a tracking member type conflicts with its target role or registration");
                            }
                            valid.then_some(target)
                        }
                    };
                    local[ordinal] = target;
                }
                self.list(source.scope, group, FormListRole::TrackingSelection, &local)?;
            }
        }
        Ok(())
    }
    fn surface(
        &mut self,
        ctx: &DecodeContext<'_>,
        scan: &ContainerScan,
        source: FormSource<'_>,
        surface: FormSurface,
        serializers: &FormCageSerializers,
        losses: &mut Vec<LossNote>,
    ) -> Result<Vec<FormObservationIndex>, CodecError> {
        let FormSurface { path, record } = surface;
        let FormSource { scope, cages, .. } = source;
        let entries = record.and_then(|surface| serializers.entries.get(&surface));
        let Some(entries) = entries else {
            return Ok(vec![self.observe(FormCageObservation {
                paths: vec![path],
                binding: FormCageBinding::Unresolved {},
            })]);
        };
        if entries.len() > 1 || entries.iter().any(|entry| entry.ambiguous) {
            note(
                losses,
                F3dLossCode::FeatureFormCageBindingAmbiguous,
                &scope.id,
                "surface has multiple serializers",
            );
        }
        let mut alternatives = Vec::new();
        for serializer in entries {
            let mut joined_path = path.clone();
            joined_path
                .references
                .push(record_id(scope, serializer.record)?);
            let path = if serializer.valid {
                joined_path
            } else {
                note(
                    losses,
                    F3dLossCode::FeatureFormReferenceTypeConflict,
                    &scope.id,
                    "a serializer reference type conflicts with the surface role",
                );
                alternatives.push(self.observe(FormCageObservation {
                    paths: vec![path.clone()],
                    binding: FormCageBinding::Unresolved {},
                }));
                FormInputPath {
                    references: NonEmptyMembers::try_from(vec![record_id(
                        scope,
                        serializer.record,
                    )?])
                    .map_err(CodecError::malformed)?,
                }
            };
            if serializer.ambiguous {
                alternatives.push(self.observe(FormCageObservation {
                    paths: vec![path.clone()],
                    binding: FormCageBinding::Unresolved {},
                }));
            }
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
            let reference = serializer
                .reference
                .as_ref()
                .ok_or_else(|| CodecError::malformed("Form serializer target is unavailable"))?;
            self.reference(
                ctx,
                scope,
                serializer.record,
                (
                    FormReferenceRole::SerializerSurface,
                    0,
                    Some(serializer.offset),
                ),
                reference,
                Some(resource.clone()),
            )?;
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
                let observation = self.observe(FormCageObservation {
                    paths: vec![path.clone()],
                    binding: FormCageBinding::Decoded {
                        cage: matches[0].id.clone(),
                        resource: Some(resource),
                    },
                });
                if serializer.valid {
                    alternatives.push(observation);
                }
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
                let observation = self.observe(FormCageObservation {
                    paths: vec![path.clone()],
                    binding: FormCageBinding::Undecoded {
                        resource,
                        availability,
                    },
                });
                if serializer.valid {
                    alternatives.push(observation);
                }
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
            slots.is_empty()
                || !slots.iter().all(|index| {
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

fn retain_serializer_candidate(
    ctx: &DecodeContext<'_>,
    scan: &ContainerScan,
    source: FormSource<'_>,
    serializer: &FormSerializer,
    facts: &mut FormFacts,
) -> Result<(), CodecError> {
    let FormSource { scope, cages, .. } = source;
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
    if !facts.inputs.resource_evidence.contains(&resource) {
        facts.inputs.resource_evidence.push(resource.clone());
    }
    if let Some(reference) = &serializer.reference {
        facts.reference(
            ctx,
            scope,
            serializer.record,
            (
                FormReferenceRole::SerializerSurface,
                0,
                Some(serializer.offset),
            ),
            reference,
            Some(resource.clone()),
        )?;
    }
    let mut path = FormInputPath {
        references: NonEmptyMembers::one(record_id(scope, serializer.record)?),
    };
    if let Some((target, _)) = serializer.reference.as_ref().and_then(FormReference::local) {
        path.references.push(record_id(scope, target)?);
    }
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
        facts.observe(FormCageObservation {
            paths: vec![path],
            binding: FormCageBinding::Decoded {
                cage: matches[0].id.clone(),
                resource: Some(resource),
            },
        });
    } else {
        let availability = match entry {
            None => FormResourceAvailability::Unknown,
            Some(_) if matches.len() > 1 => FormResourceAvailability::Ambiguous,
            Some(entry) if scan.has_entry(&entry) => FormResourceAvailability::ActiveUndecoded,
            Some(_) => FormResourceAvailability::Inactive,
        };
        facts.observe(FormCageObservation {
            paths: vec![path.clone()],
            binding: FormCageBinding::Undecoded {
                resource,
                availability,
            },
        });
        for cage in matches {
            facts.observe(FormCageObservation {
                paths: vec![path.clone()],
                binding: FormCageBinding::Decoded {
                    cage: cage.id.clone(),
                    resource: None,
                },
            });
        }
    }
    facts
        .inputs
        .resource_evidence
        .sort_by(|a, b| a.entry.cmp(&b.entry).then(a.source.cmp(&b.source)));
    Ok(())
}

fn retain_unbound_serializers(
    ctx: &DecodeContext<'_>,
    scan: &ContainerScan,
    source: FormSource<'_>,
    serializers: &FormCageSerializers,
    facts: &mut FormFacts,
    losses: &mut Vec<LossNote>,
) -> Result<(), CodecError> {
    for serializer in &serializers.unbound {
        retain_serializer_candidate(ctx, scan, source, serializer, facts)?;
        let foreign = serializer
            .reference
            .as_ref()
            .is_some_and(|reference| reference.local().is_none());
        note(
            losses,
            if foreign {
                F3dLossCode::FeatureFormForeignReferenceUnresolved
            } else {
                F3dLossCode::FeatureFormSerializerReferenceUnresolved
            },
            &source.scope.id,
            if foreign {
                "a serializer reference names another namespace"
            } else {
                "a serializer reference or frame remains unresolved"
            },
        );
    }
    let mut ambiguous = serializers
        .entries
        .values()
        .flatten()
        .filter(|entry| entry.ambiguous)
        .collect::<Vec<_>>();
    ambiguous.sort_by_key(|entry| entry.offset);
    for serializer in ambiguous {
        retain_serializer_candidate(ctx, scan, source, serializer, facts)?;
        note(
            losses,
            F3dLossCode::FeatureFormCageBindingAmbiguous,
            &source.scope.id,
            "a serializer identity has multiple frames",
        );
    }
    Ok(())
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
    let mut serializers = form_cage_serializers(ctx, bytes, records)?;
    let types = match crate::design::decode::meta::metadata_for_bulk_stream(scan, stream) {
        Ok(Some(meta)) => Some(FormRecordTypes::of(&meta)),
        Ok(None) | Err(CodecError::Malformed(_)) => None,
        Err(error) => return Err(error),
    };
    if let Some(types) = &types {
        serializers.validate_types(types);
        facts.tracking(ctx, source, types, losses)?;
    }
    retain_unbound_serializers(ctx, scan, source, &serializers, facts, losses)?;
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
                &objects.iter().map(|entry| entry.object).collect::<Vec<_>>(),
            )?;
            let closed = objects.iter().all(|entry| entry.valid);
            let slots = objects
                .into_iter()
                .map(|entry| {
                    let Some(object) = entry.object else {
                        return Ok(Vec::new());
                    };
                    let surfaces = u32::try_from(object)
                        .ok()
                        .map(|object| form_class_325_cage_surface(bytes, records, object))
                        .unwrap_or_default();
                    if surfaces.len() > 1 {
                        note(
                            losses,
                            F3dLossCode::FeatureFormCageBindingAmbiguous,
                            &scope.id,
                            "a catalog object has multiple surface references",
                        );
                    }
                    let mut alternatives = Vec::new();
                    if surfaces.is_empty() {
                        alternatives.extend(facts.surface(
                            ctx,
                            scan,
                            source,
                            FormSurface {
                                path: input_path(scope, &[object])?,
                                record: None,
                            },
                            &serializers,
                            losses,
                        )?);
                    } else {
                        for surface in surfaces {
                            alternatives.extend(facts.surface(
                                ctx,
                                scan,
                                source,
                                FormSurface {
                                    path: input_path(scope, &[object, u64::from(surface)])?,
                                    record: Some(u64::from(surface)),
                                },
                                &serializers,
                                losses,
                            )?);
                        }
                    }
                    for index in &alternatives {
                        if let Some(companion) = entry.companion {
                            let companion_path = input_path(scope, &[companion])?;
                            if !facts.observations[index.0].paths.contains(&companion_path) {
                                facts.observations[index.0].paths.push(companion_path);
                            }
                        }
                    }
                    Ok::<_, CodecError>(if entry.valid {
                        alternatives
                    } else {
                        Vec::new()
                    })
                })
                .collect::<Result<Vec<_>, _>>()?;
            let slots = active_slots(facts, slots);
            return facts.membership(
                slots,
                None,
                FormOrder::Source,
                FormKnownOrder::Source,
                closed,
            );
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
                    ctx,
                    scan,
                    source,
                    FormSurface {
                        path: input_path(scope, &[surface])?,
                        record: Some(surface),
                    },
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
                    ctx,
                    scan,
                    source,
                    FormSurface {
                        path: input_path(scope, &path)?,
                        record: surface,
                    },
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
        ordinary = Some(match ordinary {
            Some(previous) => facts.reconcile(previous, membership),
            None => membership,
        });
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
    retain_unbound_serializers(ctx, scan, source, &serializers, facts, losses)?;
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
        let alternatives = form_body_references(ctx, bytes, records, body)?;
        if alternatives.is_empty() {
            closed = false;
            continue;
        }
        let owned = alternatives.iter().any(|run| {
            run.references[0]
                .as_ref()
                .and_then(FormReference::local)
                .is_some_and(|(target, _)| target == u64::from(scope.record_index))
        });
        let body_id = record_id(scope, body)?;
        let linked = facts.observations.iter().any(|observation| {
            observation
                .paths
                .iter()
                .any(|path| path.references.contains(&body_id))
        });
        if !owned && !linked {
            let unresolved_owner = alternatives.iter().any(|run| {
                run.references[0]
                    .as_ref()
                    .and_then(FormReference::local)
                    .is_none()
            });
            if !unresolved_owner {
                continue;
            }
            closed = false;
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
        for run in alternatives {
            let references = run.references;
            let mut local = [None; FORM_BODY_REFERENCE_COUNT];
            for (ordinal, reference) in references.iter().enumerate() {
                let Some(reference) = reference else {
                    continue;
                };
                facts.reference(
                    ctx,
                    scope,
                    body,
                    (
                        FormReferenceRole::Body,
                        u32::try_from(ordinal).map_err(CodecError::malformed)?,
                        Some(run.offset),
                    ),
                    reference,
                    None,
                )?;
                match reference.local() {
                    None => note(
                        losses,
                        F3dLossCode::FeatureFormForeignReferenceUnresolved,
                        &scope.id,
                        "a body reference names another namespace",
                    ),
                    Some((target, inline)) => {
                        let expected = match ordinal {
                            1 => Some(crate::design::presentation::BREP_CONTAINER_TYPE_GUID),
                            2 => Some(FORM_SURFACE_TYPE_GUID),
                            3 => Some(crate::design::presentation::BODY_SCENE_NODE_TYPE_GUID),
                            _ => None,
                        };
                        let valid_type = inline.is_none_or(|guid| {
                            expected.is_none_or(|expected| guid.eq_ignore_ascii_case(expected))
                                && types.type_matches(target, guid)
                        });
                        if valid_type {
                            local[ordinal] = Some(target);
                        } else {
                            note(losses, F3dLossCode::FeatureFormReferenceTypeConflict, &scope.id, "an inline reference type conflicts with its target role or registration");
                        }
                    }
                }
            }
            let raw_owner = references[0]
                .as_ref()
                .and_then(FormReference::local)
                .map(|(target, _)| target);
            if raw_owner.is_some_and(|owner| owner != u64::from(scope.record_index)) {
                continue;
            }
            let owner_proven = local[0] == Some(u64::from(scope.record_index));
            if !owner_proven {
                closed = false;
            }
            if local.iter().any(Option::is_none) {
                note(
                    losses,
                    F3dLossCode::FeatureFormBodyReferencesPartialAssumed,
                    &scope.id,
                    "retains local references from an incomplete body envelope",
                );
            }
            let base = if owner_proven {
                input_path(scope, &[u64::from(body)])?
            } else {
                FormInputPath {
                    references: NonEmptyMembers::one(record_id(scope, body)?),
                }
            };
            let mut path = base.clone();
            if let Some(surface) = local[2] {
                path.references.push(record_id(scope, surface)?);
            }
            if local[2].is_none() {
                if let Some((target, _)) = references[2].as_ref().and_then(FormReference::local) {
                    if let Some(entries) = serializers.entries.get(&target) {
                        for serializer in entries {
                            retain_serializer_candidate(ctx, scan, source, serializer, facts)?;
                        }
                    }
                }
            }
            let observed = facts.surface(
                ctx,
                scan,
                source,
                FormSurface {
                    path,
                    record: local[2],
                },
                &serializers,
                losses,
            )?;
            for index in &observed {
                for target in local
                    .iter()
                    .enumerate()
                    .filter_map(|(ordinal, &target)| (ordinal != 2).then_some(target).flatten())
                {
                    let mut path = base.clone();
                    path.references.push(record_id(scope, target)?);
                    if !facts.observations[index.0].paths.contains(&path) {
                        facts.observations[index.0].paths.push(path);
                    }
                }
            }
            let valid = owner_proven
                && local[1].is_some_and(|target| types.body_containers.contains(&target))
                && local[2].is_some_and(|target| types.surfaces.contains(&target))
                && local[3].is_some_and(|target| types.scene_nodes.contains(&target));
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
    let body = facts.membership(slots, cardinality, order, FormKnownOrder::Source, closed)?;
    Ok(match ordinary {
        Some(ordinary) => facts.reconcile(ordinary, body),
        None => body,
    })
}

struct FormTrackingRun {
    owner: Option<FormReference>,
    members: Vec<Option<FormReference>>,
}

fn form_tracking_members(
    ctx: &DecodeContext<'_>,
    bytes: &[u8],
    records: &IndexedRecordOffsets,
    group: u32,
    owner: u32,
) -> Result<Option<FormTrackingRun>, CodecError> {
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
            crate::bytes::take_reference(frame, &mut at)?;
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
    let mut owner_at = selection_prefix::OWNER_MARKER;
    let owner_reference = take_form_reference(ctx, frame, &mut owner_at)?;
    let mut members = ctx.alloc_filled(count_usize, None, "Form tracking members")?;
    for member in &mut members {
        if fixed_slots {
            let end = at + 11;
            let mut cursor = at;
            *member = take_form_reference(ctx, &frame[..end], &mut cursor)?
                .filter(|_| cursor == end)
                .filter(|reference| !matches!(reference, crate::bytes::Reference::Null));
            at = end;
            continue;
        }
        let Some(reference) = take_form_reference(ctx, frame, &mut at)? else {
            break;
        };
        if !matches!(reference, crate::bytes::Reference::Null) {
            *member = Some(reference);
        }
    }
    Ok(Some(FormTrackingRun {
        owner: owner_reference,
        members,
    }))
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
    registered: HashMap<u64, Option<usize>>,
    type_guids: Vec<String>,
}

impl FormRecordTypes {
    fn of(meta: &crate::metastream::MetaStream) -> Self {
        use crate::design::presentation::{BODY_SCENE_NODE_TYPE_GUID, BREP_CONTAINER_TYPE_GUID};
        let mut types = Self {
            type_guids: meta
                .types
                .iter()
                .map(|kind| kind.type_guid.as_str().to_owned())
                .collect(),
            ..Self::default()
        };
        for (index, design_type) in meta.types.iter().enumerate() {
            let guid = design_type.type_guid.as_str();
            let version = design_type.version;
            for entity in design_type.entities.values().copied() {
                types
                    .registered
                    .entry(entity)
                    .and_modify(|old| {
                        if old.is_some_and(|previous| {
                            !meta.types[previous]
                                .type_guid
                                .as_str()
                                .eq_ignore_ascii_case(guid)
                        }) {
                            *old = None;
                        }
                    })
                    .or_insert(Some(index));
            }
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
    fn type_matches(&self, target: u64, guid: &str) -> bool {
        self.registered
            .get(&target)
            .and_then(|index| *index)
            .and_then(|index| self.type_guids.get(index))
            .is_some_and(|registered| registered.eq_ignore_ascii_case(guid))
    }
}

type FormReference = crate::bytes::Reference<String, String>;

fn take_form_reference(
    ctx: &DecodeContext<'_>,
    bytes: &[u8],
    cursor: &mut usize,
) -> Result<Option<FormReference>, CodecError> {
    if let Some(limit) = ctx.resource_refusal() {
        return Err(CodecError::ResourceLimit(limit));
    }
    Ok(crate::bytes::take_owned_reference(bytes, cursor))
}

struct FormBodyRun {
    offset: usize,
    references: [Option<FormReference>; FORM_BODY_REFERENCE_COUNT],
}

fn form_body_references(
    ctx: &DecodeContext<'_>,
    bytes: &[u8],
    records: &IndexedRecordOffsets,
    body: u32,
) -> Result<Vec<FormBodyRun>, CodecError> {
    let Some((start, paired)) = one_indexed_frame(records, body) else {
        return Ok(Vec::new());
    };
    let Some(frame) = bytes.get(start..paired) else {
        return Ok(Vec::new());
    };
    let mut runs = Vec::new();
    for at in 0..frame.len() {
        let Some(mut cursor) =
            crate::design::presentation::physical_material_envelope_end(frame, at)
        else {
            continue;
        };
        let Some((_, after_asset)) = crate::bytes::lp_utf16_bounded_view(frame, at, 36..=36) else {
            continue;
        };
        let Some((_, after_library)) =
            crate::bytes::lp_utf16_bounded_view(frame, after_asset, 0..=36)
        else {
            continue;
        };
        let Some((_, after_label)) =
            crate::bytes::lp_utf16_bounded_view(frame, after_library, 0..=256)
        else {
            continue;
        };
        let Some((token, _)) = crate::bytes::lp_utf16_bounded_view(frame, after_label, 1..=256)
        else {
            continue;
        };
        if token.to_string().contains("_physmat_aspects") {
            continue;
        }
        let mut references = std::array::from_fn(|_| None);
        for reference in &mut references {
            let Some(parsed) = take_form_reference(ctx, frame, &mut cursor)? else {
                break;
            };
            if !matches!(parsed, crate::bytes::Reference::Null) {
                *reference = Some(parsed);
            }
        }
        if references.iter().any(Option::is_some) {
            runs.push(FormBodyRun {
                offset: at,
                references,
            });
        }
    }
    Ok(runs)
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

struct FormLegacySlot {
    object: Option<u64>,
    companion: Option<u64>,
    valid: bool,
}

fn form_class_325_cage_objects(
    bytes: &[u8],
    records: &IndexedRecordOffsets,
    scope_record_index: u32,
    mut owner_record_indices: impl Iterator<Item = u32>,
) -> Result<Option<Vec<FormLegacySlot>>, CodecError> {
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
        let mut objects: Vec<FormLegacySlot> = Vec::new();
        let mut seen_type_discriminators: [Option<usize>; CAGE_COUNT] = [None; CAGE_COUNT];
        for ordinal in 0..count {
            let entry = start
                .checked_add(form_class_325_cage_table::CAGE_ENTRIES)?
                .checked_add(form_class_325_cage_entry::LEN.checked_mul(ordinal)?)?;
            let object = (bytes.get(entry + form_class_325_cage_entry::CAGE_OBJECT_MARKER)
                == Some(&1)
                && bytes.get(
                    entry + form_class_325_cage_entry::CAGE_OBJECT_ZERO
                        ..entry + form_class_325_cage_entry::TYPE_DISCRIMINATOR,
                ) == Some(&[0, 0]))
            .then(|| {
                View::u64_le_at(
                    bytes,
                    entry + form_class_325_cage_entry::CAGE_OBJECT_RECORD_INDEX,
                )
            })
            .flatten();
            let companion = (bytes.get(entry + form_class_325_cage_entry::COMPANION_MARKER)
                == Some(&1)
                && bytes.get(
                    entry + form_class_325_cage_entry::COMPANION_ZERO
                        ..entry + form_class_325_cage_entry::LEN,
                ) == Some(&[0, 0]))
            .then(|| {
                View::u64_le_at(
                    bytes,
                    entry + form_class_325_cage_entry::COMPANION_RECORD_INDEX,
                )
            })
            .flatten();
            let type_slot =
                View::u64_le_at(bytes, entry + form_class_325_cage_entry::TYPE_DISCRIMINATOR)
                    .and_then(|value| u32::try_from(value).ok())
                    .and_then(|value| value.checked_sub(TYPE_DISCRIMINATOR_FIRST))
                    .and_then(|value| usize::try_from(value).ok())
                    .filter(|&value| value < CAGE_COUNT);
            let type_valid = type_slot.is_some_and(|slot| match seen_type_discriminators[slot] {
                Some(previous) => {
                    objects[previous].valid = false;
                    false
                }
                None => {
                    seen_type_discriminators[slot] = Some(ordinal);
                    true
                }
            });
            let object_valid = object
                .and_then(|value| u32::try_from(value).ok())
                .is_some_and(|object| {
                    let mut frames = records
                        .frames(object)
                        .filter(|(_, paired)| bytes.get(paired + 4..paired + 7) == Some(b"258"));
                    frames
                        .next()
                        .is_some_and(|(at, _)| bytes.get(at + 4..at + 7) == Some(b"289"))
                        && frames.next().is_none()
                });
            let companion_valid = companion
                .and_then(|value| u32::try_from(value).ok())
                .is_some_and(|companion| {
                    records
                        .offsets(companion)
                        .first()
                        .is_some_and(|at| bytes.get(*at + 4..*at + 7) == Some(b"273"))
                });
            objects.push(FormLegacySlot {
                object,
                companion,
                valid: type_valid && object_valid && companion_valid,
            });
        }
        Some(Ok(objects))
    })();
    parsed.transpose()
}

fn form_class_325_cage_surface(
    bytes: &[u8],
    records: &IndexedRecordOffsets,
    object_record: u32,
) -> Vec<u32> {
    let mut frames = records
        .frames(object_record)
        .filter(|(_, paired)| bytes.get(paired + 4..paired + 7) == Some(b"258"));
    let Some((start, paired)) = frames.next() else {
        return Vec::new();
    };
    if frames.next().is_some() {
        return Vec::new();
    }
    if bytes.get(start + 4..start + 7) != Some(b"289") {
        return Vec::new();
    }
    let mut surfaces = Vec::new();
    for at in start + 11..paired {
        if bytes.get(at) != Some(&1) {
            continue;
        }
        let Some(target) = View::u32_le_at(bytes, at + 1) else {
            continue;
        };
        let [target_at] = records.offsets(target) else {
            continue;
        };
        if bytes.get(target_at + 4..target_at + 7) == Some(b"310") && !surfaces.contains(&target) {
            surfaces.push(target);
        }
    }
    surfaces
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
    let mut seen = HashSet::new();
    for record in references {
        if !seen.insert(record) {
            continue;
        }
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
    offset: usize,
    name: String,
    ambiguous: bool,
    reference: Option<FormReference>,
    valid: bool,
}
struct FormCageSerializers {
    ordered: Vec<u64>,
    entries: HashMap<u64, Vec<FormSerializer>>,
    unbound: Vec<FormSerializer>,
}

impl FormCageSerializers {
    fn validate_types(&mut self, types: &FormRecordTypes) {
        for (&surface, entries) in &mut self.entries {
            for entry in entries {
                if let Some((_, Some(guid))) =
                    entry.reference.as_ref().and_then(FormReference::local)
                {
                    entry.valid &= types.type_matches(surface, guid);
                }
            }
        }
    }

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
        .flat_map(|serializer| records.offsets(*serializer).iter().copied())
        .collect::<Vec<_>>();
    offsets.sort_unstable();
    let mut serializers = collect_form_cage_serializers(
        ctx,
        bytes,
        records,
        offsets
            .into_iter()
            .map(|offset| (offset, FormSerializerFrame::Ordinary)),
    )?;
    serializers.validate_types(types);
    for entry in serializers.entries.values_mut().flatten() {
        let offsets = records.offsets(entry.record);
        entry.ambiguous = offsets.first().is_some_and(|first| *first < entry.offset)
            || offsets
                .iter()
                .filter(|&&offset| {
                    next_indexed_record_offset(bytes, offset + 1)
                        .and_then(|next| next.checked_sub(offset))
                        .is_some_and(|length| {
                            length > form_serializer_frame_132::ENTRY_NAME_LENGTH + 4 + 2
                        })
                })
                .count()
                > 1;
    }
    Ok(serializers)
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
    let mut unbound = Vec::new();
    for (offset, frame) in frames {
        let Some((reference, entry_name, frame_valid)) =
            form_serializer_entry(ctx, bytes, records, offset, frame)?
        else {
            continue;
        };
        let Some(record) = View::u32_le_at(bytes, offset + 7) else {
            continue;
        };
        let local = reference.as_ref().and_then(FormReference::local);
        let Some((surface, inline)) = local.filter(|_| frame_valid) else {
            unbound.push(FormSerializer {
                record,
                offset,
                name: entry_name,
                ambiguous: false,
                reference,
                valid: false,
            });
            continue;
        };
        let valid = inline.is_none_or(|guid| guid.eq_ignore_ascii_case(FORM_SURFACE_TYPE_GUID));
        if !entries.contains_key(&surface) {
            ordered.push(surface);
        }
        entries.entry(surface).or_default().push(FormSerializer {
            record,
            offset,
            name: entry_name,
            ambiguous: false,
            reference,
            valid,
        });
    }
    Ok(FormCageSerializers {
        ordered,
        entries,
        unbound,
    })
}

fn form_serializer_entry(
    ctx: &DecodeContext<'_>,
    bytes: &[u8],
    records: &IndexedRecordOffsets,
    offset: usize,
    frame: FormSerializerFrame,
) -> Result<Option<(Option<FormReference>, String, bool)>, CodecError> {
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
    if is_class_335
        && (next != offset + form_serializer_frame_132::LEN
            || bytes.get(next + 4..next + 7) != Some(b"331"))
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
    if after_name > next {
        return Ok(None);
    }
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
    {
        return Ok(None);
    }
    let mut cursor = after_name;
    let Some(frame) = bytes.get(..next) else {
        return Ok(None);
    };
    let reference = take_form_reference(ctx, frame, &mut cursor)?
        .filter(|reference| !matches!(reference, crate::bytes::Reference::Null));
    let valid = reference.as_ref().is_some_and(|reference| {
        if is_class_335 {
            bytes
                .get(cursor..next)
                .is_some_and(|tail| tail.iter().all(|byte| *byte == 0))
                && reference
                    .local()
                    .and_then(|(surface, _)| u32::try_from(surface).ok())
                    .is_some_and(|surface| {
                        records.offsets(surface).iter().any(|surface_offset| {
                            bytes.get(*surface_offset + 4..*surface_offset + 7) == Some(b"358")
                        })
                    })
        } else {
            cursor == next
        }
    });
    Ok(Some((reference, entry_name, valid)))
}

#[cfg(test)]
mod tests;
