// SPDX-License-Identifier: Apache-2.0
//! Per-codec stage-2 oracle selection from parser manifests.

use std::fs;
use std::path::{Path, PathBuf};

use crate::execute::{ReportSummary, CODEC_IDS};

/// Legacy manifest signal for record-ticket support.
pub const TICKET_MODULE: &str = "tickets.rs";

/// The manifest capability flag a codec sets on the module that issues and
/// resolves record tickets.
pub const RECORD_TICKETS_FLAG: &str = "record_tickets";

/// The seven stage-2 oracle rows, in matrix order.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Stage2Oracle {
    /// No decode bypasses the shared session; the root-input limit holds; no
    /// `Ok` escapes a fused context.
    NoBypass,
    /// Container expansion is classified and a `ResourceLimit` is never
    /// reported as `Malformed`.
    ResourceClassification,
    /// Post-commit truncations classify as `Truncated` in strict mode.
    StrictTruncation,
    /// Allocation, work, and depth limits are enforced by the budgeted leaves.
    BudgetEnforcement,
    /// A successful salvage decode's source-fidelity ledger validates byte
    /// conservation at the codec's adopted level.
    ByteAccounting,
    /// A successful salvage decode's record dispositions account consistently
    /// against the ledger and losses.
    DispositionValidation,
    /// Every fallback or drop carries a stable loss code.
    NoSilentFallback,
}

impl Stage2Oracle {
    /// Every oracle, in matrix order.
    pub const ALL: [Stage2Oracle; 7] = [
        Stage2Oracle::NoBypass,
        Stage2Oracle::ResourceClassification,
        Stage2Oracle::StrictTruncation,
        Stage2Oracle::BudgetEnforcement,
        Stage2Oracle::ByteAccounting,
        Stage2Oracle::DispositionValidation,
        Stage2Oracle::NoSilentFallback,
    ];

    /// The capability required by this oracle.
    pub fn capability(self) -> Capability {
        match self {
            Stage2Oracle::NoBypass => Capability::SharedSession,
            Stage2Oracle::ResourceClassification => Capability::ContainerMigration,
            Stage2Oracle::StrictTruncation => Capability::CommitApiMigration,
            Stage2Oracle::BudgetEnforcement => Capability::BudgetedLeafMigration,
            Stage2Oracle::ByteAccounting => Capability::LedgerAccounting,
            Stage2Oracle::DispositionValidation => Capability::TicketIssuance,
            Stage2Oracle::NoSilentFallback => Capability::TypedLossyBuilder,
        }
    }
}

/// A capability required by a stage-2 oracle.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Capability {
    /// The shared decode session.
    SharedSession,
    /// Container expansion through derived spaces.
    ContainerMigration,
    /// Post-commit truncation classification.
    CommitApiMigration,
    /// Allocation, work, and depth enforcement in leaf parsers.
    BudgetedLeafMigration,
    /// Source-fidelity ledger accounting.
    LedgerAccounting,
    /// Record-ticket issuance and resolution.
    TicketIssuance,
    /// Typed lossy construction.
    TypedLossyBuilder,
}

/// One codec's adopted stage-2 capabilities, resolved from its manifest.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CodecStage2Status {
    /// The codec id.
    pub codec_id: String,
    /// The highest ledger level any module reaches (0 = no ledger). Non-zero
    /// adopts [`Capability::LedgerAccounting`].
    pub ledger_level: u8,
    /// A migrated [`TICKET_MODULE`] entry is present. Adopts
    /// [`Capability::TicketIssuance`].
    pub ticket_issuance: bool,
    /// Any module is flagged `semantic_builder` — the codec constructs its
    /// lossy IR through the platform typed builder. Adopts
    /// [`Capability::TypedLossyBuilder`].
    pub typed_lossy_builder: bool,
}

impl CodecStage2Status {
    /// Whether the codec has adopted `capability`.
    pub fn adopts(&self, capability: Capability) -> bool {
        match capability {
            Capability::SharedSession
            | Capability::ContainerMigration
            | Capability::CommitApiMigration
            | Capability::BudgetedLeafMigration => true,
            Capability::LedgerAccounting => self.ledger_level >= 1,
            Capability::TicketIssuance => self.ticket_issuance,
            Capability::TypedLossyBuilder => self.typed_lossy_builder,
        }
    }

    /// The stage-2 oracles that gate for this codec, in matrix order.
    pub fn gating_oracles(&self) -> Vec<Stage2Oracle> {
        Stage2Oracle::ALL
            .into_iter()
            .filter(|oracle| self.adopts(oracle.capability()))
            .collect()
    }

    /// Judge a successful decode's [`ReportSummary`] against the stage-2 report
    /// oracles this codec has adopted, returning every runtime violation.
    ///
    /// Two rows are judgeable from the summary:
    ///
    /// - [`Stage2Oracle::ByteAccounting`], for a codec that adopts a ledger:
    ///   when a decode produces a source-fidelity ledger it must validate
    ///   (byte conservation over the source spaces). A run that produced
    ///   no ledger for this fixture is not judged.
    /// - [`Stage2Oracle::NoSilentFallback`], for a codec that adopts the typed
    ///   builder: a retention degradation must be paired with a loss note, so no
    ///   drop is silent.
    ///
    /// [`Stage2Oracle::DispositionValidation`] is adopted as a gating row but its
    /// per-record disposition accounting is not carried in the summary, so it is
    /// not judged here.
    pub fn judge_report(&self, report: &ReportSummary) -> Vec<ReportViolation> {
        let mut out = Vec::new();
        if self.adopts(Capability::LedgerAccounting)
            && report.ledger_present
            && !report.ledger_valid
        {
            out.push(ReportViolation {
                oracle: Stage2Oracle::ByteAccounting,
                detail: "source-fidelity ledger failed validation".to_owned(),
            });
        }
        if self.adopts(Capability::TypedLossyBuilder)
            && report.retention_degraded
            && report.losses == 0
        {
            out.push(ReportViolation {
                oracle: Stage2Oracle::NoSilentFallback,
                detail: "retention degraded to accounted with no paired loss note".to_owned(),
            });
        }
        out
    }
}

/// One runtime stage-2 report-oracle violation found by
/// [`CodecStage2Status::judge_report`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReportViolation {
    /// The violated oracle.
    pub oracle: Stage2Oracle,
    /// Human-readable detail.
    pub detail: String,
}

/// One parsed `[[module]]` row: the fields the stage-2 derivation reads.
struct ManifestModule {
    /// The `src`-relative basename (`tickets.rs`, `brep/spline.rs`, ...).
    basename: String,
    /// `migrated` or `legacy`.
    migrated: bool,
    /// The declared `ledger_level`, defaulting to 0 when absent.
    ledger_level: u8,
    /// The module declares `semantic_builder = true`: it constructs lossy IR
    /// through the platform typed builder.
    semantic_builder: bool,
    /// The module declares `record_tickets = true`: it issues and resolves
    /// record tickets at the commit boundary.
    record_tickets: bool,
}

/// Parse the `[[module]]` rows a stage-2 derivation needs from manifest text.
///
/// Textual, matching the platform's manifest-completeness parser: a manifest is
/// simple enough that a line scanner is more legible than a full TOML dependency
/// here, and the fields read (`path`, `status`, `ledger_level`,
/// `semantic_builder`) are flat scalars. Lines inside a TOML multi-line (`"""`)
/// string are skipped so a `key = value` spelling in prose cannot masquerade as
/// a structural directive.
fn parse_modules(text: &str) -> Vec<ManifestModule> {
    let mut modules = Vec::new();
    let mut current: Option<(String, bool, u8, bool, bool)> = None;
    let flush = |slot: &mut Option<(String, bool, u8, bool, bool)>,
                 out: &mut Vec<ManifestModule>| {
        if let Some((basename, migrated, ledger_level, semantic_builder, record_tickets)) =
            slot.take()
        {
            out.push(ManifestModule {
                basename,
                migrated,
                ledger_level,
                semantic_builder,
                record_tickets,
            });
        }
    };
    let mut in_multiline = false;
    for line in text.lines() {
        // A line with an odd number of `"""` delimiters opens or closes a
        // multi-line basic string; its body is prose, never structure.
        if line.matches("\"\"\"").count() % 2 == 1 {
            in_multiline = !in_multiline;
            continue;
        }
        if in_multiline {
            continue;
        }
        let line = line.trim();
        if line.starts_with('#') {
            continue;
        }
        if line == "[[module]]" {
            flush(&mut current, &mut modules);
            current = Some((String::new(), false, 0, false, false));
            continue;
        }
        let Some((key, value)) = line.split_once('=') else {
            continue;
        };
        let Some(entry) = current.as_mut() else {
            continue;
        };
        let key = key.trim();
        let value = value.trim().trim_matches('"');
        match key {
            "path" => {
                entry.0 = value
                    .rsplit_once("/src/")
                    .map_or_else(|| value.to_string(), |(_, tail)| tail.to_string());
            }
            "status" => entry.1 = value == "migrated",
            "ledger_level" => entry.2 = value.parse().unwrap_or(0),
            "semantic_builder" => entry.3 = value == "true",
            RECORD_TICKETS_FLAG => entry.4 = value == "true",
            _ => {}
        }
    }
    flush(&mut current, &mut modules);
    modules
}

/// Derive a codec's stage-2 status from its manifest text.
pub fn status_from_manifest_text(codec_id: &str, text: &str) -> CodecStage2Status {
    let modules = parse_modules(text);
    let ledger_level = modules
        .iter()
        .filter(|m| m.migrated)
        .map(|m| m.ledger_level)
        .max()
        .unwrap_or(0);
    let has_migrated =
        |basename: &str| modules.iter().any(|m| m.migrated && m.basename == basename);
    let ticket_issuance = modules.iter().any(|m| m.record_tickets) || has_migrated(TICKET_MODULE);
    CodecStage2Status {
        codec_id: codec_id.to_string(),
        ledger_level,
        ticket_issuance,
        typed_lossy_builder: modules.iter().any(|m| m.semantic_builder),
    }
}

/// The workspace root, two levels above this crate's manifest directory.
pub fn workspace_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(Path::parent)
        .expect("workspace root is two levels above the crate manifest dir")
        .to_path_buf()
}

/// The `parser-manifest.toml` path for `codec_id` under `root`.
pub fn manifest_path(root: &Path, codec_id: &str) -> PathBuf {
    root.join(format!(
        "crates/cadmpeg-codec-{codec_id}/parser-manifest.toml"
    ))
}

/// Read a codec's stage-2 status from its manifest on disk under `root`.
pub fn status_from_manifest(root: &Path, codec_id: &str) -> std::io::Result<CodecStage2Status> {
    let text = fs::read_to_string(manifest_path(root, codec_id))?;
    Ok(status_from_manifest_text(codec_id, &text))
}

/// Read every codec's stage-2 status from the workspace manifests, in
/// [`CODEC_IDS`] order.
pub fn statuses(root: &Path) -> std::io::Result<Vec<CodecStage2Status>> {
    CODEC_IDS
        .iter()
        .map(|id| status_from_manifest(root, id))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn base_capabilities_always_adopted() {
        let status = CodecStage2Status {
            codec_id: "x".to_string(),
            ledger_level: 0,
            ticket_issuance: false,
            typed_lossy_builder: false,
        };
        assert_eq!(
            status.gating_oracles(),
            vec![
                Stage2Oracle::NoBypass,
                Stage2Oracle::ResourceClassification,
                Stage2Oracle::StrictTruncation,
                Stage2Oracle::BudgetEnforcement,
            ]
        );
    }

    #[test]
    fn ledger_turns_on_byte_accounting_only() {
        let status = CodecStage2Status {
            codec_id: "x".to_string(),
            ledger_level: 1,
            ticket_issuance: false,
            typed_lossy_builder: false,
        };
        assert!(status.adopts(Capability::LedgerAccounting));
        assert!(status
            .gating_oracles()
            .contains(&Stage2Oracle::ByteAccounting));
        assert!(!status
            .gating_oracles()
            .contains(&Stage2Oracle::DispositionValidation));
        assert!(!status
            .gating_oracles()
            .contains(&Stage2Oracle::NoSilentFallback));
    }

    #[test]
    fn tickets_turn_on_disposition() {
        let status = CodecStage2Status {
            codec_id: "x".to_string(),
            ledger_level: 1,
            ticket_issuance: true,
            typed_lossy_builder: false,
        };
        assert!(status
            .gating_oracles()
            .contains(&Stage2Oracle::DispositionValidation));
    }

    #[test]
    fn parse_derives_level_ticket_and_builder() {
        let text = r#"
[[module]]
path = "crates/cadmpeg-codec-x/src/leaf.rs"
status = "legacy"
ledger_level = 0

[[module]]
path = "crates/cadmpeg-codec-x/src/fidelity.rs"
status = "migrated"
ledger_level = 2

[[module]]
path = "crates/cadmpeg-codec-x/src/tickets.rs"
status = "migrated"
ledger_level = 1
"#;
        let status = status_from_manifest_text("x", text);
        assert_eq!(status.ledger_level, 2);
        assert!(status.ticket_issuance);
        assert!(!status.typed_lossy_builder);
    }

    #[test]
    fn parse_ignores_legacy_ticket_module() {
        let text = r#"
[[module]]
path = "crates/cadmpeg-codec-x/src/tickets.rs"
status = "legacy"
ledger_level = 0
"#;
        let status = status_from_manifest_text("x", text);
        assert!(!status.ticket_issuance);
        assert_eq!(status.ledger_level, 0);
    }

    #[test]
    fn semantic_builder_flag_adopts_regardless_of_module_name_or_status() {
        let text = r#"
[[module]]
path = "crates/cadmpeg-codec-x/src/b5_transfer.rs"
status = "legacy"
ledger_level = 0
semantic_builder = true
"#;
        let status = status_from_manifest_text("x", text);
        assert!(status.typed_lossy_builder);
        assert!(status
            .gating_oracles()
            .contains(&Stage2Oracle::NoSilentFallback));
    }

    #[test]
    fn record_tickets_flag_adopts_regardless_of_module_name_or_status() {
        let text = r#"
[[module]]
path = "crates/cadmpeg-codec-x/src/decode.rs"
status = "legacy"
ledger_level = 0
record_tickets = true
"#;
        let status = status_from_manifest_text("x", text);
        assert!(status.ticket_issuance);
        assert!(status
            .gating_oracles()
            .contains(&Stage2Oracle::DispositionValidation));
    }

    fn summary(
        ledger_present: bool,
        ledger_valid: bool,
        retention_degraded: bool,
        losses: usize,
    ) -> ReportSummary {
        ReportSummary {
            losses,
            error_losses: 0,
            ledger_present,
            ledger_valid,
            retention_degraded,
        }
    }

    #[test]
    fn byte_accounting_flags_an_invalid_ledger_for_a_ledger_codec() {
        let status = CodecStage2Status {
            codec_id: "x".to_string(),
            ledger_level: 1,
            ticket_issuance: false,
            typed_lossy_builder: false,
        };
        let violations = status.judge_report(&summary(true, false, false, 0));
        assert_eq!(violations.len(), 1);
        assert_eq!(violations[0].oracle, Stage2Oracle::ByteAccounting);
        assert!(status
            .judge_report(&summary(true, true, false, 0))
            .is_empty());
        assert!(status
            .judge_report(&summary(false, true, false, 0))
            .is_empty());
    }

    #[test]
    fn byte_accounting_does_not_judge_a_ledgerless_codec() {
        let status = CodecStage2Status {
            codec_id: "x".to_string(),
            ledger_level: 0,
            ticket_issuance: false,
            typed_lossy_builder: false,
        };
        assert!(status
            .judge_report(&summary(true, false, false, 0))
            .is_empty());
    }

    #[test]
    fn no_silent_fallback_flags_an_unpaired_retention_degradation() {
        let status = CodecStage2Status {
            codec_id: "x".to_string(),
            ledger_level: 0,
            ticket_issuance: false,
            typed_lossy_builder: true,
        };
        let violations = status.judge_report(&summary(false, true, true, 0));
        assert_eq!(violations.len(), 1);
        assert_eq!(violations[0].oracle, Stage2Oracle::NoSilentFallback);
        assert!(status
            .judge_report(&summary(false, true, true, 1))
            .is_empty());
    }

    #[test]
    fn parse_skips_multiline_string_bodies() {
        // A `status = "migrated"` spelling inside a multi-line prose string must
        // not be read as a structural directive.
        let text = r#"
[[module]]
path = "crates/cadmpeg-codec-x/src/leaf.rs"
status = "legacy"
legacy_reason = """
This module is legacy. A stray directive in prose:
status = "migrated"
semantic_builder = true
"""
ledger_level = 0
"#;
        let status = status_from_manifest_text("x", text);
        assert!(!status.typed_lossy_builder);
        assert_eq!(status.ledger_level, 0);
    }
}
