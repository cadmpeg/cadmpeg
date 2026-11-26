// SPDX-License-Identifier: Apache-2.0
//! Input detection and loading into CADIR.

use std::fs::File;
use std::path::Path;

use anyhow::{anyhow, Context};
use cadmpeg_core::decode::{ContentScope, DecodeArena, DecodeContext, View};
use cadmpeg_ir::codec::{Confidence, DecodeOptions};
use cadmpeg_ir::{CadIr, CadirRead, DecodeSidecar, ReadFailure, ReadOptions};

use cadmpeg_registry::{
    ForcedInput, InputCatalog, ResolveSourceError, ResolvedSource, Selection, DETECTION_PREFIX_LEN,
};

use crate::application::artifact_store::{self, SidecarLoad};
use crate::application::document::{LoadOrigin, LoadedDocument};
use crate::application::refusal::{ApplicationError, ConversionRefusal};

pub(crate) mod companions;

/// Restates a detection failure with the flag that overrides it.
///
/// The registry states the fact; naming `--input-format` is this crate's job,
/// because the flag is this crate's.
pub(crate) fn detection_failure(error: &ResolveSourceError) -> anyhow::Error {
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
///
/// Every command loads through here, so here is where the load's notices go
/// to stderr: a decode sidecar the load ignored, each entry of a CADIR read's
/// report, a document whose scope is below full, and a detection below high
/// confidence.
pub(crate) fn load_artifact(
    catalog: &InputCatalog,
    path: &Path,
    options: &DecodeOptions,
    forced: Option<ForcedInput>,
) -> Result<LoadedDocument, ApplicationError> {
    let document = load(catalog, path, options, forced)?;
    print_load_notice(path, &document);
    Ok(document)
}

fn print_load_notice(path: &Path, document: &LoadedDocument) {
    if let Some(loss) = &document.stale_sidecar {
        eprintln!("warning: {}: {}", loss.code, loss.message);
    }
    for entry in &document.read_report {
        eprintln!("warning: {}: {}", entry.code, entry.message);
    }
    if let Some(note) = scope_note(document.ir.scope()) {
        eprintln!("note: {}: {note}", path.display());
    }
    let LoadOrigin::Decoded {
        selection: Selection::Detected { confidence },
        report,
        ..
    } = &document.origin
    else {
        return;
    };
    if *confidence < Confidence::High {
        let format_id = report.format();
        eprintln!(
            "warning: detected {format_id} with {confidence} confidence; use --input-format to override"
        );
    }
}

/// The note for a document whose scope is below full: the arenas of an
/// excluded class state nothing about the source.
fn scope_note(scope: ContentScope) -> Option<String> {
    let excluded = scope.missing_from(ContentScope::full());
    (!excluded.is_empty()).then(|| {
        format!(
            "the document's scope excludes {excluded}; the arenas of an excluded class state \
             nothing about the source"
        )
    })
}

fn load(
    catalog: &InputCatalog,
    path: &Path,
    options: &DecodeOptions,
    forced: Option<ForcedInput>,
) -> Result<LoadedDocument, ApplicationError> {
    let companions = companions::Directory::beside(path);
    let arena = DecodeArena::new();
    let ctx =
        DecodeContext::new(&arena, &options.policy, options.depth).with_companions(&companions);
    let mut file = File::open(path).with_context(|| format!("opening {}", path.display()))?;
    let mut prefix =
        cadmpeg_container::compound::read_detection_prefix(&ctx, &mut file, DETECTION_PREFIX_LEN)?;
    let resolved = catalog
        .resolve_seekable_source(&ctx, View::over_retained(&prefix), &mut file, forced)
        .map_err(|error| match error {
            ResolveSourceError::Codec(error) => ApplicationError::from(error),
            error => ApplicationError::from(detection_failure(&error)),
        })?;
    match resolved {
        ResolvedSource::Native { codec, selection } => {
            let format_id = codec.id();
            ctx.complete_input(&mut file, &mut prefix)?;
            let result =
                codec.decode_with_context(&ctx, View::over_retained(&prefix), &options.strictness);
            ctx.finish_session()?;
            let result = result.map_err(|failure| {
                ApplicationError::from_decode_failure(path, format_id, failure)
            })?;
            return Ok(LoadedDocument::decoded(
                result,
                selection,
                cadmpeg_core::decode::u64_from_index(prefix.len()),
            ));
        }
        ResolvedSource::Cadir => {}
        ResolvedSource::Unrecognized => {
            return Err(anyhow!(
                "unrecognized format for {}; supported: FCStd, f3d, Inventor IPT/IAM, sldprt, CATPart, NX/Creo prt, Rhino 3DM, IGES, STEP, ASM sat/smt/smb/sab, Creo View pvz, eDrawings easm/eprt, OpenUSD usda/usdc/usd/usdz, .cadir.json; use --input-format to override detection",
                path.display()
            )
            .into());
        }
    }

    let max_bytes = options.policy.limits.max_input_bytes;
    ctx.complete_input(&mut file, &mut prefix)?;
    ctx.finish_session()?;
    // The decode's strictness judges the report of this CADIR read. A restored
    // sidecar's report describes an earlier decode and never decides (CADIR v11
    // §3.5).
    let read_options = ReadOptions {
        limits: options.policy.limits,
        strictness: options.strictness.clone(),
    };
    let read = CadIr::read(&prefix, &read_options).map_err(|error| match error {
        ReadFailure::ResourceLimit(limit) => {
            ApplicationError::from(cadmpeg_core::CodecError::ResourceLimit(limit))
        }
        ReadFailure::StrictRejected { report, entry } => {
            ConversionRefusal::StrictReadRejected { report, entry }.into()
        }
        _ if names_decode_sidecar(path) => anyhow!(
            "{} is a decode sidecar; pass the CADIR document beside it",
            path.display()
        )
        .into(),
        error => anyhow!(
            "{} is neither a recognized CAD file nor a valid .cadir.json document: {error}",
            path.display()
        )
        .into(),
    })?;
    let input_bytes = cadmpeg_core::decode::u64_from_index(prefix.len());
    Ok(
        match artifact_store::load_matching_sidecar(path, &prefix, max_bytes)? {
            SidecarLoad::Absent => LoadedDocument::read(read, input_bytes),
            SidecarLoad::Stale(loss) => {
                LoadedDocument::read(read, input_bytes).with_stale_sidecar(loss)
            }
            SidecarLoad::Matching(sidecar) => {
                let DecodeSidecar {
                    report,
                    mut fidelity,
                    ..
                } = *sidecar;
                let CadirRead {
                    document,
                    report: read_report,
                    fidelity: read,
                } = read;
                let (_, retained) = read.into_parts();
                for (id, record) in retained {
                    if fidelity.retained_record(id.as_str()).is_none() {
                        fidelity
                            .insert_retained_record(id, record)
                            .map_err(|error| anyhow!("{}: {error}", path.display()))?;
                    }
                }
                LoadedDocument::restored(document, report, fidelity, input_bytes)
                    .with_read_report(read_report)
            }
        },
    )
}

/// Whether the file name has the decode-sidecar suffix that `dump` writes
/// beside a CADIR document.
fn names_decode_sidecar(path: &Path) -> bool {
    path.file_name()
        .and_then(std::ffi::OsStr::to_str)
        .is_some_and(|name| name.ends_with(".fidelity.json"))
}

#[cfg(test)]
#[allow(clippy::default_trait_access, clippy::unwrap_used)]
mod tests {
    use super::load_artifact;
    use crate::application::document::LoadOrigin;
    use cadmpeg_ir::codec::DecodeOptions;
    use cadmpeg_ir::CadIr;
    use cadmpeg_ir::{report::decode::DecodeReport, DecodeSidecar, SourceFidelity};
    use cadmpeg_registry::{ForcedInput, InputCatalog};

    #[test]
    fn every_sub_full_scope_is_noted() {
        use cadmpeg_core::decode::ContentScope;

        assert_eq!(
            super::scope_note(ContentScope::geometry()).as_deref(),
            Some(
                "the document's scope excludes design, process; the arenas of an excluded class \
                 state nothing about the source"
            )
        );
        assert_eq!(super::scope_note(ContentScope::full()), None);
    }

    #[test]
    fn input_acquisition_resource_refusal_stays_typed() {
        let directory = tempfile::tempdir().expect("directory");
        let path = directory.path().join("input.json");
        std::fs::write(&path, b"12345").expect("input");
        let mut options = DecodeOptions::default();
        options.policy.limits.max_input_bytes = 4;
        let error = load_artifact(
            &InputCatalog::with_builtins(),
            &path,
            &options,
            Some(ForcedInput::Cadir),
        )
        .expect_err("input limit");
        assert!(
            matches!(error.refusal(), Some(crate::application::refusal::ConversionRefusal::ResourceLimit { limit, .. }) if limit.dimension == cadmpeg_core::decode::ResourceDimension::InputBytes && limit.limit == 4)
        );
    }

    #[test]
    fn matching_sidecar_restores_decoded_origin_and_a_stale_one_is_ignored() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("part.cadir.json");
        let text = CadIr::empty().to_canonical_json().unwrap();
        std::fs::write(&path, &text).unwrap();
        let report: DecodeReport = cadmpeg_core::serde_erase::json::from_value(serde_json::json!({
            "identity": {"kind": "unclassified", "format": "test"},
            "transfer": {"kind": "full", "geometry_transferred": false},
            "coverage": {},
            "losses": [],
            "notes": [],
            "transfer_ledger": {"entries": []},
        }))
        .unwrap();
        let sidecar = DecodeSidecar::bind_sha256(
            cadmpeg_ir::hash::digest::Sha256Digest::digest(text.as_bytes()),
            report,
            SourceFidelity::new(cadmpeg_ir::hash::digest::Sha256Digest::digest(b"source")),
        );
        std::fs::write(
            cadmpeg_ir::decode_sidecar_path(&path).unwrap(),
            sidecar.to_canonical_json().unwrap(),
        )
        .unwrap();

        let outcome = load_artifact(
            &InputCatalog::with_builtins(),
            &path,
            &DecodeOptions::default(),
            Some(ForcedInput::Cadir),
        )
        .unwrap();
        assert!(matches!(outcome.origin, LoadOrigin::Restored { .. }));
        assert_eq!(outcome.input_bytes, u64::try_from(text.len()).unwrap());

        std::fs::write(&path, format!("{text}\n")).unwrap();
        let outcome = load_artifact(
            &InputCatalog::with_builtins(),
            &path,
            &DecodeOptions::default(),
            Some(ForcedInput::Cadir),
        )
        .expect("a stale sidecar does not refuse the load");
        assert_eq!(outcome.origin, LoadOrigin::Neutral);
        assert_eq!(outcome.ir, CadIr::empty());
        let loss = outcome
            .stale_sidecar
            .expect("the load reports the stale sidecar");
        assert_eq!(loss.code, cadmpeg_ir::source_fidelity::sidecar_stale_kind());
        assert_eq!(loss.severity, cadmpeg_ir::report::Severity::Warning);
    }
}
