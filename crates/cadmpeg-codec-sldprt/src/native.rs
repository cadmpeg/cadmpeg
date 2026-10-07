// SPDX-License-Identifier: Apache-2.0
//! SOLIDWORKS native feature-history records.
#![deny(clippy::disallowed_methods)]

use serde::{ser::SerializeMap, Deserialize, Serialize};

use cadmpeg_core::decode::{DecodeContext, View};
use cadmpeg_ir::native::catalogue::{Catalogue, FamilyRow, Phase};

use crate::records::charged_clone::CloneCharged;

use crate::records::{
    FeatureHistory, FeatureInputBodySelection, FeatureInputClass, FeatureInputEdgeSelection,
    FeatureInputGeneratedSurfaceIdentity, FeatureInputLane, FeatureInputName,
    FeatureInputReference, FeatureInputRelationBinding, FeatureInputRelationInstance,
    FeatureInputScalar, FeatureInputSurfaceSelection, PmiDimension,
};

const SLDPRT_ARENA_NAMES: &[&str] = &[
    "configurations",
    "feature_histories",
    "feature_input_body_selections",
    "feature_input_classes",
    "feature_input_edge_selections",
    "feature_input_generated_surface_identities",
    "feature_input_lanes",
    "feature_input_names",
    "feature_input_references",
    "feature_input_relation_bindings",
    "feature_input_relation_instances",
    "feature_input_scalars",
    "feature_input_surface_selections",
    "features",
    "pmi_dimensions",
    "sketch_input_entities",
];

type SldprtFamilyRow = FamilyRow<SldprtNative, (), cadmpeg_ir::NativeNamespace, ()>;

struct HistoryArenaView<'a>(&'a FeatureHistory);

impl Serialize for HistoryArenaView<'_> {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let mut map = serializer.serialize_map(None)?;
        map.serialize_entry("id", &self.0.id)?;
        if self.0.part_name.is_some() {
            map.serialize_entry("part_name", &self.0.part_name)?;
        }
        if !self.0.properties.is_empty() {
            map.serialize_entry("properties", &self.0.properties)?;
        }
        if !self.0.content.is_empty() {
            map.serialize_entry("content", &self.0.content)?;
        }
        map.serialize_entry("configurations", NO_ENTRIES)?;
        map.serialize_entry("features", NO_ENTRIES)?;
        map.end()
    }
}

/// An empty array for the native sections this view leaves unpopulated.
const NO_ENTRIES: &[()] = &[];

struct LanePayload<'a>(&'a [u8]);

impl Serialize for LanePayload<'_> {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        cadmpeg_ir::bytes::serialize(self.0, serializer)
    }
}

struct LaneArenaView<'a>(&'a FeatureInputLane);

impl Serialize for LaneArenaView<'_> {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let mut map = serializer.serialize_map(None)?;
        map.serialize_entry("id", &self.0.id)?;
        if self.0.configuration.is_some() {
            map.serialize_entry("configuration", &self.0.configuration)?;
        }
        map.serialize_entry("native_payload", &LanePayload(&self.0.native_payload))?;
        for field in [
            "classes",
            "names",
            "scalars",
            "relation_bindings",
            "relation_instances",
            "body_selections",
            "edge_selections",
            "surface_selections",
            "generated_surface_identities",
            "references",
            "sketch_entities",
        ] {
            map.serialize_entry(field, NO_ENTRIES)?;
        }
        map.end()
    }
}

macro_rules! lane_family {
    ($arena:literal, $field:ident) => {
        SldprtFamilyRow {
            arena: $arena,
            exactness: (),
            phase: Phase::ArenaOnly,
            emit: |ctx, model, row, namespace| {
                namespace.set_arena_from(
                    ctx,
                    row.arena,
                    model
                        .feature_input_lanes
                        .iter()
                        .flat_map(|lane| lane.$field.iter()),
                )
            },
            len: |model| {
                model
                    .feature_input_lanes
                    .iter()
                    .map(|lane| lane.$field.len())
                    .sum()
            },
            counts_toward_emptiness: true,
        }
    };
}

const SLDPRT_FAMILIES: &[SldprtFamilyRow] = &[
    SldprtFamilyRow {
        arena: "feature_histories",
        exactness: (),
        phase: Phase::ArenaOnly,
        emit: |ctx, model, row, namespace| {
            namespace.set_arena_from(
                ctx,
                row.arena,
                model.feature_histories.iter().map(HistoryArenaView),
            )
        },
        len: |model| model.feature_histories.len(),
        counts_toward_emptiness: true,
    },
    SldprtFamilyRow {
        arena: "pmi_dimensions",
        exactness: (),
        phase: Phase::ArenaOnly,
        emit: |ctx, model, row, namespace| {
            namespace.set_arena(ctx, row.arena, &model.pmi_dimensions)
        },
        len: |model| model.pmi_dimensions.len(),
        counts_toward_emptiness: true,
    },
    SldprtFamilyRow {
        arena: "configurations",
        exactness: (),
        phase: Phase::ArenaOnly,
        emit: |ctx, model, row, namespace| {
            namespace.set_arena_from(
                ctx,
                row.arena,
                model
                    .feature_histories
                    .iter()
                    .flat_map(|history| history.configurations.iter()),
            )
        },
        len: |model| {
            model
                .feature_histories
                .iter()
                .map(|history| history.configurations.len())
                .sum()
        },
        counts_toward_emptiness: true,
    },
    SldprtFamilyRow {
        arena: "features",
        exactness: (),
        phase: Phase::ArenaOnly,
        emit: |ctx, model, row, namespace| {
            namespace.set_arena_from(
                ctx,
                row.arena,
                model
                    .feature_histories
                    .iter()
                    .flat_map(|history| history.features.iter()),
            )
        },
        len: |model| {
            model
                .feature_histories
                .iter()
                .map(|history| history.features.len())
                .sum()
        },
        counts_toward_emptiness: true,
    },
    SldprtFamilyRow {
        arena: "feature_input_lanes",
        exactness: (),
        phase: Phase::ArenaOnly,
        emit: |ctx, model, row, namespace| {
            namespace.set_arena_from(
                ctx,
                row.arena,
                model.feature_input_lanes.iter().map(LaneArenaView),
            )
        },
        len: |model| model.feature_input_lanes.len(),
        counts_toward_emptiness: true,
    },
    lane_family!("feature_input_body_selections", body_selections),
    lane_family!("feature_input_edge_selections", edge_selections),
    lane_family!("feature_input_surface_selections", surface_selections),
    lane_family!(
        "feature_input_generated_surface_identities",
        generated_surface_identities
    ),
    lane_family!("feature_input_classes", classes),
    lane_family!("feature_input_names", names),
    lane_family!("feature_input_scalars", scalars),
    lane_family!("feature_input_references", references),
    lane_family!("feature_input_relation_bindings", relation_bindings),
    lane_family!("feature_input_relation_instances", relation_instances),
    lane_family!("sketch_input_entities", sketch_entities),
];

const SLDPRT_CATALOGUE: Catalogue<'static, SldprtNative, (), cadmpeg_ir::NativeNamespace, ()> =
    Catalogue::new(SLDPRT_FAMILIES);

/// Byte-wise string equality, usable in a constant expression.
const fn arena_name_eq(left: &str, right: &str) -> bool {
    let (left, right) = (left.as_bytes(), right.as_bytes());
    if left.len() != right.len() {
        return false;
    }
    let mut index = 0;
    while index < left.len() {
        if left[index] != right[index] {
            return false;
        }
        index += 1;
    }
    true
}

/// `true` when `name` is the arena of one declared family row.
const fn family_declares(name: &str) -> bool {
    let mut index = 0;
    while index < SLDPRT_FAMILIES.len() {
        if arena_name_eq(SLDPRT_FAMILIES[index].arena, name) {
            return true;
        }
        index += 1;
    }
    false
}

/// `true` when `name` is one of the pinned arena names.
const fn pinned_declares(name: &str) -> bool {
    let mut index = 0;
    while index < SLDPRT_ARENA_NAMES.len() {
        if arena_name_eq(SLDPRT_ARENA_NAMES[index], name) {
            return true;
        }
        index += 1;
    }
    false
}

/// The pinned arena list and the emitted family table state the same arenas.
///
/// `Catalogue::emit_all` runs every row and each row writes `row.arena`, so
/// this agreement is exactly the statement that a store leaves every pinned
/// arena present. The check is a constant expression, so a family added or
/// removed without the matching pinned name fails the build.
const fn arena_names_agree_with_families() -> bool {
    if SLDPRT_ARENA_NAMES.len() != SLDPRT_FAMILIES.len() {
        return false;
    }
    let mut index = 0;
    while index < SLDPRT_ARENA_NAMES.len() {
        if !family_declares(SLDPRT_ARENA_NAMES[index]) {
            return false;
        }
        index += 1;
    }
    let mut index = 0;
    while index < SLDPRT_FAMILIES.len() {
        if !pinned_declares(SLDPRT_FAMILIES[index].arena) {
            return false;
        }
        index += 1;
    }
    true
}

const _: () = assert!(
    arena_names_agree_with_families(),
    "SLDPRT_ARENA_NAMES and SLDPRT_FAMILIES state different arenas"
);

/// SOLIDWORKS records retained outside the format-neutral model.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub(crate) struct SldprtNative {
    /// Parametric construction-history timelines decoded from the source part.
    pub(crate) feature_histories: Vec<FeatureHistory>,
    /// Native feature-input byte streams retained for parametric replay and rewrite.
    pub(crate) feature_input_lanes: Vec<FeatureInputLane>,
    /// Semantic dimensions decoded from `PMISemanticDataDB`.
    pub(crate) pmi_dimensions: Vec<PmiDimension>,
}

pub(crate) mod lanes;

impl SldprtNative {
    pub(crate) fn load(
        namespace: &cadmpeg_ir::NativeNamespace,
    ) -> Result<Self, cadmpeg_ir::NativeConvertError> {
        let arena = cadmpeg_core::decode::DecodeArena::new();
        let mut policy = cadmpeg_core::decode::DecodePolicy::default();
        // A native field can contain 256 containers; reconstruction adds the record root.
        policy.limits.max_recursion_depth = 257;
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy)?;
        Self::load_charged(&ctx, namespace)
    }

    pub(crate) fn load_charged(
        ctx: &DecodeContext<'_>,
        namespace: &cadmpeg_ir::NativeNamespace,
    ) -> Result<Self, cadmpeg_ir::NativeConvertError> {
        macro_rules! read_arena {
            ($name:literal) => {
                namespace.arena_as_for_decode(ctx, $name)?
            };
        }
        let lane_wires: Vec<crate::records::FeatureInputLaneWire> =
            read_arena!("feature_input_lanes");
        let mut native = Self {
            feature_histories: read_arena!("feature_histories"),
            feature_input_lanes: ctx.try_collect_vec(
                lane_wires.into_iter().map(|wire| wire.admit(ctx)),
                "admit SLDPRT native lanes",
            )?,
            pmi_dimensions: read_arena!("pmi_dimensions"),
        };
        let configurations: Vec<crate::records::Configuration> = read_arena!("configurations");
        let features: Vec<crate::records::Feature> = read_arena!("features");
        let entity_wires: Vec<crate::records::SketchInputEntityWire> =
            read_arena!("sketch_input_entities");
        let classes: Vec<FeatureInputClass> = read_arena!("feature_input_classes");
        let body_selections: Vec<FeatureInputBodySelection> =
            read_arena!("feature_input_body_selections");
        let edge_selections: Vec<FeatureInputEdgeSelection> =
            read_arena!("feature_input_edge_selections");
        let surface_selections: Vec<FeatureInputSurfaceSelection> =
            read_arena!("feature_input_surface_selections");
        let generated_surface_identities: Vec<FeatureInputGeneratedSurfaceIdentity> =
            read_arena!("feature_input_generated_surface_identities");
        let names: Vec<FeatureInputName> = read_arena!("feature_input_names");
        let references: Vec<FeatureInputReference> = read_arena!("feature_input_references");
        let relation_bindings: Vec<FeatureInputRelationBinding> =
            read_arena!("feature_input_relation_bindings");
        let relation_wires: Vec<crate::records::FeatureInputRelationInstanceWire> =
            read_arena!("feature_input_relation_instances");
        let relation_instances = ctx.try_collect_vec(
            relation_wires.into_iter().map(|wire| wire.admit(ctx)),
            "admit SLDPRT native relations",
        )?;
        let scalars: Vec<FeatureInputScalar> = read_arena!("feature_input_scalars");
        let (history_ids, _history_ids_reservation) = ctx.collect_scoped_string_set(
            native.feature_histories.len(),
            native
                .feature_histories
                .iter()
                .map(|history| history.id.as_str()),
            "index SLDPRT history ids",
        )?;
        if let Some(record) = first_orphan(ctx, &configurations, &history_ids, |record| {
            Some(record.parent.as_str())
        })? {
            return Err(cadmpeg_ir::NativeConvertError::InvalidOwner(
                ctx.format_retained(
                    format_args!("configuration {} references {}", record.id, record.parent),
                    "format SLDPRT native validation error",
                )?,
            ));
        }
        if let Some(record) = first_orphan(ctx, &features, &history_ids, |record| {
            Some(record.parent.as_str())
        })? {
            return Err(cadmpeg_ir::NativeConvertError::InvalidOwner(
                ctx.format_retained(
                    format_args!("feature {} references {}", record.id, record.parent),
                    "format SLDPRT native validation error",
                )?,
            ));
        }
        let (feature_ids, _feature_ids_reservation) = ctx.collect_scoped_string_set(
            features.len(),
            features.iter().map(|record| record.id.as_str()),
            "index SLDPRT feature ids",
        )?;
        let (lane_ids, _lane_ids_reservation) = ctx.collect_scoped_string_set(
            native.feature_input_lanes.len(),
            native
                .feature_input_lanes
                .iter()
                .map(|lane| lane.id.as_str()),
            "index SLDPRT lane ids",
        )?;
        let (lane_payloads, _lane_payloads_reservation) = ctx.collect_scoped_string_map(
            native.feature_input_lanes.len(),
            native
                .feature_input_lanes
                .iter()
                .map(|lane| (lane.id.as_str(), lane.native_payload.as_slice())),
            "index SLDPRT lane payloads",
        )?;
        if let Some(record) = first_orphan(ctx, &entity_wires, &lane_ids, |record| {
            Some(record.parent.as_str())
        })? {
            return Err(cadmpeg_ir::NativeConvertError::InvalidOwner(
                ctx.format_retained(
                    format_args!(
                        "sketch input entity {} references {}",
                        record.id, record.parent
                    ),
                    "format SLDPRT native validation error",
                )?,
            ));
        }
        let mut entities = Vec::new();
        {
            ctx.reserve_vec(
                &mut entities,
                entity_wires.len(),
                "load SLDPRT sketch entities",
            )?;
        }
        for wire in ctx
            .admit_iter(entity_wires, "load SLDPRT sketch entities")
            .map_err(cadmpeg_core::CodecError::from)?
        {
            let Some(payload) = ctx
                .get_hash_map(
                    &(lane_payloads),
                    wire.parent.as_str(),
                    "look up SLDPRT hash key",
                )?
                .copied()
            else {
                return Err(cadmpeg_ir::NativeConvertError::InvalidOwner(
                    ctx.format_retained(
                        format_args!(
                            "sketch input entity {} references lane {} without a payload",
                            wire.id, wire.parent
                        ),
                        "format SLDPRT native validation error",
                    )?,
                ));
            };
            let entity = crate::records::SketchInputEntity::try_from_wire(wire, payload)
                .map_err(cadmpeg_ir::NativeConvertError::InvalidOwner)?;
            entities.push(entity);
        }
        if let Some(record) = first_orphan(ctx, &classes, &lane_ids, |record| {
            Some(record.parent.as_str())
        })? {
            return Err(cadmpeg_ir::NativeConvertError::InvalidOwner(
                ctx.format_retained(
                    format_args!(
                        "feature-input class {} references {}",
                        record.id, record.parent
                    ),
                    "format SLDPRT native validation error",
                )?,
            ));
        }
        if let Some(record) = first_orphan(ctx, &body_selections, &lane_ids, |record| {
            Some(record.parent.as_str())
        })? {
            return Err(cadmpeg_ir::NativeConvertError::InvalidOwner(
                ctx.format_retained(
                    format_args!(
                        "feature-input body selection {} references {}",
                        record.id, record.parent
                    ),
                    "format SLDPRT native validation error",
                )?,
            ));
        }
        if let Some(record) = first_orphan(ctx, &edge_selections, &lane_ids, |record| {
            Some(record.parent.as_str())
        })? {
            return Err(cadmpeg_ir::NativeConvertError::InvalidOwner(
                ctx.format_retained(
                    format_args!(
                        "feature-input edge selection {} references {}",
                        record.id, record.parent
                    ),
                    "format SLDPRT native validation error",
                )?,
            ));
        }
        if let Some(record) = first_orphan(ctx, &surface_selections, &lane_ids, |record| {
            Some(record.parent.as_str())
        })? {
            return Err(cadmpeg_ir::NativeConvertError::InvalidOwner(
                ctx.format_retained(
                    format_args!(
                        "feature-input surface selection {} references {}",
                        record.id, record.parent
                    ),
                    "format SLDPRT native validation error",
                )?,
            ));
        }
        if let Some(record) =
            first_orphan(ctx, &generated_surface_identities, &lane_ids, |record| {
                Some(record.parent.as_str())
            })?
        {
            return Err(cadmpeg_ir::NativeConvertError::InvalidOwner(
                ctx.format_retained(
                    format_args!(
                        "feature-input generated surface identity {} references {}",
                        record.id, record.parent
                    ),
                    "format SLDPRT native validation error",
                )?,
            ));
        }
        if let Some(record) = first_orphan(ctx, &names, &lane_ids, |record| {
            Some(record.parent.as_str())
        })? {
            return Err(cadmpeg_ir::NativeConvertError::InvalidOwner(
                ctx.format_retained(
                    format_args!(
                        "feature-input name {} references {}",
                        record.id, record.parent
                    ),
                    "format SLDPRT native validation error",
                )?,
            ));
        }
        if let Some(record) = first_orphan(ctx, &scalars, &lane_ids, |record| {
            Some(record.parent.as_str())
        })? {
            return Err(cadmpeg_ir::NativeConvertError::InvalidOwner(
                ctx.format_retained(
                    format_args!(
                        "feature-input scalar {} references {}",
                        record.id, record.parent
                    ),
                    "format SLDPRT native validation error",
                )?,
            ));
        }
        if let Some(record) = first_orphan(ctx, &scalars, &feature_ids, |record| {
            record.feature_ref.as_deref()
        })? {
            return Err(cadmpeg_ir::NativeConvertError::InvalidOwner(
                ctx.format_retained(
                    format_args!(
                        "feature-input scalar {} references missing feature {}",
                        record.id,
                        record.feature_ref.as_deref().unwrap_or_default()
                    ),
                    "format SLDPRT native validation error",
                )?,
            ));
        }
        if let Some(record) = first_orphan(ctx, &references, &lane_ids, |record| {
            Some(record.parent.as_str())
        })? {
            return Err(cadmpeg_ir::NativeConvertError::InvalidOwner(
                ctx.format_retained(
                    format_args!(
                        "feature-input reference {} references {}",
                        record.id, record.parent
                    ),
                    "format SLDPRT native validation error",
                )?,
            ));
        }
        if let Some(record) = first_orphan(ctx, &relation_bindings, &lane_ids, |record| {
            Some(record.parent.as_str())
        })? {
            return Err(cadmpeg_ir::NativeConvertError::InvalidOwner(
                ctx.format_retained(
                    format_args!(
                        "feature-input relation binding {} references {}",
                        record.id, record.parent
                    ),
                    "format SLDPRT native validation error",
                )?,
            ));
        }
        if let Some(record) = first_orphan(ctx, &relation_instances, &lane_ids, |record| {
            Some(record.parent.as_str())
        })? {
            return Err(cadmpeg_ir::NativeConvertError::InvalidOwner(
                ctx.format_retained(
                    format_args!(
                        "feature-input relation instance {} references {}",
                        record.id, record.parent
                    ),
                    "format SLDPRT native validation error",
                )?,
            ));
        }
        let lookup = ScalarLookup::new(ctx, &scalars, &names)?;
        if let Some(record) = ctx.find_by(
            &body_selections,
            |record| {
                let record = *record;
                Ok({
                    lookup
                        .name_value(ctx, record.object_name_ref.as_str())?
                        .is_none()
                        || !ctx.contains_hash_set(
                            &(feature_ids),
                            record.feature_ref.as_str(),
                            "test SLDPRT hashed identity",
                        )?
                        || record.local_body_ids.is_empty()
                })
            },
            "validate SLDPRT native references",
        )? {
            return Err(cadmpeg_ir::NativeConvertError::InvalidOwner(
                ctx.format_retained(
                    format_args!(
                        "feature-input body selection {} has unresolved ownership",
                        record.id
                    ),
                    "format SLDPRT native validation error",
                )?,
            ));
        }
        if let Some(record) = ctx.find_by(
            &edge_selections,
            |record| {
                let record = *record;
                Ok({
                    lookup
                        .name_value(ctx, record.object_name_ref.as_str())?
                        .is_none()
                        || !ctx.contains_hash_set(
                            &(feature_ids),
                            record.feature_ref.as_str(),
                            "test SLDPRT hashed identity",
                        )?
                        || record.local_edge_ids.is_empty()
                })
            },
            "validate SLDPRT native references",
        )? {
            return Err(cadmpeg_ir::NativeConvertError::InvalidOwner(
                ctx.format_retained(
                    format_args!(
                        "feature-input edge selection {} has unresolved ownership",
                        record.id
                    ),
                    "format SLDPRT native validation error",
                )?,
            ));
        }
        if let Some(record) = ctx.find_by(
            &surface_selections,
            |record| {
                let record = *record;
                Ok({
                    lookup
                        .name_value(ctx, record.object_name_ref.as_str())?
                        .is_none()
                        || !ctx.contains_hash_set(
                            &(feature_ids),
                            record.feature_ref.as_str(),
                            "test SLDPRT hashed identity",
                        )?
                        || record.components.is_empty()
                        || ctx
                            .admit_iter(
                                &record.producer_feature_refs[..],
                                "scan SLDPRT load_charged values",
                            )
                            .map_err(cadmpeg_core::CodecError::from)?
                            .try_fold(false, |found, producer| {
                                Ok::<_, cadmpeg_core::CodecError>(
                                    found
                                        || (!ctx.contains_hash_set(
                                            &(feature_ids),
                                            producer.as_str(),
                                            "test SLDPRT hashed identity",
                                        )?),
                                )
                            })?
                        || match record.terminal_feature_ref.as_deref() {
                            Some(feature) => !ctx.contains_hash_set(
                                &(feature_ids),
                                feature,
                                "test SLDPRT hashed identity",
                            )?,
                            None => false,
                        }
                        || match record.endpoint_selector() {
                            Some(selector) => {
                                usize::try_from(record.offset)
                                    .ok()
                                    .and_then(|offset| offset.checked_sub(4))
                                    .map(|offset| {
                                        Ok::<_, cadmpeg_core::CodecError>(
                                            ctx.get_hash_map(
                                                &(lane_payloads),
                                                record.parent.as_str(),
                                                "look up SLDPRT hash key",
                                            )?
                                            .and_then(|payload| View::u32_le_at(payload, offset)),
                                        )
                                    })
                                    .transpose()?
                                    .flatten()
                                    != Some(selector)
                            }
                            None => false,
                        }
                })
            },
            "validate SLDPRT native references",
        )? {
            return Err(cadmpeg_ir::NativeConvertError::InvalidOwner(
                ctx.format_retained(
                    format_args!(
                        "feature-input surface selection {} has unresolved ownership",
                        record.id
                    ),
                    "format SLDPRT native validation error",
                )?,
            ));
        }
        if let Some(record) = ctx.find_by(
            &scalars,
            |record| {
                let record = *record;
                Ok(lookup.name_value(ctx, record.name.as_str())?.is_none())
            },
            "validate SLDPRT native references",
        )? {
            return Err(cadmpeg_ir::NativeConvertError::InvalidOwner(
                ctx.format_retained(
                    format_args!(
                        "feature-input scalar {} references name {}",
                        record.id, record.name
                    ),
                    "format SLDPRT native validation error",
                )?,
            ));
        }
        let (references_by_id, _references_by_id_reservation) = ctx.collect_scoped_string_map(
            references.len(),
            references.iter().map(|record| (record.id.as_str(), record)),
            "index SLDPRT references",
        )?;
        // The first class with an identity answers, so the index is filled
        // in reverse.
        let (classes_by_id, _classes_by_id_reservation) = ctx.collect_scoped_string_map(
            classes.len(),
            ctx.admit_iter(&classes, "index SLDPRT classes")
                .map_err(cadmpeg_core::CodecError::from)?
                .rev()
                .map(|record| (record.id.as_str(), record)),
            "index SLDPRT classes",
        )?;
        if let Some(record) = ctx.find_by(
            &relation_bindings,
            |record| {
                let record = *record;
                Ok({
                    ctx.get_hash_map(
                        &classes_by_id,
                        record.class_ref.as_str(),
                        "test SLDPRT hashed identity",
                    )?
                    .is_none()
                        || lookup.scalar(ctx, record.scalar_ref.as_str())?.is_none()
                        || match record.feature_ref.as_deref() {
                            Some(feature) => !ctx.contains_hash_set(
                                &(feature_ids),
                                feature,
                                "test SLDPRT hashed identity",
                            )?,
                            None => false,
                        }
                })
            },
            "validate SLDPRT native references",
        )? {
            return Err(cadmpeg_ir::NativeConvertError::InvalidOwner(
                ctx.format_retained(
                    format_args!(
                        "feature-input relation binding {} has an unresolved class or scalar",
                        record.id
                    ),
                    "format SLDPRT native validation error",
                )?,
            ));
        }
        let instance_owners = RelationInstanceOwners {
            classes: &classes_by_id,
            features: &feature_ids,
            lookup: &lookup,
        };
        if let Some(record) = ctx.find_by(
            &relation_instances,
            |record| instance_owners.unresolved(ctx, record),
            "validate SLDPRT native references",
        )? {
            return Err(cadmpeg_ir::NativeConvertError::InvalidOwner(
                ctx.format_retained(
                    format_args!(
                        "feature-input relation instance {} has an unresolved class, feature, or scalar",
                        record.id
                    ),
                    "format SLDPRT native validation error",
                )?,
            ));
        }
        for scalar in ctx
            .admit_iter(&scalars, "scan SLDPRT load_charged values")
            .map_err(cadmpeg_core::CodecError::from)?
        {
            for operand in ctx
                .admit_iter(&scalar.operands, "scan SLDPRT load_charged values")
                .map_err(cadmpeg_core::CodecError::from)?
            {
                let Some(reference) = ctx.get_hash_map(
                    &(references_by_id),
                    operand.reference_ref.as_str(),
                    "look up SLDPRT hash key",
                )?
                else {
                    return Err(cadmpeg_ir::NativeConvertError::InvalidOwner(
                        ctx.format_retained(
                            format_args!(
                                "feature-input scalar {} references missing cell {}",
                                scalar.id, operand.reference_ref
                            ),
                            "format SLDPRT native validation error",
                        )?,
                    ));
                };
                if reference.offset != operand.offset
                    || reference.kind != operand.kind
                    || reference.object_index != operand.entity_index
                {
                    return Err(cadmpeg_ir::NativeConvertError::InvalidOwner(
                        ctx.format_retained(
                            format_args!(
                                "feature-input scalar {} has inconsistent cell {}",
                                scalar.id, operand.reference_ref
                            ),
                            "format SLDPRT native validation error",
                        )?,
                    ));
                }
            }
        }
        // Lane records are validated against a context of all history
        // features, so lanes are filled before the features move into their
        // histories.
        let lane_ids = ctx.try_collect_vec(
            ctx.admit_iter(&native.feature_input_lanes, "index SLDPRT native lanes")
                .map_err(cadmpeg_core::CodecError::from)?
                .map(|lane| Ok::<_, cadmpeg_core::CodecError>(lane.id.as_str())),
            "index SLDPRT native lanes",
        )?;
        let mut lane_classes = attach_to_owners(
            ctx,
            &lane_ids,
            classes,
            |record| &record.parent,
            |record| record.ordinal,
        )?;
        let mut lane_names = attach_to_owners(
            ctx,
            &lane_ids,
            names,
            |record| &record.parent,
            |record| record.ordinal,
        )?;
        let mut lane_scalars = attach_to_owners(
            ctx,
            &lane_ids,
            scalars,
            |record| &record.parent,
            |record| record.ordinal,
        )?;
        let mut lane_references = attach_to_owners(
            ctx,
            &lane_ids,
            references,
            |record| &record.parent,
            |record| record.ordinal,
        )?;
        let mut lane_relation_bindings = attach_to_owners(
            ctx,
            &lane_ids,
            relation_bindings,
            |record| &record.parent,
            |record| record.ordinal,
        )?;
        let mut lane_relation_instances = attach_to_owners(
            ctx,
            &lane_ids,
            relation_instances,
            |record| &record.parent,
            |record| record.ordinal,
        )?;
        let mut lane_body_selections = attach_to_owners(
            ctx,
            &lane_ids,
            body_selections,
            |record| &record.parent,
            |record| record.ordinal,
        )?;
        let mut lane_edge_selections = attach_to_owners(
            ctx,
            &lane_ids,
            edge_selections,
            |record| &record.parent,
            |record| record.ordinal,
        )?;
        let mut lane_surface_selections = attach_to_owners(
            ctx,
            &lane_ids,
            surface_selections,
            |record| &record.parent,
            |record| record.ordinal,
        )?;
        let mut lane_generated_surfaces = attach_to_owners(
            ctx,
            &lane_ids,
            generated_surface_identities,
            |record| &record.parent,
            |record| record.ordinal,
        )?;
        let mut lane_entities = attach_to_owners(
            ctx,
            &lane_ids,
            entities,
            crate::records::SketchInputEntity::parent,
            crate::records::SketchInputEntity::ordinal,
        )?;
        drop(lane_ids);
        for (index, lane) in ctx
            .admit_iter(
                &mut native.feature_input_lanes,
                "attach SLDPRT lane records",
            )
            .map_err(cadmpeg_core::CodecError::from)?
            .enumerate()
        {
            lane.classes = std::mem::take(&mut lane_classes[index]);
            lane.names = std::mem::take(&mut lane_names[index]);
            lane.scalars = std::mem::take(&mut lane_scalars[index]);
            lane.references = std::mem::take(&mut lane_references[index]);
            lane.relation_bindings = std::mem::take(&mut lane_relation_bindings[index]);
            lane.relation_instances = std::mem::take(&mut lane_relation_instances[index]);
            lane.body_selections = std::mem::take(&mut lane_body_selections[index]);
            for record in ctx
                .admit_iter(&lane.body_selections, "validate SLDPRT body selections")
                .map_err(cadmpeg_core::CodecError::from)?
            {
                if body_selection_disagrees_with_payload(ctx, lane, record)? {
                    return Err(cadmpeg_ir::NativeConvertError::InvalidOwner(
                        ctx.format_retained(
                            format_args!(
                                "feature-input body selection {} disagrees with its payload",
                                record.id
                            ),
                            "format SLDPRT native validation error",
                        )?,
                    ));
                }
            }
            lane.edge_selections = std::mem::take(&mut lane_edge_selections[index]);
            lane.surface_selections = std::mem::take(&mut lane_surface_selections[index]);
            if !lane.edge_selections.is_empty() || !lane.surface_selections.is_empty() {
                validate_lane_selections(ctx, lane, &features)?;
            }
            lane.generated_surface_identities = std::mem::take(&mut lane_generated_surfaces[index]);
            if generated_surface_identities_disagree_with_payload(ctx, lane)? {
                return Err(cadmpeg_ir::NativeConvertError::InvalidOwner(
                    ctx.format_retained(
                        format_args!(
                            "feature-input lane {} generated surface identities disagree with its payload",
                            lane.id
                        ),
                        "format SLDPRT native validation error",
                    )?,
                ));
            }
            lane.sketch_entities = std::mem::take(&mut lane_entities[index]);
        }
        let history_ids = ctx.try_collect_vec(
            ctx.admit_iter(&native.feature_histories, "index SLDPRT native histories")
                .map_err(cadmpeg_core::CodecError::from)?
                .map(|history| Ok::<_, cadmpeg_core::CodecError>(history.id.as_str())),
            "index SLDPRT native histories",
        )?;
        let mut history_configurations = attach_to_owners(
            ctx,
            &history_ids,
            configurations,
            |record| &record.parent,
            |record| record.ordinal,
        )?;
        let mut history_features = attach_to_owners(
            ctx,
            &history_ids,
            features,
            |record| &record.parent,
            |record| record.ordinal,
        )?;
        drop(history_ids);
        for (index, history) in ctx
            .admit_iter(
                &mut native.feature_histories,
                "attach SLDPRT history records",
            )
            .map_err(cadmpeg_core::CodecError::from)?
            .enumerate()
        {
            history.configurations = std::mem::take(&mut history_configurations[index]);
            if let Some(ordinal) =
                repeated_ordinal(ctx, &history.configurations, |record| record.ordinal)?
            {
                return Err(cadmpeg_ir::NativeConvertError::InvalidOwner(
                    ctx.format_retained(
                        format_args!(
                            "SolidWorks history {} repeats configuration ordinal {ordinal}",
                            history.id
                        ),
                        "format SLDPRT native validation error",
                    )?,
                ));
            }
            history.features = std::mem::take(&mut history_features[index]);
            if let Some(ordinal) =
                repeated_ordinal(ctx, &history.features, |record| record.ordinal)?
            {
                return Err(cadmpeg_ir::NativeConvertError::InvalidOwner(
                    ctx.format_retained(
                        format_args!(
                            "SolidWorks history {} repeats feature ordinal {ordinal}",
                            history.id
                        ),
                        "format SLDPRT native validation error",
                    )?,
                ));
            }
        }
        lanes::admit(&native, ctx)?;
        Ok(native)
    }

    pub(crate) fn store(
        &self,
        ctx: &cadmpeg_core::decode::DecodeContext<'_>,
        namespace: &mut cadmpeg_ir::NativeNamespace,
    ) -> Result<(), cadmpeg_ir::NativeConvertError> {
        const CLASSES: &str = "compare SLDPRT stored history classes";
        // Load admits every record against the lane payload it is derived from;
        // that is the boundary a hand-written namespace crosses. Store holds the
        // relations between records that no single payload derives.
        for history in ctx
            .admit_iter(&self.feature_histories, "scan SLDPRT store values")
            .map_err(cadmpeg_core::CodecError::from)?
        {
            if let Some(record) = ctx.find_by(
                &history.configurations,
                |record| {
                    Ok(!ctx.equal(
                        record.parent.as_str(),
                        history.id.as_str(),
                        "validate SLDPRT stored records",
                    )?)
                },
                "validate SLDPRT stored records",
            )? {
                return Err(cadmpeg_ir::NativeConvertError::InvalidOwner(
                    ctx.format_retained(
                        format_args!(
                            "configuration {} references {} instead of {}",
                            record.id, record.parent, history.id
                        ),
                        "format SLDPRT native validation error",
                    )?,
                ));
            }
            if let Some(record) = ctx.find_by(
                &history.features,
                |record| {
                    Ok(!ctx.equal(
                        record.parent.as_str(),
                        history.id.as_str(),
                        "validate SLDPRT stored records",
                    )?)
                },
                "validate SLDPRT stored records",
            )? {
                return Err(cadmpeg_ir::NativeConvertError::InvalidOwner(
                    ctx.format_retained(
                        format_args!(
                            "feature {} references {} instead of {}",
                            record.id, record.parent, history.id
                        ),
                        "format SLDPRT native validation error",
                    )?,
                ));
            }
        }
        let mut feature_ids_reservation = ctx.reserve_scoped(0, "index SLDPRT stored features")?;
        let mut feature_ids = std::collections::HashSet::new();
        for history in ctx
            .admit_iter(&self.feature_histories, "index SLDPRT stored features")
            .map_err(cadmpeg_core::CodecError::from)?
        {
            for feature in ctx
                .admit_iter(&history.features, "index SLDPRT stored features")
                .map_err(cadmpeg_core::CodecError::from)?
            {
                feature_ids_reservation.with_storage(|| {
                    ctx.insert_hash_set(
                        &mut feature_ids,
                        feature.id.as_str(),
                        "index SLDPRT stored features",
                    )
                })?;
            }
        }
        for lane in ctx
            .admit_iter(&self.feature_input_lanes, "scan SLDPRT store values")
            .map_err(cadmpeg_core::CodecError::from)?
        {
            let lookup = ScalarLookup::new(ctx, &lane.scalars, &lane.names)?;
            let (references_by_id, _references_by_id_reservation) = ctx.collect_scoped_string_map(
                lane.references.len(),
                lane.references
                    .iter()
                    .map(|record| (record.id.as_str(), record)),
                "index SLDPRT stored references",
            )?;
            // Filled in reverse so the first class with an identity answers.
            let (classes_by_id, _classes_by_id_reservation) = ctx.collect_scoped_string_map(
                lane.classes.len(),
                ctx.admit_iter(&lane.classes, "index SLDPRT stored classes")
                    .map_err(cadmpeg_core::CodecError::from)?
                    .rev()
                    .map(|record| (record.id.as_str(), record)),
                "index SLDPRT stored classes",
            )?;
            let scalar = |id: &str| {
                lookup
                    .scalar(ctx, id)
                    .map(|scalar| scalar.map(|(_, scalar)| scalar))
            };
            if let Some(record) = ctx.find_by(
                &lane.classes,
                |record| {
                    Ok(!ctx.equal(
                        record.parent.as_str(),
                        lane.id.as_str(),
                        "validate SLDPRT stored records",
                    )?)
                },
                "validate SLDPRT stored records",
            )? {
                return Err(cadmpeg_ir::NativeConvertError::InvalidOwner(
                    ctx.format_retained(
                        format_args!(
                            "feature-input class {} references {} instead of {}",
                            record.id, record.parent, lane.id
                        ),
                        "format SLDPRT native validation error",
                    )?,
                ));
            }
            if let Some(record) = ctx.find_by(
                &lane.names,
                |record| {
                    Ok(!ctx.equal(
                        record.parent.as_str(),
                        lane.id.as_str(),
                        "validate SLDPRT stored records",
                    )?)
                },
                "validate SLDPRT stored records",
            )? {
                return Err(cadmpeg_ir::NativeConvertError::InvalidOwner(
                    ctx.format_retained(
                        format_args!(
                            "feature-input name {} references {} instead of {}",
                            record.id, record.parent, lane.id
                        ),
                        "format SLDPRT native validation error",
                    )?,
                ));
            }
            if let Some(record) = ctx.find_by(
                &lane.scalars,
                |record| {
                    Ok(!ctx.equal(
                        record.parent.as_str(),
                        lane.id.as_str(),
                        "validate SLDPRT stored records",
                    )?)
                },
                "validate SLDPRT stored records",
            )? {
                return Err(cadmpeg_ir::NativeConvertError::InvalidOwner(
                    ctx.format_retained(
                        format_args!(
                            "feature-input scalar {} references {} instead of {}",
                            record.id, record.parent, lane.id
                        ),
                        "format SLDPRT native validation error",
                    )?,
                ));
            }
            for record in ctx
                .admit_iter(&lane.body_selections, "scan SLDPRT store values")
                .map_err(cadmpeg_core::CodecError::from)?
            {
                let invalid = !ctx.equal(
                    record.parent.as_str(),
                    lane.id.as_str(),
                    "validate SLDPRT stored records",
                )? || lookup
                    .name_value(ctx, record.object_name_ref.as_str())?
                    .is_none()
                    || !ctx.contains_hash_set(
                        &(feature_ids),
                        record.feature_ref.as_str(),
                        "test SLDPRT hashed identity",
                    )?
                    || record.local_body_ids.is_empty();
                if invalid {
                    return Err(cadmpeg_ir::NativeConvertError::InvalidOwner(
                        ctx.format_retained(
                            format_args!(
                                "feature-input body selection {} has inconsistent ownership",
                                record.id
                            ),
                            "format SLDPRT native validation error",
                        )?,
                    ));
                }
                let invalid = body_state_ids_disagree_with_payload(ctx, lane, record)?
                    || body_selection_disagrees_with_payload(ctx, lane, record)?;
                if invalid {
                    return Err(cadmpeg_ir::NativeConvertError::InvalidOwner(
                        ctx.format_retained(
                            format_args!(
                                "feature-input body selection {} has inconsistent ownership",
                                record.id
                            ),
                            "format SLDPRT native validation error",
                        )?,
                    ));
                }
            }
            // Selections resolve producers against the history features with
            // this lane's object sources, built only when the lane has any.
            let (lane_features, _lane_features_reservation) =
                if lane.edge_selections.is_empty() && lane.surface_selections.is_empty() {
                    (
                        Vec::new(),
                        ctx.reserve_scoped(0, "validate SLDPRT store lane features")?,
                    )
                } else {
                    lane_feature_context(ctx, &self.feature_histories, lane)?
                };
            for record in ctx
                .admit_iter(&lane.edge_selections, "scan SLDPRT store values")
                .map_err(cadmpeg_core::CodecError::from)?
            {
                let invalid = !ctx.equal(
                    record.parent.as_str(),
                    lane.id.as_str(),
                    "validate SLDPRT stored records",
                )? || lookup
                    .name_value(ctx, record.object_name_ref.as_str())?
                    .is_none()
                    || !ctx.contains_hash_set(
                        &(feature_ids),
                        record.feature_ref.as_str(),
                        "test SLDPRT hashed identity",
                    )?
                    || record.local_edge_ids.is_empty();
                if invalid
                    || edge_selection_disagrees_with_payload(ctx, lane, record, &lane_features)?
                {
                    return Err(cadmpeg_ir::NativeConvertError::InvalidOwner(
                        ctx.format_retained(
                            format_args!(
                                "feature-input edge selection {} has inconsistent ownership",
                                record.id
                            ),
                            "format SLDPRT native validation error",
                        )?,
                    ));
                }
            }
            for record in ctx
                .admit_iter(&lane.surface_selections, "scan SLDPRT store values")
                .map_err(cadmpeg_core::CodecError::from)?
            {
                let invalid = !ctx.equal(
                    record.parent.as_str(),
                    lane.id.as_str(),
                    "validate SLDPRT stored records",
                )? || lookup
                    .name_value(ctx, record.object_name_ref.as_str())?
                    .is_none()
                    || !ctx.contains_hash_set(
                        &(feature_ids),
                        record.feature_ref.as_str(),
                        "test SLDPRT hashed identity",
                    )?
                    || record.components.is_empty();
                if invalid
                    || surface_selection_disagrees_with_payload(ctx, lane, record, &lane_features)?
                {
                    return Err(cadmpeg_ir::NativeConvertError::InvalidOwner(
                        ctx.format_retained(
                            format_args!(
                                "feature-input surface selection {} has inconsistent ownership",
                                record.id
                            ),
                            "format SLDPRT native validation error",
                        )?,
                    ));
                }
            }
            if let Some(record) = ctx.find_by(
                &lane.scalars,
                |record| {
                    let record = *record;
                    Ok({
                        match record.feature_ref.as_deref() {
                            Some(feature) => !ctx.contains_hash_set(
                                &(feature_ids),
                                feature,
                                "test SLDPRT hashed identity",
                            )?,
                            None => false,
                        }
                    })
                },
                "validate SLDPRT stored records",
            )? {
                return Err(cadmpeg_ir::NativeConvertError::InvalidOwner(
                    ctx.format_retained(
                        format_args!(
                            "feature-input scalar {} references missing feature {}",
                            record.id,
                            record.feature_ref.as_deref().unwrap_or_default()
                        ),
                        "format SLDPRT native validation error",
                    )?,
                ));
            }
            if let Some(record) = ctx.find_by(
                &lane.references,
                |record| {
                    Ok(!ctx.equal(
                        record.parent.as_str(),
                        lane.id.as_str(),
                        "validate SLDPRT stored records",
                    )?)
                },
                "validate SLDPRT stored records",
            )? {
                return Err(cadmpeg_ir::NativeConvertError::InvalidOwner(
                    ctx.format_retained(
                        format_args!(
                            "feature-input reference {} references {} instead of {}",
                            record.id, record.parent, lane.id
                        ),
                        "format SLDPRT native validation error",
                    )?,
                ));
            }
            if let Some(record) = ctx.find_by(
                &lane.relation_bindings,
                |record| {
                    let record = *record;
                    Ok({
                        !ctx.equal(
                            record.parent.as_str(),
                            lane.id.as_str(),
                            "validate SLDPRT stored records",
                        )? || ctx
                            .get_hash_map(
                                &classes_by_id,
                                record.class_ref.as_str(),
                                "test SLDPRT hashed identity",
                            )?
                            .is_none()
                            || scalar(&record.scalar_ref)?.is_none()
                            || match record.feature_ref.as_deref() {
                                Some(feature) => !ctx.contains_hash_set(
                                    &(feature_ids),
                                    feature,
                                    "test SLDPRT hashed identity",
                                )?,
                                None => false,
                            }
                    })
                },
                "validate SLDPRT stored records",
            )? {
                return Err(cadmpeg_ir::NativeConvertError::InvalidOwner(
                    ctx.format_retained(
                        format_args!(
                            "feature-input relation binding {} has inconsistent ownership",
                            record.id
                        ),
                        "format SLDPRT native validation error",
                    )?,
                ));
            }
            if let Some(record) = ctx.find_by(
                &lane.relation_instances,
                |record| {
                    let record = *record;
                    Ok(!ctx.equal(
                        record.parent.as_str(),
                        lane.id.as_str(),
                        "validate SLDPRT stored records",
                    )? || ctx
                        .get_hash_map(
                            &classes_by_id,
                            record.class_ref.as_str(),
                            "test SLDPRT hashed identity",
                        )?
                        .is_none()
                        || !ctx.contains_hash_set(
                            &(feature_ids),
                            record.feature_ref.as_str(),
                            "test SLDPRT hashed identity",
                        )?
                        || !relation_instance_shape_valid(ctx, record, &classes_by_id, &lookup)?
                        || match record.parameter_scalar_ref() {
                            Some(id) => scalar(id)?.is_none_or(|scalar| {
                                scalar.role != crate::records::FeatureInputScalarRole::Driving
                            }),
                            None => false,
                        }
                        || match record.display_scalar_ref() {
                            Some(id) => scalar(id)?.is_none_or(|scalar| {
                                scalar.role != crate::records::FeatureInputScalarRole::Display
                            }),
                            None => false,
                        })
                },
                "validate SLDPRT stored records",
            )? {
                return Err(cadmpeg_ir::NativeConvertError::InvalidOwner(
                    ctx.format_retained(
                        format_args!(
                            "feature-input relation instance {} has inconsistent ownership",
                            record.id
                        ),
                        "format SLDPRT native validation error",
                    )?,
                ));
            }
            if let Some(record) = ctx.find_by(
                &lane.relation_bindings,
                |record| {
                    let record = *record;
                    Ok(match scalar(&record.scalar_ref)? {
                        Some(scalar) => !ctx.equal(
                            &scalar.feature_ref,
                            &record.feature_ref,
                            "validate SLDPRT stored records",
                        )?,
                        None => false,
                    })
                },
                "validate SLDPRT stored records",
            )? {
                return Err(cadmpeg_ir::NativeConvertError::InvalidOwner(
                    ctx.format_retained(
                        format_args!(
                            "feature-input relation binding {} disagrees with its scalar owner",
                            record.id
                        ),
                        "format SLDPRT native validation error",
                    )?,
                ));
            }
            if let Some(record) = ctx.find_by(
                &lane.scalars,
                |record| {
                    let record = *record;
                    Ok(lookup.name_value(ctx, record.name.as_str())?.is_none())
                },
                "validate SLDPRT stored records",
            )? {
                return Err(cadmpeg_ir::NativeConvertError::InvalidOwner(
                    ctx.format_retained(
                        format_args!(
                            "feature-input scalar {} references name {}",
                            record.id, record.name
                        ),
                        "format SLDPRT native validation error",
                    )?,
                ));
            }
            let (sketch_entities, _sketch_entities_reservation) = ctx.collect_scoped_string_map(
                lane.sketch_entities.len(),
                lane.sketch_entities
                    .iter()
                    .map(|record| (record.id(), record)),
                "index SLDPRT stored sketch entities",
            )?;
            // Each scalar resolves its operands among its own feature's sketch
            // entities, in source order.
            let mut entity_groups_storage =
                ctx.reserve_scoped(0, "group SLDPRT stored sketch entities")?;
            let mut entities_by_feature = std::collections::HashMap::<
                Option<&str>,
                Vec<&crate::records::SketchInputEntity>,
            >::new();
            for entity in ctx
                .admit_iter(&lane.sketch_entities, "group SLDPRT stored sketch entities")
                .map_err(cadmpeg_core::CodecError::from)?
            {
                entity_groups_storage.with_storage(|| {
                    ctx.push_hash_group(
                        &mut entities_by_feature,
                        entity.feature_ref.as_deref(),
                        entity,
                        "group SLDPRT stored sketch entities",
                        "group SLDPRT stored sketch entities",
                    )
                })?;
            }
            for scalar in ctx
                .admit_iter(&lane.scalars, "scan SLDPRT store values")
                .map_err(cadmpeg_core::CodecError::from)?
            {
                let candidates = ctx
                    .get_hash_map(
                        &entities_by_feature,
                        &scalar.feature_ref.as_deref(),
                        "group SLDPRT stored sketch entities",
                    )?
                    .map_or(&[][..], Vec::as_slice);
                let resolved_operands =
                    crate::resolved_features::operands::resolve_scalar_operand_markers(
                        ctx,
                        candidates,
                        &scalar.operands,
                    )?;
                for (operand, resolved) in ctx
                    .admit_iter(&scalar.operands, "scan SLDPRT scalar operands")
                    .map_err(cadmpeg_core::CodecError::from)?
                    .zip(
                        ctx.admit_iter(&resolved_operands[..], "scan SLDPRT resolved operands")
                            .map_err(cadmpeg_core::CodecError::from)?,
                    )
                {
                    let Some(reference) = ctx.get_hash_map(
                        &(references_by_id),
                        operand.reference_ref.as_str(),
                        "look up SLDPRT hash key",
                    )?
                    else {
                        return Err(cadmpeg_ir::NativeConvertError::InvalidOwner(
                            ctx.format_retained(
                                format_args!(
                                    "feature-input scalar {} references missing cell {}",
                                    scalar.id, operand.reference_ref
                                ),
                                "format SLDPRT native validation error",
                            )?,
                        ));
                    };
                    if reference.offset != operand.offset
                        || reference.kind != operand.kind
                        || reference.object_index != operand.entity_index
                    {
                        return Err(cadmpeg_ir::NativeConvertError::InvalidOwner(
                            ctx.format_retained(
                                format_args!(
                                    "feature-input scalar {} has inconsistent cell {}",
                                    scalar.id, operand.reference_ref
                                ),
                                "format SLDPRT native validation error",
                            )?,
                        ));
                    }
                    if let Some(entity_ref) = operand.entity_ref.as_deref() {
                        let Some(target) = ctx.get_hash_map(
                            &(sketch_entities),
                            entity_ref,
                            "look up SLDPRT hash key",
                        )?
                        else {
                            return Err(cadmpeg_ir::NativeConvertError::InvalidOwner(
                                ctx.format_retained(
                                    format_args!(
                                    "feature-input scalar {} references missing sketch marker {}",
                                    scalar.id, entity_ref
                                ),
                                    "format SLDPRT native validation error",
                                )?,
                            ));
                        };
                        let agrees = match resolved {
                            Some(resolved) => ctx.equal(
                                resolved.id(),
                                target.id(),
                                "validate SLDPRT stored records",
                            )?,
                            None => false,
                        };
                        if !agrees {
                            return Err(cadmpeg_ir::NativeConvertError::InvalidOwner(
                                ctx.format_retained(
                                    format_args!(
                                        "feature-input scalar {} has inconsistent sketch marker {}",
                                        scalar.id, entity_ref
                                    ),
                                    "format SLDPRT native validation error",
                                )?,
                            ));
                        }
                    }
                }
            }
            if let Some(record) = ctx.find_by(
                &lane.sketch_entities,
                |record| {
                    let record = *record;
                    Ok({
                        !ctx.equal(
                            record.parent(),
                            lane.id.as_str(),
                            "validate SLDPRT stored records",
                        )? || match record.feature_ref.as_deref() {
                            Some(feature) => !ctx.contains_hash_set(
                                &(feature_ids),
                                feature,
                                "test SLDPRT hashed identity",
                            )?,
                            None => false,
                        }
                    })
                },
                "validate SLDPRT stored records",
            )? {
                return Err(cadmpeg_ir::NativeConvertError::InvalidOwner(
                    ctx.format_retained(
                        format_args!(
                            "sketch input entity {} has inconsistent lane or feature ownership",
                            record.id()
                        ),
                        "format SLDPRT native validation error",
                    )?,
                ));
            }
            for record in ctx
                .admit_iter(&lane.sketch_entities, "scan SLDPRT store values")
                .map_err(cadmpeg_core::CodecError::from)?
            {
                for link in ctx
                    .admit_iter(record.links(), "scan SLDPRT sketch input links")
                    .map_err(cadmpeg_core::CodecError::from)?
                {
                    let Some(target) = ctx.get_hash_map(
                        &(sketch_entities),
                        link.entity_ref.as_str(),
                        "look up SLDPRT hash key",
                    )?
                    else {
                        return Err(cadmpeg_ir::NativeConvertError::InvalidOwner(
                            ctx.format_retained(
                                format_args!(
                                "sketch input entity {} references missing local-link target {}",
                                record.id(),
                                link.entity_ref
                            ),
                                "format SLDPRT native validation error",
                            )?,
                        ));
                    };
                    if target.feature_ref != record.feature_ref
                        || target.local_id() != Some(u32::from(link.local_id))
                    {
                        return Err(cadmpeg_ir::NativeConvertError::InvalidOwner(
                            ctx.format_retained(
                                format_args!(
                                    "sketch input entity {} has inconsistent local-link target {}",
                                    record.id(),
                                    link.entity_ref
                                ),
                                "format SLDPRT native validation error",
                            )?,
                        ));
                    }
                }
            }
        }
        let (mut expected_histories, _expected_histories_reservation) =
            ctx.with_scoped_storage("validate SLDPRT expected histories", || {
                ctx.try_collect_retained_with(
                    self.feature_histories.iter(),
                    "validate SLDPRT expected histories",
                    |record| record.clone_charged(ctx, "validate SLDPRT expected histories"),
                )
            })?;
        let (history_lanes, _history_lanes_reservation) =
            ctx.with_scoped_storage("validate SLDPRT history lanes", || {
                ctx.try_collect_retained_with(
                    ctx.admit_iter(&self.feature_input_lanes[..], "scan SLDPRT store values")
                        .map_err(cadmpeg_core::CodecError::from)?
                        .filter(|lane| {
                            !crate::resolved_features::assembly::is_supplemental_config_lane(lane)
                        }),
                    "validate SLDPRT history lanes",
                    |record| record.clone_charged(ctx, "validate SLDPRT history lanes"),
                )
            })?;
        crate::resolved_features::classes::bind_history_classes(
            ctx,
            &mut expected_histories,
            &history_lanes,
        )?;
        if ctx.any_by(
            self.feature_histories.iter().zip(&expected_histories),
            |(history, expected)| {
                ctx.any_by(
                    history.features.iter().zip(&expected.features),
                    |(feature, expected)| {
                        Ok(!ctx.equal(&feature.input_class, &expected.input_class, CLASSES)?)
                    },
                    CLASSES,
                )
            },
            CLASSES,
        )? {
            return Err(cadmpeg_ir::NativeConvertError::InvalidOwner(
                "history feature classes do not match the feature-input index".into(),
            ));
        }
        SLDPRT_CATALOGUE.emit_all(ctx, self, namespace)?;
        Ok(())
    }
}

/// Copies of every history feature with one lane's object sources resolved,
/// held in scoped storage.
fn lane_feature_context<'ctx>(
    ctx: &'ctx DecodeContext<'_>,
    histories: &[FeatureHistory],
    lane: &FeatureInputLane,
) -> Result<
    (
        Vec<crate::records::Feature>,
        cadmpeg_core::decode::ScopedReservation<'ctx>,
    ),
    cadmpeg_core::CodecError,
> {
    const OPERATION: &str = "copy SLDPRT lane feature context";
    let mut reservation = ctx.reserve_scoped(0, OPERATION)?;
    let mut features = Vec::new();
    for history in ctx.admit_iter(histories, OPERATION)? {
        for feature in ctx.admit_iter(&history.features, OPERATION)? {
            reservation.with_storage(|| {
                let copy = feature.clone_charged(ctx, OPERATION)?;
                ctx.push_vec(&mut features, copy, OPERATION)
            })?;
        }
    }
    reservation.with_storage(|| {
        crate::resolved_features::selections::enrich_feature_object_sources(
            ctx,
            &mut features,
            std::slice::from_ref(lane),
        )
    })?;
    Ok((features, reservation))
}

/// Identity lookups over feature-input scalars and names. The first record
/// with an identity answers.
struct ScalarLookup<'a, 'ctx> {
    /// Each scalar with its position among the indexed scalars.
    scalars: std::collections::HashMap<&'a str, (usize, &'a FeatureInputScalar)>,
    name_values: std::collections::HashMap<&'a str, &'a str>,
    _storage: [cadmpeg_core::decode::ScopedReservation<'ctx>; 2],
}

impl<'a, 'ctx> ScalarLookup<'a, 'ctx> {
    const OPERATION: &'static str = "index SLDPRT scalars and names";

    fn new(
        ctx: &'ctx DecodeContext<'_>,
        scalars: &'a [FeatureInputScalar],
        names: &'a [FeatureInputName],
    ) -> Result<Self, cadmpeg_core::CodecError> {
        // Filled in reverse so the first record with an identity is the last write.
        let (scalar_index, scalar_storage) = ctx.collect_scoped_string_map(
            scalars.len(),
            ctx.admit_iter(scalars, Self::OPERATION)?
                .enumerate()
                .rev()
                .map(|(position, scalar)| (scalar.id.as_str(), (position, scalar))),
            Self::OPERATION,
        )?;
        let (name_values, name_storage) = ctx.collect_scoped_string_map(
            names.len(),
            ctx.admit_iter(names, Self::OPERATION)?
                .rev()
                .map(|name| (name.id.as_str(), name.value.as_str())),
            Self::OPERATION,
        )?;
        Ok(Self {
            scalars: scalar_index,
            name_values,
            _storage: [scalar_storage, name_storage],
        })
    }

    fn scalar(
        &self,
        ctx: &DecodeContext<'_>,
        id: &str,
    ) -> Result<Option<(usize, &'a FeatureInputScalar)>, cadmpeg_core::CodecError> {
        Ok(ctx
            .get_hash_map(&self.scalars, id, Self::OPERATION)?
            .copied())
    }

    fn name_value(
        &self,
        ctx: &DecodeContext<'_>,
        id: &str,
    ) -> Result<Option<&'a str>, cadmpeg_core::CodecError> {
        Ok(ctx
            .get_hash_map(&self.name_values, id, Self::OPERATION)?
            .copied())
    }
}

/// The records a relation instance must resolve against.
struct RelationInstanceOwners<'a, 'ctx> {
    classes: &'a std::collections::HashMap<&'a str, &'a FeatureInputClass>,
    features: &'a std::collections::HashSet<&'a str>,
    lookup: &'a ScalarLookup<'a, 'ctx>,
}

impl RelationInstanceOwners<'_, '_> {
    /// Whether a relation instance names a missing or mismatched class,
    /// feature or scalar, repeats a scalar, or states a scalar role its
    /// scalar does not carry.
    fn unresolved(
        &self,
        ctx: &DecodeContext<'_>,
        record: &FeatureInputRelationInstance,
    ) -> Result<bool, cadmpeg_core::CodecError> {
        const OPERATION: &str = "validate SLDPRT relation instance owners";
        let Some(class) = ctx.get_hash_map(self.classes, record.class_ref.as_str(), OPERATION)?
        else {
            return Ok(true);
        };
        if !matches!(
            crate::classification::native_object_class(&class.name),
            crate::classification::NativeClassKind::SketchRelation(family)
                if family == record.family
        ) || !ctx.contains_hash_set(self.features, record.feature_ref.as_str(), OPERATION)?
            || record.scalar_refs().is_empty()
            || (record.scalar_refs().len() > 3
                && !repeated_circle_display_shape_valid(ctx, record, self.lookup)?)
        {
            return Ok(true);
        }
        let mut workspace = ctx.reserve_scoped(0, OPERATION)?;
        let mut seen = std::collections::HashSet::new();
        for scalar in ctx.admit_iter(record.scalar_refs(), OPERATION)? {
            if !workspace
                .with_storage(|| ctx.insert_hash_set(&mut seen, scalar.as_str(), OPERATION))?
                || self.lookup.scalar(ctx, scalar)?.is_none()
            {
                return Ok(true);
            }
        }
        let role_mismatch = |id: Option<&str>, role| {
            Ok::<_, cadmpeg_core::CodecError>(match id {
                Some(id) => self
                    .lookup
                    .scalar(ctx, id)?
                    .is_none_or(|(_, scalar)| scalar.role != role),
                None => false,
            })
        };
        Ok(role_mismatch(
            record.parameter_scalar_ref(),
            crate::records::FeatureInputScalarRole::Driving,
        )? || role_mismatch(
            record.display_scalar_ref(),
            crate::records::FeatureInputScalarRole::Display,
        )?)
    }
}

/// Move each record into the owners it names, in source order, then order each
/// owner's records by ordinal.
///
/// Owner identities need not be unique: every owner with a record's owner
/// identity receives the record, the last by move and the others by copy.
/// A record whose owner is unknown was refused before this call.
fn attach_to_owners<T: CloneCharged>(
    ctx: &DecodeContext<'_>,
    owner_ids: &[&str],
    records: Vec<T>,
    owner: impl Fn(&T) -> &str,
    ordinal: impl Fn(&T) -> u32,
) -> Result<Vec<Vec<T>>, cadmpeg_core::CodecError> {
    const OPERATION: &str = "attach SLDPRT native records to owners";
    let mut workspace = ctx.reserve_scoped(0, OPERATION)?;
    let mut positions = std::collections::HashMap::<&str, Vec<usize>>::new();
    for (position, id) in ctx.admit_iter(owner_ids, OPERATION)?.enumerate() {
        workspace.with_storage(|| {
            ctx.push_hash_group(&mut positions, *id, position, OPERATION, OPERATION)
        })?;
    }
    let mut owned = ctx.collect_indexed_vec(owner_ids.len(), OPERATION, |_| Ok(Vec::new()))?;
    for record in ctx.admit_iter(records, OPERATION)? {
        let Some((&last, others)) = ctx
            .get_hash_map(&positions, owner(&record), OPERATION)?
            .and_then(|targets| targets.split_last())
        else {
            continue;
        };
        for &position in ctx.admit_iter(others, OPERATION)? {
            let copy = record.clone_charged(ctx, OPERATION)?;
            ctx.push_vec(&mut owned[position], copy, OPERATION)?;
        }
        ctx.push_vec(&mut owned[last], record, OPERATION)?;
    }
    for records in ctx.admit_iter(&mut owned, OPERATION)? {
        ctx.stable_sort_by_key(records, &ordinal, Ord::cmp, "sort SLDPRT native records")?;
    }
    Ok(owned)
}

/// The first ordinal two adjacent records of an ordinal-sorted sequence share.
fn repeated_ordinal<T>(
    ctx: &DecodeContext<'_>,
    records: &[T],
    ordinal: impl Fn(&T) -> u32,
) -> Result<Option<u32>, cadmpeg_core::CodecError> {
    Ok(ctx
        .find_by(
            records.windows(2),
            |pair| Ok(ordinal(&pair[0]) == ordinal(&pair[1])),
            "check SLDPRT native record ordinals",
        )?
        .map(|pair| ordinal(&pair[1])))
}

/// Check a lane's edge and surface selections against its payload, with the
/// history features enriched by this lane's object sources.
fn validate_lane_selections(
    ctx: &DecodeContext<'_>,
    lane: &FeatureInputLane,
    features: &[crate::records::Feature],
) -> Result<(), cadmpeg_ir::NativeConvertError> {
    const OPERATION: &str = "validate SLDPRT lane selections";
    let (mut lane_features, _lane_features_reservation) =
        ctx.with_scoped_storage(OPERATION, || {
            ctx.try_collect_retained_with(
                ctx.admit_iter(features, OPERATION)?,
                OPERATION,
                |record| record.clone_charged(ctx, OPERATION),
            )
        })?;
    crate::resolved_features::selections::enrich_feature_object_sources(
        ctx,
        &mut lane_features,
        std::slice::from_ref(lane),
    )?;
    let (features_by_id, _features_by_id_reservation) = ctx.collect_scoped_string_map(
        lane_features.len(),
        // Filled in reverse so the first feature with an identity answers.
        ctx.admit_iter(&lane_features, OPERATION)
            .map_err(cadmpeg_core::CodecError::from)?
            .rev()
            .map(|feature| (feature.id.as_str(), feature)),
        OPERATION,
    )?;
    let disagrees = |id: &str, kind: &str| {
        ctx.format_retained(
            format_args!("feature-input {kind} selection {id} disagrees with its payload"),
            "format SLDPRT native validation error",
        )
        .map(cadmpeg_ir::NativeConvertError::InvalidOwner)
    };
    for record in ctx
        .admit_iter(&lane.edge_selections, OPERATION)
        .map_err(cadmpeg_core::CodecError::from)?
    {
        if edge_selection_disagrees_with_payload(ctx, lane, record, &lane_features)? {
            return Err(disagrees(&record.id, "edge")?);
        }
        let references = match usize::try_from(record.offset) {
            Ok(offset) => {
                let feature_kind = ctx
                    .get_hash_map(&features_by_id, record.feature_ref.as_str(), OPERATION)?
                    .map_or("", |feature| feature.kind.as_str());
                crate::resolved_features::selections::compact_edge_reference_list_for_feature(
                    ctx,
                    &lane.native_payload,
                    offset,
                    feature_kind,
                )?
            }
            Err(_) => None,
        };
        if !ctx.equal(
            &references.unwrap_or_default(),
            &record.references,
            OPERATION,
        )? {
            return Err(disagrees(&record.id, "edge")?);
        }
    }
    for record in ctx
        .admit_iter(&lane.surface_selections, OPERATION)
        .map_err(cadmpeg_core::CodecError::from)?
    {
        if surface_selection_disagrees_with_payload(ctx, lane, record, &lane_features)? {
            return Err(disagrees(&record.id, "surface")?);
        }
    }
    Ok(())
}

/// The first record whose owner, when it names one, is not among `owners`.
fn first_orphan<'r, T>(
    ctx: &DecodeContext<'_>,
    records: &'r [T],
    owners: &std::collections::HashSet<&str>,
    owner: impl Fn(&T) -> Option<&str>,
) -> Result<Option<&'r T>, cadmpeg_core::CodecError> {
    const OPERATION: &str = "validate SLDPRT native owners";
    ctx.find_by(
        records,
        |record| {
            Ok(match owner(record) {
                Some(owner) => !ctx.contains_hash_set(owners, owner, OPERATION)?,
                None => false,
            })
        },
        OPERATION,
    )
}

const PAYLOAD_AGREEMENT: &str = "compare SLDPRT native records with their payload";

fn generated_surface_identities_disagree_with_payload(
    ctx: &DecodeContext<'_>,
    lane: &FeatureInputLane,
) -> Result<bool, cadmpeg_ir::NativeConvertError> {
    let regenerated =
        crate::resolved_features::selections::generated_surface_identities(ctx, lane)?;
    Ok(!ctx.equal(
        &lane.generated_surface_identities,
        &regenerated,
        PAYLOAD_AGREEMENT,
    )?)
}

fn body_state_ids_disagree_with_payload(
    ctx: &DecodeContext<'_>,
    lane: &FeatureInputLane,
    record: &FeatureInputBodySelection,
) -> Result<bool, cadmpeg_ir::NativeConvertError> {
    let ids = crate::resolved_features::selections::compact_body_state_ids_for_selection(
        ctx, lane, record,
    )?;
    Ok(!ctx.equal(&ids, &record.body_state_ids, PAYLOAD_AGREEMENT)?)
}

/// Whether a value read back from the payload differs from the stored one;
/// an unreadable value differs from every stored value.
fn differs<T: cadmpeg_core::decode::cost::DecodeCost + PartialEq>(
    ctx: &DecodeContext<'_>,
    read: Option<&T>,
    stored: &T,
) -> Result<bool, cadmpeg_core::CodecError> {
    Ok(match read {
        Some(read) => !ctx.equal(read, stored, PAYLOAD_AGREEMENT)?,
        None => true,
    })
}

/// `true` when a body selection disagrees with the compact selection in its lane payload.
fn body_selection_disagrees_with_payload(
    ctx: &DecodeContext<'_>,
    lane: &FeatureInputLane,
    record: &FeatureInputBodySelection,
) -> Result<bool, cadmpeg_ir::NativeConvertError> {
    let selection = match usize::try_from(record.offset) {
        Ok(offset) => crate::resolved_features::selections::compact_body_selection_at(
            ctx,
            &lane.native_payload,
            offset,
        )?,
        Err(_) => None,
    };
    Ok(differs(ctx, selection.as_ref(), &record.local_body_ids)?
        || crate::resolved_features::selections::compact_body_retention_mode_for_selection(
            ctx, lane, record,
        )? != record.mode)
}

/// `true` when an edge selection disagrees with the compact selection in its lane payload.
///
/// `edge_features` are the history features enriched with this lane's object sources.
fn edge_selection_disagrees_with_payload(
    ctx: &DecodeContext<'_>,
    lane: &FeatureInputLane,
    record: &FeatureInputEdgeSelection,
    edge_features: &[crate::records::Feature],
) -> Result<bool, cadmpeg_ir::NativeConvertError> {
    use crate::resolved_features::selections::{
        compact_edge_component_path_at, compact_edge_owner_feature_at,
        compact_edge_producer_features_at, compact_edge_selection_at,
    };
    let Ok(offset) = usize::try_from(record.offset) else {
        // An offset no index can name reads no selection from the payload.
        return Ok(true);
    };
    let payload = &lane.native_payload;
    Ok(differs(
        ctx,
        compact_edge_selection_at(ctx, payload, offset)?.as_ref(),
        &record.local_edge_ids,
    )? || !ctx.equal(
        &compact_edge_component_path_at(ctx, payload, offset)?.unwrap_or_default(),
        &record.components,
        PAYLOAD_AGREEMENT,
    )? || !ctx.equal(
        &compact_edge_producer_features_at(
            ctx,
            payload,
            offset,
            &record.components,
            edge_features,
            &record.feature_ref,
        )?,
        &record.producer_feature_refs,
        PAYLOAD_AGREEMENT,
    )? || !ctx.equal(
        &compact_edge_owner_feature_at(
            ctx,
            payload,
            offset,
            &record.components,
            edge_features,
            &record.feature_ref,
        )?,
        &record.terminal_feature_ref,
        PAYLOAD_AGREEMENT,
    )?)
}

/// `true` when a surface selection disagrees with the compact reference in its lane payload.
///
/// `surface_features` are the history features enriched with this lane's object sources.
fn surface_selection_disagrees_with_payload(
    ctx: &DecodeContext<'_>,
    lane: &FeatureInputLane,
    record: &FeatureInputSurfaceSelection,
    surface_features: &[crate::records::Feature],
) -> Result<bool, cadmpeg_ir::NativeConvertError> {
    // An offset no index can name names no byte of the payload in memory, so
    // the selection states nothing the payload agrees with.
    let Ok(offset) = usize::try_from(record.offset) else {
        return Ok(true);
    };
    if !crate::resolved_features::selections::surface_reference_matches_at(
        ctx,
        &lane.native_payload,
        offset,
        &record.components,
    )? {
        return Ok(true);
    }
    Ok(!ctx.equal(
        &crate::resolved_features::component_paths::surface_selection_producer_features(
            ctx,
            &record.components,
            record.terminal_feature_ref.as_deref(),
            surface_features,
        )?,
        &record.producer_feature_refs,
        PAYLOAD_AGREEMENT,
    )? || !ctx.equal(
        &crate::resolved_features::selections::surface_selection_terminal_feature_at(
            ctx,
            &lane.native_payload,
            offset,
            &record.components,
            surface_features,
        )?,
        &record.terminal_feature_ref,
        PAYLOAD_AGREEMENT,
    )?)
}

fn relation_instance_shape_valid(
    ctx: &DecodeContext<'_>,
    record: &FeatureInputRelationInstance,
    classes: &std::collections::HashMap<&str, &FeatureInputClass>,
    lookup: &ScalarLookup<'_, '_>,
) -> Result<bool, cadmpeg_core::CodecError> {
    const OPERATION: &str = "validate SLDPRT relation instance shape";
    let Some((first_ref, _)) = record.scalar_refs().split_first() else {
        return Ok(false);
    };
    let Some(class) = ctx.get_hash_map(classes, record.class_ref.as_str(), OPERATION)? else {
        return Ok(false);
    };
    if !matches!(
        crate::classification::native_object_class(&class.name),
        crate::classification::NativeClassKind::SketchRelation(family)
            if family == record.family
    ) {
        return Ok(false);
    }
    for scalar_ref in ctx.admit_iter(record.scalar_refs(), OPERATION)? {
        let Some((_, scalar)) = lookup.scalar(ctx, scalar_ref)? else {
            return Ok(false);
        };
        let owned = match scalar.feature_ref.as_deref() {
            Some(feature) => ctx.equal(feature, record.feature_ref.as_str(), OPERATION)?,
            None => false,
        };
        if !owned {
            return Ok(false);
        }
    }
    let repeated_circle_display = repeated_circle_display_shape_valid(ctx, record, lookup)?;
    if record.scalar_refs().len() > 3 && !repeated_circle_display {
        return Ok(false);
    }
    let scalar_operands_match = |scalar: &FeatureInputScalar| {
        let all_match = scalar.operands.len() == record.operands.len()
            && ctx.all_by(
                scalar.operands.iter().zip(&record.operands),
                |(candidate, operand)| {
                    Ok(candidate.kind == operand.kind
                        && candidate.entity_index == operand.entity_index)
                },
                OPERATION,
            )?;
        Ok::<_, cadmpeg_core::CodecError>(
            all_match
                || (record.family == crate::records::FeatureInputRelationFamily::CircleDiameter
                    && matches!(record.operands.as_slice(), [first, _]
                        if matches!(scalar.operands.as_slice(), [candidate]
                            if candidate.kind == first.kind
                                && candidate.entity_index == first.entity_index)))
                || (repeated_circle_display
                    && matches!(record.operands.as_slice(), [first]
                        if matches!(scalar.operands.as_slice(), [candidate]
                            if candidate.kind == first.kind))),
        )
    };
    let Some((_, first)) = lookup.scalar(ctx, first_ref)? else {
        return Ok(false);
    };
    if first.offset != record.offset || !scalar_operands_match(first)? {
        return Ok(false);
    }
    let mut last_operand_position = None;
    let mut detached = None;
    for scalar_ref in ctx.admit_iter(record.scalar_refs(), OPERATION)? {
        let Some((position, scalar)) = lookup.scalar(ctx, scalar_ref)? else {
            return Ok(false);
        };
        if scalar.operands.is_empty() {
            if detached.replace((position, scalar)).is_some() {
                return Ok(false);
            }
        } else {
            if last_operand_position.is_some_and(|previous| position != previous + 1)
                || !scalar_operands_match(scalar)?
            {
                return Ok(false);
            }
            last_operand_position = Some(position);
        }
    }
    let Some(last_operand_position) = last_operand_position else {
        return Ok(false);
    };
    Ok(match detached {
        None => true,
        Some((position, scalar)) => {
            position > last_operand_position
                && match record.parameter_scalar_ref() {
                    Some(parameter) => ctx.equal(parameter, scalar.id.as_str(), OPERATION)?,
                    None => false,
                }
        }
    })
}

/// Whether a circle-diameter relation is a run of display scalars with
/// consecutive ordinals, one name, one operand kind and distinct operand
/// entities.
fn repeated_circle_display_shape_valid(
    ctx: &DecodeContext<'_>,
    record: &FeatureInputRelationInstance,
    lookup: &ScalarLookup<'_, '_>,
) -> Result<bool, cadmpeg_core::CodecError> {
    const OPERATION: &str = "validate SLDPRT repeated circle display";
    if record.family != crate::records::FeatureInputRelationFamily::CircleDiameter
        || record.parameter_scalar_ref().is_some()
        || record.display_scalar_ref().is_some()
        || record.scalar_refs().len() < 2
        || record.operands.len() != 1
    {
        return Ok(false);
    }
    let Some((_, first)) = lookup.scalar(ctx, &record.scalar_refs()[0])? else {
        return Ok(false);
    };
    let Some(first_name) = lookup.name_value(ctx, &first.name)? else {
        return Ok(false);
    };
    let mut workspace = ctx.reserve_scoped(0, OPERATION)?;
    let mut entities = std::collections::HashSet::new();
    let mut previous_ordinal = None;
    for scalar_id in ctx.admit_iter(record.scalar_refs(), OPERATION)? {
        let Some((_, scalar)) = lookup.scalar(ctx, scalar_id)? else {
            return Ok(false);
        };
        if previous_ordinal.is_some_and(|ordinal: u32| {
            scalar.ordinal.checked_sub(ordinal) != Some(1)
                && !(ordinal == u32::MAX && scalar.ordinal == ordinal)
        }) {
            return Ok(false);
        }
        previous_ordinal = Some(scalar.ordinal);
        let [operand] = scalar.operands.as_slice() else {
            return Ok(false);
        };
        if scalar.role != crate::records::FeatureInputScalarRole::Display
            || operand.kind != record.operands[0].kind
        {
            return Ok(false);
        }
        let same_name = match lookup.name_value(ctx, &scalar.name)? {
            Some(name) => ctx.equal(name, first_name, OPERATION)?,
            None => false,
        };
        if !same_name
            || !workspace.with_storage(|| {
                ctx.insert_hash_set(&mut entities, operand.entity_index, OPERATION)
            })?
        {
            return Ok(false);
        }
    }
    Ok(true)
}

#[cfg(test)]
mod tests;
