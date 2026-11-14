use super::shared_frames::{exact_fixed_scalar, extrude_operation_at, marked_record_reference};
use crate::design::decode::sketch::IndexedRecordOffsets;
use crate::ids::native_stream;
use crate::layout::{
    sphere_compact_prefix as compact, sphere_shifted_prefix as shifted,
    sphere_typed_prefix as typed,
};
use crate::records::feature::primitives::DesignSpherePrimitive;
use crate::records::feature::scope::{DesignFeatureKind, DesignParameterScope};
use crate::records::parameters::{DesignParameter, DesignParameterOwner};
use crate::records::sketch_placement::SketchPlacementMatrix;
use cadmpeg_core::decode::{u64_from_index, View};
use cadmpeg_ir::scalar::PositiveReal;

pub(super) fn decode(
    bytes: &[u8],
    records: &IndexedRecordOffsets,
    scope: &DesignParameterScope,
    owners: &[DesignParameterOwner],
    parameters: &[DesignParameter],
) -> Option<DesignSpherePrimitive> {
    if scope.kind() != DesignFeatureKind::SpherePrimitive || scope.frame_length() == 462 {
        return None;
    }
    let start = usize::try_from(scope.byte_offset()).ok()?;
    let end = usize::try_from(scope.paired_byte_offset()).ok()?;
    let frame = View::over_retained(bytes).child(start, end)?;
    let data = frame.window();
    let members: Vec<u32> = scope.reference_members().values().copied().collect();
    let suffix = members.get(members.len().checked_sub(3)?..)?;
    let mut forms = [
        (compact::OPERATION, compact::PLACEMENT, false),
        (shifted::OPERATION, shifted::PLACEMENT, false),
        (typed::OPERATION, typed::PLACEMENT, true),
    ]
    .into_iter()
    .filter_map(|(operation_at, reference_at, is_typed)| {
        let operation = extrude_operation_at(data, operation_at)?;
        (data.get(operation_at + 4) == Some(&1)).then_some(())?;
        let reference = |ordinal: usize| {
            if is_typed {
                let (target, guid) =
                    crate::bytes::typed_local_reference(data, reference_at + ordinal * 51)?;
                let expected = [
                    "7B6C3C2D-4096-4E6D-B225-699E8C2D9355",
                    "D91D429C-1BB5-4640-B49F-69AF14F463E9",
                    "B556C40D-B680-4BD8-9623-028CD43307EC",
                ][ordinal];
                (guid == expected).then_some(target)
            } else {
                marked_record_reference(data, reference_at + ordinal * 11)
            }
        };
        let targets = [reference(0)?, reference(1)?, reference(2)?];
        (targets == suffix
            && targets[0] != targets[1]
            && targets[0] != targets[2]
            && targets[1] != targets[2])
            .then_some((operation, operation_at, targets))
    });
    let (operation, operation_at, [placement, diameter_record_index, _]) = forms.next()?;
    if forms.next().is_some() {
        return None;
    }
    let stream = native_stream(&scope.id)?;
    let mut candidates = owners.iter().filter(|owner| {
        owner.record_index() == diameter_record_index && native_stream(owner.id()) == Some(stream)
    });
    let owner = candidates.next();
    if candidates.next().is_some() {
        return None;
    }
    let (value, diameter_offset, parameter_index, diameter_owner_assumed) = match owner {
        Some(owner) => (
            owner.evaluated_value().get(),
            owner.evaluated_value_offset(),
            Some(owner.parameter_record_index()),
            owner.scope_record_index() != scope.record_index || owner.local_ordinal() != 0,
        ),
        None => {
            let mut raw = records
                .frames(diameter_record_index)
                .filter_map(|(start, end)| {
                    crate::design::decode::parameters::parse_parameter_owner(
                        bytes.get(start..end)?,
                    )?
                    .into_record(stream, u64_from_index(start))
                });
            match (raw.next(), raw.next()) {
                (Some(owner), None) => (
                    owner.evaluated_value().get(),
                    owner.evaluated_value_offset(),
                    Some(owner.parameter_record_index()),
                    true,
                ),
                (None, None) => {
                    let scalar = exact_fixed_scalar(bytes, records, diameter_record_index)?;
                    (scalar.value.get(), scalar.value_offset, None, true)
                }
                _ => return None,
            }
        }
    };
    let diameter_source_assumed = parameter_index.is_none_or(|index| {
        parameters
            .iter()
            .filter(|parameter| {
                parameter.record_index == index && native_stream(&parameter.id) == Some(stream)
            })
            .count()
            != 1
    });
    Some(DesignSpherePrimitive {
        placement_record_index: Some(placement),
        diameter_owner_assumed,
        diameter_source_assumed,
        transform: SketchPlacementMatrix::IDENTITY,
        transform_offset: u64_from_index(start + operation_at),
        diameter: PositiveReal::new(value)?,
        diameter_record_index,
        diameter_offset,
        operation,
        operation_offset: u64_from_index(start + operation_at),
    })
}

#[cfg(test)]
mod tests;
