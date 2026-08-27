// SPDX-License-Identifier: Apache-2.0
//! Input detection and loading into CADIR.

use std::fs::File;
use std::path::Path;

use anyhow::{anyhow, Context, Result};
use cadmpeg_ir::codec::{Confidence, DecodeOptions};
use cadmpeg_ir::CadIr;

use cadmpeg_registry::{ForcedInput, InputCatalog, ResolvedSource, DETECTION_PREFIX_LEN};

use crate::application::{ArtifactStore, LoadOrigin, LoadedDocument};

/// Non-fatal notice produced while loading an input.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LoadNotice {
    /// Content detection succeeded below high confidence.
    LowConfidenceDetection {
        /// Selected codec id.
        format_id: &'static str,
        /// Detection confidence.
        confidence: Confidence,
    },
}

/// A loaded document plus presentation notices for the CLI.
#[derive(Debug)]
pub struct LoadOutcome {
    /// Loaded document.
    pub document: LoadedDocument,
    /// Notices the presentation layer may print.
    pub notices: Vec<LoadNotice>,
}

/// Read the bounded byte image used for content-based format detection.
pub fn read_detection_input(path: &Path, n: usize, max_bytes: u64) -> Result<Vec<u8>> {
    ArtifactStore::read_detection_input(path, n, max_bytes)
}

/// Load CADIR from a native CAD file or CADIR JSON.
///
/// An explicit input format bypasses detection. Without one, the registered
/// codec with the strongest match decodes the file. An input beginning with a
/// JSON object is parsed as CADIR when no native codec recognizes it.
pub fn load_artifact(
    catalog: &InputCatalog,
    path: &Path,
    options: DecodeOptions,
    forced: Option<ForcedInput>,
) -> Result<LoadOutcome> {
    let prefix = ArtifactStore::read_detection_input(
        path,
        DETECTION_PREFIX_LEN,
        options.policy.limits.max_input_bytes,
    )?;
    let resolved = catalog
        .resolve_source(&prefix, forced)
        .map_err(|error| anyhow!(error.to_string()))?;
    let mut notices = Vec::new();
    match resolved {
        ResolvedSource::Native {
            codec,
            format_id,
            confidence,
        } => {
            if let Some(confidence) = confidence.filter(|value| *value < Confidence::High) {
                notices.push(LoadNotice::LowConfidenceDetection {
                    format_id,
                    confidence,
                });
            }
            let mut f = File::open(path).with_context(|| format!("opening {}", path.display()))?;
            let result = codec
                .decode(&mut f, &options)
                .with_context(|| format!("decoding {} as {}", path.display(), format_id))?;
            return Ok(LoadOutcome {
                document: LoadedDocument::decoded(result),
                notices,
            });
        }
        ResolvedSource::Cadir => {}
    }

    if forced.is_none() && prefix.iter().find(|byte| !byte.is_ascii_whitespace()) != Some(&b'{') {
        return Err(anyhow!(
            "unrecognized format for {}; supported: FCStd, f3d, Inventor IPT/IAM, sldprt, CATPart, NX/Creo prt, Rhino 3DM, IGES, STEP, ASM sat/smt/smb/sab, .cadir.json; use --input-format to override detection",
            path.display()
        ));
    }

    let max_bytes = options.policy.limits.max_input_bytes;
    let text = ArtifactStore::read_bounded_text(path, max_bytes)
        .with_context(|| format!("reading {} as a .cadir.json document", path.display()))?;
    let ir = CadIr::from_json(&text).map_err(|e| {
        anyhow!(
            "{} is neither a recognized CAD file nor a valid .cadir.json document: {e}",
            path.display()
        )
    })?;
    let Some(sidecar) = ArtifactStore::load_matching_sidecar(path, text.as_bytes(), max_bytes)?
    else {
        return Ok(LoadOutcome {
            document: LoadedDocument::neutral(ir),
            notices,
        });
    };
    Ok(LoadOutcome {
        document: LoadedDocument {
            ir,
            origin: LoadOrigin::Decoded {
                report: sidecar.report,
                fidelity: sidecar.fidelity,
            },
        },
        notices,
    })
}

#[cfg(test)]
#[allow(clippy::default_trait_access, clippy::unwrap_used)]
mod tests {
    use super::*;
    use cadmpeg_ir::units::Units;
    use cadmpeg_ir::{DecodeReport, DecodeSidecar, SourceFidelity};

    #[test]
    fn matching_sidecar_restores_decoded_origin_and_mismatch_is_hard_error() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("part.cadir.json");
        let text = CadIr::empty(Units::default()).to_canonical_json().unwrap();
        std::fs::write(&path, &text).unwrap();
        let report = DecodeReport {
            dialects: Vec::new(),
            format: "test".into(),
            container_only: false,
            geometry_transferred: false,
            coverage: Default::default(),
            transfer_ledger: cadmpeg_ir::report::TransferLedger::default(),
            losses: Vec::new(),
            notes: Vec::new(),
        };
        let sidecar = DecodeSidecar::bind(text.as_bytes(), report, SourceFidelity::default());
        std::fs::write(
            ArtifactStore::sidecar_path(&path),
            sidecar.to_canonical_json().unwrap(),
        )
        .unwrap();

        let outcome = load_artifact(
            &InputCatalog::with_builtins(),
            &path,
            DecodeOptions::default(),
            Some(ForcedInput::Cadir),
        )
        .unwrap();
        assert!(matches!(
            outcome.document.origin,
            LoadOrigin::Decoded { .. }
        ));

        std::fs::write(&path, format!("{text}\n")).unwrap();
        let error = load_artifact(
            &InputCatalog::with_builtins(),
            &path,
            DecodeOptions::default(),
            Some(ForcedInput::Cadir),
        )
        .unwrap_err();
        assert!(error.to_string().contains("does not match"));
    }
}
