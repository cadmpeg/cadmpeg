// SPDX-License-Identifier: Apache-2.0
//! Structured names: how the design produced an entity.
//!
//! An entity's record id persists only while the source's kernel keeps the
//! entity. A name survives a rebuild by another application. A structured
//! name states meaning and a discriminator among entities of equal meaning;
//! the source's own allocations live only in [`NameEntry::native`].

use std::collections::{BTreeMap, BTreeSet};

use base64::engine::general_purpose::STANDARD;
use base64::Engine as _;
use cadmpeg_core::text::NonBlankString;
#[cfg(feature = "schema")]
use schemars::JsonSchema;
use serde::de::Error as _;
use serde::{Deserialize, Deserializer, Serialize, Serializer};

use super::ids::ContainerId;
use super::read::{enter_de, Ceiling};
use super::refs::EntityRef;
use crate::features::FeatureId;
use crate::sketches::{SketchEntityId, SpatialSketchEntityId};

/// A format id of the dialect registry.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(transparent)]
pub struct CodecId(NonBlankString);

impl CodecId {
    /// The format id.
    #[must_use]
    pub fn as_str(&self) -> &str {
        self.0.as_str()
    }
}

impl From<NonBlankString> for CodecId {
    fn from(value: NonBlankString) -> Self {
        Self(value)
    }
}

/// A byte string, written in RFC 4648 base64.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct Base64Bytes(pub Vec<u8>);

impl Serialize for Base64Bytes {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(&STANDARD.encode(&self.0))
    }
}

impl<'de> Deserialize<'de> for Base64Bytes {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let text = String::deserialize(deserializer)?;
        let bytes = STANDARD
            .decode(&text)
            .ok()
            .filter(|bytes| STANDARD.encode(bytes) == text)
            .ok_or_else(|| D::Error::custom("not canonical RFC 4648 base64"))?;
        Ok(Self(bytes))
    }
}

#[cfg(feature = "schema")]
impl JsonSchema for Base64Bytes {
    fn schema_name() -> std::borrow::Cow<'static, str> {
        "Base64Bytes".into()
    }

    fn json_schema(_: &mut schemars::SchemaGenerator) -> schemars::Schema {
        schemars::json_schema!({ "type": "string", "contentEncoding": "base64" })
    }
}

/// The names of the entities of one state, keyed by entity.
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(transparent)]
pub struct NameTable(
    #[serde(with = "super::wire::pairs")]
    #[cfg_attr(feature = "schema", schemars(with = "Vec<(EntityRef, NameEntry)>"))]
    BTreeMap<EntityRef, NameEntry>,
);

impl NameTable {
    /// The entries, in entity order.
    #[must_use]
    pub const fn entries(&self) -> &BTreeMap<EntityRef, NameEntry> {
        &self.0
    }
}

impl From<BTreeMap<EntityRef, NameEntry>> for NameTable {
    fn from(entries: BTreeMap<EntityRef, NameEntry>) -> Self {
        Self(entries)
    }
}

/// The names of one entity. An entry holds at least one of `name` and
/// `native`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(deny_unknown_fields)]
pub struct NameEntry {
    /// The structured name. `None` where no feature of the document produced
    /// the entity, or where the source name names no feature.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "super::wire::present"
    )]
    pub name: Option<TopoName>,
    /// The source's own name in full.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "super::wire::present"
    )]
    pub native: Option<NativeName>,
}

/// The source's own name for an entity or a state, tagged with its codec.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(deny_unknown_fields)]
pub struct NativeName {
    /// The codec whose source states the name.
    pub codec: CodecId,
    /// The name as the source stores it.
    pub value: NativeNameValue,
}

/// The stored form of a native name.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(
    tag = "kind",
    content = "value",
    rename_all = "snake_case",
    deny_unknown_fields
)]
pub enum NativeNameValue {
    /// A text name.
    Text(NonBlankString),
    /// A name of integers.
    Integers(Vec<i64>),
    /// A name of bytes.
    Bytes(Base64Bytes),
}

/// The coordinates of one instance of a pattern.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(transparent)]
pub struct InstanceIndex(pub Vec<i32>);

/// A structured name: the meaning of an entity and a discriminator among
/// entities of equal meaning. Equality is structural; a name contains no
/// coordinate and no source allocation.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(
    tag = "kind",
    content = "value",
    rename_all = "snake_case",
    deny_unknown_fields
)]
pub enum TopoName {
    /// An entity that a feature generates.
    Generated {
        /// The kind of the entity.
        kind: NameKind,
        /// The feature that produced the entity.
        feature: FeatureId,
        /// The entity's role in the feature. `None` where the source names
        /// the producing feature and no role.
        #[serde(
            default,
            skip_serializing_if = "Option::is_none",
            deserialize_with = "super::wire::present"
        )]
        role: Option<GenerationRole>,
        /// The entity that generated this one.
        #[serde(
            default,
            skip_serializing_if = "Option::is_none",
            deserialize_with = "super::wire::present"
        )]
        generator: Option<Generator>,
        /// The source's own discriminator among entities of equal meaning,
        /// stated only where the source stores one.
        #[serde(
            default,
            skip_serializing_if = "Option::is_none",
            deserialize_with = "super::wire::present"
        )]
        index: Option<u32>,
    },
    /// A copy that a feature made, by its instance coordinates.
    Instance {
        /// The feature that made the copy.
        feature: FeatureId,
        /// The copy's instance coordinates.
        instance: InstanceIndex,
        /// The name of the entity copied.
        of: Box<TopoName>,
    },
    /// A fillet or chamfer face.
    Blend {
        /// The blend feature.
        feature: FeatureId,
        /// The faces the blend joins, only where the source stores them as
        /// the name.
        between: Vec<TopoName>,
        /// The source's own discriminator among entities of equal meaning.
        #[serde(
            default,
            skip_serializing_if = "Option::is_none",
            deserialize_with = "super::wire::present"
        )]
        index: Option<u32>,
    },
    /// An edge, by the faces beside it and the faces at its ends.
    Bounded {
        /// The faces beside the edge.
        #[serde(with = "super::wire::set")]
        #[cfg_attr(feature = "schema", schemars(with = "Vec<TopoName>"))]
        faces: BTreeSet<TopoName>,
        /// The faces at the edge's ends, where the source records them.
        #[serde(with = "super::wire::set")]
        #[cfg_attr(feature = "schema", schemars(with = "Vec<TopoName>"))]
        ends: BTreeSet<TopoName>,
        /// The source's own discriminator among entities of equal meaning.
        #[serde(
            default,
            skip_serializing_if = "Option::is_none",
            deserialize_with = "super::wire::present"
        )]
        index: Option<u32>,
    },
    /// A vertex, by the faces that meet at it.
    Meeting {
        /// The faces that meet at the vertex.
        #[serde(with = "super::wire::set")]
        #[cfg_attr(feature = "schema", schemars(with = "Vec<TopoName>"))]
        faces: BTreeSet<TopoName>,
        /// The source's own discriminator among entities of equal meaning.
        #[serde(
            default,
            skip_serializing_if = "Option::is_none",
            deserialize_with = "super::wire::present"
        )]
        index: Option<u32>,
    },
    /// An entity that a feature modified.
    Modified {
        /// The feature that modified the entity.
        feature: FeatureId,
        /// The name of the entity before the modification.
        from: Box<TopoName>,
    },
    /// A piece of a split entity.
    Split {
        /// The feature that split the entity, where the source stores it.
        #[serde(
            default,
            skip_serializing_if = "Option::is_none",
            deserialize_with = "super::wire::present"
        )]
        feature: Option<FeatureId>,
        /// The name of the entity that was split.
        from: Box<TopoName>,
        /// The piece, where the source states one.
        #[serde(
            default,
            skip_serializing_if = "Option::is_none",
            deserialize_with = "super::wire::present"
        )]
        piece: Option<u32>,
    },
    /// An entity merged from others of one kind.
    Merged {
        /// The feature that merged the entities, where the source stores it.
        #[serde(
            default,
            skip_serializing_if = "Option::is_none",
            deserialize_with = "super::wire::present"
        )]
        feature: Option<FeatureId>,
        /// The names of the merged entities.
        #[serde(with = "super::wire::set")]
        #[cfg_attr(feature = "schema", schemars(with = "Vec<TopoName>"))]
        from: BTreeSet<TopoName>,
    },
    /// A body whose identity is its container.
    Root {
        /// The container.
        container: ContainerId,
    },
}

/// The reading form of [`TopoName`]. Each nested name reads through
/// [`TopoName`]'s reader, which holds one level of the name ceiling.
#[derive(Deserialize)]
#[serde(
    tag = "kind",
    content = "value",
    rename_all = "snake_case",
    deny_unknown_fields
)]
enum TopoNameWire {
    Generated {
        kind: NameKind,
        feature: FeatureId,
        #[serde(default, deserialize_with = "super::wire::present")]
        role: Option<GenerationRole>,
        #[serde(default, deserialize_with = "super::wire::present")]
        generator: Option<Generator>,
        #[serde(default, deserialize_with = "super::wire::present")]
        index: Option<u32>,
    },
    Instance {
        feature: FeatureId,
        instance: InstanceIndex,
        of: Box<TopoName>,
    },
    Blend {
        feature: FeatureId,
        between: Vec<TopoName>,
        #[serde(default, deserialize_with = "super::wire::present")]
        index: Option<u32>,
    },
    Bounded {
        #[serde(deserialize_with = "super::wire::set::deserialize")]
        faces: BTreeSet<TopoName>,
        #[serde(deserialize_with = "super::wire::set::deserialize")]
        ends: BTreeSet<TopoName>,
        #[serde(default, deserialize_with = "super::wire::present")]
        index: Option<u32>,
    },
    Meeting {
        #[serde(deserialize_with = "super::wire::set::deserialize")]
        faces: BTreeSet<TopoName>,
        #[serde(default, deserialize_with = "super::wire::present")]
        index: Option<u32>,
    },
    Modified {
        feature: FeatureId,
        from: Box<TopoName>,
    },
    Split {
        #[serde(default, deserialize_with = "super::wire::present")]
        feature: Option<FeatureId>,
        from: Box<TopoName>,
        #[serde(default, deserialize_with = "super::wire::present")]
        piece: Option<u32>,
    },
    Merged {
        #[serde(default, deserialize_with = "super::wire::present")]
        feature: Option<FeatureId>,
        #[serde(deserialize_with = "super::wire::set::deserialize")]
        from: BTreeSet<TopoName>,
    },
    Root {
        container: ContainerId,
    },
}

impl From<TopoNameWire> for TopoName {
    fn from(wire: TopoNameWire) -> Self {
        match wire {
            TopoNameWire::Generated {
                kind,
                feature,
                role,
                generator,
                index,
            } => Self::Generated {
                kind,
                feature,
                role,
                generator,
                index,
            },
            TopoNameWire::Instance {
                feature,
                instance,
                of,
            } => Self::Instance {
                feature,
                instance,
                of,
            },
            TopoNameWire::Blend {
                feature,
                between,
                index,
            } => Self::Blend {
                feature,
                between,
                index,
            },
            TopoNameWire::Bounded { faces, ends, index } => Self::Bounded { faces, ends, index },
            TopoNameWire::Meeting { faces, index } => Self::Meeting { faces, index },
            TopoNameWire::Modified { feature, from } => Self::Modified { feature, from },
            TopoNameWire::Split {
                feature,
                from,
                piece,
            } => Self::Split {
                feature,
                from,
                piece,
            },
            TopoNameWire::Merged { feature, from } => Self::Merged { feature, from },
            TopoNameWire::Root { container } => Self::Root { container },
        }
    }
}

impl<'de> Deserialize<'de> for TopoName {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let _level = enter_de::<D::Error>(Ceiling::Name)?;
        TopoNameWire::deserialize(deserializer).map(Self::from)
    }
}

/// The kind of a named entity. `Surface` and `Curve` names identify carriers.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub enum NameKind {
    /// A body.
    Body,
    /// A surface carrier.
    Surface,
    /// A face.
    Face,
    /// A curve carrier.
    Curve,
    /// An edge.
    Edge,
    /// A vertex.
    Vertex,
}

/// The entity that generated a named entity.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(
    tag = "kind",
    content = "value",
    rename_all = "snake_case",
    deny_unknown_fields
)]
pub enum Generator {
    /// A sketch curve or a sketch point.
    SketchEntity(SketchEntityId),
    /// A path's spatial-sketch entity.
    SpatialSketchEntity(SpatialSketchEntityId),
    /// The entity that this one derives from, by its name.
    Entity(Box<TopoName>),
}

/// The role of a generated entity in the feature that produced it.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(
    tag = "kind",
    content = "value",
    rename_all = "snake_case",
    deny_unknown_fields
)]
pub enum GenerationRole {
    /// The cap of a swept solid at the end of the span against the sweep's
    /// direction.
    Start,
    /// The cap of a swept solid along the sweep's direction.
    End,
    /// A face swept from a profile entity.
    Side,
    /// An edge or face that a closed entity closes on.
    Seam,
    /// The body.
    Body,
    /// A face of a hole, by its place in the hole's shape.
    Hole(HoleFaceRole),
    /// A source role code whose meaning is not established, written
    /// `codec:code`.
    Other(NonBlankString),
}

/// The place of a face in a hole's shape.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub enum HoleFaceRole {
    /// The bore's wall, a tapered bore's cone included.
    BoreWall,
    /// The bore's flat or drill-point bottom.
    Bottom,
    /// A counterbore's or spotface's wall.
    HeadWall,
    /// A counterbore's or spotface's floor.
    HeadFloor,
    /// A countersink's cone or a counterdrill's entry cone.
    HeadCone,
}
