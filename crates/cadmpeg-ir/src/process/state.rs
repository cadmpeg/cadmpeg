// SPDX-License-Identifier: Apache-2.0
//! Process states (§7.9).
//!
//! A process state is stored workpiece geometry along the process (ISO 14649
//! `as_is`, `to_be`, `removal`).

#[cfg(feature = "schema")]
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::design::ids::{OperationId, SetupId};

/// The point of the process that a process state holds the workpiece at.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(
    tag = "kind",
    content = "value",
    rename_all = "snake_case",
    deny_unknown_fields
)]
pub enum ProcessStateOf {
    /// The setup's stock solid before its first step.
    Stock(SetupId),
    /// The workpiece after the operation: ISO `to_be` of the operation, and
    /// `as_is` of the next operation in machining order.
    After(OperationId),
    /// The volume the operation removes.
    Removal(OperationId),
}
