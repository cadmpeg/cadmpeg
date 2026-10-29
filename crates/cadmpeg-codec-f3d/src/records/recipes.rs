// SPDX-License-Identifier: Apache-2.0
//! Construction recipes and the naming space and timestamp a component records.

use super::identity::RecordedValue;
use super::mesh::DesignRelaxedGuidText;
use cadmpeg_ir::attributes::AttributeTarget;
use cadmpeg_ir::scalar::FiniteReal;
use serde::{Deserialize, Serialize};

cadmpeg_core::named_optional_field!(deserialize_design_id, String, "design_id");
cadmpeg_core::named_optional_field!(
    deserialize_design_selector,
    ConstructionRecipeSelector,
    "design_selector"
);
cadmpeg_core::named_optional_field!(deserialize_record_index, i32, "record_index");
cadmpeg_core::named_optional_field!(deserialize_component_name, String, "name");
/// Component-local Design naming space bound to a context UUID.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct DesignComponentNamingSpace {
    /// Globally unique deterministic identifier for this native record.
    pub(crate) id: String,
    /// Byte offset of the binding marker in its Design `BulkStream`.
    #[serde(skip)]
    pub(crate) byte_offset: Option<u64>,
    /// Component entity id named by the binding.
    pub(crate) component_record_index: u64,
    /// UUID used by persistent identities to select this component.
    pub(crate) context_uuid: DesignRelaxedGuidText,
    /// Byte offset of the UUID length prefix.
    #[serde(skip)]
    pub(crate) context_uuid_offset: Option<u64>,
    /// The component's name, stored after the context UUID at the end of the
    /// component's definition record.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_component_name"
    )]
    pub(crate) name: Option<String>,
    /// Byte offset of the name's length prefix.
    #[serde(skip)]
    pub(crate) name_offset: Option<u64>,
}

/// Original authoring time attached to a solved ASM entity.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct CreationTimestamp {
    /// Globally unique deterministic identifier for this native record.
    pub(crate) id: String,
    /// Solved B-rep entity carrying the timestamp attribute.
    pub(crate) target: AttributeTarget,
    /// Source SAB record index of the timestamp attribute.
    pub(crate) record_index: u32,
    /// Creation time as microseconds since the Unix epoch.
    pub(crate) unix_microseconds: FiniteReal,
}

/// Design `BulkStream` regeneration-recipe family.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum ConstructionRecipeKind {
    /// Recipe regenerates a whole body.
    Body,
    /// Recipe regenerates a single face.
    Face,
    /// Recipe regenerates a face bounded by an explicit region.
    BoundedFace,
    /// Recipe regenerates a single edge.
    Edge,
    /// Recipe regenerates a single vertex.
    Vertex,
}

/// A recipe Design id and its optional following selector.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub(crate) struct ConstructionRecipeDesign<Id> {
    #[serde(rename = "design_id")]
    pub(crate) id: Id,
    #[serde(rename = "design_selector", skip_serializing_if = "Option::is_none")]
    pub(crate) selector: Option<ConstructionRecipeSelector>,
}

/// One source-framed parametric regeneration recipe.
#[derive(Debug, PartialEq, Deserialize)]
#[cfg_attr(not(test), derive(Clone))]
#[serde(try_from = "ConstructionRecipeWire")]
pub(crate) struct ConstructionRecipe {
    /// Globally unique deterministic identifier for this native record.
    pub(crate) id: String,
    /// Byte offset of this recipe's family marker in its Design `BulkStream`.
    pub(crate) byte_offset: Option<u64>,
    /// Topology kind this recipe regenerates on replay.
    pub(crate) kind: ConstructionRecipeKind,
    /// Design identity carried by the recipe; absent for recipes without a body key.
    pub(crate) design: Option<ConstructionRecipeDesign<RecordedValue<String, Option<u64>>>>,
    /// Position of this recipe in the `BulkStream` recipe sequence, in source order.
    pub(crate) recipe_index: u32,
    /// Source `BulkStream` record index this recipe was decoded from, with the
    /// byte offset it was read at; `None` when the recipe's family marker opens
    /// the stream too early for the index word to precede it, so the stream
    /// states no record index for this recipe.
    pub(crate) record_index: Option<RecordedValue<i32, Option<u64>>>,
}

#[cfg(test)]
thread_local! {
    static CONSTRUCTION_RECIPE_CLONE_COUNT: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
}

#[cfg(test)]
impl Clone for ConstructionRecipe {
    fn clone(&self) -> Self {
        CONSTRUCTION_RECIPE_CLONE_COUNT.with(|count| count.set(count.get() + 1));
        Self {
            id: self.id.clone(),
            byte_offset: self.byte_offset,
            kind: self.kind,
            design: self.design.clone(),
            recipe_index: self.recipe_index,
            record_index: self.record_index,
        }
    }
}

/// Typed recipe content. Physical source lanes are retained separately.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct ConstructionRecipeWire {
    id: String,
    kind: ConstructionRecipeKind,
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_design_id"
    )]
    design_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    design_id_offset: Option<u64>,
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_design_selector"
    )]
    design_selector: Option<ConstructionRecipeSelector>,
    recipe_index: u32,
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_record_index"
    )]
    record_index: Option<i32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    record_index_offset: Option<u64>,
}
impl Serialize for ConstructionRecipe {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        #[derive(Serialize)]
        struct WireRef<'a> {
            id: &'a str,
            kind: ConstructionRecipeKind,
            #[serde(skip_serializing_if = "Option::is_none")]
            design_id: Option<&'a str>,
            #[serde(skip_serializing_if = "Option::is_none")]
            design_id_offset: Option<u64>,
            #[serde(skip_serializing_if = "Option::is_none")]
            design_selector: Option<ConstructionRecipeSelector>,
            recipe_index: u32,
            #[serde(skip_serializing_if = "Option::is_none")]
            record_index: Option<i32>,
            #[serde(skip_serializing_if = "Option::is_none")]
            record_index_offset: Option<u64>,
        }
        WireRef {
            id: &self.id,
            kind: self.kind,
            design_id: self.design.as_ref().map(|design| design.id.value.as_str()),
            design_id_offset: self.design.as_ref().and_then(|design| design.id.offset),
            design_selector: self.design.as_ref().and_then(|design| design.selector),
            recipe_index: self.recipe_index,
            record_index: self.record_index.map(|index| index.value),
            record_index_offset: self.record_index.and_then(|index| index.offset),
        }
        .serialize(serializer)
    }
}
impl TryFrom<ConstructionRecipeWire> for ConstructionRecipe {
    type Error = String;
    fn try_from(wire: ConstructionRecipeWire) -> Result<Self, String> {
        if wire.design_id_offset.is_some() && wire.design_id.is_none() {
            return Err("construction recipe design_id_offset requires design_id".into());
        }
        if wire.record_index_offset.is_some() && wire.record_index.is_none() {
            return Err("construction recipe record_index_offset requires record_index".into());
        }
        let design = match (wire.design_id, wire.design_selector) {
            (Some(value), selector) => Some(ConstructionRecipeDesign {
                id: RecordedValue {
                    value,
                    offset: wire.design_id_offset,
                },
                selector,
            }),
            (None, None) => None,
            (None, Some(_)) => {
                return Err("construction recipe design_selector requires design_id".into());
            }
        };
        Ok(Self {
            id: wire.id,
            byte_offset: None,
            kind: wire.kind,
            design,
            recipe_index: wire.recipe_index,
            record_index: wire.record_index.map(|value| RecordedValue {
                value,
                offset: wire.record_index_offset,
            }),
        })
    }
}
#[cfg(test)]
impl From<ConstructionRecipe> for ConstructionRecipeWire {
    fn from(value: ConstructionRecipe) -> Self {
        Self {
            id: value.id,
            kind: value.kind,
            design_id: value.design.as_ref().map(|design| design.id.value.clone()),
            design_id_offset: value.design.as_ref().and_then(|design| design.id.offset),
            design_selector: value.design.as_ref().and_then(|design| design.selector),
            recipe_index: value.recipe_index,
            record_index: value.record_index.map(|index| index.value),
            record_index_offset: value.record_index.and_then(|index| index.offset),
        }
    }
}
impl ConstructionRecipe {
    pub(crate) fn content_eq(&self, other: &Self) -> bool {
        self.id == other.id
            && self.kind == other.kind
            && self.recipe_index == other.recipe_index
            && self.record_index.map(|index| index.value)
                == other.record_index.map(|index| index.value)
            && self.design.as_ref().map(|design| {
                (
                    &design.id.value,
                    design.selector.map(|selector| selector.value),
                )
            }) == other.design.as_ref().map(|design| {
                (
                    &design.id.value,
                    design.selector.map(|selector| selector.value),
                )
            })
    }
    pub(crate) fn retain_source(&self, fidelity: &mut cadmpeg_ir::SourceFidelity) {
        for (name, offset) in [
            ("byte_offset", self.byte_offset),
            (
                "record_index_offset",
                self.record_index.and_then(|index| index.offset),
            ),
            (
                "design_id_offset",
                self.design.as_ref().and_then(|design| design.id.offset),
            ),
            (
                "design_selector.byte_offset",
                self.design
                    .as_ref()
                    .and_then(|design| design.selector)
                    .and_then(|selector| selector.byte_offset),
            ),
        ] {
            if let Some(offset) = offset {
                fidelity
                    .metadata
                    .insert(format!("{}.{name}", self.id), offset.to_string());
            }
        }
    }
}
impl DesignComponentNamingSpace {
    pub(crate) fn retain_source(&self, fidelity: &mut cadmpeg_ir::SourceFidelity) {
        for (name, offset) in [
            ("byte_offset", self.byte_offset),
            ("context_uuid_offset", self.context_uuid_offset),
        ] {
            if let Some(offset) = offset {
                fidelity
                    .metadata
                    .insert(format!("{}.{name}", self.id), offset.to_string());
            }
        }
    }
}

/// Serialized Design selector carried by a construction recipe.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct ConstructionRecipeSelector {
    /// Selector value.
    pub(crate) value: u32,
    /// Byte offset of `value`.
    #[serde(skip)]
    pub(crate) byte_offset: Option<u64>,
}

#[cfg(test)]
mod tests;

mod identity_rewrite;
