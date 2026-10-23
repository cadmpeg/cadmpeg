// SPDX-License-Identifier: Apache-2.0
//! One record scope around each parameter parse.

use std::collections::{BTreeMap, HashSet};

use cadmpeg_core::decode::{u64_from_index, DecodeContext, View};
use cadmpeg_core::CodecError;

use super::ParameterFrame;
use crate::records::parameters::DesignParameter;

/// Result of one parameter frame parse inside its record scope.
pub(crate) enum Captured {
    /// The frame is a parameter record. Its record scope holds its reads.
    Parameter {
        parameter: DesignParameter,
        /// Local entity index named by the typed parameter tail reference.
        state_target: Option<u64>,
    },
    /// The frame repeats the record index of an earlier parameter frame.
    Repeated,
    /// The frame matches no parameter grammar.
    Unmatched,
}

/// What the stream already states about one candidate frame.
#[derive(Clone, Copy)]
pub(crate) struct Expected<'a> {
    /// Entity ID of the primary-index record that starts at the frame, when
    /// the stream has a primary index.
    pub(crate) entity_id: Option<u64>,
    /// Record indices of the parameters already decoded from this stream.
    pub(crate) emitted: &'a HashSet<u32>,
}

/// Parse one candidate frame once, inside the record scope of the parameter
/// it would be. A frame that is not a new parameter discards its scope.
pub(crate) fn capture_record(
    ctx: &DecodeContext<'_>,
    view: View<'_>,
    stream: &str,
    offset: usize,
    expected: Expected<'_>,
    pointers: &mut BTreeMap<String, BTreeMap<String, u64>>,
) -> Result<Captured, CodecError> {
    let id = crate::ids::native_design_parameter_id(stream, u64_from_index(offset));
    if pointers.contains_key(&id) {
        return Err(CodecError::malformed(
            "parameter source owner is duplicated",
        ));
    }
    let mut miss = None;
    let result = ctx.record(&id, view, || {
        let Some(parsed) = super::parse_design_parameter(ParameterFrame::new(view))? else {
            miss = Some(Captured::Unmatched);
            return Err(CodecError::malformed("frame matches no parameter grammar"));
        };
        if expected
            .entity_id
            .is_some_and(|entity_id| entity_id != u64::from(parsed.record_index))
        {
            miss = Some(Captured::Unmatched);
            return Err(CodecError::malformed(
                "frame header differs from its primary registration",
            ));
        }
        if expected.emitted.contains(&parsed.record_index) {
            miss = Some(Captured::Repeated);
            return Err(CodecError::malformed(
                "frame repeats a parameter record index",
            ));
        }
        let state_target = parsed.state_target();
        let parameter = super::locate_design_parameter(parsed, stream, offset)?;
        if parameter.id != id {
            return Err(CodecError::malformed(
                "parameter layout has a different actual source owner",
            ));
        }
        let fields = pointer_fields(ctx, parameter.source_offsets())?;
        Ok((parameter, state_target, fields))
    });
    match result {
        Ok((parameter, state_target, fields)) => {
            pointers.insert(id, fields);
            Ok(Captured::Parameter {
                parameter,
                state_target,
            })
        }
        Err(error) if matches!(error, CodecError::ResourceLimit(_)) => Err(error),
        Err(error) => miss.map_or(Err(error), Ok),
    }
}

/// The present source field offsets of one record, as `record_layout`
/// pointers. Each kept pointer is charged once.
pub(crate) fn pointer_fields(
    ctx: &DecodeContext<'_>,
    fields: impl IntoIterator<Item = (&'static str, Option<u64>)>,
) -> Result<BTreeMap<String, u64>, CodecError> {
    let mut pointers = BTreeMap::new();
    for (name, offset) in fields
        .into_iter()
        .filter_map(|(name, offset)| offset.map(|offset| (name, offset)))
    {
        let bytes = name
            .len()
            .checked_add(size_of::<(String, u64)>())
            .ok_or_else(|| CodecError::malformed("record layout pointer storage overflows"))?;
        ctx.retain_bytes(u64_from_index(bytes), "record layout pointer")?;
        pointers.insert(name.to_owned(), offset);
    }
    Ok(pointers)
}

#[cfg(test)]
mod tests;
