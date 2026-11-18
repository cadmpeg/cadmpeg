// SPDX-License-Identifier: Apache-2.0
//! Named contents an occurrence can show in place of its prototype's full
//! content (CADIR v11 §8.1).

use cadmpeg_core::text::NonBlankString;
use cadmpeg_derive::DeserializeName;
#[cfg(feature = "schema")]
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use super::ExternalDocument;
use crate::design::selection::OccurrencePath;
use crate::features::FeatureId;
use crate::ids::BodyId;

/// A named content of a definition.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(deny_unknown_fields)]
pub struct Representation {
    /// The representation's name, by which an occurrence names it.
    pub name: NonBlankString,
    /// What the representation shows.
    pub content: RepresentationContent,
    /// The datum and curve features the representation also shows.
    pub features: Vec<FeatureId>,
    /// `Some(true)` for the one the source shows by default.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_default"
    )]
    pub default: Option<bool>,
    /// Format-native object supplying this representation.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_native_ref"
    )]
    pub native_ref: Option<String>,
}

/// What a representation shows.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(
    tag = "kind",
    content = "value",
    rename_all = "snake_case",
    deny_unknown_fields
)]
pub enum RepresentationContent {
    /// A subset of the definition's bodies.
    Bodies(Vec<BodyId>),
    /// A rule over the instances inside the definition.
    Members(MemberRule),
    /// An alternate document that stands for the definition.
    Document(ExternalDocument),
    /// A content the decoder did not read.
    #[serde(deserialize_with = "cadmpeg_core::tag_only::refuse_value")]
    Unread,
}

/// A rule over the instances inside a definition.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(deny_unknown_fields)]
pub struct MemberRule {
    /// The action applied to an instance no member names; `None` does not
    /// state it.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_unlisted"
    )]
    pub unlisted: Option<MemberAction>,
    /// The instances the rule names.
    pub members: Vec<RepresentationMember>,
}

/// One instance a member rule names, and what the rule does with it.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(deny_unknown_fields)]
pub struct RepresentationMember {
    /// The instance, by its path from the definition.
    pub path: OccurrencePath,
    /// What the representation does with it.
    pub action: MemberAction,
}

/// What a representation does with an instance.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(
    tag = "kind",
    content = "value",
    rename_all = "snake_case",
    deny_unknown_fields
)]
pub enum MemberAction {
    /// The representation includes the instance.
    #[serde(deserialize_with = "cadmpeg_core::tag_only::refuse_value")]
    Include,
    /// The representation excludes the instance.
    #[serde(deserialize_with = "cadmpeg_core::tag_only::refuse_value")]
    Exclude,
    /// The instance shows the named representation of its prototype.
    Substitute(NonBlankString),
    /// The instance shows as these other occurrences of the definition.
    ReplaceBy(Vec<OccurrencePath>),
    /// The instance shows at a reduced level of detail.
    Detail(RepresentationDetail),
    /// An action whose stored code is not read.
    Native(NonBlankString),
}

/// A reduced level of detail of an instance.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, DeserializeName)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub enum RepresentationDetail {
    /// Its complete geometry.
    Geometry,
    /// Display data only.
    Graphics,
    /// Light-weight display data.
    LightGraphics,
    /// A symbol.
    Symbol,
    /// Its bounding box.
    BoundingBox,
    /// Its default envelope.
    Envelope,
}

// Each optional key below names itself in whatever it refuses.
cadmpeg_core::named_optional_field!(deserialize_default, bool, "default");
cadmpeg_core::named_optional_field!(deserialize_native_ref, String, "native_ref");
cadmpeg_core::named_optional_field!(deserialize_unlisted, MemberAction, "unlisted");
