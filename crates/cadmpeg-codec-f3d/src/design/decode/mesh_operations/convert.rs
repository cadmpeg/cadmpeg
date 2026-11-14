use super::{Cursor, MeshOperationFraming, MeshOperationRecords};
use crate::layout::mesh_convert_input_prefix as input;
use crate::layout::mesh_convert_output_prefix as output;
use crate::layout::mesh_operation_scope_prefix as prefix;
use cadmpeg_core::decode::{bounded_len, DecodeContext};
use cadmpeg_core::CodecError;

#[derive(Debug, PartialEq, Eq)]
pub(crate) struct MeshConversionScope {
    pub(crate) prefix_word: Option<u32>,
    pub(crate) control: u32,
    pub(crate) input_count: Option<u32>,
    pub(crate) input_wrappers: Vec<u32>,
    pub(crate) output_wrappers: Vec<u32>,
    pub(crate) tail_control: Option<u32>,
    pub(crate) tail_error: Option<MeshOperationFraming>,
    pub(crate) deviations: Vec<(&'static str, u32)>,
}

#[derive(Debug, PartialEq, Eq)]
pub(crate) struct MeshConversionOutput {
    pub(crate) guid: String,
    pub(crate) control: u8,
    pub(crate) body: u32,
}

impl MeshOperationRecords<'_> {
    pub(crate) fn conversion_scope(
        &self,
        ctx: &DecodeContext<'_>,
        index: u32,
    ) -> Result<Result<MeshConversionScope, MeshOperationFraming>, CodecError> {
        let record = match self.record(
            index,
            "conversion scope record",
            "62B85DC3-27B5-47BA-9F13-B2527B72EE49",
            "2EDC2536-D95B-4469-8ACA-DC07124D9C43",
            "Fusion",
            Some(4),
        ) {
            Ok(record) => record,
            Err(error) => return Ok(Err(error)),
        };
        read_scope(ctx, record)
    }

    pub(crate) fn conversion_input_group(&self, index: u32) -> Result<u32, MeshOperationFraming> {
        let record = self.record(
            index,
            "conversion input wrapper",
            "D6FB6F05-98CD-4BFF-BF90-594F77725DC1",
            "D0D7E795-ECF7-48A8-A7A3-F983C5A29F8B",
            "Fusion",
            Some(1),
        )?;
        if record
            .get(input::ZERO_RUN_11..input::GROUP_REFERENCE)
            .is_none_or(|run| run.iter().any(|byte| *byte != 0))
        {
            return Err(MeshOperationFraming("conversion input wrapper prefix"));
        }
        Cursor {
            bytes: record,
            at: input::GROUP_REFERENCE,
            deviations: Vec::new(),
        }
        .reference("conversion input group reference")
    }

    pub(crate) fn conversion_output(
        &self,
        index: u32,
    ) -> Result<MeshConversionOutput, MeshOperationFraming> {
        let record = self.record(
            index,
            "conversion output wrapper",
            "D26351F0-5940-4D23-AA20-2C35475A6D9E",
            "CB844AB6-240D-4FC9-9C9F-3679DC896D6F",
            "Fusion",
            Some(2),
        )?;
        if record
            .get(output::ZERO_RUN_10..output::GUID_LENGTH)
            .is_none_or(|run| run.iter().any(|byte| *byte != 0))
        {
            return Err(MeshOperationFraming("conversion output wrapper prefix"));
        }
        let (guid, at) = crate::bytes::lp_utf16_bounded(record, output::GUID_LENGTH, 36..=36)
            .ok_or(MeshOperationFraming("conversion output body GUID"))?;
        let mut cursor = Cursor {
            bytes: record,
            at,
            deviations: Vec::new(),
        };
        let control = cursor.u8("conversion output control")?;
        if cursor.u8("conversion output presence")? != 1 {
            return Err(MeshOperationFraming("conversion output presence"));
        }
        Ok(MeshConversionOutput {
            guid,
            control,
            body: cursor.reference("conversion output body reference")?,
        })
    }
}

fn output_envelope(bytes: &[u8], at: usize) -> Option<(usize, usize)> {
    let count = cadmpeg_core::decode::View::u32_le_at(bytes, at)?;
    let start = at.checked_add(4)?;
    let count = bounded_len(u64::from(count), 11, bytes.len().checked_sub(start)?)?;
    let end = start.checked_add(count.checked_mul(11)?)?;
    let guid = end.checked_add(8)?;
    if bytes.get(end..guid)?.iter().any(|byte| *byte != 0) {
        return None;
    }
    crate::bytes::lp_utf16_bounded_view(bytes, guid, 36..=36)?;
    Some((count, start))
}

fn read_scope(
    ctx: &DecodeContext<'_>,
    bytes: &[u8],
) -> Result<Result<MeshConversionScope, MeshOperationFraming>, CodecError> {
    if bytes
        .get(prefix::ZERO_RUN_9..prefix::FLAG)
        .is_none_or(|run| run.iter().any(|byte| *byte != 0))
    {
        return Ok(Err(MeshOperationFraming("conversion scope prefix")));
    }
    let mut cursor = Cursor {
        bytes,
        at: prefix::FLAG,
        deviations: Vec::new(),
    };
    let prefix_word = match cursor.u8("conversion scope flag") {
        Ok(0) => None,
        Ok(1) => match cursor.u32("conversion scope prefix word") {
            Ok(word) => Some(word),
            Err(error) => return Ok(Err(error)),
        },
        _ => return Ok(Err(MeshOperationFraming("conversion scope flag"))),
    };
    let control = match cursor.u32("conversion control") {
        Ok(control) => control,
        Err(error) => return Ok(Err(error)),
    };
    let mut parsed = MeshConversionScope {
        prefix_word,
        control,
        input_count: None,
        input_wrappers: Vec::new(),
        output_wrappers: Vec::new(),
        tail_control: None,
        tail_error: None,
        deviations: Vec::new(),
    };
    let read = (|| -> Result<Result<(), MeshOperationFraming>, CodecError> {
        let declared = match cursor.u32("conversion input count") {
            Ok(count) => count,
            Err(error) => return Ok(Err(error)),
        };
        parsed.input_count = Some(declared);
        let Some(count) = bounded_len(u64::from(declared), 11, bytes.len() - cursor.at) else {
            return Ok(Err(MeshOperationFraming("conversion input count")));
        };
        parsed.input_wrappers = ctx.alloc_filled(count, 0, "F3D mesh conversion inputs")?;
        for input in &mut parsed.input_wrappers {
            match cursor.optional_reference("conversion input wrapper reference") {
                Ok(reference) => *input = reference.unwrap_or(0),
                Err(error) => return Ok(Err(error)),
            }
        }
        let tail = (|| {
            cursor.expect_u32("conversion-tail-word", 0xFFFF_FFFF)?;
            cursor.expect_u8("conversion-tail-byte:0", 0)?;
            cursor.expect_u8("conversion-tail-byte:1", 0)?;
            let short = output_envelope(bytes, cursor.at);
            let long_at = cursor.at.checked_add(4).ok_or(MeshOperationFraming("conversion output extent"))?;
            let long = output_envelope(bytes, long_at);
            match (short, long) {
                (Some(envelope), None) => Ok(envelope),
                (None, Some(envelope)) => {
                    parsed.tail_control = Some(cursor.u32("conversion output envelope word")?);
                    Ok(envelope)
                }
                _ => Err(MeshOperationFraming("conversion output envelope")),
            }
        })();
        let (count, start) = match tail {
            Ok(envelope) => envelope,
            Err(error) => return Ok(Err(error)),
        };
        cursor.at = start;
        parsed.output_wrappers = ctx.alloc_filled(count, 0, "F3D mesh conversion outputs")?;
        for output in &mut parsed.output_wrappers {
            match cursor.optional_reference("conversion output wrapper reference") {
                Ok(reference) => *output = reference.unwrap_or(0),
                Err(error) => return Ok(Err(error)),
            }
        }
        Ok(Ok(()))
    })()?;
    parsed.tail_error = read.err();
    parsed.deviations = cursor.deviations;
    Ok(Ok(parsed))
}

#[cfg(test)]
mod tests;
