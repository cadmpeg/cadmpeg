// SPDX-License-Identifier: Apache-2.0
use super::parameter_scope::payload_prologue;
use crate::bytes::finite_reals_at;
use crate::bytes::lp_ascii_filtered_view;
use crate::bytes::utf16::Utf16View;
use crate::bytes::Reference;
use crate::bytes::{take_reference, take_reference_lossless};
use crate::design::decode::sketch::next_indexed_record_offset;
use crate::loss::F3dLossCode;
use crate::records::feature::hole::DesignHoleTangentPoint;
use crate::records::feature::scope::DesignParameterScope;
use crate::records::identity::Located;
use cadmpeg_core::decode::{u64_from_index, DecodeContext, View};
use cadmpeg_core::CodecError;
use cadmpeg_ir::report::loss::LossNote;
use cadmpeg_ir::scalar::FiniteReal;

#[derive(Clone, Copy)]
pub(super) enum PointDirectionBoundary {
    Indexed(usize),
    Registered(usize),
}

#[derive(Debug, Clone, PartialEq)]
pub(super) struct PointDirectionCarrier<'a> {
    pub(super) bounding_box_index: u32,
    pub(super) position: Option<[FiniteReal; 3]>,
    pub(super) position_at: usize,
    pub(super) direction: Option<[FiniteReal; 3]>,
    pub(super) direction_at: usize,
    pub(super) point_parameters: Option<[FiniteReal; 2]>,
    pub(super) point_parameters_at: usize,
    pub(super) reference_type: Option<u32>,
    pub(super) reference_type_at: usize,
    pub(super) tangent_point_data: Option<DesignHoleTangentPoint>,
    pub(super) input_count: Option<Located<u32>>,
    pub(super) inputs: Vec<Located<Reference<&'a str, Utf16View<'a>>>>,
    pub(super) inputs_complete: bool,
}

impl PointDirectionCarrier<'_> {
    pub(super) fn local_inputs(&self) -> impl Iterator<Item = Located<u32>> + '_ {
        self.inputs.iter().filter_map(|input| {
            let (target, _) = input.value.local()?;
            (target != 0).then_some(Located {
                value: u32::try_from(target).ok()?,
                offset: input.offset.checked_add(1)?,
            })
        })
    }
}

pub(super) fn point_direction_input_loss(
    scope: &DesignParameterScope,
    carrier_record_index: u32,
    omitted_inputs: usize,
) -> Option<LossNote> {
    (omitted_inputs != 0).then(|| {
        F3dLossCode::FeaturePointDirectionInputsUnrepresented.note(format!(
            "{} scope {} carrier {carrier_record_index} has {omitted_inputs} input slots without a nonzero local u32 target.",
            scope.kind(),
            scope.id,
        ))
    })
}

pub(super) fn point_direction_carrier_at<'a>(
    ctx: &DecodeContext<'_>,
    bytes: &'a [u8],
    start: usize,
    boundary: PointDirectionBoundary,
    record_index: u32,
    version: u32,
) -> Result<Option<PointDirectionCarrier<'a>>, CodecError> {
    let Some((mut carrier, input_at)) =
        point_direction_prefix_at(bytes, start, boundary, record_index, version)
    else {
        return Ok(None);
    };
    let Some(mut cursor) = input_at else {
        return Ok(Some(carrier));
    };
    let paired_at = match boundary {
        PointDirectionBoundary::Indexed(at) | PointDirectionBoundary::Registered(at) => at,
    };
    let Some(body) = bytes.get(..paired_at) else {
        return Ok(Some(carrier));
    };
    let Some(count) = View::u32_le_at(body, cursor) else {
        return Ok(Some(carrier));
    };
    carrier.input_count = Some(Located {
        value: count,
        offset: u64_from_index(cursor),
    });
    let Some(after_count) = cursor.checked_add(4) else {
        return Ok(Some(carrier));
    };
    cursor = after_count;
    let Some(remaining) = body.get(cursor..) else {
        return Ok(Some(carrier));
    };
    let admitted = u64::from(count).min(u64_from_index(remaining.len()));
    let Some(bounded) = View::over_retained(remaining).counted(admitted, 1) else {
        return Ok(Some(carrier));
    };
    let capacity = bounded.get();
    let read_reference = |cursor: &mut usize| match boundary {
        PointDirectionBoundary::Registered(_) => take_reference_lossless(body, cursor),
        PointDirectionBoundary::Indexed(_) => take_reference(body, cursor),
    };
    let mut read = 0;
    for _ in 0..capacity {
        if read_reference(&mut cursor).is_none() {
            break;
        }
        read += 1;
    }
    let mut inputs = ctx.alloc_filled(
        read,
        Located {
            value: Reference::Null,
            offset: 0,
        },
        "point-direction inputs",
    )?;
    let mut retained_cursor = after_count;
    for slot in &mut inputs {
        let reference_at = retained_cursor;
        let value = read_reference(&mut retained_cursor).ok_or_else(|| {
            CodecError::Malformed("point-direction reference prefix changed while reading".into())
        })?;
        *slot = Located {
            value,
            offset: u64_from_index(reference_at),
        };
    }
    carrier.inputs_complete = u64_from_index(read) == u64::from(count)
        && match boundary {
            PointDirectionBoundary::Registered(_) => cursor == paired_at,
            PointDirectionBoundary::Indexed(_) => {
                cursor == paired_at || next_indexed_record_offset(bytes, cursor) == Some(paired_at)
            }
        };
    carrier.inputs = inputs;
    Ok(Some(carrier))
}

fn point_direction_prefix_at(
    bytes: &[u8],
    start: usize,
    boundary: PointDirectionBoundary,
    record_index: u32,
    version: u32,
) -> Option<(PointDirectionCarrier<'_>, Option<usize>)> {
    if !matches!(version, 1 | 2 | 4) {
        return None;
    }
    let paired_at = match boundary {
        PointDirectionBoundary::Indexed(at) | PointDirectionBoundary::Registered(at) => at,
    };
    let (_, after_tag) = lp_ascii_filtered_view(bytes, start, 3..=3, u8::is_ascii_digit)?;
    if after_tag != start.checked_add(7)? || View::u32_le_at(bytes, after_tag) != Some(record_index)
    {
        return None;
    }
    let name_at = after_tag.checked_add(8)?;
    let name_length = usize::try_from(View::u32_le_at(bytes, name_at)?).ok()?;
    if name_length > 256 {
        return None;
    }
    let payload_at = name_at.checked_add(4)?.checked_add(name_length)?;
    if !bytes
        .get(name_at.checked_add(4)?..payload_at)?
        .iter()
        .all(u8::is_ascii_graphic)
    {
        return None;
    }
    let body = bytes.get(..paired_at)?;
    if matches!(boundary, PointDirectionBoundary::Registered(_)) && body.get(payload_at) != Some(&0)
    {
        return None;
    }
    let mut cursor = payload_prologue(body, payload_at, paired_at)?;
    let bounding_box_index = View::u32_le_at(body, cursor)?;
    cursor = cursor.checked_add(4)?;
    let position_at = cursor;
    cursor = cursor.checked_add(24)?;
    let direction_at = cursor;
    cursor = cursor.checked_add(24)?;
    body.get(direction_at..cursor)?;
    let point_parameters_at = cursor;
    cursor = cursor.checked_add(16)?;
    let reference_type_at = cursor;
    let reference_type = View::u32_le_at(body, cursor);
    cursor = cursor.checked_add(4)?;
    let mut carrier = PointDirectionCarrier {
        bounding_box_index,
        position: finite_reals_at(body, position_at),
        position_at,
        direction: finite_reals_at(body, direction_at),
        direction_at,
        point_parameters: finite_reals_at(body, point_parameters_at),
        point_parameters_at,
        reference_type,
        reference_type_at,
        tangent_point_data: None,
        input_count: None,
        inputs: Vec::new(),
        inputs_complete: false,
    };
    if matches!(boundary, PointDirectionBoundary::Indexed(_))
        && (carrier.direction.is_none()
            || carrier.position.is_none()
            || carrier.point_parameters.is_none()
            || carrier.reference_type.is_none())
    {
        return None;
    }
    let tail = (|| {
        reference_type?;
        carrier.tangent_point_data = match version {
            2 | 4 => {
                let prefix = if version == 4 {
                    let prefix = *body.get(cursor)?;
                    cursor = cursor.checked_add(1)?;
                    Some(prefix)
                } else {
                    None
                };
                let tangent_point_data_at = cursor;
                cursor = cursor.checked_add(24)?;
                body.get(tangent_point_data_at..cursor)?;
                finite_reals_at(body, tangent_point_data_at).map(|value| DesignHoleTangentPoint {
                    prefix,
                    data: Located {
                        value,
                        offset: u64_from_index(tangent_point_data_at),
                    },
                })
            }
            1 => None,
            _ => return None,
        };
        if matches!(boundary, PointDirectionBoundary::Indexed(_))
            && version != 1
            && carrier.tangent_point_data.is_none()
        {
            return None;
        }
        Some(cursor)
    })();
    Some((carrier, tail))
}
