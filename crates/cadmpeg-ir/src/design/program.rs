// SPDX-License-Identifier: Apache-2.0
//! Parameter programs: ordered statements that set parameters.
//!
//! Statements run in order; a later assignment replaces an earlier one; a
//! parameter that no statement on the taken path assigns keeps its cached
//! value. Each parameter a program assigns has the definition
//! `Assigned(program)`.

use cadmpeg_core::text::NonBlankString;
#[cfg(feature = "schema")]
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use super::dimension::ParameterRef;
use super::expr::{ExpressionDialect, ExpressionToken, Neutral};
use super::ids::ParameterProgramId;
use crate::features::FeatureId;
use crate::ids::ProductDefinitionId;

/// An ordered list of statements that sets parameters.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(deny_unknown_fields)]
pub struct ParameterProgram {
    /// The program's identity.
    pub id: ParameterProgramId,
    /// The product definition whose scope holds the program.
    pub scope: ProductDefinitionId,
    /// The feature a feature-level program belongs to; it runs when its
    /// owner evaluates.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_owner"
    )]
    pub owner: Option<FeatureId>,
    /// When a program with no owner runs. `None` means not stated, and a
    /// program with an owner has none.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_stage"
    )]
    pub stage: Option<ProgramStage>,
    /// The program's name.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_name"
    )]
    pub name: Option<NonBlankString>,
    /// The program's statements.
    pub body: ProgramBody,
    /// The program's native record.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_native_ref"
    )]
    pub native_ref: Option<String>,
}

/// When a program with no owner runs.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, cadmpeg_derive::DeserializeTagged)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(
    tag = "kind",
    content = "value",
    rename_all = "snake_case",
    deny_unknown_fields
)]
pub enum ProgramStage {
    /// Before the first member of the scope's history.
    #[serde(deserialize_with = "cadmpeg_core::tag_only::refuse_value")]
    Initial,
    /// After the last member, reading the final state.
    #[serde(deserialize_with = "cadmpeg_core::tag_only::refuse_value")]
    PostRegeneration,
    /// A timing whose meaning is not established.
    Native(NonBlankString),
}

/// A program's statements: neutral when every statement translates, the
/// source's text otherwise.
#[derive(Debug, Clone, PartialEq, Serialize, cadmpeg_derive::DeserializeTagged)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(
    tag = "kind",
    content = "value",
    rename_all = "snake_case",
    deny_unknown_fields
)]
pub enum ProgramBody {
    /// Neutral statements.
    Neutral(Vec<Statement>),
    /// The source's own text.
    Source {
        /// The source's language.
        dialect: ExpressionDialect,
        /// The text, with its parameter references.
        tokens: Vec<ExpressionToken<ParameterRef>>,
    },
}

/// One neutral statement of a program.
///
/// Statement nesting has no ceiling of its own; the JSON depth limit bounds
/// it.
#[derive(Debug, Clone, PartialEq, Serialize, cadmpeg_derive::DeserializeTagged)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum Statement {
    /// Assigns a value to a parameter the program assigns.
    Assign {
        /// The assigned parameter.
        target: ParameterRef,
        /// The assigned value.
        value: Neutral,
    },
    /// Runs one of two statement lists by a condition.
    If {
        /// The condition.
        condition: Neutral,
        /// The statements run where the condition holds.
        then: Vec<Statement>,
        /// The statements run where it does not.
        otherwise: Vec<Statement>,
    },
}

cadmpeg_core::named_optional_field!(deserialize_name<T>, T, "name");
cadmpeg_core::named_optional_field!(deserialize_native_ref<T>, T, "native_ref");
cadmpeg_core::named_optional_field!(deserialize_owner<T>, T, "owner");
cadmpeg_core::named_optional_field!(deserialize_stage<T>, T, "stage");
