use super::{registered_version, scope_member_matches, Reader, WorkAxisRegistrations};
use crate::bytes::reference_fields::ReadField;
use crate::design::decode::scopes::shared_frames::exact_indexed_header_at;
use crate::design::decode::sketch::IndexedRecordOffsets;
use crate::layout::work_axis_direct_carrier_class_297 as direct_297;
use crate::layout::work_axis_direct_carrier_class_335 as direct_335;
use crate::layout::work_axis_direct_scope_long as scope_long;
use crate::layout::work_axis_direct_scope_short as scope_short;
use crate::layout::work_axis_direct_support as support_layout;
use crate::layout::work_axis_endpoint_carrier as endpoint;
use crate::layout::work_axis_two_point_carrier as two_point;
use crate::records::feature::scope::DesignParameterScope;
use crate::records::feature::work_axes::{
    AxisEndpoint, AxisEndpointReading, DesignWorkAxisCandidate, DesignWorkAxisSource,
};
use crate::records::feature::work_geometry::POINT_DIRECTION_TYPE_GUID;
use cadmpeg_core::decode::{u64_from_index, DecodeContext, View};
use cadmpeg_core::CodecError;
use cadmpeg_ir::unknown::part::GivenUpPart;

fn frame(records: &IndexedRecordOffsets, index: u32) -> Option<(usize, usize)> {
    let mut frames = records.frames(index);
    let (Some(frame), None) = (frames.next(), frames.next()) else {
        return None;
    };
    Some(frame)
}

pub(super) fn decode(
    ctx: &DecodeContext<'_>,
    source: View<'_>,
    records: &IndexedRecordOffsets,
    scope: &DesignParameterScope,
    stream: &str,
    registrations: WorkAxisRegistrations<'_>,
    given_up: &mut Vec<GivenUpPart>,
) -> Result<Option<DesignWorkAxisCandidate>, CodecError> {
    let bytes = source.window();
    let registered = |index: u32| {
        registrations
            .entities
            .get(&u64::from(index))
            .is_some_and(|(guid, version)| {
                guid.eq_ignore_ascii_case(POINT_DIRECTION_TYPE_GUID) && matches!(version, 1 | 4)
            })
    };
    let modern_version = |index: u32, start| {
        registrations
            .entities
            .get(&u64::from(index))
            .filter(|(guid, _)| guid.eq_ignore_ascii_case(POINT_DIRECTION_TYPE_GUID))
            .map(|(_, version)| *version)
            .or_else(|| registered_version(source, start, POINT_DIRECTION_TYPE_GUID, registrations))
            .map_or(ReadField::Absent, ReadField::Value)
    };
    if let Some([axis, _, first, _, second]) = scope.reference_members().values_array::<5>() {
        if let Some((start, end)) =
            frame(records, *axis)
                .filter(|_| !registered(*axis))
                .filter(|(start, end)| {
                    end - start == two_point::LEN
                        && bytes.get(start + two_point::ZERO_RUN..start + two_point::VALUE_COUNT)
                            == Some(&[0; 10])
                })
        {
            if !scope_member_matches(source, scope, 0, *axis, start, registrations) {
                return Ok(None);
            }
            let mut reader = Reader::new(source, start, end)?;
            reader
                .view
                .seek(source.start() + start + two_point::VALUE_COUNT)
                .ok_or_else(|| CodecError::malformed("axis value count outside frame"))?;
            let value_count = reader.read(View::req_u32_le);
            let mut candidate = DesignWorkAxisCandidate {
                record_index: Some(*axis),
                version: modern_version(*axis, start),
                origin: reader.reals(),
                displacement: reader.reals(),
                ..Default::default()
            };
            let reserved = reader.reals();
            reader
                .view
                .seek(source.start() + start + two_point::REFERENCE_COUNT)
                .ok_or_else(|| CodecError::malformed("axis reference count outside frame"))?;
            let inputs = reader.references(ctx)?;
            candidate.source = Some(DesignWorkAxisSource::TwoPoint {
                value_count,
                reserved,
                inputs,
            });
            candidate.complete = reader.failures.is_empty();
            reader.commit(ctx, scope, stream, given_up)?;
            for target in [*first, *second] {
                let mut evidence = AxisEndpoint {
                    record_index: target,
                    unique_frame: frame(records, target).is_some(),
                    ..Default::default()
                };
                for (start, end) in records.frames(target).filter(|(start, end)| {
                    end - start == endpoint::LEN
                        && bytes.get(start + endpoint::ZERO_RUN..start + endpoint::POSITION)
                            == Some(&[0; 31])
                }) {
                    let mut reader = Reader::new(source, start, end)?;
                    reader
                        .view
                        .seek(source.start() + start + endpoint::POSITION)
                        .ok_or_else(|| CodecError::malformed("axis endpoint outside frame"))?;
                    let mut reading = AxisEndpointReading {
                        version: ReadField::Absent,
                        position: reader.reals(),
                        ..Default::default()
                    };
                    reading.complete = reader.failures.is_empty();
                    reader.commit(ctx, scope, stream, given_up)?;
                    evidence.readings.push(reading);
                }
                candidate.endpoints.push(evidence);
            }
            return Ok(Some(candidate));
        }
    }
    let Some([carrier, support]) = scope.reference_members().values_array::<2>() else {
        return Ok(None);
    };
    if registered(*carrier) {
        return Ok(None);
    }
    let Some((start, end)) = frame(records, *carrier) else {
        return Ok(None);
    };
    let Some((support_start, support_end)) = frame(records, *support) else {
        return Ok(None);
    };
    if ![
        (scope_long::LEN, direct_297::LEN),
        (scope_short::LEN, direct_335::LEN),
    ]
    .into_iter()
    .any(|(scope_length, carrier_length)| {
        scope.frame_length() == u64_from_index(scope_length) && end - start == carrier_length
    }) || exact_indexed_header_at(bytes, start, *carrier).is_none()
        || exact_indexed_header_at(bytes, end, *carrier).is_none()
        || support_end - support_start != support_layout::LEN
        || exact_indexed_header_at(bytes, support_start, *support).is_none()
        || exact_indexed_header_at(bytes, support_end, *support).is_none()
        || bytes.get(start + direct_297::ZERO_RUN..start + direct_297::VALUE_COUNT)
            != Some(&[0; 10])
    {
        return Ok(None);
    }
    if !scope_member_matches(source, scope, 0, *carrier, start, registrations) {
        return Ok(None);
    }
    let mut reader = Reader::new(source, start, end)?;
    reader
        .view
        .seek(source.start() + start + direct_297::VALUE_COUNT)
        .ok_or_else(|| CodecError::malformed("direct axis value count outside frame"))?;
    let value_count = reader.read(View::req_u32_le);
    let mut candidate = DesignWorkAxisCandidate {
        record_index: Some(*carrier),
        version: modern_version(*carrier, start),
        origin: reader.reals(),
        displacement: reader.reals(),
        ..Default::default()
    };
    let reserved = reader.reals();
    let reference_count = reader.read(View::req_u32_le);
    let reference_preamble = reader.read(View::req_u32_le);
    candidate.source = Some(DesignWorkAxisSource::DirectCarrier {
        value_count,
        reserved,
        reference_count,
        reference_preamble,
        support_record_index: *support,
    });
    candidate.complete = reader.failures.is_empty();
    reader.commit(ctx, scope, stream, given_up)?;
    Ok(Some(candidate))
}
