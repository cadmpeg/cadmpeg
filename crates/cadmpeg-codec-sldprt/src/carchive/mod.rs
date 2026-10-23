//! An exact reader of the MFC `CArchive` object graph in a configuration's
//! `Contents/Config-N-ResolvedFeatures` stream (`docs/formats/sldprt.md` § 2.3).
//!
//! The stream continues the class map of `Contents/Config-N`: its leading u32
//! is the map count at which that stream ended, and its class tags name that
//! stream's indices ([`crate::archive::class_index_map`]). From there every
//! object is read by its class's rule, so the map counter stays exact and a
//! class tag inside the stream names one class.

// Not yet wired into the decoder: the rules are being ported and checked
// against the research grammar position by position.
#![allow(dead_code)]
// A line-for-line port keeps the Python's names, casts and branch shapes so
// the two can be read side by side.
#![allow(
    clippy::many_single_char_names,
    clippy::as_conversions,
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    clippy::cast_possible_wrap,
    clippy::cast_precision_loss,
    clippy::if_same_then_else,
    clippy::naive_bytecount,
    clippy::needless_late_init,
    clippy::too_many_lines,
    clippy::float_cmp
)]

use std::collections::{BTreeMap, HashMap};
use std::sync::Arc;

#[cfg(test)]
mod corpus_tests;

pub(crate) mod reader;
pub(crate) mod rules;

/// How a walk ended.
#[derive(Debug, Clone, PartialEq)]
pub(crate) enum Status {
    /// Every top-level object was read to the end of the stream.
    End,
    /// A rule stopped the walk.
    Stop(reader::Stop),
}

/// A walk of one `ResolvedFeatures` stream.
#[derive(Debug, Clone)]
pub(crate) struct Walk {
    pub(crate) status: Status,
    /// Where the walk ended or stopped.
    pub(crate) p: usize,
    /// Every object opened, in order.
    pub(crate) trace: Vec<reader::Trace>,
    /// Parameter objects read.
    pub(crate) parameters: Vec<reader::ParameterRecord>,
    /// Named objects read.
    pub(crate) named: Vec<reader::NamedRecord>,
    /// Top-level pointers read.
    pub(crate) tops: usize,
}

/// Walk `resolved` (a `Config-N-ResolvedFeatures` stream) with the class map
/// `known` its `Config-N` stream ended with (`ftop.run`): the map count u32,
/// a u16, then top-level objects to the end.
pub(crate) fn walk(
    known: &BTreeMap<u32, String>,
    resolved: &[u8],
    ver: Option<u32>,
) -> Option<Walk> {
    let counter = u32::from_le_bytes(resolved.get(0..4)?.try_into().ok()?);
    let known = known
        .iter()
        .map(|(&index, name)| (index, Arc::<str>::from(name.as_str())))
        .collect::<HashMap<_, _>>();
    let mut r = reader::Reader::new(resolved, 6, known, counter, ver);
    let mut tops = 0;
    let status = loop {
        if r.p + 1 >= resolved.len() {
            break Status::End;
        }
        match r.ptr(None) {
            Ok(_) => tops += 1,
            Err(stop) => break Status::Stop(stop),
        }
    };
    Some(Walk {
        status,
        p: r.p,
        trace: r.trace,
        parameters: r.parameters,
        named: r.named,
        tops,
    })
}

/// Walk a configuration's `ResolvedFeatures` stream from its `Config-N`
/// stream's class map: `None` when that map does not reconcile with the
/// stream's count, or the stream carries no count.
pub(crate) fn walk_configuration(
    config: &[u8],
    resolved: &[u8],
    ver: Option<u32>,
) -> Result<Option<Walk>, cadmpeg_core::CodecError> {
    let Some(count) = crate::archive::resolved_features_count(resolved) else {
        return Ok(None);
    };
    let Some(map) = crate::archive::class_index_map(config, count)? else {
        return Ok(None);
    };
    Ok(walk(&map, resolved, ver))
}
