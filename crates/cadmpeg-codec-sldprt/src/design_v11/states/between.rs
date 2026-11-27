// SPDX-License-Identifier: Apache-2.0
//! The states between the features of each configuration whose partition
//! the decoder reads (§5.4).
//!
//! A configuration's `moPMarkRecord_c` records join its features to the
//! marks of its partition's main line (`docs/formats/sldprt.md` §3.4). The
//! state at a joined mark is the state after the last of the mark's features
//! in the timeline, `of: State(After(feature))`, where the timeline order of
//! the configuration's recorded features agrees with the order of their
//! marks. The replay writes each such state as a partition
//! transmit, which decodes as the configuration's current partition does.
//! The states chain back from the configuration's latest state: the active
//! configuration's chain is based on `Current`, another configuration's on
//! the evaluation of its state at the end of the tree, and each state on the
//! next later stated one.

#[cfg(test)]
mod tests;

use std::collections::btree_map::Entry;
use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};

use cadmpeg_core::decode::{
    u64_from_index, DecodeContext, LimitScope, ResourceDimension, ResourceFailure, ResourceLimit,
};
use cadmpeg_core::CodecError;
use cadmpeg_ir::annotations::Annotations;
use cadmpeg_ir::design::evaluation::{
    ChainBack, Evaluation, EvaluationStatus, HeldState, StateBase,
};
use cadmpeg_ir::design::feature::{Feature, FeatureOperation, OperationFamily};
use cadmpeg_ir::design::ids::EvaluationId;
use cadmpeg_ir::design::names::NameEntry;
use cadmpeg_ir::design::refs::{EntityRef, Evaluated, Member, Position};
use cadmpeg_ir::document::{CadIr, Topology};
use cadmpeg_ir::features::{ConfigurationId, FeatureId};
use cadmpeg_ir::ids::{BodyId, Identity, ProductDefinitionId};
use cadmpeg_ir::report::loss::LossNote;
use cadmpeg_parasolid::deltas::replay::{MainLine, StateError};
use cadmpeg_parasolid::records::{DeltasRecords, PrimaryRecords};

use super::{end_of_tree, stateless, topology_ids, topology_len, Reach, Scope};
use crate::container::{contains_ascii_case_insensitive, ContainerScan, Section};
use crate::design_v11::names::{Names, Persistent};
use crate::loss::SldprtLossCode;
use crate::records::{FeatureHistory, FeatureInputLane, ObjectId};
use crate::rollback::{MarkRecord, StoredMarks};

/// What the states between features are read from.
pub(in crate::design_v11) struct Sources<'a> {
    pub(in crate::design_v11) scan: &'a ContainerScan<'a>,
    pub(in crate::design_v11) histories: &'a [FeatureHistory],
    pub(in crate::design_v11) lanes: &'a [FeatureInputLane],
    /// The mark records of each partition site.
    pub(in crate::design_v11) marks: &'a StoredMarks,
}

/// The latest state of each configuration: the current state, with its name
/// table, and the evaluations of the other configurations' states.
pub(in crate::design_v11) struct Latest<'a> {
    /// The current state's position.
    pub(in crate::design_v11) at: &'a Position,
    /// The current state's name table.
    pub(in crate::design_v11) names: &'a BTreeMap<EntityRef, NameEntry>,
    /// The evaluations of the states of the other configurations.
    pub(in crate::design_v11) evaluations: &'a [Evaluation],
}

/// The states between features that [`states`] stated.
#[derive(Default)]
pub(in crate::design_v11) struct Between {
    pub(in crate::design_v11) evaluations: Vec<Evaluation>,
    /// The number of records the evaluations hold.
    pub(in crate::design_v11) records: usize,
    /// The annotations of the records of the evaluations' deltas.
    pub(in crate::design_v11) annotations: Annotations,
    losses: Vec<LossNote>,
}

/// The states between the features of each configuration whose latest state
/// is stated and whose partition stores its earlier states. Each
/// configuration whose stored states are not all stated charges one note to
/// `losses`. A resource ceiling fails the call.
pub(in crate::design_v11) fn states(
    ctx: &DecodeContext<'_>,
    ir: &CadIr,
    scope: &Scope<'_>,
    latest: &Latest<'_>,
    names: &Names<'_>,
    sources: &Sources<'_>,
    losses: &mut Vec<LossNote>,
) -> Result<Between, CodecError> {
    let reader = Reader {
        ctx,
        timeline: Timeline::new(scope.features, sources.histories),
        names,
        definition: scope.definition,
    };
    let mut between = Between::default();
    let active = crate::container::select_active_parasolid_site(sources.scan);
    if let Some(active) = active.filter(|_| !scope.current.is_empty()) {
        let end = End {
            configuration: scope.active,
            at: latest.at,
            base: StateBase::Current(latest.at.clone()),
            status: EvaluationStatus::Current,
            bodies: scope.current,
            held: Held::Current {
                geometry: &ir.geometry,
                names: latest.names,
            },
        };
        let site = Site {
            section: active.section,
            qualifier: qualified(active.section, scope.current),
        };
        let unread = reader.site(sources, &site, &end, &mut between)?;
        between.losses.extend(unread.note(end.configuration));
    }
    let (end_at, _) = end_of_tree(scope);
    for evaluation in latest.evaluations {
        let (Some(configuration), Some(content)) = (&evaluation.configuration, &evaluation.content)
        else {
            continue;
        };
        if content.base != StateBase::Empty {
            continue;
        }
        let bodies = content
            .delta
            .records
            .bodies
            .iter()
            .map(|body| body.id.clone())
            .collect::<Vec<_>>();
        let site = sources.scan.sections().find_map(|section| {
            let name = section.name()?;
            if !crate::container::is_partition_section(name) {
                return None;
            }
            Some(Site {
                section,
                qualifier: Some(qualified(section, &bodies)?),
            })
        });
        let Some(site) = site else {
            continue;
        };
        let end = End {
            configuration: Some(configuration),
            at: &end_at,
            base: StateBase::Evaluation(evaluation.id.clone()),
            status: evaluation.status,
            bodies: &bodies,
            held: Held::Evaluation {
                records: &content.delta.records,
                names: &content.delta.names,
            },
        };
        let unread = reader.site(sources, &site, &end, &mut between)?;
        between.losses.extend(unread.note(end.configuration));
    }
    losses.append(&mut between.losses);
    Ok(between)
}

/// The site key of `section` where every body of `bodies` carries it as its
/// qualifier.
fn qualified(section: Section<'_>, bodies: &[BodyId]) -> Option<String> {
    let site = section.site_key();
    let tail = format!("@{site}");
    (!bodies.is_empty() && bodies.iter().all(|body| body.as_str().ends_with(&tail))).then_some(site)
}

/// The partition site of one configuration: its section, and the qualifier
/// of its ids where it is not the active site.
struct Site<'a> {
    section: Section<'a>,
    qualifier: Option<String>,
}

/// The latest state of one configuration, from which its earlier states
/// chain back.
struct End<'a> {
    /// The configuration, or `None` in a scope without configurations.
    configuration: Option<&'a ConfigurationId>,
    /// The state's position.
    at: &'a Position,
    /// The base of the latest earlier state.
    base: StateBase,
    status: EvaluationStatus,
    bodies: &'a [BodyId],
    held: Held<'a>,
}

/// Where the records and names of a configuration's latest state are.
enum Held<'a> {
    /// The records of the current geometry that the state's bodies reach.
    Current {
        geometry: &'a Topology,
        names: &'a BTreeMap<EntityRef, NameEntry>,
    },
    /// An evaluation's records, based on `Empty`.
    Evaluation {
        records: &'a Topology,
        names: &'a BTreeMap<EntityRef, Option<NameEntry>>,
    },
}

impl End<'_> {
    fn held(&self, ctx: &DecodeContext<'_>) -> Result<HeldState, CodecError> {
        Ok(match &self.held {
            Held::Current { geometry, names } => HeldState {
                records: Reach::new(ctx, geometry)?.state(ctx, self.bodies)?,
                names: (*names).clone(),
            },
            Held::Evaluation { records, names } => HeldState {
                records: (*records).clone(),
                names: names
                    .iter()
                    .filter_map(|(entity, entry)| Some((entity.clone(), entry.clone()?)))
                    .collect(),
            },
        })
    }

    /// The timeline index of the feature the latest state is after.
    fn feature(&self, timeline: &Timeline<'_>) -> Option<usize> {
        match self.at {
            Position::After(Member::Feature(id)) => timeline
                .features
                .iter()
                .position(|feature| feature.id == *id),
            _ => None,
        }
    }
}

/// What the states of every configuration are read with.
struct Reader<'a, 'c> {
    ctx: &'a DecodeContext<'c>,
    timeline: Timeline<'a>,
    names: &'a Names<'a>,
    definition: &'a ProductDefinitionId,
}

impl Reader<'_, '_> {
    /// The states between the features of the configuration whose latest
    /// state is `end`, read from its partition `site`. Returns the states
    /// the site stores that are not stated.
    fn site(
        &self,
        sources: &Sources<'_>,
        site: &Site<'_>,
        end: &End<'_>,
        between: &mut Between,
    ) -> Result<Unread, CodecError> {
        let streams = site.section.ps_streams();
        let stream = |kind: &str| {
            streams
                .iter()
                .find(|stream| contains_ascii_case_insensitive(&stream.header.description, kind))
        };
        let Some(deltas) = stream("deltas") else {
            return Ok(Unread::default());
        };
        let records = match DeltasRecords::read(&deltas.payload) {
            Ok(records) => records,
            Err(error @ CodecError::ResourceLimit(_)) => return Err(error),
            Err(error) => {
                return Ok(Unread::stopped(format!(
                    "its deltas transmit does not read ({error})"
                )));
            }
        };
        let line = match MainLine::read(&records) {
            Ok(line) => line,
            Err(error) => {
                return Ok(Unread::stopped(format!(
                    "its main line does not read ({error})"
                )));
            }
        };
        if line.marks().len() == 1 {
            return Ok(Unread::default());
        }
        let has_intermediate_mark = line.marks().len() > 2;
        let marks = match site
            .section
            .name()
            .and_then(|partition| sources.marks.get(partition))
        {
            Some(Ok(marks)) => marks,
            _ if !has_intermediate_mark => return Ok(Unread::default()),
            Some(Err(unread)) => return Ok(Unread::stopped(unread.to_string())),
            None => {
                return Ok(Unread::stopped(
                    "its deltas transmit frames no current mark".to_owned(),
                ));
            }
        };
        let Some(lane) = marks
            .lane
            .as_ref()
            .and_then(|id| sources.lanes.iter().find(|lane| &lane.id == id))
        else {
            return Ok(if has_intermediate_mark {
                Unread::stopped("its feature stream has no serialized names".to_owned())
            } else {
                Unread::default()
            });
        };
        let names = object_names(lane);
        let mut wanted = wanted(
            &self.timeline,
            &line,
            &marks.records,
            &names,
            end.feature(&self.timeline),
        );
        if marks.inferred
            && matches!(end.base, StateBase::Evaluation(_))
            && (has_intermediate_mark || !wanted.states.is_empty())
        {
            between
                .losses
                .push(crate::rollback::mark_class_inferred(&marks.configuration));
        }
        if wanted.states.is_empty() {
            return Ok(wanted.unread);
        }
        within_chain_depth(self.ctx, wanted.states.len(), &end.base)?;
        let partition = streams.iter().find(|stream| {
            let description = &stream.header.description;
            contains_ascii_case_insensitive(description, "partition")
                && !contains_ascii_case_insensitive(description, "deltas")
        });
        let Some(partition) = partition else {
            wanted.refuse(&self.timeline, "the site stores no partition transmit");
            return Ok(wanted.unread);
        };
        let primary = match PrimaryRecords::read(&partition.payload) {
            Ok(Some(primary)) => primary,
            Ok(None) => {
                wanted.refuse(&self.timeline, "the partition transmit has no header");
                return Ok(wanted.unread);
            }
            Err(error @ CodecError::ResourceLimit(_)) => return Err(error),
            Err(error) => {
                wanted.refuse(
                    &self.timeline,
                    &format!("the partition transmit does not read ({error})"),
                );
                return Ok(wanted.unread);
            }
        };
        let read = |transmit: &[u8], feature: usize| self.read(site, transmit, feature);
        self.replay(end, &line, &primary, &mut wanted, read, between)?;
        Ok(wanted.unread)
    }

    /// Decode the state after the feature `feature` from `transmit`, a state
    /// of `site`: its records, the names the features through `feature`
    /// give them, and their annotations.
    fn read(
        &self,
        site: &Site<'_>,
        transmit: &[u8],
        feature: usize,
    ) -> Result<Result<Read, String>, CodecError> {
        let decoded = match crate::decode::replayed_states::decode_state(
            self.ctx,
            site.section,
            site.qualifier.as_deref(),
            transmit,
        )? {
            Ok(decoded) => decoded,
            Err(reason) => return Ok(Err(reason)),
        };
        let records = Reach::new(self.ctx, &decoded.geometry)?.state(self.ctx, &decoded.bodies)?;
        let depended = self.timeline.features[..=feature]
            .iter()
            .map(|feature| &feature.id)
            .collect::<HashSet<_>>();
        let names = self.names.state_entries(
            Persistent {
                faces: &decoded.faces,
                bodies: &decoded.body_identities,
            },
            &records,
            &decoded.bodies,
            |id| depended.contains(id),
        );
        Ok(Ok(Read {
            held: HeldState { records, names },
            annotations: decoded.annotations,
            losses: decoded.losses,
        }))
    }

    /// Read the states `wanted` names from the replay of `line` over
    /// `primary`, from the latest back, and state each in `end`'s chain.
    /// `read` decodes the transmit of the state after one feature, or gives
    /// the reason it does not.
    fn replay(
        &self,
        end: &End<'_>,
        line: &MainLine<'_>,
        primary: &PrimaryRecords<'_>,
        wanted: &mut Wanted,
        mut read: impl FnMut(&[u8], usize) -> Result<Result<Read, String>, CodecError>,
        between: &mut Between,
    ) -> Result<(), CodecError> {
        let marks = wanted.states.keys().copied().collect::<BTreeSet<_>>();
        let states = match line.states(primary, marks) {
            Ok(states) => states,
            Err(error) => {
                wanted.refuse(&self.timeline, &format!("the replay refuses ({error})"));
                return Ok(());
            }
        };
        let timeline = &self.timeline;
        let mut chain = None;
        for state in states {
            let (mark, transmit) = match state {
                Ok((mark, transmit)) => (mark.identifier(), transmit),
                Err(StateError::Transmit { mark, error }) => {
                    if let Some(feature) = wanted.states.remove(&mark) {
                        wanted.unread.feature(
                            format!("the replay writes no transmit of their state ({error})"),
                            timeline.label(feature),
                        );
                    }
                    continue;
                }
                Err(StateError::Replay(error)) => {
                    wanted.refuse(timeline, &format!("the replay refuses ({error})"));
                    return Ok(());
                }
            };
            let Some(feature) = wanted.states.remove(&mark) else {
                continue;
            };
            let state = match read(&transmit, feature)? {
                Ok(state) => state,
                Err(reason) => {
                    wanted.unread.feature(
                        format!("their state does not decode ({reason})"),
                        timeline.label(feature),
                    );
                    continue;
                }
            };
            let chain = match &mut chain {
                Some(chain) => chain,
                None => chain.insert(ChainBack::new(end.base.clone(), end.held(self.ctx)?)),
            };
            let feature = &timeline.features[feature].id;
            let id = evaluation_id(end.configuration, feature);
            let content = chain.push(id.clone(), state.held);
            for mut loss in state.losses {
                let identity: &Identity = id.as_ref();
                let key = identity.key();
                cadmpeg_ir::unknown::part::rescope_give_up(&mut loss, |record| {
                    Ok(format!("{record}@state/{key}"))
                })?;
                loss.message = format!("Evaluation {id} (PMARK {mark}): {}", loss.message);
                between.losses.push(loss);
            }
            let mut held = HashSet::new();
            topology_ids(&content.delta.records, |record| {
                held.insert(record.as_str().to_owned());
            });
            between
                .annotations
                .append(
                    state
                        .annotations
                        .in_delta(&id, |record| held.contains(record)),
                )
                .map_err(CodecError::from)?;
            between.records += topology_len(&content.delta.records)
                + content.delta.removed.len()
                + content.delta.names.len();
            between.evaluations.push(Evaluation {
                id,
                scope: self.definition.clone(),
                configuration: end.configuration.cloned(),
                of: Evaluated::State(Position::After(Member::Feature(feature.clone()))),
                status: end.status,
                content: Some(content),
                native_ref: Some(format!("PMARK {mark}")),
            });
        }
        wanted.refuse(timeline, "the replay does not reach their mark");
        Ok(())
    }
}

/// Refuses a chain of `states` states past the ceiling on state base chains,
/// counting the configuration evaluation the chain is based on, where it is.
fn within_chain_depth(
    ctx: &DecodeContext<'_>,
    states: usize,
    base: &StateBase,
) -> Result<(), CodecError> {
    let limit = ctx.policy().limits.max_state_chain_depth;
    let depth =
        u64_from_index(states).checked_add(u64::from(matches!(base, StateBase::Evaluation(_))));
    let Some(additional) = depth
        .and_then(|depth| depth.checked_sub(limit))
        .filter(|&additional| additional > 0)
    else {
        return Ok(());
    };
    Err(CodecError::ResourceLimit(ResourceLimit {
        scope: LimitScope::for_dimension(ResourceDimension::RecursionDepth),
        dimension: ResourceDimension::RecursionDepth,
        reason: ResourceFailure::BudgetExceeded,
        limit,
        used: limit,
        additional,
        operation: "SLDPRT state base chain depth",
    }))
}

/// The id of the evaluation of the state after `feature` in `configuration`.
fn evaluation_id(configuration: Option<&ConfigurationId>, feature: &FeatureId) -> EvaluationId {
    let feature = Identity::from(feature.clone()).key();
    let key = match configuration {
        Some(configuration) => Identity::from(configuration.clone()).key().colon(feature),
        None => cadmpeg_ir::identity_key!("state").colon(feature),
    };
    EvaluationId::from(Identity::compose(
        &cadmpeg_ir::identity_namespace!("sldprt", "model", "evaluation"),
        key,
    ))
}

/// The serialized names of a lane that carry an object id, in stream order:
/// each name's offset and object id.
fn object_names(lane: &FeatureInputLane) -> Vec<(u64, u32)> {
    let mut named = lane
        .names
        .iter()
        .filter_map(|name| Some((name.offset, name.object_id.and_then(ObjectId::value)?)))
        .collect::<Vec<_>>();
    named.sort_unstable();
    named
}

/// The timeline's features, and what a native object id names among them.
struct Timeline<'f> {
    features: &'f [Feature],
    by_object: HashMap<u32, Object>,
    /// The features that are imported bodies, whose own state the file does
    /// not store.
    imported: HashSet<usize>,
}

/// What one native object id names.
#[derive(Clone, Copy)]
enum Object {
    /// A timeline feature, by index.
    Feature(usize),
    /// A history record that is no timeline feature.
    Other,
    /// Several history records.
    Ambiguous,
}

impl<'f> Timeline<'f> {
    fn new(features: &'f [Feature], histories: &[FeatureHistory]) -> Self {
        let at = features
            .iter()
            .enumerate()
            .filter_map(|(index, feature)| Some((feature.native_ref.as_deref()?, index)))
            .collect::<HashMap<_, _>>();
        let mut by_object = HashMap::new();
        let mut imported = HashSet::new();
        for record in histories.iter().flat_map(|history| &history.features) {
            let index = at.get(record.id.as_str()).copied();
            if let Some(index) = index.filter(|_| {
                record.input_class.as_deref() == Some(crate::design_v11::operation::BASE_BODY_CLASS)
            }) {
                imported.insert(index);
            }
            let Some(object) = record.source_value() else {
                continue;
            };
            by_object
                .entry(object)
                .and_modify(|named| *named = Object::Ambiguous)
                .or_insert(index.map_or(Object::Other, Object::Feature));
        }
        Self {
            features,
            by_object,
            imported,
        }
    }

    /// The feature can hold a stored state: its operation can change a
    /// state, and it is no cosmetic thread, which the stream serializes
    /// inside the feature it decorates.
    fn holds_state(&self, index: usize) -> bool {
        let feature = &self.features[index];
        !stateless(feature)
            && !matches!(
                feature.definition.operation,
                FeatureOperation::Unresolved {
                    family: OperationFamily::Thread
                }
            )
    }

    /// The features a mark record at offset `at` names: the holder, the
    /// nearest named object before it that can hold a state, and the
    /// features named between the holder and the record, which the stream
    /// serializes inside the holder. `None` where an object id on the way
    /// names no single history record, or no named object before the record
    /// can hold a state.
    fn namers(&self, names: &[(u64, u32)], at: usize) -> Option<Vec<usize>> {
        let at = u64::try_from(at).ok()?;
        let before = names.partition_point(|(offset, _)| *offset < at);
        let mut namers = Vec::new();
        for (_, object) in names[..before].iter().rev() {
            match self.by_object.get(object).copied()? {
                Object::Ambiguous => return None,
                Object::Other => {}
                Object::Feature(index) => {
                    namers.push(index);
                    if self.holds_state(index) {
                        return Some(namers);
                    }
                }
            }
        }
        None
    }

    fn label(&self, index: usize) -> String {
        let feature = &self.features[index];
        feature
            .name
            .clone()
            .unwrap_or_else(|| feature.id.to_string())
    }
}

/// The states of one configuration to read, and the stored states it does
/// not state.
struct Wanted {
    /// The feature each state is after, by the identifier of its mark.
    states: BTreeMap<u32, usize>,
    unread: Unread,
}

impl Wanted {
    /// Every state not yet read is not read, for `reason`.
    fn refuse(&mut self, timeline: &Timeline<'_>, reason: &str) {
        for feature in std::mem::take(&mut self.states).into_values() {
            self.unread
                .feature(reason.to_owned(), timeline.label(feature));
        }
    }
}

/// The states between features that `records` join, on the main line
/// `line` before its current mark: the state at each mark is after the last
/// of the mark's features in the timeline that is not stateless. A state is
/// not read where its feature is an imported body, or the feature of the
/// latest state, `last`, or a later one; where its feature holds records of
/// two marks; or where a recorded feature that is not stateless is on one
/// side of it in the timeline and on the other side of its mark on the main
/// line. `names` holds the configuration's named objects.
fn wanted(
    timeline: &Timeline<'_>,
    line: &MainLine<'_>,
    records: &[MarkRecord],
    names: &[(u64, u32)],
    last: Option<usize>,
) -> Wanted {
    let mut unread = Unread::default();
    let main = line
        .marks()
        .iter()
        .enumerate()
        .map(|(position, mark)| (mark.identifier(), position))
        .collect::<HashMap<_, _>>();
    let mut mark_of = BTreeMap::<usize, usize>::new();
    let mut twice = BTreeSet::new();
    let mut by_mark = BTreeMap::<usize, Vec<usize>>::new();
    for record in records {
        let Some(&position) = main.get(&record.mark) else {
            continue;
        };
        let Some(namers) = timeline.namers(names, record.at) else {
            unread.records += usize::from(position != 0);
            continue;
        };
        for feature in namers {
            match mark_of.entry(feature) {
                Entry::Vacant(entry) => {
                    entry.insert(position);
                }
                Entry::Occupied(entry) if *entry.get() != position => {
                    twice.insert(feature);
                }
                Entry::Occupied(_) => {}
            }
            by_mark.entry(position).or_default().push(feature);
        }
    }
    let stateful = |feature: usize| !stateless(&timeline.features[feature]);
    let mut states = BTreeMap::new();
    for (&position, namers) in &by_mark {
        if position == 0 {
            continue;
        }
        let Some(feature) = namers
            .iter()
            .copied()
            .filter(|&namer| stateful(namer))
            .max()
        else {
            continue;
        };
        if timeline.imported.contains(&feature) {
            continue;
        }
        let reason = if namers.iter().any(|namer| twice.contains(namer)) {
            Some("their feature holds records of two marks")
        } else if last.is_some_and(|last| feature >= last) {
            Some("their mark precedes the latest state's mark, and their feature does not precede its feature")
        } else if mark_of.iter().any(|(&other, &at)| {
            !twice.contains(&other) && stateful(other) && (other <= feature) != (at >= position)
        }) {
            Some("the order of the recorded features' marks differs from their timeline order")
        } else {
            None
        };
        match reason {
            Some(reason) => unread.feature(reason.to_owned(), timeline.label(feature)),
            None => {
                states.insert(line.marks()[position].identifier(), feature);
            }
        }
    }
    Wanted { states, unread }
}

/// One stored state, read.
struct Read {
    held: HeldState,
    annotations: Annotations,
    losses: Vec<LossNote>,
}

/// The stored states of one configuration that are not stated.
#[derive(Debug, Default, PartialEq, Eq)]
struct Unread {
    /// Why no stored state of the configuration is read.
    stop: Option<String>,
    /// The features whose stored state is not stated, by reason.
    features: BTreeMap<String, Vec<String>>,
    /// The mark records on the main line whose feature is not read.
    records: usize,
}

impl Unread {
    fn stopped(reason: String) -> Self {
        Self {
            stop: Some(reason),
            ..Self::default()
        }
    }

    fn feature(&mut self, reason: String, label: String) {
        self.features.entry(reason).or_default().push(label);
    }

    /// The note of the stored states of `configuration` that are not
    /// stated, where there are any.
    fn note(&self, configuration: Option<&ConfigurationId>) -> Option<LossNote> {
        let mut parts = self.stop.iter().cloned().collect::<Vec<_>>();
        parts.extend(self.features.iter().map(|(reason, labels)| {
            format!("the states after {} ({reason})", crate::loss::named(labels))
        }));
        if self.records > 0 {
            parts.push(format!(
                "the states at {} main-line mark record(s) whose feature is not read",
                self.records
            ));
        }
        if parts.is_empty() {
            return None;
        }
        let subject = configuration.map_or_else(
            || "The part".to_owned(),
            |configuration| format!("Configuration {configuration}"),
        );
        Some(SldprtLossCode::HistoryStatesUnread.note(format!(
            "{subject} stores states between features that are not stated: {}.",
            parts.join("; ")
        )))
    }
}
