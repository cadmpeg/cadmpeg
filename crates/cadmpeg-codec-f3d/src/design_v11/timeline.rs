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
//! The decoder does not read the rollback marker of a timeline, so each
//! timeline is stated built through its last item, and one loss names it.
//!
//! A Base Feature is stored geometry (§5.10) where the v10 projection names
//! its bodies and each is a body of the current geometry, whose records the
//! state after it holds; otherwise its bodies are typed with the carrier
//! unread, and it is native.

use std::collections::{BTreeMap, BTreeSet, HashMap};

use cadmpeg_core::decode::DecodeContext;
use cadmpeg_core::text::NonBlankString;
use cadmpeg_core::CodecError;
use cadmpeg_ir::design::container::{Container, OrderedKind, Rollback};
use cadmpeg_ir::design::dimension::Dimension;
use cadmpeg_ir::design::feature::{Feature, FeatureDefinition, FeatureOperation, StoredOrigin};
use cadmpeg_ir::design::ids::ContainerId;
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

/// The containers and features of one document's timelines.
#[derive(Default)]
pub(super) struct Projected {
    /// One ordered timeline per timeline record that lists an item, in
    /// stream and `MetaStream` order.
    pub(super) containers: Vec<Container>,
    /// The features of the items, in member order.
    pub(super) features: Vec<Feature>,
}

/// The timeline containers and item features of every document of the
/// decode, keyed by the document's identity prefix (empty for the root
/// document, `xref/…/occurrence-N/` for a referenced design). `bodies` are
/// the bodies of the current geometry. Each timeline charges one loss to
/// `losses` for its unread rollback marker.
pub(super) fn project(
    ctx: &DecodeContext<'_>,
    features: &[LegacyFeature],
    timelines: &[DesignFeatureTimeline],
    types: &[SegmentType],
    bodies: &BTreeSet<&str>,
    losses: &mut Vec<LossNote>,
) -> Result<BTreeMap<String, Projected>, CodecError> {
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
            for item in items {
                ctx.charge_entities(1, "project F3D v11 timeline item")?;
                let feature = positioned
                    .get(&(stream, ordinal))
                    .and_then(|candidates| item_feature(candidates, item.value))
                    .map_or_else(
                        || unprojected_item(stream, timeline, item.value, &item_types),
                        |feature| resolved_item(feature, bodies),
                    );
                members.push(Member::Feature(feature.id.clone()));
                projected.features.push(feature);
                ordinal = ordinal
                    .checked_add(1)
                    .ok_or_else(|| CodecError::malformed("F3D timeline ordinal exceeds u64"))?;
            }
            ctx.charge_entities(1, "project F3D v11 timeline")?;
            let id = container_id(timeline.id());
            losses.push(F3dLossCode::HistoryRollbackUnread.note(format!(
                "timeline {id}: rollback marker not read; stated built through its last item"
            )));
            projected.containers.push(Container::Ordered {
                id,
                kind: OrderedKind::Timeline,
                members,
                folders: Vec::new(),
                rollback: Rollback::End,
                placement: None,
            });
        }
    }
    Ok(documents)
}

/// The identity prefix of the document that holds `stream`: everything up to
/// and including its last `occurrence-N/` segment, empty for the root
/// document.
pub(super) fn document_prefix(stream: &str) -> &str {
    let local = stream.strip_prefix("f3d:").unwrap_or(stream);
    local
        .rfind("/occurrence-")
        .and_then(|at| {
            let tail = at + "/occurrence-".len();
            local[tail..].find('/').map(|slash| &local[..=tail + slash])
        })
        .unwrap_or("")
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

fn resolved_item(feature: &LegacyFeature, bodies: &BTreeSet<&str>) -> Feature {
    let operation = feature.evaluation.definition().operation();
    let operation = stored(operation, bodies).unwrap_or_else(|| match family(operation) {
        Family::Typed(family) => FeatureOperation::Unresolved { family },
        Family::Native(kind) => native(&kind),
    });
    Feature {
        id: feature.id.clone(),
        name: feature.name.clone(),
        suppressed: feature.suppressed.map(Dimension::Literal),
        definition: FeatureDefinition {
            operation,
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
/// type's family, or a native operation named by that type.
fn unprojected_item(
    stream: &str,
    timeline: &DesignFeatureTimeline,
    record: u64,
    types: &ItemTypes<'_>,
) -> Feature {
    let operation = match types.guid(record).map(kind_family) {
        Some(Family::Typed(family)) => FeatureOperation::Unresolved { family },
        Some(Family::Native(kind)) => native(&kind),
        None => native("timeline-item"),
    };
    Feature {
        id: item_feature_id(stream, record),
        name: None,
        suppressed: None,
        definition: FeatureDefinition {
            operation,
            refine: None,
            fuzzy_tolerance: None,
        },
        placement: None,
        component: None,
        driven_by: None,
        native_ref: Some(timeline.id().clone()),
    }
}

/// A native operation of source kind `kind`, written `f3d:<kind>`.
fn native(kind: &str) -> FeatureOperation {
    let kind = NonBlankString::new(format!("f3d:{}", kind.strip_prefix("f3d:").unwrap_or(kind)))
        .unwrap_or_else(|| cadmpeg_core::nonblank_literal!("f3d:timeline-item"));
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
/// stream length-prefixed.
fn item_feature_id(stream: &str, record: u64) -> FeatureId {
    let stream = IdentityKeyTail::percent_encode(stream);
    let key = cadmpeg_ir::ids::IdentityKey::from(stream.as_str().len())
        .then(cadmpeg_ir::identity_key!(":"))
        .with_tail(&stream)
        .colon(record);
    FeatureId::compose(
        &cadmpeg_ir::identity_namespace!("f3d", "design", "timeline-item"),
        key,
    )
}

/// The container of the timeline record `native`.
fn container_id(native: &str) -> ContainerId {
    let native = IdentityKeyTail::percent_encode(native);
    ContainerId::from(cadmpeg_ir::ids::Identity::compose(
        &cadmpeg_ir::identity_namespace!("f3d", "design", "timeline"),
        cadmpeg_ir::ids::IdentityKey::from(native.as_str().len())
            .then(cadmpeg_ir::identity_key!(":"))
            .with_tail(&native),
    ))
}
