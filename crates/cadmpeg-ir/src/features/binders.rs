// SPDX-License-Identifier: Apache-2.0
//! Binder constructions.

use crate::scalar::NonZeroLength;
use cadmpeg_core::text::NonBlankString;
use cadmpeg_derive::DeserializeName;

use super::{
    deserialize_binder_construction_offset, deserialize_context, deserialize_local_document,
    deserialize_local_object, deserialize_local_reference, deserialize_local_subelements,
    FeatureId,
};
#[cfg(feature = "schema")]
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

/// One object or subelement selection consumed by a design binder.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(deny_unknown_fields)]
pub struct BinderSource {
    /// Bound object identity.
    pub target: BinderTarget,
    /// Ordered native subelement selectors; empty selects the complete object.
    #[serde(deserialize_with = "deserialize_local_subelements")]
    pub subelements: Vec<NonBlankString>,
}

/// Resolved or externally scoped binder target.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, cadmpeg_derive::DeserializeTagged)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(tag = "kind", rename_all = "snake_case")]
#[serde(deny_unknown_fields)]
pub enum BinderTarget {
    /// Feature in this CADIR document.
    Feature {
        /// Target feature identity.
        feature: FeatureId,
    },
    /// Object in another source document.
    External {
        /// Source document identity.
        #[serde(deserialize_with = "deserialize_local_document")]
        document: NonBlankString,
        /// Object identity within the source document.
        #[serde(deserialize_with = "deserialize_local_object")]
        object: NonBlankString,
    },
    /// Source-native target identity that cannot be resolved further.
    Native {
        /// Opaque source-native target identity.
        #[serde(deserialize_with = "deserialize_local_reference")]
        reference: NonBlankString,
    },
}

/// Binding behavior and optional derived-shape construction.
#[derive(Debug, Clone, PartialEq, Serialize, cadmpeg_derive::DeserializeTagged)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(tag = "kind", rename_all = "snake_case")]
#[serde(deny_unknown_fields)]
pub enum BinderConstruction {
    /// Simple binder over one support object.
    Shape {
        /// Transform support geometry between its container and the binder container.
        trace_support: bool,
    },
    /// Multi-object subshape binder.
    SubShape {
        /// Live-update lifecycle.
        lifecycle: BinderLifecycle,
        /// Placement interpretation for linked subobjects.
        placement: BinderPlacement,
        /// Copy-on-change state.
        copy_on_change: BinderCopyOnChange,
        /// Whether linked objects are claimed as children in the tree.
        claim_children: bool,
        /// Whether multiple resulting solids are fused.
        fuse: bool,
        /// Whether bound wires are promoted to faces.
        make_face: bool,
        /// Whether external documents may remain partially loaded.
        partial_load: bool,
        /// Whether redundant edges are removed from the result.
        refine: bool,
        /// Optional two-dimensional offset construction.
        #[serde(
            default,
            skip_serializing_if = "Option::is_none",
            deserialize_with = "deserialize_binder_construction_offset"
        )]
        offset: Option<BinderOffset>,
        /// Context object used to interpret relative placement.
        #[serde(
            default,
            skip_serializing_if = "Option::is_none",
            deserialize_with = "deserialize_context"
        )]
        context: Option<BinderTarget>,
    },
}

/// Update lifecycle of a subshape binder.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, DeserializeName)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(rename_all = "snake_case")]
#[serde(deny_unknown_fields)]
pub enum BinderLifecycle {
    /// Automatically tracks changes to its sources.
    Synchronized,
    /// Retains links but updates only when explicitly requested.
    Frozen,
    /// Stores a copied shape and no longer retains live binding behavior.
    Detached,
}

/// Placement interpretation for bound subobjects.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, DeserializeName)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(rename_all = "snake_case")]
#[serde(deny_unknown_fields)]
pub enum BinderPlacement {
    /// Interpret source placement relative to the binder context.
    Relative,
    /// Preserve source placement in global coordinates.
    Global,
}

/// Copy-on-change state of a subshape binder.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, DeserializeName)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(rename_all = "snake_case")]
#[serde(deny_unknown_fields)]
pub enum BinderCopyOnChange {
    /// Do not clone configurable source properties.
    Disabled,
    /// Clone configurable source properties when they change.
    Enabled,
    /// A private source copy has already been mutated.
    Mutated,
}

/// Two-dimensional offset applied to bound faces or wires.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(deny_unknown_fields)]
pub struct BinderOffset {
    /// Signed offset distance.
    pub distance: NonZeroLength,
    /// Join law at offset corners.
    pub join: BinderOffsetJoin,
    /// Whether to fill between original and offset wires.
    pub fill: bool,
    /// Whether open input wires produce open offset results.
    pub open_result: bool,
    /// Whether child-wire intersections are resolved together.
    pub intersection: bool,
}

/// Corner join law of a binder offset.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, DeserializeName)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(rename_all = "snake_case")]
#[serde(deny_unknown_fields)]
pub enum BinderOffsetJoin {
    /// Circular corner arcs.
    Arcs,
    /// Tangent continuation.
    Tangent,
    /// Sharp line-line intersections.
    Intersection,
}
