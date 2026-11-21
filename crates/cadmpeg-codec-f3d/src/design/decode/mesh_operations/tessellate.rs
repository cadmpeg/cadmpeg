use super::{Cursor, MeshOperationFraming, MeshOperationRecords};
use crate::layout::mesh_tessellate_operand as operand;
use crate::layout::mesh_tessellate_scope_prefix as prefix;
use cadmpeg_core::decode::{bounded_len, DecodeContext};
use cadmpeg_core::CodecError;

const TAIL: [u8; 6] = [1, 1, 0, 0, 0, 0];

pub(crate) const SCOPE_TYPE_GUID: &str = "86133A9B-B604-4283-B9BA-C5D365F72763";
pub(crate) const SCOPE_BASE_TYPE_GUID: &str = "95644CD5-1338-40F2-8016-1FB68DB06296";
pub(crate) const SCOPE_TYPE_VERSION: u32 = 3;
pub(crate) const OPERAND_TYPE_GUID: &str = "7102E543-AD48-4709-B559-286F5F3A7189";
pub(crate) const OPERAND_BASE_TYPE_GUID: &str = "D0D7E795-ECF7-48A8-A7A3-F983C5A29F8B";
pub(crate) const OPERAND_TYPE_VERSION: u32 = 1;

/// The parameter-owner slots of a `MeshTessellate` scope, in scope order.
pub(crate) const OWNER_ROLES: [&str; 4] = [
    "maximum-edge-length",
    "normal-deviation",
    "aspect-ratio",
    "surface-deviation",
];

#[derive(Debug, PartialEq, Eq)]
pub(crate) struct MeshTessellateScope {
    pub(crate) owners: [Option<u32>; 4],
    pub(crate) operand_count: Option<u32>,
    pub(crate) operands: Vec<u32>,
    pub(crate) tail_error: Option<MeshOperationFraming>,
    pub(crate) deviations: Vec<(&'static str, u32)>,
}

#[derive(Debug, PartialEq, Eq)]
pub(crate) struct MeshTessellateOperand {
    pub(crate) group: u32,
    pub(crate) body: u32,
    pub(crate) end: usize,
    pub(crate) deviations: Vec<(&'static str, u32)>,
}

impl MeshOperationRecords<'_> {
    pub(crate) fn tessellate_scope(
        &self,
        ctx: &DecodeContext<'_>,
        index: u32,
    ) -> Result<Result<MeshTessellateScope, MeshOperationFraming>, CodecError> {
        let record = match self.record(
            index,
            "tessellate scope record",
            SCOPE_TYPE_GUID,
            SCOPE_BASE_TYPE_GUID,
            "Fusion",
            Some(SCOPE_TYPE_VERSION),
        ) {
            Ok(record) => record,
            Err(error) => return Ok(Err(error)),
        };
        read_scope(ctx, record.window())
    }

    pub(crate) fn tessellate_operand(
        &self,
        index: u32,
    ) -> Result<MeshTessellateOperand, MeshOperationFraming> {
        let record = self
            .record(
                index,
                "tessellation operand record",
                OPERAND_TYPE_GUID,
                OPERAND_BASE_TYPE_GUID,
                "Fusion",
                Some(OPERAND_TYPE_VERSION),
            )?
            .window();
        let operand = read_operand(record)?;
        if record.len() != operand.end {
            return Err(MeshOperationFraming("tessellation operand length"));
        }
        Ok(operand)
    }
}

fn expect_tail(
    cursor: &mut Cursor<'_>,
    fields: [&'static str; 6],
) -> Result<(), MeshOperationFraming> {
    for (field, expected) in fields.into_iter().zip(TAIL) {
        cursor.expect_u8(field, expected)?;
    }
    Ok(())
}

pub(crate) fn read_scope(
    ctx: &DecodeContext<'_>,
    bytes: &[u8],
) -> Result<Result<MeshTessellateScope, MeshOperationFraming>, CodecError> {
    if bytes
        .get(prefix::ZERO_RUN_9..prefix::FLAG)
        .is_none_or(|run| run.iter().any(|byte| *byte != 0))
    {
        return Ok(Err(MeshOperationFraming("tessellate scope prefix")));
    }
    if bytes.get(prefix::FLAG) != Some(&prefix::FLAG_VALUE) {
        return Ok(Err(MeshOperationFraming("tessellate scope flag")));
    }
    let mut cursor = Cursor {
        bytes,
        at: prefix::EDGE_LENGTH_OWNER,
        deviations: Vec::new(),
    };
    let mut parsed = MeshTessellateScope {
        owners: [None; 4],
        operand_count: None,
        operands: Vec::new(),
        tail_error: None,
        deviations: Vec::new(),
    };
    let read = (|| -> Result<Result<(), MeshOperationFraming>, CodecError> {
        for owner in &mut parsed.owners {
            match cursor.optional_reference("tessellate owner reference") {
                Ok(reference) => *owner = reference,
                Err(error) => return Ok(Err(error)),
            }
        }
        if cursor.at != prefix::OPERAND_COUNT {
            return Ok(Err(MeshOperationFraming("tessellate owner reference")));
        }
        let declared = match cursor.u32("tessellate operand count") {
            Ok(count) => count,
            Err(error) => return Ok(Err(error)),
        };
        parsed.operand_count = Some(declared);
        let Some(count) = bounded_len(u64::from(declared), 1, bytes.len() - cursor.at) else {
            return Ok(Err(MeshOperationFraming("tessellate operand count")));
        };
        parsed.operands = ctx.alloc_filled(count, 0, "F3D mesh tessellate operands")?;
        for operand in &mut parsed.operands {
            match cursor.reference("tessellate operand reference") {
                Ok(reference) => *operand = reference,
                Err(error) => return Ok(Err(error)),
            }
        }
        Ok(expect_tail(
            &mut cursor,
            [
                "tessellate-tail-byte:0",
                "tessellate-tail-byte:1",
                "tessellate-tail-byte:2",
                "tessellate-tail-byte:3",
                "tessellate-tail-byte:4",
                "tessellate-tail-byte:5",
            ],
        ))
    })()?;
    parsed.tail_error = read.err();
    parsed.deviations = cursor.deviations;
    Ok(Ok(parsed))
}

pub(crate) fn read_operand(bytes: &[u8]) -> Result<MeshTessellateOperand, MeshOperationFraming> {
    if bytes
        .get(operand::ZERO_RUN_10..operand::GROUP_REFERENCE)
        .is_none_or(|run| run.iter().any(|byte| *byte != 0))
    {
        return Err(MeshOperationFraming("tessellation operand prefix"));
    }
    let mut cursor = Cursor {
        bytes,
        at: operand::GROUP_REFERENCE,
        deviations: Vec::new(),
    };
    let group = cursor.reference("tessellation operand group reference")?;
    if cursor.at != operand::BODY_REFERENCE {
        return Err(MeshOperationFraming("tessellation operand group reference"));
    }
    let body = cursor.reference("tessellation operand body reference")?;
    expect_tail(
        &mut cursor,
        [
            "tessellation-operand-tail-byte:0",
            "tessellation-operand-tail-byte:1",
            "tessellation-operand-tail-byte:2",
            "tessellation-operand-tail-byte:3",
            "tessellation-operand-tail-byte:4",
            "tessellation-operand-tail-byte:5",
        ],
    )?;
    Ok(MeshTessellateOperand {
        group,
        body,
        end: cursor.at,
        deviations: cursor.deviations,
    })
}

#[cfg(test)]
mod tests;
