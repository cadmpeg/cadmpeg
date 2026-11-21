use super::{MeshOperationFraming, MeshOperationRecords};
use crate::bytes::take_reference;
use crate::layout::mesh_operation_scope_prefix as prefix;
use cadmpeg_core::decode::{bounded_len, DecodeContext, View};
use cadmpeg_core::CodecError;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum CombineReference<'a> {
    Null,
    Local(u32),
    TypedLocal { target: u32, inline_type: &'a str },
    Other(&'a [u8]),
}

#[derive(Debug, PartialEq, Eq)]
pub(crate) struct MeshCombineScope<'a> {
    pub(crate) version: u32,
    pub(crate) prefix: &'a [u8],
    pub(crate) flag: Option<u8>,
    pub(crate) prefix_word: Option<u32>,
    pub(crate) control: Option<u32>,
    pub(crate) extra_word: Option<u32>,
    pub(crate) raw_byte: Option<u8>,
    pub(crate) tool_count: Option<u32>,
    pub(crate) tools: Vec<CombineReference<'a>>,
    pub(crate) output_geometry: Option<CombineReference<'a>>,
    pub(crate) input_geometry: Option<CombineReference<'a>>,
    pub(crate) tail_word: Option<u32>,
    pub(crate) target: Option<CombineReference<'a>>,
    pub(crate) framing: Option<MeshOperationFraming>,
}

struct Cursor<'a>(View<'a>);

impl<'a> Cursor<'a> {
    fn word(&mut self, field: &'static str) -> Result<u32, MeshOperationFraming> {
        self.0.u32_le().ok_or(MeshOperationFraming(field))
    }

    fn byte(&mut self, field: &'static str) -> Result<u8, MeshOperationFraming> {
        self.0.u8().ok_or(MeshOperationFraming(field))
    }

    fn reference(
        &mut self,
        field: &'static str,
    ) -> Result<CombineReference<'a>, MeshOperationFraming> {
        let start = self.0.position() - self.0.start();
        let mut end = start;
        let reference =
            take_reference(self.0.window(), &mut end).ok_or(MeshOperationFraming(field))?;
        let at = self
            .0
            .start()
            .checked_add(end)
            .ok_or(MeshOperationFraming(field))?;
        self.0.seek(at).ok_or(MeshOperationFraming(field))?;
        if end == start + 1 {
            return Ok(CombineReference::Null);
        }
        if let Some((target, inline_type)) = reference.into_local() {
            if let Some(target) = u32::try_from(target).ok().filter(|target| *target != 0) {
                return Ok(match inline_type {
                    Some(inline_type) => CombineReference::TypedLocal {
                        target,
                        inline_type,
                    },
                    None => CombineReference::Local(target),
                });
            }
        }
        Ok(CombineReference::Other(&self.0.window()[start..end]))
    }
}

impl<'a> MeshOperationRecords<'a> {
    pub(crate) fn combine_scope(
        &self,
        ctx: &DecodeContext<'_>,
        index: u32,
    ) -> Result<Result<MeshCombineScope<'a>, MeshOperationFraming>, CodecError> {
        for version in [1, 2] {
            if let Ok(record) = self.record(
                index,
                "combine scope record",
                "EE0E56E8-5894-4D65-BAF4-9DAAF811307F",
                "90B8D2C6-05BB-4D0F-89EF-83AAC46CA1FF",
                "Fusion",
                Some(version),
            ) {
                return read_scope(ctx, record, version).map(Ok);
            }
        }
        Ok(Err(MeshOperationFraming("combine scope record")))
    }
}

pub(crate) fn read_scope<'a>(
    ctx: &DecodeContext<'_>,
    frame: View<'a>,
    version: u32,
) -> Result<MeshCombineScope<'a>, CodecError> {
    let mut parsed = MeshCombineScope {
        version,
        prefix: frame
            .window()
            .get(prefix::ZERO_RUN_9..prefix::FLAG)
            .unwrap_or(&[]),
        flag: None,
        prefix_word: None,
        control: None,
        extra_word: None,
        raw_byte: None,
        tool_count: None,
        tools: Vec::new(),
        output_geometry: None,
        input_geometry: None,
        tail_word: None,
        target: None,
        framing: None,
    };
    let mut cursor = Cursor(frame);
    let read = (|| -> Result<Result<(), MeshOperationFraming>, CodecError> {
        let result = (|| -> Result<(), MeshOperationFraming> {
            let at = frame
                .start()
                .checked_add(prefix::FLAG)
                .ok_or(MeshOperationFraming("combine prefix"))?;
            cursor
                .0
                .seek(at)
                .ok_or(MeshOperationFraming("combine prefix"))?;
            let flag = cursor.byte("combine flag")?;
            parsed.flag = Some(flag);
            match flag {
                0 => (),
                1 => parsed.prefix_word = Some(cursor.word("combine prefix word")?),
                _ => return Err(MeshOperationFraming("combine flag")),
            }
            parsed.control = Some(cursor.word("combine operation")?);
            if version == 2 {
                parsed.extra_word = Some(cursor.word("combine version word")?);
            }
            parsed.raw_byte = Some(cursor.byte("combine control byte")?);
            parsed.tool_count = Some(cursor.word("combine tool count")?);
            Ok(())
        })();
        if let Err(error) = result {
            return Ok(Err(error));
        }
        let declared = parsed.tool_count.unwrap_or(0);
        let complete = bounded_len(u64::from(declared), 1, cursor.0.remaining());
        let count = complete.unwrap_or(cursor.0.remaining());
        parsed.tools = ctx.alloc_filled(count, CombineReference::Null, "F3D mesh combine tools")?;
        for ordinal in 0..count {
            match cursor.reference("combine tool reference") {
                Ok(reference) => parsed.tools[ordinal] = reference,
                Err(error) => {
                    parsed.tools.truncate(ordinal);
                    return Ok(Err(error));
                }
            }
        }
        if complete.is_none() {
            return Ok(Err(MeshOperationFraming("combine tool count")));
        }
        Ok((|| {
            parsed.output_geometry = Some(cursor.reference("combine output geometry")?);
            parsed.input_geometry = Some(cursor.reference("combine input geometry")?);
            parsed.tail_word = Some(cursor.word("combine tail word")?);
            parsed.target = Some(cursor.reference("combine target group")?);
            Ok(())
        })())
    })()?;
    parsed.framing = read.err();
    Ok(parsed)
}

#[cfg(test)]
mod tests;
