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
    FormCageBinding, FormCageObservation, FormClosureEvidence, FormConstruction,
    FormExternalVersion, FormInputPath, FormInputs, FormKnownOrder, FormListRole, FormMember,
    FormMembership, FormNativeList, FormNativeMember, FormNativeReference,
    FormNativeReferenceTarget, FormObservationIndex, FormOrder, FormOrderEvidence,
    FormPositionEvidence, FormReferenceGroup, FormReferenceRole, FormResource,
    FormResourceAssociation, FormResourceAvailability, FormResourceStateEvidence,
};
use cadmpeg_ir::features::selections::NonEmptyMembers;
use cadmpeg_ir::features::{Feature, FeatureDefinition, FeatureOperation};
use cadmpeg_ir::ids::{NativeId, SubdId};
use cadmpeg_ir::report::loss::LossNote;
use cadmpeg_ir::subd::SubdSurface;

pub(super) fn unresolved(
    scope: &DesignParameterScope,
    parameters: &[(u32, &crate::records::parameters::DesignParameter)],
) -> Result<FeatureDefinition, CodecError> {
    let mut inputs = form_inputs(scope)?;
    inputs.parameters =
        crate::design::feature_project::scope_properties::scope_parameter_expressions(
            scope, parameters,
        )?;
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

#[derive(Clone, PartialEq, Eq, Hash)]
struct ReferenceLane {
    source: NativeId,
    role: FormReferenceRole,
}

#[derive(Clone, PartialEq, Eq, Hash)]
enum PhysicalGroup {
    Body {
        stream: String,
        frame: usize,
        envelope: usize,
    },
    Serializer {
        stream: String,
        frame: usize,
    },
}

#[derive(Clone, PartialEq, Eq, Hash)]
struct GroupKey {
    lane: ReferenceLane,
    label: FormReferenceGroup,
}

#[derive(Clone, Copy)]
struct BoundGroup {
    source: u32,
    role: FormReferenceRole,
    label: FormReferenceGroup,
}

#[derive(Clone, Copy)]
struct ReferenceSlot {
    source: u32,
    role: FormReferenceRole,
    ordinal: u32,
    group: Option<FormReferenceGroup>,
}

impl BoundGroup {
    fn slot(self, ordinal: u32) -> ReferenceSlot {
        ReferenceSlot {
            source: self.source,
            role: self.role,
            ordinal,
            group: Some(self.label),
        }
    }
}

impl ReferenceSlot {
    fn ungrouped(source: u32, role: FormReferenceRole, ordinal: u32) -> Self {
        Self {
            source,
            role,
            ordinal,
            group: None,
        }
    }
}

#[derive(Default)]
struct ReferenceGroups {
    origins: HashMap<ReferenceLane, HashMap<PhysicalGroup, FormReferenceGroup>>,
    active: HashSet<GroupKey>,
}

fn group_allocation_error<T>(ctx: &DecodeContext<'_>, count: usize) -> CodecError {
    let Some(bytes) = count.checked_mul(std::mem::size_of::<T>()) else {
        return cadmpeg_core::decode::ResourceLimit::allocation_failed(
            cadmpeg_core::decode::ResourceDimension::RetainedBytes,
            ctx.policy().limits.max_retained_bytes,
            u64::MAX,
            "F3D Form group allocation",
        )
        .into();
    };
    cadmpeg_core::decode::ResourceLimit::allocation_failed(
        cadmpeg_core::decode::ResourceDimension::RetainedBytes,
        ctx.policy().limits.max_retained_bytes,
        cadmpeg_core::decode::u64_from_index(bytes),
        "F3D Form group allocation",
    )
    .into()
}

fn group_string(ctx: &DecodeContext<'_>, value: &str) -> Result<String, CodecError> {
    let mut copy = String::new();
    copy.try_reserve_exact(value.len())
        .map_err(|_| group_allocation_error::<u8>(ctx, value.len()))?;
    copy.push_str(value);
    Ok(copy)
}

fn group_record_id(
    ctx: &DecodeContext<'_>,
    scope: &DesignParameterScope,
    record: u64,
) -> Result<NativeId, CodecError> {
    use std::fmt::Write;
    let stream = native_stream(&scope.id).unwrap_or(ids::DEFAULT_STREAM);
    let capacity = stream
        .len()
        .checked_add(":design-record#".len() + 20)
        .ok_or_else(|| group_allocation_error::<u8>(ctx, usize::MAX))?;
    let mut text = String::new();
    text.try_reserve_exact(capacity)
        .map_err(|_| group_allocation_error::<u8>(ctx, capacity))?;
    write!(&mut text, "{stream}:design-record#{record}").map_err(CodecError::malformed)?;
    NativeId::mint(text).map_err(CodecError::malformed)
}

#[derive(Default)]
struct GroupKeyWriter {
    bytes: Vec<u8>,
    refused: Option<usize>,
}

impl GroupKeyWriter {
    fn reserve(&mut self, count: usize) {
        if self.refused.is_none() && self.bytes.try_reserve(count).is_err() {
            self.refused = Some(count);
        }
    }

    fn finish(self, ctx: &DecodeContext<'_>) -> Result<Vec<u8>, CodecError> {
        match self.refused {
            Some(count) => Err(group_allocation_error::<u8>(ctx, count)),
            None => Ok(self.bytes),
        }
    }
}

impl std::io::Write for GroupKeyWriter {
    fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
        self.reserve(bytes.len());
        if self.refused.is_none() {
            self.bytes.extend_from_slice(bytes);
        }
        Ok(bytes.len())
    }

    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

fn group_json_key<T: serde::Serialize>(
    ctx: &DecodeContext<'_>,
    value: &T,
) -> Result<Vec<u8>, CodecError> {
    let mut writer = GroupKeyWriter::default();
    let result = serde_json::to_writer(&mut writer, value);
    let bytes = writer.finish(ctx)?;
    result.map_err(CodecError::malformed)?;
    Ok(bytes)
}

impl ReferenceGroups {
    fn bind(
        &mut self,
        ctx: &DecodeContext<'_>,
        scope: &DesignParameterScope,
        source: u32,
        role: FormReferenceRole,
        origin: PhysicalGroup,
    ) -> Result<BoundGroup, CodecError> {
        let lane = ReferenceLane {
            source: group_record_id(ctx, scope, u64::from(source))?,
            role,
        };
        let known = self
            .origins
            .get(&lane)
            .and_then(|groups| groups.get(&origin))
            .copied();
        let label = if let Some(label) = known {
            label
        } else {
            self.origins
                .try_reserve(1)
                .map_err(|_| group_allocation_error::<(ReferenceLane, HashMap<PhysicalGroup, FormReferenceGroup>)>(ctx, 1))?;
            let stored_lane = ReferenceLane {
                source: NativeId::mint(group_string(ctx, lane.source.as_str())?)
                    .map_err(CodecError::malformed)?,
                role,
            };
            let groups = self.origins.entry(stored_lane).or_default();
            groups.try_reserve(1).map_err(|_| {
                group_allocation_error::<(PhysicalGroup, FormReferenceGroup)>(ctx, 1)
            })?;
            let next = u64::try_from(groups.len()).map_err(CodecError::malformed)?;
            let label = FormReferenceGroup::from_label(next);
            groups.insert(origin, label);
            label
        };
        self.active
            .try_reserve(1)
            .map_err(|_| group_allocation_error::<GroupKey>(ctx, 1))?;
        self.active.insert(GroupKey { lane, label });
        Ok(BoundGroup {
            source,
            role,
            label,
        })
    }

    fn check(
        &self,
        ctx: &DecodeContext<'_>,
        construction: &FormConstruction,
        witness: Option<&HashSet<Vec<u8>>>,
        fresh: bool,
    ) -> Result<(), CodecError> {
        for reference in &construction.inputs().reference_evidence {
            let Some(label) = reference.group else {
                continue;
            };
            let witnessed = if fresh {
                let key = GroupKey {
                    lane: ReferenceLane {
                        source: NativeId::mint(group_string(ctx, reference.source.as_str())?)
                            .map_err(CodecError::malformed)?,
                        role: reference.role,
                    },
                    label,
                };
                self.active.contains(&key)
            } else if let Some(facts) = witness {
                facts.contains(&group_json_key(ctx, reference)?)
            } else {
                false
            };
            if !witnessed {
                return Err(CodecError::malformed("Form group origin is unavailable"));
            }
        }
        Ok(())
    }
}

fn published_facts(
    ctx: &DecodeContext<'_>,
    construction: &FormConstruction,
) -> Result<HashSet<Vec<u8>>, CodecError> {
    let references = &construction.inputs().reference_evidence;
    let count = references
        .iter()
        .filter(|reference| reference.group.is_some())
        .count();
    let mut facts = HashSet::new();
    facts
        .try_reserve(count)
        .map_err(|_| group_allocation_error::<Vec<u8>>(ctx, count))?;
    for reference in references
        .iter()
        .filter(|reference| reference.group.is_some())
    {
        facts.insert(group_json_key(ctx, reference)?);
    }
    Ok(facts)
}

fn sort_reference_evidence(
    ctx: &DecodeContext<'_>,
    inputs: &mut FormInputs,
) -> Result<(), CodecError> {
    let count = inputs.reference_evidence.len();
    let mut references = Vec::new();
    references
        .try_reserve_exact(count)
        .map_err(|_| group_allocation_error::<(Vec<u8>, usize, FormNativeReference)>(ctx, count))?;
    for (index, reference) in inputs.reference_evidence.drain(..).enumerate() {
        references.push((group_json_key(ctx, &reference)?, index, reference));
    }
    references.sort_unstable_by(|left, right| left.0.cmp(&right.0).then(left.1.cmp(&right.1)));
    inputs
        .reference_evidence
        .extend(references.into_iter().map(|(_, _, reference)| reference));
    Ok(())
}

pub(crate) struct FormProjection<'scan> {
    scan: &'scan ContainerScan<'scan>,
    groups: ReferenceGroups,
    published: HashMap<String, HashSet<Vec<u8>>>,
}

impl<'scan> FormProjection<'scan> {
    pub(crate) fn new(scan: &'scan ContainerScan<'scan>) -> Self {
        Self {
            scan,
            groups: ReferenceGroups::default(),
            published: HashMap::new(),
        }
    }

    pub(crate) fn project(
        &mut self,
        ctx: &DecodeContext<'_>,
        scopes: &[DesignParameterScope],
        features: &mut [Feature],
        cages: &[SubdSurface],
        losses: &mut Vec<LossNote>,
    ) -> Result<(), CodecError> {
        if let Some(limit) = ctx.resource_refusal() {
            return Err(CodecError::ResourceLimit(limit));
        }
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
            let witness = self.published.get(feature.id.as_str());
            self.groups.check(ctx, &retained, witness, false)?;
            let scan = self.scan;
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
                merge_reference_runs(&mut inputs, &form_inputs(scope)?.references, &[]);
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
                let records = scan.design_record_offsets(&entry.name)?;
                let mut inputs = form_inputs(scope)?;
                inputs.parameters = retained.inputs().parameters.clone();
                let mut facts = FormFacts {
                    inputs,
                    observations: Vec::new(),
                };
                let source = FormSource {
                    bytes: bytes.window(),
                    records: &records,
                    scope,
                    cages,
                    stream: &entry.name,
                };
                self.groups.active.clear();
                let membership = collect_membership(
                    ctx,
                    scan,
                    source,
                    scopes,
                    &mut facts,
                    &mut self.groups,
                    losses,
                )?;
                let compacted = facts.prepare_resources();
                let mut membership = remap_membership(membership, &compacted);
                if membership_has_duplicate_members(&membership) {
                    membership = facts.reconcile(membership.clone(), membership);
                }
                let membership = facts.enforce_resource_membership(membership);
                sort_reference_evidence(ctx, &mut facts.inputs)?;
                let construction =
                    FormConstruction::try_new(facts.inputs, facts.observations, membership)
                        .map_err(CodecError::malformed)?;
                retained = retain_knowledge(ctx, &retained, &construction, &self.groups, witness)?;
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
            let published = published_facts(ctx, &retained)?;
            if published.is_empty() {
                self.published.remove(feature.id.as_str());
            } else {
                self.published
                    .try_reserve(1)
                    .map_err(|_| group_allocation_error::<(String, HashSet<Vec<u8>>)>(ctx, 1))?;
                let feature_id = group_string(ctx, feature.id.as_str())?;
                self.published.insert(feature_id, published);
            }
            feature
                .evaluation
                .set_definition(FeatureDefinition::Operation(FeatureOperation::Form(
                    retained,
                )));
        }
        Ok(())
    }
}

fn retain_knowledge(
    ctx: &DecodeContext<'_>,
    old: &FormConstruction,
    new: &FormConstruction,
    groups: &ReferenceGroups,
    witness: Option<&HashSet<Vec<u8>>>,
) -> Result<FormConstruction, CodecError> {
    groups.check(ctx, old, witness, false)?;
    groups.check(ctx, new, None, true)?;
    let mut facts = FormFacts {
        inputs: old.inputs().clone(),
        observations: old.observations().to_vec(),
    };
    if facts.inputs.parameters.is_empty() {
        facts.inputs.parameters = new.inputs().parameters.clone();
    }
    merge_reference_runs(
        &mut facts.inputs,
        &new.inputs().references,
        &new.inputs().reference_runs,
    );
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
    for old_observation in old.observations() {
        for new_observation in new.observations() {
            let (Some((old_resource, old_state)), Some((new_resource, new_state))) = (
                resource_state(&old_observation.binding),
                resource_state(&new_observation.binding),
            ) else {
                continue;
            };
            if old_resource.entry != new_resource.entry
                || (old_state != Some(FormResourceAvailability::Inactive)
                    && new_state != Some(FormResourceAvailability::Inactive))
                || old_state == new_state
            {
                continue;
            }
            for (resource, availability) in [(old_resource, old_state), (new_resource, new_state)] {
                if let Some(availability) = availability {
                    if availability != FormResourceAvailability::Ambiguous {
                        let state = FormResourceStateEvidence {
                            resource: resource.clone(),
                            availability,
                        };
                        if !facts.inputs.resource_state_evidence.contains(&state) {
                            facts.inputs.resource_state_evidence.push(state);
                        }
                    }
                }
            }
        }
    }
    let mut mapping = new
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
    for evidence in &new.inputs().order_evidence {
        let mut evidence = evidence.clone();
        for member in &mut evidence.members {
            *member = mapping[member.0];
        }
        if !facts.inputs.order_evidence.contains(&evidence) {
            facts.inputs.order_evidence.push(evidence);
        }
    }
    for &count in &new.inputs().cardinality_evidence {
        if !facts.inputs.cardinality_evidence.contains(&count) {
            facts.inputs.cardinality_evidence.push(count);
        }
    }
    for claim in &new.inputs().closure_evidence {
        let mut claim = claim.clone();
        for member in &mut claim.members {
            *member = mapping[member.0];
        }
        if !facts.inputs.closure_evidence.contains(&claim) {
            facts.inputs.closure_evidence.push(claim);
        }
    }
    for resource in &new.inputs().resource_evidence {
        if !facts.inputs.resource_evidence.contains(resource) {
            facts.inputs.resource_evidence.push(resource.clone());
        }
    }
    for association in &new.inputs().resource_associations {
        facts.remember_association(association.cage.clone(), association.resource.clone());
    }
    for state in &new.inputs().resource_state_evidence {
        if !facts.inputs.resource_state_evidence.contains(state) {
            facts.inputs.resource_state_evidence.push(state.clone());
        }
    }
    let compacted = facts.prepare_resources();
    for index in &mut mapping {
        *index = compacted[index.0];
    }
    let old_membership = remap_membership(old.membership().clone(), &compacted);
    let new_membership = remap_membership(new.membership().clone(), &mapping);
    let old_empty = old.observations().is_empty()
        && old.inputs().lists.is_empty()
        && old.inputs().reference_evidence.is_empty()
        && old.inputs().position_evidence.is_empty()
        && old.inputs().order_evidence.is_empty()
        && old.inputs().cardinality_evidence.is_empty()
        && old.inputs().closure_evidence.is_empty()
        && membership_unknown(old.membership());
    let new_empty = new.observations().is_empty()
        && new.inputs().lists.is_empty()
        && new.inputs().reference_evidence.is_empty()
        && new.inputs().position_evidence.is_empty()
        && new.inputs().order_evidence.is_empty()
        && new.inputs().cardinality_evidence.is_empty()
        && new.inputs().closure_evidence.is_empty()
        && membership_unknown(new.membership());
    let mut membership = if old_empty && !membership_has_duplicate_members(&new_membership) {
        new_membership
    } else if new_empty && !membership_has_duplicate_members(&old_membership) {
        old_membership
    } else {
        facts.reconcile(old_membership, new_membership)
    };
    membership = facts.enforce_resource_membership(membership);
    sort_reference_evidence(ctx, &mut facts.inputs)?;
    FormConstruction::try_new(facts.inputs, facts.observations, membership)
        .map_err(CodecError::malformed)
}

fn resource_state(
    binding: &FormCageBinding,
) -> Option<(&FormResource, Option<FormResourceAvailability>)> {
    match binding {
        FormCageBinding::Decoded {
            resource: Some(resource),
            ..
        } => Some((resource, None)),
        FormCageBinding::Undecoded {
            resource,
            availability,
        } => Some((resource, Some(*availability))),
        _ => None,
    }
}

fn merge_resource_availability(
    left: FormResourceAvailability,
    right: FormResourceAvailability,
) -> FormResourceAvailability {
    match (left, right) {
        (a, b) if a == b => a,
        (FormResourceAvailability::Unknown, value) | (value, FormResourceAvailability::Unknown) => {
            value
        }
        _ => FormResourceAvailability::Ambiguous,
    }
}

fn merge_reference_runs(
    inputs: &mut FormInputs,
    fresh: &[NativeId],
    alternatives: &[Vec<NativeId>],
) {
    let mut runs = std::mem::take(&mut inputs.reference_runs);
    if !inputs.references.is_empty() {
        runs.push(std::mem::take(&mut inputs.references));
    }
    for run in std::iter::once(fresh).chain(alternatives.iter().map(Vec::as_slice)) {
        if run.is_empty() || runs.iter().any(|old| old.starts_with(run)) {
            continue;
        }
        runs.retain(|old| !run.starts_with(old));
        runs.push(run.to_vec());
    }
    if runs.len() == 1 {
        inputs.references = runs.pop().unwrap_or_default();
    } else {
        runs.sort();
        runs.dedup();
        inputs.reference_runs = runs;
    }
}

fn membership_unknown(membership: &FormMembership) -> bool {
    matches!(membership, FormMembership::Partial { cardinality: None, order: FormOrder::Unknown, members } if members.is_empty())
}

fn membership_evidence_conflicts(inputs: &FormInputs) -> bool {
    if membership_count_conflicts(inputs) {
        return true;
    }
    let mut positions = HashMap::new();
    let mut positions_by_member = HashMap::new();
    for evidence in &inputs.position_evidence {
        if positions
            .insert(evidence.ordinal, evidence.observation)
            .is_some_and(|old| old != evidence.observation)
            || positions_by_member
                .insert(evidence.observation, evidence.ordinal)
                .is_some_and(|old| old != evidence.ordinal)
        {
            return true;
        }
    }
    let mut seen = HashSet::new();
    let members = inputs
        .order_evidence
        .iter()
        .flat_map(|evidence| evidence.members.iter().copied())
        .chain(
            inputs
                .closure_evidence
                .iter()
                .flat_map(|claim| claim.members.iter().copied()),
        )
        .chain(
            inputs
                .position_evidence
                .iter()
                .map(|evidence| evidence.observation),
        )
        .filter(|index| seen.insert(*index))
        .map(|observation| FormMember {
            observation,
            ordinal: positions_by_member.get(&observation).copied(),
        })
        .collect::<Vec<_>>();
    ordered_observations(
        &members,
        &inputs.order_evidence,
        &inputs.closure_evidence,
        false,
        true,
        false,
    )
    .is_none()
}

fn membership_count_conflicts(inputs: &FormInputs) -> bool {
    let mut counts = inputs
        .cardinality_evidence
        .iter()
        .map(|count| cadmpeg_core::decode::index_from_u32(*count))
        .chain(
            inputs
                .closure_evidence
                .iter()
                .map(|claim| claim.members.len()),
        );
    counts
        .next()
        .is_some_and(|first| counts.any(|count| count != first))
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

fn membership_has_duplicate_members(membership: &FormMembership) -> bool {
    let mut seen = HashSet::new();
    match membership {
        FormMembership::Complete { members, .. } => {
            members.iter().any(|index| !seen.insert(*index))
        }
        FormMembership::Partial { members, .. } => members
            .iter()
            .any(|member| !seen.insert(member.observation)),
    }
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
    if left == right && !membership_has_duplicate_members(&left) {
        return left;
    }
    if let (
        FormMembership::Complete {
            members: left_members,
            order: left_order,
            certainty: left_certainty,
        },
        FormMembership::Complete {
            members: right_members,
            order: right_order,
            certainty: right_certainty,
        },
    ) = (&left, &right)
    {
        if left_members == right_members && !membership_has_duplicate_members(&left) {
            let stronger = |a, b| {
                if a == FormKnownOrder::Source || b == FormKnownOrder::Source {
                    FormKnownOrder::Source
                } else {
                    FormKnownOrder::Assumed
                }
            };
            return FormMembership::Complete {
                members: left_members.clone(),
                order: stronger(*left_order, *right_order),
                certainty: stronger(*left_certainty, *right_certainty),
            };
        }
    }
    let complete = match (&left, &right) {
        (FormMembership::Complete { .. }, _) if compatible_with_complete(&left, &right) => {
            Some(left.clone())
        }
        (_, FormMembership::Complete { .. }) if compatible_with_complete(&right, &left) => {
            Some(right.clone())
        }
        _ => None,
    };
    if let Some(mut complete) = complete {
        let other = if matches!(&left, FormMembership::Complete { .. }) {
            &right
        } else {
            &left
        };
        if let (
            FormMembership::Complete { members, order, .. },
            FormMembership::Partial {
                order: FormOrder::Source,
                members: claims,
                ..
            },
        ) = (&mut complete, other)
        {
            if claims
                .iter()
                .map(|claim| claim.observation)
                .eq(members.iter().copied())
            {
                *order = FormKnownOrder::Source;
            }
        }
        return complete;
    }
    let (left_count, left_order, first) = membership_parts(left);
    let (right_count, right_order, more) = membership_parts(right);
    let cardinality = match (left_count, right_count) {
        (Some(a), Some(b)) if a != b => None,
        (Some(a), _) | (_, Some(a)) => Some(a),
        _ => None,
    };
    let mut conflicting = HashSet::new();
    let mut members: Vec<FormMember> = Vec::new();
    for member in first.into_iter().chain(more) {
        if let Some(existing) = members
            .iter_mut()
            .find(|existing| existing.observation == member.observation)
        {
            match (existing.ordinal, member.ordinal) {
                (None, Some(ordinal)) => existing.ordinal = Some(ordinal),
                (Some(a), Some(b)) if a != b => {
                    existing.ordinal = None;
                    conflicting.insert(existing.observation);
                }
                _ => {}
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
        order: if conflicting.is_empty()
            && (left_order == right_order
                || left_order == FormOrder::Unknown
                || right_order == FormOrder::Unknown)
        {
            if left_order == FormOrder::Unknown {
                right_order
            } else {
                left_order
            }
        } else {
            FormOrder::Unknown
        },
        members,
    }
}

fn compatible_with_complete(complete: &FormMembership, other: &FormMembership) -> bool {
    let FormMembership::Complete { members, .. } = complete else {
        return false;
    };
    let (count, order, claims) = membership_parts(other.clone());
    let mut previous = None;
    let relative_order = order == FormOrder::Unknown
        || claims.iter().all(|claim| {
            let Some(position) = members
                .iter()
                .position(|member| *member == claim.observation)
            else {
                return false;
            };
            let valid = previous.is_none_or(|last| last < position);
            previous = Some(position);
            valid
        });
    !membership_has_duplicate_members(complete)
        && !membership_has_duplicate_members(other)
        && count.is_none_or(|count| usize::try_from(count).ok() == Some(members.len()))
        && relative_order
        && claims.iter().all(|claim| {
            members
                .iter()
                .position(|index| *index == claim.observation)
                .is_some_and(|ordinal| {
                    claim
                        .ordinal
                        .is_none_or(|claimed| usize::try_from(claimed).ok() == Some(ordinal))
                })
        })
}

struct FormFacts {
    inputs: FormInputs,
    observations: Vec<FormCageObservation>,
}

struct FormSurface {
    path: FormInputPath,
    record: Option<u64>,
}

fn native_reference(
    ctx: &DecodeContext<'_>,
    scope: &DesignParameterScope,
    slot: ReferenceSlot,
    reference: &FormReference,
    resource: Option<FormResource>,
) -> Result<FormNativeReference, CodecError> {
    use crate::bytes::Reference;
    let (inline_type_guid, target) = match reference {
        Reference::Null => (None, FormNativeReferenceTarget::Null {}),
        Reference::Local {
            target,
            inline_type_guid,
        } => (
            inline_type_guid
                .as_deref()
                .map(|value| group_string(ctx, value))
                .transpose()?,
            FormNativeReferenceTarget::Local {
                target: group_record_id(ctx, scope, *target)?,
            },
        ),
        Reference::CrossSegment {
            target,
            inline_type_guid,
            segment,
            asset_guid,
        } => (
            inline_type_guid
                .as_deref()
                .map(|value| group_string(ctx, value))
                .transpose()?,
            FormNativeReferenceTarget::Segment {
                target: *target,
                segment: *segment,
                asset_guid: asset_guid
                    .as_deref()
                    .map(|value| group_string(ctx, value))
                    .transpose()?,
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
            inline_type_guid
                .as_deref()
                .map(|value| group_string(ctx, value))
                .transpose()?,
            FormNativeReferenceTarget::Document {
                target: *target,
                segment: *segment,
                asset_guid: group_string(ctx, asset_guid)?,
                target_type_guid: target_type_guid
                    .as_deref()
                    .map(|value| group_string(ctx, value))
                    .transpose()?,
                link_name: group_string(ctx, link_name)?,
                version: version
                    .as_ref()
                    .map(|(key, urn)| {
                        Ok::<_, CodecError>(FormExternalVersion {
                            key: group_string(ctx, key)?,
                            urn: group_string(ctx, urn)?,
                        })
                    })
                    .transpose()?,
            },
        ),
    };
    let mut evidence = FormNativeReference::new(
        group_record_id(ctx, scope, u64::from(slot.source))?,
        slot.role,
        slot.ordinal,
        target,
    );
    evidence.group = slot.group;
    evidence.inline_type_guid = inline_type_guid;
    evidence.resource = resource;
    Ok(evidence)
}

impl FormFacts {
    fn capture_closure(&mut self, membership: &FormMembership) {
        if let FormMembership::Complete {
            members,
            order,
            certainty,
        } = membership
        {
            let claim = FormClosureEvidence {
                members: members.clone(),
                order: *order,
                certainty: *certainty,
            };
            if !self.inputs.closure_evidence.contains(&claim) {
                self.inputs.closure_evidence.push(claim);
            }
        }
    }
    fn capture_membership(&mut self, membership: &FormMembership) {
        let (count, order, members) = membership_parts(membership.clone());
        if members.len() > 1 && order != FormOrder::Unknown {
            let evidence = FormOrderEvidence {
                order: if order == FormOrder::Source {
                    FormKnownOrder::Source
                } else {
                    FormKnownOrder::Assumed
                },
                members: members.iter().map(|member| member.observation).collect(),
            };
            if !self.inputs.order_evidence.contains(&evidence) {
                self.inputs.order_evidence.push(evidence);
            }
        }
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
        let closure_claims = [left.clone(), right.clone()];
        let capture = left != right || membership_has_duplicate_members(&left);
        if capture {
            self.capture_membership(&left);
            self.capture_membership(&right);
        }
        let mut membership = merge_memberships(left, right);
        let mut order_refined = false;
        for claim in self.inputs.closure_evidence.clone() {
            let historical = FormMembership::Complete {
                members: claim.members,
                order: claim.order,
                certainty: claim.certainty,
            };
            self.capture_membership(&historical);
            membership = merge_memberships(membership, historical);
        }
        if let FormMembership::Complete {
            members,
            order,
            certainty,
        } = &mut membership
        {
            if self
                .inputs
                .closure_evidence
                .iter()
                .any(|claim| claim.certainty == FormKnownOrder::Source && claim.members == *members)
            {
                *certainty = FormKnownOrder::Source;
            }
            if self
                .inputs
                .closure_evidence
                .iter()
                .any(|claim| claim.order == FormKnownOrder::Source && claim.members == *members)
            {
                *order = FormKnownOrder::Source;
            }
            if *order == FormKnownOrder::Assumed && members.len() > 1 {
                let candidates = members
                    .iter()
                    .map(|&observation| FormMember {
                        observation,
                        ordinal: None,
                    })
                    .collect::<Vec<_>>();
                if ordered_observations(
                    &candidates,
                    &self.inputs.order_evidence,
                    &self.inputs.closure_evidence,
                    true,
                    false,
                    true,
                )
                .as_deref()
                    == Some(members.as_slice())
                {
                    *order = FormKnownOrder::Source;
                    order_refined = true;
                }
            }
            let conflicts = self
                .inputs
                .cardinality_evidence
                .iter()
                .any(|count| usize::try_from(*count).ok() != Some(members.len()))
                || self
                    .inputs
                    .closure_evidence
                    .iter()
                    .any(|claim| claim.members != *members)
                || self.inputs.position_evidence.iter().any(|evidence| {
                    usize::try_from(evidence.ordinal)
                        .ok()
                        .and_then(|index| members.get(index))
                        != Some(&evidence.observation)
                })
                || self.inputs.order_evidence.iter().any(|evidence| {
                    let mut remaining = members.iter();
                    !evidence
                        .members
                        .iter()
                        .all(|index| remaining.any(|member| member == index))
                });
            if conflicts {
                let (cardinality, order, members) = membership_parts(membership);
                membership = FormMembership::Partial {
                    cardinality,
                    order,
                    members,
                };
            }
        }
        if order_refined && matches!(membership, FormMembership::Complete { .. }) {
            self.capture_membership(&membership);
        }
        if matches!(membership, FormMembership::Partial { .. }) {
            for claim in &closure_claims {
                self.capture_closure(claim);
            }
        }
        if membership_count_conflicts(&self.inputs) {
            if let FormMembership::Partial { cardinality, .. } = &mut membership {
                *cardinality = None;
            }
        }
        if membership_evidence_conflicts(&self.inputs) {
            if let FormMembership::Partial { order, members, .. } = &mut membership {
                *order = FormOrder::Unknown;
                for member in members.iter_mut() {
                    let positions = self
                        .inputs
                        .position_evidence
                        .iter()
                        .filter(|e| e.observation == member.observation)
                        .map(|e| e.ordinal)
                        .collect::<HashSet<_>>();
                    if positions.len() > 1
                        || positions.iter().any(|ordinal| {
                            self.inputs.position_evidence.iter().any(|e| {
                                e.ordinal == *ordinal && e.observation != member.observation
                            })
                        })
                    {
                        member.ordinal = None;
                    }
                }
                members.sort_by_key(|member| {
                    (
                        member.ordinal.is_none(),
                        member.ordinal,
                        member.observation.0,
                    )
                });
            }
        } else if let FormMembership::Partial { order, members, .. } = &mut membership {
            if members.len() > 1 {
                let source = ordered_observations(
                    members,
                    &self.inputs.order_evidence,
                    &self.inputs.closure_evidence,
                    true,
                    *order == FormOrder::Source,
                    true,
                );
                let any = ordered_observations(
                    members,
                    &self.inputs.order_evidence,
                    &self.inputs.closure_evidence,
                    false,
                    true,
                    true,
                );
                let (sequence, certainty) = if let Some(sequence) = source {
                    (Some(sequence), FormOrder::Source)
                } else if let Some(sequence) = any {
                    (Some(sequence), FormOrder::Assumed)
                } else {
                    (None, FormOrder::Unknown)
                };
                *order = certainty;
                if let Some(sequence) = sequence {
                    members.sort_by_key(|member| {
                        sequence
                            .iter()
                            .position(|index| *index == member.observation)
                    });
                }
            }
        }
        if capture
            && matches!(&membership, FormMembership::Partial { order: FormOrder::Source | FormOrder::Assumed, members, .. } if members.len() > 1)
        {
            self.capture_membership(&membership);
        }
        membership
    }
    fn reference(
        &mut self,
        ctx: &DecodeContext<'_>,
        scope: &DesignParameterScope,
        slot: ReferenceSlot,
        reference: &crate::bytes::Reference<String, String>,
        resource: Option<FormResource>,
    ) -> Result<(), CodecError> {
        let evidence = native_reference(ctx, scope, slot, reference, resource)?;
        if !self.inputs.reference_evidence.contains(&evidence) {
            let mut lengths = [0; 6];
            lengths[0] = evidence.inline_type_guid.as_deref().map_or(0, str::len);
            if let FormNativeReferenceTarget::Segment {
                asset_guid: Some(asset_guid),
                ..
            } = &evidence.target
            {
                lengths[1] = asset_guid.len();
            }
            if let FormNativeReferenceTarget::Document {
                asset_guid,
                target_type_guid,
                link_name,
                version,
                ..
            } = &evidence.target
            {
                lengths[1] = asset_guid.len();
                lengths[2] = target_type_guid.as_deref().map_or(0, str::len);
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
    fn missing_reference(
        &mut self,
        scope: &DesignParameterScope,
        slot: ReferenceSlot,
        unvisited: bool,
    ) -> Result<(), CodecError> {
        let target = if unvisited {
            FormNativeReferenceTarget::Unvisited {}
        } else {
            FormNativeReferenceTarget::Unreadable {}
        };
        let mut evidence = FormNativeReference::new(
            record_id(scope, slot.source)?,
            slot.role,
            slot.ordinal,
            target,
        );
        evidence.group = slot.group;
        if !self.inputs.reference_evidence.contains(&evidence) {
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
    fn remember_association(&mut self, cage: SubdId, resource: FormResource) {
        let association = FormResourceAssociation { cage, resource };
        if self.observations.iter().any(|observation| {
            matches!(&observation.binding,
                FormCageBinding::Decoded { cage, resource: Some(resource) }
                if cage == &association.cage && resource == &association.resource)
        }) {
            return;
        }
        if !self.inputs.resource_associations.contains(&association) {
            self.inputs.resource_associations.push(association);
        }
        self.inputs.resource_associations.sort_by(|a, b| {
            (&a.cage, &a.resource.entry, &a.resource.source).cmp(&(
                &b.cage,
                &b.resource.entry,
                &b.resource.source,
            ))
        });
    }
    fn detach_resource_entry(&mut self, entry: &cadmpeg_core::text::NonBlankString) {
        let mut displaced = Vec::new();
        for observation in &mut self.observations {
            if let FormCageBinding::Decoded { cage, resource } = &mut observation.binding {
                if let Some(bound) = resource
                    .as_ref()
                    .filter(|resource| &resource.entry == entry)
                {
                    displaced.push((cage.clone(), bound.clone()));
                    *resource = None;
                }
            }
        }
        for (cage, resource) in displaced {
            self.remember_resource(resource.clone());
            self.remember_association(cage, resource);
        }
    }
    fn prepare_resources(&mut self) -> Vec<FormObservationIndex> {
        self.reconcile_resource_states();
        self.normalize_resource_conflicts();
        let mapping = self.coalesce_resource_observations();
        self.reconcile_resource_states();
        self.inputs.resource_associations.retain(|association| {
            !self.observations.iter().any(|observation| {
                matches!(&observation.binding,
                    FormCageBinding::Decoded { cage, resource: Some(resource) }
                    if cage == &association.cage && resource == &association.resource)
            })
        });
        mapping
    }
    fn enforce_resource_membership(&mut self, mut membership: FormMembership) -> FormMembership {
        let (_, _, selected) = membership_parts(membership.clone());
        let mut conflict = false;
        for member in selected {
            if let FormCageBinding::Decoded { cage, .. } =
                &self.observations[member.observation.0].binding
            {
                conflict |= self
                    .inputs
                    .cage_resource_conflicts(&self.observations, cage);
            }
            match &mut self.observations[member.observation.0].binding {
                FormCageBinding::Undecoded {
                    resource,
                    availability,
                } if *availability == FormResourceAvailability::Inactive => {
                    let state = FormResourceStateEvidence {
                        resource: resource.clone(),
                        availability: *availability,
                    };
                    if !self.inputs.resource_state_evidence.contains(&state) {
                        self.inputs.resource_state_evidence.push(state);
                    }
                    *availability = FormResourceAvailability::Ambiguous;
                }
                _ => {}
            }
        }
        if conflict && matches!(membership, FormMembership::Complete { .. }) {
            self.capture_membership(&membership);
            self.capture_closure(&membership);
            let (cardinality, order, members) = membership_parts(membership);
            membership = FormMembership::Partial {
                cardinality,
                order,
                members,
            };
        }
        self.inputs.resource_state_evidence.sort_by(|a, b| {
            (&a.resource.entry, &a.resource.source, a.availability).cmp(&(
                &b.resource.entry,
                &b.resource.source,
                b.availability,
            ))
        });
        membership
    }
    fn normalize_resource_conflicts(&mut self) {
        let ambiguous = self
            .observations
            .iter()
            .filter_map(|observation| match &observation.binding {
                FormCageBinding::Undecoded {
                    resource,
                    availability: FormResourceAvailability::Ambiguous,
                } => Some(resource.entry.clone()),
                _ => None,
            })
            .collect::<Vec<_>>();
        for entry in ambiguous {
            self.detach_resource_entry(&entry);
        }
        let mut groups: HashMap<cadmpeg_core::text::NonBlankString, Vec<(SubdId, FormResource)>> =
            HashMap::new();
        for observation in &self.observations {
            if let FormCageBinding::Decoded {
                cage,
                resource: Some(resource),
            } = &observation.binding
            {
                groups
                    .entry(resource.entry.clone())
                    .or_default()
                    .push((cage.clone(), resource.clone()));
            }
        }
        for association in &self.inputs.resource_associations {
            groups
                .entry(association.resource.entry.clone())
                .or_default()
                .push((association.cage.clone(), association.resource.clone()));
        }
        let mut conflicts = groups
            .into_iter()
            .filter(|(_, associations)| {
                associations
                    .iter()
                    .map(|(cage, _)| cage)
                    .collect::<HashSet<_>>()
                    .len()
                    > 1
            })
            .collect::<Vec<_>>();
        conflicts.sort_by(|a, b| a.0.cmp(&b.0));
        for (entry, associations) in conflicts {
            self.detach_resource_entry(&entry);
            let Some(resource) = associations
                .iter()
                .map(|(_, resource)| resource)
                .min_by(|a, b| (&a.entry, &a.source).cmp(&(&b.entry, &b.source)))
                .cloned()
            else {
                continue;
            };
            for (_, item) in associations {
                self.remember_resource(item);
            }
            self.observe(FormCageObservation {
                paths: Vec::new(),
                binding: FormCageBinding::Undecoded {
                    resource,
                    availability: FormResourceAvailability::Ambiguous,
                },
            });
        }
    }
    fn coalesce_resource_observations(&mut self) -> Vec<FormObservationIndex> {
        let mut mapping = (0..self.observations.len())
            .map(FormObservationIndex)
            .collect::<Vec<_>>();
        loop {
            let pair = self
                .observations
                .iter()
                .enumerate()
                .find_map(|(undecoded, observation)| {
                    let FormCageBinding::Undecoded {
                        resource,
                        availability,
                    } = &observation.binding
                    else {
                        return None;
                    };
                    if *availability == FormResourceAvailability::Ambiguous {
                        return None;
                    }
                    let matches = self
                        .observations
                        .iter()
                        .enumerate()
                        .filter_map(|(decoded, candidate)| {
                            let FormCageBinding::Decoded {
                                cage,
                                resource: bound,
                            } = &candidate.binding
                            else {
                                return None;
                            };
                            let existing = bound
                                .as_ref()
                                .filter(|bound| bound.entry == resource.entry)
                                .or_else(|| {
                                    self.inputs
                                        .resource_associations
                                        .iter()
                                        .find(|association| {
                                            association.cage == *cage
                                                && association.resource.entry == resource.entry
                                        })
                                        .map(|association| &association.resource)
                                });
                            existing.map(|existing| (decoded, existing.clone()))
                        })
                        .collect::<Vec<_>>();
                    match matches.as_slice() {
                        [(decoded, existing)] => Some((
                            *decoded,
                            undecoded,
                            existing.clone(),
                            resource.clone(),
                            *availability,
                        )),
                        _ => None,
                    }
                });
            let Some((decoded, undecoded, existing, resource, availability)) = pair else {
                break;
            };
            let removed = self.observations.remove(undecoded);
            let decoded_after = decoded - usize::from(undecoded < decoded);
            if matches!(
                availability,
                FormResourceAvailability::ActiveUndecoded | FormResourceAvailability::Inactive
            ) {
                let state = FormResourceStateEvidence {
                    resource: resource.clone(),
                    availability,
                };
                if !self.inputs.resource_state_evidence.contains(&state) {
                    self.inputs.resource_state_evidence.push(state);
                }
            }
            if resource != existing {
                self.remember_resource(resource);
                self.remember_resource(existing);
            }
            let paths = &mut self.observations[decoded_after].paths;
            for path in removed.paths {
                if !paths.contains(&path) {
                    paths.push(path);
                }
            }
            paths.sort_by(|a, b| a.references.as_slice().cmp(b.references.as_slice()));
            for index in &mut mapping {
                if index.0 == undecoded {
                    index.0 = decoded;
                }
                if index.0 > undecoded {
                    index.0 -= 1;
                }
            }
        }
        let mut positions = Vec::new();
        for mut evidence in std::mem::take(&mut self.inputs.position_evidence) {
            evidence.observation = mapping[evidence.observation.0];
            if !positions.contains(&evidence) {
                positions.push(evidence);
            }
        }
        self.inputs.position_evidence = positions;
        let mut orders = Vec::new();
        for mut evidence in std::mem::take(&mut self.inputs.order_evidence) {
            for member in &mut evidence.members {
                *member = mapping[member.0];
            }
            if !orders.contains(&evidence) {
                orders.push(evidence);
            }
        }
        self.inputs.order_evidence = orders;
        let mut closures = Vec::new();
        for mut claim in std::mem::take(&mut self.inputs.closure_evidence) {
            for member in &mut claim.members {
                *member = mapping[member.0];
            }
            if !closures.contains(&claim) {
                closures.push(claim);
            }
        }
        self.inputs.closure_evidence = closures;
        mapping
    }
    fn reconcile_resource_states(&mut self) {
        let mut retained = Vec::new();
        for observation in &mut self.observations {
            let FormCageBinding::Undecoded {
                resource,
                availability,
            } = &mut observation.binding
            else {
                continue;
            };
            let previous = *availability;
            *availability = self
                .inputs
                .resource_state_evidence
                .iter()
                .filter(|claim| claim.resource.entry == resource.entry)
                .fold(previous, |state, claim| {
                    merge_resource_availability(state, claim.availability)
                });
            if *availability == FormResourceAvailability::Ambiguous
                && !matches!(
                    previous,
                    FormResourceAvailability::Unknown | FormResourceAvailability::Ambiguous
                )
            {
                retained.push(FormResourceStateEvidence {
                    resource: resource.clone(),
                    availability: previous,
                });
            }
        }
        for state in retained {
            if !self.inputs.resource_state_evidence.contains(&state) {
                self.inputs.resource_state_evidence.push(state);
            }
        }
    }
    fn observe(&mut self, mut observation: FormCageObservation) -> FormObservationIndex {
        if let FormCageBinding::Decoded {
            cage,
            resource: Some(resource),
        } = &observation.binding
        {
            if self.observations.iter().any(|old| matches!(&old.binding, FormCageBinding::Undecoded { resource: existing, availability: FormResourceAvailability::Ambiguous } if existing.entry == resource.entry)) {
                self.remember_resource(resource.clone());
                self.remember_association(cage.clone(), resource.clone());
                if let FormCageBinding::Decoded { resource, .. } = &mut observation.binding { *resource = None; }
            }
        }
        if let FormCageBinding::Undecoded {
            resource,
            availability: FormResourceAvailability::Ambiguous,
        } = &observation.binding
        {
            self.detach_resource_entry(&resource.entry);
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
            let mut states = Vec::new();
            if let (
                FormCageBinding::Undecoded {
                    resource: old_resource,
                    availability: old,
                },
                FormCageBinding::Undecoded {
                    resource: new_resource,
                    availability: new,
                },
            ) = (&mut self.observations[index].binding, &observation.binding)
            {
                let joined = merge_resource_availability(*old, *new);
                if joined == FormResourceAvailability::Ambiguous {
                    for (resource, availability) in
                        [(old_resource.clone(), *old), (new_resource.clone(), *new)]
                    {
                        if !matches!(
                            availability,
                            FormResourceAvailability::Unknown | FormResourceAvailability::Ambiguous
                        ) {
                            states.push(FormResourceStateEvidence {
                                resource,
                                availability,
                            });
                        }
                    }
                }
                if (&new_resource.entry, &new_resource.source)
                    < (&old_resource.entry, &old_resource.source)
                {
                    old_resource.clone_from(new_resource);
                }
                *old = joined;
            }
            for state in states {
                if !self.inputs.resource_state_evidence.contains(&state) {
                    self.inputs.resource_state_evidence.push(state);
                }
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
            if let FormCageBinding::Decoded { cage, resource } = &self.observations[index].binding {
                let cage = cage.clone();
                let primary = resource.clone();
                if let Some(primary) = &primary {
                    self.inputs.resource_associations.retain(|association| {
                        association.cage != cage || &association.resource != primary
                    });
                }
                for resource in [old_resource, new_resource].into_iter().flatten() {
                    self.remember_association(cage.clone(), resource);
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
            if let Some(run) = form_tracking_members(ctx, source.bytes, source.records, group)? {
                let FormTrackingRun::Parsed { owner, members } = run else {
                    self.missing_reference(
                        source.scope,
                        ReferenceSlot::ungrouped(group, FormReferenceRole::TrackingOwner, 0),
                        false,
                    )?;
                    note(
                        losses,
                        F3dLossCode::FeatureFormTrackingOwnerConflict,
                        &source.scope.id,
                        "a tracking group owner is unreadable",
                    );
                    continue;
                };
                let owner = *owner;
                self.reference(
                    ctx,
                    source.scope,
                    ReferenceSlot::ungrouped(group, FormReferenceRole::TrackingOwner, 0),
                    &owner,
                    None,
                )?;
                if foreign_type_conflict(&owner) {
                    note(
                        losses,
                        F3dLossCode::FeatureFormReferenceTypeConflict,
                        &source.scope.id,
                        "foreign reference type claims conflict",
                    );
                }
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
                        if !types.reference_matches(target, None, inline) {
                            note(
                                losses,
                                F3dLossCode::FeatureFormReferenceTypeConflict,
                                &source.scope.id,
                                "a tracking owner type conflicts with its target registration",
                            );
                        }
                    }
                }
                let Some(run) = members else {
                    note(
                        losses,
                        F3dLossCode::FeatureFormTrackingRunUnresolved,
                        &source.scope.id,
                        "a tracking owner is retained but its member count is unavailable or exceeds the frame",
                    );
                    continue;
                };
                for (members, reference_role, list_role, fixed_slots) in std::iter::once((
                    &run.members,
                    FormReferenceRole::TrackingMember,
                    FormListRole::TrackingSelection,
                    false,
                ))
                .chain(run.fixed_members.iter().map(|members| {
                    (
                        members,
                        FormReferenceRole::TrackingMemberFixedAssumed,
                        FormListRole::TrackingSelectionFixedAssumed,
                        true,
                    )
                })) {
                    if fixed_slots {
                        note(
                            losses,
                            F3dLossCode::FeatureFormTrackingFixedSlotsAssumed,
                            &source.scope.id,
                            "a sparse tracking run also admits independently framed fixed-width slots",
                        );
                    }
                    let mut local =
                        ctx.alloc_filled(members.len(), None, "Form tracking local targets")?;
                    let mut failed = false;
                    for (ordinal, reference) in members.iter().enumerate() {
                        let Some(reference) = reference else {
                            self.missing_reference(
                                source.scope,
                                ReferenceSlot::ungrouped(
                                    group,
                                    reference_role,
                                    u32::try_from(ordinal).map_err(CodecError::malformed)?,
                                ),
                                failed && !fixed_slots,
                            )?;
                            failed = true;
                            continue;
                        };
                        self.reference(
                            ctx,
                            source.scope,
                            ReferenceSlot::ungrouped(
                                group,
                                reference_role,
                                u32::try_from(ordinal).map_err(CodecError::malformed)?,
                            ),
                            reference,
                            None,
                        )?;
                        if foreign_type_conflict(reference) {
                            note(
                                losses,
                                F3dLossCode::FeatureFormReferenceTypeConflict,
                                &source.scope.id,
                                "foreign reference type claims conflict",
                            );
                        }
                        let target = match reference.local() {
                            None if matches!(reference, FormReference::Null) => None,
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
                                let valid = types.reference_matches(
                                    target,
                                    Some("C46D3EEB-40E1-43FA-83DC-9CB204335417"),
                                    inline,
                                );
                                if !valid {
                                    note(
                                        losses,
                                        F3dLossCode::FeatureFormReferenceTypeConflict,
                                        &source.scope.id,
                                        "a tracking member type conflicts with its target role or registration",
                                    );
                                }
                                valid.then_some(target)
                            }
                        };
                        local[ordinal] = target;
                    }
                    self.list(source.scope, group, list_role, &local)?;
                }
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
                serializer.slot()?,
                reference,
                Some(resource.clone()),
            )?;
            let matches = cages
                .iter()
                .filter(|cage| {
                    entry.as_ref().is_some_and(|entry| {
                        scan.has_entry(entry)
                            && cage.source_object.as_ref().is_some_and(|source| {
                                source.format == cadmpeg_ir::codec_id!("f3d")
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
                        resource: resource.clone(),
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
                            resource: Some(resource.clone()),
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
            let mut alternatives = alternatives.into_iter();
            let Some(observation) = alternatives.next() else {
                complete = false;
                continue;
            };
            if alternatives.any(|alternative| alternative != observation) {
                complete = false;
                continue;
            }
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
            if !seen.insert(observation) {
                complete = false;
                continue;
            }
            complete &= matches!(
                self.observations[observation.0].binding,
                FormCageBinding::Decoded { .. }
            );
            members.push(FormMember {
                observation,
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
            serializer.slot()?,
            reference,
            Some(resource.clone()),
        )?;
    } else {
        facts.missing_reference(scope, serializer.slot()?, false)?;
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
                        source.format == cadmpeg_ir::codec_id!("f3d")
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
                resource: resource.clone(),
                availability,
            },
        });
        for cage in matches {
            facts.observe(FormCageObservation {
                paths: vec![path.clone()],
                binding: FormCageBinding::Decoded {
                    cage: cage.id.clone(),
                    resource: Some(resource.clone()),
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
        let foreign = matches!(
            &serializer.reference,
            Some(FormReference::CrossSegment { .. } | FormReference::CrossDocument { .. })
        );
        if serializer
            .reference
            .as_ref()
            .is_some_and(foreign_type_conflict)
        {
            note(
                losses,
                F3dLossCode::FeatureFormReferenceTypeConflict,
                &source.scope.id,
                "foreign reference type claims conflict",
            );
        }
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
    groups: &mut ReferenceGroups,
    losses: &mut Vec<LossNote>,
) -> Result<FormMembership, CodecError> {
    let FormSource {
        bytes,
        records,
        scope,
        stream,
        ..
    } = source;
    let types = match crate::design::decode::meta::metadata_for_bulk_stream(scan, stream) {
        Ok(Some(meta)) => Some(FormRecordTypes::of(&meta)),
        Ok(None) | Err(CodecError::Malformed(_)) => None,
        Err(error) => return Err(error),
    };
    let mut serializers = form_serializers(ctx, bytes, records, types.as_ref())?;
    bind_serializers(ctx, source, &mut serializers, groups)?;
    if let Some(types) = &types {
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
            facts.list(
                scope,
                scope.record_index,
                FormListRole::CageCatalogCompanions,
                &objects
                    .iter()
                    .map(|entry| entry.companion)
                    .collect::<Vec<_>>(),
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
    body_membership(
        ctx,
        scan,
        source,
        (&types, &serializers),
        (facts, groups),
        ordinary,
        losses,
    )
}

fn body_membership(
    ctx: &DecodeContext<'_>,
    scan: &ContainerScan,
    source: FormSource<'_>,
    admission: (&FormRecordTypes, &FormCageSerializers),
    state: (&mut FormFacts, &mut ReferenceGroups),
    ordinary: Option<FormMembership>,
    losses: &mut Vec<LossNote>,
) -> Result<FormMembership, CodecError> {
    let (types, serializers) = admission;
    let (facts, groups) = state;
    let FormSource {
        bytes,
        records,
        scope,
        ..
    } = source;
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
        let mut alternatives = form_body_references(ctx, bytes, records, body)?;
        bind_body_runs(ctx, source, body, &mut alternatives, groups)?;
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
            let unresolved_owner =
                alternatives
                    .iter()
                    .filter(|run| !run.is_null_only())
                    .any(|run| {
                        run.references[0]
                            .as_ref()
                            .and_then(FormReference::local)
                            .is_none()
                    })
                    || alternatives.iter().all(FormBodyRun::is_null_only);
            if !unresolved_owner {
                continue;
            }
            closed = false;
        }
        let competing = alternatives
            .iter()
            .filter(|run| !run.is_null_only())
            .count();
        let unique = competing == 1;
        if competing == 0 {
            closed = false;
        } else if !unique {
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
            let null_only = run.is_null_only();
            let group = run
                .group
                .ok_or_else(|| CodecError::malformed("Form body group is unavailable"))?;
            let references = run.references;
            let mut local = [None; FORM_BODY_REFERENCE_COUNT];
            let mut failed = false;
            for (ordinal, reference) in references.iter().enumerate() {
                let Some(reference) = reference else {
                    facts.missing_reference(
                        scope,
                        group.slot(u32::try_from(ordinal).map_err(CodecError::malformed)?),
                        failed,
                    )?;
                    failed = true;
                    continue;
                };
                facts.reference(
                    ctx,
                    scope,
                    group.slot(u32::try_from(ordinal).map_err(CodecError::malformed)?),
                    reference,
                    None,
                )?;
                if foreign_type_conflict(reference) {
                    note(
                        losses,
                        F3dLossCode::FeatureFormReferenceTypeConflict,
                        &scope.id,
                        "foreign reference type claims conflict",
                    );
                }
                match reference.local() {
                    None if matches!(reference, FormReference::Null) => {}
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
                        let valid_type = types.reference_matches(target, expected, inline);
                        if valid_type {
                            local[ordinal] = Some(target);
                        } else {
                            note(
                                losses,
                                F3dLossCode::FeatureFormReferenceTypeConflict,
                                &scope.id,
                                "an inline reference type conflicts with its target role or registration",
                            );
                        }
                    }
                }
            }
            if null_only {
                continue;
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
                serializers,
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
            let binding_proven =
                owner_proven && local[2].is_some_and(|target| types.surfaces.contains(&target));
            if unique && binding_proven {
                selected = observed;
            }
            if !unique || !valid || !binding_proven {
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

enum FormTrackingRun {
    UnreadableOwner,
    Parsed {
        owner: Box<FormReference>,
        members: Option<FormTrackingMembers>,
    },
}

struct FormTrackingMembers {
    members: Vec<Option<FormReference>>,
    fixed_members: Option<Vec<Option<FormReference>>>,
}

fn form_tracking_members(
    ctx: &DecodeContext<'_>,
    bytes: &[u8],
    records: &IndexedRecordOffsets,
    group: u32,
) -> Result<Option<FormTrackingRun>, CodecError> {
    let framed = (|| {
        let (start, paired) = one_indexed_frame(records, group)?;
        let frame = bytes.get(start..paired)?;
        if frame.get(selection_prefix::ZERO_RUN_10..selection_prefix::OWNER_MARKER)
            != Some(&[0; 10])
        {
            return None;
        }
        Some(frame)
    })();
    let Some(frame) = framed else {
        return Ok(None);
    };
    if let Some(limit) = ctx.resource_refusal() {
        return Err(CodecError::ResourceLimit(limit));
    }
    let mut at = selection_prefix::OWNER_MARKER;
    let fixed_owner = if frame.get(selection_prefix::OWNER_MARKER)
        == Some(&selection_prefix::OWNER_MARKER_VALUE)
        && frame.get(selection_prefix::OWNER_ZERO_TAIL..selection_prefix::MEMBER_COUNT)
            == Some(&[0; 2])
    {
        View::u64_le_at(frame, selection_prefix::OWNER_SCOPE_RECORD_INDEX)
    } else {
        None
    };
    let owner = if let Some(target) = fixed_owner {
        at = selection_prefix::MEMBER_COUNT;
        FormReference::Local {
            target,
            inline_type_guid: None,
        }
    } else {
        let Some(owner) = take_form_reference(ctx, frame, &mut at)? else {
            return Ok(Some(FormTrackingRun::UnreadableOwner));
        };
        owner
    };
    let parsed = (|| {
        let count = View::u32_le_at(frame, at)?;
        at = at.checked_add(4)?;
        let count_usize = bounded_len(u64::from(count), 1, frame.len().checked_sub(at)?)?;
        Some((count_usize, at))
    })();
    let Some((count_usize, start)) = parsed else {
        return Ok(Some(FormTrackingRun::Parsed {
            owner: Box::new(owner),
            members: None,
        }));
    };
    let mut members = ctx.alloc_filled(count_usize, None, "Form tracking members")?;
    let mut at = start;
    for member in &mut members {
        let Some(reference) = take_form_reference(ctx, frame, &mut at)? else {
            break;
        };
        *member = Some(reference);
    }
    let variable_framed = members.iter().all(Option::is_some)
        && frame
            .len()
            .checked_sub(at)
            .is_some_and(|tail| matches!(tail, 52 | 53));
    let fixed_slots = !variable_framed
        && count_usize
            .checked_mul(11)
            .and_then(|len| start.checked_add(len))
            .and_then(|end| frame.len().checked_sub(end))
            .is_some_and(|tail| matches!(tail, 52 | 53));
    let fixed_members = if fixed_slots {
        let mut fixed = ctx.alloc_filled(count_usize, None, "Form fixed tracking members")?;
        at = start;
        for member in &mut fixed {
            let end = at + 11;
            let mut cursor = at;
            *member = take_form_reference(ctx, &frame[..end], &mut cursor)?.filter(|reference| {
                cursor == end
                    || (matches!(reference, FormReference::Null)
                        && frame[cursor..end].iter().all(|byte| *byte == 0))
            });
            at = end;
        }
        Some(fixed)
    } else {
        None
    };
    Ok(Some(FormTrackingRun::Parsed {
        owner: Box::new(owner),
        members: Some(FormTrackingMembers {
            members,
            fixed_members,
        }),
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
    fn reference_matches(&self, target: u64, expected: Option<&str>, inline: Option<&str>) -> bool {
        match self.registered.get(&target) {
            Some(None) => false,
            Some(Some(index)) => {
                let Some(registered) = self.type_guids.get(*index) else {
                    return false;
                };
                expected.is_none_or(|guid| registered.eq_ignore_ascii_case(guid))
                    && inline.is_none_or(|guid| registered.eq_ignore_ascii_case(guid))
            }
            None => inline.is_none(),
        }
    }
}

type FormReference = crate::bytes::Reference<String, String>;

fn foreign_type_conflict(reference: &FormReference) -> bool {
    matches!(reference, FormReference::CrossDocument { inline_type_guid: Some(inline), target_type_guid: Some(external), .. } if !inline.eq_ignore_ascii_case(external))
}

fn take_form_reference(
    ctx: &DecodeContext<'_>,
    bytes: &[u8],
    cursor: &mut usize,
) -> Result<Option<FormReference>, CodecError> {
    if let Some(limit) = ctx.resource_refusal() {
        return Err(CodecError::ResourceLimit(limit));
    }
    Ok(crate::bytes::take_reference_lossless(bytes, cursor)
        .map(crate::bytes::Reference::into_owned))
}

struct FormBodyRun {
    offset: usize,
    references: [Option<FormReference>; FORM_BODY_REFERENCE_COUNT],
    group: Option<BoundGroup>,
}

impl FormBodyRun {
    fn is_null_only(&self) -> bool {
        self.references
            .iter()
            .all(|reference| matches!(reference, Some(FormReference::Null)))
    }
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
            *reference = Some(parsed);
        }
        runs.push(FormBodyRun {
            offset: at,
            references,
            group: None,
        });
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
    group: Option<BoundGroup>,
}

impl FormSerializer {
    fn slot(&self) -> Result<ReferenceSlot, CodecError> {
        self.group
            .map(|group| group.slot(0))
            .ok_or_else(|| CodecError::malformed("Form serializer group is unavailable"))
    }
}

fn profile_reference(
    ctx: &DecodeContext<'_>,
    scope: &DesignParameterScope,
    slot: ReferenceSlot,
    reference: Option<&FormReference>,
    unvisited: bool,
) -> Result<FormNativeReference, CodecError> {
    if let Some(reference) = reference {
        native_reference(ctx, scope, slot, reference, None)
    } else {
        let target = if unvisited {
            FormNativeReferenceTarget::Unvisited {}
        } else {
            FormNativeReferenceTarget::Unreadable {}
        };
        Ok(FormNativeReference::new(
            group_record_id(ctx, scope, u64::from(slot.source))?,
            slot.role,
            slot.ordinal,
            target,
        ))
    }
}

fn bind_serializers(
    ctx: &DecodeContext<'_>,
    source: FormSource<'_>,
    serializers: &mut FormCageSerializers,
    groups: &mut ReferenceGroups,
) -> Result<(), CodecError> {
    let mut entries = Vec::new();
    for serializer in serializers
        .entries
        .values_mut()
        .flatten()
        .chain(serializers.unbound.iter_mut())
    {
        let slot =
            ReferenceSlot::ungrouped(serializer.record, FormReferenceRole::SerializerSurface, 0);
        let fact = profile_reference(
            ctx,
            source.scope,
            slot,
            serializer.reference.as_ref(),
            false,
        )?;
        let profile = group_json_key(ctx, &(fact, &serializer.name))?;
        entries
            .try_reserve(1)
            .map_err(|_| group_allocation_error::<(Vec<u8>, usize, &mut FormSerializer)>(ctx, 1))?;
        let index = entries.len();
        entries.push((profile, index, serializer));
    }
    entries.sort_unstable_by(|left, right| left.0.cmp(&right.0).then(left.1.cmp(&right.1)));
    for (_, _, serializer) in entries {
        serializer.group = Some(groups.bind(
            ctx,
            source.scope,
            serializer.record,
            FormReferenceRole::SerializerSurface,
            PhysicalGroup::Serializer {
                stream: group_string(ctx, source.stream)?,
                frame: serializer.offset,
            },
        )?);
    }
    Ok(())
}

fn bind_body_runs(
    ctx: &DecodeContext<'_>,
    source: FormSource<'_>,
    body: u32,
    runs: &mut [FormBodyRun],
    groups: &mut ReferenceGroups,
) -> Result<(), CodecError> {
    if runs.is_empty() {
        return Ok(());
    }
    let (frame, _) = one_indexed_frame(source.records, body)
        .ok_or_else(|| CodecError::malformed("Form body frame is unavailable"))?;
    let mut profiles = Vec::new();
    profiles
        .try_reserve(runs.len())
        .map_err(|_| group_allocation_error::<(Vec<u8>, usize)>(ctx, runs.len()))?;
    for (index, run) in runs.iter().enumerate() {
        let mut facts = Vec::new();
        facts.try_reserve_exact(run.references.len()).map_err(|_| {
            group_allocation_error::<FormNativeReference>(ctx, run.references.len())
        })?;
        let mut failed = false;
        for (ordinal, reference) in run.references.iter().enumerate() {
            let slot = ReferenceSlot::ungrouped(
                body,
                FormReferenceRole::Body,
                u32::try_from(ordinal).map_err(CodecError::malformed)?,
            );
            facts.push(profile_reference(
                ctx,
                source.scope,
                slot,
                reference.as_ref(),
                failed,
            )?);
            failed |= reference.is_none();
        }
        profiles.push((group_json_key(ctx, &facts)?, index));
    }
    profiles.sort_unstable_by(|left, right| left.0.cmp(&right.0).then(left.1.cmp(&right.1)));
    for (_, index) in profiles {
        let run = &mut runs[index];
        run.group = Some(groups.bind(
            ctx,
            source.scope,
            body,
            FormReferenceRole::Body,
            PhysicalGroup::Body {
                stream: group_string(ctx, source.stream)?,
                frame,
                envelope: run.offset,
            },
        )?);
    }
    Ok(())
}
struct FormCageSerializers {
    ordered: Vec<u64>,
    entries: HashMap<u64, Vec<FormSerializer>>,
    unbound: Vec<FormSerializer>,
    primary_offsets: HashMap<u32, HashSet<usize>>,
}

impl FormCageSerializers {
    fn merge(&mut self, other: Self) {
        let mut offsets = self
            .entries
            .values()
            .flatten()
            .chain(self.unbound.iter())
            .map(|entry| entry.offset)
            .collect::<HashSet<_>>();
        for (record, primary) in other.primary_offsets {
            self.primary_offsets
                .entry(record)
                .or_default()
                .extend(primary);
        }
        for (surface, entries) in other.entries {
            for entry in entries {
                if offsets.insert(entry.offset) {
                    self.entries.entry(surface).or_default().push(entry);
                }
            }
        }
        for entry in other.unbound {
            if offsets.insert(entry.offset) {
                self.unbound.push(entry);
            }
        }
        let mut ordered = self
            .entries
            .iter_mut()
            .filter_map(|(&surface, entries)| {
                entries.sort_by_key(|entry| entry.offset);
                entries.first().map(|entry| (entry.offset, surface))
            })
            .collect::<Vec<_>>();
        ordered.sort_unstable();
        self.ordered = ordered.into_iter().map(|(_, surface)| surface).collect();
        self.unbound.sort_by_key(|entry| entry.offset);
        self.mark_ambiguity();
    }

    fn mark_ambiguity(&mut self) {
        for entry in self
            .entries
            .values_mut()
            .flatten()
            .chain(self.unbound.iter_mut())
        {
            entry.ambiguous |= self
                .primary_offsets
                .get(&entry.record)
                .is_some_and(|offsets| offsets.len() > 1);
        }
    }

    fn validate_types(&mut self, types: &FormRecordTypes) {
        for (&surface, entries) in &mut self.entries {
            for entry in entries {
                entry.valid &= types.reference_matches(
                    u64::from(entry.record),
                    Some(FORM_SERIALIZER_TYPE_GUID),
                    None,
                ) && (!types.registered.contains_key(&u64::from(entry.record))
                    || types.serializers.contains(&entry.record));
                if let Some((_, inline)) = entry.reference.as_ref().and_then(FormReference::local) {
                    entry.valid &=
                        types.reference_matches(surface, Some(FORM_SURFACE_TYPE_GUID), inline);
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
    let mut primary_offsets = HashMap::<u32, HashSet<usize>>::new();
    for (offset, _) in &frames {
        if let Some(record) = View::u32_le_at(bytes, offset + 7) {
            primary_offsets.entry(record).or_default().insert(*offset);
        }
    }
    let mut serializers = collect_form_cage_serializers(ctx, bytes, records, frames)?;
    for (record, offsets) in primary_offsets {
        serializers
            .primary_offsets
            .entry(record)
            .or_default()
            .extend(offsets);
    }
    serializers.mark_ambiguity();
    Ok(serializers)
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
    let mut primary_offsets = HashMap::<u32, HashSet<usize>>::new();
    for &offset in &offsets {
        let primary_type = bytes
            .get(offset + 4..offset + 7)
            .and_then(|tag| std::str::from_utf8(tag).ok())
            .and_then(|tag| tag.parse::<usize>().ok())
            .and_then(|tag| tag.checked_sub(256))
            .and_then(|index| types.type_guids.get(index));
        if primary_type.is_some_and(|guid| guid.eq_ignore_ascii_case(FORM_SERIALIZER_TYPE_GUID)) {
            if let Some(record) = View::u32_le_at(bytes, offset + 7) {
                primary_offsets.entry(record).or_default().insert(offset);
            }
        }
    }
    let mut serializers = collect_form_cage_serializers(
        ctx,
        bytes,
        records,
        offsets
            .into_iter()
            .map(|offset| (offset, FormSerializerFrame::Ordinary)),
    )?;
    serializers.validate_types(types);
    for (record, offsets) in primary_offsets {
        serializers
            .primary_offsets
            .entry(record)
            .or_default()
            .extend(offsets);
    }
    serializers.mark_ambiguity();
    Ok(serializers)
}

fn form_serializers(
    ctx: &DecodeContext<'_>,
    bytes: &[u8],
    records: &IndexedRecordOffsets,
    types: Option<&FormRecordTypes>,
) -> Result<FormCageSerializers, CodecError> {
    let mut serializers = form_cage_serializers(ctx, bytes, records)?;
    if let Some(types) = types {
        serializers.merge(form_typed_serializers(ctx, bytes, records, types)?);
        serializers.validate_types(types);
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
                group: None,
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
            group: None,
        });
    }
    let mut primary_offsets = HashMap::<u32, HashSet<usize>>::new();
    for entry in entries.values().flatten().chain(unbound.iter()) {
        primary_offsets
            .entry(entry.record)
            .or_default()
            .insert(entry.offset);
    }
    for entry in entries.values_mut().flatten().chain(unbound.iter_mut()) {
        let offsets = records.offsets(entry.record);
        entry.ambiguous = offsets.first().is_some_and(|first| *first < entry.offset)
            || primary_offsets
                .get(&entry.record)
                .is_some_and(|offsets| offsets.len() > 1);
    }
    Ok(FormCageSerializers {
        ordered,
        entries,
        unbound,
        primary_offsets,
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
    let reference = take_form_reference(ctx, frame, &mut cursor)?;
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

fn ordered_observations(
    members: &[FormMember],
    evidence: &[FormOrderEvidence],
    closures: &[FormClosureEvidence],
    source_only: bool,
    use_positions: bool,
    require_unique: bool,
) -> Option<Vec<FormObservationIndex>> {
    let mut pending = members
        .iter()
        .map(|member| member.observation)
        .collect::<HashSet<_>>();
    let mut predecessors = pending
        .iter()
        .copied()
        .map(|index| (index, HashSet::new()))
        .collect::<HashMap<_, _>>();
    for (_, members) in evidence
        .iter()
        .map(|claim| (claim.order, claim.members.as_slice()))
        .chain(
            closures
                .iter()
                .map(|claim| (claim.order, claim.members.as_slice())),
        )
        .filter(|(order, _)| !source_only || *order == FormKnownOrder::Source)
    {
        let sequence = members
            .iter()
            .copied()
            .filter(|index| pending.contains(index))
            .collect::<Vec<_>>();
        for pair in sequence.windows(2) {
            predecessors.entry(pair[1]).or_default().insert(pair[0]);
        }
    }
    if use_positions {
        let mut positions = members
            .iter()
            .filter_map(|member| member.ordinal.map(|ordinal| (ordinal, member.observation)))
            .collect::<Vec<_>>();
        positions.sort_unstable_by_key(|(ordinal, _)| *ordinal);
        for pair in positions.windows(2) {
            predecessors.entry(pair[1].1).or_default().insert(pair[0].1);
        }
    }
    let mut ordered = Vec::new();
    while !pending.is_empty() {
        let mut ready = pending
            .iter()
            .copied()
            .filter(|index| {
                predecessors
                    .get(index)
                    .is_none_or(|before| before.is_disjoint(&pending))
            })
            .collect::<Vec<_>>();
        if ready.is_empty() || (require_unique && ready.len() != 1) {
            return None;
        }
        ready.sort_unstable_by_key(|index| index.0);
        let next = ready[0];
        pending.remove(&next);
        ordered.push(next);
    }
    Some(ordered)
}

#[cfg(test)]
mod tests;
