// SPDX-License-Identifier: Apache-2.0
//! Writer header metadata and target-schema selection.

/// Metadata written to the STEP `FILE_NAME` header record.
///
/// Default values produce deterministic output. They identify the file as
/// `cadmpeg_model`, leave the author and organization empty, use `cadmpeg` as
/// the originating system, and substitute `1970-01-01T00:00:00` for the empty
/// timestamp.
#[derive(Debug, Clone)]
pub struct StepWriteOptions {
    /// Application protocol and edition declared by `FILE_SCHEMA`.
    pub schema: StepSchema,
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
            schema: StepSchema::Ap214,
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
        let (name, oid) = split_schema_identifier(identifier)?;
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

fn split_schema_identifier(identifier: &str) -> Option<(&str, Option<Vec<u64>>)> {
    let identifier = identifier.trim();
    let Some(open) = identifier.rfind('{') else {
        return Some((identifier, None));
    };
    if !identifier.ends_with('}') || identifier[..open].trim().is_empty() {
        return None;
    }
    let oid = identifier[open + 1..identifier.len() - 1]
        .split_whitespace()
        .map(str::parse::<u64>)
        .collect::<Result<Vec<u64>, _>>()
        .ok()?;
    (oid.len() >= 2).then(|| (identifier[..open].trim_end(), Some(oid)))
}
