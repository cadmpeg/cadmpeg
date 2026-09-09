// SPDX-License-Identifier: Apache-2.0
//! Format-independent native record types retained from a decoded ASM stream.

#[cfg(feature = "schema")]
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use cadmpeg_ir::ids::{BodyId, CoedgeId, EdgeId, FaceId, ShellId, SurfaceId, VertexId};

/// Source namespaces used to derive native record ids.
pub mod identity;

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
    endpoint_index: EndpointSlot,
}

/// Endpoint selected by a native vertex ownership record.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(try_from = "u8", into = "u8")]
pub enum EndpointSlot {
    /// Start vertex of the owning edge.
    Start,
    /// End vertex of the owning edge.
    End,
}

impl EndpointSlot {
    /// Native endpoint index.
    #[must_use]
    pub const fn code(self) -> u8 {
        match self {
            Self::Start => 0,
            Self::End => 1,
        }
    }
}

impl From<EndpointSlot> for u8 {
    fn from(value: EndpointSlot) -> Self {
        value.code()
    }
}

impl TryFrom<u8> for EndpointSlot {
    type Error = &'static str;

    fn try_from(value: u8) -> Result<Self, Self::Error> {
        match value {
            0 => Ok(Self::Start),
            1 => Ok(Self::End),
            _ => Err("endpoint_index must be 0 or 1"),
        }
    }
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

/// Native sidedness fields stored on one ASM face record.
#[derive(Debug, Clone, PartialEq)]
pub struct FaceSidedness {
    /// Source namespace of the native record.
    pub source_namespace: identity::NativeRecordNamespace,
    /// Source SAB record index.
    pub record_index: u32,
    /// Solved B-rep face carrying the fields.
    pub face: FaceId,
    /// Sense token stored in the native face record before carrier normalization.
    pub native_sense: cadmpeg_ir::topology::Sense,
    /// Whether decoding reversed the native surface carrier orientation.
    pub carrier_flipped: bool,
    /// Conditional containment direction; absence denotes a single-sided face.
    pub containment: Option<FaceContainment>,
}

impl Serialize for FaceSidedness {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        FaceSidednessWire::from(self.clone()).serialize(serializer)
    }
}

impl<'de> Deserialize<'de> for FaceSidedness {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        FaceSidednessWire::deserialize(deserializer)
            .and_then(|w| FaceSidedness::try_from(w).map_err(serde::de::Error::custom))
    }
}

#[cfg(feature = "schema")]
impl JsonSchema for FaceSidedness {
    fn schema_name() -> std::borrow::Cow<'static, str> {
        "FaceSidedness".into()
    }

    fn json_schema(generator: &mut schemars::SchemaGenerator) -> schemars::Schema {
        FaceSidednessWire::json_schema(generator)
    }
}

impl FaceSidedness {
    /// Derive the native record id from its source identity.
    #[must_use]
    pub fn id(&self) -> String {
        self.source_namespace
            .id("face-sidedness", self.record_index)
    }
}

/// Serialized face sidedness with the native and normalized senses.
#[derive(Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
pub struct FaceSidednessWire {
    id: String,
    face: FaceId,
    record_index: u32,
    native_sense: cadmpeg_ir::topology::Sense,
    normalized_sense: cadmpeg_ir::topology::Sense,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    containment: Option<FaceContainment>,
}

impl From<FaceSidedness> for FaceSidednessWire {
    fn from(value: FaceSidedness) -> Self {
        use cadmpeg_ir::topology::Sense;
        let normalized_sense = match (value.native_sense, value.carrier_flipped) {
            (Sense::Forward, true) => Sense::Reversed,
            (Sense::Reversed, true) => Sense::Forward,
            (sense, false) => sense,
        };
        Self {
            id: value.id(),
            face: value.face,
            record_index: value.record_index,
            native_sense: value.native_sense,
            normalized_sense,
            containment: value.containment,
        }
    }
}

impl TryFrom<FaceSidednessWire> for FaceSidedness {
    type Error = String;

    fn try_from(value: FaceSidednessWire) -> Result<Self, Self::Error> {
        let source_namespace = identity::NativeRecordNamespace::from_wire(
            &value.id,
            value.record_index,
            "face-sidedness",
        )?;
        Ok(Self {
            source_namespace,
            record_index: value.record_index,
            face: value.face,
            native_sense: value.native_sense,
            carrier_flipped: value.native_sense != value.normalized_sense,
            containment: value.containment,
        })
    }
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

/// Shape of the evaluated tolerance slot of one tolerant ASM vertex record.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(rename_all = "snake_case")]
pub enum EvaluatedToleranceSlot {
    /// The record ends before the slot; the vertex carries no tolerance.
    Absent,
    /// The slot holds the `-1` unset sentinel; the vertex carries no tolerance.
    Unset,
    /// The slot holds a tolerance, stored on the vertex.
    Evaluated,
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
    /// Shape of the evaluated tolerance slot. The unset sentinel is a marker
    /// rather than a length, so the neutral vertex carries no tolerance and
    /// this record keeps whether the slot was unset or absent.
    evaluated_slot: EvaluatedToleranceSlot,
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
    /// Edge ring or isolated vertex owned by the native wire.
    members: WireMembers [serde(flatten)],
    /// Native side classification.
    side: WireSide,
}

/// Mutually exclusive native wire members.
#[derive(Debug, Clone, PartialEq, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(try_from = "WireMembersWire")]
pub enum WireMembers {
    /// Ordered edges reached from the first coedge; empty when none resolve.
    Edges(Vec<EdgeId>),
    /// Isolated vertex of a wire with no first coedge.
    Vertex(VertexId),
}

impl WireMembers {
    /// Ordered edge membership, empty for an isolated vertex.
    #[must_use]
    pub fn edges(&self) -> &[EdgeId] {
        match self {
            Self::Edges(edges) => edges,
            Self::Vertex(_) => &[],
        }
    }

    /// Isolated vertex membership.
    #[must_use]
    pub fn free_vertex(&self) -> Option<&VertexId> {
        match self {
            Self::Edges(_) => None,
            Self::Vertex(vertex) => Some(vertex),
        }
    }
}

#[derive(Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
struct WireMembersWire {
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    edges: Vec<EdgeId>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    free_vertex: Option<VertexId>,
}

impl Serialize for WireMembers {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        use serde::ser::SerializeMap;

        let mut map = serializer.serialize_map(None)?;
        match self {
            Self::Edges(edges) if !edges.is_empty() => map.serialize_entry("edges", edges)?,
            Self::Edges(_) => {}
            Self::Vertex(vertex) => map.serialize_entry("free_vertex", vertex)?,
        }
        map.end()
    }
}

impl TryFrom<WireMembersWire> for WireMembers {
    type Error = &'static str;

    fn try_from(wire: WireMembersWire) -> Result<Self, Self::Error> {
        match wire.free_vertex {
            None => Ok(Self::Edges(wire.edges)),
            Some(vertex) if wire.edges.is_empty() => Ok(Self::Vertex(vertex)),
            Some(_) => Err("wire edges and free_vertex are mutually exclusive"),
        }
    }
}

native_record! {
    /// Native Design-join key stored on one ASM body record.
    BodyNativeKey, body_native_key, "body-native-key",
    /// Source SAB body record index.
    record_index,
    /// Solved body carrying the key.
    body: BodyId,
    /// Zero-based body-record position within the BREP blob.
    body_ordinal: u32,
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

#[cfg(test)]
mod tests {
    use super::{EndpointSlot, WireMembers};
    use cadmpeg_ir::ids::{EdgeId, VertexId};
    use serde::Deserialize;

    #[test]
    fn tolerant_coedge_curve_sense_requires_the_current_wire_name() {
        let extension = super::TolerantCoedgeExtension::EmbeddedCurve {
            target: None,
            curve_reversed: true,
            payload_token_count: 0,
            parameter_range: None,
        };
        let wire = serde_value::to_value(&extension).unwrap();
        assert_eq!(
            super::TolerantCoedgeExtension::deserialize(wire.clone()).unwrap(),
            extension
        );
        let serde_value::Value::Map(mut fields) = wire else {
            panic!("extension map")
        };
        let value = fields
            .remove(&serde_value::Value::String("curve_reversed".into()))
            .unwrap();
        fields.insert(serde_value::Value::String("flag".into()), value);
        let error = super::TolerantCoedgeExtension::deserialize(serde_value::Value::Map(fields))
            .unwrap_err();
        assert!(error.to_string().contains("curve_reversed"));
    }

    #[test]
    fn body_native_key_requires_explicit_body_ordinal() {
        let key = super::BodyNativeKey {
            source_namespace: super::identity::NativeRecordNamespace::new(crate::ids::IdFormat(
                "f3d",
            )),
            record_index: 17,
            body: cadmpeg_ir::ids::BodyId::mint("f3d:brep:entity#17").unwrap(),
            body_ordinal: 0,
            source_brep: Some("Body1.sab".into()),
            asm_body_key: None,
        };
        let wire = serde_value::to_value(&key).unwrap();
        assert_eq!(
            super::BodyNativeKey::deserialize(wire.clone()).unwrap(),
            key
        );
        let serde_value::Value::Map(mut fields) = wire else {
            panic!("record map")
        };
        assert_eq!(
            fields.remove(&serde_value::Value::String("body_ordinal".into())),
            Some(serde_value::Value::U32(0))
        );
        let error = super::BodyNativeKey::deserialize(serde_value::Value::Map(fields)).unwrap_err();
        assert!(error.to_string().contains("body_ordinal"));
    }

    #[test]
    fn endpoint_slot_preserves_numeric_wire_and_rejects_other_indices() {
        for (slot, code) in [(EndpointSlot::Start, 0), (EndpointSlot::End, 1)] {
            let wire = serde_value::to_value(slot).expect("serialize endpoint");
            assert_eq!(wire, serde_value::Value::U8(code));
            assert_eq!(
                EndpointSlot::deserialize(wire).expect("read endpoint"),
                slot
            );
        }
        for code in [2, u8::MAX] {
            let error = EndpointSlot::deserialize(serde_value::Value::U8(code))
                .expect_err("undefined endpoint");
            assert!(error.to_string().contains("endpoint_index"));
        }
    }

    #[test]
    fn wire_members_preserve_flat_fields_and_reject_mixed_membership() {
        use serde_value::Value;

        let edge = EdgeId::mint("asm:test:edge#1").expect("edge id");
        let vertex = VertexId::mint("asm:test:vertex#2").expect("vertex id");
        let edge_value = serde_value::to_value(&edge).expect("edge wire");
        let vertex_value = serde_value::to_value(&vertex).expect("vertex wire");
        for (members, fields) in [
            (WireMembers::Edges(Vec::new()), Vec::new()),
            (
                WireMembers::Edges(vec![edge]),
                vec![(
                    Value::String("edges".into()),
                    Value::Seq(vec![edge_value.clone()]),
                )],
            ),
            (
                WireMembers::Vertex(vertex),
                vec![(Value::String("free_vertex".into()), vertex_value.clone())],
            ),
        ] {
            let wire = Value::Map(fields.into_iter().collect());
            assert_eq!(
                serde_value::to_value(&members).expect("serialize members"),
                wire
            );
            assert_eq!(
                WireMembers::deserialize(wire).expect("read members"),
                members
            );
        }
        let mixed = Value::Map(
            [
                (Value::String("edges".into()), Value::Seq(vec![edge_value])),
                (Value::String("free_vertex".into()), vertex_value),
            ]
            .into_iter()
            .collect(),
        );
        let error = WireMembers::deserialize(mixed).expect_err("mixed wire members");
        assert!(error.to_string().contains("edges and free_vertex"));
    }
}
