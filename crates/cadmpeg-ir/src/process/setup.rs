// SPDX-License-Identifier: Apache-2.0
//! Setups, their order of steps, and NC programs (§7.1).
//!
//! Order lives only in `Setup.steps` and `Program.steps`; ids carry no
//! order. Every operation is in exactly one setup: it is one of the setup's
//! steps, or an input of a modifier in that setup.

use std::num::NonZeroU32;

use cadmpeg_core::text::NonBlankString;
use cadmpeg_derive::DeserializeName;
#[cfg(feature = "schema")]
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use super::stock::{StockRule, Wcs};
use super::Choice;
use crate::design::ids::{MachineId, OperationId, ProgramId, SetupId};
use crate::design::selection::SetSelection;
use crate::design::units::UnitSymbol;
use crate::ids::ProductDefinitionId;
use crate::transform::Transform;

/// One placement of workpieces on a machine.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(deny_unknown_fields)]
pub struct Setup {
    /// The setup's identity.
    pub id: SetupId,
    /// The design scope the setup machines. Process picks, process parameter
    /// references, and occurrence paths resolve from this definition.
    pub scope: ProductDefinitionId,
    /// The source's name.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_name"
    )]
    pub name: Option<String>,
    /// The process family.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_job"
    )]
    pub job: Option<Choice<JobKind>>,
    /// The motion class the setup is programmed for.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_axes"
    )]
    pub axes: Option<AxisClass>,
    /// The steps in machining order, with folders.
    pub steps: Vec<Step>,
    /// The workpieces the setup places.
    pub workpieces: Vec<WorkpieceSetup>,
    /// The fixture and clamp geometry of the setup.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_fixtures"
    )]
    pub fixtures: Option<SetSelection>,
    /// The rule that states the stock.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_stock"
    )]
    pub stock: Option<Choice<StockRule>>,
    /// The work coordinate system.
    pub wcs: Wcs,
    /// The work offsets the setup is output at, in output order.
    pub work_offsets: Vec<Choice<WorkOffset>>,
    /// The machine the setup runs on.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_machine"
    )]
    pub machine: Option<MachineId>,
    /// The source's own reference to its record.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_native_ref"
    )]
    pub native_ref: Option<String>,
}

/// A process family.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(
    tag = "kind",
    content = "value",
    rename_all = "snake_case",
    deny_unknown_fields
)]
pub enum JobKind {
    /// Milling.
    #[serde(deserialize_with = "cadmpeg_core::tag_only::refuse_value")]
    Milling,
    /// Turning.
    #[serde(deserialize_with = "cadmpeg_core::tag_only::refuse_value")]
    Turning,
    /// Jet cutting.
    #[serde(deserialize_with = "cadmpeg_core::tag_only::refuse_value")]
    Jet,
    /// Wire EDM.
    #[serde(deserialize_with = "cadmpeg_core::tag_only::refuse_value")]
    WireEdm,
    /// Additive manufacturing.
    #[serde(deserialize_with = "cadmpeg_core::tag_only::refuse_value")]
    Additive,
    /// A family that no arm states, by the source's name.
    Native(NonBlankString),
}

/// The motion class a setup is programmed for.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, DeserializeName)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub enum AxisClass {
    /// Planar motion.
    Planar,
    /// Two-and-a-half-axis motion.
    TwoAndHalf,
    /// Three-axis motion.
    Three,
    /// Indexed multi-axis motion.
    Indexed,
    /// Simultaneous multi-axis motion.
    Simultaneous,
}

/// A workpiece of a setup: design bodies or occurrences, placed in setup
/// space.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(deny_unknown_fields)]
pub struct WorkpieceSetup {
    /// The design bodies or occurrences of the workpiece.
    pub workpiece: SetSelection,
    /// The map from the selected body's local frame into setup space, in
    /// place of the body's own transform. Without one, setup space is design
    /// space, and the selected bodies are read in the definition's frame.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_placement"
    )]
    pub placement: Option<Transform>,
}

/// A work offset of the control.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(
    tag = "kind",
    content = "value",
    rename_all = "snake_case",
    deny_unknown_fields
)]
pub enum WorkOffset {
    /// The source's explicit default offset.
    #[serde(deserialize_with = "cadmpeg_core::tag_only::refuse_value")]
    Default,
    /// The control's n-th work offset; 1 is G54.
    Number(NonZeroU32),
}

/// A step of a setup, in machining order.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(
    tag = "kind",
    content = "value",
    rename_all = "snake_case",
    deny_unknown_fields
)]
pub enum Step {
    /// An operation.
    Operation(OperationId),
    /// A folder of steps.
    Folder {
        /// The source's name.
        #[serde(
            default,
            skip_serializing_if = "Option::is_none",
            deserialize_with = "deserialize_name"
        )]
        name: Option<String>,
        /// The folder's steps, in order.
        steps: Vec<Step>,
    },
    /// A step that machines nothing.
    Function(FunctionStep),
}

/// A step that machines nothing: a stop, an optional stop, an operator
/// message, a work offset change, or raw NC code.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(deny_unknown_fields)]
pub struct FunctionStep {
    /// The function.
    pub function: NcFunction,
    /// Whether the step is suppressed.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_suppressed"
    )]
    pub suppressed: Option<bool>,
    /// The source's own reference to its record.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_native_ref"
    )]
    pub native_ref: Option<String>,
}

/// An NC function that machines nothing.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(
    tag = "kind",
    content = "value",
    rename_all = "snake_case",
    deny_unknown_fields
)]
pub enum NcFunction {
    /// A program stop.
    #[serde(deserialize_with = "cadmpeg_core::tag_only::refuse_value")]
    Stop,
    /// An optional stop.
    #[serde(deserialize_with = "cadmpeg_core::tag_only::refuse_value")]
    OptionalStop,
    /// An operator message.
    Message(String),
    /// A change of work offset.
    WorkOffset(WorkOffset),
    /// Raw NC code.
    Code(String),
    /// A function that no arm states, by the source's name.
    Native(NonBlankString),
}

/// An ordered NC output, independent of setups.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(deny_unknown_fields)]
pub struct Program {
    /// The program's identity.
    pub id: ProgramId,
    /// The source's name.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_name"
    )]
    pub name: Option<String>,
    /// The program's steps, in output order.
    pub steps: Vec<ProgramStep>,
    /// The source's identifier of its post processor.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_post"
    )]
    pub post: Option<NonBlankString>,
    /// The machine the program is output for.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_machine"
    )]
    pub machine: Option<MachineId>,
    /// The output file.
    pub output: ProgramOutput,
    /// The source's own reference to its record.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_native_ref"
    )]
    pub native_ref: Option<String>,
}

/// A step of a program, in output order.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(
    tag = "kind",
    content = "value",
    rename_all = "snake_case",
    deny_unknown_fields
)]
pub enum ProgramStep {
    /// All of a setup's steps, in their order.
    Setup(SetupId),
    /// An operation.
    Operation(OperationId),
    /// A step that machines nothing.
    Function(FunctionStep),
    /// A nested group of steps.
    Group {
        /// The source's name.
        #[serde(
            default,
            skip_serializing_if = "Option::is_none",
            deserialize_with = "deserialize_name"
        )]
        name: Option<String>,
        /// The group's steps, in order.
        steps: Vec<ProgramStep>,
    },
}

/// The output file of a program. A field that the source states only through
/// a path template is `None`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(deny_unknown_fields)]
pub struct ProgramOutput {
    /// The output file's name.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_filename"
    )]
    pub filename: Option<NonBlankString>,
    /// The output folder.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_folder"
    )]
    pub folder: Option<String>,
    /// The output file's extension.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_extension"
    )]
    pub extension: Option<NonBlankString>,
    /// The length unit the program is output in.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_unit"
    )]
    pub unit: Option<Choice<UnitSymbol>>,
}

cadmpeg_core::named_optional_field!(deserialize_axes, AxisClass, "axes");
cadmpeg_core::named_optional_field!(deserialize_extension, NonBlankString, "extension");
cadmpeg_core::named_optional_field!(deserialize_filename, NonBlankString, "filename");
cadmpeg_core::named_optional_field!(deserialize_fixtures, SetSelection, "fixtures");
cadmpeg_core::named_optional_field!(deserialize_folder, String, "folder");
cadmpeg_core::named_optional_field!(deserialize_job, Choice<JobKind>, "job");
cadmpeg_core::named_optional_field!(deserialize_machine, MachineId, "machine");
cadmpeg_core::named_optional_field!(deserialize_name, String, "name");
cadmpeg_core::named_optional_field!(deserialize_native_ref, String, "native_ref");
cadmpeg_core::named_optional_field!(deserialize_placement, Transform, "placement");
cadmpeg_core::named_optional_field!(deserialize_post, NonBlankString, "post");
cadmpeg_core::named_optional_field!(deserialize_stock, Choice<StockRule>, "stock");
cadmpeg_core::named_optional_field!(deserialize_suppressed, bool, "suppressed");
cadmpeg_core::named_optional_field!(deserialize_unit, Choice<UnitSymbol>, "unit");
