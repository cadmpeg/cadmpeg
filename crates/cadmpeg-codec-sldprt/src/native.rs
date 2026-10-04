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
        if let Some(record) = configurations
            .iter()
            .find(|record| !history_ids.contains(record.parent.as_str()))
        {
            return Err(cadmpeg_ir::NativeConvertError::InvalidOwner(
                ctx.format_retained(
                    format_args!("configuration {} references {}", record.id, record.parent),
                    "format SLDPRT native validation error",
                )?,
            ));
        }
        if let Some(record) = features
            .iter()
            .find(|record| !history_ids.contains(record.parent.as_str()))
        {
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
        if let Some(record) = entity_wires
            .iter()
            .find(|record| !lane_ids.contains(record.parent.as_str()))
        {
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
        for wire in entity_wires {
            let Some(payload) = lane_payloads.get(wire.parent.as_str()).copied() else {
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
            entities.push(
                crate::records::SketchInputEntity::try_from_wire(wire, payload)
                    .map_err(cadmpeg_ir::NativeConvertError::InvalidOwner)?,
            );
        }
        if let Some(record) = classes
            .iter()
            .find(|record| !lane_ids.contains(record.parent.as_str()))
        {
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
        if let Some(record) = body_selections
            .iter()
            .find(|record| !lane_ids.contains(record.parent.as_str()))
        {
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
        if let Some(record) = edge_selections
            .iter()
            .find(|record| !lane_ids.contains(record.parent.as_str()))
        {
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
        if let Some(record) = surface_selections
            .iter()
            .find(|record| !lane_ids.contains(record.parent.as_str()))
        {
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
        if let Some(record) = generated_surface_identities
            .iter()
            .find(|record| !lane_ids.contains(record.parent.as_str()))
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
        if let Some(record) = names
            .iter()
            .find(|record| !lane_ids.contains(record.parent.as_str()))
        {
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
        if let Some(record) = scalars
            .iter()
            .find(|record| !lane_ids.contains(record.parent.as_str()))
        {
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
        if let Some(record) = scalars.iter().find(|record| {
            record
                .feature_ref
                .as_deref()
                .is_some_and(|feature| !feature_ids.contains(feature))
        }) {
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
        if let Some(record) = references
            .iter()
            .find(|record| !lane_ids.contains(record.parent.as_str()))
        {
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
        if let Some(record) = relation_bindings
            .iter()
            .find(|record| !lane_ids.contains(record.parent.as_str()))
        {
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
        if let Some(record) = relation_instances
            .iter()
            .find(|record| !lane_ids.contains(record.parent.as_str()))
        {
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
        let (name_ids, _name_ids_reservation) = ctx.collect_scoped_string_set(
            names.len(),
            names.iter().map(|record| record.id.as_str()),
            "index SLDPRT feature names",
        )?;
        if let Some(record) = body_selections.iter().find(|record| {
            !name_ids.contains(record.object_name_ref.as_str())
                || !feature_ids.contains(record.feature_ref.as_str())
                || record.local_body_ids.is_empty()
        }) {
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
        if let Some(record) = edge_selections.iter().find(|record| {
            !name_ids.contains(record.object_name_ref.as_str())
                || !feature_ids.contains(record.feature_ref.as_str())
                || record.local_edge_ids.is_empty()
        }) {
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
        if let Some(record) = surface_selections.iter().find(|record| {
            !name_ids.contains(record.object_name_ref.as_str())
                || !feature_ids.contains(record.feature_ref.as_str())
                || record.components.is_empty()
                || record
                    .producer_feature_refs
                    .iter()
                    .any(|producer| !feature_ids.contains(producer.as_str()))
                || record
                    .terminal_feature_ref
                    .as_deref()
                    .is_some_and(|feature| !feature_ids.contains(feature))
                || record.endpoint_selector().is_some_and(|selector| {
                    usize::try_from(record.offset)
                        .ok()
                        .and_then(|offset| offset.checked_sub(4))
                        .and_then(|offset| {
                            lane_payloads
                                .get(record.parent.as_str())
                                .and_then(|payload| View::u32_le_at(payload, offset))
                        })
                        != Some(selector)
                })
        }) {
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
        if let Some(record) = scalars
            .iter()
            .find(|record| !name_ids.contains(record.name.as_str()))
        {
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
        let (class_ids, _class_ids_reservation) = ctx.collect_scoped_string_set(
            classes.len(),
            classes.iter().map(|record| record.id.as_str()),
            "index SLDPRT classes",
        )?;
        let (scalar_ids, _scalar_ids_reservation) = ctx.collect_scoped_string_set(
            scalars.len(),
            scalars.iter().map(|record| record.id.as_str()),
            "index SLDPRT scalars",
        )?;
        if let Some(record) = relation_bindings.iter().find(|record| {
            !class_ids.contains(record.class_ref.as_str())
                || !scalar_ids.contains(record.scalar_ref.as_str())
                || record
                    .feature_ref
                    .as_deref()
                    .is_some_and(|feature| !feature_ids.contains(feature))
        }) {
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
        if let Some(record) = relation_instances.iter().find(|record| {
            !class_ids.contains(record.class_ref.as_str())
                || !feature_ids.contains(record.feature_ref.as_str())
                || record.scalar_refs().is_empty()
                || (record.scalar_refs().len() > 3
                    && !repeated_circle_display_shape_valid(record, &scalars, &names))
                || record
                    .scalar_refs()
                    .iter()
                    .enumerate()
                    .any(|(index, scalar)| record.scalar_refs()[..index].contains(scalar))
                || classes
                    .iter()
                    .find(|class| class.id == record.class_ref)
                    .is_none_or(|class| {
                        !matches!(
                            crate::classification::native_object_class(&class.name),
                            crate::classification::NativeClassKind::SketchRelation(family)
                                if family == record.family
                        )
                    })
                || record
                    .scalar_refs()
                    .iter()
                    .any(|scalar| !scalar_ids.contains(scalar.as_str()))
                || record.parameter_scalar_ref().is_some_and(|id| {
                    scalars
                        .iter()
                        .find(|scalar| scalar.id == id)
                        .is_none_or(|scalar| {
                            scalar.role != crate::records::FeatureInputScalarRole::Driving
                        })
                })
                || record.display_scalar_ref().is_some_and(|id| {
                    scalars
                        .iter()
                        .find(|scalar| scalar.id == id)
                        .is_none_or(|scalar| {
                            scalar.role != crate::records::FeatureInputScalarRole::Display
                        })
                })
        }) {
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
        for scalar in &scalars {
            for operand in &scalar.operands {
                let Some(reference) = references_by_id.get(operand.reference_ref.as_str()) else {
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
        for history in &mut native.feature_histories {
            history.configurations = ctx.try_collect_retained_with(
                configurations
                    .iter()
                    .filter(|record| record.parent == history.id),
                "attach SLDPRT history configurations",
                |record| record.clone_charged(ctx, "attach SLDPRT history configurations"),
            )?;
            ctx.stable_sort_by(
                &mut history.configurations,
                |value| &value.ordinal,
                Ord::cmp,
                "sort SLDPRT native records",
            )?;
            if let Some(pair) = history
                .configurations
                .windows(2)
                .find(|pair| pair[0].ordinal == pair[1].ordinal)
            {
                return Err(cadmpeg_ir::NativeConvertError::InvalidOwner(
                    ctx.format_retained(
                        format_args!(
                            "SolidWorks history {} repeats configuration ordinal {}",
                            history.id, pair[1].ordinal
                        ),
                        "format SLDPRT native validation error",
                    )?,
                ));
            }
            history.features = ctx.try_collect_retained_with(
                features.iter().filter(|record| record.parent == history.id),
                "attach SLDPRT history features",
                |record| record.clone_charged(ctx, "attach SLDPRT history features"),
            )?;
            ctx.stable_sort_by(
                &mut history.features,
                |value| &value.ordinal,
                Ord::cmp,
                "sort SLDPRT native records",
            )?;
            if let Some(pair) = history
                .features
                .windows(2)
                .find(|pair| pair[0].ordinal == pair[1].ordinal)
            {
                return Err(cadmpeg_ir::NativeConvertError::InvalidOwner(
                    ctx.format_retained(
                        format_args!(
                            "SolidWorks history {} repeats feature ordinal {}",
                            history.id, pair[1].ordinal
                        ),
                        "format SLDPRT native validation error",
                    )?,
                ));
            }
        }
        for lane in &mut native.feature_input_lanes {
            lane.classes = ctx.try_collect_retained_with(
                classes.iter().filter(|record| record.parent == lane.id),
                "attach SLDPRT lane classes",
                |record| record.clone_charged(ctx, "attach SLDPRT lane classes"),
            )?;
            ctx.stable_sort_by(
                &mut lane.classes,
                |value| &value.ordinal,
                Ord::cmp,
                "sort SLDPRT native records",
            )?;
            lane.names = ctx.try_collect_retained_with(
                names.iter().filter(|record| record.parent == lane.id),
                "attach SLDPRT lane names",
                |record| record.clone_charged(ctx, "attach SLDPRT lane names"),
            )?;
            ctx.stable_sort_by(
                &mut lane.names,
                |value| &value.ordinal,
                Ord::cmp,
                "sort SLDPRT native records",
            )?;
            lane.scalars = ctx.try_collect_retained_with(
                scalars.iter().filter(|record| record.parent == lane.id),
                "attach SLDPRT lane scalars",
                |record| record.clone_charged(ctx, "attach SLDPRT lane scalars"),
            )?;
            ctx.stable_sort_by(
                &mut lane.scalars,
                |value| &value.ordinal,
                Ord::cmp,
                "sort SLDPRT native records",
            )?;
            lane.references = ctx.try_collect_retained_with(
                references.iter().filter(|record| record.parent == lane.id),
                "attach SLDPRT lane references",
                |record| record.clone_charged(ctx, "attach SLDPRT lane references"),
            )?;
            ctx.stable_sort_by(
                &mut lane.references,
                |value| &value.ordinal,
                Ord::cmp,
                "sort SLDPRT native records",
            )?;
            lane.relation_bindings = ctx.try_collect_retained_with(
                relation_bindings
                    .iter()
                    .filter(|record| record.parent == lane.id),
                "attach SLDPRT lane relation bindings",
                |record| record.clone_charged(ctx, "attach SLDPRT lane relation bindings"),
            )?;
            ctx.stable_sort_by(
                &mut lane.relation_bindings,
                |value| &value.ordinal,
                Ord::cmp,
                "sort SLDPRT native records",
            )?;
            lane.relation_instances = ctx.try_collect_retained_with(
                relation_instances
                    .iter()
                    .filter(|record| record.parent == lane.id),
                "attach SLDPRT lane relation instances",
                |record| record.clone_charged(ctx, "attach SLDPRT lane relation instances"),
            )?;
            ctx.stable_sort_by(
                &mut lane.relation_instances,
                |value| &value.ordinal,
                Ord::cmp,
                "sort SLDPRT native records",
            )?;
            lane.body_selections = ctx.try_collect_retained_with(
                body_selections
                    .iter()
                    .filter(|record| record.parent == lane.id),
                "attach SLDPRT lane body selections",
                |record| record.clone_charged(ctx, "attach SLDPRT lane body selections"),
            )?;
            ctx.stable_sort_by(
                &mut lane.body_selections,
                |value| &value.ordinal,
                Ord::cmp,
                "sort SLDPRT native records",
            )?;
            for record in &lane.body_selections {
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
            lane.edge_selections = ctx.try_collect_retained_with(
                edge_selections
                    .iter()
                    .filter(|record| record.parent == lane.id),
                "attach SLDPRT lane edge selections",
                |record| record.clone_charged(ctx, "attach SLDPRT lane edge selections"),
            )?;
            ctx.stable_sort_by(
                &mut lane.edge_selections,
                |value| &value.ordinal,
                Ord::cmp,
                "sort SLDPRT native records",
            )?;
            let (mut edge_features, _edge_features_reservation) =
                ctx.with_scoped_storage("validate SLDPRT edge feature context", || {
                    ctx.try_collect_retained_with(
                        features.iter(),
                        "validate SLDPRT edge feature context",
                        |record| record.clone_charged(ctx, "validate SLDPRT edge feature context"),
                    )
                })?;
            crate::resolved_features::selections::enrich_feature_object_sources(
                ctx,
                &mut edge_features,
                std::slice::from_ref(lane),
            )?;
            for record in &lane.edge_selections {
                if edge_selection_disagrees_with_payload(ctx, lane, record, &edge_features)? {
                    return Err(cadmpeg_ir::NativeConvertError::InvalidOwner(
                        ctx.format_retained(
                            format_args!(
                                "feature-input edge selection {} disagrees with its payload",
                                record.id
                            ),
                            "format SLDPRT native validation error",
                        )?,
                    ));
                }
                let references = match usize::try_from(record.offset) {
                    Ok(offset) => {
                        let feature_kind = edge_features
                            .iter()
                            .find(|feature| feature.id == record.feature_ref)
                            .map(|feature| feature.kind.as_str())
                            .unwrap_or_default();
                        crate::resolved_features::selections::compact_edge_reference_list_for_feature(
                            ctx, &lane.native_payload, offset, feature_kind,
                        )?
                    }
                    Err(_) => None,
                };
                let disagreement = references.unwrap_or_default() != record.references;
                if disagreement {
                    return Err(cadmpeg_ir::NativeConvertError::InvalidOwner(
                        ctx.format_retained(
                            format_args!(
                                "feature-input edge selection {} disagrees with its payload",
                                record.id
                            ),
                            "format SLDPRT native validation error",
                        )?,
                    ));
                }
            }
            let (mut surface_features, _surface_features_reservation) =
                ctx.with_scoped_storage("validate SLDPRT surface feature context", || {
                    ctx.try_collect_retained_with(
                        features.iter(),
                        "validate SLDPRT surface feature context",
                        |record| {
                            record.clone_charged(ctx, "validate SLDPRT surface feature context")
                        },
                    )
                })?;
            crate::resolved_features::selections::enrich_feature_object_sources(
                ctx,
                &mut surface_features,
                std::slice::from_ref(lane),
            )?;
            lane.surface_selections = ctx.try_collect_retained_with(
                surface_selections
                    .iter()
                    .filter(|record| record.parent == lane.id),
                "attach SLDPRT lane surface selections",
                |record| record.clone_charged(ctx, "attach SLDPRT lane surface selections"),
            )?;
            ctx.stable_sort_by(
                &mut lane.surface_selections,
                |value| &value.ordinal,
                Ord::cmp,
                "sort SLDPRT native records",
            )?;
            for record in &lane.surface_selections {
                if surface_selection_disagrees_with_payload(ctx, lane, record, &surface_features)? {
                    return Err(cadmpeg_ir::NativeConvertError::InvalidOwner(
                        ctx.format_retained(
                            format_args!(
                                "feature-input surface selection {} disagrees with its payload",
                                record.id
                            ),
                            "format SLDPRT native validation error",
                        )?,
                    ));
                }
            }
            let mut records = ctx.try_collect_retained_with(
                generated_surface_identities
                    .iter()
                    .filter(|record| record.parent == lane.id),
                "attach SLDPRT lane generated surfaces",
                |record| record.clone_charged(ctx, "attach SLDPRT lane generated surfaces"),
            )?;
            ctx.stable_sort_by(
                &mut records,
                |value| &value.ordinal,
                Ord::cmp,
                "sort SLDPRT native records",
            )?;
            lane.generated_surface_identities = records;
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
            lane.sketch_entities = ctx.try_collect_retained_with(
                entities.iter().filter(|record| record.parent() == lane.id),
                "attach SLDPRT lane sketch entities",
                |record| record.clone_charged(ctx, "attach SLDPRT lane sketch entities"),
            )?;
            ctx.stable_sort_by_key(
                &mut lane.sketch_entities,
                super::records::SketchInputEntity::ordinal,
                Ord::cmp,
                "sort SLDPRT sketch entity records",
            )?;
        }
        lanes::admit(&native, ctx)?;
        Ok(native)
    }

    pub(crate) fn store(
        &self,
        ctx: &cadmpeg_core::decode::DecodeContext<'_>,
        namespace: &mut cadmpeg_ir::NativeNamespace,
    ) -> Result<(), cadmpeg_ir::NativeConvertError> {
        // Load admits every record against the lane payload it is derived from;
        // that is the boundary a hand-written namespace crosses. Store holds the
        // relations between records that no single payload derives.
        for history in &self.feature_histories {
            if let Some(record) = history
                .configurations
                .iter()
                .find(|record| record.parent != history.id)
            {
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
            if let Some(record) = history
                .features
                .iter()
                .find(|record| record.parent != history.id)
            {
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
        ctx.charge_work(
            cadmpeg_core::decode::u64_from_index(self.feature_histories.len()),
            "validate SLDPRT store features",
        )?;
        let (features, _features_reservation) =
            ctx.with_scoped_storage("validate SLDPRT store features", || {
                ctx.try_collect_retained_with(
                    self.feature_histories
                        .iter()
                        .flat_map(|history| &history.features),
                    "validate SLDPRT store features",
                    |record| record.clone_charged(ctx, "validate SLDPRT store features"),
                )
            })?;
        let (feature_ids, _feature_ids_reservation) = ctx.collect_scoped_string_set(
            features.len(),
            features.iter().map(|feature| feature.id.as_str()),
            "index SLDPRT stored features",
        )?;
        for lane in &self.feature_input_lanes {
            let (name_ids, _name_ids_reservation) = ctx.collect_scoped_string_set(
                lane.names.len(),
                lane.names.iter().map(|record| record.id.as_str()),
                "index SLDPRT stored names",
            )?;
            let (references_by_id, _references_by_id_reservation) = ctx.collect_scoped_string_map(
                lane.references.len(),
                lane.references
                    .iter()
                    .map(|record| (record.id.as_str(), record)),
                "index SLDPRT stored references",
            )?;
            let (class_ids, _class_ids_reservation) = ctx.collect_scoped_string_set(
                lane.classes.len(),
                lane.classes.iter().map(|record| record.id.as_str()),
                "index SLDPRT stored classes",
            )?;
            let (scalar_ids, _scalar_ids_reservation) = ctx.collect_scoped_string_set(
                lane.scalars.len(),
                lane.scalars.iter().map(|record| record.id.as_str()),
                "index SLDPRT stored scalars",
            )?;
            if let Some(record) = lane.classes.iter().find(|record| record.parent != lane.id) {
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
            if let Some(record) = lane.names.iter().find(|record| record.parent != lane.id) {
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
            if let Some(record) = lane.scalars.iter().find(|record| record.parent != lane.id) {
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
            for record in &lane.body_selections {
                let invalid = record.parent != lane.id
                    || !name_ids.contains(record.object_name_ref.as_str())
                    || !feature_ids.contains(record.feature_ref.as_str())
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
            let (mut edge_features, _edge_features_reservation) =
                ctx.with_scoped_storage("validate SLDPRT store edge features", || {
                    ctx.try_collect_retained_with(
                        features.iter(),
                        "validate SLDPRT store edge features",
                        |record| record.clone_charged(ctx, "validate SLDPRT store edge features"),
                    )
                })?;
            crate::resolved_features::selections::enrich_feature_object_sources(
                ctx,
                &mut edge_features,
                std::slice::from_ref(lane),
            )?;
            for record in &lane.edge_selections {
                let invalid = record.parent != lane.id
                    || !name_ids.contains(record.object_name_ref.as_str())
                    || !feature_ids.contains(record.feature_ref.as_str())
                    || record.local_edge_ids.is_empty();
                if invalid
                    || edge_selection_disagrees_with_payload(ctx, lane, record, &edge_features)?
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
            let (mut surface_features, _surface_features_reservation) =
                ctx.with_scoped_storage("validate SLDPRT store surface features", || {
                    ctx.try_collect_retained_with(
                        features.iter(),
                        "validate SLDPRT store surface features",
                        |record| {
                            record.clone_charged(ctx, "validate SLDPRT store surface features")
                        },
                    )
                })?;
            crate::resolved_features::selections::enrich_feature_object_sources(
                ctx,
                &mut surface_features,
                std::slice::from_ref(lane),
            )?;
            for record in &lane.surface_selections {
                let invalid = record.parent != lane.id
                    || !name_ids.contains(record.object_name_ref.as_str())
                    || !feature_ids.contains(record.feature_ref.as_str())
                    || record.components.is_empty();
                if invalid
                    || surface_selection_disagrees_with_payload(
                        ctx,
                        lane,
                        record,
                        &surface_features,
                    )?
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
            if let Some(record) = lane.scalars.iter().find(|record| {
                record
                    .feature_ref
                    .as_deref()
                    .is_some_and(|feature| !feature_ids.contains(feature))
            }) {
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
            if let Some(record) = lane
                .references
                .iter()
                .find(|record| record.parent != lane.id)
            {
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
            if let Some(record) = lane.relation_bindings.iter().find(|record| {
                record.parent != lane.id
                    || !class_ids.contains(record.class_ref.as_str())
                    || !scalar_ids.contains(record.scalar_ref.as_str())
                    || record
                        .feature_ref
                        .as_deref()
                        .is_some_and(|feature| !feature_ids.contains(feature))
            }) {
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
            if let Some(record) = lane.relation_instances.iter().find(|record| {
                record.parent != lane.id
                    || !class_ids.contains(record.class_ref.as_str())
                    || !feature_ids.contains(record.feature_ref.as_str())
                    || !relation_instance_shape_valid(record, lane)
                    || record
                        .scalar_refs()
                        .iter()
                        .any(|scalar| !scalar_ids.contains(scalar.as_str()))
                    || record.parameter_scalar_ref().is_some_and(|id| {
                        lane.scalars
                            .iter()
                            .find(|scalar| scalar.id == id)
                            .is_none_or(|scalar| {
                                scalar.role != crate::records::FeatureInputScalarRole::Driving
                            })
                    })
                    || record.display_scalar_ref().is_some_and(|id| {
                        lane.scalars
                            .iter()
                            .find(|scalar| scalar.id == id)
                            .is_none_or(|scalar| {
                                scalar.role != crate::records::FeatureInputScalarRole::Display
                            })
                    })
            }) {
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
            if let Some(record) = lane.relation_bindings.iter().find(|record| {
                lane.scalars
                    .iter()
                    .find(|scalar| scalar.id == record.scalar_ref)
                    .is_some_and(|scalar| scalar.feature_ref != record.feature_ref)
            }) {
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
            if let Some(record) = lane
                .scalars
                .iter()
                .find(|record| !name_ids.contains(record.name.as_str()))
            {
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
            for scalar in &lane.scalars {
                let resolved_operands = resolved_scalar_operand_markers(ctx, lane, scalar)?;
                for (operand, resolved) in scalar.operands.iter().zip(resolved_operands) {
                    let Some(reference) = references_by_id.get(operand.reference_ref.as_str())
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
                        let Some(target) = sketch_entities.get(entity_ref) else {
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
                        if resolved != Some(*target) {
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
            if let Some(record) = lane.sketch_entities.iter().find(|record| {
                record.parent() != lane.id
                    || record
                        .feature_ref
                        .as_deref()
                        .is_some_and(|feature| !feature_ids.contains(feature))
            }) {
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
            for record in &lane.sketch_entities {
                for link in record.links() {
                    let Some(target) = sketch_entities.get(link.entity_ref.as_str()) else {
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
                    self.feature_input_lanes.iter().filter(|lane| {
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
        if self
            .feature_histories
            .iter()
            .zip(&expected_histories)
            .flat_map(|(history, expected)| history.features.iter().zip(&expected.features))
            .any(|(feature, expected)| feature.input_class != expected.input_class)
        {
            return Err(cadmpeg_ir::NativeConvertError::InvalidOwner(
                "history feature classes do not match the feature-input index".into(),
            ));
        }
        SLDPRT_CATALOGUE.emit_all(ctx, self, namespace)?;
        Ok(())
    }
}

fn resolved_scalar_operand_markers<'a>(
    ctx: &DecodeContext<'_>,
    lane: &'a FeatureInputLane,
    scalar: &crate::records::FeatureInputScalar,
) -> Result<Vec<Option<&'a crate::records::SketchInputEntity>>, cadmpeg_ir::NativeConvertError> {
    let (entities, _entity_storage) =
        ctx.with_scoped_storage("SLDPRT scalar operand candidates", || {
            ctx.collect_vec(
                ctx.admit_iter(
                    &lane.sketch_entities,
                    "scan SLDPRT scalar operand candidates",
                )?
                .filter(|candidate| candidate.feature_ref == scalar.feature_ref),
                "collect SLDPRT scalar operand markers",
            )
        })?;
    Ok(
        crate::resolved_features::operands::resolve_scalar_operand_markers(
            ctx,
            &entities,
            &scalar.operands,
        )?,
    )
}

fn generated_surface_identities_disagree_with_payload(
    ctx: &DecodeContext<'_>,
    lane: &FeatureInputLane,
) -> Result<bool, cadmpeg_ir::NativeConvertError> {
    Ok(lane.generated_surface_identities
        != crate::resolved_features::selections::generated_surface_identities(ctx, lane)?)
}

fn body_state_ids_disagree_with_payload(
    ctx: &DecodeContext<'_>,
    lane: &FeatureInputLane,
    record: &FeatureInputBodySelection,
) -> Result<bool, cadmpeg_ir::NativeConvertError> {
    Ok(
        crate::resolved_features::selections::compact_body_state_ids_for_selection(
            ctx, lane, record,
        )? != record.body_state_ids,
    )
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
    Ok(selection.as_ref() != Some(&record.local_body_ids)
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
    Ok(match usize::try_from(record.offset) {
        Ok(offset) => crate::resolved_features::selections::compact_edge_selection_at(
            ctx,
            &lane.native_payload,
            offset,
        )?,
        Err(_) => None,
    }
    .as_ref()
        != Some(&record.local_edge_ids)
        || match usize::try_from(record.offset) {
            Ok(offset) => crate::resolved_features::selections::compact_edge_component_path_at(
                ctx,
                &lane.native_payload,
                offset,
            )?,
            Err(_) => None,
        }
        .unwrap_or_default()
            != record.components
        || match usize::try_from(record.offset) {
            Ok(offset) => crate::resolved_features::selections::compact_edge_producer_features_at(
                ctx,
                &lane.native_payload,
                offset,
                &record.components,
                edge_features,
                &record.feature_ref,
            )?,
            Err(_) => Vec::new(),
        } != record.producer_feature_refs
        || match usize::try_from(record.offset) {
            Ok(offset) => crate::resolved_features::selections::compact_edge_owner_feature_at(
                ctx,
                &lane.native_payload,
                offset,
                &record.components,
                edge_features,
                &record.feature_ref,
            )?,
            Err(_) => None,
        } != record.terminal_feature_ref)
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
    let matches_payload = match usize::try_from(record.offset) {
        Ok(offset) => crate::resolved_features::selections::surface_reference_matches_at(
            ctx,
            &lane.native_payload,
            offset,
            &record.components,
        )?,
        Err(_) => false,
    };
    Ok(!matches_payload
        || crate::resolved_features::component_paths::surface_selection_producer_features(
            ctx,
            &record.components,
            record.terminal_feature_ref.as_deref(),
            surface_features,
        )? != record.producer_feature_refs
        || match usize::try_from(record.offset) {
            Ok(offset) => {
                crate::resolved_features::selections::surface_selection_terminal_feature_at(
                    ctx,
                    &lane.native_payload,
                    offset,
                    &record.components,
                    surface_features,
                )?
            }
            Err(_) => None,
        } != record.terminal_feature_ref)
}

fn relation_instance_shape_valid(
    record: &FeatureInputRelationInstance,
    lane: &FeatureInputLane,
) -> bool {
    if record.scalar_refs().is_empty() {
        return false;
    }
    let Some(class) = lane
        .classes
        .iter()
        .find(|class| class.id == record.class_ref)
    else {
        return false;
    };
    if !matches!(
        crate::classification::native_object_class(&class.name),
        crate::classification::NativeClassKind::SketchRelation(family)
            if family == record.family
    ) {
        return false;
    }
    for scalar_ref in record.scalar_refs() {
        let Some(scalar) = lane.scalars.iter().find(|scalar| scalar.id == *scalar_ref) else {
            return false;
        };
        if scalar.feature_ref.as_deref() != Some(record.feature_ref.as_str()) {
            return false;
        }
    }
    let repeated_circle_display =
        repeated_circle_display_shape_valid(record, &lane.scalars, &lane.names);
    if record.scalar_refs().len() > 3 && !repeated_circle_display {
        return false;
    }
    let scalar_operands_match = |scalar: &crate::records::FeatureInputScalar| {
        scalar
            .operands
            .iter()
            .map(|operand| (operand.kind, operand.entity_index))
            .eq(record
                .operands
                .iter()
                .map(|operand| (operand.kind, operand.entity_index)))
            || (record.family == crate::records::FeatureInputRelationFamily::CircleDiameter
                && matches!(record.operands.as_slice(), [first, _]
                    if matches!(scalar.operands.as_slice(), [candidate]
                        if candidate.kind == first.kind
                            && candidate.entity_index == first.entity_index)))
            || (repeated_circle_display
                && matches!(record.operands.as_slice(), [first]
                    if matches!(scalar.operands.as_slice(), [candidate]
                        if candidate.kind == first.kind)))
    };
    let Some(first) = lane
        .scalars
        .iter()
        .find(|scalar| scalar.id == record.scalar_refs()[0])
    else {
        return false;
    };
    if first.offset != record.offset || !scalar_operands_match(first) {
        return false;
    }
    let mut last_operand_position = None;
    let mut detached = None;
    for scalar_ref in record.scalar_refs() {
        let Some((position, scalar)) = lane
            .scalars
            .iter()
            .enumerate()
            .find(|(_, scalar)| scalar.id == *scalar_ref)
        else {
            return false;
        };
        if scalar.operands.is_empty() {
            if detached.replace((position, scalar)).is_some() {
                return false;
            }
        } else {
            if last_operand_position.is_some_and(|previous| position != previous + 1)
                || !scalar_operands_match(scalar)
            {
                return false;
            }
            last_operand_position = Some(position);
        }
    }
    let Some(last_operand_position) = last_operand_position else {
        return false;
    };
    detached.is_none_or(|(position, scalar)| {
        record.parameter_scalar_ref() == Some(scalar.id.as_str())
            && position > last_operand_position
    })
}

fn repeated_circle_display_shape_valid(
    record: &FeatureInputRelationInstance,
    scalars: &[FeatureInputScalar],
    names: &[FeatureInputName],
) -> bool {
    if record.family != crate::records::FeatureInputRelationFamily::CircleDiameter
        || record.parameter_scalar_ref().is_some()
        || record.display_scalar_ref().is_some()
        || record.scalar_refs().len() < 2
        || record.operands.len() != 1
    {
        return false;
    }
    let scalar_name_value = |scalar: &FeatureInputScalar| {
        names
            .iter()
            .find(|name| name.id == scalar.name)
            .map(|name| name.value.as_str())
    };
    let Some(first) = scalars
        .iter()
        .find(|scalar| scalar.id == record.scalar_refs()[0])
    else {
        return false;
    };
    let Some(first_name) = scalar_name_value(first) else {
        return false;
    };
    let mut previous_ordinal = None;
    record
        .scalar_refs()
        .iter()
        .enumerate()
        .all(|(index, scalar_id)| {
            let Some(scalar) = scalars.iter().find(|scalar| scalar.id == *scalar_id) else {
                return false;
            };
            if previous_ordinal.is_some_and(|ordinal: u32| {
                scalar.ordinal.checked_sub(ordinal) != Some(1)
                    && !(ordinal == u32::MAX && scalar.ordinal == ordinal)
            }) {
                return false;
            }
            previous_ordinal = Some(scalar.ordinal);
            let [operand] = scalar.operands.as_slice() else {
                return false;
            };
            scalar.role == crate::records::FeatureInputScalarRole::Display
                && operand.kind == record.operands[0].kind
                && scalar_name_value(scalar) == Some(first_name)
                && record.scalar_refs()[..index].iter().all(|previous_id| {
                    scalars
                        .iter()
                        .find(|candidate| candidate.id == *previous_id)
                        .and_then(|previous| previous.operands.first())
                        .is_some_and(|previous| previous.entity_index != operand.entity_index)
                })
        })
}

#[cfg(test)]
mod tests;
