// SPDX-License-Identifier: Apache-2.0
//! The stored states between features of a `SolidWorks` partition (§5.4).
//!
//! Feature records name persistent PMARK IDs. The linked main line supplies
//! their regeneration order and the deltas to replay from the partition.

mod join;
mod name_order;
#[cfg(test)]
mod tests;

use std::cmp::Reverse;
use std::collections::{BTreeMap, HashSet};

use cadmpeg_core::decode::DecodeContext;
use cadmpeg_core::CodecError;
use cadmpeg_ir::annotations::Annotations;
use cadmpeg_ir::design::evaluation::{
    ChainBack, Evaluation, EvaluationStatus, HeldState, StateBase,
};
use cadmpeg_ir::design::ids::EvaluationId;
use cadmpeg_ir::design::names::{NameEntry, TopoName};
use cadmpeg_ir::design::refs::{EntityRef, Evaluated, Member, Position};
use cadmpeg_ir::document::{CadIr, Topology};
use cadmpeg_ir::features::{ConfigurationId, FeatureId};
use cadmpeg_ir::ids::{BodyId, Identity, ProductDefinitionId};
use cadmpeg_ir::report::loss::LossNote;
use cadmpeg_parasolid::deltas::replay::MainLine;
use cadmpeg_parasolid::records::PrimaryRecords;

use super::{stateless, topology_ids, topology_len, Reach, Scope};
use crate::container::{contains_ascii_case_insensitive, ContainerScan, Section};
use crate::design_v11::names::{Names, Persistent};
use crate::loss::SldprtLossCode;
use crate::records::{FeatureHistory, FeatureInputLane};
use crate::rollback::{StoredMarks, StoredSite};

use self::join::{FeatureIndex, Joined, Prefix};

pub(in crate::design_v11) struct Sources<'a> {
    pub(in crate::design_v11) scan: &'a ContainerScan<'a>,
    pub(in crate::design_v11) histories: &'a [FeatureHistory],
    pub(in crate::design_v11) lanes: &'a [FeatureInputLane],
    pub(in crate::design_v11) marks: &'a StoredMarks<'a>,
}

pub(in crate::design_v11) struct Latest<'a> {
    pub(in crate::design_v11) joins: Joins<'a>,
    pub(in crate::design_v11) at: &'a Position,
    pub(in crate::design_v11) names: &'a BTreeMap<EntityRef, NameEntry>,
    pub(in crate::design_v11) evaluations: &'a mut [Evaluation],
}

#[derive(Default)]
pub(in crate::design_v11) struct Between {
    pub(in crate::design_v11) evaluations: Vec<Evaluation>,
    pub(in crate::design_v11) records: usize,
    pub(in crate::design_v11) annotations: Annotations,
}

struct Active<'a> {
    section: Section<'a>,
    stored: &'a StoredSite<'a>,
}

pub(in crate::design_v11) struct Joins<'a> {
    active: Option<Active<'a>>,
    sites: BTreeMap<String, Joined>,
}

impl Joins<'_> {
    pub(in crate::design_v11) fn native_ref(&self) -> Option<String> {
        native_ref(self.active.as_ref()?.stored)
    }

    pub(in crate::design_v11) fn earlier(&self, feature: &FeatureId) -> bool {
        self.active
            .as_ref()
            .and_then(|active| self.sites.get(&active.section.site_key()))
            .is_some_and(|joined| joined.states.values().any(|id| id == feature))
    }

    pub(in crate::design_v11) fn orders(&self) -> impl Iterator<Item = &[FeatureId]> {
        let active = self.active.as_ref().map(|site| site.section.site_key());
        let first = active.as_ref().and_then(|key| self.sites.get(key));
        first
            .into_iter()
            .chain(
                self.sites
                    .iter()
                    .filter(move |(key, _)| Some(*key) != active.as_ref())
                    .map(|(_, join)| join),
            )
            .map(|join| join.order.as_slice())
    }
}

pub(in crate::design_v11) fn joins<'a>(
    sources: &Sources<'a>,
    features: &[cadmpeg_ir::design::feature::Feature],
) -> Joins<'a> {
    let features = FeatureIndex::new(features, sources.histories);
    let section =
        crate::container::select_active_parasolid_site(sources.scan).map(|site| site.section);
    let active = section.and_then(|section| {
        let stored = sources.marks.get(&section.site_key())?;
        Some(Active { section, stored })
    });
    let sites = sources
        .marks
        .iter()
        .filter_map(|(key, stored)| Some((key.clone(), joined(&features, stored, sources.lanes)?)))
        .collect();
    Joins { active, sites }
}

fn joined(
    features: &FeatureIndex<'_>,
    stored: &StoredSite<'_>,
    lanes: &[FeatureInputLane],
) -> Option<Joined> {
    let chain = stored.chain.as_ref()?;
    let marks = stored.marks.as_ref().ok()?;
    let lane = lanes
        .iter()
        .find(|lane| Some(&lane.id) == marks.lane.as_ref())?;
    Some(features.join(chain, &marks.records, lane))
}

pub(in crate::design_v11) fn states(
    ctx: &DecodeContext<'_>,
    ir: &CadIr,
    scope: &Scope<'_>,
    latest: Latest<'_>,
    names: &Names<'_>,
    sources: &Sources<'_>,
    losses: &mut Vec<LossNote>,
) -> Result<Between, CodecError> {
    let reader = Reader {
        ctx,
        names,
        definition: scope.definition,
    };
    let mut between = Between::default();
    let mut output = Output {
        between: &mut between,
        losses,
    };
    let active = latest.joins.active.and_then(|active| {
        let joined = latest.joins.sites.get(&active.section.site_key())?.clone();
        Some((active, joined))
    });
    if let Some((Active { section, stored }, joined)) = active {
        let has_current = !scope.current.is_empty();
        let end = End {
            configuration: scope.active,
            base: if has_current {
                StateBase::Current(latest.at.clone())
            } else {
                StateBase::Empty
            },
            status: EvaluationStatus::Current,
            held: if has_current {
                Held::Current {
                    geometry: &ir.geometry,
                    names: latest.names,
                    bodies: scope.current,
                }
            } else {
                Held::Empty
            },
        };
        let site = Site {
            section,
            qualifier: qualified(section, scope.current),
        };
        let unread = reader.site(&site, stored, &end, joined, &mut output)?;
        output.losses.extend(unread.note(end.configuration));
    }

    let timeline = name_order::Timeline::new(scope.features);
    for evaluation in latest.evaluations {
        let (Some(configuration), Some(content)) = (&evaluation.configuration, &evaluation.content)
        else {
            continue;
        };
        let Some(section) =
            configuration_site(sources, scope, configuration, &content.delta.records)
        else {
            continue;
        };
        let key = section.site_key();
        let Some(stored) = sources.marks.get(&key) else {
            continue;
        };
        evaluation.native_ref = native_ref(stored);
        let Some(mut joined) = latest.joins.sites.get(&key).cloned() else {
            continue;
        };
        if let Ok(marks) = &stored.marks {
            if marks.inferred && (!joined.states.is_empty() || joined.tip.is_some()) {
                output
                    .losses
                    .push(crate::rollback::mark_class_inferred(&marks.configuration));
            }
        }
        if let Some(feature) = joined.tip.take() {
            evaluation.of = Evaluated::State(Position::After(Member::Feature(feature)));
        } else if let Evaluated::State(Position::After(Member::Feature(feature))) = &evaluation.of {
            // The configuration's inferred tip and an earlier explicit mark
            // cannot occupy the same evaluation key.
            joined.states.retain(|_, earlier| {
                if earlier == feature {
                    joined.unread.feature(
                        "their mark conflicts with the unjoined configuration tip",
                        earlier,
                    );
                    false
                } else {
                    true
                }
            });
        }
        let end = End {
            configuration: Some(configuration),
            base: StateBase::Evaluation(evaluation.id.clone()),
            status: evaluation.status,
            held: Held::Evaluation {
                records: &content.delta.records,
                names: &content.delta.names,
            },
        };
        let site = Site {
            section,
            qualifier: Some(section.site_key()),
        };
        let start = output.between.evaluations.len();
        let unread = reader.site(&site, stored, &end, joined, &mut output)?;
        output.losses.extend(unread.note(end.configuration));
        output
            .losses
            .extend(timeline.project(evaluation, &mut output.between.evaluations[start..]));
    }
    between.records += between
        .evaluations
        .iter()
        .filter_map(|evaluation| evaluation.content.as_ref())
        .map(|content| content.delta.names.len())
        .sum::<usize>();
    Ok(between)
}

fn native_ref(stored: &StoredSite<'_>) -> Option<String> {
    stored
        .chain
        .as_ref()
        .map(|chain| format!("PMARK {}", chain.current))
}

fn configuration_site<'a>(
    sources: &Sources<'a>,
    scope: &Scope<'_>,
    configuration: &ConfigurationId,
    records: &Topology,
) -> Option<Section<'a>> {
    let native = scope
        .configurations
        .iter()
        .find(|record| &record.id == configuration)?
        .native_ref
        .as_deref()?;
    let index = sources
        .histories
        .iter()
        .flat_map(|history| &history.configurations)
        .find(|record| record.id == native)
        .and_then(crate::records::Configuration::partition_index)
        .and_then(|index| usize::try_from(index).ok());
    let mut matches = sources.scan.sections().filter(|section| {
        let Some(name) = section
            .name()
            .filter(|name| crate::container::is_partition_section(name))
        else {
            return false;
        };
        if records.bodies.is_empty() {
            index.is_some() && crate::container::configuration_index(name) == index
        } else {
            let tail = format!("@{}", section.site_key());
            records
                .bodies
                .iter()
                .all(|body| body.id.as_str().ends_with(&tail))
        }
    });
    let site = matches.next()?;
    matches.next().is_none().then_some(site)
}

fn qualified(section: Section<'_>, bodies: &[BodyId]) -> Option<String> {
    let site = section.site_key();
    let tail = format!("@{site}");
    (!bodies.is_empty() && bodies.iter().all(|body| body.as_str().ends_with(&tail))).then_some(site)
}

struct Site<'a> {
    section: Section<'a>,
    qualifier: Option<String>,
}

struct End<'a> {
    configuration: Option<&'a ConfigurationId>,
    base: StateBase,
    status: EvaluationStatus,
    held: Held<'a>,
}

enum Held<'a> {
    Current {
        geometry: &'a Topology,
        names: &'a BTreeMap<EntityRef, NameEntry>,
        bodies: &'a [BodyId],
    },
    Evaluation {
        records: &'a Topology,
        names: &'a BTreeMap<EntityRef, Option<NameEntry>>,
    },
    Empty,
}

impl Held<'_> {
    fn state(&self, ctx: &DecodeContext<'_>) -> Result<HeldState, CodecError> {
        Ok(match self {
            Self::Current {
                geometry,
                names,
                bodies,
            } => HeldState {
                records: Reach::new(ctx, geometry)?.state(ctx, bodies)?,
                names: (*names).clone(),
            },
            Self::Evaluation { records, names } => HeldState {
                records: (*records).clone(),
                names: names
                    .iter()
                    .filter_map(|(entity, entry)| Some((entity.clone(), entry.clone()?)))
                    .collect(),
            },
            Self::Empty => HeldState::default(),
        })
    }

    fn has_bodies(&self) -> bool {
        match self {
            Self::Current { bodies, .. } => !bodies.is_empty(),
            Self::Evaluation { records, .. } => !records.bodies.is_empty(),
            Self::Empty => false,
        }
    }
}

struct Output<'a> {
    between: &'a mut Between,
    losses: &'a mut Vec<LossNote>,
}

struct Reader<'a, 'c> {
    ctx: &'a DecodeContext<'c>,
    names: &'a Names<'a>,
    definition: &'a ProductDefinitionId,
}

impl Reader<'_, '_> {
    fn site(
        &self,
        site: &Site<'_>,
        stored: &StoredSite<'_>,
        end: &End<'_>,
        mut joined: Joined,
        output: &mut Output<'_>,
    ) -> Result<Unread, CodecError> {
        if joined.states.is_empty() {
            return Ok(joined.unread);
        }
        let line = match MainLine::read(&stored.frame) {
            Ok(line) => line,
            Err(error) => {
                joined.refuse(&format!("their main line does not read ({error})"));
                return Ok(joined.unread);
            }
        };
        let partition = site.section.ps_streams().iter().find(|stream| {
            let description = &stream.header.description;
            contains_ascii_case_insensitive(description, "partition")
                && !contains_ascii_case_insensitive(description, "deltas")
        });
        let Some(partition) = partition else {
            joined.refuse("the site stores no partition transmit");
            return Ok(joined.unread);
        };
        let primary = match PrimaryRecords::read(&partition.payload) {
            Ok(Some(primary)) => primary,
            Ok(None) => {
                joined.refuse("the partition transmit has no header");
                return Ok(joined.unread);
            }
            Err(error @ CodecError::ResourceLimit(_)) => return Err(error),
            Err(error) => {
                joined.refuse(&format!("the partition transmit does not read ({error})"));
                return Ok(joined.unread);
            }
        };
        let empty = End {
            configuration: end.configuration,
            base: StateBase::Empty,
            status: end.status,
            held: Held::Empty,
        };
        let end = if primary.records().is_empty() && matches!(end.base, StateBase::Current(_)) {
            &empty
        } else {
            end
        };
        let read =
            |transmit: &[u8], mark: u32, prefix: &Prefix| self.read(site, transmit, mark, prefix);
        self.replay(end, &line, &primary, &mut joined, read, output)?;
        Ok(joined.unread)
    }

    fn read(
        &self,
        site: &Site<'_>,
        transmit: &[u8],
        mark: u32,
        prefix: &Prefix,
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
        let names = self.names.state_entries(
            Persistent {
                faces: &decoded.faces,
                bodies: &decoded.body_identities,
            },
            &records,
            &decoded.bodies,
            |id| prefix.includes(mark, id),
        );
        Ok(Ok(Read {
            held: HeldState { records, names },
            annotations: decoded.annotations,
            losses: decoded.losses,
        }))
    }

    fn replay(
        &self,
        end: &End<'_>,
        line: &MainLine<'_>,
        primary: &PrimaryRecords<'_>,
        joined: &mut Joined,
        mut read: impl FnMut(&[u8], u32, &Prefix) -> Result<Result<Read, String>, CodecError>,
        output: &mut Output<'_>,
    ) -> Result<(), CodecError> {
        let mut replay = match line.replay(primary) {
            Ok(replay) => replay,
            Err(error) => {
                joined.refuse(&format!("the replay refuses ({error})"));
                return Ok(());
            }
        };
        let mut depth = u64::from(matches!(end.base, StateBase::Evaluation(_)));
        let limit = self.ctx.policy().limits.max_state_chain_depth;
        let mut chain = None;
        let mut changed = end.held.has_bodies() == replay.state().is_empty();
        let mut generated = None;
        for delta in line.deltas() {
            if joined.states.is_empty() {
                break;
            }
            if depth >= limit {
                joined.refuse("the part's evaluation-chain depth limit is reached");
                break;
            }
            if let Err(error) = replay.step() {
                joined.refuse(&format!("the replay refuses ({error})"));
                break;
            }
            changed |= !delta.is_empty();
            let mark = delta.target().identifier();
            let Some(feature) = joined.states.remove(&mark) else {
                continue;
            };
            let id = evaluation_id(end.configuration, &feature);
            let (content, annotations) =
                if changed {
                    let state = match replay.transmit() {
                        Ok(transmit) => match read(&transmit, mark, &joined.prefix)? {
                            Ok(state) => state,
                            Err(reason) => {
                                joined.unread.feature(
                                    &format!("their state does not decode ({reason})"),
                                    &feature,
                                );
                                continue;
                            }
                        },
                        Err(error) => {
                            joined.unread.feature(
                                &format!("the replay writes no transmit of their state ({error})"),
                                &feature,
                            );
                            continue;
                        }
                    };
                    let chain = match &mut chain {
                        Some(chain) => chain,
                        None => chain
                            .insert(ChainBack::new(end.base.clone(), end.held.state(self.ctx)?)),
                    };
                    let content = chain.push(id.clone(), state.held);
                    annotate_losses(&id, mark, state.losses, output.losses)?;
                    changed = false;
                    generated = None;
                    (content, state.annotations)
                } else {
                    let chain = match &mut chain {
                        Some(chain) => chain,
                        None => chain
                            .insert(ChainBack::new(end.base.clone(), end.held.state(self.ctx)?)),
                    };
                    let generated = generated
                        .get_or_insert_with(|| generated_names(chain.names(), &joined.prefix));
                    let cutoff = joined.prefix.cutoff(mark);
                    let mut clear = Vec::new();
                    while generated.first_key_value().is_some_and(|(rank, _)| {
                        rank.zip(cutoff)
                            .is_none_or(|(Reverse(rank), cutoff)| rank > cutoff)
                    }) {
                        if let Some((_, entities)) = generated.pop_first() {
                            clear.extend(entities);
                        }
                    }
                    (chain.push_shared(id.clone(), clear), Annotations::default())
                };

            let mut held = HashSet::new();
            topology_ids(&content.delta.records, |record| {
                held.insert(record.as_str().to_owned());
            });
            output
                .between
                .annotations
                .append(annotations.in_delta(&id, |record| held.contains(record)))
                .map_err(CodecError::from)?;
            output.between.records +=
                topology_len(&content.delta.records) + content.delta.removed.len();
            output.between.evaluations.push(Evaluation {
                id,
                scope: self.definition.clone(),
                configuration: end.configuration.cloned(),
                of: Evaluated::State(Position::After(Member::Feature(feature))),
                status: end.status,
                content: Some(content),
                native_ref: Some(format!("PMARK {mark}")),
            });
            depth += 1;
        }
        joined.refuse("the replay does not reach their mark");
        Ok(())
    }
}

fn generated_names(
    names: &BTreeMap<EntityRef, NameEntry>,
    prefix: &Prefix,
) -> BTreeMap<Option<Reverse<usize>>, Vec<EntityRef>> {
    let mut generated = BTreeMap::<_, Vec<_>>::new();
    for (entity, entry) in names {
        if let Some(TopoName::Generated { feature, .. }) = &entry.name {
            generated
                .entry(prefix.rank(feature).map(Reverse))
                .or_default()
                .push(entity.clone());
        }
    }
    generated
}

fn annotate_losses(
    id: &EvaluationId,
    mark: u32,
    state_losses: Vec<LossNote>,
    losses: &mut Vec<LossNote>,
) -> Result<(), CodecError> {
    for mut loss in state_losses {
        let identity: &Identity = id.as_ref();
        let key = identity.key();
        cadmpeg_ir::unknown::part::rescope_give_up(&mut loss, |record| {
            Ok(format!("{record}@state/{key}"))
        })?;
        loss.message = format!("Evaluation {id} (PMARK {mark}): {}", loss.message);
        losses.push(loss);
    }
    Ok(())
}

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

struct Read {
    held: HeldState,
    annotations: Annotations,
    losses: Vec<LossNote>,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
struct Unread(BTreeMap<String, Vec<FeatureId>>);

impl Joined {
    fn refuse(&mut self, reason: &str) {
        for feature in std::mem::take(&mut self.states).into_values() {
            self.unread.feature(reason, &feature);
        }
    }
}

impl Unread {
    fn feature(&mut self, reason: &str, feature: &FeatureId) {
        self.0
            .entry(reason.to_owned())
            .or_default()
            .push(feature.clone());
    }

    fn note(&self, configuration: Option<&ConfigurationId>) -> Option<LossNote> {
        if self.0.is_empty() {
            return None;
        }
        let subject = configuration.map_or_else(
            || "The part".to_owned(),
            |configuration| format!("Configuration {configuration}"),
        );
        let parts = self
            .0
            .iter()
            .map(|(reason, features)| {
                let mut ids = features.iter().map(FeatureId::as_str).collect::<Vec<_>>();
                ids.sort_unstable();
                ids.dedup();
                let ids = ids.join(", ");
                format!("the states after {ids} ({reason})")
            })
            .collect::<Vec<_>>();
        Some(SldprtLossCode::HistoryStatesUnread.note(format!(
            "{subject} stores states between features that are not stated: {}.",
            parts.join("; ")
        )))
    }
}
