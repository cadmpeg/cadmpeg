// SPDX-License-Identifier: Apache-2.0
#![cfg_attr(test, allow(clippy::unwrap_used))]
//! Read and write Autodesk Fusion `.f3d` archives.
//!
//! [`F3dCodec`] implements [`cadmpeg_ir::codec::Codec`] and
//! [`cadmpeg_ir::codec::write::Encoder`]. Decoding produces a
//! [`CadIr`] document with B-rep topology, analytic and cached NURBS geometry,
//! body transforms, design and sketch records, construction history, and
//! appearances. Encoding authors document content from neutral and supported
//! typed native records. Retained bytes may select layout when all source
//! content is represented; unsupported patches fall back to synthesis.
//!
//! <!-- generated: capability f3d -->
//! Support: L5 ([ladder](https://github.com/cadmpeg/cadmpeg/blob/main/docs/format-support.md#fusion-360-f3d)).
//! <!-- /generated: capability f3d -->
//!
//! # Decode
//!
//! ```no_run
//! use cadmpeg_codec_f3d::F3dCodec;
//! use cadmpeg_ir::{CodecBackend, Codec, DecodeOptions};
//! use std::fs::File;
//!
//! # fn main() -> Result<(), Box<dyn std::error::Error>> {
//! let mut input = File::open("part.f3d")?;
//! let result = F3dCodec.decode(&mut input, &DecodeOptions::default())?;
//! for loss in &result.report().losses {
//!     eprintln!("{:?}: {}", loss.severity, loss.message);
//! }
//! # Ok(())
//! # }
//! ```
//!
//! [`Codec::inspect`](cadmpeg_ir::Codec::inspect) classifies the ZIP entries and reads ASM B-rep headers
//! without building geometry. `DecodeDepth::Container` provides the
//! corresponding metadata-only `CadIr`.
//!
//! # Encode
//!
//! ```no_run
//! use cadmpeg_codec_f3d::F3dCodec;
//! use cadmpeg_ir::codec::write::target::TargetRequest;
//! use cadmpeg_ir::codec::write::Encoder;
//! use cadmpeg_ir::{CodecBackend, Codec, DecodeOptions};
//! use std::fs::File;
//!
//! # fn main() -> Result<(), Box<dyn std::error::Error>> {
//! let mut input = File::open("part.f3d")?;
//! let result = F3dCodec.decode(&mut input, &DecodeOptions::default())?;
//! let (ir, _report, fidelity) = result.into_parts();
//! // Edit supported fields in `ir`.
//! let mut output = File::create("part-edited.f3d")?;
//! F3dCodec
//!     .plan(cadmpeg_ir::codec::write::EncodeInput::new(&ir, Some(&fidelity)), TargetRequest::Inherit)?
//!     .write_to(&mut output)?;
//! # Ok(())
//! # }
//! ```
//!
//! Retained `source_fidelity` permits replay or patch when source content is
//! fully represented and the output agrees with synthesis. Omitting `fidelity`
//! authors an archive from the document. Export losses report local reductions.
//!
//! # Data flow
//!
//! The Design body map selects every B-rep blob contributing bodies to the
//! document model; `.smb` and `.smbh` extensions do not choose either model or
//! history role. [`container`] locates history streams by the ASM header flag.
//! When a Design stream is absent or has no body-map carriers, every binary
//! BREP basename in the active asset is a model candidate. [`cadmpeg_asm::sab`] frames each selected
//! active record slice.
//! [`brep`] builds each topology chain from bodies through vertices and points,
//! while [`cadmpeg_asm::nurbs`] decodes cached spline carriers.
//! [`design`], [`history`], and [`materials`] populate source-native records and
//! appearance bindings. [`f3z`] merges multi-document archives into one model.
//!
//! ASM model-space lengths become millimetres. Directions, ratios, angles,
//! knots, weights, and UV parameters retain their native scale.
//!
//! Inspect [`cadmpeg_ir::report::decode::DecodeReport::losses`] before consuming a
//! decode. A stream that cannot produce geometry returns container metadata,
//! retained source data, and blocking geometry and topology losses. Referenced
//! carrier bytes needed for passthrough remain available as
//! [`cadmpeg_ir::unknown::UnknownRecord`] values.

mod act;
mod brep;
mod bytes;
mod container;
mod decode;
mod design;
mod dialect;
mod f3z;
pub use f3z::{
    observe_f3z_members, F3zDecodedMember, F3zMember, F3zMemberKind, F3zMemberObserver, F3zPhase,
    F3zSession,
};
mod history;
mod history_records;
mod ids;
/// Byte-offset constants generated from `docs/layouts/f3d.toml`.
mod layout;
mod loss;
mod manifest;
mod materials;
mod metastream;
mod native;
mod paramesh;
mod records;
mod report;
mod repro;
mod tsm;
mod validate;
mod writer;
mod xref;
mod zip_write;

use cadmpeg_core::bytes::contains;
use cadmpeg_core::decode::{DecodeContext, DecodeDepth, View};
use cadmpeg_core::CodecError;
use cadmpeg_ir::codec::write::{
    target::{Catalog, ResolvedWrite},
    transitional_from_codec, EncodeError, EncodeInput, EncoderBackend, ExportBody,
};
use cadmpeg_ir::codec::{CodecBackend, Confidence, Decoded, FormatId};
use cadmpeg_ir::document::CadIr;
use cadmpeg_ir::ContainerSummary;
use std::io::Write;

#[cfg(test)]
use crate::loss::F3dLossCode;

/// The ZIP local-file-header magic.
const ZIP_MAGIC: &[u8] = b"PK\x03\x04";

/// The Autodesk Fusion `.f3d` container codec.
#[derive(Debug, Default, Clone, Copy)]
pub struct F3dCodec;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum PreservedWritePath {
    Patched,
    VerbatimReplay,
}

impl F3dCodec {
    /// Replay retained source bytes when the image still digests to the source
    /// baseline and decodes to the current document; otherwise patch. Returns
    /// which branch ran.
    fn write_preserved_bytes(
        ir: &CadIr,
        data: &[u8],
        expected: &cadmpeg_ir::hash::digest::Sha256Digest,
        scope: cadmpeg_core::decode::ContentScope,
        limits: cadmpeg_core::decode::ResourceLimits,
        writer: &mut dyn Write,
        notes: &mut Vec<String>,
    ) -> Result<PreservedWritePath, CodecError> {
        let current = decode::document_local_sha256(ir)?;
        let arena = cadmpeg_core::decode::DecodeArena::new();
        let policy = cadmpeg_core::decode::DecodePolicy {
            limits,
            ..Default::default()
        };
        let context = DecodeContext::for_encode(&arena, &policy);
        let session = cadmpeg_core::encode::EncodeSession::from_context(&context)?;
        let depth = match (scope.model, scope.design) {
            (true, false) => DecodeDepth::Geometry,
            (false, false) => DecodeDepth::Container,
            _ => DecodeDepth::Full,
        };
        let (source_ctx, source_root) =
            DecodeContext::for_encode_session_root(&arena, &session, data)?;
        let replays = if cadmpeg_ir::hash::digest::Sha256Digest::digest(data) == *expected {
            let replay_arena = cadmpeg_core::decode::DecodeArena::new();
            let replay_ctx = source_ctx.encode_sibling(&replay_arena, depth)?;
            let fresh = replay_ctx
                .run_part(|| {
                    Ok(
                        cadmpeg_ir::Codec::decode_with_context(&F3dCodec, &replay_ctx, source_root)
                            .ok(),
                    )
                })?
                .ok()
                .flatten();
            session.finish()?;
            fresh
                .and_then(|decoded| decode::document_local_sha256(decoded.ir()).ok())
                .is_some_and(|hash| hash == current)
        } else {
            false
        };
        if !replays {
            let result = writer::patch::write_with_context(
                ir,
                data,
                writer,
                notes,
                &source_ctx,
                source_root,
            );
            session.finish()?;
            result?;
            return Ok(PreservedWritePath::Patched);
        }
        writer.write_all(data)?;
        Ok(PreservedWritePath::VerbatimReplay)
    }
}

impl CodecBackend for F3dCodec {
    fn repro_redactions(
        ir: &CadIr,
    ) -> Result<std::collections::BTreeMap<String, String>, CodecError> {
        repro::redactions(ir)
    }

    const FORMAT: FormatId = FormatId::new(dialect::FORMAT);

    fn validate_native(
        ctx: &DecodeContext<'_>,
        ir: &CadIr,
        _source: Option<&cadmpeg_ir::SourceFidelity>,
    ) -> Result<Vec<cadmpeg_ir::report::check::Finding>, CodecError> {
        validate::validate_native(ctx, ir)
    }

    fn detect_impl(
        &self,
        ctx: &cadmpeg_core::decode::DecodeContext<'_>,
        prefix: cadmpeg_core::decode::View<'_>,
    ) -> Result<Confidence, cadmpeg_core::CodecError> {
        let prefix = prefix.window();
        if !prefix.starts_with(ZIP_MAGIC) {
            if cadmpeg_container::ArchiveSnapshot::has_footer(prefix) {
                match cadmpeg_container::ArchiveSnapshot::contains_matching_name(
                    ctx,
                    cadmpeg_core::decode::View::over_retained(prefix),
                    |name| {
                        container::DETECT_MARKERS
                            .iter()
                            .chain(container::F3Z_DETECT_MARKERS)
                            .any(|marker| contains(name.as_bytes(), marker))
                    },
                ) {
                    Ok(true) => return Ok(Confidence::Medium),
                    Err(error @ CodecError::ResourceLimit(_)) => return Err(error),
                    Ok(false) | Err(_) => {}
                }
            }
            return Ok(Confidence::No);
        }
        for marker in container::DETECT_MARKERS
            .iter()
            .chain(container::F3Z_DETECT_MARKERS)
        {
            if contains(prefix, marker) {
                return Ok(Confidence::High);
            }
        }
        Ok(Confidence::Low)
    }

    fn inspect_impl(
        &self,
        ctx: &DecodeContext<'_>,
        root: View<'_>,
    ) -> Result<ContainerSummary, CodecError> {
        let scan = container::scan(ctx, root)?;
        match &scan.kind {
            container::F3dContainerKind::MultiDocument { .. } => f3z::inspect(ctx, &scan),
            container::F3dContainerKind::Document { .. } => report::build_inspection_summary(&scan),
        }
    }

    fn decode_impl(&self, ctx: &DecodeContext<'_>, root: View<'_>) -> Result<Decoded, CodecError> {
        decode::decode(ctx, root)
    }
}

impl EncoderBackend for F3dCodec {
    const REUSES_SOURCE: bool = true;
    const REDUCED_FIELDS: &'static [cadmpeg_ir::codec::write::ReducedField] = writer::target::REDUCED_FIELDS;
    const FORMAT: FormatId = <Self as CodecBackend>::FORMAT;
    /// Patches the retained source, so the history and design records it rewrites
    /// must be present.
    const REQUEST: DecodeDepth = DecodeDepth::Full;
    type Target = Catalog;
    const TARGET: Catalog = Catalog::new(dialect::TARGETS, Some(0));

    fn plan_resolved(
        &self,
        input: EncodeInput<'_>,
        target: ResolvedWrite<'_>,
    ) -> Result<ExportBody, EncodeError> {
        writer::target::plan(input, &target).map_err(transitional_from_codec)
    }
}

#[cfg(test)]
mod golden_tests;
#[cfg(test)]
mod integration_tests;
#[cfg(test)]
mod test_support;
