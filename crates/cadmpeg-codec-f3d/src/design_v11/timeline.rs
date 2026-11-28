// SPDX-License-Identifier: Apache-2.0
//! The timelines of a Fusion design as ordered containers (§5.2), and one
//! feature per timeline item.
//!
//! The v10 projection orders the features of one Design stream by the item
//! runs of its timeline records, in their `MetaStream` order, so the feature
//! at an item's position in that run is the item's feature. An item the v10
//! projection did not resolve is a feature of the family its record type
//! establishes, or a native feature named by that type.
//!
//! The rolled-back flags of the items' scope tails place the rollback marker
//! of a timeline before its first rolled-back item. A timeline whose items
//! carry no rolled-back flag is built through its last item.
//!
//! A Base Feature is stored geometry (§5.10) where the v10 projection names
//! its bodies and each is a body of the current geometry, whose records the
//! state after it holds; otherwise its bodies are typed with the carrier
//! unread, and it is native.

use std::collections::{BTreeMap, BTreeSet, HashMap};

use cadmpeg_core::decode::DecodeContext;
use cadmpeg_core::CodecError;
use cadmpeg_ir::design::container::{Container, OrderedKind, Rollback};
use cadmpeg_ir::design::dimension::Dimension;
use cadmpeg_ir::design::feature::{Feature, FeatureDefinition, FeatureOperation, StoredOrigin};
use cadmpeg_ir::design::ids::ContainerId;
use cadmpeg_ir::design::names::CodecName;
use cadmpeg_ir::design::refs::Member;
use cadmpeg_ir::features::selections::BodySelection;
use cadmpeg_ir::features::{
    Feature as LegacyFeature, FeatureId, FeatureOperation as LegacyOperation,
};
use cadmpeg_ir::ids::IdentityKeyTail;
use cadmpeg_ir::report::loss::LossNote;

use super::operation::{family, kind_family, Family};
use crate::ids::native_stream;
use crate::loss::F3dLossCode;
use crate::records::entity_header::{DesignFeatureTimeline, SegmentType};
use crate::records::feature::scope::DesignScopeTailFlags;

/// The tail flags of the parameter scopes, keyed by stream and record index.
pub(super) type TailFlags<'a> = HashMap<(&'a str, u64), DesignScopeTailFlags>;

/// The parameter scopes of the decode: the scope records and the tail flags
/// read from them.
pub(super) struct ScopeInputs<'a> {
    pub(super) records: &'a [crate::records::feature::scope::DesignParameterScope],
    pub(super) tail_flags: &'a TailFlags<'a>,
}

/// The containers and features of one document's timelines.
#[derive(Default)]
pub(super) struct Projected {
    /// One ordered timeline per timeline record that lists an item, in
    /// stream and `MetaStream` order.
    pub(super) containers: Vec<Container>,
    /// The features of the items, in member order.
    pub(super) features: Vec<Feature>,
    /// The stream and scope record of the item of each feature.
    pub(super) items: BTreeMap<FeatureId, (String, u64)>,
}

/// The timeline containers and item features of every document of the
/// decode, keyed by the document's identity prefix (empty for the root
/// document, the definition key and `@` for a referenced design). `bodies` are
/// the bodies of the current geometry. A timeline whose rollback marker the
/// flags leave open charges a loss to `losses`.
pub(super) fn project(
    ctx: &DecodeContext<'_>,
    features: &[LegacyFeature],
    timelines: &[DesignFeatureTimeline],
    types: &[SegmentType],
    scope_inputs: &ScopeInputs<'_>,
    bodies: &BTreeSet<&str>,
    losses: &mut Vec<LossNote>,
) -> Result<BTreeMap<String, Projected>, CodecError> {
    let tail_flags = scope_inputs.tail_flags;
    let scopes = scope_inputs
        .records
        .iter()
        .map(|scope| (scope.id.as_str(), scope))
        .collect::<HashMap<_, _>>();
    let companion_suppressed = scope_inputs
        .records
        .iter()
        .filter_map(|scope| {
            Some((
                (native_stream(&scope.id)?, u64::from(scope.record_index)),
                scope.companion_suppressed?.value,
            ))
        })
        .collect::<HashMap<_, _>>();
    let mut streams = BTreeMap::<&str, Vec<&DesignFeatureTimeline>>::new();
    for timeline in timelines {
        if let Some(stream) = native_stream(timeline.id()) {
            streams.entry(stream).or_default().push(timeline);
        }
    }
    let mut positioned = HashMap::<(&str, u64), Vec<&LegacyFeature>>::new();
    for feature in features {
        if let Some(stream) = feature.native_ref.as_deref().and_then(native_stream) {
            positioned
                .entry((stream, feature.ordinal))
                .or_default()
                .push(feature);
        }
    }
    let mut documents = BTreeMap::<String, Projected>::new();
    for (stream, mut records) in streams {
        records.sort_by_key(|timeline| timeline.source_ordinal);
        let item_types = ItemTypes::new(types, stream);
        let mut ordinal = 0_u64;
        for timeline in records {
            let items = timeline.frame().items();
            if items.is_empty() {
                continue;
            }
            let projected = documents
                .entry(document_prefix(stream).to_owned())
                .or_default();
            let mut members = Vec::with_capacity(items.len());
            let mut rolled_back = Vec::with_capacity(items.len());
            for item in items {
                ctx.charge_entities(1, "project F3D v11 timeline item")?;
                let flags = tail_flags.get(&(stream, item.value)).copied();
                rolled_back.push(flags.map(|flags| flags.rolled_back));
                let feature = match positioned
                    .get(&(stream, ordinal))
                    .and_then(|candidates| item_feature(candidates, item.value))
                {
                    Some(feature) => resolved_item(feature, bodies, &scopes, losses),
                    None => unprojected_item(
                        stream,
                        timeline,
                        item.value,
                        &item_types,
                        flags,
                        companion_suppressed.get(&(stream, item.value)).copied(),
                        losses,
                    )?,
                };
                members.push(Member::Feature(feature.id.clone()));
                projected
                    .items
                    .insert(feature.id.clone(), (stream.to_owned(), item.value));
                projected.features.push(feature);
                ordinal = ordinal
                    .checked_add(1)
                    .ok_or_else(|| CodecError::malformed("F3D timeline ordinal exceeds u64"))?;
            }
            ctx.charge_entities(1, "project F3D v11 timeline")?;
            let id = container_id(timeline.id())?;
            let rollback = rollback(&id, &members, &rolled_back, losses);
            projected.containers.push(Container::Ordered {
                id,
                kind: OrderedKind::Timeline,
                members,
                folders: Vec::new(),
                rollback: Some(rollback),
                placement: None,
            });
        }
    }
    Ok(documents)
}

/// The rollback marker of the timeline `id` from the rolled-back flag of each
/// of its `members` (`None` where the flags are unread): before the first
/// rolled-back item when no later item is read as built, else after the last
/// item. A loss names a marker the flags leave open: an unread item before the
/// first rolled-back item, or after it.
fn rollback(
    id: &ContainerId,
    members: &[Member],
    rolled_back: &[Option<bool>],
    losses: &mut Vec<LossNote>,
) -> Rollback {
    let Some(first) = rolled_back.iter().position(|flag| *flag == Some(true)) else {
        if rolled_back.contains(&None) {
            losses.push(F3dLossCode::HistoryRollbackUnread.note(format!(
                "timeline {id}: rollback marker not read; stated built through its last item"
            )));
        }
        return Rollback::End;
    };
    if rolled_back
        .iter()
        .skip(first)
        .any(|flag| *flag == Some(false))
    {
        losses.push(F3dLossCode::HistoryRollbackUnread.note(format!(
            "timeline {id}: a built item follows a rolled-back item; stated built through its last item"
        )));
        return Rollback::End;
    }
    if first
        .checked_sub(1)
        .and_then(|previous| rolled_back.get(previous))
        == Some(&None)
    {
        losses.push(F3dLossCode::HistoryRollbackBoundUnread.note(format!(
            "timeline {id}: the item before the first rolled-back item is unread; the rollback marker can stand before it"
        )));
    }
    if rolled_back.iter().skip(first).any(Option::is_none) {
        losses.push(F3dLossCode::HistoryRollbackUnread.note(format!(
            "timeline {id}: an item after the first rolled-back item is unread; stated rolled back from the first rolled-back item"
        )));
    }
    members
        .get(first)
        .cloned()
        .map_or(Rollback::End, Rollback::Before)
}

/// The identity prefix of the document that holds `stream`: the definition
/// key of the archive member and its `@`, empty for the root document.
pub(super) fn document_prefix(stream: &str) -> &str {
    let local = stream.strip_prefix("f3d:").unwrap_or(stream);
    let scope = local.split_once(':').map_or(local, |(scope, _)| scope);
    scope
        .split_once('@')
        .map_or("", |(key, _)| &local[..=key.len()])
}

/// The v10 feature of the item whose record is `record`, among the features
/// the v10 projection placed at the item's position: the one keyed by that
/// record, else the first by id.
fn item_feature<'a>(candidates: &[&'a LegacyFeature], record: u64) -> Option<&'a LegacyFeature> {
    let suffix = format!(":{record}");
    candidates
        .iter()
        .find(|feature| feature.id.as_str().ends_with(&suffix))
        .or_else(|| candidates.iter().min_by_key(|feature| feature.id.as_str()))
        .copied()
}

fn resolved_item(
    feature: &LegacyFeature,
    bodies: &BTreeSet<&str>,
    scopes: &HashMap<&str, &crate::records::feature::scope::DesignParameterScope>,
    losses: &mut Vec<LossNote>,
) -> Feature {
    let operation = feature.evaluation.definition().operation();
    let typed_pattern = |losses: &mut Vec<LossNote>| {
        let scope = scopes.get(feature.native_ref.as_deref()?)?;
        scope.rectangular_pattern_construction()?;
        let projected = super::pattern::project(operation, scope, losses);
        if projected.is_none() {
            losses.push(F3dLossCode::FeatureRectangularPatternDesignNative.note(format!(
                "feature {} retains its rectangular pattern in its native record: its direction, seeds, or run cannot be transferred to a v11 linear layout.", feature.id
            )));
        }
        projected
    };
    let operation = super::datum::project(feature, losses)
        .or_else(|| stored(operation, bodies))
        .or_else(|| typed_pattern(losses))
        .or_else(|| super::split::project(operation, feature.native_ref.as_deref()?))
        .unwrap_or_else(|| match family(operation) {
            Family::Typed(family) => FeatureOperation::Unresolved { family },
            Family::Native(kind) => native(&kind),
        });
    Feature {
        id: feature.id.clone(),
        name: feature.name.clone(),
        suppressed: feature.suppressed.map(Dimension::Literal),
        definition: FeatureDefinition {
            operation,
            assembly: None,
            refine: None,
            fuzzy_tolerance: None,
        },
        placement: None,
        component: None,
        driven_by: None,
        native_ref: feature.native_ref.clone(),
    }
}

/// The stored geometry of a Base Feature whose bodies the v10 projection
/// names, each a body of the current geometry.
fn stored(operation: &LegacyOperation, bodies: &BTreeSet<&str>) -> Option<FeatureOperation> {
    let LegacyOperation::BaseFeature {
        bodies: BodySelection::Bodies(named) | BodySelection::Resolved { bodies: named, .. },
    } = operation
    else {
        return None;
    };
    let named = named.iter().cloned().collect::<Vec<_>>();
    (!named.is_empty() && named.iter().all(|body| bodies.contains(body.as_str()))).then(|| {
        FeatureOperation::Stored {
            bodies: Some(named),
            tessellations: Vec::new(),
            origin: StoredOrigin::Base {},
        }
    })
}

/// The feature of an item the v10 projection did not resolve: its record
/// type's family, or a native operation named by that type, suppressed as
/// its scope tail `flags` state or its Thread companion or reciprocal Hole
/// tail states.
fn unprojected_item(
    stream: &str,
    timeline: &DesignFeatureTimeline,
    record: u64,
    types: &ItemTypes<'_>,
    flags: Option<DesignScopeTailFlags>,
    companion: Option<bool>,
    losses: &mut Vec<LossNote>,
) -> Result<Feature, CodecError> {
    let id = item_feature_id(stream, record)?;
    if let Some(flags) = flags {
        losses.extend(flags.assumption_notes(&format!("feature {id}")));
    }
    let operation = match types.guid(record).map(kind_family) {
        Some(Family::Typed(family)) => FeatureOperation::Unresolved { family },
        Some(Family::Native(kind)) => native(&kind),
        None => native("timeline-item"),
    };
    Ok(Feature {
        id,
        name: None,
        suppressed: flags
            .map(|flags| flags.suppressed())
            .or(companion)
            .map(Dimension::Literal),
        definition: FeatureDefinition {
            operation,
            assembly: None,
            refine: None,
            fuzzy_tolerance: None,
        },
        placement: None,
        component: None,
        driven_by: None,
        native_ref: Some(timeline.id().clone()),
    })
}

/// A native operation of source kind `kind`, written `f3d:<kind>`.
fn native(kind: &str) -> FeatureOperation {
    let kind = CodecName::new(
        &cadmpeg_ir::codec_id!("f3d"),
        kind.strip_prefix("f3d:").unwrap_or(kind),
    )
    .unwrap_or_else(|| cadmpeg_ir::codec_name!("f3d:timeline-item"));
    FeatureOperation::Native {
        kind,
        native_options: BTreeMap::new(),
        source_settings: BTreeMap::new(),
    }
}

/// The type GUID of each record of one Design stream, from its `MetaStream`
/// type table.
struct ItemTypes<'a>(HashMap<u64, &'a str>);

impl<'a> ItemTypes<'a> {
    fn new(types: &'a [SegmentType], stream: &str) -> Self {
        let Some(entry) = stream
            .strip_prefix("f3d:")
            .and_then(crate::ids::decode_identity_key_component)
        else {
            return Self(HashMap::new());
        };
        let mut guids = HashMap::new();
        for design_type in types.iter().filter(|design_type| {
            native_stream(design_type.id()).is_some_and(|scope| {
                crate::design::decode::meta::meta_scope_matches_bulk(scope, &entry)
            })
        }) {
            for &entity in design_type.entities.values() {
                guids.insert(entity, design_type.type_guid.as_str());
            }
        }
        Self(guids)
    }

    fn guid(&self, record: u64) -> Option<&'a str> {
        self.0.get(&record).copied()
    }
}

/// The feature of an unresolved item: its stream and record index, the
/// stream length-prefixed. The item of an archive member takes the id the
/// member states alone, qualified by the member's definition key.
fn item_feature_id(stream: &str, record: u64) -> Result<FeatureId, CodecError> {
    let (key, stream) = member_scope(stream);
    let stream = IdentityKeyTail::percent_encode(&stream);
    let id = FeatureId::compose(
        &cadmpeg_ir::identity_namespace!("f3d", "design", "timeline-item"),
        cadmpeg_ir::ids::IdentityKey::from(stream.as_str().len())
            .then(cadmpeg_ir::identity_key!(":"))
            .with_tail(&stream)
            .colon(record),
    );
    match key {
        Some(key) => FeatureId::mint(qualified(key, id.as_str())?).map_err(CodecError::malformed),
        None => Ok(id),
    }
}

/// The container of the timeline record `native`. The container of an
/// archive member takes the id the member states alone, qualified by the
/// member's definition key.
fn container_id(native: &str) -> Result<ContainerId, CodecError> {
    let (key, native) = member_scope(native);
    let native = IdentityKeyTail::percent_encode(&native);
    let id = ContainerId::from(cadmpeg_ir::ids::Identity::compose(
        &cadmpeg_ir::identity_namespace!("f3d", "design", "timeline"),
        cadmpeg_ir::ids::IdentityKey::from(native.as_str().len())
            .then(cadmpeg_ir::identity_key!(":"))
            .with_tail(&native),
    ));
    match key {
        Some(key) => ContainerId::mint(qualified(key, id.as_str())?).map_err(CodecError::malformed),
        None => Ok(id),
    }
}

/// The definition key of the archive member that `native` was read from,
/// and `native` as the member states it alone.
fn member_scope(native: &str) -> (Option<&str>, std::borrow::Cow<'_, str>) {
    match crate::ids::member_key(native) {
        Some(key) => {
            let alone = &native[crate::ids::SCHEME_PREFIX.len() + key.len() + 1..];
            (
                Some(key),
                format!("{}{alone}", crate::ids::SCHEME_PREFIX).into(),
            )
        }
        None => (None, native.into()),
    }
}

fn qualified(key: &str, id: &str) -> Result<String, CodecError> {
    crate::ids::qualify(id, key)
        .ok_or_else(|| CodecError::malformed("F3D design identity is not an f3d identity"))
}
