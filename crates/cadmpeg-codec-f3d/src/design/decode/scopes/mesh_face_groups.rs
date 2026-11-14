use crate::bytes::{lp_ascii_filtered_view, take_reference};
use crate::layout::{mesh_face_group_uuid as uuid, mesh_face_groups_scope_prefix as prefix};
use crate::records::feature::mesh_face_groups::{registered_kind, DesignMeshFaceGroups};
use crate::records::feature::scope::{DesignFeatureKind, DesignParameterScope};
use crate::records::identity::Located;
use cadmpeg_core::decode::{u64_from_index, DecodeContext, View};
use cadmpeg_core::CodecError;

pub(super) fn decode_prefix(
    ctx: &DecodeContext<'_>,
    bytes: &[u8],
    scope: &DesignParameterScope,
    info: Option<&crate::records::entity_header::SegmentType>,
    primary_range: Option<(usize, usize)>,
) -> Result<Option<DesignMeshFaceGroups>, CodecError> {
    if info.is_none_or(|info| {
        registered_kind(info) != Some(scope.kind())
            || !info
                .entities
                .values()
                .any(|entity| *entity == u64::from(scope.record_index))
    }) || primary_range.is_none_or(|(start, end)| {
        u64::try_from(start).ok() != Some(scope.byte_offset())
            || u64::try_from(end)
                .ok()
                .is_none_or(|end| scope.paired_byte_offset() != end)
    }) {
        return Ok(None);
    }
    let mut decoded = DesignMeshFaceGroups::default();
    let counted_groups = (|| {
        let start = usize::try_from(scope.byte_offset()).ok()?;
        let end = usize::try_from(scope.reference_count_offset()).ok()?;
        let bytes = bytes.get(..end)?;
        if bytes.get(start.checked_add(prefix::ZERO_RUN)?..start.checked_add(prefix::LEN)?)?
            != [0; prefix::LEN - prefix::ZERO_RUN]
        {
            return None;
        }
        let mut at = start.checked_add(prefix::LEN)?;
        if scope.kind() == DesignFeatureKind::GenerateFaceGroups {
            for _ in 0..3 {
                take_reference(bytes, &mut at)?;
            }
            decoded.method_code = Some(Located {
                value: View::u32_le_at(bytes, at)?,
                offset: u64_from_index(at),
            });
            at = at.checked_add(4)?;
        } else {
            decoded.group_hint = Some(group_token(bytes, &mut at)?);
        }
        let related_at = at.checked_add(1)?;
        decoded.related_mesh = take_reference(bytes, &mut at)?
            .local()
            .and_then(|(target, _)| u32::try_from(target).ok())
            .map(|value| Located {
                value,
                offset: u64_from_index(related_at),
            });
        let source_at = at.checked_add(1)?;
        decoded.source_mesh = take_reference(bytes, &mut at)?
            .local()
            .and_then(|(target, _)| u32::try_from(target).ok())
            .map(|value| Located {
                value,
                offset: u64_from_index(source_at),
            });
        let count = View::u32_le_at(bytes, at)?;
        decoded.group_count = Some(Located {
            value: count,
            offset: u64_from_index(at),
        });
        at = at.checked_add(4)?;
        let count = View::over_retained(bytes.get(at..)?)
            .counted(u64::from(count), uuid::LEN)?
            .get();
        Some((bytes, at, count))
    })();
    if let Some((bytes, mut at, count)) = counted_groups {
        let mut groups = ctx.alloc_filled(
            count,
            Located {
                value: String::new(),
                offset: 0,
            },
            "mesh face-group selectors",
        )?;
        let mut read = 0;
        while read < count {
            let Some(group) = group_token(bytes, &mut at) else {
                break;
            };
            groups[read] = group;
            read += 1;
        }
        groups.truncate(read);
        decoded.groups = groups;
    }
    Ok((decoded != DesignMeshFaceGroups::default()).then_some(decoded))
}

fn group_token(bytes: &[u8], at: &mut usize) -> Option<Located<String>> {
    let (value, end) = lp_ascii_filtered_view(
        bytes,
        *at,
        (uuid::LEN - uuid::TEXT)..=(uuid::LEN - uuid::TEXT),
        |byte| byte.is_ascii_hexdigit() || *byte == b'-',
    )?;
    let offset = u64_from_index(at.checked_add(uuid::TEXT)?);
    *at = end;
    Some(Located {
        value: value.to_owned(),
        offset,
    })
}

#[cfg(test)]
mod tests;
