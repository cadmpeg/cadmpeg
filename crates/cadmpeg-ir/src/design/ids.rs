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
    /// Identifies an arrangement of a scope's occurrences.
    ArrangementId
);

crate::ids::id_type!(
    /// Identifies a sheet-metal rule.
    SheetMetalRuleId
);

crate::ids::id_type!(
    /// Identifies a constraint of a spatial sketch.
    SpatialSketchConstraintId
);
