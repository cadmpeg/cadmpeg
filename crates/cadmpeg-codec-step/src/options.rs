// SPDX-License-Identifier: Apache-2.0
//! Writer header metadata and unrepresentable-content policy.
//!
//! These configure *how* a target is written. Which target is written is the
//! export request's answer, resolved against the source: `StepCodec::plan`
//! takes it from `TargetRequest`, and a direct [`crate::write_step`] caller
//! names it in the call.

/// Metadata written to the STEP `FILE_NAME` header record.
///
/// Default values produce deterministic output. They identify the file as
/// `cadmpeg_model`, leave the author and organization empty, use `cadmpeg` as
/// the originating system, and substitute `1970-01-01T00:00:00` for the empty
/// timestamp.
#[derive(Debug, Clone)]
pub struct StepWriteOptions {
    /// Handling of IR content the selected writer cannot represent exactly.
    pub unsupported: StepUnsupportedPolicy,
    /// The `FILE_NAME` name field.
    ///
    /// The STEP `PRODUCT` id and name come from the first IR body name, or
    /// `cadmpeg_model` when that body has no name.
    pub product_name: String,
    /// The sole entry in the `FILE_NAME` author list.
    pub author: String,
    /// The sole entry in the `FILE_NAME` organization list.
    pub organization: String,
    /// The `FILE_NAME` timestamp.
    ///
    /// Supply an ISO 8601 value. An empty string is written as
    /// `1970-01-01T00:00:00`.
    pub timestamp: String,
    /// The `FILE_NAME` originating-system field.
    pub originating_system: String,
}

impl Default for StepWriteOptions {
    fn default() -> Self {
        StepWriteOptions {
            unsupported: StepUnsupportedPolicy::Report,
            product_name: "cadmpeg_model".to_string(),
            author: String::new(),
            organization: String::new(),
            timestamp: String::new(),
            originating_system: "cadmpeg".to_string(),
        }
    }
}

/// Policy for semantic content not representable by the selected STEP target.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum StepUnsupportedPolicy {
    /// Emit the representable subset and return machine-readable loss notes.
    #[default]
    Report,
    /// Reject the document before writing any output byte.
    Reject,
}

/// STEP application-protocol targets supported by the Part 21 writer.
///
/// The AP242 edition number and the long-form schema revision are distinct:
/// editions 1, 2, and 3 use long-form revisions 1, 3, and 4 respectively.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum StepSchema {
    /// AP203 edition 1 `CONFIG_CONTROL_DESIGN`.
    Ap203Edition1,
    /// AP203 edition 2 modular long form.
    Ap203Edition2,
    /// AP214 `AUTOMOTIVE_DESIGN`.
    #[default]
    Ap214,
    /// AP242 edition 1 modular long form.
    Ap242Edition1,
    /// AP242 edition 2 modular long form.
    Ap242Edition2,
    /// AP242 edition 3 modular long form.
    Ap242Edition3,
}

impl StepSchema {
    /// Every schema the Part 21 writer can emit, and so every row of the
    /// synthesis catalog. Resolution maps a dialect id back to a schema through
    /// this list, and `crate::codec` pins the two sets equal in both
    /// directions.
    pub(crate) const ALL: [Self; 6] = [
        Self::Ap203Edition1,
        Self::Ap203Edition2,
        Self::Ap214,
        Self::Ap242Edition1,
        Self::Ap242Edition2,
        Self::Ap242Edition3,
    ];

    /// The registry dialect id this schema writes.
    ///
    /// The spelling a caller passes as `TargetRequest::Explicit`.
    #[must_use]
    pub const fn target(self) -> &'static str {
        match self {
            Self::Ap203Edition1 => "step:ap203-e1",
            Self::Ap203Edition2 => "step:ap203-e2",
            Self::Ap214 => "step:ap214",
            Self::Ap242Edition1 => "step:ap242-e1",
            Self::Ap242Edition2 => "step:ap242-e2",
            Self::Ap242Edition3 => "step:ap242-e3",
        }
    }

    /// Exact schema identifier written in `FILE_SCHEMA`.
    pub const fn file_schema(self) -> &'static str {
        match self {
            Self::Ap203Edition1 => "CONFIG_CONTROL_DESIGN",
            Self::Ap203Edition2 => "AP203_CONFIGURATION_CONTROLLED_3D_DESIGN_OF_MECHANICAL_PARTS_AND_ASSEMBLIES_MIM_LF { 1 0 10303 403 2 1 2 }",
            Self::Ap214 => "AUTOMOTIVE_DESIGN { 1 0 10303 214 1 1 1 1 }",
            Self::Ap242Edition1 => "AP242_MANAGED_MODEL_BASED_3D_ENGINEERING_MIM_LF { 1 0 10303 442 1 1 4 }",
            Self::Ap242Edition2 => "AP242_MANAGED_MODEL_BASED_3D_ENGINEERING_MIM_LF { 1 0 10303 442 3 1 4 }",
            Self::Ap242Edition3 => "AP242_MANAGED_MODEL_BASED_3D_ENGINEERING_MIM_LF { 1 0 10303 442 4 1 4 }",
        }
    }

    pub(crate) fn ap242_edition(identifier: &str) -> Option<&'static str> {
        const NAME: &str = "AP242_MANAGED_MODEL_BASED_3D_ENGINEERING_MIM_LF";
        let (name, oid) = schema_identifier_arcs(identifier)?;
        if !name.eq_ignore_ascii_case(NAME) {
            return None;
        }
        match oid.as_deref() {
            Some([1, 0, 10303, 442, 1, 1, 4]) => Some("edition 1"),
            Some([1, 0, 10303, 442, 3, 1, 4]) => Some("edition 2"),
            Some([1, 0, 10303, 442, 4, 1, 4]) => Some("edition 3"),
            _ => None,
        }
    }

    pub(crate) const fn supports_tessellation(self) -> bool {
        matches!(
            self,
            Self::Ap242Edition1 | Self::Ap242Edition2 | Self::Ap242Edition3
        )
    }

    pub(crate) const fn supports_semantic_pmi(self) -> bool {
        self.supports_tessellation()
    }

    pub(crate) const fn supports_visibility(self) -> bool {
        !matches!(self, Self::Ap203Edition1)
    }

    pub(crate) const fn application_protocol(self) -> (&'static str, &'static str, i32) {
        match self {
            Self::Ap203Edition1 => (
                "configuration controlled 3d designs of mechanical parts and assemblies",
                "config_control_design",
                1994,
            ),
            Self::Ap203Edition2 => (
                "configuration controlled 3d designs of mechanical parts and assemblies",
                "ap203_configuration_controlled_3d_design_of_mechanical_parts_and_assemblies",
                2011,
            ),
            Self::Ap214 => ("automotive design", "automotive_design", 2000),
            Self::Ap242Edition1 => (
                "managed model based 3d engineering",
                "ap242_managed_model_based_3d_engineering",
                2014,
            ),
            Self::Ap242Edition2 => (
                "managed model based 3d engineering",
                "ap242_managed_model_based_3d_engineering",
                2020,
            ),
            Self::Ap242Edition3 => (
                "managed model based 3d engineering",
                "ap242_managed_model_based_3d_engineering",
                2022,
            ),
        }
    }
}

/// The schema name and the object identifier arcs of one schema identifier.
///
/// The edition report needs the arcs as numbers, so an object identifier with a
/// named component, with fewer than two components, or with a component that is
/// not a plain decimal number has no arcs. `split_schema_identifier` owns the
/// name and object identifier split.
fn schema_identifier_arcs(identifier: &str) -> Option<(&str, Option<Vec<u64>>)> {
    let (name, object_identifier) =
        crate::parse::schema_identifier::split_schema_identifier(identifier)?;
    let Some(object_identifier) = object_identifier else {
        return Some((name, None));
    };
    if name.is_empty() {
        return None;
    }
    let arcs = object_identifier
        .split_whitespace()
        .map(str::parse::<u64>)
        .collect::<Result<Vec<u64>, _>>()
        .ok()?;
    (arcs.len() >= 2).then_some((name, Some(arcs)))
}
