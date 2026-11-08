// SPDX-License-Identifier: Apache-2.0
//! Part-model dimension annotations, their tolerances, and their targets.
//!
//! A feature label names an annotation owner. A dimension owner names one
//! value record and its geometry intents. A relation joins each intent to a
//! selection item, and a naming selector names that item with a key of the
//! naming table of the segment. The key selects a name record. A generated
//! name record matches the generated-name attributes of the active body, and
//! an edge name record names an edge by its two faces.

use std::collections::{HashMap, HashSet};

use cadmpeg_core::decode::{DecodeContext, View};
use cadmpeg_core::CodecError;
use cadmpeg_ir::attributes::{AttributeTarget, AttributeValue};
use cadmpeg_ir::document::Model;
use cadmpeg_ir::ids::{EdgeId, FaceId, PmiId};
use cadmpeg_ir::pmi::zones::ToleranceConstraints;
use cadmpeg_ir::pmi::{
    DatumReference, DatumReferences, DimensionKind, DimensionTolerance, GeometricToleranceKind,
    LimitsAndFits, PmiAnnotation, PmiDefinition, PmiDimension, PmiMagnitude, PmiQuantity,
    PmiTarget, PmiValue,
};
use cadmpeg_ir::report::loss::LossNote;

use crate::design::PmDcParameter;
use crate::feature::PmDcFeatureLabel;
use crate::loss::InventorLossCode;
use crate::pmdc::{
    content_header, state_content_header, u32_list_with_context, Cursor, PmDcReference,
};
use crate::records::RseRecordFrame;
use crate::rse::{RecordFrameState, RseInventory, SegmentBulkState, SegmentKind};

const DIAMETER_OWNER: [u8; 16] = [
    0x75, 0x1a, 0xde, 0x16, 0x74, 0x4f, 0xe9, 0x4f, 0xc4, 0x98, 0xa8, 0xa8, 0xe8, 0x9f, 0x29, 0xdd,
];
const LINEAR_OWNER: [u8; 16] = [
    0xe1, 0xc3, 0x66, 0x65, 0xd2, 0x49, 0x68, 0xac, 0x24, 0xce, 0xa9, 0xb3, 0xcc, 0x60, 0x55, 0xcd,
];
const ANGULAR_OWNER: [u8; 16] = [
    0x4f, 0x02, 0xd6, 0x9e, 0x38, 0x44, 0x25, 0x08, 0x2e, 0xa7, 0x33, 0xb6, 0xa1, 0x07, 0x0a, 0x20,
];
const RADIAL_OWNER: [u8; 16] = [
    0x3c, 0xeb, 0x8c, 0xbe, 0x02, 0x44, 0x69, 0x07, 0xd3, 0xb5, 0x48, 0xa3, 0xbf, 0xc8, 0x7b, 0x32,
];
const HOLE_NOTE_OWNER: [u8; 16] = [
    0x0c, 0x00, 0x74, 0x95, 0x8f, 0x4a, 0x8f, 0x36, 0x7f, 0x67, 0xfe, 0x82, 0x13, 0x29, 0xcd, 0xbd,
];
const HOLE_VALUE: [u8; 16] = [
    0x1c, 0x0e, 0xcf, 0xdd, 0x56, 0x42, 0x1a, 0x02, 0xb3, 0x14, 0x56, 0x92, 0x15, 0xe8, 0x30, 0xb1,
];
const PARAMETER: [u8; 16] = crate::pmdc::inventor_id(0x9087_4d26);
/// The flag block between the hole parameters and the hole text of a plain
/// hole note.
const HOLE_FLAGS: [u8; 16] = [1, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 1, 0, 0, 0, 0];
const VALUE: [u8; 16] = [
    0x1f, 0x02, 0xe7, 0x8b, 0x32, 0x45, 0x48, 0xa6, 0xbd, 0x6d, 0x4c, 0x90, 0x71, 0x1d, 0xb6, 0xd0,
];
const DATUM_OWNER: [u8; 16] = [
    0x47, 0x59, 0x6f, 0x34, 0x43, 0x4b, 0xd3, 0xef, 0x76, 0x8f, 0x59, 0x9a, 0x02, 0xd0, 0xef, 0xab,
];
const DATUM_FEATURE: [u8; 16] = [
    0x7b, 0x02, 0x3e, 0x03, 0x30, 0x49, 0xcd, 0x9e, 0xf9, 0x1b, 0x87, 0xa2, 0x59, 0x82, 0x9c, 0x67,
];
const FRAME_OWNER: [u8; 16] = [
    0x37, 0xe2, 0xb7, 0xca, 0xf5, 0x4e, 0xea, 0x9d, 0x7f, 0x98, 0x4b, 0xa8, 0x3f, 0x1c, 0x1a, 0xb1,
];
const FRAME_ROW: [u8; 16] = [
    0x26, 0x1b, 0x9f, 0xa9, 0xbc, 0x4d, 0xaf, 0xc3, 0x8f, 0x05, 0x70, 0x97, 0x0a, 0x83, 0x13, 0xce,
];
const TOLERANCE_FEATURE: [u8; 16] = [
    0xcf, 0x2b, 0x17, 0xde, 0x9c, 0x46, 0xa8, 0x94, 0xe0, 0x64, 0x59, 0xa7, 0x05, 0x46, 0xaa, 0x94,
];
const FACE_COLLECTION: [u8; 16] = [
    0xf1, 0xb2, 0xbc, 0x24, 0xd0, 0x11, 0x21, 0xdd, 0x00, 0x08, 0xd0, 0xbc, 0x06, 0x63, 0xdc, 0x09,
];
const FACE_SELECTION_ITEM: [u8; 16] = [
    0xcc, 0xb5, 0x9f, 0x50, 0xd1, 0x11, 0x4f, 0x6b, 0x00, 0x08, 0xa1, 0xba, 0x32, 0xa3, 0xdc, 0x09,
];
const FEATURE_FACE_SELECTOR: [u8; 16] = [
    0x2c, 0x81, 0xaa, 0x03, 0xd2, 0x11, 0xff, 0x26, 0x00, 0x08, 0x1f, 0xbb, 0x32, 0xa3, 0xdc, 0x09,
];
const INTENT: [u8; 16] = [
    0xb4, 0x31, 0x79, 0x61, 0x69, 0x44, 0x93, 0x84, 0xe3, 0xad, 0x40, 0x9b, 0xde, 0xbf, 0x7e, 0x05,
];
const STYLE: [u8; 16] = [
    0xae, 0x38, 0xb9, 0x99, 0xd2, 0x11, 0xc3, 0x5b, 0x60, 0x00, 0xb3, 0x85, 0x6c, 0x23, 0x87, 0xb0,
];
const SIZE_RELATION: [u8; 16] = [
    0x3c, 0x31, 0x13, 0x3b, 0x47, 0x4a, 0x98, 0x6b, 0xd1, 0xea, 0x9b, 0xa7, 0x30, 0xc7, 0xe5, 0x7c,
];
const FACE_RELATION: [u8; 16] = [
    0xc4, 0xf8, 0x1a, 0x38, 0x29, 0x4b, 0xd8, 0xee, 0x18, 0xe9, 0x0e, 0xb9, 0x1d, 0xf9, 0xb3, 0x76,
];
const PAIR_RELATION: [u8; 16] = [
    0x8e, 0x92, 0xd9, 0x6f, 0x62, 0x48, 0x9d, 0xb9, 0xa2, 0xb2, 0x4d, 0xa6, 0x6e, 0x31, 0x22, 0x98,
];
const EDGE_ITEM: [u8; 16] = [
    0x6a, 0x71, 0x43, 0x9e, 0xd2, 0x11, 0xa5, 0x0f, 0x60, 0x00, 0x84, 0xb7, 0xb0, 0x35, 0xc3, 0xb0,
];
const FACE_ITEM: [u8; 16] = [
    0x89, 0x6c, 0xf0, 0x8e, 0xd1, 0x11, 0x3c, 0x04, 0x60, 0x00, 0x7c, 0xb8, 0x01, 0xf3, 0x1b, 0xb0,
];
const GEOMETRY_LINK: [u8; 16] = [
    0x97, 0x11, 0x32, 0x7c, 0x5a, 0x4b, 0x08, 0xdd, 0x9a, 0x44, 0x21, 0xb6, 0x52, 0x00, 0xc2, 0x5e,
];
const GEOMETRY: [u8; 16] = [
    0xa4, 0x26, 0x0c, 0xf1, 0x3b, 0x48, 0x84, 0x0a, 0xa3, 0xaf, 0x85, 0xaa, 0xad, 0xaf, 0xf6, 0x5d,
];
const PLANE: [u8; 16] = [
    0x42, 0xdf, 0x52, 0xce, 0xd0, 0x11, 0xd0, 0xd2, 0x00, 0x08, 0xcc, 0xbc, 0x06, 0x63, 0xdc, 0x09,
];
const FACE_SELECTOR: [u8; 16] = [
    0x68, 0x05, 0xba, 0x23, 0x32, 0x46, 0x11, 0xfa, 0xf6, 0x72, 0x5e, 0xbb, 0x87, 0xbd, 0xfd, 0x05,
];
const EDGE_SELECTOR: [u8; 16] = [
    0x02, 0x2d, 0xa9, 0x6c, 0xd2, 0x11, 0x7c, 0x10, 0x60, 0x00, 0x84, 0xb7, 0xb0, 0x35, 0xc3, 0xb0,
];
const NAMING_TABLE: [u8; 16] = [
    0x42, 0x20, 0xe9, 0xcc, 0xd1, 0x11, 0xc5, 0x71, 0x00, 0x08, 0xa7, 0xba, 0x32, 0xa3, 0xdc, 0x09,
];
const SWEEP_NAME: [u8; 16] = [
    0xa5, 0xc2, 0x45, 0x06, 0x28, 0x46, 0xbb, 0xe6, 0x1d, 0xa9, 0xed, 0xa1, 0x20, 0x72, 0xce, 0x35,
];
const GENERATED_NAME: [u8; 16] = [
    0x20, 0x5f, 0xa9, 0x56, 0xd1, 0x11, 0xaf, 0x49, 0x00, 0x08, 0x87, 0xba, 0x32, 0xa3, 0xdc, 0x09,
];
const HOLE_NAME: [u8; 16] = [
    0x7a, 0x26, 0x0e, 0xae, 0xd2, 0x11, 0x71, 0x5d, 0x60, 0x00, 0x7e, 0xb7, 0x12, 0x69, 0xf1, 0xb0,
];
const BLEND_NAME: [u8; 16] = [
    0x98, 0x41, 0x5d, 0x8e, 0xc3, 0x41, 0xc7, 0xa5, 0xad, 0xc5, 0xf9, 0x90, 0x46, 0x05, 0x79, 0xa4,
];
const EDGE_NAME: [u8; 16] = [
    0xa6, 0xe0, 0x32, 0xbf, 0xd1, 0x11, 0x46, 0x4a, 0x00, 0x08, 0x88, 0xba, 0x32, 0xa3, 0xdc, 0x09,
];

/// Internal centimetres per neutral millimetre.
const MILLIMETRES_PER_CENTIMETRE: f64 = 10.0;

/// Byte length of the fixed block between the fit texts and the parameter
/// references of a value record.
const VALUE_BLOCK_LEN: usize = 76;

/// Segment major versions whose annotation records follow this grammar.
pub(crate) const fn version_supported(version: u8) -> bool {
    matches!(version, 25 | 28)
}

pub(crate) struct Inputs<'a> {
    pub(crate) rse: &'a RseInventory<'a>,
    pub(crate) labels: &'a [PmDcFeatureLabel],
    pub(crate) parameters: &'a [PmDcParameter],
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Measure {
    Linear,
    Angular,
    Diameter,
    Radial,
    HoleNote,
}

impl Measure {
    fn of(type_id: [u8; 16]) -> Option<Self> {
        match type_id {
            LINEAR_OWNER => Some(Self::Linear),
            ANGULAR_OWNER => Some(Self::Angular),
            DIAMETER_OWNER => Some(Self::Diameter),
            RADIAL_OWNER => Some(Self::Radial),
            HOLE_NOTE_OWNER => Some(Self::HoleNote),
            _ => None,
        }
    }

    const fn kind(self) -> DimensionKind {
        match self {
            Self::Linear => DimensionKind::Location,
            Self::Angular => DimensionKind::Angular,
            Self::Diameter | Self::HoleNote => DimensionKind::Diameter,
            Self::Radial => DimensionKind::Radius,
        }
    }

    const fn quantity(self) -> PmiQuantity {
        match self {
            Self::Angular => PmiQuantity::Angle,
            Self::Linear | Self::Diameter | Self::Radial | Self::HoleNote => PmiQuantity::Length,
        }
    }

    const fn value_type(self) -> [u8; 16] {
        match self {
            Self::HoleNote => HOLE_VALUE,
            Self::Linear | Self::Angular | Self::Diameter | Self::Radial => VALUE,
        }
    }

    /// Value-record measure code of the owner kind.
    const fn code(self) -> u32 {
        match self {
            Self::Linear | Self::Angular => 0,
            Self::Diameter | Self::HoleNote => 1,
            Self::Radial => 2,
        }
    }
}

/// The owner fields that the projection reads.
#[derive(Debug, PartialEq, Eq)]
struct Owner {
    value: u32,
    intents: Vec<u32>,
}

/// The value-record fields that the projection reads.
#[derive(Debug, PartialEq)]
struct Value {
    measure: u32,
    tolerance: u32,
    display: u32,
    upper: f64,
    lower: f64,
    hole_fit: String,
    shaft_fit: String,
    nominal_parameter: u32,
}

/// A naming selector: its key, the naming table that its context names, and
/// the item that it names.
#[derive(Debug, PartialEq, Eq)]
struct Selector {
    face: bool,
    state: i32,
    table: u32,
    key: u32,
    item: u32,
}

/// One generated name: ordered `(source, name value)` pairs and the tail
/// bytes after them.
#[derive(Debug, PartialEq, Eq)]
struct GeneratedName {
    family: NameFamily,
    pairs: Vec<(i64, i64)>,
    tail: Vec<u8>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum NameFamily {
    Generated,
    Hole,
    Blend,
}

impl NameFamily {
    fn of(type_id: [u8; 16]) -> Option<Self> {
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
    fn record_tail(self, tail: &[u8]) -> Option<Vec<i64>> {
        let word = |offset: usize| View::u32_le_at(tail, offset).map(i64::from);
        if word(0)? != 0 {
            return None;
        }
        match self {
            Self::Generated => {
                if word(4)? != 0 {
                    return None;
                }
                Some(vec![word(8)?, word(12)?])
            }
            Self::Hole => Some(vec![word(4)?, i64::from(View::u16_le_at(tail, 8)?)]),
            Self::Blend => Some(vec![
                i64::from(*tail.get(4)?),
                i64::from(*tail.get(5)?),
                word(9)?,
            ]),
        }
    }

    /// The tail values of a generated-name attribute that its name record
    /// stores.
    fn attribute_tail(self, tail: &[i64]) -> Option<Vec<i64>> {
        match (self, tail) {
            (Self::Generated, [associative, 0, role, ..]) => Some(vec![*associative, *role]),
            (Self::Hole, [first, role, ..]) => Some(vec![*first, *role]),
            (Self::Blend, [first, second, third, ..]) => Some(vec![*first, *second, *third]),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
enum EntityKind {
    Face,
    Edge,
}

/// One generated-name attribute of the active body.
struct AttributeName<'m> {
    kind: EntityKind,
    entity: &'m str,
    family: NameFamily,
    tail: Vec<i64>,
}

/// The generated-name attributes of the active body, keyed by their pairs.
#[derive(Default)]
struct BodyNames<'m> {
    by_pairs: HashMap<Vec<(i64, i64)>, Vec<AttributeName<'m>>>,
    face_edges: HashMap<&'m str, HashSet<&'m str>>,
}

impl<'m> BodyNames<'m> {
    fn new(model: &'m Model) -> Self {
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
    fn select(&self, name: &GeneratedName, kind: EntityKind) -> Option<&'m str> {
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
    fn shared_edge(&self, first: &str, second: &str) -> Option<&'m str> {
        let first = self.face_edges.get(first)?;
        let second = self.face_edges.get(second)?;
        let mut shared = first.intersection(second);
        let edge = shared.next()?;
        shared.next().is_none().then_some(*edge)
    }
}

/// The family, pairs, and tail of a generated-name attribute.
type AttributeNameParts = (NameFamily, Vec<(i64, i64)>, Vec<i64>);

/// The family, pairs, and tail of a generated-name attribute.
fn attribute_name(name: &str, values: &[AttributeValue]) -> Option<AttributeNameParts> {
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

/// The records of one `PmDc` segment by ordinal.
struct Segment<'s, 'a> {
    token: &'s str,
    version: u8,
    records: HashMap<u32, &'s RseRecordFrame<'a>>,
}

impl<'a> Segment<'_, 'a> {
    fn record(&self, ordinal: u32, type_id: [u8; 16]) -> Option<View<'a>> {
        self.records
            .get(&ordinal)
            .filter(|record| record.type_id == type_id)
            .map(|record| record.payload)
    }

    fn target_type(&self, reference: PmDcReference) -> Option<[u8; 16]> {
        self.records
            .get(&reference.record_ordinal()?)
            .map(|record| record.type_id)
    }
}

/// The content header of an annotation record and its stamp.
fn header(cursor: &mut Cursor<'_>, version: u8) -> Result<(), CodecError> {
    state_content_header(cursor, version)?;
    match cursor.i32("annotation stamp")? {
        -1 => Ok(()),
        stamp => Err(CodecError::malformed(format_args!(
            "Inventor annotation stamp is {stamp}"
        ))),
    }
}

fn ordinal(reference: PmDcReference, field: &str) -> Result<u32, CodecError> {
    reference
        .record_ordinal()
        .ok_or_else(|| CodecError::malformed(format_args!("Inventor annotation {field} is null")))
}

fn owner(
    segment: &Segment<'_, '_>,
    payload: View<'_>,
    measure: Measure,
) -> Result<Owner, CodecError> {
    let mut cursor = Cursor::new(payload);
    header(&mut cursor, segment.version)?;
    cursor.u32("annotation attachment")?;
    let plane = cursor.reference("annotation plane")?;
    if segment.target_type(plane) != Some(PLANE) {
        return Err(CodecError::malformed(
            "Inventor annotation plane is not a work plane",
        ));
    }
    cursor.u32("annotation sequence")?;
    if segment.version >= 28 {
        let list = crate::pmdc::reference_list(&mut cursor, 2, "annotation list")?;
        if !list.references().is_empty() {
            return Err(CodecError::malformed(
                "Inventor annotation list is not empty",
            ));
        }
    }
    if measure == Measure::HoleNote {
        cursor.reference("hole-note definition")?;
        cursor.reference("hole-note feature")?;
    }
    cursor.reference("annotation layout")?;
    let value = cursor.reference("annotation value")?;
    if segment.target_type(value) != Some(measure.value_type()) {
        return Err(CodecError::malformed(
            "Inventor annotation value reference is not a value record",
        ));
    }
    let mut intents = Vec::new();
    let mut styles = 0usize;
    while cursor.remaining() >= 4 {
        let reference = cursor.reference("annotation operand")?;
        match segment.target_type(reference) {
            Some(INTENT) if styles == 0 => intents.push(ordinal(reference, "intent")?),
            Some(STYLE) => styles += 1,
            _ if measure == Measure::HoleNote && styles > 0 => break,
            _ => {
                return Err(CodecError::malformed(
                    "Inventor annotation operand is neither an intent nor a style",
                ))
            }
        }
    }
    Ok(Owner {
        value: ordinal(value, "value")?,
        intents,
    })
}

fn value(
    ctx: &DecodeContext<'_>,
    segment: &Segment<'_, '_>,
    payload: View<'_>,
    kind: Measure,
) -> Result<Value, CodecError> {
    let mut cursor = Cursor::new(payload);
    content_header(&mut cursor, segment.version)?;
    cursor.u32_array::<3>("value prefix")?;
    let measure = cursor.u32("value measure")?;
    let tolerance = cursor.u32("value tolerance form")?;
    cursor.u32_array::<6>("value controls")?;
    let display = cursor.u32("value display")?;
    cursor.u32_array::<2>("value display controls")?;
    let upper = cursor.f64("value upper tolerance")?.get();
    let lower = cursor.f64("value lower tolerance")?.get();
    cursor.u32_array::<2>("value tolerance controls")?;
    let hole_fit = cursor.utf16(ctx, "value hole fit")?;
    let shaft_fit = cursor.utf16(ctx, "value shaft fit")?;
    cursor.take_array::<VALUE_BLOCK_LEN>("value block")?;
    let nominal = cursor.reference("value nominal parameter")?;
    if segment.target_type(nominal) != Some(PARAMETER) {
        return Err(CodecError::malformed(
            "Inventor value nominal reference is not a parameter",
        ));
    }
    let (tolerance, upper, lower) = if kind == Measure::HoleNote {
        let hole = hole_tolerance(ctx, segment, cursor)?;
        if tolerance != 0 {
            return Err(CodecError::malformed(
                "Inventor hole-note value stores a dimension tolerance form",
            ));
        }
        hole
    } else {
        (tolerance, upper, lower)
    };
    Ok(Value {
        measure,
        tolerance,
        display,
        upper,
        lower,
        hole_fit,
        shaft_fit,
        nominal_parameter: ordinal(nominal, "nominal parameter")?,
    })
}

/// The diameter tolerance of a hole note, as a value-record tolerance form
/// with its deviations. The hole parameters are the first four consecutive
/// parameter references after the reference parameter.
fn hole_tolerance(
    ctx: &DecodeContext<'_>,
    segment: &Segment<'_, '_>,
    mut cursor: Cursor<'_>,
) -> Result<(u32, f64, f64), CodecError> {
    let reference = cursor.reference("hole-note reference parameter")?;
    if segment.target_type(reference) != Some(PARAMETER) {
        return Err(CodecError::malformed(
            "Inventor hole-note reference parameter is not a parameter",
        ));
    }
    let tail = cursor.view().unread();
    let parameter_at = |offset: usize| {
        View::u32_le_at(tail, offset)
            .map(PmDcReference::from_packed)
            .is_some_and(|reference| {
                reference.qualified() && segment.target_type(reference) == Some(PARAMETER)
            })
    };
    let start = (0..tail.len())
        .find(|offset| (0..4).all(|index| parameter_at(offset + 4 * index)))
        .ok_or_else(|| CodecError::malformed("Inventor hole note has no hole parameters"))?;
    let mut hole = Cursor::new(View::over_retained(tail.get(start..).unwrap_or_default()));
    for _ in 0..4 {
        hole.reference("hole-note parameter")?;
    }
    if hole.take_array::<16>("hole-note flags")? != HOLE_FLAGS {
        return Err(CodecError::malformed(
            "Inventor hole note is not a plain hole note",
        ));
    }
    let text = hole.utf16(ctx, "hole-note text")?;
    if !text.contains("<HDIA>") {
        return Err(CodecError::malformed(
            "Inventor hole note shows no hole diameter",
        ));
    }
    let form = hole.u32("hole-note tolerance form")?;
    hole.u32_array::<3>("hole-note tolerance controls")?;
    let upper = hole.f64("hole-note upper tolerance")?.get();
    let lower = hole.f64("hole-note lower tolerance")?.get();
    let form = match form {
        0 => 0,
        3 => 6,
        4 => 7,
        _ => u32::MAX,
    };
    Ok((form, upper, lower))
}

/// Byte length of the datum-modifier block of a frame row.
const FRAME_MODIFIERS_LEN: usize = 36;

/// The prefix of a frame or datum-identifier owner: its intent.
fn tolerance_owner(
    segment: &Segment<'_, '_>,
    payload: View<'_>,
) -> Result<Option<u32>, CodecError> {
    let mut cursor = Cursor::new(payload);
    header(&mut cursor, segment.version)?;
    cursor.u32("tolerance-owner attachment")?;
    let plane = cursor.reference("tolerance-owner plane")?;
    if segment.target_type(plane) != Some(PLANE) {
        return Err(CodecError::malformed(
            "Inventor tolerance owner plane is not a work plane",
        ));
    }
    cursor.u32("tolerance-owner sequence")?;
    let intent = cursor.reference("tolerance-owner intent")?;
    match (intent.record_ordinal(), segment.target_type(intent)) {
        (None, _) => Ok(None),
        (Some(intent), Some(INTENT)) => Ok(Some(intent)),
        _ => Err(CodecError::malformed(
            "Inventor tolerance-owner intent is not an intent",
        )),
    }
}

/// The record that the last u32 of an owner payload names, as an
/// unqualified one-based reference.
fn trailing_reference(payload: View<'_>) -> Option<u32> {
    let bytes = payload.window();
    let offset = bytes.len().checked_sub(4)?;
    PmDcReference::from_packed(View::u32_le_at(bytes, offset)?).record_ordinal()
}

/// The datum identifier owner and label of a datum feature.
fn datum_feature(
    ctx: &DecodeContext<'_>,
    segment: &Segment<'_, '_>,
    payload: View<'_>,
) -> Result<(u32, String), CodecError> {
    let mut cursor = Cursor::new(payload);
    content_header(&mut cursor, segment.version)?;
    let owners = crate::pmdc::reference_list(&mut cursor, 2, "datum-feature owner list")?;
    let [owner] = owners.references() else {
        return Err(CodecError::malformed(
            "Inventor datum feature names other than one owner",
        ));
    };
    if segment.version < 28 {
        crate::pmdc::reference_list(&mut cursor, 2, "datum-feature member list")?;
    }
    cursor.reference("datum-feature style")?;
    cursor.reference("datum-feature definition")?;
    let label = cursor.utf16(ctx, "datum label")?;
    Ok((ordinal(*owner, "datum-feature owner")?, label))
}

/// The fields of a frame row that the projection reads.
struct FrameRow {
    characteristic: u32,
    magnitude: u32,
    secondary: [u32; 2],
    datums: Vec<u32>,
    plain: bool,
}

fn frame_row(segment: &Segment<'_, '_>, payload: View<'_>) -> Result<FrameRow, CodecError> {
    let mut cursor = Cursor::new(payload);
    cursor.u32("frame header value")?;
    cursor.u16("frame header id")?;
    let characteristic = cursor.u32("frame characteristic")?;
    let magnitude = cursor.reference("frame magnitude")?;
    let flags = cursor.u32_array::<3>("frame zone modifiers")?;
    let secondary = [
        cursor.reference("frame second value")?,
        cursor.reference("frame third value")?,
    ];
    for reference in std::iter::once(magnitude).chain(secondary) {
        if segment.target_type(reference) != Some(PARAMETER) {
            return Err(CodecError::malformed(
                "Inventor frame value is not a parameter",
            ));
        }
    }
    let mut datums = Vec::new();
    for _ in 0..3 {
        let datum = cursor.reference("frame datum")?;
        if let Some(record) = datum.record_ordinal() {
            if segment.target_type(datum) != Some(DATUM_FEATURE) {
                return Err(CodecError::malformed(
                    "Inventor frame datum is not a datum feature",
                ));
            }
            datums.push(record);
        }
    }
    let modifiers = cursor.take_array::<FRAME_MODIFIERS_LEN>("frame datum modifiers")?;
    Ok(FrameRow {
        characteristic,
        magnitude: ordinal(magnitude, "frame magnitude")?,
        secondary: [
            ordinal(secondary[0], "frame second value")?,
            ordinal(secondary[1], "frame third value")?,
        ],
        datums,
        plain: flags == [0; 3] && modifiers == [0; FRAME_MODIFIERS_LEN],
    })
}

fn characteristic(code: u32) -> Option<GeometricToleranceKind> {
    Some(match code {
        1 => GeometricToleranceKind::Straightness,
        2 => GeometricToleranceKind::Flatness,
        3 => GeometricToleranceKind::Roundness,
        5 => GeometricToleranceKind::SurfaceProfile,
        7 => GeometricToleranceKind::Perpendicularity,
        9 => GeometricToleranceKind::Position,
        10 => GeometricToleranceKind::Concentricity,
        11 => GeometricToleranceKind::CircularRunout,
        12 => GeometricToleranceKind::Symmetry,
        13 => GeometricToleranceKind::TotalRunout,
        _ => return None,
    })
}

/// The selection items that a relation joins to an intent.
fn relation(segment: &Segment<'_, '_>, payload: View<'_>) -> Result<(u32, Vec<u32>), CodecError> {
    let mut cursor = Cursor::new(payload);
    header(&mut cursor, segment.version)?;
    if cursor.u32("relation link")? != 0 {
        return Err(CodecError::malformed("Inventor relation link is not null"));
    }
    let intent = cursor.reference("relation intent")?;
    if segment.target_type(intent) != Some(INTENT) {
        return Err(CodecError::malformed(
            "Inventor relation intent is not an intent",
        ));
    }
    let mut items = Vec::new();
    while cursor.remaining() >= 4 {
        let reference = cursor.reference("relation operand")?;
        if matches!(segment.target_type(reference), Some(EDGE_ITEM | FACE_ITEM)) {
            items.push(ordinal(reference, "relation item")?);
        }
    }
    Ok((ordinal(intent, "relation intent")?, items))
}

/// The selection item and stored geometry that a geometry link joins.
fn geometry_link(segment: &Segment<'_, '_>, payload: View<'_>) -> Result<(u32, u32), CodecError> {
    let mut cursor = Cursor::new(payload);
    header(&mut cursor, segment.version)?;
    let geometry = cursor.reference("geometry-link geometry")?;
    let plane = cursor.reference("geometry-link plane")?;
    let item = cursor.reference("geometry-link item")?;
    cursor.finish("geometry link")?;
    if segment.target_type(geometry) != Some(GEOMETRY)
        || segment.target_type(plane) != Some(PLANE)
        || !matches!(segment.target_type(item), Some(EDGE_ITEM | FACE_ITEM))
    {
        return Err(CodecError::malformed(
            "Inventor geometry link references unexpected record types",
        ));
    }
    Ok((
        ordinal(item, "geometry-link item")?,
        ordinal(geometry, "geometry")?,
    ))
}

fn selector(
    ctx: &DecodeContext<'_>,
    segment: &Segment<'_, '_>,
    payload: View<'_>,
    face: bool,
) -> Result<Selector, CodecError> {
    let mut cursor = Cursor::new(payload);
    state_content_header(&mut cursor, segment.version)?;
    let state = cursor.i32("selector state")?;
    cursor.u8("selector enabled")?;
    cursor.u8("selector form")?;
    cursor.reference("selector body")?;
    cursor.u32("selector topology")?;
    cursor.u32("selector qualifier")?;
    cursor.u32("selector naming form")?;
    u32_list_with_context(ctx, &mut cursor, 8, "selector generators")?;
    u32_list_with_context(ctx, &mut cursor, 8, "selector secondary names")?;
    if cursor.u8("selector separator")? != 0 {
        return Err(CodecError::malformed(
            "Inventor selector separator is not 0",
        ));
    }
    let (table, key, item) = if face {
        let table = cursor.reference("selector context")?;
        let key = cursor.u32("selector key")?;
        let count = cursor.u32("selector entity count")?;
        for _ in 0..count {
            cursor.reference("selector entity context")?;
            cursor.u32("selector entity source")?;
        }
        let item = cursor.reference("selector item")?;
        cursor.finish("face selector")?;
        (table, key, item)
    } else {
        let item = cursor.reference("selector item")?;
        let table = cursor.reference("selector context")?;
        let key = cursor.u32("selector key")?;
        if cursor.remaining() > 1 {
            return Err(CodecError::malformed(
                "Inventor edge selector has trailing bytes",
            ));
        }
        (table, key, item)
    };
    if segment.target_type(table) != Some(NAMING_TABLE) {
        return Err(CodecError::malformed(
            "Inventor selector context is not a naming table",
        ));
    }
    Ok(Selector {
        face,
        state,
        table: ordinal(table, "selector context")?,
        key,
        item: ordinal(item, "selector item")?,
    })
}

/// The entries of a naming table: name identifier to record ordinal.
fn naming_table(payload: View<'_>) -> Result<HashMap<u32, u32>, CodecError> {
    let mut cursor = Cursor::new(payload);
    cursor.u32("naming-table header value")?;
    cursor.u16("naming-table header id")?;
    cursor.u32("naming-table next")?;
    cursor.u32("naming-table last identifier")?;
    let count = cursor.u32("naming-table entry count")?;
    let mut entries = HashMap::new();
    for _ in 0..count {
        let id = cursor.u32("naming-table identifier")?;
        let record = cursor.reference("naming-table record")?;
        if entries
            .insert(id, ordinal(record, "naming-table record")?)
            .is_some()
        {
            return Err(CodecError::malformed(
                "Inventor naming table repeats an identifier",
            ));
        }
    }
    cursor.finish("naming table")?;
    Ok(entries)
}

/// The common prefix of a name record, up to its first list.
fn name_prefix(cursor: &mut Cursor<'_>) -> Result<(), CodecError> {
    cursor.u32("name header value")?;
    cursor.u16("name header id")?;
    cursor.u32("name form")?;
    if cursor.u32("name signature")? != 0x3000_0002 {
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

fn generated_name(family: NameFamily, payload: View<'_>) -> Result<GeneratedName, CodecError> {
    let mut cursor = Cursor::new(payload);
    name_prefix(&mut cursor)?;
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
            "Inventor name does not end in name value -1",
        ));
    }
    Ok(GeneratedName {
        family,
        pairs,
        tail: cursor.view().unread().to_vec(),
    })
}

/// The two face names of an edge name.
fn edge_name(payload: View<'_>) -> Result<Option<[u32; 2]>, CodecError> {
    let mut cursor = Cursor::new(payload);
    name_prefix(&mut cursor)?;
    cursor.u32_array::<4>("edge-name base")?;
    cursor.u8("edge-name flag")?;
    let list = crate::pmdc::reference_list(&mut cursor, 2, "edge-name list")?;
    if !list.references().is_empty() {
        return Ok(None);
    }
    cursor.u32("edge-name control")?;
    let count = cursor.u32("edge-name face count")?;
    if count != 2 {
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

/// Per-segment indexes from intents to selectors.
#[derive(Default)]
struct Joins {
    items: HashMap<u32, Vec<u32>>,
    geometry: HashMap<u32, Vec<u32>>,
    selectors: HashMap<u32, Vec<Selector>>,
    tables: HashMap<u32, HashMap<u32, u32>>,
}

impl Joins {
    fn new(ctx: &DecodeContext<'_>, segment: &Segment<'_, '_>) -> Self {
        let mut joins = Self::default();
        for (&record_ordinal, record) in &segment.records {
            match record.type_id {
                SIZE_RELATION | FACE_RELATION | PAIR_RELATION => {
                    if let Ok((intent, items)) = relation(segment, record.payload) {
                        joins.items.entry(intent).or_default().extend(items);
                    }
                }
                GEOMETRY_LINK => {
                    if let Ok((item, geometry)) = geometry_link(segment, record.payload) {
                        joins.geometry.entry(item).or_default().push(geometry);
                    }
                }
                FACE_SELECTOR | FEATURE_FACE_SELECTOR | EDGE_SELECTOR => {
                    let face = record.type_id != EDGE_SELECTOR;
                    if let Ok(selector) = selector(ctx, segment, record.payload, face) {
                        joins
                            .selectors
                            .entry(selector.item)
                            .or_default()
                            .push(selector);
                    }
                }
                NAMING_TABLE => {
                    if let Ok(table) = naming_table(record.payload) {
                        joins.tables.insert(record_ordinal, table);
                    }
                }
                _ => {}
            }
        }
        joins
    }

    /// The selectors that name a selection item directly or through its
    /// stored geometry.
    fn item_selectors(&self, item: u32) -> Vec<&Selector> {
        let mut found = Vec::new();
        found.extend(self.selectors.get(&item).into_iter().flatten());
        for geometry in self.geometry.get(&item).into_iter().flatten() {
            found.extend(self.selectors.get(geometry).into_iter().flatten());
        }
        found
    }
}

/// The joins of one segment that a dimension projection reads.
struct Scope<'s, 'a, 'm> {
    segment: &'s Segment<'s, 'a>,
    /// The tolerance feature whose label lists each annotation owner.
    features: &'s HashMap<u32, u32>,
    joins: &'s Joins,
    parameters: &'s HashMap<u32, f64>,
    names: &'s BodyNames<'m>,
}

#[derive(Default)]
struct Outcome {
    dimensions: usize,
    frames: usize,
    frames_native: usize,
    datum_systems_unresolved: usize,
    unresolved_targets: usize,
    malformed: usize,
    tolerance_unresolved: usize,
}

pub(crate) fn project(
    ctx: &DecodeContext<'_>,
    inputs: &Inputs<'_>,
    model: &mut Model,
) -> Result<Vec<LossNote>, CodecError> {
    let names = BodyNames::new(model);
    let mut outcome = Outcome::default();
    let mut annotations = Vec::new();
    for rse_segment in &inputs.rse.segments {
        if rse_segment.kind != SegmentKind::PmDc {
            continue;
        }
        let Some(version) = rse_segment.registry.map(|join| join.version_major) else {
            continue;
        };
        if !version_supported(version) {
            continue;
        }
        let SegmentBulkState::Framed(bulk) = &rse_segment.bulk else {
            continue;
        };
        let RecordFrameState::Framed(table) = &bulk.records else {
            continue;
        };
        let segment = Segment {
            token: rse_segment.pair.token.as_str(),
            version,
            records: table
                .records
                .iter()
                .map(|record| (record.ordinal, record))
                .collect(),
        };
        let token_key = rse_segment.pair.token.key();
        let parameters = inputs
            .parameters
            .iter()
            .filter(|parameter| &parameter.identity.segment_token == token_key)
            .map(|parameter| {
                (
                    parameter.identity.record_ordinal,
                    parameter.model_value.get(),
                )
            })
            .collect::<HashMap<_, _>>();
        let joins = Joins::new(ctx, &segment);
        let mut features = HashMap::new();
        for label in inputs.labels {
            if &label.identity.segment_token != token_key {
                continue;
            }
            let Some(feature) = label.header.owner.record_ordinal() else {
                continue;
            };
            if segment.record(feature, TOLERANCE_FEATURE).is_none() {
                continue;
            }
            for participant in label.participants.references() {
                if let Some(owner) = participant.record_ordinal() {
                    features.insert(owner, feature);
                }
            }
        }
        let scope = Scope {
            segment: &segment,
            features: &features,
            joins: &joins,
            parameters: &parameters,
            names: &names,
        };
        let names_by_owner = inputs
            .labels
            .iter()
            .filter(|label| &label.identity.segment_token == token_key)
            .filter_map(|label| {
                Some((
                    label.header.owner.record_ordinal()?,
                    label.name().map(str::to_owned),
                ))
            })
            .collect::<HashMap<_, _>>();
        let (datum_annotations, datum_ids) = datums(ctx, &scope, &names_by_owner, &mut outcome)?;
        annotations.extend(datum_annotations);
        for label in inputs.labels {
            if &label.identity.segment_token != token_key {
                continue;
            }
            let Some(owner_ordinal) = label.header.owner.record_ordinal() else {
                continue;
            };
            let Some(record) = segment.records.get(&owner_ordinal) else {
                continue;
            };
            if record.type_id == FRAME_OWNER {
                annotations.extend(frame(
                    &scope,
                    &datum_ids,
                    (record.payload, owner_ordinal),
                    label.name(),
                    &mut outcome,
                )?);
                continue;
            }
            let Some(measure) = Measure::of(record.type_id) else {
                continue;
            };
            let Some(annotation) = dimension(
                ctx,
                &scope,
                measure,
                (record.payload, owner_ordinal),
                label.name(),
                &mut outcome,
            )?
            else {
                continue;
            };
            annotations.push(annotation);
        }
    }
    ctx.charge_entities(
        cadmpeg_core::decode::u64_from_index(annotations.len()),
        "Inventor part annotations",
    )?;
    model.pmi.extend(annotations);
    model
        .pmi
        .sort_by(|left, right| left.id.as_str().cmp(right.id.as_str()));
    Ok(losses(&outcome))
}

fn dimension(
    ctx: &DecodeContext<'_>,
    scope: &Scope<'_, '_, '_>,
    measure: Measure,
    (payload, owner_ordinal): (View<'_>, u32),
    name: Option<&str>,
    outcome: &mut Outcome,
) -> Result<Option<PmiAnnotation>, CodecError> {
    let segment = scope.segment;
    let Ok(owner) = owner(segment, payload, measure) else {
        outcome.malformed += 1;
        return Ok(None);
    };
    let Some(value_payload) = segment.record(owner.value, measure.value_type()) else {
        outcome.malformed += 1;
        return Ok(None);
    };
    let Ok(value) = value(ctx, segment, value_payload, measure) else {
        outcome.malformed += 1;
        return Ok(None);
    };
    let Some(&model_value) = scope.parameters.get(&value.nominal_parameter) else {
        outcome.malformed += 1;
        return Ok(None);
    };
    if value.measure != measure.code() {
        outcome.malformed += 1;
        return Ok(None);
    }
    let scale = match measure.quantity() {
        PmiQuantity::Length => MILLIMETRES_PER_CENTIMETRE,
        PmiQuantity::Angle | PmiQuantity::Ratio => 1.0,
    };
    let quantity = measure.quantity();
    let nominal = PmiValue::new(model_value * scale, quantity);
    let Tolerance {
        bounds,
        modifier,
        resolved,
    } = tolerance(&value, model_value, scale, quantity);
    if !resolved {
        outcome.tolerance_unresolved += 1;
    }
    let mut modifiers = Vec::new();
    modifiers.extend(modifier.map(str::to_owned));
    match value.display {
        1 => modifiers.push("theoretical".to_owned()),
        2 => modifiers.push("auxiliary".to_owned()),
        _ => {}
    }
    let definition = PmiDimension::new(measure.kind(), nominal, bounds)
        .and_then(|definition| definition.with_modifiers(modifiers))
        .map_err(|error| CodecError::malformed(format_args!("Inventor dimension: {error}")))?;
    let targets = targets(scope, &owner.intents, outcome);
    outcome.dimensions += 1;
    Ok(Some(PmiAnnotation {
        id: annotation_id(segment, owner_ordinal)?,
        name: name.map(str::to_owned),
        visible: None,
        targets,
        definition: PmiDefinition::Dimension(definition),
    }))
}

/// The faces of a tolerance feature: the faces that the selectors of the
/// face items of its face collections select, and those of its member
/// tolerance features. References in the feature payload are read at the
/// four-byte positions of its label reference.
fn feature_faces(
    scope: &Scope<'_, '_, '_>,
    feature: u32,
    depth: usize,
    outcome: &mut Outcome,
) -> Vec<PmiTarget> {
    let segment = scope.segment;
    let Some(payload) = segment.record(feature, TOLERANCE_FEATURE) else {
        return Vec::new();
    };
    let bytes = payload.window();
    let mut faces = Vec::new();
    for offset in (6..bytes.len()).step_by(4) {
        let Some(reference) = View::u32_le_at(bytes, offset).map(PmDcReference::from_packed) else {
            continue;
        };
        if !reference.qualified() {
            continue;
        }
        let Some(ordinal) = reference.record_ordinal() else {
            continue;
        };
        match segment.target_type(reference) {
            Some(FACE_COLLECTION) => {
                let Some(item) = segment
                    .record(ordinal, FACE_COLLECTION)
                    .and_then(|collection| collection_item(segment, collection))
                else {
                    outcome.unresolved_targets += 1;
                    continue;
                };
                let selectors = scope.joins.item_selectors(item);
                if selectors.is_empty() {
                    outcome.unresolved_targets += 1;
                }
                for selector in selectors {
                    match selector_target(segment, scope.joins, scope.names, selector) {
                        Some(face) if !faces.contains(&face) => faces.push(face),
                        Some(_) => {}
                        None => outcome.unresolved_targets += 1,
                    }
                }
            }
            Some(TOLERANCE_FEATURE) if depth == 0 => {
                for face in feature_faces(scope, ordinal, depth + 1, outcome) {
                    if !faces.contains(&face) {
                        faces.push(face);
                    }
                }
            }
            _ => {}
        }
    }
    faces
}

/// The face item that a face collection names in its last four bytes.
fn collection_item(segment: &Segment<'_, '_>, payload: View<'_>) -> Option<u32> {
    let bytes = payload.window();
    let reference =
        PmDcReference::from_packed(View::u32_le_at(bytes, bytes.len().checked_sub(4)?)?);
    (segment.target_type(reference) == Some(FACE_SELECTION_ITEM))
        .then(|| reference.record_ordinal())
        .flatten()
}

/// The targets of a frame or datum owner: those of its intent, or else the
/// faces of the tolerance feature that lists it.
fn owner_targets(
    scope: &Scope<'_, '_, '_>,
    owner: u32,
    intents: &[u32],
    outcome: &mut Outcome,
) -> Vec<PmiTarget> {
    let mut intent_outcome = Outcome::default();
    let found = targets(scope, intents, &mut intent_outcome);
    if found.is_empty() {
        if let Some(feature) = scope.features.get(&owner) {
            return feature_faces(scope, *feature, 0, outcome);
        }
    }
    outcome.unresolved_targets += intent_outcome.unresolved_targets;
    found
}

fn annotation_id(segment: &Segment<'_, '_>, owner: u32) -> Result<PmiId, CodecError> {
    PmiId::try_from(format!("inventor:annotation:pmi#{}-{owner}", segment.token))
        .map_err(|_| CodecError::malformed("invalid Inventor annotation identity"))
}

/// The distinct targets of the intents of an owner.
fn targets(scope: &Scope<'_, '_, '_>, intents: &[u32], outcome: &mut Outcome) -> Vec<PmiTarget> {
    let mut targets = Vec::new();
    for intent in intents {
        let resolved = intent_targets(scope.segment, scope.joins, scope.names, *intent);
        if resolved.is_empty() {
            outcome.unresolved_targets += 1;
        }
        for target in resolved {
            match target {
                Some(target) if !targets.contains(&target) => targets.push(target),
                Some(_) => {}
                None => outcome.unresolved_targets += 1,
            }
        }
    }
    targets
}

/// The tolerance of a value record and its single-limit modifier.
struct Tolerance {
    bounds: Option<DimensionTolerance>,
    modifier: Option<&'static str>,
    /// The tolerance form is one of the established codes.
    resolved: bool,
}

fn tolerance(value: &Value, model_value: f64, scale: f64, quantity: PmiQuantity) -> Tolerance {
    let at = |internal: f64| PmiValue::new(internal * scale, quantity);
    let mut modifier = None;
    let tolerance = match value.tolerance {
        0 => Some(None),
        6 => at(-value.upper.abs())
            .zip(at(value.upper.abs()))
            .map(|(lower, upper)| Some(DimensionTolerance::PlusMinus { lower, upper })),
        7 => at(value.lower)
            .zip(at(value.upper))
            .map(|(lower, upper)| Some(DimensionTolerance::PlusMinus { lower, upper })),
        3 => at(model_value + value.lower)
            .zip(at(model_value + value.upper))
            .map(|(lower, upper)| Some(DimensionTolerance::Limits { lower, upper })),
        2 => at(value.lower)
            .zip(at(value.upper))
            .zip(fit(&value.hole_fit, &value.shaft_fit))
            .map(|((lower, upper), fit)| {
                Some(DimensionTolerance::PlusMinusFit { lower, upper, fit })
            }),
        4 => {
            modifier = Some("maximum");
            Some(None)
        }
        _ => None,
    };
    Tolerance {
        resolved: tolerance.is_some(),
        bounds: tolerance.flatten(),
        modifier,
    }
}

/// An ISO 286 tolerance class: fundamental-deviation letters and grade.
fn fit(hole: &str, shaft: &str) -> Option<LimitsAndFits> {
    let (class, zone) = match (hole.is_empty(), shaft.is_empty()) {
        (false, true) => (hole, "hole"),
        (true, false) => (shaft, "shaft"),
        _ => return None,
    };
    let split = class.find(|character: char| character.is_ascii_digit())?;
    let (letters, grade) = class.split_at(split);
    let valid_letters = if zone == "hole" {
        letters
            .chars()
            .all(|character| character.is_ascii_uppercase())
    } else {
        letters
            .chars()
            .all(|character| character.is_ascii_lowercase())
    };
    (!letters.is_empty()
        && valid_letters
        && grade.chars().all(|character| character.is_ascii_digit()))
    .then(|| LimitsAndFits {
        form_variance: letters.to_owned(),
        zone_variance: zone.to_owned(),
        grade: grade.to_owned(),
        source: String::new(),
    })
}

/// The faces or edges that the selectors of an intent name, one for each
/// selection item. `None` marks an item without a unique target.
fn intent_targets(
    segment: &Segment<'_, '_>,
    joins: &Joins,
    names: &BodyNames<'_>,
    intent: u32,
) -> Vec<Option<PmiTarget>> {
    joins
        .items
        .get(&intent)
        .into_iter()
        .flatten()
        .map(|item| {
            let mut found: Option<PmiTarget> = None;
            for selector in joins.item_selectors(*item) {
                let target = selector_target(segment, joins, names, selector)?;
                match &found {
                    None => found = Some(target),
                    Some(previous) if *previous == target => {}
                    Some(_) => return None,
                }
            }
            found
        })
        .collect()
}

fn selector_target(
    segment: &Segment<'_, '_>,
    joins: &Joins,
    names: &BodyNames<'_>,
    selector: &Selector,
) -> Option<PmiTarget> {
    let table = joins.tables.get(&selector.table)?;
    if selector.face {
        if selector.state == 0 {
            return None;
        }
        let face = name_entity(segment, table, names, selector.key, EntityKind::Face)?;
        return Some(PmiTarget::Face {
            face: FaceId::mint(face).ok()?,
        });
    }
    let record = *table.get(&selector.key)?;
    let faces = edge_name(segment.record(record, EDGE_NAME)?).ok()??;
    let first = name_entity(segment, table, names, faces[0], EntityKind::Face)?;
    let second = name_entity(segment, table, names, faces[1], EntityKind::Face)?;
    let edge = names.shared_edge(first, second)?;
    Some(PmiTarget::Edge {
        edge: EdgeId::mint(edge).ok()?,
    })
}

fn name_entity<'m>(
    segment: &Segment<'_, '_>,
    table: &HashMap<u32, u32>,
    names: &BodyNames<'m>,
    key: u32,
    kind: EntityKind,
) -> Option<&'m str> {
    let record = segment.records.get(table.get(&key)?)?;
    let family = NameFamily::of(record.type_id)?;
    let name = generated_name(family, record.payload).ok()?;
    names.select(&name, kind)
}

/// The datum identifier owner, label, and targets of each datum feature, and
/// the datum annotations.
fn datums(
    ctx: &DecodeContext<'_>,
    scope: &Scope<'_, '_, '_>,
    labels: &HashMap<u32, Option<String>>,
    outcome: &mut Outcome,
) -> Result<(Vec<PmiAnnotation>, HashMap<u32, PmiId>), CodecError> {
    let segment = scope.segment;
    let mut annotations = Vec::new();
    let mut ids = HashMap::new();
    let mut features: Vec<_> = segment
        .records
        .iter()
        .filter(|(_, record)| record.type_id == DATUM_FEATURE)
        .map(|(ordinal, record)| (*ordinal, record.payload))
        .collect();
    features.sort_by_key(|(ordinal, _)| *ordinal);
    for (feature, payload) in features {
        let Ok((owner, label)) = datum_feature(ctx, segment, payload) else {
            outcome.malformed += 1;
            continue;
        };
        let Some(owner_payload) = segment.record(owner, DATUM_OWNER) else {
            outcome.malformed += 1;
            continue;
        };
        let Ok(intent) = tolerance_owner(segment, owner_payload) else {
            outcome.malformed += 1;
            continue;
        };
        if label.is_empty() {
            outcome.malformed += 1;
            continue;
        }
        let id = annotation_id(segment, owner)?;
        let targets = owner_targets(scope, owner, intent.as_slice(), outcome);
        ids.insert(feature, id.clone());
        annotations.push(PmiAnnotation {
            id,
            name: labels.get(&owner).cloned().flatten(),
            visible: None,
            targets,
            definition: PmiDefinition::Datum {
                identification: label,
            },
        });
    }
    Ok((annotations, ids))
}

/// A geometric tolerance and its datum system from a frame owner.
fn frame(
    scope: &Scope<'_, '_, '_>,
    datum_ids: &HashMap<u32, PmiId>,
    (payload, owner_ordinal): (View<'_>, u32),
    name: Option<&str>,
    outcome: &mut Outcome,
) -> Result<Vec<PmiAnnotation>, CodecError> {
    let segment = scope.segment;
    let Ok(intent) = tolerance_owner(segment, payload) else {
        outcome.malformed += 1;
        return Ok(Vec::new());
    };
    let Some(row_payload) =
        trailing_reference(payload).and_then(|row| segment.record(row, FRAME_ROW))
    else {
        outcome.malformed += 1;
        return Ok(Vec::new());
    };
    let Ok(row) = frame_row(segment, row_payload) else {
        outcome.malformed += 1;
        return Ok(Vec::new());
    };
    let magnitude = scope.parameters.get(&row.magnitude).copied();
    let secondary_zero = row
        .secondary
        .iter()
        .all(|parameter| scope.parameters.get(parameter) == Some(&0.0));
    let (Some(kind), Some(magnitude), true, true) = (
        characteristic(row.characteristic),
        magnitude,
        row.plain,
        secondary_zero,
    ) else {
        outcome.frames_native += 1;
        return Ok(Vec::new());
    };
    let Some(magnitude) =
        PmiValue::new(magnitude * MILLIMETRES_PER_CENTIMETRE, PmiQuantity::Length)
            .and_then(PmiMagnitude::new)
    else {
        outcome.frames_native += 1;
        return Ok(Vec::new());
    };
    let mut annotations = Vec::new();
    let datum_system = if row.datums.is_empty() {
        if segment.version >= 28 && row_has_frame_reference(row_payload) {
            outcome.datum_systems_unresolved += 1;
        }
        None
    } else {
        let mut references = Vec::new();
        for (datum, precedence) in row.datums.iter().zip(1_u32..) {
            let Some(datum) = datum_ids.get(datum) else {
                outcome.datum_systems_unresolved += 1;
                references.clear();
                break;
            };
            let precedence = std::num::NonZeroU32::new(precedence)
                .ok_or_else(|| CodecError::malformed("Inventor datum precedence is zero"))?;
            references.push(DatumReference {
                datum: datum.clone(),
                precedence,
                common_group: None,
                modifiers: Vec::new(),
            });
        }
        if references.is_empty() {
            None
        } else {
            let id = PmiId::try_from(format!(
                "inventor:annotation:pmi#{}-{owner_ordinal}-datums",
                segment.token
            ))
            .map_err(|_| CodecError::malformed("invalid Inventor datum-system identity"))?;
            let references = DatumReferences::try_from(references).map_err(|error| {
                CodecError::malformed(format_args!("Inventor datum system: {error}"))
            })?;
            annotations.push(PmiAnnotation {
                id: id.clone(),
                name: None,
                visible: None,
                targets: Vec::new(),
                definition: PmiDefinition::DatumSystem { references },
            });
            Some(id)
        }
    };
    annotations.push(PmiAnnotation {
        id: annotation_id(segment, owner_ordinal)?,
        name: name.map(str::to_owned),
        visible: None,
        targets: owner_targets(scope, owner_ordinal, intent.as_slice(), outcome),
        definition: PmiDefinition::GeometricTolerance {
            tolerance: kind,
            magnitude,
            defined_unit: None,
            defined_area_unit: None,
            defined_area_second_unit: None,
            datum_system,
            modifiers: Vec::new(),
            constraints: ToleranceConstraints::default(),
        },
    });
    outcome.frames += 1;
    Ok(annotations)
}

/// Whether the trailing reference of a version-28 frame row names a datum
/// reference frame.
fn row_has_frame_reference(payload: View<'_>) -> bool {
    trailing_reference(payload).is_some()
}

fn losses(outcome: &Outcome) -> Vec<LossNote> {
    let mut losses = Vec::new();
    if outcome.unresolved_targets > 0 {
        losses.push(InventorLossCode::PmiTargetUnresolved.note(format!(
            "{} dimension geometry intents name no unique active-body face or edge; their dimensions carry no target for them",
            outcome.unresolved_targets
        )));
    }
    if outcome.tolerance_unresolved > 0 {
        losses.push(InventorLossCode::PmiToleranceUnresolved.note(format!(
            "{} dimensions store a tolerance form outside the established codes; they transfer without a tolerance",
            outcome.tolerance_unresolved
        )));
    }
    if outcome.frames_native > 0 {
        losses.push(InventorLossCode::PmiFrameNative.note(format!(
            "{} feature control frames store a characteristic code, zone or datum modifiers, or further values outside the established grammar; they stay native",
            outcome.frames_native
        )));
    }
    if outcome.datum_systems_unresolved > 0 {
        losses.push(InventorLossCode::PmiDatumSystemUnresolved.note(format!(
            "{} feature control frames name datums through a datum reference frame or an unresolved datum feature; they transfer without a datum system",
            outcome.datum_systems_unresolved
        )));
    }
    if outcome.malformed > 0 {
        losses.push(InventorLossCode::PmiRecordMalformed.note(format!(
            "{} dimension owners or value records are outside the established grammar; they stay native",
            outcome.malformed
        )));
    }
    losses
}

#[cfg(test)]
mod tests;
