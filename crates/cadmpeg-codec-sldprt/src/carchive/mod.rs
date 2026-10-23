//! An exact reader of the MFC `CArchive` object graph in a configuration's
//! `Contents/Config-N-ResolvedFeatures` stream (`docs/formats/sldprt.md` § 2.3).
//!
//! The stream continues the class map of `Contents/Config-N`: its leading u32
//! is the map count at which that stream ended, and its class tags name that
//! stream's indices ([`crate::archive::class_index_map`]). From there every
//! object is read by its class's rule, so the map counter stays exact and a
//! class tag inside the stream names one class.

use std::collections::{BTreeMap, HashMap};
use std::sync::Arc;

use cadmpeg_core::decode::{DecodeContext, View};

#[cfg(test)]
mod tests;

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
    /// Every object opened, in order.
    #[cfg(test)]
    pub(crate) trace: Vec<reader::Trace>,
    /// Parameter objects read.
    pub(crate) parameters: Vec<reader::ParameterRecord>,
    /// Top-level pointers read.
    #[cfg(test)]
    pub(crate) tops: usize,
}

/// Walk `resolved` (a `Config-N-ResolvedFeatures` stream) with the class map
/// `known` its `Config-N` stream ended with (`ftop.run`): the map count u32,
/// a u16, then top-level objects to the end.
pub(crate) fn walk<'b>(
    ctx: &'b DecodeContext<'b>,
    known: &BTreeMap<u32, String>,
    resolved: &'b [u8],
    ver: Option<u32>,
) -> Option<Walk> {
    let counter = View::u32_le_at(resolved, 0)?;
    let known = known
        .iter()
        .map(|(&index, name)| (index, Arc::<str>::from(name.as_str())))
        .collect::<HashMap<_, _>>();
    let mut r = reader::Reader::new(ctx, resolved, 6, known, counter, ver);
    #[cfg(test)]
    let mut tops = 0;
    let status = loop {
        if r.p + 1 >= resolved.len() {
            break Status::End;
        }
        match r.ptr(None) {
            Ok(_) => {
                #[cfg(test)]
                {
                    tops += 1;
                }
            }
            Err(stop) => break Status::Stop(stop),
        }
    };
    Some(Walk {
        status,
        parameters: r.parameters,
        #[cfg(test)]
        trace: r.trace,
        #[cfg(test)]
        tops,
    })
}

/// Walk a configuration's `ResolvedFeatures` stream from its `Config-N`
/// stream's class map: `None` when that map does not reconcile with the
/// stream's count, or the stream carries no count. A decode-context refusal
/// is an error, not a stop.
pub(crate) fn walk_configuration(
    ctx: &DecodeContext<'_>,
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
    let walk = walk(ctx, &map, resolved, ver);
    if let Some(Status::Stop(reader::Stop::Refused(limit))) = walk.as_ref().map(|walk| &walk.status)
    {
        return Err((*limit).into());
    }
    Ok(walk)
}

/// The version the walk reads a configuration with: the declared `swVersion`
/// when it is a number, else the u32 at offset 22 of the `Config-N` stream
/// when it lies in 9000..=40000, else 17000.
pub(crate) fn walk_version(declared: Option<&str>, config: &[u8]) -> u32 {
    if let Some(version) = declared.and_then(|value| value.trim().parse::<u32>().ok()) {
        return version;
    }
    View::u32_le_at(config, 22)
        .filter(|value| (9000..=40000).contains(value))
        .unwrap_or(17000)
}
