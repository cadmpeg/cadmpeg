// SPDX-License-Identifier: Apache-2.0
//! Input detection and loading into CADIR.

use std::fs::File;
use std::io::Read;
use std::path::Path;

use anyhow::{anyhow, Context, Result};
use cadmpeg_ir::codec::{CodecEntry, Confidence, DecodeOptions};
use cadmpeg_ir::{CadIr, DecodeReport, SourceFidelity};

use crate::registry::Registry;
use crate::ForcedInput;

/// Leading byte window available to content-based codec detection.
pub const DETECTION_PREFIX_LEN: usize = 128 * 1024;

/// CADIR loaded from an input path, with native-decoder diagnostics when used.
pub struct LoadedIr {
    /// Loaded model data.
    pub ir: CadIr,
    /// Native decode result, or `None` when the input was CADIR JSON.
    pub decode_report: Option<DecodeReport>,
    /// Decode-time source accounting, absent for neutral CADIR input.
    pub source_fidelity: Option<SourceFidelity>,
}

/// Read at most `n` leading bytes for content-based format detection.
pub fn read_prefix(path: &Path, n: usize) -> Result<Vec<u8>> {
    let mut f = File::open(path).with_context(|| format!("opening {}", path.display()))?;
    let mut buf = Vec::with_capacity(n);
    f.by_ref().take(n as u64).read_to_end(&mut buf)?;
    Ok(buf)
}

/// Load CADIR from a native CAD file or CADIR JSON.
///
/// An explicit input format bypasses detection. Without one, the registered
/// codec with the strongest match decodes the file. An input beginning with a
/// JSON object is parsed as CADIR when no native codec recognizes it.
pub fn load_ir(
    registry: &Registry,
    path: &Path,
    options: DecodeOptions,
    forced: Option<ForcedInput>,
) -> Result<LoadedIr> {
    let prefix = read_prefix(path, DETECTION_PREFIX_LEN)?;
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
            source_fidelity: Some(result.source_fidelity),
        });
    }

    if forced.is_none() && prefix.iter().find(|byte| !byte.is_ascii_whitespace()) != Some(&b'{') {
        return Err(anyhow!(
            "unrecognized format for {}; supported: FCStd, f3d, sldprt, CATPart, NX/Creo prt, Rhino 3DM, IGES, STEP, .cadir.json; use --input-format to override detection",
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
        source_fidelity: None,
    })
}
