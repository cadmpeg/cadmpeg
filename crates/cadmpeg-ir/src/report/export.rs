// SPDX-License-Identifier: Apache-2.0
//! Export reports, write-path fidelity, and entity census.

use std::collections::BTreeMap;
use std::fmt;

use cadmpeg_core::dialect::DialectId;
#[cfg(feature = "schema")]
use schemars::JsonSchema;
use serde::ser::SerializeStruct;
use serde::{Deserialize, Deserializer, Serialize, Serializer};

use super::{LossNote, Severity};

/// Entity census and fidelity details from a successful export.
#[derive(Debug, Clone, PartialEq)]
pub struct ExportReport {
    identity: ExportIdentity,
    /// Entity counts and the semantic basis on which they were measured.
    pub census: EntityCensus,
    /// How decode-time source fidelity was handled.
    pub fidelity: FidelityResolution,
    /// Which write path produced the exported bytes.
    pub write_path: WritePath,
    /// Omitted, normalized, or reduced content.
    pub losses: Vec<LossNote>,
    /// Informational details about the export path.
    pub notes: Vec<String>,
}

#[derive(Debug, Clone, PartialEq)]
enum ExportIdentity {
    /// A dialect-free current export or a migrated report that predates
    /// required native targets.
    Targetless(String),
    /// A current native export, identified by its resolved target.
    Native(DialectId),
}

#[derive(Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
struct ExportReportWire {
    format: String,
    census: EntityCensus,
    fidelity: FidelityResolution,
    write_path: WritePath,
    losses: Vec<LossNote>,
    notes: Vec<String>,
    #[serde(default)]
    target: Option<DialectId>,
}

impl Serialize for ExportReport {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let mut state = serializer.serialize_struct("ExportReport", 7)?;
        state.serialize_field("format", self.format())?;
        state.serialize_field("census", &self.census)?;
        state.serialize_field("fidelity", &self.fidelity)?;
        state.serialize_field("write_path", &self.write_path)?;
        state.serialize_field("losses", &self.losses)?;
        state.serialize_field("notes", &self.notes)?;
        state.serialize_field("target", &self.target())?;
        state.end()
    }
}

impl<'de> Deserialize<'de> for ExportReport {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let wire = ExportReportWire::deserialize(deserializer)?;
        let identity = match wire.target {
            Some(target) if wire.format == "cadir" => {
                return Err(serde::de::Error::custom(format_args!(
                    "CADIR export report cannot name native dialect {:?}",
                    target.as_str()
                )))
            }
            Some(target) if target.namespace() == wire.format => ExportIdentity::Native(target),
            Some(target) => {
                return Err(serde::de::Error::custom(format_args!(
                    "format {:?} does not match classified payload format {:?}",
                    wire.format,
                    target.namespace(),
                )))
            }
            None => ExportIdentity::Targetless(wire.format),
        };
        Ok(Self {
            identity,
            census: wire.census,
            fidelity: wire.fidelity,
            write_path: wire.write_path,
            losses: wire.losses,
            notes: wire.notes,
        })
    }
}

#[cfg(feature = "schema")]
impl JsonSchema for ExportReport {
    fn schema_name() -> std::borrow::Cow<'static, str> {
        "ExportReport".into()
    }

    fn schema_id() -> std::borrow::Cow<'static, str> {
        concat!(module_path!(), "::ExportReport").into()
    }

    fn json_schema(generator: &mut schemars::SchemaGenerator) -> schemars::Schema {
        let mut schema = ExportReportWire::json_schema(generator);
        crate::schema::require_object_fields(&mut schema, ["target"]);
        schema
    }
}

#[cfg(all(test, feature = "schema"))]
mod schema_tests {
    #[test]
    fn current_export_report_schema_requires_target() {
        let schema = serde_json::to_value(schemars::schema_for!(super::ExportReport))
            .expect("export report schema serializes");
        let required = schema["required"]
            .as_array()
            .expect("export report schema has required fields");
        assert!(required.iter().any(|field| field == "target"), "{schema:#}");
    }
}

/// Which of an encoder's write paths produced the exported bytes.
///
/// An encoder that retains its source bytes has two ways to answer "write this
/// document": copy the retained bytes out, or run the writer. The two are
/// indistinguishable from the output alone whenever the writer happens to
/// reproduce the input, so a round-trip test that only compares bytes cannot say
/// which one it exercised — and a test over an unedited document takes the copy
/// path, proving nothing about the writer. This value is set at the branch the
/// encoder actually took, never derived from the output afterwards, so the
/// distinction is a fact the caller can assert on.
///
/// The variants are ordered by how much of the output the encoder authored.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(rename_all = "snake_case")]
pub enum WritePath {
    /// Retained source bytes were copied to the output unchanged. No writer code
    /// ran, so the output says nothing about the writer.
    VerbatimReplay,
    /// The writer ran and consumed retained source content, rewriting part of a
    /// container it did not author in full.
    Patched,
    /// The writer ran over neutral IR content alone, authoring every output byte.
    Synthesized,
}

impl fmt::Display for WritePath {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::VerbatimReplay => "verbatim_replay",
            Self::Patched => "patched",
            Self::Synthesized => "synthesized",
        })
    }
}

/// How an encoder resolved optional source fidelity.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(rename_all = "snake_case", tag = "status")]
pub enum FidelityResolution {
    /// The input had no decode-time fidelity state.
    NotProvided,
    /// Preserved source content was consumed successfully.
    Replayed,
    /// The encoder does not consume source fidelity.
    NotConsumed,
    /// Fidelity was available but could not be consumed.
    Degraded {
        /// Explanation of the degradation.
        reason: String,
    },
}

/// The model against which export entity counts were measured.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(rename_all = "snake_case")]
pub enum CensusBasis {
    /// Counts describe records emitted in the target format.
    TargetRecords,
    /// Counts describe input IR arenas.
    IrArenas,
}

/// Explicitly based entity counts for one export.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
pub struct EntityCensus {
    /// Semantic basis of `counts`.
    pub basis: CensusBasis,
    /// Counts keyed by arena or target-record kind.
    pub counts: BTreeMap<String, usize>,
}

impl EntityCensus {
    /// Total count across every census row.
    pub fn total(&self) -> usize {
        self.counts.values().sum()
    }
}

impl ExportReport {
    /// Returns the native format namespace, or `"cadir"` for neutral CADIR.
    #[must_use]
    pub fn format(&self) -> &str {
        match &self.identity {
            ExportIdentity::Targetless(format) => format,
            ExportIdentity::Native(target) => target.namespace(),
        }
    }

    /// The concrete native dialect written.
    ///
    /// `None` identifies neutral CADIR or a migrated native report written
    /// before export targets entered the wire format. New native reports always
    /// name a target.
    #[must_use]
    pub fn target(&self) -> Option<&DialectId> {
        match &self.identity {
            ExportIdentity::Native(target) => Some(target),
            ExportIdentity::Targetless(_) => None,
        }
    }

    /// Constructs a report for the neutral CADIR document, which has no native
    /// dialect target.
    #[must_use]
    #[cfg(test)]
    pub(crate) fn cadir(
        census: EntityCensus,
        fidelity: FidelityResolution,
        write_path: WritePath,
        losses: Vec<LossNote>,
        notes: Vec<String>,
    ) -> Self {
        Self::dialect_free("cadir", census, fidelity, write_path, losses, notes)
    }

    /// Constructs a targetless report stamped with a dialect-free encoder's
    /// format.
    pub(crate) fn dialect_free(
        format: impl Into<String>,
        census: EntityCensus,
        fidelity: FidelityResolution,
        write_path: WritePath,
        losses: Vec<LossNote>,
        notes: Vec<String>,
    ) -> Self {
        Self {
            identity: ExportIdentity::Targetless(format.into()),
            census,
            fidelity,
            write_path,
            losses,
            notes,
        }
    }

    /// Constructs a native-format report with its required dialect target.
    ///
    #[must_use]
    pub(crate) fn native(
        target: DialectId,
        census: EntityCensus,
        fidelity: FidelityResolution,
        write_path: WritePath,
        losses: Vec<LossNote>,
        notes: Vec<String>,
    ) -> Self {
        Self {
            identity: ExportIdentity::Native(target),
            census,
            fidelity,
            write_path,
            losses,
            notes,
        }
    }

    /// Count loss notes at or above [`Severity::Error`].
    pub fn error_count(&self) -> usize {
        self.losses
            .iter()
            .filter(|loss| loss.severity >= Severity::Error)
            .count()
    }
}
