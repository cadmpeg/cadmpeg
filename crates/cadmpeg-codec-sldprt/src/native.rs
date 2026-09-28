// SPDX-License-Identifier: Apache-2.0
//! SOLIDWORKS native feature-history records.
#![deny(clippy::disallowed_methods)]

use serde::{ser::SerializeMap, Deserialize, Serialize};

use cadmpeg_core::decode::{DecodeContext, View};
use cadmpeg_ir::native::catalogue::{Catalogue, FamilyRow, Phase};

use self::admission::{admit_retained_clones, admit_temporary_clones, admit_validation_candidates};

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

mod admission;

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
        map.serialize_entry("configurations", &[] as &[()])?;
        map.serialize_entry("features", &[] as &[()])?;
        map.end()
    }
}

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
            map.serialize_entry(field, &[] as &[()])?;
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
        Self::load_inner(None, namespace)
    }

    pub(crate) fn load_charged(
        ctx: &DecodeContext<'_>,
        namespace: &cadmpeg_ir::NativeNamespace,
    ) -> Result<Self, cadmpeg_ir::NativeConvertError> {
        Self::load_inner(Some(ctx), namespace)
    }

    fn load_inner(
        ctx: Option<&DecodeContext<'_>>,
        namespace: &cadmpeg_ir::NativeNamespace,
    ) -> Result<Self, cadmpeg_ir::NativeConvertError> {
        macro_rules! read_arena {
            ($name:literal) => {
                match ctx {
                    Some(ctx) => namespace.arena_as_charged(ctx, $name)?,
                    None => namespace.arena_as($name)?,
                }
            };
        }
        macro_rules! admit_index {
            ($count:expr, $operation:literal) => {
                if let Some(ctx) = ctx {
                    ctx.charge_collection_items(
                        u64::try_from($count).map_err(|_| {
                            ctx.refuse_codec_limit($operation, u64::MAX - 1, u64::MAX)
                        })?,
                        $operation,
                    )?;
                }
            };
        }
        let mut native = Self {
            feature_histories: read_arena!("feature_histories"),
            feature_input_lanes: read_arena!("feature_input_lanes"),
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
        let relation_instances: Vec<FeatureInputRelationInstance> =
            read_arena!("feature_input_relation_instances");
        let scalars: Vec<FeatureInputScalar> = read_arena!("feature_input_scalars");
        admit_index!(native.feature_histories.len(), "index SLDPRT history ids");
        let history_ids = native
            .feature_histories
            .iter()
            .map(|history| history.id.as_str())
            .collect::<std::collections::HashSet<_>>();
        if let Some(record) = configurations
            .iter()
            .find(|record| !history_ids.contains(record.parent.as_str()))
        {
            return Err(cadmpeg_ir::NativeConvertError::InvalidOwner(format!(
                "configuration {} references {}",
                record.id, record.parent
            )));
        }
        if let Some(record) = features
            .iter()
            .find(|record| !history_ids.contains(record.parent.as_str()))
        {
            return Err(cadmpeg_ir::NativeConvertError::InvalidOwner(format!(
                "feature {} references {}",
                record.id, record.parent
            )));
        }
        admit_index!(features.len(), "index SLDPRT feature ids");
        let feature_ids = features
            .iter()
            .map(|record| record.id.as_str())
            .collect::<std::collections::HashSet<_>>();
        admit_index!(native.feature_input_lanes.len(), "index SLDPRT lane ids");
        let lane_ids = native
            .feature_input_lanes
            .iter()
            .map(|lane| lane.id.as_str())
            .collect::<std::collections::HashSet<_>>();
        admit_index!(
            native.feature_input_lanes.len(),
            "index SLDPRT lane payloads"
        );
        let lane_payloads = native
            .feature_input_lanes
            .iter()
            .map(|lane| (lane.id.as_str(), lane.native_payload.as_slice()))
            .collect::<std::collections::HashMap<_, _>>();
        if let Some(record) = entity_wires
            .iter()
            .find(|record| !lane_ids.contains(record.parent.as_str()))
        {
            return Err(cadmpeg_ir::NativeConvertError::InvalidOwner(format!(
                "sketch input entity {} references {}",
                record.id, record.parent
            )));
        }
        admit_index!(entity_wires.len(), "load SLDPRT sketch entities");
        let entities = entity_wires
            .into_iter()
            .map(|wire| {
                let payload = lane_payloads
                    .get(wire.parent.as_str())
                    .copied()
                    .ok_or_else(|| {
                        cadmpeg_ir::NativeConvertError::InvalidOwner(format!(
                            "sketch input entity {} references lane {} without a payload",
                            wire.id, wire.parent
                        ))
                    })?;
                crate::records::SketchInputEntity::try_from_wire(wire, payload)
                    .map_err(cadmpeg_ir::NativeConvertError::InvalidOwner)
            })
            .collect::<Result<Vec<_>, _>>()?;
        if let Some(record) = classes
            .iter()
            .find(|record| !lane_ids.contains(record.parent.as_str()))
        {
            return Err(cadmpeg_ir::NativeConvertError::InvalidOwner(format!(
                "feature-input class {} references {}",
                record.id, record.parent
            )));
        }
        if let Some(record) = body_selections
            .iter()
            .find(|record| !lane_ids.contains(record.parent.as_str()))
        {
            return Err(cadmpeg_ir::NativeConvertError::InvalidOwner(format!(
                "feature-input body selection {} references {}",
                record.id, record.parent
            )));
        }
        if let Some(record) = edge_selections
            .iter()
            .find(|record| !lane_ids.contains(record.parent.as_str()))
        {
            return Err(cadmpeg_ir::NativeConvertError::InvalidOwner(format!(
                "feature-input edge selection {} references {}",
                record.id, record.parent
            )));
        }
        if let Some(record) = surface_selections
            .iter()
            .find(|record| !lane_ids.contains(record.parent.as_str()))
        {
            return Err(cadmpeg_ir::NativeConvertError::InvalidOwner(format!(
                "feature-input surface selection {} references {}",
                record.id, record.parent
            )));
        }
        if let Some(record) = generated_surface_identities
            .iter()
            .find(|record| !lane_ids.contains(record.parent.as_str()))
        {
            return Err(cadmpeg_ir::NativeConvertError::InvalidOwner(format!(
                "feature-input generated surface identity {} references {}",
                record.id, record.parent
            )));
        }
        if let Some(record) = names
            .iter()
            .find(|record| !lane_ids.contains(record.parent.as_str()))
        {
            return Err(cadmpeg_ir::NativeConvertError::InvalidOwner(format!(
                "feature-input name {} references {}",
                record.id, record.parent
            )));
        }
        if let Some(record) = scalars
            .iter()
            .find(|record| !lane_ids.contains(record.parent.as_str()))
        {
            return Err(cadmpeg_ir::NativeConvertError::InvalidOwner(format!(
                "feature-input scalar {} references {}",
                record.id, record.parent
            )));
        }
        if let Some(record) = scalars.iter().find(|record| {
            record
                .feature_ref
                .as_deref()
                .is_some_and(|feature| !feature_ids.contains(feature))
        }) {
            return Err(cadmpeg_ir::NativeConvertError::InvalidOwner(format!(
                "feature-input scalar {} references missing feature {}",
                record.id,
                record.feature_ref.as_deref().unwrap_or_default()
            )));
        }
        if let Some(record) = references
            .iter()
            .find(|record| !lane_ids.contains(record.parent.as_str()))
        {
            return Err(cadmpeg_ir::NativeConvertError::InvalidOwner(format!(
                "feature-input reference {} references {}",
                record.id, record.parent
            )));
        }
        if let Some(record) = relation_bindings
            .iter()
            .find(|record| !lane_ids.contains(record.parent.as_str()))
        {
            return Err(cadmpeg_ir::NativeConvertError::InvalidOwner(format!(
                "feature-input relation binding {} references {}",
                record.id, record.parent
            )));
        }
        if let Some(record) = relation_instances
            .iter()
            .find(|record| !lane_ids.contains(record.parent.as_str()))
        {
            return Err(cadmpeg_ir::NativeConvertError::InvalidOwner(format!(
                "feature-input relation instance {} references {}",
                record.id, record.parent
            )));
        }
        admit_index!(names.len(), "index SLDPRT feature names");
        let name_ids = names
            .iter()
            .map(|record| record.id.as_str())
            .collect::<std::collections::HashSet<_>>();
        if let Some(record) = body_selections.iter().find(|record| {
            !name_ids.contains(record.object_name_ref.as_str())
                || !feature_ids.contains(record.feature_ref.as_str())
                || record.local_body_ids.is_empty()
        }) {
            return Err(cadmpeg_ir::NativeConvertError::InvalidOwner(format!(
                "feature-input body selection {} has unresolved ownership",
                record.id
            )));
        }
        if let Some(record) = edge_selections.iter().find(|record| {
            !name_ids.contains(record.object_name_ref.as_str())
                || !feature_ids.contains(record.feature_ref.as_str())
                || record.local_edge_ids.is_empty()
        }) {
            return Err(cadmpeg_ir::NativeConvertError::InvalidOwner(format!(
                "feature-input edge selection {} has unresolved ownership",
                record.id
            )));
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
            return Err(cadmpeg_ir::NativeConvertError::InvalidOwner(format!(
                "feature-input surface selection {} has unresolved ownership",
                record.id
            )));
        }
        if let Some(record) = scalars
            .iter()
            .find(|record| !name_ids.contains(record.name.as_str()))
        {
            return Err(cadmpeg_ir::NativeConvertError::InvalidOwner(format!(
                "feature-input scalar {} references name {}",
                record.id, record.name
            )));
        }
        admit_index!(references.len(), "index SLDPRT references");
        let references_by_id = references
            .iter()
            .map(|record| (record.id.as_str(), record))
            .collect::<std::collections::HashMap<_, _>>();
        admit_index!(classes.len(), "index SLDPRT classes");
        let class_ids = classes
            .iter()
            .map(|record| record.id.as_str())
            .collect::<std::collections::HashSet<_>>();
        admit_index!(scalars.len(), "index SLDPRT scalars");
        let scalar_ids = scalars
            .iter()
            .map(|record| record.id.as_str())
            .collect::<std::collections::HashSet<_>>();
        if let Some(record) = relation_bindings.iter().find(|record| {
            !class_ids.contains(record.class_ref.as_str())
                || !scalar_ids.contains(record.scalar_ref.as_str())
                || record
                    .feature_ref
                    .as_deref()
                    .is_some_and(|feature| !feature_ids.contains(feature))
        }) {
            return Err(cadmpeg_ir::NativeConvertError::InvalidOwner(format!(
                "feature-input relation binding {} has an unresolved class or scalar",
                record.id
            )));
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
            return Err(cadmpeg_ir::NativeConvertError::InvalidOwner(format!(
                "feature-input relation instance {} has an unresolved class, feature, or scalar",
                record.id
            )));
        }
        for scalar in &scalars {
            for operand in &scalar.operands {
                let Some(reference) = references_by_id.get(operand.reference_ref.as_str()) else {
                    return Err(cadmpeg_ir::NativeConvertError::InvalidOwner(format!(
                        "feature-input scalar {} references missing cell {}",
                        scalar.id, operand.reference_ref
                    )));
                };
                if reference.offset != operand.offset
                    || reference.kind != operand.kind
                    || reference.object_index != operand.entity_index
                {
                    return Err(cadmpeg_ir::NativeConvertError::InvalidOwner(format!(
                        "feature-input scalar {} has inconsistent cell {}",
                        scalar.id, operand.reference_ref
                    )));
                }
            }
        }
        for history in &mut native.feature_histories {
            admit_retained_clones(
                ctx,
                configurations
                    .iter()
                    .filter(|record| record.parent == history.id),
                "attach SLDPRT history configurations",
            )?;
            history.configurations = configurations
                .iter()
                .filter(|record| record.parent == history.id)
                .cloned()
                .collect();
            history.configurations.sort_by_key(|record| record.ordinal);
            if let Some(pair) = history
                .configurations
                .windows(2)
                .find(|pair| pair[0].ordinal == pair[1].ordinal)
            {
                return Err(cadmpeg_ir::NativeConvertError::InvalidOwner(format!(
                    "SolidWorks history {} repeats configuration ordinal {}",
                    history.id, pair[1].ordinal
                )));
            }
            admit_retained_clones(
                ctx,
                features.iter().filter(|record| record.parent == history.id),
                "attach SLDPRT history features",
            )?;
            history.features = features
                .iter()
                .filter(|record| record.parent == history.id)
                .cloned()
                .collect();
            history.features.sort_by_key(|record| record.ordinal);
            if let Some(pair) = history
                .features
                .windows(2)
                .find(|pair| pair[0].ordinal == pair[1].ordinal)
            {
                return Err(cadmpeg_ir::NativeConvertError::InvalidOwner(format!(
                    "SolidWorks history {} repeats feature ordinal {}",
                    history.id, pair[1].ordinal
                )));
            }
        }
        for lane in &mut native.feature_input_lanes {
            admit_retained_clones(
                ctx,
                classes.iter().filter(|record| record.parent == lane.id),
                "attach SLDPRT lane classes",
            )?;
            lane.classes = classes
                .iter()
                .filter(|record| record.parent == lane.id)
                .cloned()
                .collect();
            lane.classes.sort_by_key(|record| record.ordinal);
            admit_retained_clones(
                ctx,
                names.iter().filter(|record| record.parent == lane.id),
                "attach SLDPRT lane names",
            )?;
            lane.names = names
                .iter()
                .filter(|record| record.parent == lane.id)
                .cloned()
                .collect();
            lane.names.sort_by_key(|record| record.ordinal);
            admit_retained_clones(
                ctx,
                scalars.iter().filter(|record| record.parent == lane.id),
                "attach SLDPRT lane scalars",
            )?;
            lane.scalars = scalars
                .iter()
                .filter(|record| record.parent == lane.id)
                .cloned()
                .collect();
            lane.scalars.sort_by_key(|record| record.ordinal);
            admit_retained_clones(
                ctx,
                references.iter().filter(|record| record.parent == lane.id),
                "attach SLDPRT lane references",
            )?;
            lane.references = references
                .iter()
                .filter(|record| record.parent == lane.id)
                .cloned()
                .collect();
            lane.references.sort_by_key(|record| record.ordinal);
            admit_retained_clones(
                ctx,
                relation_bindings
                    .iter()
                    .filter(|record| record.parent == lane.id),
                "attach SLDPRT lane relation bindings",
            )?;
            lane.relation_bindings = relation_bindings
                .iter()
                .filter(|record| record.parent == lane.id)
                .cloned()
                .collect();
            lane.relation_bindings.sort_by_key(|record| record.ordinal);
            admit_retained_clones(
                ctx,
                relation_instances
                    .iter()
                    .filter(|record| record.parent == lane.id),
                "attach SLDPRT lane relation instances",
            )?;
            lane.relation_instances = relation_instances
                .iter()
                .filter(|record| record.parent == lane.id)
                .cloned()
                .collect();
            lane.relation_instances.sort_by_key(|record| record.ordinal);
            admit_retained_clones(
                ctx,
                body_selections
                    .iter()
                    .filter(|record| record.parent == lane.id),
                "attach SLDPRT lane body selections",
            )?;
            lane.body_selections = body_selections
                .iter()
                .filter(|record| record.parent == lane.id)
                .cloned()
                .collect();
            lane.body_selections.sort_by_key(|record| record.ordinal);
            for record in &lane.body_selections {
                if body_selection_disagrees_with_payload(ctx, lane, record)? {
                    return Err(cadmpeg_ir::NativeConvertError::InvalidOwner(format!(
                        "feature-input body selection {} disagrees with its payload",
                        record.id
                    )));
                }
            }
            admit_retained_clones(
                ctx,
                edge_selections
                    .iter()
                    .filter(|record| record.parent == lane.id),
                "attach SLDPRT lane edge selections",
            )?;
            lane.edge_selections = edge_selections
                .iter()
                .filter(|record| record.parent == lane.id)
                .cloned()
                .collect();
            lane.edge_selections.sort_by_key(|record| record.ordinal);
            let _edge_features_reservation = admit_temporary_clones(
                ctx,
                features.iter(),
                "validate SLDPRT edge feature context",
            )?;
            let mut edge_features = features.clone();
            crate::resolved_features::selections::enrich_feature_object_sources(
                &mut edge_features,
                std::slice::from_ref(lane),
            );
            for record in &lane.edge_selections {
                if edge_selection_disagrees_with_payload(ctx, lane, record, &edge_features)? {
                    return Err(cadmpeg_ir::NativeConvertError::InvalidOwner(format!(
                        "feature-input edge selection {} disagrees with its payload",
                        record.id
                    )));
                }
                let _reference_reservation = admit_validation_candidates(
                    ctx,
                    selection_payload_span(lane, record.offset),
                    "validate SLDPRT edge reference candidates",
                )?;
                let disagreement = usize::try_from(record.offset)
                        .ok()
                        .and_then(|offset| {
                            let feature_kind = edge_features
                                .iter()
                                .find(|feature| feature.id == record.feature_ref)
                                .map(|feature| feature.kind.as_str())
                                .unwrap_or_default();
                            crate::resolved_features::selections::compact_edge_reference_list_for_feature(
                                &lane.native_payload,
                                offset,
                                feature_kind,
                            )
                        })
                        .unwrap_or_default()
                        != record.references;
                if disagreement {
                    return Err(cadmpeg_ir::NativeConvertError::InvalidOwner(format!(
                        "feature-input edge selection {} disagrees with its payload",
                        record.id
                    )));
                }
            }
            let _surface_features_reservation = admit_temporary_clones(
                ctx,
                features.iter(),
                "validate SLDPRT surface feature context",
            )?;
            let mut surface_features = features.clone();
            crate::resolved_features::selections::enrich_feature_object_sources(
                &mut surface_features,
                std::slice::from_ref(lane),
            );
            admit_retained_clones(
                ctx,
                surface_selections
                    .iter()
                    .filter(|record| record.parent == lane.id),
                "attach SLDPRT lane surface selections",
            )?;
            lane.surface_selections = surface_selections
                .iter()
                .filter(|record| record.parent == lane.id)
                .cloned()
                .collect();
            lane.surface_selections.sort_by_key(|record| record.ordinal);
            for record in &lane.surface_selections {
                if surface_selection_disagrees_with_payload(ctx, lane, record, &surface_features)? {
                    return Err(cadmpeg_ir::NativeConvertError::InvalidOwner(format!(
                        "feature-input surface selection {} disagrees with its payload",
                        record.id
                    )));
                }
            }
            admit_retained_clones(
                ctx,
                generated_surface_identities
                    .iter()
                    .filter(|record| record.parent == lane.id),
                "attach SLDPRT lane generated surfaces",
            )?;
            let mut records = generated_surface_identities
                .iter()
                .filter(|record| record.parent == lane.id)
                .cloned()
                .collect::<Vec<_>>();
            records.sort_by_key(|record| record.ordinal);
            lane.generated_surface_identities = records;
            if generated_surface_identities_disagree_with_payload(ctx, lane)? {
                return Err(cadmpeg_ir::NativeConvertError::InvalidOwner(format!(
                    "feature-input lane {} generated surface identities disagree with its payload",
                    lane.id
                )));
            }
            admit_retained_clones(
                ctx,
                entities.iter().filter(|record| record.parent() == lane.id),
                "attach SLDPRT lane sketch entities",
            )?;
            lane.sketch_entities = entities
                .iter()
                .filter(|record| record.parent() == lane.id)
                .cloned()
                .collect();
            lane.sketch_entities
                .sort_by_key(crate::records::SketchInputEntity::ordinal);
        }
        lanes::admit(&native, ctx)?;
        Ok(native)
    }

    pub(crate) fn store(
        &self,
        ctx: &cadmpeg_core::decode::DecodeContext<'_>,
        namespace: &mut cadmpeg_ir::NativeNamespace,
    ) -> Result<(), cadmpeg_ir::NativeConvertError> {
        macro_rules! admit_store_index {
            ($count:expr, $operation:literal) => {
                ctx.charge_collection_items(
                    u64::try_from($count)
                        .map_err(|_| ctx.refuse_codec_limit($operation, u64::MAX - 1, u64::MAX))?,
                    $operation,
                )?;
            };
        }
        // Load admits every record against the lane payload it is derived from;
        // that is the boundary a hand-written namespace crosses. Store holds the
        // relations between records that no single payload derives.
        for history in &self.feature_histories {
            if let Some(record) = history
                .configurations
                .iter()
                .find(|record| record.parent != history.id)
            {
                return Err(cadmpeg_ir::NativeConvertError::InvalidOwner(format!(
                    "configuration {} references {} instead of {}",
                    record.id, record.parent, history.id
                )));
            }
            if let Some(record) = history
                .features
                .iter()
                .find(|record| record.parent != history.id)
            {
                return Err(cadmpeg_ir::NativeConvertError::InvalidOwner(format!(
                    "feature {} references {} instead of {}",
                    record.id, record.parent, history.id
                )));
            }
        }
        let _features_reservation = admit_temporary_clones(
            Some(ctx),
            self.feature_histories
                .iter()
                .flat_map(|history| &history.features),
            "validate SLDPRT store features",
        )?;
        let features = self
            .feature_histories
            .iter()
            .flat_map(|history| &history.features)
            .cloned()
            .collect::<Vec<_>>();
        admit_store_index!(features.len(), "index SLDPRT stored features");
        let feature_ids = features
            .iter()
            .map(|feature| feature.id.as_str())
            .collect::<std::collections::HashSet<_>>();
        for lane in &self.feature_input_lanes {
            admit_store_index!(lane.names.len(), "index SLDPRT stored names");
            let name_ids = lane
                .names
                .iter()
                .map(|record| record.id.as_str())
                .collect::<std::collections::HashSet<_>>();
            admit_store_index!(lane.references.len(), "index SLDPRT stored references");
            let references_by_id = lane
                .references
                .iter()
                .map(|record| (record.id.as_str(), record))
                .collect::<std::collections::HashMap<_, _>>();
            admit_store_index!(lane.classes.len(), "index SLDPRT stored classes");
            let class_ids = lane
                .classes
                .iter()
                .map(|record| record.id.as_str())
                .collect::<std::collections::HashSet<_>>();
            admit_store_index!(lane.scalars.len(), "index SLDPRT stored scalars");
            let scalar_ids = lane
                .scalars
                .iter()
                .map(|record| record.id.as_str())
                .collect::<std::collections::HashSet<_>>();
            if let Some(record) = lane.classes.iter().find(|record| record.parent != lane.id) {
                return Err(cadmpeg_ir::NativeConvertError::InvalidOwner(format!(
                    "feature-input class {} references {} instead of {}",
                    record.id, record.parent, lane.id
                )));
            }
            if let Some(record) = lane.names.iter().find(|record| record.parent != lane.id) {
                return Err(cadmpeg_ir::NativeConvertError::InvalidOwner(format!(
                    "feature-input name {} references {} instead of {}",
                    record.id, record.parent, lane.id
                )));
            }
            if let Some(record) = lane.scalars.iter().find(|record| record.parent != lane.id) {
                return Err(cadmpeg_ir::NativeConvertError::InvalidOwner(format!(
                    "feature-input scalar {} references {} instead of {}",
                    record.id, record.parent, lane.id
                )));
            }
            for record in &lane.body_selections {
                let invalid = record.parent != lane.id
                    || !name_ids.contains(record.object_name_ref.as_str())
                    || !feature_ids.contains(record.feature_ref.as_str())
                    || record.local_body_ids.is_empty();
                if invalid {
                    return Err(cadmpeg_ir::NativeConvertError::InvalidOwner(format!(
                        "feature-input body selection {} has inconsistent ownership",
                        record.id
                    )));
                }
                let invalid = body_state_ids_disagree_with_payload(Some(ctx), lane, record)?
                    || body_selection_disagrees_with_payload(Some(ctx), lane, record)?;
                if invalid {
                    return Err(cadmpeg_ir::NativeConvertError::InvalidOwner(format!(
                        "feature-input body selection {} has inconsistent ownership",
                        record.id
                    )));
                }
            }
            let _edge_features_reservation = admit_temporary_clones(
                Some(ctx),
                features.iter(),
                "validate SLDPRT store edge features",
            )?;
            let mut edge_features = features.clone();
            crate::resolved_features::selections::enrich_feature_object_sources(
                &mut edge_features,
                std::slice::from_ref(lane),
            );
            for record in &lane.edge_selections {
                let invalid = record.parent != lane.id
                    || !name_ids.contains(record.object_name_ref.as_str())
                    || !feature_ids.contains(record.feature_ref.as_str())
                    || record.local_edge_ids.is_empty();
                if invalid
                    || edge_selection_disagrees_with_payload(
                        Some(ctx),
                        lane,
                        record,
                        &edge_features,
                    )?
                {
                    return Err(cadmpeg_ir::NativeConvertError::InvalidOwner(format!(
                        "feature-input edge selection {} has inconsistent ownership",
                        record.id
                    )));
                }
            }
            let _surface_features_reservation = admit_temporary_clones(
                Some(ctx),
                features.iter(),
                "validate SLDPRT store surface features",
            )?;
            let mut surface_features = features.clone();
            crate::resolved_features::selections::enrich_feature_object_sources(
                &mut surface_features,
                std::slice::from_ref(lane),
            );
            for record in &lane.surface_selections {
                let invalid = record.parent != lane.id
                    || !name_ids.contains(record.object_name_ref.as_str())
                    || !feature_ids.contains(record.feature_ref.as_str())
                    || record.components.is_empty();
                if invalid
                    || surface_selection_disagrees_with_payload(
                        Some(ctx),
                        lane,
                        record,
                        &surface_features,
                    )?
                {
                    return Err(cadmpeg_ir::NativeConvertError::InvalidOwner(format!(
                        "feature-input surface selection {} has inconsistent ownership",
                        record.id
                    )));
                }
            }
            if let Some(record) = lane.scalars.iter().find(|record| {
                record
                    .feature_ref
                    .as_deref()
                    .is_some_and(|feature| !feature_ids.contains(feature))
            }) {
                return Err(cadmpeg_ir::NativeConvertError::InvalidOwner(format!(
                    "feature-input scalar {} references missing feature {}",
                    record.id,
                    record.feature_ref.as_deref().unwrap_or_default()
                )));
            }
            if let Some(record) = lane
                .references
                .iter()
                .find(|record| record.parent != lane.id)
            {
                return Err(cadmpeg_ir::NativeConvertError::InvalidOwner(format!(
                    "feature-input reference {} references {} instead of {}",
                    record.id, record.parent, lane.id
                )));
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
                return Err(cadmpeg_ir::NativeConvertError::InvalidOwner(format!(
                    "feature-input relation binding {} has inconsistent ownership",
                    record.id
                )));
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
                return Err(cadmpeg_ir::NativeConvertError::InvalidOwner(format!(
                    "feature-input relation instance {} has inconsistent ownership",
                    record.id
                )));
            }
            if let Some(record) = lane.relation_bindings.iter().find(|record| {
                lane.scalars
                    .iter()
                    .find(|scalar| scalar.id == record.scalar_ref)
                    .is_some_and(|scalar| scalar.feature_ref != record.feature_ref)
            }) {
                return Err(cadmpeg_ir::NativeConvertError::InvalidOwner(format!(
                    "feature-input relation binding {} disagrees with its scalar owner",
                    record.id
                )));
            }
            if let Some(record) = lane
                .scalars
                .iter()
                .find(|record| !name_ids.contains(record.name.as_str()))
            {
                return Err(cadmpeg_ir::NativeConvertError::InvalidOwner(format!(
                    "feature-input scalar {} references name {}",
                    record.id, record.name
                )));
            }
            admit_store_index!(
                lane.sketch_entities.len(),
                "index SLDPRT stored sketch entities"
            );
            let sketch_entities = lane
                .sketch_entities
                .iter()
                .map(|record| (record.id(), record))
                .collect::<std::collections::HashMap<_, _>>();
            for scalar in &lane.scalars {
                let resolved_operands = resolved_scalar_operand_markers(ctx, lane, scalar)?;
                for (operand, resolved) in scalar.operands.iter().zip(resolved_operands) {
                    let Some(reference) = references_by_id.get(operand.reference_ref.as_str())
                    else {
                        return Err(cadmpeg_ir::NativeConvertError::InvalidOwner(format!(
                            "feature-input scalar {} references missing cell {}",
                            scalar.id, operand.reference_ref
                        )));
                    };
                    if reference.offset != operand.offset
                        || reference.kind != operand.kind
                        || reference.object_index != operand.entity_index
                    {
                        return Err(cadmpeg_ir::NativeConvertError::InvalidOwner(format!(
                            "feature-input scalar {} has inconsistent cell {}",
                            scalar.id, operand.reference_ref
                        )));
                    }
                    if let Some(entity_ref) = operand.entity_ref.as_deref() {
                        let Some(target) = sketch_entities.get(entity_ref) else {
                            return Err(cadmpeg_ir::NativeConvertError::InvalidOwner(format!(
                                "feature-input scalar {} references missing sketch marker {}",
                                scalar.id, entity_ref
                            )));
                        };
                        if resolved != Some(*target) {
                            return Err(cadmpeg_ir::NativeConvertError::InvalidOwner(format!(
                                "feature-input scalar {} has inconsistent sketch marker {}",
                                scalar.id, entity_ref
                            )));
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
                return Err(cadmpeg_ir::NativeConvertError::InvalidOwner(format!(
                    "sketch input entity {} has inconsistent lane or feature ownership",
                    record.id()
                )));
            }
            for record in &lane.sketch_entities {
                for link in record.links() {
                    let Some(target) = sketch_entities.get(link.entity_ref.as_str()) else {
                        return Err(cadmpeg_ir::NativeConvertError::InvalidOwner(format!(
                            "sketch input entity {} references missing local-link target {}",
                            record.id(),
                            link.entity_ref
                        )));
                    };
                    if target.feature_ref != record.feature_ref
                        || target.local_id() != Some(u32::from(link.local_id))
                    {
                        return Err(cadmpeg_ir::NativeConvertError::InvalidOwner(format!(
                            "sketch input entity {} has inconsistent local-link target {}",
                            record.id(),
                            link.entity_ref
                        )));
                    }
                }
            }
        }
        let _expected_histories_reservation = admit_temporary_clones(
            Some(ctx),
            self.feature_histories.iter(),
            "validate SLDPRT expected histories",
        )?;
        let mut expected_histories = self.feature_histories.clone();
        let _history_lanes_reservation = admit_temporary_clones(
            Some(ctx),
            self.feature_input_lanes.iter().filter(|lane| {
                !crate::resolved_features::assembly::is_supplemental_config_lane(lane)
            }),
            "validate SLDPRT history lanes",
        )?;
        let history_lanes = self
            .feature_input_lanes
            .iter()
            .filter(|lane| !crate::resolved_features::assembly::is_supplemental_config_lane(lane))
            .cloned()
            .collect::<Vec<_>>();
        bind_history_classes_charged(ctx, &mut expected_histories, &history_lanes)?;
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

fn bind_history_classes_charged(
    ctx: &DecodeContext<'_>,
    histories: &mut [FeatureHistory],
    lanes: &[FeatureInputLane],
) -> Result<(), cadmpeg_ir::NativeConvertError> {
    let source_items = lanes.iter().try_fold(0usize, |count, lane| {
        count
            .checked_add(lane.names.len())
            .and_then(|count| count.checked_add(lane.classes.len()))
    });
    let source_items = source_items.ok_or_else(|| {
        ctx.refuse_codec_limit(
            "validate SLDPRT history class candidates",
            u64::MAX - 1,
            u64::MAX,
        )
    })?;
    let _reservation = admit_validation_candidates(
        Some(ctx),
        source_items,
        "validate SLDPRT history class candidates",
    )?;
    crate::resolved_features::classes::bind_history_classes(histories, lanes);
    Ok(())
}

fn resolved_scalar_operand_markers<'a>(
    ctx: &DecodeContext<'_>,
    lane: &'a FeatureInputLane,
    scalar: &crate::records::FeatureInputScalar,
) -> Result<Vec<Option<&'a crate::records::SketchInputEntity>>, cadmpeg_ir::NativeConvertError> {
    let source_items = lane
        .sketch_entities
        .len()
        .checked_add(scalar.operands.len())
        .ok_or_else(|| {
            ctx.refuse_codec_limit(
                "validate SLDPRT scalar operand candidates",
                u64::MAX - 1,
                u64::MAX,
            )
        })?;
    let _reservation = admit_validation_candidates(
        Some(ctx),
        source_items,
        "validate SLDPRT scalar operand candidates",
    )?;
    Ok(
        crate::resolved_features::operands::resolve_scalar_operand_markers(
            lane.sketch_entities
                .iter()
                .filter(|candidate| candidate.feature_ref == scalar.feature_ref),
            &scalar.operands,
        ),
    )
}

fn generated_surface_identities_disagree_with_payload(
    ctx: Option<&DecodeContext<'_>>,
    lane: &FeatureInputLane,
) -> Result<bool, cadmpeg_ir::NativeConvertError> {
    let _reservation = if lane
        .classes
        .iter()
        .any(|class| class.name.ends_with("SurfIdRep_c"))
    {
        admit_validation_candidates(
            ctx,
            lane.native_payload.len(),
            "validate SLDPRT generated surface identities",
        )?
    } else {
        None
    };
    Ok(lane.generated_surface_identities
        != crate::resolved_features::selections::generated_surface_identities(lane))
}

fn selection_payload_span(lane: &FeatureInputLane, offset: u64) -> usize {
    usize::try_from(offset)
        .ok()
        .and_then(|start| lane.native_payload.len().checked_sub(start))
        .unwrap_or(0)
}

fn body_state_ids_disagree_with_payload(
    ctx: Option<&DecodeContext<'_>>,
    lane: &FeatureInputLane,
    record: &FeatureInputBodySelection,
) -> Result<bool, cadmpeg_ir::NativeConvertError> {
    let source_units = lane
        .names
        .iter()
        .find(|name| name.id == record.object_name_ref)
        .and_then(|name| record.offset.checked_sub(name.offset))
        .and_then(|span| usize::try_from(span).ok())
        .unwrap_or(0);
    let _reservation =
        admit_validation_candidates(ctx, source_units, "validate SLDPRT body state candidates")?;
    Ok(
        crate::resolved_features::selections::compact_body_state_ids_for_selection(lane, record)
            != record.body_state_ids,
    )
}

/// `true` when a body selection disagrees with the compact selection in its lane payload.
fn body_selection_disagrees_with_payload(
    ctx: Option<&DecodeContext<'_>>,
    lane: &FeatureInputLane,
    record: &FeatureInputBodySelection,
) -> Result<bool, cadmpeg_ir::NativeConvertError> {
    let _reservation = admit_validation_candidates(
        ctx,
        selection_payload_span(lane, record.offset),
        "validate SLDPRT body selection candidates",
    )?;
    Ok(usize::try_from(record.offset)
        .ok()
        .and_then(|offset| {
            crate::resolved_features::selections::compact_body_selection_at(
                &lane.native_payload,
                offset,
            )
        })
        .as_ref()
        != Some(&record.local_body_ids)
        || crate::resolved_features::selections::compact_body_retention_mode_for_selection(
            lane, record,
        ) != record.mode)
}

/// `true` when an edge selection disagrees with the compact selection in its lane payload.
///
/// `edge_features` are the history features enriched with this lane's object sources.
fn edge_selection_disagrees_with_payload(
    ctx: Option<&DecodeContext<'_>>,
    lane: &FeatureInputLane,
    record: &FeatureInputEdgeSelection,
    edge_features: &[crate::records::Feature],
) -> Result<bool, cadmpeg_ir::NativeConvertError> {
    let _reservation = admit_validation_candidates(
        ctx,
        selection_payload_span(lane, record.offset),
        "validate SLDPRT edge selection candidates",
    )?;
    Ok(usize::try_from(record.offset)
        .ok()
        .and_then(|offset| {
            crate::resolved_features::selections::compact_edge_selection_at(
                &lane.native_payload,
                offset,
            )
        })
        .as_ref()
        != Some(&record.local_edge_ids)
        || usize::try_from(record.offset)
            .ok()
            .and_then(|offset| {
                crate::resolved_features::selections::compact_edge_component_path_at(
                    &lane.native_payload,
                    offset,
                )
            })
            .unwrap_or_default()
            != record.components
        || usize::try_from(record.offset)
            .ok()
            .map(|offset| {
                crate::resolved_features::selections::compact_edge_producer_features_at(
                    &lane.native_payload,
                    offset,
                    &record.components,
                    edge_features,
                    &record.feature_ref,
                )
            })
            .unwrap_or_default()
            != record.producer_feature_refs
        || usize::try_from(record.offset).ok().and_then(|offset| {
            crate::resolved_features::selections::compact_edge_owner_feature_at(
                &lane.native_payload,
                offset,
                &record.components,
                edge_features,
                &record.feature_ref,
            )
        }) != record.terminal_feature_ref)
}

/// `true` when a surface selection disagrees with the compact reference in its lane payload.
///
/// `surface_features` are the history features enriched with this lane's object sources.
fn surface_selection_disagrees_with_payload(
    ctx: Option<&DecodeContext<'_>>,
    lane: &FeatureInputLane,
    record: &FeatureInputSurfaceSelection,
    surface_features: &[crate::records::Feature],
) -> Result<bool, cadmpeg_ir::NativeConvertError> {
    let _reservation = admit_validation_candidates(
        ctx,
        selection_payload_span(lane, record.offset),
        "validate SLDPRT surface selection candidates",
    )?;
    // An offset no index can name names no byte of the payload in memory, so
    // the selection states nothing the payload agrees with.
    let matches_payload = match usize::try_from(record.offset) {
        Ok(offset) => crate::resolved_features::selections::surface_reference_matches_at(
            &lane.native_payload,
            offset,
            &record.components,
        ),
        Err(_) => false,
    };
    Ok(!matches_payload
        || crate::resolved_features::component_paths::surface_selection_producer_features(
            &record.components,
            record.terminal_feature_ref.as_deref(),
            surface_features,
        ) != record.producer_feature_refs
        || usize::try_from(record.offset).ok().and_then(|offset| {
            crate::resolved_features::selections::surface_selection_terminal_feature_at(
                &lane.native_payload,
                offset,
                &record.components,
                surface_features,
            )
        }) != record.terminal_feature_ref)
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
