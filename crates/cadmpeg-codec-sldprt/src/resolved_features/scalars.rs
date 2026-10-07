//! Named scalar records, operands and feature object names.

use super::relation_records::{
    compact_scalar_layout, legacy_scalar_layout, scalar_role, shifted_value_only_scalar_trailer,
};
use super::{COMPACT_SCALAR_HEADER, NAME_MARKER, SCALAR_HEADER, VALUE_ONLY_SCALAR_HEADER};
use crate::records::{
    FeatureInputLane, FeatureInputName, FeatureInputOperand, FeatureInputOperandKind,
    FeatureInputScalar,
};
use cadmpeg_core::decode::{DecodeContext, View};
use cadmpeg_core::CodecError;
use cadmpeg_ir::scalar::FiniteReal;

use crate::layout::feature_input_operand_cell12 as operand_cell;
use crate::records::ObjectId;

pub(crate) fn named_scalars_charged(
    ctx: &DecodeContext<'_>,
    payload: &[u8],
    parent: &str,
    names: &[FeatureInputName],
) -> Result<Vec<FeatureInputScalar>, CodecError> {
    let lane_key = ctx
        .rsplit_once(parent, "#", "split SLDPRT feature-input lane key")?
        .map_or(parent, |(_, key)| key);
    let mut scalars = Vec::new();
    for name in ctx.admit_iter(names, "collect SLDPRT named scalars")? {
        let Some(name_offset) = usize::try_from(name.offset).ok() else {
            continue;
        };
        let Some(value_offset) = scalar_value_offset(payload, name_offset)? else {
            continue;
        };
        let Some(value) = View::f64_le_at(payload, value_offset).and_then(FiniteReal::new) else {
            continue;
        };
        let Some(trailer_offset) = value_offset.checked_add(8) else {
            continue;
        };
        let Some(object_id) = trailer_offset
            .checked_add(3)
            .and_then(|offset| View::u32_le_at(payload, offset))
        else {
            continue;
        };
        let ordinal = u32::try_from(scalars.len()).map_err(|_| {
            ctx.refuse_codec_limit("number SLDPRT named scalars", u64::MAX - 1, u64::MAX)
        })?;
        let offset = u64::try_from(value_offset).map_err(|_| {
            ctx.refuse_codec_limit("address SLDPRT named scalar", u64::MAX - 1, u64::MAX)
        })?;
        let operands = scalar_operands_charged(ctx, payload, trailer_offset, lane_key)?;
        let id = ctx.format_retained(
            format_args!("sldprt:feature-input:scalar#{lane_key}:{value_offset}"),
            "retain SLDPRT scalar identity",
        )?;
        let parent = ctx.copy_retained_text(parent, "retain SLDPRT scalar identity")?;
        let name_id = ctx.copy_retained_text(&name.id, "retain SLDPRT scalar identity")?;
        let role = scalar_role(payload, trailer_offset);
        ctx.push_vec(
            &mut scalars,
            FeatureInputScalar {
                id,
                parent,
                feature_ref: None,
                ordinal,
                offset,
                object_id,
                name: name_id,
                value,
                role,
                operands,
            },
            "collect SLDPRT named scalars",
        )?;
    }
    Ok(scalars)
}

/// The scalar payload offset that follows the serialized object name at
/// `name_offset`.
///
/// The name length comes from the payload's own length byte, so the offset is a
/// function of the retained bytes alone and never of a stored name value.
fn scalar_value_offset(payload: &[u8], name_offset: usize) -> Result<Option<usize>, CodecError> {
    let Some((header_offset, value_offset)) = (|| {
        let units = usize::from(*payload.get(name_offset.checked_add(NAME_MARKER.len())?)?);
        let header_offset = name_offset
            .checked_add(NAME_MARKER.len() + 1)?
            .checked_add(units.checked_mul(2)?)?;
        Some((
            header_offset,
            header_offset.checked_add(SCALAR_HEADER.len())?,
        ))
    })() else {
        return Ok(None);
    };
    if payload.get(header_offset..value_offset) == Some(SCALAR_HEADER) {
        return Ok(Some(value_offset));
    }
    let Some(compact_value_offset) = header_offset.checked_add(COMPACT_SCALAR_HEADER.len()) else {
        return Ok(None);
    };
    if payload.get(header_offset..compact_value_offset) == Some(COMPACT_SCALAR_HEADER) {
        let Some(trailer_offset) = compact_value_offset.checked_add(8) else {
            return Ok(None);
        };
        if compact_scalar_layout(payload, trailer_offset) {
            return Ok(Some(compact_value_offset));
        }
    }
    let Some((value_only_offset, shifted_value_offset, shifted_trailer_offset)) = (|| {
        let value_only_offset = header_offset.checked_add(VALUE_ONLY_SCALAR_HEADER.len())?;
        let shifted_value_offset = value_only_offset.checked_add(4)?;
        Some((
            value_only_offset,
            shifted_value_offset,
            shifted_value_offset.checked_add(8)?,
        ))
    })() else {
        return Ok(None);
    };
    if payload.get(header_offset..value_only_offset) == Some(VALUE_ONLY_SCALAR_HEADER)
        && payload.get(value_only_offset..shifted_value_offset) == Some(&[0; 4])
        && View::f64_le_at(payload, shifted_value_offset).is_some_and(f64::is_finite)
        && shifted_value_only_scalar_trailer(payload, shifted_trailer_offset)
    {
        return Ok(Some(shifted_value_offset));
    }
    Ok(
        (payload.get(header_offset..value_only_offset) == Some(VALUE_ONLY_SCALAR_HEADER))
            .then_some(value_only_offset),
    )
}

/// Whether two scalar indexes agree, field by field, with values within four
/// units in the last place.
pub(crate) fn scalar_indices_match(
    ctx: &DecodeContext<'_>,
    actual: &[FeatureInputScalar],
    expected: &[FeatureInputScalar],
) -> Result<bool, CodecError> {
    Ok(actual.len() == expected.len()
        && ctx.all_by(
            actual.iter().zip(expected),
            |(actual, expected)| scalar_matches(ctx, actual, expected),
            SCALAR_MATCH,
        )?)
}

const SCALAR_MATCH: &str = "compare SLDPRT scalar indices";

fn scalar_matches(
    ctx: &DecodeContext<'_>,
    actual: &FeatureInputScalar,
    expected: &FeatureInputScalar,
) -> Result<bool, CodecError> {
    Ok(actual.ordinal == expected.ordinal
        && actual.offset == expected.offset
        && actual.object_id == expected.object_id
        && ulp_distance(actual.value.get(), expected.value.get()) <= 4
        && actual.role == expected.role
        && actual.operands.len() == expected.operands.len()
        && ctx.equal(actual.id.as_str(), expected.id.as_str(), SCALAR_MATCH)?
        && ctx.equal(
            actual.parent.as_str(),
            expected.parent.as_str(),
            SCALAR_MATCH,
        )?
        && ctx.equal(&actual.feature_ref, &expected.feature_ref, SCALAR_MATCH)?
        && ctx.equal(actual.name.as_str(), expected.name.as_str(), SCALAR_MATCH)?
        && ctx.all_by(
            actual.operands.iter().zip(&expected.operands),
            |(actual, expected)| {
                Ok(actual.offset == expected.offset
                    && actual.kind == expected.kind
                    && actual.entity_index == expected.entity_index
                    && ctx.equal(
                        actual.reference_ref.as_str(),
                        expected.reference_ref.as_str(),
                        SCALAR_MATCH,
                    )?
                    && ctx.equal(&actual.entity_ref, &expected.entity_ref, SCALAR_MATCH)?)
            },
            SCALAR_MATCH,
        )?)
}

fn ulp_distance(left: f64, right: f64) -> u64 {
    fn ordered(value: f64) -> u64 {
        let bits = value.to_bits();
        if bits & (1 << 63) == 0 {
            bits | (1 << 63)
        } else {
            !bits
        }
    }
    ordered(left).abs_diff(ordered(right))
}

fn scalar_operands_charged(
    ctx: &DecodeContext<'_>,
    payload: &[u8],
    trailer_offset: usize,
    lane_key: &str,
) -> Result<Vec<FeatureInputOperand>, CodecError> {
    let mut operands = Vec::new();
    for ScalarOperandCell {
        offset,
        kind,
        entity_index,
    } in operand_cells(payload, trailer_offset)?
        .into_iter()
        .flatten()
    {
        let offset_u64 = u64::try_from(offset).map_err(|_| {
            ctx.refuse_codec_limit("address SLDPRT scalar operand", u64::MAX - 1, u64::MAX)
        })?;
        let reference_ref = ctx.format_retained(
            format_args!("sldprt:feature-input:reference#{lane_key}:{offset}"),
            "retain SLDPRT scalar identity",
        )?;
        ctx.push_vec(
            &mut operands,
            FeatureInputOperand {
                offset: offset_u64,
                reference_ref,
                kind,
                entity_index,
                entity_ref: None,
            },
            "collect SLDPRT scalar operands",
        )?;
    }
    Ok(operands)
}

struct ScalarOperandCell {
    offset: usize,
    kind: FeatureInputOperandKind,
    entity_index: u16,
}

fn operand_cells(
    payload: &[u8],
    trailer_offset: usize,
) -> Result<[Option<ScalarOperandCell>; 2], CodecError> {
    let compact = compact_scalar_layout(payload, trailer_offset);
    let first = if compact || !legacy_scalar_layout(payload, trailer_offset) {
        35
    } else {
        36
    };
    let second = if compact {
        43
    } else {
        first + operand_cell::LEN
    };
    Ok([first, second].map(|relative| {
        let offset = trailer_offset.checked_add(relative)?;
        if compact {
            let cell = payload.get(offset..offset.checked_add(8)?)?;
            if cell[4..8] != [0xff; 4] {
                return None;
            }
            return Some(ScalarOperandCell {
                offset,
                kind: operand_kind([cell[0], cell[1]])?,
                entity_index: View::u16_le_at(cell, 2)?,
            });
        }
        let cell = payload.get(offset..offset.checked_add(operand_cell::LEN)?)?;
        if cell[operand_cell::REFERENCE_SENTINEL..operand_cell::ZERO_TRAILER] != [0xff; 4]
            || cell[operand_cell::ZERO_TRAILER..operand_cell::LEN] != [0; 4]
        {
            return None;
        }
        Some(ScalarOperandCell {
            offset,
            kind: operand_kind([
                cell[operand_cell::CLASS_TOKEN],
                cell[operand_cell::CLASS_TOKEN + 1],
            ])?,
            entity_index: View::u16_le_at(cell, operand_cell::MARKER_ADDRESS)?,
        })
    }))
}

pub(super) fn operand_kind(tag: [u8; 2]) -> Option<FeatureInputOperandKind> {
    match tag {
        [0, 0] | [0xff, 0xff] => None,
        [0xd6, 0x80] => Some(FeatureInputOperandKind::D6),
        [0xe1, 0x80] => Some(FeatureInputOperandKind::E1),
        bytes => Some(FeatureInputOperandKind::Native(
            View::u16_le_at(&bytes, 0)?.try_into().ok()?,
        )),
    }
}

/// The serialized object name of each history feature in one lane, indexed
/// once so a feature's name is a keyed lookup rather than a scan of the lane.
///
/// A feature's name is the one lane name carrying its source object id; when
/// no name carries that id, it is the one name whose text equals the feature
/// name. A repeated id or text names no feature.
pub(crate) struct ObjectNames<'lane, 'ctx> {
    by_object: std::collections::HashMap<u32, Option<&'lane FeatureInputName>>,
    by_value: std::collections::HashMap<&'lane str, Option<&'lane FeatureInputName>>,
    _storage: [cadmpeg_core::decode::ScopedReservation<'ctx>; 2],
}

impl<'lane, 'ctx> ObjectNames<'lane, 'ctx> {
    pub(crate) fn new(
        ctx: &'ctx DecodeContext<'_>,
        lane: &'lane FeatureInputLane,
    ) -> Result<Self, CodecError> {
        let (by_object, object_storage) = ctx.unique_index(
            ctx.admit_iter(&lane.names, "index SLDPRT object names by id")?
                .filter_map(|name| Some((name.object_id?.value()?, name))),
            "index SLDPRT object names by id",
        )?;
        let (by_value, value_storage) = ctx.unique_index(
            lane.names.iter().map(|name| (name.value.as_str(), name)),
            "index SLDPRT object names by text",
        )?;
        Ok(Self {
            by_object,
            by_value,
            _storage: [object_storage, value_storage],
        })
    }

    /// The lane name that serializes `feature`'s object.
    pub(crate) fn of(
        &self,
        ctx: &DecodeContext<'_>,
        feature: &crate::records::Feature,
    ) -> Result<Option<&'lane FeatureInputName>, CodecError> {
        Ok(match self.lookup(ctx, feature)? {
            NameLookup::One(name) => Some(name),
            NameLookup::Absent | NameLookup::Repeated => None,
        })
    }

    /// The lane names that could serialize `feature`'s object: those with its
    /// object id, or, when no name carries that id, those with its text.
    pub(crate) fn lookup(
        &self,
        ctx: &DecodeContext<'_>,
        feature: &crate::records::Feature,
    ) -> Result<NameLookup<'lane>, CodecError> {
        if let Some(source_id) = feature.source_value() {
            if let Some(name) =
                ctx.get_hash_map(&self.by_object, &source_id, "find SLDPRT object name by id")?
            {
                return Ok(NameLookup::from_unique(*name));
            }
        }
        Ok(
            match ctx.get_hash_map(
                &self.by_value,
                feature.name.as_str(),
                "find SLDPRT object name by text",
            )? {
                Some(name) => NameLookup::from_unique(*name),
                None => NameLookup::Absent,
            },
        )
    }
}

/// How many lane names could serialize one feature's object.
pub(crate) enum NameLookup<'lane> {
    Absent,
    One(&'lane FeatureInputName),
    Repeated,
}

impl<'lane> NameLookup<'lane> {
    fn from_unique(name: Option<&'lane FeatureInputName>) -> Self {
        name.map_or(Self::Repeated, Self::One)
    }
}

pub(crate) fn feature_object_name<'a>(
    feature: &crate::records::Feature,
    lane: &'a FeatureInputLane,
) -> Option<&'a FeatureInputName> {
    if let Some(source_id) = feature.source_value() {
        let mut matches = lane
            .names
            .iter()
            .filter(|name| name.object_id.and_then(ObjectId::value) == Some(source_id));
        if let Some(first) = matches.next() {
            if matches.next().is_none() {
                return Some(first);
            }
            return None;
        }
    }
    let mut matches = lane.names.iter().filter(|name| name.value == feature.name);
    let first = matches.next()?;
    matches.next().is_none().then_some(first)
}

#[cfg(test)]
mod scalars_tests;
