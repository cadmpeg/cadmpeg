// SPDX-License-Identifier: Apache-2.0
//! Input detection and loading into CADIR.

use std::fs::File;
use std::path::Path;

use anyhow::{anyhow, Context};
use cadmpeg_ir::codec::DecodeOptions;
use cadmpeg_ir::CadIr;

use cadmpeg_registry::{
    ForcedInput, InputCatalog, ResolveSourceError, ResolvedSource, DETECTION_PREFIX_LEN,
};

use crate::application::refusal::ApplicationError;
use crate::application::{ArtifactStore, LoadedDocument};

/// Restates a detection failure with the flag that overrides it.
///
/// The registry states the fact; naming `--input-format` is this crate's job,
/// because the flag is this crate's. Ambiguity is the only resolution error.
pub fn detection_failure(error: &ResolveSourceError) -> anyhow::Error {
    anyhow!("{error}; pass --input-format")
}

/// Load CADIR from a native CAD file or CADIR JSON.
///
/// A native decode failure is classified into an [`ApplicationError`] here,
/// at the only site that knows the path and the selected codec.
///
/// An explicit input format bypasses detection. Without one, the registered
/// codec with the strongest match decodes the file. An input beginning with a
/// JSON object is parsed as CADIR when no native codec recognizes it.
pub fn load_artifact(
    catalog: &InputCatalog,
    path: &Path,
    options: DecodeOptions,
    forced: Option<ForcedInput>,
) -> Result<LoadedDocument, ApplicationError> {
    let prefix = ArtifactStore::read_detection_input(
        path,
        DETECTION_PREFIX_LEN,
        options.policy.limits.max_input_bytes,
    )?;
    let resolved = catalog
        .resolve_source(&prefix, forced)
        .map_err(|error| detection_failure(&error))?;
    match resolved {
        ResolvedSource::Native { codec, selection } => {
            let format_id = codec.id();
            let mut f = File::open(path).with_context(|| format!("opening {}", path.display()))?;
            let result = codec.decode(&mut f, &options).map_err(|failure| {
                ApplicationError::from_decode_failure(path, format_id, failure)
            })?;
            return Ok(LoadedDocument::decoded(result, selection));
        }
        ResolvedSource::Cadir => {}
        ResolvedSource::Unrecognized => {
            return Err(anyhow!(
                "unrecognized format for {}; supported: FCStd, f3d, Inventor IPT/IAM, sldprt, CATPart, NX/Creo prt, Rhino 3DM, IGES, STEP, ASM sat/smt/smb/sab, .cadir.json; use --input-format to override detection",
                path.display()
            )
            .into());
        }
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
        return Ok(LoadedDocument::neutral(ir));
    };
    Ok(LoadedDocument::restored(
        ir,
        sidecar.report,
        sidecar.fidelity,
    ))
}

#[cfg(test)]
#[allow(clippy::default_trait_access, clippy::unwrap_used)]
mod tests {
    use super::*;
    use crate::application::LoadOrigin;
    use cadmpeg_ir::units::Units;
    use cadmpeg_ir::{DecodeReport, DecodeSidecar, SourceFidelity};

    #[test]
    fn matching_sidecar_restores_decoded_origin_and_mismatch_is_hard_error() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("part.cadir.json");
        let text = CadIr::empty(Units::default()).to_canonical_json().unwrap();
        std::fs::write(&path, &text).unwrap();
        let report: DecodeReport = serde_json::from_value(serde_json::json!({
            "format": "test",
            "container_only": false,
            "geometry_transferred": false,
            "losses": [],
            "notes": [],
            "dialects": null,
        }))
        .unwrap();
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
        assert!(matches!(outcome.origin, LoadOrigin::Decoded { .. }));

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
