// SPDX-License-Identifier: Apache-2.0
//! Join of `PmDc` features to the `delta_state` history of the active kernel
//! carrier ([spec §13.10](https://github.com/cadmpeg/cadmpeg/blob/main/docs/formats/inventor.md#1310-feature-history-states)).
//!
//! The reader materializes no state. It walks the entity-version maps of the
//! chain, newest state first, and keeps for each state only the body keys of
//! its body records and, for each feature source index, the oldest state whose
//! live naming attributes list it.

use std::collections::{BTreeSet, HashMap};

use cadmpeg_asm::history::{
    archived_active_record_count, decode_record_archive, frame_history_chain,
    insert_only_active_record_count, walk_entity_versions, ChainFraming, DeltaStateView,
    HistoryChain, RecordArchiveFrames,
};
use cadmpeg_asm::kernel_header::RefWidth;
use cadmpeg_asm::sab::{Record, Token};
use cadmpeg_core::decode::{work_units, DecodeContext, WorkBudget};
use cadmpeg_core::CodecError;
use cadmpeg_ir::features::{BodySelection, Feature, FeatureDefinition, FeatureOperation};
use cadmpeg_ir::history_body::HistoryBody;
use cadmpeg_ir::report::loss::LossNote;
use serde::{Deserialize, Serialize};

use super::body_selection::WrapperLink;
use super::{PmDcFeature, PmDcFeatureProperty, PmDcKernelEntityLink};
use crate::native::ActiveCarrierRecord;

/// Local work cap of one state-index read, in live records inspected. The
/// work is the state count times the live record count.
pub(crate) const MAX_STATE_SCAN_WORK: usize = 1 << 24;

/// Source tag of a Split feature that partitions one body.
pub(crate) const SPLIT_TAG: &str = "split_body";
/// Source tag of a stored-body base feature.
pub(crate) const BASE_TAG: &str = "base";
/// Kind of a base feature whose result body does not join the current model.
pub(crate) const NATIVE_BASE_KIND: &str = "native:Base";

/// Schema-44 attribute families that carry source-name pairs.
const SCHEMA_44_FAMILIES: [&str; 6] = [
    "INV_NMX_GEN_TAG",
    "INV_NMX_SWEEPGENERATED_TAG",
    "INV_NMX_FLANGE_TAG",
    "INV_NMX_BEND_TAG",
    "INV_NMX_BLEND_TAG",
    "INV_NMX_FEATURE_TAG",
];
/// Class-chain suffix of the schema-38, -40, and -42 naming tags.
const NAMING_TAG_SUFFIX: &str = "NMx_Brep_tag-NamingMatching-attrib";
/// Integers that follow the family string of a schema-44 naming tag, before
/// the source-name count: the form 1 and the schema 44.
const SCHEMA_44_PREFIX: [i64; 2] = [1, 44];

/// One state of the chain, oldest first.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct IndexedState {
    /// State identifier of the `delta_state` header.
    pub(crate) state_id: i64,
    /// Sorted keys of the body records alive in the state.
    pub(crate) body_keys: Vec<i64>,
}

/// The states of a closed chain and the first state of each source index.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub(crate) struct StateIndex {
    /// States, oldest first.
    pub(crate) states: Vec<IndexedState>,
    /// Oldest-first ordinal of the oldest state whose live naming attributes
    /// list the source index.
    pub(crate) first: HashMap<u32, u32>,
    /// Source indices that a live naming attribute lists with a stored schema
    /// other than the carrier schema.
    pub(crate) schema_mismatch: BTreeSet<u32>,
}

/// Why a state index could not be read.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Unread {
    /// The chain does not frame, does not close, or does not agree with the
    /// archived records.
    Unclosed,
    /// The scan passed its work cap.
    Bounded,
}

/// Result of reading the history of a carrier.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Reading {
    /// The chain was not read.
    Unread(Unread),
    /// The chain closed.
    Read(StateIndex),
}

/// Class of a record that a state scan keeps.
#[derive(Debug, Clone, PartialEq, Eq)]
enum Class {
    /// A body record with its key.
    Body(i64),
    /// A naming attribute with its stored schema and the source indices it lists.
    Naming(i64, Vec<u32>),
}

/// The integer at payload position `index`.
fn long(record: &Record, index: usize) -> Option<i64> {
    match record.chunk(index) {
        Some(Token::Long(value)) => Some(*value),
        _ => None,
    }
}

/// Source indices of the `count` pairs that start at payload position
/// `first`. A pair is a source index and a signed name value.
fn pairs(record: &Record, count_at: usize) -> Option<Vec<u32>> {
    let count = usize::try_from(long(record, count_at)?).ok()?;
    if count == 0 || count_at + 1 + count.checked_mul(2)? > record.chunk_len() {
        return None;
    }
    let mut sources = Vec::with_capacity(count);
    for pair in 0..count {
        let at = count_at + 1 + pair * 2;
        let source = u32::try_from(long(record, at)?).ok()?;
        long(record, at + 1)?;
        sources.push(source);
    }
    Some(sources)
}

/// The stored schema and the source indices of a naming attribute, or `None`
/// when the record is not a naming tag of a recognized form.
pub(crate) fn naming_tag(record: &Record) -> Option<(i64, Vec<u32>)> {
    if record.name == "ATTRIB_CUSTOM-attrib" {
        let Some(Token::Str(family)) = record.chunk(5) else {
            return None;
        };
        if !SCHEMA_44_FAMILIES.contains(&family.as_str()) {
            return None;
        }
        if SCHEMA_44_PREFIX
            .iter()
            .enumerate()
            .any(|(offset, expected)| long(record, 6 + offset) != Some(*expected))
        {
            return None;
        }
        return Some((44, pairs(record, 8)?));
    }
    if record.name.starts_with("NMx_") && record.name.ends_with(NAMING_TAG_SUFFIX) {
        return Some((long(record, 5)?, pairs(record, 6)?));
    }
    None
}

/// Class of one record, or `None` when a scan ignores it.
fn classify(record: &Record) -> Option<Class> {
    if record.head() == "body" {
        return Some(Class::Body(long(record, 1).filter(|key| *key >= 0)?));
    }
    naming_tag(record).map(|(schema, sources)| Class::Naming(schema, sources))
}

/// A framed chain together with the records it archives.
pub(crate) struct Chain<'a> {
    /// Views of the states, in stream order.
    pub(crate) views: Vec<DeltaStateView<'a>>,
    /// Framed active and archived records.
    pub(crate) frames: RecordArchiveFrames,
    /// Sorted revision identifiers of the archived records.
    pub(crate) archived_ids: Vec<i64>,
    /// Length of the active record table.
    pub(crate) active_count: i64,
}

/// Result of framing the chain of a carrier.
pub(crate) enum Prepared<'a> {
    /// The chain was not read.
    Unread(Unread),
    /// The chain frames and agrees with its archived records.
    Ready(Box<Chain<'a>>),
}

/// Frame the chain of `bytes` and check it against the archived records.
pub(crate) fn prepare<'a>(
    ctx: &DecodeContext<'_>,
    chain: &'a HistoryChain,
    bytes: &'a [u8],
    width: RefWidth,
) -> Result<Prepared<'a>, CodecError> {
    let views = chain.views(bytes);
    let Some(frames) = decode_record_archive(ctx, &views, bytes, width)? else {
        return Ok(Prepared::Unread(Unread::Unclosed));
    };
    let archived_ids = views
        .iter()
        .flat_map(|state| &state.records)
        .filter_map(|record| record.revision_id)
        .collect::<BTreeSet<_>>()
        .into_iter()
        .collect::<Vec<_>>();
    let active_count = archived_active_record_count(&views)?
        .or_else(|| insert_only_active_record_count(&views))
        .and_then(|count| i64::try_from(count).ok());
    let Some(active_count) = active_count else {
        return Ok(Prepared::Unread(Unread::Unclosed));
    };
    if frames.archived.len() != archived_ids.len()
        || i64::try_from(frames.active.len()).ok() != Some(active_count)
    {
        return Ok(Prepared::Unread(Unread::Unclosed));
    }
    Ok(Prepared::Ready(Box::new(Chain {
        views,
        frames,
        archived_ids,
        active_count,
    })))
}

/// Result of framing the `delta_state` chain of a stream.
pub(crate) enum Framing {
    /// The stream holds no `delta_state` record.
    Absent,
    /// The chain does not frame.
    Unread(Unread),
    /// Every state frames.
    Framed(HistoryChain),
}

/// Frame the `delta_state` chain of `bytes` as a stream of the given reference width.
pub(crate) fn frame_chain(
    ctx: &DecodeContext<'_>,
    bytes: &[u8],
    width: RefWidth,
) -> Result<Framing, CodecError> {
    Ok(match frame_history_chain(ctx, bytes, width)? {
        ChainFraming::Absent => Framing::Absent,
        ChainFraming::Unframed => Framing::Unread(Unread::Unclosed),
        ChainFraming::Framed(chain) => Framing::Framed(chain),
    })
}

/// Read the state index of a prepared chain with a work cap of `limit` live
/// records inspected.
pub(crate) fn index_chain(chain: &Chain<'_>, schema: u32, limit: usize) -> Reading {
    let Chain {
        views,
        frames,
        archived_ids,
        active_count,
    } = chain;
    let active_count = *active_count;

    let mut classes = HashMap::new();
    for (revision, record) in frames.active.iter().enumerate() {
        let (Ok(revision), Some(class)) = (i64::try_from(revision), classify(record)) else {
            continue;
        };
        classes.insert(revision, class);
    }
    for (&revision, record) in &frames.archived {
        if let Some(class) = classify(record) {
            classes.insert(revision, class);
        }
    }

    let work = WorkBudget::new(limit);
    let mut scanned = Vec::<IndexedState>::new();
    let mut latest = HashMap::<u32, usize>::new();
    let mut schema_mismatch = BTreeSet::new();
    let mut bounded = false;
    let closed = walk_entity_versions(views, active_count, archived_ids, |ordinal, versions| {
        if bounded {
            return;
        }
        if !work.charge_by(work_units(versions.len())) {
            bounded = true;
            return;
        }
        let visit = scanned.len();
        let mut body_keys = Vec::new();
        for revision in versions.values() {
            match classes.get(revision) {
                Some(Class::Body(key)) => body_keys.push(*key),
                Some(Class::Naming(stored, sources)) => {
                    for &source in sources {
                        latest.insert(source, visit);
                        if *stored != i64::from(schema) {
                            schema_mismatch.insert(source);
                        }
                    }
                }
                None => {}
            }
        }
        body_keys.sort_unstable();
        scanned.push(IndexedState {
            state_id: views[ordinal].state_id,
            body_keys,
        });
    });
    if bounded {
        return Reading::Unread(Unread::Bounded);
    }
    if !closed || scanned.len() != views.len() {
        return Reading::Unread(Unread::Unclosed);
    }
    let last = scanned.len() - 1;
    let mut first = HashMap::with_capacity(latest.len());
    for (source, visit) in latest {
        let Ok(ordinal) = u32::try_from(last - visit) else {
            return Reading::Unread(Unread::Unclosed);
        };
        first.insert(source, ordinal);
    }
    scanned.reverse();
    Reading::Read(StateIndex {
        states: scanned,
        first,
        schema_mismatch,
    })
}

/// A state of the chain by oldest-first ordinal.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct StateRef {
    /// Oldest-first ordinal in the chain.
    pub(crate) ordinal: u32,
    /// State identifier of the `delta_state` header.
    pub(crate) state_id: i64,
}

/// Why a feature has no bound state.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum UnboundReason {
    /// No live naming attribute of any state lists the source index.
    NoTag,
    /// The state that adds the tag is the oldest state.
    NoPredecessor,
    /// The body key of a Split or Base feature does not resolve.
    BodyKeyUnresolved,
    /// The body key does not change between the two states as the feature requires.
    BodyKeyMismatch,
}

/// Outcome of the join of one feature.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "status", rename_all = "snake_case")]
pub(crate) enum Binding {
    /// The feature has a state pair and, for Split and Base, a confirmed body key.
    Bound,
    /// The feature has no bound state.
    Unbound {
        /// Reason for the refusal.
        reason: UnboundReason,
    },
}

/// Body check that a feature kind requires.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Check {
    None,
    Split,
    Base,
}

/// One `PmDc` feature joined to its history states.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct FeatureHistoryState {
    /// Identifier of this record.
    pub(crate) id: String,
    /// Native identifier of the feature record.
    pub(crate) feature: String,
    /// Source index of the feature header.
    pub(crate) source_index: u32,
    /// Source tag of the neutral feature that the record projects to.
    pub(crate) kind: String,
    /// Oldest state whose live naming attributes list the source index.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_state_after"
    )]
    pub(crate) state_after: Option<StateRef>,
    /// Ordinal of the next older state.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_state_before"
    )]
    pub(crate) state_before: Option<u32>,
    /// Body key that a Split or Base check used.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_body_key"
    )]
    pub(crate) body_key: Option<i64>,
    /// Nested history body of the Split or Base body in its state.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_history_body"
    )]
    pub(crate) history_body: Option<String>,
    /// Join outcome.
    #[serde(flatten)]
    pub(crate) binding: Binding,
}

cadmpeg_core::named_optional_field!(deserialize_state_after, StateRef, "state_after");
cadmpeg_core::named_optional_field!(deserialize_state_before, u32, "state_before");
cadmpeg_core::named_optional_field!(deserialize_body_key, i64, "body_key");
cadmpeg_core::named_optional_field!(deserialize_history_body, String, "history_body");

/// Records the join reads.
pub(crate) struct Inputs<'a> {
    /// `PmDc` feature records.
    pub(crate) features: &'a [PmDcFeature],
    /// Neutral features, which carry the native reference and source tag.
    pub(crate) model_features: &'a [Feature],
    /// Kernel-entity links.
    pub(crate) kernel_links: &'a [PmDcKernelEntityLink],
    /// Joins of wrappers to kernel-entity links.
    pub(crate) wrapper_links: &'a [WrapperLink],
    /// Feature property records, which include the surface-body wrappers.
    pub(crate) properties: &'a [PmDcFeatureProperty],
}

/// Whether features of `kind` need a body-key check to bind.
fn body_checked(kind: &str) -> bool {
    matches!(kind, SPLIT_TAG | BASE_TAG | NATIVE_BASE_KIND)
}

/// Kind of a neutral feature: its source tag, with the native family of a
/// feature that has no neutral construction.
fn kind_of(feature: &Feature) -> String {
    let tag = feature.source_tag.clone().unwrap_or_default();
    match feature.evaluation.definition() {
        FeatureDefinition::Operation(FeatureOperation::Native { kind, .. }) if tag == "native" => {
            format!("native:{}", kind.as_str())
        }
        _ => tag,
    }
}

fn target_wrapper(feature: &Feature) -> Option<&str> {
    let FeatureDefinition::Operation(FeatureOperation::SplitBody { targets, .. }) =
        feature.evaluation.definition()
    else {
        return None;
    };
    match targets {
        BodySelection::Native(native) => Some(native.as_str()),
        BodySelection::Resolved { native, .. } => Some(native.as_str()),
        BodySelection::Local { native, .. } => Some(native.as_str()),
        _ => None,
    }
}

/// Join the features that the neutral model projects from `PmDc` feature
/// records to the states of `index`.
pub(crate) fn join(index: &StateIndex, inputs: &Inputs<'_>) -> Vec<FeatureHistoryState> {
    let features = inputs
        .features
        .iter()
        .map(|feature| (feature.id(), feature))
        .collect::<HashMap<_, _>>();
    let links = inputs
        .kernel_links
        .iter()
        .map(|link| (link.id(), link))
        .collect::<HashMap<_, _>>();
    let wrappers = inputs
        .wrapper_links
        .iter()
        .map(|join| (join.wrapper.as_str(), join))
        .collect::<HashMap<_, _>>();
    let properties = inputs
        .properties
        .iter()
        .map(|property| {
            (
                (
                    property.identity.segment_token.as_str(),
                    property.identity.record_ordinal,
                ),
                property,
            )
        })
        .collect::<HashMap<_, _>>();
    let mut records = Vec::new();
    for model in inputs.model_features {
        let Some(native) = model.native_ref.as_deref() else {
            continue;
        };
        let Some(feature) = features.get(native) else {
            continue;
        };
        let kind = kind_of(model);
        let (check, wrapper) = match kind.as_str() {
            SPLIT_TAG => (Check::Split, target_wrapper(model).map(str::to_owned)),
            BASE_TAG | NATIVE_BASE_KIND => (
                Check::Base,
                super::base::wrappers(feature, super::BASE_CLASS_ID, |ordinal| {
                    properties
                        .get(&(feature.identity.segment_token.as_str(), ordinal))
                        .copied()
                })
                .map(|(_, output)| output.id()),
            ),
            _ => (Check::None, None),
        };
        let link = wrapper
            .as_deref()
            .and_then(|wrapper| wrappers.get(wrapper))
            .map(|join| join.link.as_str());
        let key = link
            .and_then(|id| links.get(id))
            .map(|link| i64::from(link.entity_id));
        let source_index = feature.header.source_index;
        records.push(bind(
            index,
            FeatureHistoryState {
                id: format!(
                    "inventor:pmdc:feature-history-state#{}-{}",
                    feature.identity.segment_token, feature.identity.record_ordinal
                ),
                feature: native.to_owned(),
                source_index,
                kind,
                state_after: None,
                state_before: None,
                body_key: None,
                history_body: None,
                binding: Binding::Bound,
            },
            check,
            key,
        ));
    }
    records
}

/// Fill the states and the binding of one record.
fn bind(
    index: &StateIndex,
    mut record: FeatureHistoryState,
    check: Check,
    key: Option<i64>,
) -> FeatureHistoryState {
    let unbound = |reason| Binding::Unbound { reason };
    let Some(&after) = index.first.get(&record.source_index) else {
        record.binding = unbound(UnboundReason::NoTag);
        return record;
    };
    let Some(state) = usize::try_from(after)
        .ok()
        .and_then(|ordinal| index.states.get(ordinal))
    else {
        record.binding = unbound(UnboundReason::NoTag);
        return record;
    };
    record.state_after = Some(StateRef {
        ordinal: after,
        state_id: state.state_id,
    });
    record.state_before = after.checked_sub(1);
    if check == Check::None {
        return record;
    }
    let Some(key) = key else {
        record.binding = unbound(UnboundReason::BodyKeyUnresolved);
        return record;
    };
    record.body_key = Some(key);
    let Some(before) = record
        .state_before
        .and_then(|ordinal| usize::try_from(ordinal).ok())
        .and_then(|ordinal| index.states.get(ordinal))
    else {
        record.binding = unbound(UnboundReason::NoPredecessor);
        return record;
    };
    let in_after = state.body_keys.binary_search(&key).is_ok();
    let in_before = before.body_keys.binary_search(&key).is_ok();
    let confirmed = match check {
        Check::Split => in_before && !in_after,
        Check::Base => in_after && !in_before,
        Check::None => true,
    };
    if !confirmed {
        record.binding = unbound(UnboundReason::BodyKeyMismatch);
    }
    record
}

/// Records of the active carrier and the losses that the read reports.
pub(crate) struct Projection {
    /// Joined features; empty when the chain was not read.
    pub(crate) records: Vec<FeatureHistoryState>,
    /// Unread reading, if any.
    pub(crate) unread: Option<Unread>,
    /// Features that retain a state from a naming tag with a stored schema
    /// other than the carrier schema, bound or not.
    pub(crate) schema_assumed: usize,
    /// Nested history bodies of the joined features.
    pub(crate) bodies: super::history_bodies::Built,
}

impl Projection {
    fn without_records(unread: Option<Unread>) -> Self {
        Self {
            records: Vec::new(),
            unread,
            schema_assumed: 0,
            bodies: super::history_bodies::Built::default(),
        }
    }
}

/// Features that retain a state derived from a naming tag whose stored schema
/// differs from the carrier schema, bound or left unbound by the body check.
fn schema_assumed(records: &[FeatureHistoryState], index: &StateIndex) -> usize {
    records
        .iter()
        .filter(|record| {
            record.state_after.is_some() && index.schema_mismatch.contains(&record.source_index)
        })
        .count()
}

/// Read the history of the active carrier and join the features to it.
pub(crate) fn project(
    ctx: &DecodeContext<'_>,
    carrier: &crate::kernel::ActiveCarrier<'_>,
    inputs: &Inputs<'_>,
) -> Result<Projection, CodecError> {
    if carrier.family != crate::kernel::KernelFamily::Asm {
        return Ok(Projection::without_records(None));
    }
    let Ok(header) = carrier.header.as_ref() else {
        return Ok(Projection::without_records(None));
    };
    let bytes = carrier.bytes.window();
    let framed = match frame_chain(ctx, bytes, header.width)? {
        Framing::Framed(chain) => chain,
        Framing::Absent => return Ok(Projection::without_records(None)),
        Framing::Unread(reason) => return Ok(Projection::without_records(Some(reason))),
    };
    let chain = match prepare(ctx, &framed, bytes, header.width)? {
        Prepared::Unread(reason) => return Ok(Projection::without_records(Some(reason))),
        Prepared::Ready(chain) => chain,
    };
    let index = match index_chain(&chain, carrier.schema, MAX_STATE_SCAN_WORK) {
        Reading::Read(index) => index,
        Reading::Unread(reason) => return Ok(Projection::without_records(Some(reason))),
    };
    let mut records = join(&index, inputs);
    ctx.charge_entities(
        cadmpeg_core::decode::u64_from_index(records.len()),
        "join Inventor features to history states",
    )?;
    let schema_assumed = schema_assumed(&records, &index);
    let bodies =
        super::history_bodies::build(ctx, carrier, *chain, &records, inputs.model_features)?;
    for (&record, body) in &bodies.links {
        records[record].history_body = Some(body.as_str().to_owned());
    }
    Ok(Projection {
        records,
        unread: None,
        schema_assumed,
        bodies,
    })
}

/// Losses that a projection reports: an unread chain, and Split and Base
/// features without a bound state.
pub(crate) fn losses(projection: &Projection) -> Vec<LossNote> {
    use crate::loss::InventorLossCode;
    let mut notes = Vec::new();
    match projection.unread {
        Some(Unread::Unclosed) => notes.push(InventorLossCode::KernelHistoryUnclosed.note(
            "The kernel carrier's delta_state chain does not frame or close; features have no history states.",
        )),
        Some(Unread::Bounded) => notes.push(InventorLossCode::KernelHistoryBounded.note(
            "The kernel carrier's delta_state chain passed the local scan cap; features have no history states.",
        )),
        None => {}
    }
    if projection.schema_assumed > 0 {
        notes.push(InventorLossCode::KernelHistoryTagSchemaAssumed.note(format!(
            "{} feature(s) retain a history state from a naming tag whose stored schema differs from the carrier schema, including features that the body check leaves unbound; the state assumes that the tag still names the feature.",
            projection.schema_assumed
        )));
    }
    let mut counts = [0_usize; 4];
    for record in &projection.records {
        if !body_checked(&record.kind) {
            continue;
        }
        if let Binding::Unbound { reason } = record.binding {
            let slot = match reason {
                UnboundReason::NoTag => 0,
                UnboundReason::NoPredecessor => 1,
                UnboundReason::BodyKeyUnresolved => 2,
                UnboundReason::BodyKeyMismatch => 3,
            };
            counts[slot] += 1;
        }
    }
    let [no_tag, no_predecessor, unresolved, mismatch] = counts;
    if counts.iter().sum::<usize>() > 0 {
        notes.push(InventorLossCode::FeatureHistoryStateUnbound.note(format!(
            "Split and Base features have no bound history state: {no_tag} without a naming tag, {no_predecessor} at the oldest state, {unresolved} with an unresolved body key, {mismatch} with a body key that does not change as the feature requires."
        )));
    }
    notes.extend(body_losses(&projection.bodies.outcome));
    notes
}

/// Losses of the nests: bodies that were asked for and not produced.
fn body_losses(outcome: &super::history_bodies::Outcome) -> Vec<LossNote> {
    use crate::loss::InventorLossCode;
    let mut notes = Vec::new();
    if outcome.undecoded > 0 {
        notes.push(InventorLossCode::FeatureHistoryBodyUndecoded.note(format!(
            "{} history state(s) of Split and Base features did not materialize or decode to the body of the feature; those bodies have no history body.",
            outcome.undecoded
        )));
    }
    if outcome.not_closed() > 0 {
        let [reference, kind, edge] = outcome.open;
        notes.push(InventorLossCode::FeatureHistoryBodyNotClosed.note(format!(
            "{} body(ies) of Split and Base features are not closed manifold solids in their history state ({reference} with a reference outside the body, {kind} not solid bodies, {edge} with an edge that does not join two coedges); those bodies have no history body.",
            outcome.not_closed()
        )));
    }
    if outcome.bounded > 0 {
        notes.push(InventorLossCode::FeatureHistoryBodyBounded.note(format!(
            "{} body(ies) of Split and Base features passed the local cap of history bodies; those bodies have no history body.",
            outcome.bounded
        )));
    }
    notes
}

/// Findings of the references in an arena of joined features.
pub(crate) fn validate(
    records: &[FeatureHistoryState],
    features: &[PmDcFeature],
    carrier: &ActiveCarrierRecord,
    bodies: &[HistoryBody],
) -> Vec<String> {
    let mut findings = Vec::new();
    let nests = bodies
        .iter()
        .map(|body| (body.id.as_str(), body))
        .collect::<HashMap<_, _>>();
    let sources = features
        .iter()
        .map(|feature| (feature.id(), feature.header.source_index))
        .collect::<HashMap<_, _>>();
    if !records.is_empty() && !matches!(carrier, ActiveCarrierRecord::Selected { .. }) {
        findings.push("feature history states have no selected active carrier".to_owned());
    }
    let mut ids = BTreeSet::new();
    let mut joined = BTreeSet::new();
    for record in records {
        if !ids.insert(record.id.as_str()) {
            findings.push(format!("feature history state {} repeats an id", record.id));
        }
        if !joined.insert(record.feature.as_str()) {
            findings.push(format!(
                "feature {} has several history state records",
                record.feature
            ));
        }
        if sources.get(&record.feature) != Some(&record.source_index) {
            findings.push(format!(
                "feature history state {} names no feature with its source index",
                record.id
            ));
        }
        let ordered = match (record.state_after, record.state_before) {
            (Some(after), Some(before)) => after.ordinal.checked_sub(1) == Some(before),
            (Some(after), None) => after.ordinal == 0,
            (None, before) => before.is_none(),
        };
        if !ordered {
            findings.push(format!(
                "feature history state {} has inconsistent state ordinals",
                record.id
            ));
        }
        let bound = matches!(record.binding, Binding::Bound);
        if let Some(link) = &record.history_body {
            let expected = match record.kind.as_str() {
                SPLIT_TAG => record.state_before,
                BASE_TAG | NATIVE_BASE_KIND => record.state_after.map(|state| state.ordinal),
                _ => None,
            };
            match nests.get(link.as_str()) {
                None => findings.push(format!(
                    "feature history state {} names no history body",
                    record.id
                )),
                Some(body) if !bound || Some(body.state.ordinal) != expected => {
                    findings.push(format!(
                        "feature history state {} names a history body of another state",
                        record.id
                    ));
                }
                Some(_) => {}
            }
        }
        if bound && record.state_after.is_none() {
            findings.push(format!(
                "bound feature history state {} has no state after",
                record.id
            ));
        }
        if bound && body_checked(&record.kind) {
            if record.body_key.is_none() || record.state_before.is_none() {
                findings.push(format!(
                    "bound feature history state {} lacks its checked body key",
                    record.id
                ));
            }
        } else if bound && record.body_key.is_some() {
            findings.push(format!(
                "feature history state {} has a body key without a body check",
                record.id
            ));
        }
    }
    findings
}

#[cfg(test)]
mod tests;
