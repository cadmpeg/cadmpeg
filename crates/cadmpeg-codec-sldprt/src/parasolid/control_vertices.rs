// SPDX-License-Identifier: Apache-2.0
//! Reference-directed construction-curve control vertices.

use cadmpeg_core::decode::{bounded_len, u64_from_index, DecodeContext, View, WorkBudget};
use cadmpeg_core::CodecError;
use cadmpeg_ir::math::Point3;

use super::StreamHeader;
use crate::layout::{
    bspline_array_header as array_header, parasolid_b_curve_short as curve_header,
    parasolid_construction_prefix as construction_prefix,
    parasolid_nurbs_curve_short as nurbs_header,
};

const MILLIMETERS_PER_METER: f64 = 1000.0;
const BASE_BODY: [Field<'static>; 23] = [
    Field::new("highest_node_id", Kind::Int),
    Field::new("attributes_groups", Kind::Ref),
    Field::new("attribute_chains", Kind::Ref),
    Field::new("surface", Kind::Ref),
    Field::new("curve", Kind::Ref),
    Field::new("point", Kind::Ref),
    Field::new("key", Kind::Ref),
    Field::new("res_size", Kind::Double),
    Field::new("res_linear", Kind::Double),
    Field::new("ref_instance", Kind::Ref),
    Field::new("next", Kind::Ref),
    Field::new("previous", Kind::Ref),
    Field::new("state", Kind::Byte),
    Field::new("owner", Kind::Ref),
    Field::new("body_type", Kind::Byte),
    Field::new("nom_geom_state", Kind::Byte),
    Field::new("shell", Kind::Ref),
    Field::new("boundary_surface", Kind::Ref),
    Field::new("boundary_curve", Kind::Ref),
    Field::new("boundary_point", Kind::Ref),
    Field::new("region", Kind::Ref),
    Field::new("edge", Kind::Ref),
    Field::new("vertex", Kind::Ref),
];

#[derive(Clone, Copy)]
enum Kind {
    Ref,
    Int,
    Short,
    Double,
    Byte,
    Logical,
}
#[derive(Clone, Copy)]
struct Field<'a> {
    name: &'a str,
    kind: Kind,
}
impl<'a> Field<'a> {
    const fn new(name: &'a str, kind: Kind) -> Self {
        Self { name, kind }
    }
}
struct Cursor<'a> {
    bytes: &'a [u8],
    at: usize,
}
impl<'a> Cursor<'a> {
    fn byte(&mut self) -> Option<u8> {
        let n = *self.bytes.get(self.at)?;
        self.at += 1;
        Some(n)
    }
    fn short(&mut self) -> Option<u16> {
        let n = View::u16_be_at(self.bytes, self.at)?;
        self.at += 2;
        Some(n)
    }
    fn int(&mut self) -> Option<u32> {
        let n = View::u32_be_at(self.bytes, self.at)?;
        self.at += 4;
        Some(n)
    }
    fn double(&mut self) -> Option<f64> {
        let n = View::f64_be_at(self.bytes, self.at)?;
        self.at += 8;
        n.is_finite().then_some(n)
    }
    fn reference(&mut self) -> Option<u32> {
        let r = self.short()?;
        if r == 0 {
            return None;
        }
        if r <= i16::MAX.cast_unsigned() {
            return Some(u32::from(r));
        }
        let magnitude = u32::from((!r).checked_add(1)?);
        let quotient = u32::from(self.short()?);
        if quotient == 0 || quotient > 32767 || magnitude > 32767 {
            return None;
        }
        quotient.checked_mul(32767)?.checked_add(magnitude)
    }
    fn string(&mut self) -> Option<&'a str> {
        let count = usize::from(self.byte()?);
        let end = self.at.checked_add(count)?;
        let value = std::str::from_utf8(self.bytes.get(self.at..end)?).ok()?;
        self.at = end;
        Some(value)
    }
    fn field(&mut self) -> Option<Field<'a>> {
        let name = self.string()?;
        let pointer = self.short()?;
        let count = self.reference()?.checked_sub(1)?;
        let kind = if pointer != 0 {
            Kind::Ref
        } else {
            match self.string()? {
                "d" => Kind::Int,
                "n" => Kind::Short,
                "f" => Kind::Double,
                "u" | "c" => Kind::Byte,
                "l" => Kind::Logical,
                _ => return None,
            }
        };
        (count == 0).then_some(Field::new(name, kind))
    }
    fn skip(&mut self, kind: Kind) -> Option<()> {
        match kind {
            Kind::Ref => {
                self.reference()?;
            }
            Kind::Int => {
                self.int()?;
            }
            Kind::Short => {
                self.short()?;
            }
            Kind::Double => {
                self.double()?;
            }
            Kind::Byte => {
                self.byte()?;
            }
            Kind::Logical => {
                (self.byte()? <= 1).then_some(())?;
            }
        }
        Some(())
    }
}

struct BodySchema<'a> {
    fields: [Field<'a>; 255],
    len: usize,
}

fn body_schema<'a>(bytes: &'a [u8], header: &StreamHeader) -> Option<(BodySchema<'a>, usize, u16)> {
    bytes.get(header.body_offset..header.body_offset.checked_add(construction_prefix::LEN)?)?;
    let mut cursor = Cursor {
        bytes,
        at: header.body_offset,
    };
    let max_type = cursor.short()?;
    (max_type >= 136
        && cursor.int()? == construction_prefix::USER_FIELD_WIDTH_VALUE
        && cursor.short()? == construction_prefix::FIRST_NODE_TYPE_VALUE)
        .then_some(())?;
    let count = cursor.byte()?;
    let mut fields = [Field::new("", Kind::Byte); 255];
    let len;
    if count == 255 {
        fields[..BASE_BODY.len()].copy_from_slice(&BASE_BODY);
        len = BASE_BODY.len();
    } else {
        len = usize::from(count);
        let mut base = 0;
        let mut out = 0;
        loop {
            let instruction = cursor.byte()?;
            if instruction == b'Z' {
                break;
            }
            if out >= len && instruction != b'D' {
                return None;
            }
            match instruction {
                b'C' => {
                    fields[out] = *BASE_BODY.get(base)?;
                    base += 1;
                    out += 1;
                }
                b'D' => {
                    BASE_BODY.get(base)?;
                    base += 1;
                }
                b'I' | b'A' => {
                    if (instruction == b'A') != (base == BASE_BODY.len()) {
                        return None;
                    }
                    fields[out] = cursor.field()?;
                    out += 1;
                }
                _ => return None,
            }
        }
        (out == len && base == BASE_BODY.len()).then_some(())?;
    }
    Some((BodySchema { fields, len }, cursor.at, max_type))
}

fn body_values(
    bytes: &[u8],
    at: usize,
    schema: &BodySchema<'_>,
    max_type: u16,
) -> Option<(u8, u32, usize)> {
    let mut cursor = Cursor { bytes, at };
    (cursor.reference()? > 1).then_some(())?;
    let (mut kind, mut boundary, mut highest, mut state, mut nominal, mut size, mut linear) =
        (None, None, None, None, None, None, None);
    for field in &schema.fields[..schema.len] {
        match field.name {
            "body_type" => {
                if kind.is_some() || !matches!(field.kind, Kind::Byte) {
                    return None;
                }
                kind = Some(cursor.byte()?);
            }
            "boundary_curve" => {
                if boundary.is_some() || !matches!(field.kind, Kind::Ref) {
                    return None;
                }
                boundary = Some(cursor.reference()?);
            }
            "highest_node_id" => {
                if highest.is_some() || !matches!(field.kind, Kind::Int) {
                    return None;
                }
                highest = Some(cursor.int()?);
            }
            "state" => {
                if state.is_some() || !matches!(field.kind, Kind::Byte) {
                    return None;
                }
                state = Some(cursor.byte()?);
            }
            "nom_geom_state" => {
                if nominal.is_some() || !matches!(field.kind, Kind::Byte) {
                    return None;
                }
                nominal = Some(cursor.byte()?);
            }
            "res_size" => {
                if size.is_some() || !matches!(field.kind, Kind::Double) {
                    return None;
                }
                size = Some(cursor.double()?);
            }
            "res_linear" => {
                if linear.is_some() || !matches!(field.kind, Kind::Double) {
                    return None;
                }
                linear = Some(cursor.double()?);
            }
            _ => cursor.skip(field.kind)?,
        }
    }
    let kind = kind?;
    if kind == 2
        && (highest? == 0
            || state? != 1
            || nominal? != 1
            || size? <= 0.0
            || linear? <= 0.0
            || linear? >= size?)
    {
        return None;
    }
    let end = cursor.at;
    let next = cursor.short()?;
    (next > 0 && next <= max_type).then_some((kind, boundary?, end))
}

fn body_boundary(
    bytes: &[u8],
    header: &StreamHeader,
    work: &WorkBudget,
) -> Option<(u32, usize, u16)> {
    let (schema, at, max_type) = body_schema(bytes, header)?;
    let (kind, boundary, end) = body_values(bytes, at, &schema, max_type)?;
    (kind == 2 && boundary > 1).then_some(())?;
    work.charge_by(bytes.len()).then_some(())?;
    for at in end..bytes.len().checked_sub(2)? {
        if View::u16_be_at(bytes, at) != Some(12) {
            continue;
        }
        work.charge_by(schema.len).then_some(())?;
        if let Some((2, other_boundary, _)) = body_values(bytes, at + 2, &schema, max_type) {
            if other_boundary > 1 {
                return None;
            }
        }
    }
    Some((boundary, end, max_type))
}

#[derive(Clone, Copy)]
struct Node {
    end: usize,
    next: u32,
    target: u32,
    count: usize,
    dimension: u16,
    values: usize,
}
fn node_at(bytes: &[u8], at: usize, kind: u16, first: bool, max_type: u16) -> Option<(u32, Node)> {
    let mut cursor = Cursor { bytes, at };
    (cursor.short()? == kind).then_some(())?;
    if first {
        (cursor.byte()? == 255).then_some(())?;
    }
    let frame_start = cursor.at;
    let min_size = match kind {
        134 => curve_header::LEN,
        136 => nurbs_header::LEN,
        45 => array_header::LEN,
        _ => return None,
    };
    bytes.get(frame_start..frame_start.checked_add(min_size)?)?;
    let count = if kind == 45 {
        usize::try_from(cursor.int()?).ok()?
    } else {
        0
    };
    let identity = cursor.reference()?;
    (identity > 1).then_some(())?;
    let mut node = Node {
        end: 0,
        next: 1,
        target: 1,
        count,
        dimension: 0,
        values: 0,
    };
    match kind {
        134 => {
            (cursor.int()? > 0).then_some(())?;
            cursor.reference()?;
            cursor.reference()?;
            node.next = cursor.reference()?;
            cursor.reference()?;
            cursor.reference()?;
            matches!(cursor.byte()?, b'+' | b'-').then_some(())?;
            node.target = cursor.reference()?;
            cursor.reference()?;
        }
        136 => {
            let degree = cursor.short()?;
            node.count = usize::try_from(cursor.int()?).ok()?;
            node.dimension = cursor.short()?;
            let knots = cursor.int()?;
            (degree > 0
                && usize::from(degree) < node.count
                && node.count > 0
                && node.count <= bytes.len() / 8
                && (2..=4).contains(&node.dimension)
                && knots > 0
                && usize::try_from(knots).ok()? <= bytes.len() / 2)
                .then_some(())?;
            cursor.byte()?;
            for _ in 0..2 {
                (cursor.byte()? <= 1).then_some(())?;
            }
            (cursor.byte()? == 0).then_some(())?;
            cursor.byte()?;
            node.target = cursor.reference()?;
            cursor.reference()?;
            cursor.reference()?;
        }
        45 => {
            (count > 0 && count <= bytes.len() / 8).then_some(())?;
            node.values = cursor.at;
            cursor.at = cursor.at.checked_add(count.checked_mul(8)?)?;
            bytes.get(node.values..cursor.at)?;
        }
        _ => return None,
    }
    node.end = cursor.at;
    let next = cursor.short()?;
    (next > 0 && next <= max_type).then_some(())?;
    Some((identity, node))
}

fn referenced_node(
    bytes: &[u8],
    start: usize,
    kind: u16,
    identity: u32,
    max_type: u16,
    work: &WorkBudget,
) -> Option<Node> {
    work.charge_by(bytes.len()).then_some(())?;
    let tag = kind.to_be_bytes();
    let mut first = None;
    for at in start..bytes.len().checked_sub(2)? {
        if bytes.get(at..at + 2) != Some(tag.as_slice()) || bytes.get(at + 2) != Some(&255) {
            continue;
        }
        if node_at(bytes, at, kind, true, max_type).is_some() {
            if first.replace(at).is_some() {
                return None;
            }
        }
    }
    let first = first?;
    let mut selected = None;
    let mut at = first;
    while at < bytes.len().checked_sub(1)? {
        if bytes.get(at..at + 2) == Some(tag.as_slice()) {
            if let Some((candidate, node)) = node_at(bytes, at, kind, at == first, max_type) {
                if candidate == identity {
                    if selected.replace(node).is_some() {
                        return None;
                    }
                }
                at = node.end;
                continue;
            }
        }
        at += 1;
    }
    selected
}

pub(crate) fn control_vertices(
    ctx: &DecodeContext<'_>,
    bytes: &[u8],
    header: &StreamHeader,
) -> Result<Option<Vec<Point3>>, CodecError> {
    if !header.schema.value().as_bytes().ends_with(b"_13006") {
        return Ok(None);
    }
    let work = WorkBudget::new(bytes.len().checked_mul(16).ok_or_else(|| {
        CodecError::Malformed("SLDPRT construction scan length overflows".into())
    })?);
    let Some((mut head, start, max_type)) = body_boundary(bytes, header, &work) else {
        return Ok(None);
    };
    let mut seen = std::collections::HashSet::new();
    let mut selected = None;
    while head != 1 {
        if !seen.insert(head) {
            return Ok(None);
        }
        let Some(curve) = referenced_node(bytes, start, 134, head, max_type, &work) else {
            return Ok(None);
        };
        let Some(nurbs) = referenced_node(bytes, start, 136, curve.target, max_type, &work) else {
            return Ok(None);
        };
        let Some(vertices) = referenced_node(bytes, start, 45, nurbs.target, max_type, &work)
        else {
            return Ok(None);
        };
        if nurbs.dimension != 3
            || nurbs.count.checked_mul(3) != Some(vertices.count)
            || selected.replace(vertices).is_some()
        {
            return Ok(None);
        }
        head = curve.next;
    }
    let Some(vertices) = selected else {
        return Ok(None);
    };
    let Some(count) = bounded_len(
        u64_from_index(vertices.count / 3),
        24,
        vertices.end - vertices.values,
    ) else {
        return Ok(None);
    };
    let mut points = ctx.alloc_filled(
        count,
        Point3::new(0.0, 0.0, 0.0),
        "decode SLDPRT construction control vertices",
    )?;
    for (index, point) in points.iter_mut().enumerate() {
        let at = vertices.values + index * 24;
        let xyz = [0, 8, 16]
            .map(|offset| View::f64_be_at(bytes, at + offset).map(|v| v * MILLIMETERS_PER_METER));
        let [Some(x), Some(y), Some(z)] = xyz else {
            return Ok(None);
        };
        *point = Point3::new(x, y, z);
        if !point.is_finite() {
            return Ok(None);
        }
    }
    Ok(Some(points))
}

#[cfg(test)]
mod tests;
