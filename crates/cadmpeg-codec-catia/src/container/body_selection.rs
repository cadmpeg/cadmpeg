// SPDX-License-Identifier: Apache-2.0
//! Visible body allocation ends in the declared geometry container.

use cadmpeg_core::decode::{DecodeContext, View};
use cadmpeg_core::CodecError;

use super::{reconstruct_logical_stream, ContainerScan, Descriptor};
use crate::layout::{visible_body_roster_prefix as prefix, visible_body_roster_suffix as suffix};
use crate::wire::records::SourceExtent;

pub(crate) struct BodySelection {
    pub(crate) source: usize,
    pub(crate) allocation_ends: Vec<usize>,
}

fn slots(ctx: &DecodeContext<'_>, bytes: &[u8]) -> Result<Option<Vec<usize>>, CodecError> {
    let Some(count) = View::u32_le_at(bytes, prefix::COUNT) else {
        return Ok(None);
    };
    let mut view = View::over_retained(bytes.get(prefix::LEN..).unwrap_or_default());
    if view.counted(u64::from(count), 4).is_none() {
        return Ok(None);
    }
    let count = usize::try_from(count)
        .map_err(|_| CodecError::malformed("CATIA visible body count exceeds usize"))?;
    let mut values = ctx.alloc_filled(count, 0, "CATIA visible body allocation ends")?;
    for value in &mut values {
        let Some(slot) = view.u32_le().filter(|&slot| slot != 0) else {
            return Ok(None);
        };
        *value = usize::try_from(slot)
            .map_err(|_| CodecError::malformed("CATIA visible body allocation exceeds usize"))?;
    }
    Ok(
        (view.take(suffix::LEN) == Some(&[0; suffix::LEN]) && view.remaining() == 0)
            .then_some(values),
    )
}

pub(crate) fn read(
    ctx: &DecodeContext<'_>,
    scan: &ContainerScan<'_>,
    sources: &[Vec<SourceExtent>],
) -> Result<Option<BodySelection>, CodecError> {
    let Some(directory) = scan.outer.as_ref() else {
        return Ok(None);
    };
    let declared = |class| -> Option<&Descriptor> {
        let mut declarations = scan
            .outer_container_declarations
            .iter()
            .filter(|declaration| declaration.class_name == class);
        let declaration = declarations.next()?;
        if declarations.next().is_some() {
            return None;
        }
        let mut descriptors = directory
            .descriptors
            .iter()
            .filter(|descriptor| descriptor.name == declaration.stream_name);
        let descriptor = descriptors.next()?;
        descriptors.next().is_none().then_some(descriptor)
    };
    let (Some(selection), Some(geometry)) = (declared("CATSeeBodyCont"), declared("CGMGeom"))
    else {
        return Ok(None);
    };
    let mut extents = Vec::new();
    for extent in &geometry.extents {
        let start = usize::try_from(extent.phys_off)
            .map_err(|_| CodecError::malformed("CATIA geometry extent exceeds usize"))?;
        let length = usize::try_from(extent.phys_len)
            .map_err(|_| CodecError::malformed("CATIA geometry length exceeds usize"))?;
        let Some(end) = start.checked_add(length) else {
            return Ok(None);
        };
        let Some(extent) = SourceExtent::within(&scan.data, start, end) else {
            return Ok(None);
        };
        extents.push(extent);
    }
    let Some(source) = sources.iter().position(|source| source == &extents) else {
        return Ok(None);
    };
    let bytes = reconstruct_logical_stream(&scan.data, selection, directory.inner);
    Ok(slots(ctx, &bytes)?.map(|allocation_ends| BodySelection {
        source,
        allocation_ends,
    }))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn visible_body_roster_preserves_absolute_allocation_ends() {
        let mut bytes = vec![2, 0, 0, 0];
        bytes.extend(7_u32.to_le_bytes());
        bytes.extend(503_u32.to_le_bytes());
        bytes.extend([0; 16]);
        crate::test_support::with_service_context(|ctx| {
            assert_eq!(slots(ctx, &bytes).expect("decode"), Some(vec![7, 503]));
            assert_eq!(slots(ctx, &[0; 20]).expect("empty"), Some(Vec::new()));
            assert!(slots(ctx, &bytes[..bytes.len() - 1])
                .expect("truncated")
                .is_none());
            bytes[0] = 255;
            assert!(slots(ctx, &bytes).expect("count").is_none());
        });
    }
}
