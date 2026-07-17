// SPDX-License-Identifier: Apache-2.0
//! Stage-2 gating: the §7 capability matrix, resolved per codec from its
//! `parser-manifest.toml`.
//!
//! Stage-2 oracles gate **progressively, per codec**, as the capability each
//! one tests lands — not as a Phase-4 big bang (doc §7, §10 Phase 4C). This
//! module is the gating-adoption layer: it names the seven matrix rows, the
//! capability that turns each one on, and — for the rows a codec adopts through
//! its own migration state — derives adoption from the manifest, the single
//! source of truth for what has landed.
//!
//! # The matrix
//!
//! | Capability reached | Newly gating oracle |
//! |---|---|
//! | shared session (0B, all codecs) | [`Stage2Oracle::NoBypass`] |
//! | container migration (Phase 1) | [`Stage2Oracle::ResourceClassification`] |
//! | commit-API migration (Phase 2) | [`Stage2Oracle::StrictTruncation`] |
//! | budgeted leaf migration (Phase 2) | [`Stage2Oracle::BudgetEnforcement`] |
//! | L1/L2 ledger (Phase 3C/3E) | [`Stage2Oracle::ByteAccounting`] |
//! | ticket issuance (Phase 3D) | [`Stage2Oracle::DispositionValidation`] |
//! | typed lossy builder (Phase 4B) | [`Stage2Oracle::NoSilentFallback`] |
//!
//! # What 4C turns on
//!
//! The first four rows are in force for every codec from 0B/Phase 1/Phase 2
//! onward (all landed, doc §10), so [`CodecStage2Status`] reports them adopted
//! unconditionally. Phase 4C completes gating by turning on the last three rows
//! **per codec, keyed on the manifest**:
//!
//! - **byte-accounting validation** once the codec carries an L1/L2 ledger
//!   module (`ledger_level >= 1`);
//! - **disposition validation** once the codec issues and resolves record
//!   tickets (a migrated [`TICKET_MODULE`] entry, Phase 3D);
//! - **no silent fallback/drop** once the codec constructs lossy IR through the
//!   typed builder (a migrated [`BUILDER_MODULE`] entry, Phase 4B).
//!
//! A row stays off where the capability is genuinely not adopted; the manifest
//! is the single source of that truth, so a codec advancing (or a module
//! regressing to `legacy`) moves the gate with it, never a hand-maintained
//! list here.
//!
//! # Residual
//!
//! The runtime predicates ([`byte_accounting`], [`disposition_validation`])
//! judge one already-produced [`DecodeReport`]; wiring them across the
//! subprocess boundary so the stage-2 gate runs the full corpus under isolation
//! needs the child runner to serialize the decode report back, which the
//! stage-1 wire protocol does not yet carry. Until then the adoption matrix is
//! gated deterministically from the manifests and the predicates run in-process
//! against the curated fixtures.

use std::fs;
use std::path::{Path, PathBuf};

use cadmpeg_ir::{DecodeReport, LedgerLevel, LossCode};

use crate::execute::CODEC_IDS;
use crate::oracle::OracleStatus;

/// The `src`-relative basename of the module a codec adds when it issues and
/// resolves record tickets at the commit boundary (doc §6.2 / §10 Phase 3D).
/// A migrated entry with this basename is the manifest signal that disposition
/// validation has landed.
pub const TICKET_MODULE: &str = "tickets.rs";

/// The `src`-relative basename of the module a codec adds when lossy IR is
/// constructed only through the typed builder (doc §10 Phase 4B). A migrated
/// entry with this basename is the manifest signal that the no-silent-fallback
/// oracle can gate.
pub const BUILDER_MODULE: &str = "builder.rs";

/// The seven stage-2 oracle rows of the §7 capability matrix, in matrix order.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Stage2Oracle {
    /// No decode bypasses the shared session; the root-input limit holds; no
    /// `Ok` escapes a fused context.
    NoBypass,
    /// Container expansion is classified and a `ResourceLimit` is never
    /// reported as `Malformed`.
    ResourceClassification,
    /// Post-commit truncations classify as `Truncated` in strict mode (§3.3).
    StrictTruncation,
    /// Allocation, work, and depth limits are enforced by the budgeted leaves.
    BudgetEnforcement,
    /// A successful salvage decode's source-fidelity ledger validates byte
    /// conservation at the codec's adopted level (§6.1).
    ByteAccounting,
    /// A successful salvage decode's record dispositions account consistently
    /// against the ledger and losses (§6.2).
    DispositionValidation,
    /// No fallback or drop happens silently: every one carries a stable loss
    /// code (§10 Phase 4B).
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

    /// The stable label used in gate output and baselines.
    pub fn label(self) -> &'static str {
        match self {
            Stage2Oracle::NoBypass => "no_bypass",
            Stage2Oracle::ResourceClassification => "resource_classification",
            Stage2Oracle::StrictTruncation => "strict_truncation",
            Stage2Oracle::BudgetEnforcement => "budget_enforcement",
            Stage2Oracle::ByteAccounting => "byte_accounting",
            Stage2Oracle::DispositionValidation => "disposition_validation",
            Stage2Oracle::NoSilentFallback => "no_silent_fallback",
        }
    }

    /// The capability whose landing turns this oracle on.
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

/// The migration capability a stage-2 oracle keys on.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Capability {
    /// The shared decode session (0B, every codec).
    SharedSession,
    /// Container migration through the derived-space expander (Phase 1).
    ContainerMigration,
    /// Commit-API migration for post-commit truncation classification (Phase 2).
    CommitApiMigration,
    /// Budgeted leaf migration enforcing alloc/work/depth (Phase 2).
    BudgetedLeafMigration,
    /// An L1/L2 source-fidelity ledger (Phase 3C/3E).
    LedgerAccounting,
    /// Record-ticket issuance and resolution (Phase 3D).
    TicketIssuance,
    /// Typed lossy construction at the named boundaries (Phase 4B).
    TypedLossyBuilder,
}

/// One codec's adopted stage-2 capabilities, resolved from its manifest.
///
/// The first four matrix rows are in force for every codec since 0B/Phase 1/
/// Phase 2 landed, so they are not tracked here; [`Self::adopts`] reports them
/// adopted unconditionally. The three fields below are the Phase-3+/4C rows the
/// manifest drives per codec.
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
    /// A migrated [`BUILDER_MODULE`] entry is present. Adopts
    /// [`Capability::TypedLossyBuilder`].
    pub typed_lossy_builder: bool,
}

impl CodecStage2Status {
    /// Whether the codec has adopted `capability`.
    ///
    /// The four pre-Phase-3 capabilities are adopted by every codec (their
    /// phases all landed); the three later ones read from the manifest-derived
    /// fields.
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
}

/// One parsed `[[module]]` row: the fields the stage-2 derivation reads.
struct ManifestModule {
    /// The `src`-relative basename (`tickets.rs`, `brep/spline.rs`, ...).
    basename: String,
    /// `migrated` or `legacy`.
    migrated: bool,
    /// The declared `ledger_level`, defaulting to 0 when absent.
    ledger_level: u8,
}

/// Parse the `[[module]]` rows a stage-2 derivation needs from manifest text.
///
/// Textual, matching the platform's manifest-completeness parser: a manifest is
/// simple enough that a line scanner is more legible than a full TOML dependency
/// here, and the fields read (`path`, `status`, `ledger_level`) are flat scalars.
fn parse_modules(text: &str) -> Vec<ManifestModule> {
    let mut modules = Vec::new();
    let mut current: Option<(String, bool, u8)> = None;
    let flush = |slot: &mut Option<(String, bool, u8)>, out: &mut Vec<ManifestModule>| {
        if let Some((basename, migrated, ledger_level)) = slot.take() {
            out.push(ManifestModule {
                basename,
                migrated,
                ledger_level,
            });
        }
    };
    for line in text.lines() {
        let line = line.trim();
        if line.starts_with('#') {
            continue;
        }
        if line == "[[module]]" {
            flush(&mut current, &mut modules);
            current = Some((String::new(), false, 0));
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
    CodecStage2Status {
        codec_id: codec_id.to_string(),
        ledger_level,
        ticket_issuance: has_migrated(TICKET_MODULE),
        typed_lossy_builder: has_migrated(BUILDER_MODULE),
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

/// Byte-accounting oracle over one successful decode report (§6.1).
///
/// A codec that adopted ledger accounting must emit a validated source-fidelity
/// ledger at or above `adopted_level`. A missing ledger, a level below the
/// adopted one, or a conservation-invariant violation is a
/// [`OracleStatus::Fail`].
pub fn byte_accounting(report: &DecodeReport, adopted_level: u8) -> OracleStatus {
    let Some(fidelity) = report.source_fidelity.as_ref() else {
        return OracleStatus::Fail;
    };
    if level_rank(fidelity.level) < adopted_level {
        return OracleStatus::Fail;
    }
    match fidelity.validate() {
        Ok(()) => OracleStatus::Pass,
        Err(_) => OracleStatus::Fail,
    }
}

/// Disposition oracle over one successful decode report (§6.2).
///
/// A [`LossCode::TransferAccounting`] note is the codec reporting that a
/// committed record's disposition failed accounting validation against the
/// ledger, native unknowns, or the losses. Its presence is a
/// [`OracleStatus::Fail`]; a report free of it passes.
pub fn disposition_validation(report: &DecodeReport) -> OracleStatus {
    let accounted = report
        .losses
        .iter()
        .all(|loss| loss.code != LossCode::TransferAccounting);
    if accounted {
        OracleStatus::Pass
    } else {
        OracleStatus::Fail
    }
}

/// Rank a ledger level as the `u8` the status derivation compares against.
fn level_rank(level: LedgerLevel) -> u8 {
    match level {
        LedgerLevel::L0 => 0,
        LedgerLevel::L1 => 1,
        LedgerLevel::L2 => 2,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use cadmpeg_ir::{LossCategory, LossNote, ProfileVersions, Severity, SourceFidelity};

    fn report_with(losses: Vec<LossNote>, fidelity: Option<SourceFidelity>) -> DecodeReport {
        DecodeReport {
            format: "test".to_string(),
            container_only: false,
            geometry_transferred: false,
            losses,
            notes: Vec::new(),
            retention_degraded: false,
            profile_versions: ProfileVersions::default(),
            source_fidelity: fidelity,
        }
    }

    fn loss(code: LossCode) -> LossNote {
        LossNote {
            code,
            category: LossCategory::Other,
            severity: Severity::Warning,
            message: String::new(),
            provenance: None,
        }
    }

    #[test]
    fn oracle_labels_are_stable_and_unique() {
        let mut labels: Vec<&str> = Stage2Oracle::ALL.iter().map(|o| o.label()).collect();
        labels.sort_unstable();
        labels.dedup();
        assert_eq!(labels.len(), Stage2Oracle::ALL.len());
    }

    #[test]
    fn base_capabilities_always_adopted() {
        let status = CodecStage2Status {
            codec_id: "x".to_string(),
            ledger_level: 0,
            ticket_issuance: false,
            typed_lossy_builder: false,
        };
        // The first four rows gate even with no manifest-derived capability.
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
    fn disposition_fails_on_transfer_accounting_loss() {
        let clean = report_with(vec![loss(LossCode::CarrierSummary)], None);
        assert_eq!(disposition_validation(&clean), OracleStatus::Pass);
        let broken = report_with(vec![loss(LossCode::TransferAccounting)], None);
        assert_eq!(disposition_validation(&broken), OracleStatus::Fail);
    }

    #[test]
    fn byte_accounting_fails_without_a_ledger() {
        let report = report_with(Vec::new(), None);
        assert_eq!(byte_accounting(&report, 1), OracleStatus::Fail);
    }
}
