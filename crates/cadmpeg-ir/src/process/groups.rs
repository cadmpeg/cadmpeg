// SPDX-License-Identifier: Apache-2.0
//! Process parameters, their definitions and owners, and groups (§7.7).
//!
//! A group is a parent that operations share for values and geometry, and
//! that they inherit from per value. An operation lists at most one group of
//! each kind, and groups nest through their parent.

use cadmpeg_core::text::NonBlankString;
#[cfg(feature = "schema")]
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use super::operation::ProcessGeometry;
use crate::design::dimension::ParamRef;
use crate::design::expr::Expression;
use crate::design::ids::{
    MachineId, MachiningFeatureId, OperationId, PresetId, ProcessGroupId, ProcessParameterId,
    ProgramId, SetupId, ToolId,
};
use crate::design::parameter::ParameterValue;
use crate::design::units::ValueKind;

/// A named value of a process record.
///
/// A value with no typed field is a parameter. A typed field binds a
/// parameter where the source gives its value an expression or a name other
/// expressions can refer to.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(deny_unknown_fields)]
pub struct ProcessParameter {
    /// The parameter's identity.
    pub id: ProcessParameterId,
    /// The record that owns the parameter.
    pub owner: ProcessOwner,
    /// The source's name, unique per owner.
    pub name: NonBlankString,
    /// The declared quantity. `None` states a number whose quantity the
    /// decoder has not established: its value is the source's number,
    /// unconverted, with meaning only to a writer of the same codec.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_kind"
    )]
    pub kind: Option<ValueKind>,
    /// How the value is determined.
    pub definition: ProcessDefinition,
    /// The cache of the definition, in CADIR units.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_value"
    )]
    pub value: Option<ParameterValue>,
    /// The source's own reference to its record.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_native_ref"
    )]
    pub native_ref: Option<String>,
}

/// The definition of a process parameter.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(
    tag = "kind",
    content = "value",
    rename_all = "snake_case",
    deny_unknown_fields
)]
pub enum ProcessDefinition {
    /// An expression in the source's language. The expression is canonical
    /// and the parameter's value is its cache.
    Expression(Expression<ParamRef>),
    /// A value that the application computes from stock, model, or tool and
    /// stores; expressions bind it.
    #[serde(deserialize_with = "cadmpeg_core::tag_only::refuse_value")]
    Computed,
}

/// The process record that owns a process parameter. Names are unique per
/// owner.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(
    tag = "kind",
    content = "value",
    rename_all = "snake_case",
    deny_unknown_fields
)]
pub enum ProcessOwner {
    /// A setup.
    Setup(SetupId),
    /// An operation.
    Operation(OperationId),
    /// A machining feature.
    Feature(MachiningFeatureId),
    /// A group.
    Group(ProcessGroupId),
    /// A tool.
    Tool(ToolId),
    /// A preset.
    Preset(PresetId),
    /// A machine.
    Machine(MachineId),
    /// An NC program.
    Program(ProgramId),
}

/// A group that operations share for values and geometry.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(deny_unknown_fields)]
pub struct ProcessGroup {
    /// The group's identity.
    pub id: ProcessGroupId,
    /// The group's kind.
    pub kind: GroupKind,
    /// The source's name.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_name"
    )]
    pub name: Option<String>,
    /// The group this group nests in.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_parent"
    )]
    pub parent: Option<ProcessGroupId>,
    /// The source's own reference to its record.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_native_ref"
    )]
    pub native_ref: Option<String>,
}

/// The kind of a group.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(
    tag = "kind",
    content = "value",
    rename_all = "snake_case",
    deny_unknown_fields
)]
pub enum GroupKind {
    /// A group whose values are parameters it owns; an operation value that
    /// inherits one binds it.
    #[serde(deserialize_with = "cadmpeg_core::tag_only::refuse_value")]
    Method,
    /// A group that holds geometry by role. An operation's geometry for a
    /// role it states no geometry for is its geometry group's, else that
    /// group's parent's.
    Geometry {
        /// The group's geometry.
        geometry: Vec<ProcessGeometry>,
    },
    /// A group of a kind no arm states, by the source's name.
    Native(NonBlankString),
}

cadmpeg_core::named_optional_field!(deserialize_kind, ValueKind, "kind");
cadmpeg_core::named_optional_field!(deserialize_name, String, "name");
cadmpeg_core::named_optional_field!(deserialize_native_ref, String, "native_ref");
cadmpeg_core::named_optional_field!(deserialize_parent, ProcessGroupId, "parent");
cadmpeg_core::named_optional_field!(deserialize_value, ParameterValue, "value");
