// SPDX-License-Identifier: Apache-2.0
//! Owned model and unit-system scalar heaps.

use cadmpeg_core::bytes::find_from;
use cadmpeg_core::decode::{DecodeContext, View};
use cadmpeg_core::CodecError;

use crate::layout::model_double_xar_control as control;
use crate::layout::model_double_xar_imperial_scale as imperial;
use crate::layout::model_double_xar_metric_scale as metric;

#[derive(Debug, Clone, PartialEq)]
pub(crate) struct DoubleXarTable {
    pub(crate) offset: usize,
    pub(crate) body_offset: usize,
    pub(crate) logical_count: usize,
    pub(crate) entries: Vec<DoubleXarSlot>,
}

#[derive(Debug, Clone, PartialEq)]
pub(crate) enum DoubleXarSlot {
    StockZero,
    StockOne,
    Literal { value: f64, raw: Vec<u8> },
    ModelScale { raw: Vec<u8> },
    ModelBound { value: Option<f64>, raw: Vec<u8> },
    RepeatTwo { value: Option<f64> },
    ModelControl,
}

impl DoubleXarSlot {
    pub(crate) fn raw(&self) -> &[u8] {
        match self {
            Self::StockZero => &[0x0b],
            Self::StockOne => &[0x10],
            Self::Literal { raw, .. } | Self::ModelScale { raw } | Self::ModelBound { raw, .. } => {
                raw
            }
            Self::RepeatTwo { .. } => &[0xe5],
            Self::ModelControl => &control::IMAGE_VALUE,
        }
    }

    pub(crate) fn value(&self) -> Option<f64> {
        match self {
            Self::StockZero => Some(0.0),
            Self::StockOne => Some(1.0),
            Self::Literal { value, .. } => Some(*value),
            Self::RepeatTwo { value } | Self::ModelBound { value, .. } => *value,
            Self::ModelScale { .. } | Self::ModelControl => None,
        }
    }

    pub(crate) fn logical_width(&self) -> usize {
        match self {
            Self::RepeatTwo { .. } => 2,
            Self::ModelControl => 0,
            _ => 1,
        }
    }

    pub(crate) fn kind(&self) -> &'static str {
        match self {
            Self::StockZero => "stock_zero",
            Self::StockOne => "stock_one",
            Self::Literal { .. } => "literal",
            Self::ModelScale { .. } => "model_scale",
            Self::ModelBound { .. } => "model_bound",
            Self::RepeatTwo { .. } => "repeat_two",
            Self::ModelControl => "model_control",
        }
    }
}

#[derive(Clone, Copy)]
enum Heap {
    Model,
    Units,
}

const TABLE: &[u8] = b"\xe0\x02double_xar\0\xf8";
const TYPES: &[u8] = b"\xe0\x04type_har\0\xf8";
const STRINGS: &[u8] = b"\xe0\x03string_xar\0";

fn unique_label(data: &[u8], label: &[u8]) -> Option<usize> {
    let offset = find_from(data, label, 0)?;
    find_from(data, label, offset.checked_add(label.len())?)
        .is_none()
        .then_some(offset)
}

fn counted_array<'a>(data: &'a [u8], label: &[u8]) -> Option<(usize, u32, View<'a>)> {
    let offset = unique_label(data, label)?;
    let count_offset = offset.checked_add(label.len())?;
    let (count, begin) = crate::psb::complete_compact_int(data, count_offset)?;
    let mut view = View::over_retained(data);
    view.seek(begin)?;
    Some((offset, count, view))
}

fn type_count(data: &[u8]) -> Option<usize> {
    let (_, declared, view) = counted_array(data, TYPES)?;
    let end = find_from(view.unread(), &[0xe0], 0)?;
    let types = view.child(view.position(), view.position().checked_add(end)?)?;
    let mut logical = 0usize;
    let mut handles = 0usize;
    let mut preceding = None;
    for &token in types.window() {
        let width = match token {
            0xe4..=0xe6 if preceding == Some(b'C') => usize::from(token - 0xe3),
            0xe4..=0xe6 => return None,
            _ => 1,
        };
        logical = logical.checked_add(width)?;
        handles = handles.checked_add(usize::from(token == b'H'))?;
        preceding = Some(token);
    }
    if logical != usize::try_from(declared).ok()? {
        return None;
    }
    Some(handles)
}

fn peek(view: View<'_>) -> Option<u8> {
    let mut copy = view;
    copy.u8()
}

fn raw_copy(ctx: &DecodeContext<'_>, raw: &[u8]) -> Result<Vec<u8>, CodecError> {
    let mut copy = ctx.alloc_filled(raw.len(), 0u8, "creo double_xar token bytes")?;
    copy.copy_from_slice(raw);
    Ok(copy)
}

struct Slots {
    entries: Vec<DoubleXarSlot>,
    physical: usize,
    logical: usize,
    expected: usize,
}

impl Slots {
    fn push(&mut self, slot: DoubleXarSlot) -> Option<()> {
        let logical = self.logical.checked_add(slot.logical_width())?;
        if logical > self.expected {
            return None;
        }
        *self.entries.get_mut(self.physical)? = slot;
        self.physical = self.physical.checked_add(1)?;
        self.logical = logical;
        Some(())
    }

    fn repeat(&mut self) -> Option<()> {
        let previous = self
            .entries
            .get(..self.physical)?
            .iter()
            .rev()
            .find(|s| s.logical_width() != 0)?;
        self.push(DoubleXarSlot::RepeatTwo {
            value: previous.value(),
        })
    }
}

fn unit_slot(
    ctx: &DecodeContext<'_>,
    view: &mut View<'_>,
) -> Result<Option<DoubleXarSlot>, CodecError> {
    let mut probe = *view;
    let Some(head) = probe.u8() else {
        return Ok(None);
    };
    let (width, value) = match head {
        0x07 => (1, Some(0.001)),
        0x0f => (1, Some(1.0)),
        0x18 => (1, Some(0.0)),
        0x2f => (
            3,
            crate::psb::short_form_float(view.unread(), 0).map(|s| s.0),
        ),
        0x28 => (8, crate::scalar::ieee8(view.unread(), 0, 0x3f).map(|s| s.0)),
        0x2d => (8, crate::scalar::ieee8(view.unread(), 0, 0x40).map(|s| s.0)),
        0x68 => (
            7,
            crate::scalar::ieee7_with_prefix(view.unread(), 0, 0x3f, 0xdd).map(|s| s.0),
        ),
        0x6c => (
            7,
            crate::scalar::ieee7_with_prefix(view.unread(), 0, 0x3f, 0xe1).map(|s| s.0),
        ),
        _ => return Ok(None),
    };
    let Some(value) = value.filter(|v| v.is_finite()) else {
        return Ok(None);
    };
    let Some(raw) = view.take(width) else {
        return Ok(None);
    };
    Ok(Some(DoubleXarSlot::Literal {
        value,
        raw: raw_copy(ctx, raw)?,
    }))
}

fn model_slots(
    ctx: &DecodeContext<'_>,
    view: &mut View<'_>,
    slots: &mut Slots,
) -> Result<Option<()>, CodecError> {
    if view.u8() != Some(0x10) || slots.push(DoubleXarSlot::StockOne).is_none() {
        return Ok(None);
    }
    if peek(*view) == Some(0xe5) {
        view.u8();
        if slots.repeat().is_none() {
            return Ok(None);
        }
    }
    let scale = if view.unread().starts_with(&metric::IMAGE_VALUE) {
        view.take(metric::LEN)
    } else if peek(*view) == Some(imperial::OPENER_VALUE) {
        let Some(raw) = view.take(imperial::LEN) else {
            return Ok(None);
        };
        if raw.get(imperial::TRAILER..) != Some(imperial::TRAILER_VALUE.as_slice()) {
            return Ok(None);
        }
        Some(raw)
    } else {
        None
    };
    let Some(scale) = scale else {
        return Ok(None);
    };
    if slots
        .push(DoubleXarSlot::ModelScale {
            raw: raw_copy(ctx, scale)?,
        })
        .is_none()
    {
        return Ok(None);
    }
    if view.u8() != Some(0x0b) || slots.push(DoubleXarSlot::StockZero).is_none() {
        return Ok(None);
    }
    if view.take(control::LEN) != Some(control::IMAGE_VALUE.as_slice())
        || slots.push(DoubleXarSlot::ModelControl).is_none()
    {
        return Ok(None);
    }
    if peek(*view) == Some(0x10) {
        view.u8();
        if slots.push(DoubleXarSlot::StockOne).is_none() {
            return Ok(None);
        }
    }
    let bounding_begin = slots.logical;
    while slots
        .logical
        .checked_sub(bounding_begin)
        .is_some_and(|n| n < 6)
    {
        if peek(*view) == Some(0xe5) {
            if slots.logical == bounding_begin {
                return Ok(None);
            }
            view.u8();
            if slots.repeat().is_none() {
                return Ok(None);
            }
        } else {
            let Some(head) = peek(*view) else {
                return Ok(None);
            };
            let width = match head {
                0x2d | 0x41 | 0x46 => 8,
                0x31 | 0x4a | 0x54 | 0x6d | 0x73 | 0x8a | 0x8b | 0x8c | 0x92 | 0x93 | 0x9a
                | 0x9b | 0x9c | 0x9f | 0xa3 | 0xa5 | 0xa8 | 0xbb | 0xbf | 0xd2 | 0xdf => 7,
                _ => return Ok(None),
            };
            let value = match head {
                0x2d => crate::scalar::ieee8(view.unread(), 0, 0xc0).map(|s| s.0),
                0x46 => crate::scalar::ieee8(view.unread(), 0, 0x40).map(|s| s.0),
                _ => None,
            };
            if value.is_some_and(|v| !v.is_finite()) {
                return Ok(None);
            }
            let Some(raw) = view.take(width) else {
                return Ok(None);
            };
            if slots
                .push(DoubleXarSlot::ModelBound {
                    value,
                    raw: raw_copy(ctx, raw)?,
                })
                .is_none()
            {
                return Ok(None);
            }
        }
    }
    Ok((slots.logical.checked_sub(bounding_begin) == Some(6)).then_some(()))
}

pub(crate) fn tables(
    ctx: &DecodeContext<'_>,
    section: &str,
    data: &[u8],
) -> Result<Vec<DoubleXarTable>, CodecError> {
    let heap = match section {
        "Model_L05P" | "Model_L05_PX" => Heap::Model,
        "UnitSystemDef_L03" => Heap::Units,
        _ => return Ok(Vec::new()),
    };
    let Some((offset, declared, mut view)) = counted_array(data, TABLE) else {
        return Ok(Vec::new());
    };
    let Some(count) = View::over_retained(data)
        .counted(u64::from(declared), 1)
        .map(cadmpeg_core::decode::BoundedCount::get)
    else {
        return Ok(Vec::new());
    };
    if type_count(data) != Some(count) {
        return Ok(Vec::new());
    }
    let Some(capacity) = count.checked_add(1) else {
        return Ok(Vec::new());
    };
    let body_offset = view.position();
    let entries = ctx.alloc_filled(
        capacity,
        DoubleXarSlot::StockZero,
        "creo double_xar physical tokens",
    )?;
    let mut slots = Slots {
        entries,
        physical: 0,
        logical: 0,
        expected: count,
    };
    match heap {
        Heap::Model => {
            if model_slots(ctx, &mut view, &mut slots)?.is_none() {
                return Ok(Vec::new());
            }
        }
        Heap::Units => {
            if count != 10 {
                return Ok(Vec::new());
            }
            for _ in 0..count {
                let Some(slot) = unit_slot(ctx, &mut view)? else {
                    return Ok(Vec::new());
                };
                if slots.push(slot).is_none() {
                    return Ok(Vec::new());
                }
            }
        }
    }
    if slots.logical != count || !view.unread().starts_with(STRINGS) {
        return Ok(Vec::new());
    }
    slots.entries.truncate(slots.physical);
    let Some(offset) = offset.checked_add(2) else {
        return Ok(Vec::new());
    };
    Ok(vec![DoubleXarTable {
        offset,
        body_offset,
        logical_count: count,
        entries: slots.entries,
    }])
}

#[cfg(test)]
mod tests {
    mod bounding_openers;

    use super::{DoubleXarSlot, STRINGS, TABLE, TYPES};

    fn framed(count: u8, body: &[u8]) -> Vec<u8> {
        let mut bytes = TYPES.to_vec();
        bytes.push(count);
        bytes.extend(std::iter::repeat_n(b'H', usize::from(count)));
        bytes.extend(TABLE);
        bytes.push(count);
        bytes.extend(body);
        bytes.extend(STRINGS);
        bytes
    }

    fn decode(section: &str, bytes: &[u8]) -> Vec<super::DoubleXarTable> {
        crate::decode::with_service_context(|ctx| super::tables(ctx, section, bytes))
            .expect("bounded heap")
    }

    #[test]
    fn unit_heaps_use_owned_positive_mass_and_temperature_scalars() {
        for (body, expected) in [
            (
                vec![
                    0x07, 0x18, 0x0f, 0x18, 0x2f, 0x8f, 0x40, 0x18, 0x0f, 0x2d, 0x71, 0x12, 0x66,
                    0x66, 0x66, 0x66, 0x66, 0x0f, 0x18,
                ],
                [0.001, 0.0, 1.0, 0.0, 1000.0, 0.0, 1.0, 273.15, 1.0, 0.0],
            ),
            (
                vec![
                    0x28, 0x9a, 0x02, 0x75, 0x25, 0x46, 0x0a, 0xa6, 0x18, 0x0f, 0x18, 0x68, 0x07,
                    0xa8, 0x4a, 0xae, 0xd0, 0xe0, 0x18, 0x6c, 0xc7, 0x1c, 0x71, 0xc7, 0x1c, 0x72,
                    0x2d, 0x6f, 0xeb, 0xe9, 0x3e, 0x93, 0xe9, 0x3e, 0x0f, 0x18,
                ],
                [
                    0.0254,
                    0.0,
                    1.0,
                    0.0,
                    0.453_592_369_968_886_2,
                    0.0,
                    0.555_555_555_555_555_6,
                    255.372_222_222_222_2,
                    1.0,
                    0.0,
                ],
            ),
        ] {
            let bytes = framed(10, &body);
            let tables = decode("UnitSystemDef_L03", &bytes);
            assert_eq!(tables.len(), 1);
            assert_eq!(tables[0].logical_count, 10);
            assert_eq!(
                tables[0]
                    .entries
                    .iter()
                    .flat_map(DoubleXarSlot::raw)
                    .copied()
                    .collect::<Vec<_>>(),
                body
            );
            for (slot, expected) in tables[0].entries.iter().zip(expected) {
                assert_eq!(slot.value(), Some(expected));
                assert_eq!(slot.logical_width(), 1);
            }
            assert!(decode("Unowned", &bytes).is_empty());
        }
    }

    #[test]
    fn model_repeats_and_zero_slot_control_keep_exact_physical_ownership() {
        let body = vec![
            0x10, 0xe5, 0x07, 0x23, 0x11, 0x2e, 0x0b, 0xe8, 0x26, 0xd6, 0x95, 0x46, 0x6f, 0xca,
            0x90, 0x66, 0x89, 0xb9, 0x7f, 0xe5, 0x2d, 0x6f, 0xca, 0x90, 0x66, 0x89, 0xb9, 0x7f,
            0xe5,
        ];
        let bytes = framed(11, &body);
        let tables = decode("Model_L05P", &bytes);
        let table = &tables[0];
        assert_eq!(table.logical_count, 11);
        assert_eq!(table.entries.len(), 9);
        assert_eq!(
            table
                .entries
                .iter()
                .map(DoubleXarSlot::logical_width)
                .sum::<usize>(),
            11
        );
        assert_eq!(
            table
                .entries
                .iter()
                .flat_map(DoubleXarSlot::raw)
                .copied()
                .collect::<Vec<_>>(),
            body
        );
        assert_eq!(table.entries[1].kind(), "repeat_two");
        assert_eq!(table.entries[1].raw(), [0xe5]);
        assert_eq!(table.entries[1].value(), Some(1.0));
        assert_eq!(table.entries[2].raw(), [0x07, 0x23, 0x11, 0x2e]);
        assert_eq!(table.entries[4].kind(), "model_control");
        assert_eq!(table.entries[4].logical_width(), 0);
        assert_eq!(table.entries[6].value(), table.entries[5].value());
        assert_eq!(table.entries[8].value(), table.entries[7].value());
        assert!(!table.entries.iter().any(|s| s.raw().contains(&0xe0)));
    }

    #[test]
    fn false_terminal_and_unowned_or_incomplete_slots_are_not_tables() {
        let old = framed(2, &[0x10, 0xe0]);
        assert!(decode("Model_L05P", &old).is_empty());
        let unit = [
            0x07, 0x18, 0x0f, 0x18, 0x2f, 0x8f, 0x40, 0x18, 0x0f, 0x2d, 0x71, 0x12, 0x66, 0x66,
            0x66, 0x66, 0x66, 0x0f, 0x18,
        ];
        let mut wrong_count = framed(10, &unit);
        wrong_count[TYPES.len()] = 9;
        assert!(decode("UnitSystemDef_L03", &wrong_count).is_empty());
        let mut before_value = vec![0xe5];
        before_value.extend([
            0x10, 0xe5, 0x07, 0x23, 0x11, 0x2e, 0x0b, 0xe8, 0x26, 0xd6, 0x95, 0x46, 0x6f, 0xca,
            0x90, 0x66, 0x89, 0xb9, 0x7f, 0xe5, 0x2d, 0x6f, 0xca, 0x90, 0x66, 0x89, 0xb9, 0x7f,
            0xe5,
        ]);
        assert!(decode("Model_L05P", &framed(11, &before_value)).is_empty());
        let mut unknown = vec![
            0x10, 0xe5, 0x07, 0x23, 0x11, 0x2e, 0x0b, 0xe8, 0x26, 0xd6, 0x95, 0x46, 0x6f, 0xca,
            0x90, 0x66, 0x89, 0xb9, 0x7f, 0xe5, 0x2d, 0x6f, 0xca, 0x90, 0x66, 0x89, 0xb9, 0x7f,
            0xe5,
        ];
        let first_bound = unknown
            .windows(8)
            .position(|s| s == [0x46, 0x6f, 0xca, 0x90, 0x66, 0x89, 0xb9, 0x7f])
            .expect("owned bound");
        unknown[first_bound] = 0x43;
        assert!(decode("Model_L05P", &framed(11, &unknown)).is_empty());
        let complete = framed(10, &unit);
        assert!(decode(
            "UnitSystemDef_L03",
            &complete[..complete.len() - STRINGS.len()]
        )
        .is_empty());
        assert!(decode("UnitSystemDef_L03", &framed(10, &unit[..unit.len() - 1])).is_empty());
    }
}
