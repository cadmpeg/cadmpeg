use super::{MeshOperationFraming, MeshOperationRecords};
use crate::bytes::take_reference;
use crate::layout::mesh_operation_scope_prefix as prefix;
use crate::layout::mesh_plane_cut_scope_prefix as cut;
use cadmpeg_core::decode::View;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum PlaneCutReference<'a> {
    Null,
    Local(u32),
    TypedLocal { target: u32, inline_type: &'a str },
    Other(&'a [u8]),
}

#[derive(Debug, PartialEq, Eq)]
pub(crate) struct MeshPlaneCutScope<'a> {
    pub(crate) prefix: &'a [u8],
    pub(crate) flag: Option<u8>,
    pub(crate) prefix_word: Option<u32>,
    pub(crate) mode: Option<u32>,
    pub(crate) matrix_count: Option<u32>,
    pub(crate) matrix: [Option<u64>; 16],
    pub(crate) fill: Option<u32>,
    pub(crate) side: Option<u8>,
    pub(crate) references: [Option<PlaneCutReference<'a>>; 6],
    pub(crate) tail_word: Option<u32>,
    pub(crate) tail_byte: Option<u8>,
    pub(crate) framing: Option<MeshOperationFraming>,
}

struct Cursor<'a>(View<'a>);

impl<'a> Cursor<'a> {
    fn reference(&mut self) -> Result<PlaneCutReference<'a>, MeshOperationFraming> {
        let error = MeshOperationFraming("plane cut reference");
        let start = self.0.position() - self.0.start();
        let mut end = start;
        let reference = take_reference(self.0.window(), &mut end).ok_or(error)?;
        self.0
            .seek(self.0.start().checked_add(end).ok_or(error)?)
            .ok_or(error)?;
        if end == start + 1 {
            return Ok(PlaneCutReference::Null);
        }
        if let Some((target, inline_type)) = reference.into_local() {
            if let Some(target) = u32::try_from(target).ok().filter(|target| *target != 0) {
                return Ok(match inline_type {
                    Some(inline_type) => PlaneCutReference::TypedLocal {
                        target,
                        inline_type,
                    },
                    None => PlaneCutReference::Local(target),
                });
            }
        }
        Ok(PlaneCutReference::Other(&self.0.window()[start..end]))
    }
}

impl<'a> MeshOperationRecords<'a> {
    pub(crate) fn plane_cut_scope(
        &self,
        index: u32,
    ) -> Result<MeshPlaneCutScope<'a>, MeshOperationFraming> {
        let frame = self.record(
            index,
            "plane cut scope record",
            "E3EB3760-4362-48D6-ABF3-698B0C12C5A9",
            "90B8D2C6-05BB-4D0F-89EF-83AAC46CA1FF",
            "Fusion",
            Some(1),
        )?;
        Ok(read_scope(frame))
    }
}

pub(crate) fn read_scope(frame: View<'_>) -> MeshPlaneCutScope<'_> {
    let mut parsed = MeshPlaneCutScope {
        prefix: frame
            .window()
            .get(prefix::ZERO_RUN_9..frame.window().len().min(prefix::FLAG))
            .unwrap_or(&[]),
        flag: None,
        prefix_word: None,
        mode: None,
        matrix_count: None,
        matrix: [None; 16],
        fill: None,
        side: None,
        references: [None; 6],
        tail_word: None,
        tail_byte: None,
        framing: None,
    };
    let mut cursor = Cursor(frame);
    let result = (|| {
        let at = frame
            .start()
            .checked_add(prefix::FLAG)
            .ok_or(MeshOperationFraming("plane cut prefix"))?;
        cursor
            .0
            .seek(at)
            .ok_or(MeshOperationFraming("plane cut prefix"))?;
        let flag = cursor
            .0
            .u8()
            .ok_or(MeshOperationFraming("plane cut flag"))?;
        parsed.flag = Some(flag);
        match flag {
            0 => (),
            1 => {
                parsed.prefix_word = Some(
                    cursor
                        .0
                        .u32_le()
                        .ok_or(MeshOperationFraming("plane cut prefix word"))?,
                );
            }
            _ => return Err(MeshOperationFraming("plane cut flag")),
        }
        parsed.mode = Some(
            cursor
                .0
                .u32_le()
                .ok_or(MeshOperationFraming("plane cut mode"))?,
        );
        let count = cursor
            .0
            .u32_le()
            .ok_or(MeshOperationFraming("plane cut matrix count"))?;
        parsed.matrix_count = Some(count);
        if usize::try_from(count).ok() != Some((cut::FILL_SELECTOR - cut::MATRIX) / 8) {
            return Err(MeshOperationFraming("plane cut matrix count"));
        }
        for bits in &mut parsed.matrix {
            *bits = Some(
                cursor
                    .0
                    .u64_le()
                    .ok_or(MeshOperationFraming("plane cut matrix component"))?,
            );
        }
        parsed.fill = Some(
            cursor
                .0
                .u32_le()
                .ok_or(MeshOperationFraming("plane cut fill"))?,
        );
        parsed.side = Some(
            cursor
                .0
                .u8()
                .ok_or(MeshOperationFraming("plane cut side"))?,
        );
        for reference in &mut parsed.references[..5] {
            *reference = Some(cursor.reference()?);
        }
        parsed.tail_word = Some(
            cursor
                .0
                .u32_le()
                .ok_or(MeshOperationFraming("plane cut tail word"))?,
        );
        parsed.references[5] = Some(cursor.reference()?);
        parsed.tail_byte = Some(
            cursor
                .0
                .u8()
                .ok_or(MeshOperationFraming("plane cut tail byte"))?,
        );
        Ok(())
    })();
    parsed.framing = result.err();
    parsed
}

#[cfg(test)]
mod tests;
