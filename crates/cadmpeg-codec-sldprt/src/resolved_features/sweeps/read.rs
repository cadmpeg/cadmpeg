use super::super::is_class_token;
use super::super::selections::component_profile_path_record_at;
use crate::layout::{
    mfc_new_class_header as class_head, sweep_component_suffix as suffix,
    sweep_component_tail as tail, sweep_options as options, sweep_reference_blob_prefix as blob,
    sweep_reference_blob_trailer as footer, sweep_wire_contact as wire, zlb_wrapper_header as zlb,
};
use crate::records::FeatureInputLane;
use cadmpeg_core::decode::{bounded_len, View};

#[derive(Debug, Clone, PartialEq)]
pub(super) struct Operand {
    pub(super) offset: usize,
    pub(super) source: Option<u32>,
    pub(super) radius_mm: Option<f64>,
    pub(super) header_assumed: bool,
}

fn class_body(payload: &[u8], offset: usize, class: &str) -> Option<usize> {
    let token = View::u16_le_at(payload, offset + class_head::TAG)?;
    if is_class_token(token) {
        return offset.checked_add(std::mem::size_of::<u16>());
    }
    if token != u16::MAX || View::u16_le_at(payload, offset + class_head::SCHEMA)? != 1 {
        return None;
    }
    let count = View::u16_le_at(payload, offset + class_head::NAME_LENGTH)?;
    let start = offset.checked_add(class_head::LEN)?;
    let len = bounded_len(u64::from(count), 1, payload.len().checked_sub(start)?)?;
    let end = start.checked_add(len)?;
    (payload.get(start..end)? == class.as_bytes()).then_some(end)
}

fn component_end(payload: &[u8], source_end: usize) -> Option<usize> {
    let frame = payload.get(source_end..source_end.checked_add(tail::LEN)?)?;
    if frame[tail::LEADING_ZEROS..tail::REFERENCE_SENTINEL] != tail::LEADING_ZEROS_VALUE
        || frame[tail::REFERENCE_SENTINEL..tail::SECOND_ZEROS] != tail::REFERENCE_SENTINEL_VALUE
        || frame[tail::SECOND_ZEROS..tail::UNIT] != tail::SECOND_ZEROS_VALUE
        || View::f64_le_at(frame, tail::UNIT)? != 1.0
    {
        return None;
    }
    let mut end = source_end.checked_add(tail::LEN)?;
    match frame[tail::BODY_FLAG] {
        0 => {}
        1 => {
            if payload.get(end + blob::FIRST_ZERO) != Some(&blob::FIRST_ZERO_VALUE)
                || payload.get(end + blob::SECOND_ZERO) != Some(&blob::SECOND_ZERO_VALUE)
            {
                return None;
            }
            let outer = View::u32_le_at(payload, end + blob::OUTER_LENGTH)?;
            let inner = View::u32_le_at(payload, end + blob::INNER_LENGTH)?;
            let overhead = u32::try_from(zlb::LEN + footer::LEN).ok()?;
            if outer.checked_sub(5)? != inner || inner < overhead {
                return None;
            }
            let body = end.checked_add(blob::LEN)?;
            let len = bounded_len(u64::from(inner), 1, payload.len().checked_sub(body)?)?;
            let packed = View::u32_le_at(payload, body + zlb::ZLIB_MEMBER_SIZE)?;
            if payload.get(body..body + zlb::MAGIC_VALUE.len())? != zlb::MAGIC_VALUE {
                return None;
            }
            if packed.checked_add(overhead)? != inner {
                return None;
            }
            end = body.checked_add(len)?;
        }
        _ => return None,
    }
    let frame = payload.get(end..end.checked_add(suffix::LEN)?)?;
    (frame[suffix::SENTINEL..suffix::ZEROS] == suffix::SENTINEL_VALUE
        && frame[suffix::ZEROS..] == suffix::ZEROS_VALUE)
        .then_some(end + suffix::LEN)
}

fn circular_radius(payload: &[u8], offset: usize) -> Option<f64> {
    let frame = payload.get(offset..offset.checked_add(options::LEN)?)?;
    if View::u16_le_at(frame, options::CIRCULAR_SECTION)? != 1
        || View::u16_le_at(frame, options::SECTION_WORD)? != options::SECTION_WORD_VALUE
    {
        return None;
    }
    let diameter_m = View::f64_le_at(frame, options::DIAMETER)?;
    let radius_mm = diameter_m * 500.0;
    (radius_mm.is_finite() && radius_mm > 0.0).then_some(radius_mm)
}

pub(super) fn path(lane: &FeatureInputLane, start: usize, end: usize) -> Option<Operand> {
    let payload = lane.native_payload.get(..end)?;
    let mut contacts = lane.classes.iter().filter(|class| {
        class.name == "moWireContact_c"
            && usize::try_from(class.offset).is_ok_and(|offset| (start..end).contains(&offset))
    });
    let contact = contacts.next()?;
    if contacts.next().is_some() {
        return None;
    }
    let body = class_body(
        payload,
        usize::try_from(contact.offset).ok()?,
        "moWireContact_c",
    )?;
    if View::u32_le_at(payload, body + wire::ZERO)? != wire::ZERO_VALUE
        || View::u32_le_at(payload, body + wire::ONE)? != wire::ONE_VALUE
        || View::i32_le_at(payload, body + wire::SENTINEL)? != wire::SENTINEL_VALUE
        || View::u16_le_at(payload, body + wire::DIMENSION)? != wire::DIMENSION_VALUE
    {
        return None;
    }
    for vector in [wire::FIRST_VECTOR, wire::SECOND_VECTOR] {
        for axis in 0..3 {
            if !View::f64_le_at(payload, body + vector + axis * 8)?.is_finite() {
                return None;
            }
        }
    }
    let offset = body.checked_add(wire::LEN)?;
    let child = class_body(payload, offset, "moGeneralCurveRef_w")?;
    let prefix = class_body(payload, child, "moCompProfile_c")
        .or_else(|| class_body(payload, child, "moCompReferenceCurve_c"))?;
    let record = component_profile_path_record_at(payload, prefix);
    let radius_mm = record
        .and_then(|(_, end, _)| component_end(payload, end))
        .and_then(|options| circular_radius(payload, options));
    Some(Operand {
        offset,
        source: record.map(|(source, _, _)| source),
        radius_mm,
        header_assumed: record.is_some_and(|(_, _, assumed)| assumed),
    })
}

#[cfg(test)]
mod tests;
