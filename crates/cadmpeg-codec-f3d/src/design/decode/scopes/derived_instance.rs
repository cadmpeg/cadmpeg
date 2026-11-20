// SPDX-License-Identifier: Apache-2.0

use cadmpeg_core::decode::{bounded_len, u64_from_index, DecodeContext, View};
use cadmpeg_core::CodecError;

use crate::design::decode::sketch::{next_indexed_record_offset, IndexedRecordOffsets};
use crate::layout::{
    derived_instance_count_prefix as count_prefix, derived_instance_guid_trailer as guid_trailer,
    derived_instance_matrix as matrix_layout, derived_instance_prefix as prefix_layout,
    derived_instance_reference as reference_layout, derived_instance_reference_pair as pair_layout,
    derived_instance_source_entry_prefix as entry_layout,
    derived_instance_suffix_prefix as suffix_layout, indexed_design_record_header as header_layout,
};
use crate::records::feature::assembly_features::{
    DesignComponentOccurrence, DesignDerivedInstanceAuxiliaryPair, DesignDerivedInstanceCarrier,
    DesignDerivedInstanceConstruction, DesignDerivedInstancePlacement,
    DesignDerivedInstanceReference, DesignDerivedInstanceRelation, DesignDerivedInstanceSource,
    DesignDerivedInstanceSourcePlacement, DERIVED_INSTANCE_RELATION_TYPE_GUID,
    OCCURRENCE_PLACEMENT_TYPE_GUID,
};
use crate::records::feature::scope::{DesignFeatureKind, DesignParameterScope};
use crate::records::mesh::{DesignGuidText, DesignRelaxedGuidText};

const SCOPE_TYPE_GUID: &str = "66E375EA-40F1-4115-8D83-0F23DD30593E";

enum ReadError {
    Form,
    Codec(CodecError),
}

impl From<CodecError> for ReadError {
    fn from(error: CodecError) -> Self {
        Self::Codec(error)
    }
}

type ReadResult<T> = Result<T, ReadError>;

pub(super) fn construction(
    ctx: &DecodeContext<'_>,
    bytes: &[u8],
    records: &IndexedRecordOffsets,
    scope: &DesignParameterScope,
    occurrences: &[DesignComponentOccurrence],
) -> Result<Option<DesignDerivedInstanceConstruction>, CodecError> {
    if scope.kind() != DesignFeatureKind::DerivedInstance {
        return Ok(None);
    }
    match read_construction(ctx, bytes, records, scope, occurrences) {
        Ok(value) => Ok(Some(value)),
        Err(ReadError::Form) => Ok(None),
        Err(ReadError::Codec(error)) => Err(error),
    }
}

fn read_construction(
    ctx: &DecodeContext<'_>,
    bytes: &[u8],
    records: &IndexedRecordOffsets,
    scope: &DesignParameterScope,
    occurrences: &[DesignComponentOccurrence],
) -> ReadResult<DesignDerivedInstanceConstruction> {
    let origin = usize::try_from(scope.byte_offset()).map_err(|_| ReadError::Form)?;
    let end = usize::try_from(scope.paired_byte_offset()).map_err(|_| ReadError::Form)?;
    let reference_at = usize::try_from(
        scope
            .reference_count_offset()
            .checked_sub(scope.byte_offset())
            .ok_or(ReadError::Form)?,
    )
    .map_err(|_| ReadError::Form)?;
    let frame = bytes.get(origin..end).ok_or(ReadError::Form)?;
    let prefix = frame.get(..reference_at).ok_or(ReadError::Form)?;
    if prefix.get(header_layout::LEN..prefix_layout::REFERENCE_MARKER) != Some(&[0; 10]) {
        return Err(ReadError::Form);
    }
    let mut at = prefix_layout::REFERENCE_MARKER;
    let reference = read_reference(ctx, prefix, &mut at)?;
    let count = read_count(prefix, &mut at, entry_layout::LEN)?;
    let mut placements = members(
        ctx,
        count,
        DesignDerivedInstanceSourcePlacement {
            reference: DesignDerivedInstanceReference::plain(0),
            placement: DesignDerivedInstancePlacement::Identity { flag_offset: 0 },
        },
        "F3D derived source placements",
    )?;
    for entry in &mut placements {
        entry.reference = read_reference(ctx, prefix, &mut at)?;
        entry.placement = read_placement(prefix, origin, &mut at)?;
    }
    let count = read_count(prefix, &mut at, pair_layout::LEN)?;
    let mut auxiliary_pairs = members(
        ctx,
        count,
        DesignDerivedInstanceAuxiliaryPair {
            auxiliary: DesignDerivedInstanceReference::plain(0),
            relation: DesignDerivedInstanceReference::plain(0),
        },
        "F3D derived auxiliary pairs",
    )?;
    for pair in &mut auxiliary_pairs {
        pair.auxiliary = read_reference(ctx, prefix, &mut at)?;
        pair.relation = read_reference(ctx, prefix, &mut at)?;
    }
    let count = read_count(prefix, &mut at, reference_layout::LEN)?;
    if count != auxiliary_pairs.len() {
        return Err(ReadError::Form);
    }
    let mut repeated_references = members(
        ctx,
        count,
        DesignDerivedInstanceReference::plain(0),
        "F3D derived repeated relations",
    )?;
    for entry in &mut repeated_references {
        *entry = read_reference(ctx, prefix, &mut at)?;
    }
    let mut pair_targets = members(ctx, count, 0u64, "F3D derived auxiliary membership")?;
    let mut repeat_targets = members(ctx, count, 0u64, "F3D derived repeated membership")?;
    for ((pair_target, repeat_target), (pair, repeat)) in pair_targets
        .iter_mut()
        .zip(&mut repeat_targets)
        .zip(auxiliary_pairs.iter().zip(&repeated_references))
    {
        *pair_target = pair.relation.target;
        *repeat_target = repeat.target;
    }
    pair_targets.sort_unstable();
    repeat_targets.sort_unstable();
    if pair_targets != repeat_targets {
        return Err(ReadError::Form);
    }
    if take(prefix, &mut at, suffix_layout::PRIMARY_RELATION_REFERENCE)? != [0; 4] {
        return Err(ReadError::Form);
    }
    let primary_reference = read_reference(ctx, prefix, &mut at)?;
    if take(prefix, &mut at, 1)? != [suffix_layout::CONTEXT_MARKER_VALUE] {
        return Err(ReadError::Form);
    }
    let context_placement = read_placement(prefix, origin, &mut at)?;
    let guid_at = at;
    let (guid, guid_end) =
        crate::bytes::lp_utf16_bounded(prefix, guid_at + guid_trailer::GUID, 36..=36)
            .ok_or(ReadError::Form)?;
    ctx.retain_bytes(u64_from_index(guid.len()), "F3D derived trailer GUID")?;
    let trailer_guid = DesignRelaxedGuidText::try_from(guid).map_err(|_| ReadError::Form)?;
    if guid_at.checked_add(guid_trailer::LEN) != Some(prefix.len())
        || prefix.get(guid_end..) != Some(&[0; 3])
    {
        return Err(ReadError::Form);
    }
    let mut at = reference_at;
    let count = read_count(frame, &mut at, reference_layout::LEN)?;
    if count != scope.reference_members().len() {
        return Err(ReadError::Form);
    }
    let mut sources = members(
        ctx,
        count,
        DesignDerivedInstanceSource {
            reference: DesignDerivedInstanceReference::plain(0),
            relation: None,
        },
        "F3D derived source references",
    )?;
    let mut targets = members(ctx, count, 0u64, "F3D derived reference membership")?;
    for ((source, target), expected) in sources
        .iter_mut()
        .zip(&mut targets)
        .zip(scope.reference_members().values())
    {
        source.reference = read_reference(ctx, frame, &mut at)?;
        if source.reference.target != u64::from(*expected) {
            return Err(ReadError::Form);
        }
        *target = source.reference.target;
        if source
            .reference
            .matches_type(DERIVED_INSTANCE_RELATION_TYPE_GUID)
        {
            source.relation = read_relation(ctx, bytes, records, scope, *expected, occurrences)?;
        }
    }
    targets.sort_unstable();
    targets.dedup();
    if !targets.contains(&primary_reference.target)
        || placements
            .iter()
            .any(|entry| targets.binary_search(&entry.reference.target).is_err())
        || auxiliary_pairs
            .iter()
            .any(|pair| targets.binary_search(&pair.relation.target).is_err())
        || repeated_references
            .iter()
            .any(|reference| targets.binary_search(&reference.target).is_err())
    {
        return Err(ReadError::Form);
    }
    Ok(DesignDerivedInstanceConstruction {
        reference,
        placements,
        auxiliary_pairs,
        repeated_references,
        primary_reference,
        context_placement,
        trailer_guid,
        sources,
    })
}

fn members<T: Clone>(
    ctx: &DecodeContext<'_>,
    count: usize,
    placeholder: T,
    operation: &'static str,
) -> Result<Vec<T>, CodecError> {
    ctx.retain_bytes(
        u64_from_index(count)
            .checked_mul(u64_from_index(std::mem::size_of::<T>()))
            .ok_or_else(|| CodecError::malformed("F3D derived member storage overflows"))?,
        operation,
    )?;
    ctx.alloc_filled(count, placeholder, operation)
}

fn take<'a>(bytes: &'a [u8], at: &mut usize, count: usize) -> ReadResult<&'a [u8]> {
    let end = at.checked_add(count).ok_or(ReadError::Form)?;
    let value = bytes.get(*at..end).ok_or(ReadError::Form)?;
    *at = end;
    Ok(value)
}

fn read_count(bytes: &[u8], at: &mut usize, minimum: usize) -> ReadResult<usize> {
    let count = View::u32_le_at(bytes, *at).ok_or(ReadError::Form)?;
    take(bytes, at, count_prefix::LEN)?;
    bounded_len(
        u64::from(count),
        minimum,
        bytes.len().checked_sub(*at).ok_or(ReadError::Form)?,
    )
    .ok_or(ReadError::Form)
}

fn read_reference(
    ctx: &DecodeContext<'_>,
    bytes: &[u8],
    at: &mut usize,
) -> ReadResult<DesignDerivedInstanceReference> {
    let mut end = *at;
    let reference = crate::bytes::take_reference(bytes, &mut end).ok_or(ReadError::Form)?;
    let (target, type_guid) = reference.local().ok_or(ReadError::Form)?;
    let type_guid = match type_guid {
        None => None,
        Some(guid) => {
            ctx.retain_bytes(
                u64_from_index(guid.len()),
                "F3D derived reference type GUID",
            )?;
            let mut owned = ctx.alloc_filled(guid.len(), 0u8, "F3D derived reference type GUID")?;
            owned.copy_from_slice(guid.as_bytes());
            let text = String::from_utf8(owned).map_err(|_| ReadError::Form)?;
            Some(DesignGuidText::try_from(text).map_err(|_| ReadError::Form)?)
        }
    };
    *at = end;
    Ok(DesignDerivedInstanceReference { target, type_guid })
}

fn read_placement(
    bytes: &[u8],
    origin: usize,
    at: &mut usize,
) -> ReadResult<DesignDerivedInstancePlacement> {
    let flag_offset = u64_from_index(origin.checked_add(*at).ok_or(ReadError::Form)?);
    let flag = take(bytes, at, 1)?;
    match flag {
        [1] => Ok(DesignDerivedInstancePlacement::Identity { flag_offset }),
        [0] => {
            let offset = u64_from_index(origin.checked_add(*at).ok_or(ReadError::Form)?);
            let scalars = crate::bytes::f64s_at::<16>(bytes, *at).ok_or(ReadError::Form)?;
            take(bytes, at, matrix_layout::LEN)?;
            let mut value = [[cadmpeg_ir::scalar::FiniteReal::ZERO; 4]; 4];
            for (target, scalar) in value.iter_mut().flatten().zip(scalars) {
                *target = cadmpeg_ir::scalar::FiniteReal::try_from(scalar)
                    .map_err(|_| ReadError::Form)?;
            }
            Ok(DesignDerivedInstancePlacement::Matrix { value, offset })
        }
        _ => Err(ReadError::Form),
    }
}

fn read_relation(
    ctx: &DecodeContext<'_>,
    bytes: &[u8],
    records: &IndexedRecordOffsets,
    scope: &DesignParameterScope,
    record_index: u32,
    occurrences: &[DesignComponentOccurrence],
) -> ReadResult<Option<DesignDerivedInstanceRelation>> {
    let mut found = None;
    for origin in records.offsets(record_index) {
        let Some(end) = origin
            .checked_add(1)
            .and_then(|at| next_indexed_record_offset(bytes, at))
        else {
            continue;
        };
        let Some(frame) = bytes.get(*origin..end) else {
            continue;
        };
        match relation_frame(ctx, frame, scope.record_index) {
            Ok(value) => {
                if found.is_some() {
                    return Ok(None);
                }
                found = Some(value);
            }
            Err(ReadError::Form) => {}
            Err(error) => return Err(error),
        }
    }
    let Some(mut relation) = found else {
        return Ok(None);
    };
    if !relation
        .carrier_reference
        .matches_type(OCCURRENCE_PLACEMENT_TYPE_GUID)
    {
        return Ok(Some(relation));
    }
    let Some(stream) = crate::ids::native_stream(&scope.id) else {
        return Ok(Some(relation));
    };
    let mut candidates = occurrences.iter().filter(|candidate| {
        crate::ids::native_stream(&candidate.id) == Some(stream)
            && u64::from(candidate.record_index) == relation.carrier_reference.target
    });
    let Some(carrier) = candidates.next() else {
        return Ok(Some(relation));
    };
    if candidates.next().is_some() {
        return Ok(Some(relation));
    }
    ctx.retain_bytes(
        u64_from_index(carrier.component_guid.as_str().len())
            .checked_add(u64_from_index(carrier.occurrence_guid.as_str().len()))
            .ok_or_else(|| {
                CodecError::malformed("F3D derived carrier identity storage overflows")
            })?,
        "F3D derived carrier identities",
    )?;
    relation.carrier = Some(DesignDerivedInstanceCarrier {
        component_record_index: carrier.component_record_index,
        component_guid: carrier.component_guid.clone(),
        occurrence_guid: carrier.occurrence_guid.clone(),
        transform: carrier.transform().map_or(
            crate::records::sketch_placement::SketchPlacementMatrix::IDENTITY,
            |matrix| matrix.value,
        ),
    });
    Ok(Some(relation))
}

fn relation_frame(
    ctx: &DecodeContext<'_>,
    frame: &[u8],
    scope_record_index: u32,
) -> ReadResult<DesignDerivedInstanceRelation> {
    if frame.get(header_layout::LEN..prefix_layout::REFERENCE_MARKER) != Some(&[0; 10]) {
        return Err(ReadError::Form);
    }
    let mut at = prefix_layout::REFERENCE_MARKER;
    let carrier_reference = read_reference(ctx, frame, &mut at)?;
    match frame.get(at) {
        Some(0) => {
            if take(frame, &mut at, 2)? != [0; 2] {
                return Err(ReadError::Form);
            }
        }
        Some(1) => {
            if take(frame, &mut at, 3)? != [1, 0, 0] {
                return Err(ReadError::Form);
            }
        }
        _ => return Err(ReadError::Form),
    }
    let middle_reference = read_reference(ctx, frame, &mut at)?;
    if take(frame, &mut at, 1)? != [0] {
        return Err(ReadError::Form);
    }
    let scope_reference = read_reference(ctx, frame, &mut at)?;
    if at != frame.len()
        || scope_reference.target != u64::from(scope_record_index)
        || !scope_reference.matches_type(SCOPE_TYPE_GUID)
    {
        return Err(ReadError::Form);
    }
    Ok(DesignDerivedInstanceRelation {
        carrier_reference,
        middle_reference,
        scope_reference,
        carrier: None,
    })
}
