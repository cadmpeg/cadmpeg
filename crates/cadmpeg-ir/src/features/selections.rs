// SPDX-License-Identifier: Apache-2.0
//! Edge, face, vertex and body selections and their member sets.

use super::membership;
use super::{
    deserialize_selection_edges, deserialize_selection_faces, deserialize_selection_local_id,
    deserialize_selection_native, deserialize_selection_unresolved, FeatureCollectionError,
    FeatureId,
};
use crate::ids::{
    BodyId, EdgeId, FaceId, FeatureInputTopologyId, HistoricalBodyId, HistoricalEdgeId,
    HistoricalFaceId, HistoricalVertexId, OccurrenceId,
};
use cadmpeg_core::text::NonBlankString;
#[cfg(feature = "schema")]
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

/// Edge operands resolved by the decoder or retained in native form.
#[derive(Debug, Clone, PartialEq, Serialize, cadmpeg_derive::DeserializeTagged)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(
    tag = "kind",
    content = "value",
    rename_all = "snake_case",
    deny_unknown_fields
)]
pub enum EdgeSelection {
    /// Selection exists semantically but its operands are not resolved.
    #[serde(deserialize_with = "cadmpeg_core::tag_only::refuse_value")]
    Unresolved,
    /// Every edge of the operation's input body.
    #[serde(deserialize_with = "cadmpeg_core::tag_only::refuse_value")]
    All,
    /// Resolved topological edges.
    Edges(Vec<EdgeId>),
    /// Resolved edges paired with the format-native selection required for rewrite.
    Resolved {
        /// Resolved topological edges.
        edges: Vec<EdgeId>,
        /// Format-native selection reference.
        native: String,
    },
    /// Edges resolved in the containing feature's input topology.
    Historical {
        /// Input topology containing every selected edge.
        state: FeatureInputTopologyId,
        /// State-local edge identities in operand order.
        #[serde(deserialize_with = "deserialize_selection_edges")]
        edges: SelectionMembers<HistoricalEdgeId>,
        /// Format-native selection reference.
        #[serde(deserialize_with = "deserialize_selection_native")]
        native: NonBlankString,
    },
    /// Proven historical edges plus source operands whose edge identity is unresolved.
    /// `edges` is empty when the input state is known but no member identity resolves.
    HistoricalPartial {
        /// Input topology containing every resolved edge.
        state: FeatureInputTopologyId,
        /// Proven state-local edge identities in source operand order.
        #[serde(deserialize_with = "deserialize_selection_edges")]
        edges: DistinctMembers<HistoricalEdgeId>,
        /// Stable native identities of unresolved source operands.
        #[serde(deserialize_with = "deserialize_selection_unresolved")]
        unresolved: NativeSelections,
        /// Format-native group selection reference.
        #[serde(deserialize_with = "deserialize_selection_native")]
        native: NonBlankString,
    },
    /// Edges in intermediate regenerated feature results, paired with the
    /// format-native selection required for rewrite.
    Generated {
        /// Feature-local edge identities.
        #[serde(deserialize_with = "deserialize_selection_edges")]
        edges: NonEmptyMembers<GeneratedEdgeRef>,
        /// Format-native persistent selection reference.
        #[serde(deserialize_with = "deserialize_selection_native")]
        native: SelectionReference,
    },
    /// Format-native selection reference.
    Native(String),
}

/// Persistent identity of an edge in one regenerated feature result.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(deny_unknown_fields)]
pub struct GeneratedEdgeRef {
    /// Feature whose regenerated result owns the edge.
    pub feature: FeatureId,
    /// Feature-local persistent edge identity.
    #[serde(deserialize_with = "deserialize_selection_local_id")]
    pub local_id: SelectionReference,
}

/// Persistent identity of a face in one regenerated feature result.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(deny_unknown_fields)]
pub struct GeneratedFaceRef {
    /// Feature whose regenerated result owns the face.
    pub feature: FeatureId,
    /// Feature-local persistent face identity.
    #[serde(deserialize_with = "deserialize_selection_local_id")]
    pub local_id: SelectionReference,
}

/// Persistent identity of a vertex in one regenerated feature result.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(deny_unknown_fields)]
pub struct GeneratedVertexRef {
    /// Feature whose regenerated result owns the vertex.
    pub feature: FeatureId,
    /// Feature-local persistent vertex identity.
    #[serde(deserialize_with = "deserialize_selection_local_id")]
    pub local_id: SelectionReference,
}

/// Vertex operand resolved by the decoder or retained in native form.
#[derive(Debug, Clone, PartialEq, Serialize, cadmpeg_derive::DeserializeTagged)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(tag = "kind", content = "value", rename_all = "snake_case")]
#[serde(deny_unknown_fields)]
pub enum VertexSelection {
    /// Selection exists semantically but its operand is not resolved.
    #[serde(deserialize_with = "cadmpeg_core::tag_only::refuse_value")]
    Unresolved,
    /// Vertex in an intermediate regenerated feature result, paired with the
    /// format-native selection required for rewrite.
    Generated {
        /// Feature-local vertex identity.
        vertex: GeneratedVertexRef,
        /// Format-native persistent selection reference.
        #[serde(deserialize_with = "deserialize_selection_native")]
        native: SelectionReference,
    },
    /// Vertex resolved in the containing feature's input topology.
    Historical {
        /// Input topology containing the selected vertex.
        state: FeatureInputTopologyId,
        /// State-local vertex identity.
        vertex: HistoricalVertexId,
        /// Format-native persistent selection reference.
        #[serde(deserialize_with = "deserialize_selection_native")]
        native: NonBlankString,
    },
    /// Format-native selection reference.
    Native(#[serde(deserialize_with = "deserialize_selection_native")] SelectionReference),
}

/// Face operands resolved by the decoder or retained in native form.
#[derive(Debug, Clone, PartialEq, Serialize, cadmpeg_derive::DeserializeTagged)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(
    tag = "kind",
    content = "value",
    rename_all = "snake_case",
    deny_unknown_fields
)]
pub enum FaceSelection {
    /// Selection exists semantically but its operands are not resolved.
    #[serde(deserialize_with = "cadmpeg_core::tag_only::refuse_value")]
    Unresolved,
    /// Resolved topological faces; empty for no selected faces.
    Faces(Vec<FaceId>),
    /// Resolved faces paired with the format-native selection required for rewrite.
    Resolved {
        /// Resolved topological faces.
        faces: Vec<FaceId>,
        /// Format-native selection reference.
        native: String,
    },
    /// Faces resolved in the containing feature's input topology.
    Historical {
        /// Input topology containing every selected face.
        state: FeatureInputTopologyId,
        /// State-local face identities in operand order.
        #[serde(deserialize_with = "deserialize_selection_faces")]
        faces: SelectionMembers<HistoricalFaceId>,
        /// Format-native selection reference.
        #[serde(deserialize_with = "deserialize_selection_native")]
        native: NonBlankString,
    },
    /// Historical faces proven for part of a native selection.
    HistoricalPartial {
        /// Input topology containing every resolved face.
        state: FeatureInputTopologyId,
        /// Proven state-local face identities in source operand order.
        #[serde(deserialize_with = "deserialize_selection_faces")]
        faces: DistinctMembers<HistoricalFaceId>,
        /// Stable native identities of unresolved source operands.
        #[serde(deserialize_with = "deserialize_selection_unresolved")]
        unresolved: NativeSelections,
        /// Format-native selection reference.
        #[serde(deserialize_with = "deserialize_selection_native")]
        native: NonBlankString,
    },
    /// Faces in an intermediate regenerated feature result, paired with the
    /// format-native selection required for rewrite.
    Generated {
        /// Feature-local face identities.
        #[serde(deserialize_with = "deserialize_selection_faces")]
        faces: NonEmptyMembers<GeneratedFaceRef>,
        /// Format-native persistent selection reference.
        #[serde(deserialize_with = "deserialize_selection_native")]
        native: SelectionReference,
    },
    /// Format-native selection reference.
    Native(String),
}

/// A nonempty sequence of members in source order.
///
/// The members stay contiguous: the sequence is read through slice patterns
/// and slice methods. The mint refuses an empty list, `push` only grows, and
/// `DerefMut` hands out `&mut [T]`, which cannot shorten the sequence, so no
/// reader has to ask whether a member is there.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, cadmpeg_derive::StructuralOrd)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(transparent)]
#[cfg_attr(feature = "schema", schemars(extend("minItems" = 1)))]
pub struct NonEmptyMembers<T>(pub(super) Vec<T>);

impl<T> TryFrom<Vec<T>> for NonEmptyMembers<T> {
    type Error = BodySelectionError;
    fn try_from(value: Vec<T>) -> Result<Self, Self::Error> {
        if value.is_empty() {
            return Err(BodySelectionError::Empty);
        }
        Ok(Self(value))
    }
}

impl<T> NonEmptyMembers<T> {
    /// Construct a sequence with one member.
    pub fn one(member: T) -> Self {
        Self(vec![member])
    }

    /// Append a member while retaining the nonempty witness.
    pub fn push(&mut self, member: T) {
        self.0.push(member);
    }

    /// Map the members in source order while retaining the nonempty witness.
    pub fn map<U>(self, map: impl FnMut(T) -> U) -> NonEmptyMembers<U> {
        NonEmptyMembers(self.0.into_iter().map(map).collect())
    }

    /// The members in source order.
    pub fn as_slice(&self) -> &[T] {
        &self.0
    }
}

impl<T> std::ops::Deref for NonEmptyMembers<T> {
    type Target = [T];
    fn deref(&self) -> &[T] {
        &self.0
    }
}

impl<T> IntoIterator for NonEmptyMembers<T> {
    type Item = T;
    type IntoIter = std::vec::IntoIter<T>;
    fn into_iter(self) -> Self::IntoIter {
        self.0.into_iter()
    }
}

impl<'a, T> IntoIterator for &'a NonEmptyMembers<T> {
    type Item = &'a T;
    type IntoIter = std::slice::Iter<'a, T>;
    fn into_iter(self) -> Self::IntoIter {
        self.0.iter()
    }
}

impl<'de, T: Deserialize<'de>> Deserialize<'de> for NonEmptyMembers<T> {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        Self::try_from(Vec::<T>::deserialize(deserializer)?).map_err(serde::de::Error::custom)
    }
}

impl<T> std::ops::DerefMut for NonEmptyMembers<T> {
    fn deref_mut(&mut self) -> &mut [T] {
        &mut self.0
    }
}

impl<'a, T> IntoIterator for &'a mut NonEmptyMembers<T> {
    type Item = &'a mut T;
    type IntoIter = std::slice::IterMut<'a, T>;
    fn into_iter(self) -> Self::IntoIter {
        self.0.iter_mut()
    }
}

impl VertexSelection {
    /// Admit a generated vertex and its native reference.
    pub fn generated(
        vertex: GeneratedVertexRef,
        native: String,
    ) -> Result<Self, BodySelectionError> {
        SelectionReference::new(native).map(|native| Self::Generated { vertex, native })
    }
    /// Admit a historical vertex and its native reference.
    pub fn historical(
        state: FeatureInputTopologyId,
        vertex: HistoricalVertexId,
        native: String,
    ) -> Result<Self, BodySelectionError> {
        NonBlankString::new(native)
            .ok_or(BodySelectionError::BlankNativeMember)
            .map(|native| Self::Historical {
                state,
                vertex,
                native,
            })
    }
    /// Admit a native vertex reference.
    pub fn native(native: String) -> Result<Self, BodySelectionError> {
        SelectionReference::new(native).map(Self::Native)
    }
}

impl EdgeSelection {
    /// Admit nonempty distinct historical members and their native reference.
    pub fn historical(
        state: FeatureInputTopologyId,
        edges: Vec<HistoricalEdgeId>,
        native: String,
    ) -> Result<Self, BodySelectionError> {
        let edges = SelectionMembers::try_from(edges)?;
        let native = NonBlankString::new(native).ok_or(BodySelectionError::BlankNativeMember)?;
        Ok(Self::Historical {
            state,
            edges,
            native,
        })
    }

    /// Admit distinct historical members, unresolved native members and their native reference.
    pub fn historical_partial(
        state: FeatureInputTopologyId,
        edges: Vec<HistoricalEdgeId>,
        unresolved: Vec<String>,
        native: String,
    ) -> Result<Self, BodySelectionError> {
        let edges =
            DistinctMembers::try_from(edges).map_err(|_| BodySelectionError::RepeatedBody)?;
        let unresolved = NativeSelections::try_from(unresolved)?;
        let native = NonBlankString::new(native).ok_or(BodySelectionError::BlankNativeMember)?;
        Ok(Self::HistoricalPartial {
            state,
            edges,
            unresolved,
            native,
        })
    }

    /// Admits generated members and their native reference.
    pub fn generated(
        edges: Vec<GeneratedEdgeRef>,
        native: String,
    ) -> Result<Self, BodySelectionError> {
        let edges = edges.try_into()?;
        SelectionReference::new(native).map(|native| Self::Generated { edges, native })
    }
}

impl FaceSelection {
    /// Admit nonempty distinct historical members and their native reference.
    pub fn historical(
        state: FeatureInputTopologyId,
        faces: Vec<HistoricalFaceId>,
        native: String,
    ) -> Result<Self, BodySelectionError> {
        let faces = SelectionMembers::try_from(faces)?;
        let native = NonBlankString::new(native).ok_or(BodySelectionError::BlankNativeMember)?;
        Ok(Self::Historical {
            state,
            faces,
            native,
        })
    }

    /// Admit distinct historical members, unresolved native members and their native reference.
    pub fn historical_partial(
        state: FeatureInputTopologyId,
        faces: Vec<HistoricalFaceId>,
        unresolved: Vec<String>,
        native: String,
    ) -> Result<Self, BodySelectionError> {
        let faces =
            DistinctMembers::try_from(faces).map_err(|_| BodySelectionError::RepeatedBody)?;
        let unresolved = NativeSelections::try_from(unresolved)?;
        let native = NonBlankString::new(native).ok_or(BodySelectionError::BlankNativeMember)?;
        Ok(Self::HistoricalPartial {
            state,
            faces,
            unresolved,
            native,
        })
    }

    /// Admits generated members and their native reference.
    pub fn generated(
        faces: Vec<GeneratedFaceRef>,
        native: String,
    ) -> Result<Self, BodySelectionError> {
        let faces = faces.try_into()?;
        SelectionReference::new(native).map(|native| Self::Generated { faces, native })
    }
}

impl GeneratedEdgeRef {
    /// Admit a feature-local persistent identity.
    pub fn new(feature: FeatureId, local_id: String) -> Result<Self, BodySelectionError> {
        SelectionReference::new(local_id).map(|local_id| Self { feature, local_id })
    }
}

impl GeneratedFaceRef {
    /// Admit a feature-local persistent identity.
    pub fn new(feature: FeatureId, local_id: String) -> Result<Self, BodySelectionError> {
        SelectionReference::new(local_id).map(|local_id| Self { feature, local_id })
    }
}

impl GeneratedVertexRef {
    /// Admit a feature-local persistent identity.
    pub fn new(feature: FeatureId, local_id: String) -> Result<Self, BodySelectionError> {
        SelectionReference::new(local_id).map(|local_id| Self { feature, local_id })
    }
}

/// A nonblank persistent selection reference.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(transparent)]
pub struct SelectionReference(pub(super) String);

impl TryFrom<String> for SelectionReference {
    type Error = BodySelectionError;
    fn try_from(value: String) -> Result<Self, Self::Error> {
        if value.trim().is_empty() {
            return Err(BodySelectionError::BlankNativeMember);
        }
        Ok(Self(value))
    }
}

impl SelectionReference {
    /// Admit a nonblank persistent reference.
    pub fn new(value: String) -> Result<Self, BodySelectionError> {
        Self::try_from(value)
    }

    /// The retained reference text.
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl std::ops::Deref for SelectionReference {
    type Target = str;
    fn deref(&self) -> &str {
        &self.0
    }
}

impl PartialEq<str> for SelectionReference {
    fn eq(&self, other: &str) -> bool {
        self.as_str() == other
    }
}

impl PartialEq<&str> for SelectionReference {
    fn eq(&self, other: &&str) -> bool {
        self.as_str() == *other
    }
}

impl<'de> Deserialize<'de> for SelectionReference {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        Self::try_from(String::deserialize(deserializer)?).map_err(serde::de::Error::custom)
    }
}

/// Distinct members in source order, including an empty sequence.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(transparent)]
pub struct DistinctMembers<T>(pub(super) Vec<T>);

impl<T> Default for DistinctMembers<T> {
    fn default() -> Self {
        Self(Vec::new())
    }
}

impl<T: Eq + std::hash::Hash> TryFrom<Vec<T>> for DistinctMembers<T> {
    type Error = FeatureCollectionError;
    /// Admit members that are pairwise distinct, in source order.
    fn try_from(value: Vec<T>) -> Result<Self, Self::Error> {
        if !membership::distinct(&value, |_| true) {
            return Err(FeatureCollectionError::Invalid("members must be distinct"));
        }
        Ok(Self(value))
    }
}

impl<T: PartialEq> DistinctMembers<T> {
    /// Append a member unless it is already present, and return whether it was appended.
    pub fn insert(&mut self, value: T) -> bool {
        membership::insert(&mut self.0, value)
    }
}

impl<T> DistinctMembers<T> {
    /// Removes all members.
    pub fn clear(&mut self) {
        self.0.clear();
    }

    /// Retains members that satisfy the predicate without changing their order.
    pub fn retain(&mut self, predicate: impl FnMut(&T) -> bool) {
        self.0.retain(predicate);
    }

    /// Whether the sequence contains no members.
    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }

    /// The members in source order.
    pub fn as_slice(&self) -> &[T] {
        &self.0
    }
}

impl<T: PartialEq> Extend<T> for DistinctMembers<T> {
    fn extend<I: IntoIterator<Item = T>>(&mut self, iter: I) {
        for member in iter {
            membership::insert(&mut self.0, member);
        }
    }
}

impl<T: PartialEq> FromIterator<T> for DistinctMembers<T> {
    fn from_iter<I: IntoIterator<Item = T>>(iter: I) -> Self {
        let mut members = Self::default();
        members.extend(iter);
        members
    }
}

impl<T> std::ops::Deref for DistinctMembers<T> {
    type Target = [T];
    fn deref(&self) -> &[T] {
        &self.0
    }
}

impl<'a, T> IntoIterator for &'a DistinctMembers<T> {
    type Item = &'a T;
    type IntoIter = std::slice::Iter<'a, T>;
    fn into_iter(self) -> Self::IntoIter {
        self.0.iter()
    }
}

impl<'de, T: Deserialize<'de> + Eq + std::hash::Hash> Deserialize<'de> for DistinctMembers<T> {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let values = Vec::<T>::deserialize(deserializer)?;
        Self::try_from(values).map_err(serde::de::Error::custom)
    }
}

/// Nonempty distinct selection members in source order.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(transparent)]
pub struct SelectionMembers<T>(pub(super) Vec<T>);

impl<T: Eq + std::hash::Hash> TryFrom<Vec<T>> for SelectionMembers<T> {
    type Error = BodySelectionError;
    /// Admit nonempty pairwise distinct members, in source order.
    fn try_from(value: Vec<T>) -> Result<Self, Self::Error> {
        if value.is_empty() {
            return Err(BodySelectionError::Empty);
        }
        if !membership::distinct(&value, |_| true) {
            return Err(BodySelectionError::RepeatedBody);
        }
        Ok(Self(value))
    }
}

impl<T> SelectionMembers<T> {
    /// The selected members in source order.
    pub fn as_slice(&self) -> &[T] {
        &self.0
    }
}

impl<T> IntoIterator for SelectionMembers<T> {
    type Item = T;
    type IntoIter = std::vec::IntoIter<T>;
    fn into_iter(self) -> Self::IntoIter {
        self.0.into_iter()
    }
}

impl<T> std::ops::Deref for SelectionMembers<T> {
    type Target = [T];
    fn deref(&self) -> &[T] {
        &self.0
    }
}

impl<'a, T> IntoIterator for &'a SelectionMembers<T> {
    type Item = &'a T;
    type IntoIter = std::slice::Iter<'a, T>;
    fn into_iter(self) -> Self::IntoIter {
        self.0.iter()
    }
}

impl<'de, T: Deserialize<'de> + Eq + std::hash::Hash> Deserialize<'de> for SelectionMembers<T> {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let values = Vec::<T>::deserialize(deserializer)?;
        Self::try_from(values).map_err(serde::de::Error::custom)
    }
}

/// Nonempty distinct nonblank native selection names.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(transparent)]
pub struct NativeSelections(pub(super) Vec<String>);

impl TryFrom<Vec<String>> for NativeSelections {
    type Error = BodySelectionError;
    /// Admit nonempty, nonblank, pairwise distinct native names, in source order.
    fn try_from(value: Vec<String>) -> Result<Self, Self::Error> {
        if value.is_empty() {
            return Err(BodySelectionError::Empty);
        }
        if value.iter().any(|name| name.trim().is_empty()) {
            return Err(BodySelectionError::BlankNativeMember);
        }
        if !membership::distinct(&value, |_| true) {
            return Err(BodySelectionError::RepeatedNativeMember);
        }
        Ok(Self(value))
    }
}

impl NativeSelections {
    /// The native names in source order.
    pub fn as_slice(&self) -> &[String] {
        &self.0
    }
}

impl IntoIterator for NativeSelections {
    type Item = String;
    type IntoIter = std::vec::IntoIter<String>;
    fn into_iter(self) -> Self::IntoIter {
        self.0.into_iter()
    }
}

impl std::ops::Deref for NativeSelections {
    type Target = [String];
    fn deref(&self) -> &[String] {
        &self.0
    }
}

impl<'de> Deserialize<'de> for NativeSelections {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let values = Vec::<String>::deserialize(deserializer)?;
        Self::try_from(values).map_err(serde::de::Error::custom)
    }
}

/// Failure to construct a body-selection member set.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum BodySelectionError {
    /// A selection set contains no members.
    #[error("body selection set must not be empty")]
    Empty,
    /// A body occurs more than once in the set.
    #[error("body selection set repeats a body")]
    RepeatedBody,
    /// A native selection member is empty or contains only whitespace.
    #[error("body selection member must not be blank")]
    BlankNativeMember,
    /// A native member occurs more than once in the set.
    #[error("body selection set repeats a native member")]
    RepeatedNativeMember,
    /// The standard allocator refused context-free selection storage.
    #[error("selection member allocation failed")]
    Allocation,
}

/// Native selection of the bodies a copy-and-paste operation introduces. The
/// copied bodies themselves are the feature's outputs, so no variant restates
/// them.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, cadmpeg_derive::DeserializeTagged)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(
    tag = "kind",
    content = "value",
    rename_all = "snake_case",
    deny_unknown_fields
)]
pub enum InsertedBodies {
    /// The copied bodies are not resolved to the neutral model.
    Native(String),
    /// The copied bodies are resolved and listed in the feature's outputs.
    Resolved {
        /// Format-native selection expression.
        native: String,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
#[doc = "Invalid membership or native reference of a component insertion."]
pub enum InsertedComponentsError {
    #[doc = "A resolved or partial insertion has no known occurrence."]
    #[error("resolved or partial component insertion must select an occurrence")]
    Empty,
    #[doc = "The known membership repeats an occurrence."]
    #[error("component insertion repeats an occurrence")]
    RepeatedOccurrence,
    #[doc = "The retained native selection is blank."]
    #[error("component insertion native reference must not be blank")]
    BlankNative,
    #[doc = "The allocator refused selection storage."]
    #[error("component insertion allocation failed")]
    Allocation,
}

impl From<BodySelectionError> for InsertedComponentsError {
    fn from(error: BodySelectionError) -> Self {
        match error {
            BodySelectionError::Empty => Self::Empty,
            BodySelectionError::RepeatedBody | BodySelectionError::RepeatedNativeMember => {
                Self::RepeatedOccurrence
            }
            BodySelectionError::BlankNativeMember => Self::BlankNative,
            BodySelectionError::Allocation => Self::Allocation,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, cadmpeg_derive::DeserializeTagged)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
#[doc = "Occurrence membership of an insertion with its complete native selection."]
pub enum InsertedComponents {
    #[doc = "Insertion membership remains in its native selection."]
    Native {
        #[doc = "Format-native selection expression."]
        native: SelectionReference,
    },
    #[doc = "Every inserted occurrence resolves to the neutral model."]
    Resolved {
        #[doc = "Nonempty distinct occurrences in source order."]
        occurrences: SelectionMembers<OccurrenceId>,
        #[doc = "Format-native selection expression."]
        native: SelectionReference,
    },
    #[doc = "Known inserted occurrences accompany unresolved native membership."]
    Partial {
        #[doc = "Nonempty distinct known occurrences in source order."]
        occurrences: SelectionMembers<OccurrenceId>,
        #[doc = "Format-native expression retaining the complete selection."]
        native: SelectionReference,
    },
}

impl InsertedComponents {
    #[doc = "Retain unresolved membership through a nonblank native selection."]
    pub fn native(native: String) -> Result<Self, InsertedComponentsError> {
        Ok(Self::Native {
            native: SelectionReference::new(native)?,
        })
    }

    #[doc = "Admit complete nonempty distinct membership and its native selection."]
    pub fn resolved(
        occurrences: Vec<OccurrenceId>,
        native: String,
    ) -> Result<Self, InsertedComponentsError> {
        Ok(Self::Resolved {
            occurrences: SelectionMembers::try_from(occurrences)?,
            native: SelectionReference::new(native)?,
        })
    }

    #[doc = "Admit nonempty distinct known membership and the complete native selection."]
    pub fn partial(
        occurrences: Vec<OccurrenceId>,
        native: String,
    ) -> Result<Self, InsertedComponentsError> {
        Ok(Self::Partial {
            occurrences: SelectionMembers::try_from(occurrences)?,
            native: SelectionReference::new(native)?,
        })
    }

    #[must_use]
    #[doc = "Whether every inserted occurrence is resolved."]
    pub const fn is_resolved(&self) -> bool {
        matches!(self, Self::Resolved { .. })
    }

    #[must_use]
    #[doc = "Known occurrence identities in source order."]
    pub fn occurrences(&self) -> &[OccurrenceId] {
        match self {
            Self::Native { .. } => &[],
            Self::Resolved { occurrences, .. } | Self::Partial { occurrences, .. } => occurrences,
        }
    }

    #[must_use]
    #[doc = "The retained format-native expression for the complete insertion."]
    pub fn native_reference(&self) -> &SelectionReference {
        match self {
            Self::Native { native }
            | Self::Resolved { native, .. }
            | Self::Partial { native, .. } => native,
        }
    }
}

impl InsertedBodies {
    /// Whether the copied bodies are resolved to the neutral model.
    #[must_use]
    pub const fn is_resolved(&self) -> bool {
        matches!(self, Self::Resolved { .. })
    }
}

/// Body operands resolved by the decoder or retained in native form.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
pub struct BodyMember<B> {
    pub(super) body: B,
    pub(super) native: cadmpeg_core::text::NonBlankString,
}

impl<B> BodyMember<B> {
    /// Construct one body/native selection row.
    ///
    /// The native member is non-blank by type; the caller mints it at the
    /// boundary where the source states it.
    pub const fn new(body: B, native: cadmpeg_core::text::NonBlankString) -> Self {
        Self { body, native }
    }

    /// Body identity in this row.
    #[must_use]
    pub const fn body(&self) -> &B {
        &self.body
    }

    /// Consume the row and return its body identity and native member.
    #[must_use]
    pub fn into_parts(self) -> (B, String) {
        (self.body, self.native.into_string())
    }
}

impl<'de, B> Deserialize<'de> for BodyMember<B>
where
    B: Deserialize<'de>,
{
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        #[derive(Deserialize)]
        #[serde(deny_unknown_fields)]
        struct Wire<B> {
            body: B,
            native: String,
        }
        let wire = Wire::deserialize(deserializer)?;
        let native = cadmpeg_core::text::NonBlankString::new(wire.native).ok_or_else(|| {
            serde::de::Error::custom(BodySelectionError::BlankNativeMember.to_string())
        })?;
        Ok(Self::new(wire.body, native))
    }
}

/// Immutable, checked rows pairing a body identity with its native member.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(transparent)]
pub struct BodyMembers<B>(pub(super) Vec<BodyMember<B>>);

impl<B> BodyMembers<B> {
    /// Construct checked rows from body/native pairs.
    pub fn try_from_rows(rows: Vec<BodyMember<B>>) -> Result<Self, BodySelectionError>
    where
        B: Eq + std::hash::Hash,
    {
        if rows.is_empty() {
            return Err(BodySelectionError::Empty);
        }
        let mut bodies = std::collections::HashSet::new();
        let mut native = std::collections::HashSet::new();
        for row in &rows {
            if !bodies.insert(&row.body) {
                return Err(BodySelectionError::RepeatedBody);
            }
            if !native.insert(&row.native) {
                return Err(BodySelectionError::RepeatedNativeMember);
            }
        }
        Ok(Self(rows))
    }

    /// Count paired selection rows.
    #[must_use]
    pub const fn count(&self) -> usize {
        self.0.len()
    }

    /// Iterate over paired selection rows in source order.
    pub fn iter(&self) -> std::slice::Iter<'_, BodyMember<B>> {
        self.0.iter()
    }

    /// Borrow the body identities in source order.
    pub fn bodies(&self) -> impl Iterator<Item = &B> {
        self.0.iter().map(BodyMember::body)
    }
}

impl<'a, B> IntoIterator for &'a BodyMembers<B> {
    type Item = &'a BodyMember<B>;
    type IntoIter = std::slice::Iter<'a, BodyMember<B>>;

    fn into_iter(self) -> Self::IntoIter {
        self.iter()
    }
}

impl<'de, B> Deserialize<'de> for BodyMembers<B>
where
    B: Deserialize<'de> + Eq + std::hash::Hash,
{
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let rows = Vec::<BodyMember<B>>::deserialize(deserializer)?;
        Self::try_from_rows(rows).map_err(serde::de::Error::custom)
    }
}

/// Body operands resolved by the decoder or retained in native form.
#[derive(Debug, Clone, PartialEq, Serialize, cadmpeg_derive::DeserializeTagged)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(
    tag = "kind",
    content = "value",
    rename_all = "snake_case",
    deny_unknown_fields
)]
pub enum BodySelection {
    /// Selection exists semantically but its operands are not resolved.
    #[serde(deserialize_with = "cadmpeg_core::tag_only::refuse_value")]
    Unresolved,
    /// Resolved topological bodies.
    Bodies(#[cfg_attr(feature = "schema", schemars(with = "Vec<BodyId>"))] DistinctMembers<BodyId>),
    /// Resolved bodies paired with the format-native selection required for rewrite.
    Resolved {
        /// Resolved topological bodies.
        #[cfg_attr(feature = "schema", schemars(with = "Vec<BodyId>"))]
        bodies: DistinctMembers<BodyId>,
        /// Format-native selection expression.
        native: String,
    },
    /// Resolved bodies paired with independently retained native selection members.
    ResolvedSet {
        /// Checked rows in native member order.
        members: BodyMembers<BodyId>,
    },
    /// Bodies resolved in the containing feature's input topology.
    Historical {
        /// Input topology containing every selected body.
        state: FeatureInputTopologyId,
        /// State-local body identities in operand order.
        bodies: SelectionMembers<HistoricalBodyId>,
        /// Format-native selection expression.
        native: SelectionReference,
    },
    /// Bodies resolved in the containing feature's input topology from
    /// independently retained native selection members.
    HistoricalSet {
        /// Input topology containing every selected body.
        state: FeatureInputTopologyId,
        /// Checked rows in native member order.
        members: BodyMembers<HistoricalBodyId>,
    },
    /// Bodies in intermediate regenerated feature results, paired with the
    /// format-native selection required for rewrite.
    Generated {
        /// Feature-local body identities.
        bodies: SelectionMembers<GeneratedBodyRef>,
        /// Format-native persistent selection reference.
        native: SelectionReference,
    },
    /// Persistent bodies in the consuming feature's regeneration input state.
    Local {
        /// Ordered feature-input-local body identities.
        bodies: NativeSelections,
        /// Format-native persistent selection reference.
        native: SelectionReference,
    },
    /// Format-native selection expression.
    Native(String),
    /// Ordered format-native selection members that have no enclosing native
    /// group record.
    NativeSet(NativeSelections),
}

/// Persistent identity of a body in one regenerated feature result.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(deny_unknown_fields)]
pub struct GeneratedBodyRef {
    /// Feature whose regenerated result owns the body.
    pub feature: FeatureId,
    /// Feature-local persistent body identity.
    pub local_id: SelectionReference,
}

impl GeneratedBodyRef {
    /// Admit a feature-local persistent identity.
    pub fn new(feature: FeatureId, local_id: String) -> Result<Self, BodySelectionError> {
        SelectionReference::new(local_id).map(|local_id| Self { feature, local_id })
    }
}
