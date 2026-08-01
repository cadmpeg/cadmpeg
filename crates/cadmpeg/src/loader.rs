// SPDX-License-Identifier: Apache-2.0
//! Input detection and loading into CADIR.

use std::fs::File;
use std::io::Read;
use std::path::Path;

use anyhow::{anyhow, Context, Result};
use cadmpeg_ir::codec::{CodecEntry, Confidence, DecodeOptions};

use cadmpeg_ir::{decode_sidecar_path, CadIr, DecodeSidecar, DocumentArtifact, DocumentOrigin};

use crate::registry::{DetectionOutcome, Registry};
use crate::ForcedInput;

/// Leading byte window available to content-based codec detection.
pub const DETECTION_PREFIX_LEN: usize = 128 * 1024;

/// Read at most `n` leading bytes for content-based format detection.
pub fn read_prefix(path: &Path, n: usize) -> Result<Vec<u8>> {
    let mut f = File::open(path).with_context(|| format!("opening {}", path.display()))?;
    let mut buf = Vec::with_capacity(n);
    let mut chunk = [0_u8; 16 * 1024];
    while buf.len() < n {
        let remaining = n - buf.len();
        let chunk_len = remaining.min(chunk.len());
        let read = f.read(&mut chunk[..chunk_len])?;
        if read == 0 {
            break;
        }
        buf.extend_from_slice(&chunk[..read]);
    }
    Ok(buf)
}

/// Load CADIR from a native CAD file or CADIR JSON.
///
/// An explicit input format bypasses detection. Without one, the registered
/// codec with the strongest match decodes the file. An input beginning with a
/// JSON object is parsed as CADIR when no native codec recognizes it.
pub fn load_artifact(
    registry: &Registry,
    path: &Path,
    options: DecodeOptions,
    forced: Option<ForcedInput>,
) -> Result<DocumentArtifact> {
    let prefix = read_prefix(path, DETECTION_PREFIX_LEN)?;
    let detected = match forced {
        Some(ForcedInput::Codec(id)) => Some((
            registry
                .by_id(id)
                .ok_or_else(|| anyhow!("unsupported input format {id}"))?,
            None,
        )),
        Some(ForcedInput::Cadir) => None,
        None => match registry.detect(&prefix) {
            DetectionOutcome::None => None,
            DetectionOutcome::Detected {
                descriptor,
                confidence,
            } => Some((
                descriptor
                    .codec
                    .as_deref()
                    .expect("detected descriptor has codec"),
                Some(confidence),
            )),
            DetectionOutcome::Ambiguous {
                confidence,
                candidates,
            } => {
                return Err(anyhow!(
                    "ambiguous {confidence}-confidence input format: {}; pass --input-format",
                    candidates
                        .iter()
                        .map(|candidate| candidate.id)
                        .collect::<Vec<_>>()
                        .join(", ")
                ));
            }
        },
    };
    if let Some((codec, confidence)) = detected {
        if let Some(confidence) = confidence.filter(|value| *value < Confidence::High) {
            eprintln!(
                "warning: detected {} with {confidence} confidence; use --input-format to override",
                codec.id()
            );
        }
        let mut f = File::open(path).with_context(|| format!("opening {}", path.display()))?;
        let result = codec
            .decode(&mut f, &options)
            .with_context(|| format!("decoding {} as {}", path.display(), codec.id()))?;
        return Ok(DocumentArtifact::decoded(result));
    }

    if forced.is_none() && prefix.iter().find(|byte| !byte.is_ascii_whitespace()) != Some(&b'{') {
        return Err(anyhow!(
            "unrecognized format for {}; supported: FCStd, f3d, sldprt, CATPart, NX/Creo prt, Rhino 3DM, IGES, STEP, .cadir.json; use --input-format to override detection",
            path.display()
        ));
    }

    let max_bytes = options.policy.limits.max_input_bytes;
    let text = read_bounded_text(path, max_bytes)
        .with_context(|| format!("reading {} as a .cadir.json document", path.display()))?;
    let ir = CadIr::from_json(&text).map_err(|e| {
        anyhow!(
            "{} is neither a recognized CAD file nor a valid .cadir.json document: {e}",
            path.display()
        )
    })?;
    let sidecar_path = decode_sidecar_path(path);
    if !sidecar_path.exists() {
        return Ok(DocumentArtifact::neutral(ir));
    }
    let sidecar_text = read_bounded_text(&sidecar_path, max_bytes)
        .with_context(|| format!("reading decode sidecar {}", sidecar_path.display()))?;
    let sidecar = DecodeSidecar::from_json(&sidecar_text)
        .with_context(|| format!("parsing decode sidecar {}", sidecar_path.display()))?;
    if !sidecar.matches(text.as_bytes()) {
        return Err(anyhow!(
            "decode sidecar {} does not match {}",
            sidecar_path.display(),
            path.display()
        ));
    }
    Ok(DocumentArtifact {
        ir,
        origin: DocumentOrigin::Decoded {
            report: sidecar.report,
            fidelity: sidecar.fidelity,
        },
    })
}

fn read_bounded_text(path: &Path, max_bytes: u64) -> Result<String> {
    let file = File::open(path).with_context(|| format!("opening {}", path.display()))?;
    let mut limited = file.take(max_bytes.saturating_add(1));
    let mut text = String::new();
    limited
        .read_to_string(&mut text)
        .with_context(|| format!("reading UTF-8 text from {}", path.display()))?;
    if text.len() as u64 > max_bytes {
        return Err(anyhow!(
            "{} exceeds the configured {}-byte input limit",
            path.display(),
            max_bytes
        ));
    }
    Ok(text)
}

#[cfg(test)]
mod tests {
    use super::*;
    use cadmpeg_ir::units::Units;
    use cadmpeg_ir::{DecodeReport, SourceFidelity};

    #[test]
    fn matching_sidecar_restores_decoded_origin_and_mismatch_is_hard_error() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("part.cadir.json");
        let text = CadIr::empty(Units::default()).to_canonical_json().unwrap();
        std::fs::write(&path, &text).unwrap();
        let report = DecodeReport {
            format: "test".into(),
            container_only: false,
            geometry_transferred: false,
            coverage: Default::default(),
            losses: Vec::new(),
            notes: Vec::new(),
        };
        let sidecar = DecodeSidecar::bind(text.as_bytes(), report, SourceFidelity::default());
        std::fs::write(
            decode_sidecar_path(&path),
            sidecar.to_canonical_json().unwrap(),
        )
        .unwrap();

        let artifact = load_artifact(
            &Registry::with_builtins(),
            &path,
            DecodeOptions::default(),
            Some(ForcedInput::Cadir),
        )
        .unwrap();
        assert!(matches!(artifact.origin, DocumentOrigin::Decoded { .. }));

        std::fs::write(&path, format!("{text}\n")).unwrap();
        let error = load_artifact(
            &Registry::with_builtins(),
            &path,
            DecodeOptions::default(),
            Some(ForcedInput::Cadir),
        )
        .unwrap_err();
        assert!(error.to_string().contains("does not match"));
    }

    #[test]
    fn text_reader_refuses_input_above_the_configured_limit() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("large.json");
        std::fs::write(&path, "12345").unwrap();
        let error = read_bounded_text(&path, 4).unwrap_err();
        assert!(error.to_string().contains("4-byte input limit"));
    }
}
