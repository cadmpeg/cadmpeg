// SPDX-License-Identifier: Apache-2.0
//! Cross-codec one-home checks for actual given-up source parts.

use std::ops::Range;

use cadmpeg_ir::report::{loss::LossNote, Severity};
use cadmpeg_ir::source_fidelity::parts::PartState;
use cadmpeg_ir::{CodecFormat, Native, NativeUnknownRecord, SourceFidelity};

/// Check §4.5's one-home invariant against independently supplied source bytes.
///
/// The source resolver must expose the actual named stream and exact interval;
/// an unavailable stream is an error. This does not infer a byte's B1 class or
/// replace the removed-failure-name census.
///
/// # Errors
/// Returns a missing/duplicate stub, incorrect identity/source image, an extra
/// contained layout, or absent/competing unreadable-loss attribution.
pub fn check_one_home<'a>(
    codec: CodecFormat,
    native: &Native,
    fidelity: &SourceFidelity,
    losses: &[LossNote],
    root_stream: &str,
    mut source: impl FnMut(&str, Range<u64>) -> Option<&'a [u8]>,
) -> Result<(), String> {
    let unknowns = native
        .namespace(codec.as_str())
        .map(|namespace| namespace.arena_as::<NativeUnknownRecord>("unknowns"))
        .transpose()
        .map_err(|error| error.to_string())?
        .unwrap_or_default();
    for (id, part) in fidelity.parts() {
        let matching = unknowns
            .iter()
            .filter(|stub| stub.id.as_str() == id.as_str())
            .count();
        if !matches!(part.state(), PartState::GivenUp {}) {
            if matching != 0 || fidelity.retained_record(id.as_str()).is_some() {
                return Err(format!("non-given-up part {id} has a failure home"));
            }
            continue;
        }
        let expected = address(codec, part.level().as_str(), part.stream(), part.offset());
        if id.as_str() != expected {
            return Err(format!("part {id} has the wrong deterministic identity"));
        }
        if matching != 1 {
            return Err(format!("given-up part {id} has {matching} unknown stubs"));
        }
        let owners = native
            .0
            .values()
            .flat_map(|namespace| namespace.arenas().values())
            .flatten()
            .filter(|record| record.id() == id.as_str())
            .count();
        if owners != 1 {
            return Err(format!(
                "given-up part {id} has {owners} native records across namespaces"
            ));
        }
        let end = part
            .offset()
            .checked_add(part.byte_len())
            .ok_or_else(|| format!("part {id} extent overflows"))?;
        let expected = source(part.stream(), part.offset()..end)
            .ok_or_else(|| format!("part {id} source bytes are unavailable"))?;
        if u64::try_from(expected.len()).ok() != Some(part.byte_len()) {
            return Err(format!(
                "part {id} source resolver returned the wrong length"
            ));
        }
        let retained = fidelity
            .retained_record(id.as_str())
            .ok_or_else(|| format!("part {id} has no retained source image"))?;
        if retained.stream() != part.stream()
            || retained.offset() != part.offset()
            || retained.byte_len() != part.byte_len()
            || retained.data() != Some(expected)
        {
            return Err(format!(
                "part {id} retained image differs from its actual source"
            ));
        }
        let own_layout = fidelity
            .record_layout()
            .get(id.as_str())
            .ok_or_else(|| format!("part {id} stub has no layout"))?;
        if own_layout.stream() != part.stream()
            || own_layout.offset() != part.offset()
            || own_layout.byte_len() != part.byte_len()
            || !own_layout.reads().is_empty()
        {
            return Err(format!(
                "part {id} stub layout is not exact framing-only evidence"
            ));
        }
        for (record, layout) in fidelity.record_layout() {
            if record == id.as_str() || layout.stream() != part.stream() {
                continue;
            }
            let layout_end = layout
                .offset()
                .checked_add(layout.byte_len())
                .ok_or_else(|| format!("record {record} extent overflows"))?;
            if contained(part.offset(), end, layout.offset(), layout_end) {
                return Err(format!(
                    "record {record} has a layout inside given-up part {id}"
                ));
            }
        }
        let mut own_losses = 0;
        for loss in losses {
            let Some(provenance) = &loss.provenance else {
                continue;
            };
            if provenance.format() != codec.as_str()
                || provenance.stream_name().unwrap_or(root_stream) != part.stream()
            {
                continue;
            }
            let length = provenance.byte_len().unwrap_or(0);
            let loss_end = provenance
                .offset
                .checked_add(length)
                .ok_or_else(|| "loss source extent overflows".to_owned())?;
            let same = provenance.offset == part.offset() && length == part.byte_len();
            let unreadable = loss.code.to_string() == "decode/record.unreadable";
            if same && unreadable {
                if loss.severity != Severity::Error {
                    return Err(format!("part {id} unreadable loss is not Error"));
                }
                own_losses += 1;
            } else if contained(part.offset(), end, provenance.offset, loss_end) {
                return Err(format!(
                    "loss {} also names source inside given-up part {id}",
                    loss.code
                ));
            }
        }
        if own_losses != 1 {
            return Err(format!(
                "part {id} has {own_losses} exact unreadable losses"
            ));
        }
    }
    let part_prefix = format!("{}:source:part#", codec.as_str());
    for (namespace_name, namespace) in &native.0 {
        for (arena_name, records) in namespace.arenas() {
            for record in records {
                if !record.id().starts_with(&part_prefix) {
                    continue;
                }
                if !fidelity
                    .parts()
                    .get(record.id())
                    .is_some_and(|part| matches!(part.state(), PartState::GivenUp {}))
                {
                    return Err(format!(
                        "unknown stub {} has no given-up source part",
                        record.id()
                    ));
                }
                if namespace_name != codec.as_str() || arena_name != "unknowns" {
                    return Err(format!(
                        "part {} has a native home outside its codec unknowns arena",
                        record.id()
                    ));
                }
            }
        }
    }
    Ok(())
}

fn contained(begin: u64, end: u64, start: u64, stop: u64) -> bool {
    start >= begin && stop <= end && (stop > start || start < end || begin == end)
}

fn address(codec: CodecFormat, level: &str, stream: &str, offset: u64) -> String {
    let mut bytes = Vec::new();
    bytes.extend_from_slice(&cadmpeg_core::decode::u64_from_index(level.len()).to_be_bytes());
    bytes.extend_from_slice(level.as_bytes());
    bytes.extend_from_slice(&cadmpeg_core::decode::u64_from_index(stream.len()).to_be_bytes());
    bytes.extend_from_slice(stream.as_bytes());
    bytes.extend_from_slice(&offset.to_be_bytes());
    format!(
        "{}:source:part#{}",
        codec.as_str(),
        cadmpeg_ir::hash::sha256_hex(&bytes)
    )
}

#[cfg(test)]
mod tests;
