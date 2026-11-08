// SPDX-License-Identifier: Apache-2.0
//! Local geometry from source-selected outer `DataBloc` fields.

use cadmpeg_core::decode::View;
use cadmpeg_ir::math::{Point2, Point3, Vector3};
use cadmpeg_ir::topology::IncreasingParameterInterval;

use crate::checked::{ExactDeviation, ExactHypotUnitVector3, UnitFrame3, UnitVector2};
use crate::native::entity_record::{CatiaEntityRecord, CatiaEntityRecordBody};

pub(super) fn chart(
    entity: &CatiaEntityRecord,
    data_bloc_index: u32,
) -> Option<(Point3, Vector3, Vector3)> {
    let values = selected_values::<9>(entity, data_bloc_index, 0x07e0, 10)?;
    let horizontal = ExactHypotUnitVector3::new([values[0], values[1], values[2]])?;
    let vertical = ExactHypotUnitVector3::new([values[3], values[4], values[5]])?;
    let normal = Vector3::from(horizontal.get()).cross(Vector3::from(vertical.get()));
    let normal = ExactHypotUnitVector3::new([normal.x, normal.y, normal.z])?;
    UnitFrame3::right_handed(normal, horizontal, vertical)?;
    Some((
        Point3::new(values[6], values[7], values[8]),
        Vector3::from(horizontal.get()),
        Vector3::from(vertical.get()),
    ))
}

pub(super) fn point(entity: &CatiaEntityRecord, data_bloc_index: u32) -> Option<Point2> {
    let values = selected_values::<2>(entity, data_bloc_index, 0x07e4, 3)?;
    Some(Point2::new(values[0], values[1]))
}

pub(super) fn line(
    entity: &CatiaEntityRecord,
    data_bloc_index: u32,
) -> Option<(Point2, Point2, [f64; 2])> {
    let values = selected_values::<7>(entity, data_bloc_index, 0x07e4, 8)?;
    UnitVector2::<ExactDeviation>::from_hypot([values[2], values[3]])?;
    let interval = IncreasingParameterInterval::new([values[4], values[5]])?;
    (values[6] == 1.0).then_some((
        Point2::new(values[0], values[1]),
        Point2::new(values[2], values[3]),
        interval.endpoints(),
    ))
}

pub(super) fn integer(entity: &CatiaEntityRecord, schema_index: u32) -> Option<i32> {
    selected(entity, schema_index, |payload| {
        value_header(payload, 0x1cc0)?;
        let value = match payload.u8()? {
            0x80 => payload.i32_le()?,
            code @ 0x81..=0xd0 => i32::from(code - 0x81),
            _ => return None,
        };
        (payload.u8()? == 0xfe).then_some(value)
    })
    .ok()?
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) struct MalformedField;

pub(super) fn flags(
    entity: &CatiaEntityRecord,
    schema_index: u32,
) -> Result<Option<u8>, MalformedField> {
    selected(entity, schema_index, |payload| {
        value_header(payload, 0x0ae0)?;
        if payload.array::<2>()? != [0x82, 0x81] {
            return None;
        }
        let value = payload.u8()?;
        (payload.u8()? == 0xfe).then_some(value)
    })
}

fn selected_values<const N: usize>(
    entity: &CatiaEntityRecord,
    data_bloc_index: u32,
    value_type: u16,
    layout: u8,
) -> Option<[f64; N]> {
    selected(entity, data_bloc_index, |payload| {
        packet::<N>(payload, value_type, layout)
    })
    .ok()?
}

fn selected<T>(
    entity: &CatiaEntityRecord,
    schema_index: u32,
    parse: impl Fn(&mut View<'_>) -> Option<T>,
) -> Result<Option<T>, MalformedField> {
    let CatiaEntityRecordBody::Nested { value_payload, .. } = &entity.body else {
        return Err(MalformedField);
    };
    let mut payload = View::over_retained(value_payload);
    let mut field_start = true;
    let mut selected = None;
    while !payload.is_empty() {
        let code = payload.u8().ok_or(MalformedField)?;
        match code {
            0xe5 => {
                let count = payload.u32_le().ok_or(MalformedField)?;
                let count = payload
                    .counted(u64::from(count), 1)
                    .ok_or(MalformedField)?
                    .get();
                payload.skip(count).ok_or(MalformedField)?;
                field_start = false;
                continue;
            }
            0xe6 => {
                payload.skip(8).ok_or(MalformedField)?;
                field_start = false;
                continue;
            }
            0x8e => {
                let mut inline = payload;
                if let Some([size, 0x84]) = inline.array::<2>() {
                    if (0xe8..=0xef).contains(&size) {
                        inline
                            .skip(usize::from(size - 0xe7))
                            .ok_or(MalformedField)?;
                        payload = inline;
                        field_start = false;
                        continue;
                    }
                }
            }
            _ => {}
        }
        if field_start {
            let index = if code == 0x32 {
                payload.u32_le().ok_or(MalformedField)?.checked_sub(1)
            } else if (0x34..=0xe4).contains(&code) {
                Some(u32::from(code - 0x34))
            } else {
                None
            };
            if index == Some(schema_index) {
                compact_counter(&mut payload).ok_or(MalformedField)?;
                let values = parse(&mut payload).ok_or(MalformedField)?;
                if selected.replace(values).is_some() {
                    return Err(MalformedField);
                }
                field_start = true;
                continue;
            }
            if index.is_some() {
                field_start = false;
                continue;
            }
        }
        match code {
            0x32 | 0x80 => payload.skip(4).ok_or(MalformedField)?,
            0xd1..=0xe4 => payload.skip(1).ok_or(MalformedField)?,
            _ => {}
        }
        field_start = code == 0xfe;
    }
    Ok(selected)
}

fn compact_counter(payload: &mut View<'_>) -> Option<()> {
    match payload.u8()? {
        0x80 => payload.skip(4)?,
        0x81..=0xd0 => {}
        0xd1..=0xe4 => payload.skip(1)?,
        _ => return None,
    }
    Some(())
}

fn packet<const N: usize>(payload: &mut View<'_>, value_type: u16, layout: u8) -> Option<[f64; N]> {
    value_header(payload, value_type)?;
    if payload.u8()? != 0x80 + layout || payload.u8()? != 0x81 {
        return None;
    }
    let mut values = [0.0; N];
    for value in &mut values {
        *value = match payload.u8()? {
            0xe6 => payload.f64_le()?,
            0xe7 => 1.0,
            0xe8 => 0.0,
            0xe9 => -1.0,
            _ => return None,
        };
        if !value.is_finite() {
            return None;
        }
    }
    (payload.u8()? == 0xfe).then_some(values)
}

fn value_header(payload: &mut View<'_>, value_type: u16) -> Option<()> {
    (payload.u8()? == 0xe8 && payload.u16_le()? == value_type && payload.u8()? == 0x37)
        .then_some(())
}

#[cfg(test)]
mod tests;
