// SPDX-License-Identifier: Apache-2.0
//! The identity types that CADIR v11 adds.
//!
//! Each is a newtype over a string id of the form
//! `<format>:<scope>:<kind>#<key>`, as the branch's id types are.

crate::ids::id_type!(
    /// Identifies a design container: an ordered history or an unordered set.
    ContainerId
);

crate::ids::id_type!(
    /// Identifies an evaluation: the state of a scope at one position.
    EvaluationId
);

crate::ids::id_type!(
    /// Identifies a publication: a named entity that a definition publishes.
    PublicationId
);

crate::ids::id_type!(
    /// Identifies an arrangement of a scope's occurrences.
    ArrangementId
);

crate::ids::id_type!(
    /// Identifies a parameter program: an ordered list of statements that
    /// sets parameters.
    ParameterProgramId
);

crate::ids::id_type!(
    /// Identifies a sheet-metal rule.
    SheetMetalRuleId
);

crate::ids::id_type!(
    /// Identifies a constraint of a spatial sketch.
    SpatialSketchConstraintId
);

crate::ids::id_type!(
    /// Identifies a manufacturing setup.
    SetupId
);

crate::ids::id_type!(
    /// Identifies a manufacturing operation.
    OperationId
);

crate::ids::id_type!(
    /// Identifies a machining feature.
    MachiningFeatureId
);

crate::ids::id_type!(
    /// Identifies a cutting tool.
    ToolId
);

crate::ids::id_type!(
    /// Identifies a tool preset.
    PresetId
);

crate::ids::id_type!(
    /// Identifies a machine.
    MachineId
);

crate::ids::id_type!(
    /// Identifies an NC program.
    ProgramId
);

crate::ids::id_type!(
    /// Identifies a group of manufacturing operations.
    ProcessGroupId
);

crate::ids::id_type!(
    /// Identifies a process parameter.
    ProcessParameterId
);

crate::ids::id_type!(
    /// Identifies a toolpath.
    ToolpathId
);

crate::ids::id_type!(
    /// Identifies a process state: the stock of a setup at one position.
    ProcessStateId
);
