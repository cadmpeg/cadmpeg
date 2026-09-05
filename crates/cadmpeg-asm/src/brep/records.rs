// SPDX-License-Identifier: Apache-2.0
//! Format-independent native record types retained from a decoded ASM stream.

#[cfg(feature = "schema")]
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use cadmpeg_ir::ids::{BodyId, CoedgeId, EdgeId, FaceId, ShellId, SurfaceId, VertexId};

/// Source namespaces used to derive native record ids.
pub mod identity;

fn is_false(value: &bool) -> bool {
    !*value
}

macro_rules! native_record {
    (
        $(#[doc = $record_doc:literal])*
        $name:ident, $wire:ident, $kind:literal,
        #[doc = $index_doc:literal]
        record_index,
        $(#[doc = $entity_doc:literal])*
        $entity:ident: $entity_ty:ty,
        $(
            $(#[doc = $field_doc:literal])*
            $field:ident: $field_ty:ty $([$($wire_attr:meta),*])?,
        )*
    ) => {
        $(#[doc = $record_doc])*
        #[derive(Debug, Clone, PartialEq)]
        pub struct $name {
            /// Source namespace of the native record.
            pub source_namespace: identity::NativeRecordNamespace,
            #[doc = $index_doc]
            pub record_index: u32,
            $(#[doc = $entity_doc])*
            pub $entity: $entity_ty,
            $(
                $(#[doc = $field_doc])*
                pub $field: $field_ty,
            )*
        }

        impl $name {
            /// Derive the native record id from its source identity.
            #[must_use]
            pub fn id(&self) -> String {
                self.source_namespace.id($kind, self.record_index)
            }
        }

        mod $wire {
            use super::*;

            $(#[doc = $record_doc])*
            #[derive(Deserialize)]
            #[cfg_attr(feature = "schema", derive(JsonSchema))]
            pub(super) struct Wire {
                /// Globally unique deterministic identifier for this native record.
                pub id: String,
                $(#[doc = $entity_doc])*
                pub $entity: $entity_ty,
                #[doc = $index_doc]
                pub record_index: u32,
                $(
                    $(#[doc = $field_doc])*
                    $($(#[$wire_attr])*)?
                    pub $field: $field_ty,
                )*
            }
        }

        impl Serialize for $name {
            fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
                #[derive(Serialize)]
                struct Wire<'a> {
                    id: String,
                    $entity: &'a $entity_ty,
                    record_index: u32,
                    $(
                        $($(#[$wire_attr])*)?
                        $field: &'a $field_ty,
                    )*
                }
                Wire {
                    id: self.id(),
                    $entity: &self.$entity,
                    record_index: self.record_index,
                    $($field: &self.$field,)*
                }.serialize(serializer)
            }
        }

        impl<'de> Deserialize<'de> for $name {
            fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
                let wire = $wire::Wire::deserialize(deserializer)?;
                let source_namespace = identity::NativeRecordNamespace::from_wire(&wire.id, wire.record_index, $kind)
                    .map_err(serde::de::Error::custom)?;
                Ok(Self {
                    source_namespace,
                    record_index: wire.record_index,
                    $entity: wire.$entity,
                    $($field: wire.$field,)*
                })
            }
        }

        #[cfg(feature = "schema")]
        impl JsonSchema for $name {
            fn schema_name() -> std::borrow::Cow<'static, str> {
                stringify!($name).into()
            }

            fn json_schema(generator: &mut schemars::SchemaGenerator) -> schemars::Schema {
                $wire::Wire::json_schema(generator)
            }
        }
    };
}

native_record! {
    /// Kernel continuity classification stored on one solved ASM edge record.
    EdgeContinuity, edge_continuity, "edge-continuity",
    /// Source SAB record index.
    record_index,
    /// Solved B-rep edge carrying the classification.
    edge: EdgeId,
    /// Native curve-parameterization sense before IR carrier normalization.
    sense: cadmpeg_ir::topology::Sense,
    /// Native continuity token, normally `tangent` or `unknown`.
    continuity: String,
}

native_record! {
    /// Native owner-coedge selector stored on one ASM edge record.
    EdgeOwnership, edge_ownership, "edge-ownership",
    /// Source SAB record index.
    record_index,
    /// Solved B-rep edge carrying the selector.
    edge: EdgeId,
    /// Selected coedge, or null when the native edge has no owner back-reference.
    owner_coedge: Option<CoedgeId> [serde(default, skip_serializing_if = "Option::is_none")],
}

native_record! {
    /// Native owner-edge and endpoint-slot fields stored on one ASM vertex.
    VertexOwnership, vertex_ownership, "vertex-ownership",
    /// Source SAB record index.
    record_index,
    /// Solved B-rep vertex carrying the fields.
    vertex: VertexId,
    /// Edge selected as this vertex record's native owner.
    owning_edge: EdgeId,
    /// Endpoint slot on `owning_edge`: `0` for start, `1` for end.
    endpoint_index: u8,
}

/// Conditional containment direction on a double-sided ASM face.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(rename_all = "snake_case")]
pub enum FaceContainment {
    /// The face bounds the inside side of its surface.
    In,
    /// The face bounds the outside side of its surface.
    Out,
}

native_record! {
    /// Native sidedness fields stored on one ASM face record.
    FaceSidedness, face_sidedness, "face-sidedness",
    /// Source SAB record index.
    record_index,
    /// Solved B-rep face carrying the fields.
    face: FaceId,
    /// Sense token stored in the native face record before carrier normalization.
    native_sense: cadmpeg_ir::topology::Sense,
    /// IR sense produced when `native_sense` was decoded.
    normalized_sense: cadmpeg_ir::topology::Sense,
    /// Conditional containment direction; absence denotes a single-sided face.
    containment: Option<FaceContainment> [serde(default, skip_serializing_if = "Option::is_none")],
}

native_record! {
    /// Native Design-join key stored on one ASM face record.
    FaceNativeKey, face_native_key, "face-native-key",
    /// Source SAB face record index.
    record_index,
    /// Solved face carrying the key.
    face: FaceId,
    /// Non-negative Design-join key; absence is the native `-1` null value.
    asm_face_key: Option<u64> [serde(default, skip_serializing_if = "Option::is_none")],
}

native_record! {
    /// Native leading tolerance slots retained from one tolerant ASM vertex
    /// record. The record's three f64 tolerance slots are three independent
    /// tolerance evaluations, each using `-1` as its unset sentinel; the third
    /// slot is the effective vertex tolerance and is stored on the vertex, while
    /// the first two are retained here verbatim.
    TolerantVertexTail, tolerant_vertex_tail, "tolerant-vertex-tail",
    /// Source SAB record index.
    record_index,
    /// Solved B-rep vertex carrying the tolerant record.
    vertex: VertexId,
    /// The first two independent tolerance evaluations, retained verbatim in
    /// native centimetres; `-1` denotes an unset evaluation.
    leading_tolerances: [f64; 2],
    /// Version-gated trailing LONG following the evaluated tolerance,
    /// retained verbatim; absent in older streams, a small non-negative
    /// per-entity change counter when present.
    trailing_field: Option<i64> [serde(default, skip_serializing_if = "Option::is_none")],
    /// Whether the evaluated tolerance slot holds the `-1` unset sentinel.
    /// The sentinel is a marker rather than a length, so the neutral vertex
    /// carries no tolerance and this record keeps the fact.
    evaluated_unset: bool [serde(default, skip_serializing_if = "is_false")],
}

native_record! {
    /// Native tail retained from one tolerant ASM edge record: the entity
    /// serializer revision stamp followed by a version-gated LONG.
    TolerantEdgeTail, tolerant_edge_tail, "tolerant-edge-tail",
    /// Source SAB record index.
    record_index,
    /// Solved B-rep edge carrying the tolerant record.
    edge: EdgeId,
    /// Per-entity serializer revision stamp following the model-space
    /// tolerance, matching the stream's revision value space.
    entity_revision: i64,
    /// Version-gated trailing LONG following the revision stamp, retained
    /// verbatim; absent in older streams, a small non-negative per-entity
    /// change counter when present.
    trailing_field: Option<i64>,
}

native_record! {
    /// Parameter interval stored by one tolerant ASM coedge.
    TolerantCoedgeParameters, tolerant_coedge_parameters, "tolerant-coedge-parameters",
    /// Source SAB record index.
    record_index,
    /// Solved B-rep coedge carrying the tolerant interval.
    coedge: CoedgeId,
    /// Native start and end parameters following the base coedge fields.
    parameter_range: [f64; 2],
    /// Release-selected fixed fields following the parameter interval.
    extension: TolerantCoedgeExtension [serde(default)],
}

/// Release-selected fixed fields following a tolerant-coedge parameter interval.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(rename_all = "snake_case", tag = "layout")]
pub enum TolerantCoedgeExtension {
    /// Releases below 215 have no fixed extension fields.
    #[default]
    None,
    /// Releases 215 through 219 carry one nullable entity reference.
    Reference {
        /// Referenced record index; `None` is the native null reference.
        target: Option<i64>,
    },
    /// Modern releases carry no embedded tolerant-curve payload.
    Empty {
        /// Nullable record reference preceding the zero selector.
        target: Option<i64>,
    },
    /// Modern releases carry one balanced embedded tolerant-curve payload.
    EmbeddedCurve {
        /// Nullable record reference preceding the one selector.
        target: Option<i64>,
        /// Whether the embedded intcurve is evaluated with parameter negation.
        #[serde(alias = "flag")]
        curve_reversed: bool,
        /// Number of tokens inside the balanced outer subtype delimiters.
        payload_token_count: u32,
        /// Optional parameter interval following the embedded subtype.
        parameter_range: Option<[f64; 2]>,
    },
}

native_record! {
    /// Zero-payload ASM surface sentinel whose shape is supplied only by tessellation attributes.
    MeshSurfaceSentinel, mesh_surface_sentinel, "mesh-surface-sentinel",
    /// Source SAB record index.
    record_index,
    /// Unknown exact-surface placeholder emitted for the sentinel record.
    surface: SurfaceId,
}

/// Native side classification stored on an ASM wire record.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(rename_all = "snake_case")]
pub enum WireSide {
    /// Wire bounds the inside side.
    In,
    /// Wire bounds the outside side.
    Out,
}

native_record! {
    /// Native wire record projected onto one neutral-IR shell.
    WireTopology, wire_topology, "wire-topology",
    /// Source SAB record index.
    record_index,
    /// Neutral shell containing the wire.
    shell: ShellId,
    /// Ordered edge ring owned through the wire's first-coedge reference.
    edges: Vec<EdgeId> [serde(default, skip_serializing_if = "Vec::is_empty")],
    /// Isolated vertex owned when the first-coedge reference is null.
    free_vertex: Option<VertexId> [serde(default, skip_serializing_if = "Option::is_none")],
    /// Native side classification.
    side: WireSide,
}

native_record! {
    /// Native Design-join key stored on one ASM body record.
    BodyNativeKey, body_native_key, "body-native-key",
    /// Source SAB body record index.
    record_index,
    /// Solved body carrying the key.
    body: BodyId,
    /// Zero-based body-record position within the BREP blob.
    body_ordinal: u32 [serde(default)],
    /// Basename of the BREP blob containing this body.
    source_brep: Option<String> [serde(default, skip_serializing_if = "Option::is_none")],
    /// Non-negative Design-join key; absence is the native `-1` null value.
    asm_body_key: Option<u64> [serde(default, skip_serializing_if = "Option::is_none")],
}

native_record! {
    /// Native rotation, reflection, and shear classifications on an ASM transform.
    TransformHints, transform_hints, "transform-hints",
    /// Source SAB transform record index.
    record_index,
    /// Solved body referencing the transform record.
    body: BodyId,
    /// The linear transform includes rotation.
    rotation: bool,
    /// The linear transform includes reflection.
    reflection: bool,
    /// The linear transform includes shear.
    shear: bool,
}
