// SPDX-License-Identifier: Apache-2.0
//! Loading IR from either a `.cadir.json` document or a source CAD file.

use std::fs::File;
use std::io::Read;
use std::path::Path;

use anyhow::{anyhow, Context, Result};
use cadmpeg_ir::codec::{Confidence, DecodeOptions};
use cadmpeg_ir::{CadIr, DecodeReport};

use crate::registry::Registry;
use crate::ForcedInput;

/// An IR obtained from an input path, plus the decode report when the input was
/// a source file (rather than an already-decoded `.cadir.json`).
pub struct LoadedIr {
    /// The IR document.
    pub ir: CadIr,
    /// Present when the IR came from decoding a source file.
    pub decode_report: Option<DecodeReport>,
}

/// Read up to `n` leading bytes of a file for format detection.
pub fn read_prefix(path: &Path, n: usize) -> Result<Vec<u8>> {
    let mut f = File::open(path).with_context(|| format!("opening {}", path.display()))?;
    let mut buf = Vec::with_capacity(n);
    f.by_ref().take(n as u64).read_to_end(&mut buf)?;
    Ok(buf)
}

/// Load an IR document from `path`. If a registered codec recognizes the bytes,
/// the file is decoded and the decode report is returned alongside the IR.
/// Otherwise the file is parsed as a canonical `.cadir.json` IR.
pub fn load_ir(
    registry: &Registry,
    path: &Path,
    options: DecodeOptions,
    forced: Option<ForcedInput>,
) -> Result<LoadedIr> {
    let prefix = read_prefix(path, 512)?;
    let detected = match forced {
        Some(ForcedInput::Codec(id)) => Some((
            registry
                .by_id(id)
                .ok_or_else(|| anyhow!("unsupported input format {id}"))?,
            None,
        )),
        Some(ForcedInput::Cadir) => None,
        None => registry
            .detect(&prefix)
            .map(|(codec, confidence)| (codec, Some(confidence))),
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
        return Ok(LoadedIr {
            ir: result.ir,
            decode_report: Some(result.report),
        });
    }

    if forced.is_none() && prefix.iter().find(|byte| !byte.is_ascii_whitespace()) != Some(&b'{') {
        return Err(anyhow!(
            "unrecognized format for {}; supported: f3d, sldprt, CATPart, NX/Creo prt, .cadir.json; use --input-format to override detection",
            path.display()
        ));
    }

    let text = std::fs::read_to_string(path)
        .with_context(|| format!("reading {} as a .cadir.json document", path.display()))?;
    let ir = CadIr::from_json(&text).map_err(|e| {
        anyhow!(
            "{} is neither a recognized CAD file nor a valid .cadir.json document: {e}",
            path.display()
        )
    })?;
    Ok(LoadedIr {
        ir,
        decode_report: None,
    })
}
