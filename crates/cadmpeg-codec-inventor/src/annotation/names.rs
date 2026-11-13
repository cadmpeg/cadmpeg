// SPDX-License-Identifier: Apache-2.0
//! Name records and the generated-name attributes of the active body.

use std::collections::{HashMap, HashSet};

use cadmpeg_core::decode::View;
use cadmpeg_core::CodecError;
use cadmpeg_ir::attributes::{AttributeTarget, AttributeValue};
use cadmpeg_ir::document::Model;

use crate::layout::{
    pm_dc_blend_name_tail as blend_tail, pm_dc_generated_name_tail as generated_tail,
    pm_dc_hole_name_tail as hole_tail,
};
use crate::pmdc::Cursor;

pub(super) const SWEEP_NAME: [u8; 16] = [
    0xa5, 0xc2, 0x45, 0x06, 0x28, 0x46, 0xbb, 0xe6, 0x1d, 0xa9, 0xed, 0xa1, 0x20, 0x72, 0xce, 0x35,
];
pub(super) const GENERATED_NAME: [u8; 16] = [
    0x20, 0x5f, 0xa9, 0x56, 0xd1, 0x11, 0xaf, 0x49, 0x00, 0x08, 0x87, 0xba, 0x32, 0xa3, 0xdc, 0x09,
];
pub(super) const HOLE_NAME: [u8; 16] = [
    0x7a, 0x26, 0x0e, 0xae, 0xd2, 0x11, 0x71, 0x5d, 0x60, 0x00, 0x7e, 0xb7, 0x12, 0x69, 0xf1, 0xb0,
];
pub(super) const BLEND_NAME: [u8; 16] = [
    0x98, 0x41, 0x5d, 0x8e, 0xc3, 0x41, 0xc7, 0xa5, 0xad, 0xc5, 0xf9, 0x90, 0x46, 0x05, 0x79, 0xa4,
];
pub(super) const EDGE_NAME: [u8; 16] = [
    0xa6, 0xe0, 0x32, 0xbf, 0xd1, 0x11, 0x46, 0x4a, 0x00, 0x08, 0x88, 0xba, 0x32, 0xa3, 0xdc, 0x09,
];
/// `word_a` of a generated name record.
const GENERATED_FORM: u32 = 5;
/// `word_a` of an edge name record.
const EDGE_FORM: u32 = 1;
/// The list signature of the name-record prefix.
const NAME_SIGNATURE: u32 = 0x3000_0002;

/// One generated name: ordered `(source, name value)` pairs and the tail
/// bytes after them.
#[derive(Debug, PartialEq, Eq)]
pub(super) struct GeneratedName {
    pub(super) family: NameFamily,
    pub(super) pairs: Vec<(i64, i64)>,
    pub(super) tail: Vec<u8>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum NameFamily {
    Generated,
    Hole,
    Blend,
}

impl NameFamily {
    pub(super) fn of(type_id: [u8; 16]) -> Option<Self> {
        match type_id {
            SWEEP_NAME | GENERATED_NAME => Some(Self::Generated),
            HOLE_NAME => Some(Self::Hole),
            BLEND_NAME => Some(Self::Blend),
            _ => None,
        }
    }

    fn attribute(family: &str) -> Option<Self> {
        match family {
            "NMx_Generated_Brep_tag"
            | "NMx_Sweep_Generated_Brep_tag"
            | "INV_NMX_GEN_TAG"
            | "INV_NMX_SWEEPGENERATED_TAG" => Some(Self::Generated),
            "NMx_Hole_Brep_tag" | "INV_NMX_HOLE_TAG" => Some(Self::Hole),
            "NMx_blend_tag" | "INV_NMX_BLEND_TAG" => Some(Self::Blend),
            _ => None,
        }
    }

    /// The tail values of a name record that its matching attribute repeats.
    pub(super) fn record_tail(self, tail: &[u8]) -> Option<Vec<i64>> {
        let word = |offset: usize| View::u32_le_at(tail, offset).map(i64::from);
        let byte = |offset: usize| tail.get(offset).copied().map(i64::from);
        match self {
            Self::Generated => {
                (View::u32_le_at(tail, generated_tail::LEAD)? == generated_tail::LEAD_VALUE
                    && View::u32_le_at(tail, generated_tail::SECOND)?
                        == generated_tail::SECOND_VALUE)
                    .then_some(())?;
                Some(vec![
                    word(generated_tail::ASSOCIATIVE_ID)?,
                    word(generated_tail::ROLE)?,
                ])
            }
            Self::Hole => {
                (View::u32_le_at(tail, hole_tail::LEAD)? == hole_tail::LEAD_VALUE).then_some(())?;
                Some(vec![
                    word(hole_tail::FIRST)?,
                    i64::from(View::u16_le_at(tail, hole_tail::ROLE)?),
                ])
            }
            Self::Blend => {
                (View::u32_le_at(tail, blend_tail::LEAD)? == blend_tail::LEAD_VALUE)
                    .then_some(())?;
                Some(vec![
                    byte(blend_tail::FIRST)?,
                    byte(blend_tail::SECOND)?,
                    word(blend_tail::THIRD)?,
                ])
            }
        }
    }

    /// The tail values of a generated-name attribute that its name record
    /// stores.
    pub(super) fn attribute_tail(self, tail: &[i64]) -> Option<Vec<i64>> {
        match (self, tail) {
            (Self::Generated, [associative, 0, role, ..]) => Some(vec![*associative, *role]),
            (Self::Hole, [first, role, ..]) => Some(vec![*first, *role]),
            (Self::Blend, [first, second, third, ..]) => Some(vec![*first, *second, *third]),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub(super) enum EntityKind {
    Face,
    Edge,
}

/// One generated-name attribute of the active body.
pub(super) struct AttributeName<'m> {
    pub(super) kind: EntityKind,
    pub(super) entity: &'m str,
    pub(super) family: NameFamily,
    pub(super) tail: Vec<i64>,
}

/// The generated-name attributes of the active body, keyed by their pairs.
#[derive(Default)]
pub(super) struct BodyNames<'m> {
    pub(super) by_pairs: HashMap<Vec<(i64, i64)>, Vec<AttributeName<'m>>>,
    pub(super) face_edges: HashMap<&'m str, HashSet<&'m str>>,
}

impl<'m> BodyNames<'m> {
    pub(super) fn new(model: &'m Model) -> Self {
        let mut names = Self::default();
        for attribute in &model.attributes {
            let (kind, entity) = match &attribute.target {
                AttributeTarget::Face(face) => (EntityKind::Face, face.as_str()),
                AttributeTarget::Edge(edge) => (EntityKind::Edge, edge.as_str()),
                _ => continue,
            };
            let Some((family, pairs, tail)) = attribute_name(&attribute.name, &attribute.values)
            else {
                continue;
            };
            names
                .by_pairs
                .entry(pairs)
                .or_default()
                .push(AttributeName {
                    kind,
                    entity,
                    family,
                    tail,
                });
        }
        let loop_faces = model
            .loops
            .iter()
            .map(|entry| (entry.id.as_str(), entry.face.as_str()))
            .collect::<HashMap<_, _>>();
        for coedge in &model.coedges {
            if let Some(face) = loop_faces.get(coedge.owner_loop.as_str()) {
                names
                    .face_edges
                    .entry(face)
                    .or_default()
                    .insert(coedge.edge.as_str());
            }
        }
        names
    }

    /// The unique entity of `kind` whose generated name equals `name`.
    pub(super) fn select(&self, name: &GeneratedName, kind: EntityKind) -> Option<&'m str> {
        let expected = name.family.record_tail(&name.tail)?;
        let mut found = None;
        for candidate in self.by_pairs.get(&name.pairs)? {
            if candidate.kind != kind
                || candidate.family != name.family
                || name.family.attribute_tail(&candidate.tail).as_ref() != Some(&expected)
            {
                continue;
            }
            match found {
                None => found = Some(candidate.entity),
                Some(entity) if entity == candidate.entity => {}
                Some(_) => return None,
            }
        }
        found
    }

    /// The unique edge that bounds both faces.
    pub(super) fn shared_edge(&self, first: &str, second: &str) -> Option<&'m str> {
        let first = self.face_edges.get(first)?;
        let second = self.face_edges.get(second)?;
        let mut shared = first.intersection(second);
        let edge = shared.next()?;
        shared.next().is_none().then_some(*edge)
    }
}

/// The family, pairs, and tail of a generated-name attribute.
pub(super) type AttributeNameParts = (NameFamily, Vec<(i64, i64)>, Vec<i64>);

/// The family, pairs, and tail of a generated-name attribute.
pub(super) fn attribute_name(name: &str, values: &[AttributeValue]) -> Option<AttributeNameParts> {
    let fields = values.get(5..)?;
    let (family, counted) = match fields {
        [AttributeValue::String(family), AttributeValue::Integer(1), AttributeValue::Integer(44), rest @ ..]
            if name.starts_with("ATTRIB_CUSTOM") =>
        {
            (NameFamily::attribute(family)?, rest)
        }
        [AttributeValue::Integer(_), rest @ ..] => {
            (NameFamily::attribute(name.split('-').next()?)?, rest)
        }
        _ => return None,
    };
    let integers = counted
        .iter()
        .map(|value| match value {
            AttributeValue::Integer(value) => Some(*value),
            _ => None,
        })
        .collect::<Option<Vec<_>>>()?;
    let (&count, rest) = integers.split_first()?;
    let count = usize::try_from(count).ok().filter(|count| *count > 0)?;
    let pair_values = rest.get(..count.checked_mul(2)?)?;
    let pairs = pair_values
        .chunks_exact(2)
        .map(|pair| (pair[0], pair[1]))
        .collect();
    Some((family, pairs, rest.get(count * 2..)?.to_vec()))
}

/// The common prefix of a name record, up to its first list.
fn name_prefix(cursor: &mut Cursor<'_>, form: u32) -> Result<(), CodecError> {
    cursor.u32("name header value")?;
    cursor.u16("name header id")?;
    if cursor.u32("name form")? != form {
        return Err(CodecError::malformed("Inventor name form differs"));
    }
    if cursor.u32("name signature")? != NAME_SIGNATURE {
        return Err(CodecError::malformed("Inventor name signature differs"));
    }
    if cursor.u32_array::<6>("name zero values")? != [0; 6] {
        return Err(CodecError::malformed("Inventor name zero values differ"));
    }
    cursor.u32("name class value")?;
    if cursor.u32("name zero value")? != 0 {
        return Err(CodecError::malformed("Inventor name zero value differs"));
    }
    Ok(())
}

pub(super) fn generated_name(
    family: NameFamily,
    payload: View<'_>,
) -> Result<GeneratedName, CodecError> {
    let mut cursor = Cursor::new(payload);
    name_prefix(&mut cursor, GENERATED_FORM)?;
    if [
        cursor.u16("name list marker")?,
        cursor.u16("name list signature")?,
    ] != [2, 0x3000]
    {
        return Err(CodecError::malformed("Inventor name list marker differs"));
    }
    let count = cursor.u32("name pair count")?;
    if count == 0 {
        return Err(CodecError::malformed("Inventor name has no pairs"));
    }
    cursor.u32_array::<2>("name list metadata")?;
    let mut pairs = Vec::new();
    for _ in 0..count {
        let source = cursor.u32("name source")?;
        let value = cursor.i32("name value")?;
        pairs.push((i64::from(source), i64::from(value)));
    }
    if pairs.last().map(|pair| pair.1) != Some(-1) {
        return Err(CodecError::malformed(
            "Inventor name ends in a name value other than -1",
        ));
    }
    Ok(GeneratedName {
        family,
        pairs,
        tail: cursor.view().unread().to_vec(),
    })
}

/// The two face names of an edge name in the empty-list form, or `None` for
/// another form.
pub(super) fn edge_name(payload: View<'_>) -> Result<Option<[u32; 2]>, CodecError> {
    let mut cursor = Cursor::new(payload);
    name_prefix(&mut cursor, EDGE_FORM)?;
    cursor.u32_array::<4>("edge-name base")?;
    cursor.u8("edge-name flag")?;
    let list = crate::pmdc::reference_list(&mut cursor, 2, "edge-name list")?;
    if !list.references().is_empty() {
        return Ok(None);
    }
    cursor.u32("edge-name control")?;
    if cursor.u32("edge-name face count")? != 2 {
        return Ok(None);
    }
    let mut faces = [0; 2];
    for face in &mut faces {
        cursor.reference("edge-name context")?;
        *face = cursor.u32("edge-name face")?;
    }
    cursor.finish("edge name")?;
    Ok(Some(faces))
}
