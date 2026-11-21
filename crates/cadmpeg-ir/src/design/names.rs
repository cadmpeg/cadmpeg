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
use cadmpeg_derive::DeserializeName;
#[cfg(feature = "schema")]
use schemars::JsonSchema;
use serde::{Deserialize, Serialize, Serializer};

use super::ids::ContainerId;
use super::read::{self, Ceiling};
use super::refs::EntityRef;
use crate::features::FeatureId;
use crate::sketches::{SketchEntityId, SpatialSketchEntityId};

/// A format id of the dialect registry.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
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
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Deserialize)]
#[serde(try_from = "String")]
pub struct Base64Bytes(pub Vec<u8>);

impl Serialize for Base64Bytes {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(&STANDARD.encode(&self.0))
    }
}

impl TryFrom<String> for Base64Bytes {
    type Error = &'static str;

    /// Admits canonical RFC 4648 base64: the text that encoding its bytes
    /// writes.
    fn try_from(text: String) -> Result<Self, Self::Error> {
        STANDARD
            .decode(&text)
            .ok()
            .filter(|bytes| STANDARD.encode(bytes) == text)
            .map(Self)
            .ok_or("not canonical RFC 4648 base64")
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
    #[serde(with = "cadmpeg_core::distinct_keys::pairs")]
    #[cfg_attr(
        feature = "schema",
        schemars(with = "BTreeSet<(EntityRef, NameEntry)>")
    )]
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
        deserialize_with = "deserialize_name"
    )]
    pub name: Option<TopoName>,
    /// The source's own name in full.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_native"
    )]
    pub native: Option<NativeName>,
}

/// The source's own name for an entity or a state, tagged with its codec.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(deny_unknown_fields)]
pub struct NativeName {
    /// The codec whose source states the name.
    pub codec: CodecId,
    /// The name as the source stores it.
    pub value: NativeNameValue,
}

/// The stored form of a native name.
#[derive(
    Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, cadmpeg_derive::DeserializeTagged,
)]
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
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[cfg_attr(feature = "schema", schemars(with = "TopoNameWire"))]
#[serde(
    tag = "kind",
    content = "value",
    rename_all = "snake_case",
    deny_unknown_fields,
    try_from = "TopoNameWire"
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
        #[serde(skip_serializing_if = "Option::is_none")]
        role: Option<GenerationRole>,
        /// The entity that generated this one.
        #[serde(skip_serializing_if = "Option::is_none")]
        generator: Option<Generator>,
        /// The source's own discriminator among entities of equal meaning,
        /// stated only where the source stores one.
        #[serde(skip_serializing_if = "Option::is_none")]
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
        #[serde(skip_serializing_if = "Option::is_none")]
        index: Option<u32>,
    },
    /// An edge, by the faces beside it and the faces at its ends.
    Bounded {
        /// The faces beside the edge.
        faces: BTreeSet<TopoName>,
        /// The faces at the edge's ends, where the source records them.
        ends: BTreeSet<TopoName>,
        /// The source's own discriminator among entities of equal meaning.
        #[serde(skip_serializing_if = "Option::is_none")]
        index: Option<u32>,
    },
    /// A vertex, by the faces that meet at it.
    Meeting {
        /// The faces that meet at the vertex.
        faces: BTreeSet<TopoName>,
        /// The source's own discriminator among entities of equal meaning.
        #[serde(skip_serializing_if = "Option::is_none")]
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
        #[serde(skip_serializing_if = "Option::is_none")]
        feature: Option<FeatureId>,
        /// The name of the entity that was split.
        from: Box<TopoName>,
        /// The piece, where the source states one.
        #[serde(skip_serializing_if = "Option::is_none")]
        piece: Option<u32>,
    },
    /// An entity merged from others of one kind.
    Merged {
        /// The feature that merged the entities, where the source stores it.
        #[serde(skip_serializing_if = "Option::is_none")]
        feature: Option<FeatureId>,
        /// The names of the merged entities.
        from: BTreeSet<TopoName>,
    },
    /// A body whose identity is its container.
    Root {
        /// The container.
        container: ContainerId,
    },
}

/// The wire form of [`TopoName`]. Each nested name has passed the name
/// ceiling; reading admits this one level above the deepest of them.
#[derive(cadmpeg_derive::DeserializeTagged)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(
    tag = "kind",
    content = "value",
    rename_all = "snake_case",
    deny_unknown_fields
)]
enum TopoNameWire {
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
            deserialize_with = "deserialize_role"
        )]
        role: Option<GenerationRole>,
        /// The entity that generated this one.
        #[serde(
            default,
            skip_serializing_if = "Option::is_none",
            deserialize_with = "deserialize_generator"
        )]
        generator: Option<Generator>,
        /// The source's own discriminator among entities of equal meaning,
        /// stated only where the source stores one.
        #[serde(
            default,
            skip_serializing_if = "Option::is_none",
            deserialize_with = "deserialize_index"
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
            deserialize_with = "deserialize_index"
        )]
        index: Option<u32>,
    },
    /// An edge, by the faces beside it and the faces at its ends.
    Bounded {
        /// The faces beside the edge.
        #[serde(with = "cadmpeg_core::distinct_keys::set")]
        #[cfg_attr(feature = "schema", schemars(with = "BTreeSet<TopoName>"))]
        faces: BTreeSet<TopoName>,
        /// The faces at the edge's ends, where the source records them.
        #[serde(with = "cadmpeg_core::distinct_keys::set")]
        #[cfg_attr(feature = "schema", schemars(with = "BTreeSet<TopoName>"))]
        ends: BTreeSet<TopoName>,
        /// The source's own discriminator among entities of equal meaning.
        #[serde(
            default,
            skip_serializing_if = "Option::is_none",
            deserialize_with = "deserialize_index"
        )]
        index: Option<u32>,
    },
    /// A vertex, by the faces that meet at it.
    Meeting {
        /// The faces that meet at the vertex.
        #[serde(with = "cadmpeg_core::distinct_keys::set")]
        #[cfg_attr(feature = "schema", schemars(with = "BTreeSet<TopoName>"))]
        faces: BTreeSet<TopoName>,
        /// The source's own discriminator among entities of equal meaning.
        #[serde(
            default,
            skip_serializing_if = "Option::is_none",
            deserialize_with = "deserialize_index"
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
    Split(SplitName),
    /// An entity merged from others of one kind.
    Merged(MergedName),
    /// A body whose identity is its container.
    Root {
        /// The container.
        container: ContainerId,
    },
}

/// The fields of [`TopoName::Split`] on the wire.
#[derive(Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(deny_unknown_fields)]
struct SplitName {
    /// The feature that split the entity, where the source stores it.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_feature"
    )]
    feature: Option<FeatureId>,
    /// The name of the entity that was split.
    from: Box<TopoName>,
    /// The piece, where the source states one.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_piece"
    )]
    piece: Option<u32>,
}

/// The fields of [`TopoName::Merged`] on the wire.
#[derive(Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(deny_unknown_fields)]
struct MergedName {
    /// The feature that merged the entities, where the source stores it.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_feature"
    )]
    feature: Option<FeatureId>,
    /// The names of the merged entities.
    #[serde(with = "cadmpeg_core::distinct_keys::set")]
    #[cfg_attr(feature = "schema", schemars(with = "BTreeSet<TopoName>"))]
    from: BTreeSet<TopoName>,
}

impl TryFrom<TopoNameWire> for TopoName {
    type Error = String;

    /// Admits a name nested no deeper than the name ceiling.
    fn try_from(wire: TopoNameWire) -> Result<Self, Self::Error> {
        let name = match wire {
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
            TopoNameWire::Split(SplitName {
                feature,
                from,
                piece,
            }) => Self::Split {
                feature,
                from,
                piece,
            },
            TopoNameWire::Merged(MergedName { feature, from }) => Self::Merged { feature, from },
            TopoNameWire::Root { container } => Self::Root { container },
        };
        read::within(Ceiling::Name, name.depth())?;
        Ok(name)
    }
}

impl TopoName {
    /// The levels of this name and the names nested in it.
    fn depth(&self) -> u64 {
        let below = match self {
            Self::Generated { generator, .. } => match generator {
                Some(Generator::Entity(name)) => name.depth(),
                Some(Generator::SketchEntity(_) | Generator::SpatialSketchEntity(_)) | None => 0,
            },
            Self::Instance { of: from, .. }
            | Self::Modified { from, .. }
            | Self::Split { from, .. } => from.depth(),
            Self::Blend { between, .. } => deepest(between),
            Self::Bounded { faces, ends, .. } => deepest(faces).max(deepest(ends)),
            Self::Meeting { faces, .. } | Self::Merged { from: faces, .. } => deepest(faces),
            Self::Root { .. } => 0,
        };
        below + 1
    }
}

fn deepest<'a>(names: impl IntoIterator<Item = &'a TopoName>) -> u64 {
    names.into_iter().map(TopoName::depth).max().unwrap_or(0)
}

/// The kind of a named entity. `Surface` and `Curve` names identify carriers.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, DeserializeName)]
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
#[derive(
    Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize, cadmpeg_derive::DeserializeTagged,
)]
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
#[derive(
    Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize, cadmpeg_derive::DeserializeTagged,
)]
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
    #[serde(deserialize_with = "cadmpeg_core::tag_only::refuse_value")]
    Start,
    /// The cap of a swept solid along the sweep's direction.
    #[serde(deserialize_with = "cadmpeg_core::tag_only::refuse_value")]
    End,
    /// A face swept from a profile entity.
    #[serde(deserialize_with = "cadmpeg_core::tag_only::refuse_value")]
    Side,
    /// An edge or face that a closed entity closes on.
    #[serde(deserialize_with = "cadmpeg_core::tag_only::refuse_value")]
    Seam,
    /// The body.
    #[serde(deserialize_with = "cadmpeg_core::tag_only::refuse_value")]
    Body,
    /// A face of a hole, by its place in the hole's shape.
    Hole(HoleFaceRole),
    /// A source role code whose meaning is not established, written
    /// `codec:code`.
    Other(NonBlankString),
}

/// The place of a face in a hole's shape.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, DeserializeName)]
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

cadmpeg_core::named_optional_field!(deserialize_feature, FeatureId, "feature");
cadmpeg_core::named_optional_field!(deserialize_generator, Generator, "generator");
cadmpeg_core::named_optional_field!(deserialize_index, u32, "index");
cadmpeg_core::named_optional_field!(deserialize_name, TopoName, "name");
cadmpeg_core::named_optional_field!(deserialize_native, NativeName, "native");
cadmpeg_core::named_optional_field!(deserialize_piece, u32, "piece");
cadmpeg_core::named_optional_field!(deserialize_role, GenerationRole, "role");
