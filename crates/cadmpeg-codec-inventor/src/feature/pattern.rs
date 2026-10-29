// SPDX-License-Identifier: Apache-2.0
//! Rectangular-pattern operands and bounded neutral projection.

use super::{
    f64_triple, history, history_feature_id, length_reference, native_feature, resolve_property,
    result_from_collection, PmDcFeatureLabel, PmDcFeatureProperty, PmDcFeaturePropertyKind,
    PmDcPatternFamily, PmDcPatternFeature, ProjectionIndex, WORK_AXIS_TYPE,
};
use crate::layout::{
    pm_dc_content_header as header, pm_dc_content_header_v28 as header_v28,
    pm_dc_pattern_cached_edge_body as cached_edge, pm_dc_pattern_cached_line_body as cached_line,
    pm_dc_pattern_selection_control_body as selection_control,
    pm_dc_pattern_suppression_pair as suppression_pair,
    pm_dc_pattern_suppression_prefix as suppression,
};
use crate::pmdc::{content_header, state_content_header, Cursor};
use crate::record_identity::{Located, RecordPayload, RecordTypeId};
use cadmpeg_core::decode::{DecodeContext, View};
use cadmpeg_core::CodecError;
use cadmpeg_ir::features::patterns::{
    LinearPatternDirection, PatternKind, PatternSeed, PatternTransform,
};
use cadmpeg_ir::features::FeatureDirection3;
use cadmpeg_ir::features::{Feature, FeatureDefinition, FeatureOperation, FeatureResultTopology};
use cadmpeg_ir::math::Vector3;
use cadmpeg_ir::scalar::PositiveLength;
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};

const SUPPRESSION_TYPE: [u8; 16] = [
    0xbe, 0xf0, 0x6e, 0x8d, 0x35, 0x4c, 0xc7, 0xaa, 0xd7, 0xc8, 0x8d, 0xb7, 0xea, 0x9b, 0x0f, 0xfb,
];
const SELECTION_CONTROL_TYPE: [u8; 16] = [
    0x5a, 0x4a, 0x68, 0x99, 0xd2, 0x11, 0x7f, 0xec, 0x60, 0x00, 0xb0, 0xab, 0xc8, 0x43, 0x3c, 0xb0,
];
const EDGE_DIRECTION_TYPE: [u8; 16] = [
    0x7b, 0x45, 0x44, 0xa2, 0xab, 0x4d, 0x9e, 0x16, 0x85, 0x0c, 0x48, 0xa2, 0x1e, 0xdf, 0x9e, 0x3a,
];
const LINE_DIRECTION_TYPE: [u8; 16] = [
    0xaa, 0x7b, 0x97, 0xa5, 0x70, 0x45, 0x79, 0x58, 0x97, 0xb5, 0x13, 0x96, 0xff, 0xc5, 0xd6, 0x31,
];
const IDENTITY_PLACEMENT: [[f64; 4]; 4] = [
    [1.0, 0.0, 0.0, 0.0],
    [0.0, 1.0, 0.0, 0.0],
    [0.0, 0.0, 1.0, 0.0],
    [0.0, 0.0, 0.0, 1.0],
];

#[derive(Debug, Default)]
pub(crate) struct Operands {
    records: HashMap<(String, u32), Option<Operand>>,
    raw: HashMap<(String, u32), Option<[u8; 16]>>,
    lengths: HashMap<(String, u32), u64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "form", rename_all = "snake_case")]
pub(crate) enum Operand {
    Unresolved,
    Suppression {
        pairs: Vec<[u32; 2]>,
    },
    SelectionControl {
        references: [crate::pmdc::PmDcReference; 2],
        value: u8,
    },
    Line {
        state: i32,
        reverse: crate::pmdc::PmDcReference,
        geometry: crate::pmdc::PmDcReference,
        links: Vec<crate::pmdc::PmDcReference>,
        span: Vector3,
    },
}

pub(super) fn is_operand(type_id: [u8; 16]) -> bool {
    matches!(
        type_id,
        SUPPRESSION_TYPE | SELECTION_CONTROL_TYPE | EDGE_DIRECTION_TYPE | LINE_DIRECTION_TYPE
    )
}

impl RecordPayload for Operand {
    const KIND: &'static str = "pattern-operand";
}

pub(crate) type NativeOperand = Located<Operand>;

impl Operands {
    pub(crate) fn native_records(&self) -> Result<Vec<NativeOperand>, CodecError> {
        let mut records = Vec::new();
        for ((token, ordinal), operand) in &self.records {
            let (Some(operand), Some(Some(type_id))) =
                (operand, self.raw.get(&(token.clone(), *ordinal)))
            else {
                continue;
            };
            records.push(Located::new(
                operand.clone(),
                RecordTypeId::from_bytes(*type_id),
                cadmpeg_ir::ids::IdentityKey::try_new(token.clone())
                    .map_err(CodecError::malformed)?,
                *ordinal,
            ));
        }
        records.sort_by_key(Located::id);
        Ok(records)
    }

    pub(crate) fn from_native(
        records: &[NativeOperand],
        raw: &[crate::native::RseRecordRecord],
    ) -> Self {
        let mut operands = Self::default();
        for record in raw {
            let key = (record.token.as_str().to_owned(), record.ordinal);
            operands.lengths.insert(key.clone(), record.payload_len());
            let type_id = hex_type(&record.type_id);
            operands
                .raw
                .entry(key)
                .and_modify(|value| *value = None)
                .or_insert(type_id);
        }
        for record in records {
            let key = (
                record.identity.segment_token.as_str().to_owned(),
                record.identity.record_ordinal,
            );
            let type_id = hex_type(record.identity.type_id.as_str());
            let value = if operands.raw.get(&key) == Some(&type_id)
                && type_id.is_some_and(|type_id| operand_type(record, type_id))
            {
                Some((**record).clone())
            } else {
                None
            };
            operands
                .records
                .entry(key)
                .and_modify(|value| *value = None)
                .or_insert(value);
        }
        operands
    }

    pub(crate) fn matches_record(&self, record: &NativeOperand) -> bool {
        let key = (
            record.identity.segment_token.as_str().to_owned(),
            record.identity.record_ordinal,
        );
        self.raw.get(&key).is_some_and(Option::is_some)
            && self.records.get(&key).is_some_and(Option::is_some)
    }

    pub(super) fn note_record(&mut self, token: &str, ordinal: u32, type_id: [u8; 16]) {
        self.raw
            .entry((token.to_owned(), ordinal))
            .and_modify(|value| *value = None)
            .or_insert(Some(type_id));
    }

    fn resolves(&self, token: &str, reference: crate::pmdc::PmDcReference) -> bool {
        reference.record_ordinal().is_some_and(|ordinal| {
            self.raw
                .get(&(token.to_owned(), ordinal))
                .is_some_and(Option::is_some)
        })
    }

    pub(super) fn read(
        &mut self,
        ctx: &DecodeContext<'_>,
        token: &str,
        ordinal: u32,
        type_id: [u8; 16],
        source: View<'_>,
        version: u8,
    ) -> Result<(), CodecError> {
        let operand = parse(ctx, type_id, source, version)?;
        self.lengths.insert(
            (token.to_owned(), ordinal),
            cadmpeg_core::decode::u64_from_index(source.window().len()),
        );
        ctx.charge_entities(1, "admit Inventor rectangular-pattern operand")?;
        match self.records.entry((token.to_owned(), ordinal)) {
            std::collections::hash_map::Entry::Vacant(entry) => {
                entry.insert(Some(operand));
            }
            std::collections::hash_map::Entry::Occupied(mut entry) => {
                entry.insert(None);
            }
        }
        Ok(())
    }

    pub(crate) fn validate_suppression(
        &self,
        source: &PmDcPatternFeature,
        properties: &[PmDcFeatureProperty],
    ) -> Result<(), CodecError> {
        let Some(reference) = slot(source, 1) else {
            return Ok(());
        };
        let Some(Operand::Suppression { pairs }) = self.get_for(source, reference) else {
            return Ok(());
        };
        let count = |number| {
            let ordinal = slot(source, number)?.record_ordinal()?;
            let mut records = properties.iter().filter(|record| {
                record.identity.segment_token == source.identity.segment_token
                    && record.identity.record_ordinal == ordinal
            });
            let record = records.next()?;
            if records.next().is_some() {
                return None;
            }
            match record.kind {
                PmDcFeaturePropertyKind::RdxVariable { model_value, .. } if model_value > 0 => {
                    Some(model_value)
                }
                _ => None,
            }
        };
        let base = if source.save_version_major >= 21 {
            18
        } else {
            12
        };
        let first = count(base).ok_or_else(|| {
            CodecError::malformed("Inventor pattern first instance count is unresolved or zero")
        })?;
        let second = if slot(source, base + 1).is_some_and(|reference| reference.index() == 0) {
            1
        } else {
            count(base + 1).ok_or_else(|| {
                CodecError::malformed(
                    "Inventor pattern second instance count is unresolved or zero",
                )
            })?
        };
        if pairs.iter().any(|pair| pair[0] > first || pair[1] > second) {
            return Err(CodecError::malformed(
                "Inventor pattern suppression coordinate exceeds its instance count",
            ));
        }
        Ok(())
    }

    fn get_for(
        &self,
        source: &PmDcPatternFeature,
        reference: crate::pmdc::PmDcReference,
    ) -> Option<&Operand> {
        let token = source.identity.segment_token.as_str();
        let operand = self.get(token, reference)?;
        let key = (token.to_owned(), reference.record_ordinal()?);
        let type_id = self.raw.get(&key)?.as_ref()?;
        (self.lengths.get(&key).copied()
            == operand_length(source.save_version_major, *type_id, operand))
        .then_some(operand)
    }

    fn get(&self, token: &str, reference: crate::pmdc::PmDcReference) -> Option<&Operand> {
        if !self.resolves(token, reference) {
            return None;
        }
        let key = (token.to_owned(), reference.record_ordinal()?);
        let operand = self.records.get(&key)?.as_ref()?;
        let type_id = self.raw.get(&key)?.as_ref()?;
        (operand_type(operand, *type_id) && !matches!(operand, Operand::Unresolved))
            .then_some(operand)
    }
}

fn parse(
    ctx: &DecodeContext<'_>,
    type_id: [u8; 16],
    source: View<'_>,
    version: u8,
) -> Result<Operand, CodecError> {
    let mut cursor = Cursor::new(source);
    if matches!(type_id, SUPPRESSION_TYPE | SELECTION_CONTROL_TYPE) {
        content_header(&mut cursor, version)?;
    } else {
        state_content_header(&mut cursor, version)?;
    }
    let operand = match type_id {
        SUPPRESSION_TYPE => {
            if [
                cursor.u16("suppression marker")?,
                cursor.u16("suppression marker")?,
            ] != [suppression::MARKER_VALUE, suppression::MARKER_KIND_VALUE]
            {
                return Err(CodecError::malformed(
                    "Inventor pattern suppression marker is invalid",
                ));
            }
            let count = cursor.u32("suppression count")?;
            if count != 0 && cursor.u32_array::<2>("suppression metadata")? != [0, 0] {
                return Err(CodecError::malformed(
                    "Inventor pattern suppression metadata is invalid",
                ));
            }
            let count = cadmpeg_core::decode::bounded_len(
                u64::from(count),
                suppression_pair::LEN,
                cursor.remaining(),
            )
            .ok_or_else(|| {
                CodecError::malformed("Inventor pattern suppression count exceeds payload")
            })?;
            let mut pairs =
                ctx.alloc_filled(count, [0; 2], "read Inventor pattern suppressions")?;
            for pair in &mut pairs {
                *pair = cursor.u32_array("suppressed instance coordinates")?;
            }
            cursor.finish("pattern suppressions")?;
            let mut distinct = HashSet::new();
            if pairs
                .iter()
                .any(|pair| pair.contains(&0) || *pair == [1, 1] || !distinct.insert(*pair))
            {
                return Err(CodecError::malformed(
                    "Inventor pattern suppression coordinates are invalid",
                ));
            }
            return Ok(Operand::Suppression { pairs });
        }
        SELECTION_CONTROL_TYPE => {
            if cursor.remaining() != selection_control::LEN {
                return Err(CodecError::malformed(
                    "Inventor pattern selection control length is invalid",
                ));
            }
            Operand::SelectionControl {
                references: [
                    cursor.reference("pattern selection control reference")?,
                    cursor.reference("pattern selection control reference")?,
                ],
                value: cursor.u8("pattern selection control value")?,
            }
        }
        EDGE_DIRECTION_TYPE | LINE_DIRECTION_TYPE => {
            let body_len = cursor.remaining();
            let state = cursor.i32("pattern selection state")?;
            let reverse = cursor.reference("pattern selection reversal")?;
            let geometry = cursor.reference("pattern selection geometry")?;
            let selection_form = cursor.u32("pattern selection form")?;
            let zero = cursor.u16("pattern selection value")?;
            let curve_form = cursor.u32("pattern selection curve form")?;
            if selection_form != cached_edge::SELECTION_FORM_VALUE
                || curve_form != cached_edge::CURVE_FORM_VALUE
            {
                return Ok(Operand::Unresolved);
            }
            let expected_len = if type_id == EDGE_DIRECTION_TYPE {
                cached_edge::LEN
            } else {
                cached_line::LEN
            };
            if body_len != expected_len || zero != cached_edge::ZERO_VALUE {
                return Err(CodecError::malformed(
                    "Inventor pattern line selection prefix is invalid",
                ));
            }
            f64_triple(&mut cursor, "pattern line origin")?;
            let span = f64_triple(&mut cursor, "pattern line span")?;
            f64_triple(&mut cursor, "pattern line selection point")?;
            if cursor.u8("pattern line state")? != cached_edge::LINE_STATE_VALUE {
                return Err(CodecError::malformed(
                    "Inventor pattern line state is invalid",
                ));
            }
            let mut links = vec![cursor.reference("pattern line source")?];
            if type_id == EDGE_DIRECTION_TYPE {
                links.push(cursor.reference("pattern line source")?);
            }
            let span = Vector3::new(span[0].get(), span[1].get(), span[2].get());
            if span == Vector3::new(0.0, 0.0, 0.0) {
                return Err(CodecError::malformed("Inventor pattern line span is zero"));
            }
            Operand::Line {
                state,
                reverse,
                geometry,
                links,
                span,
            }
        }
        _ => {
            return Err(CodecError::malformed(
                "Inventor pattern operand type is invalid",
            ))
        }
    };
    cursor.finish("pattern operand")?;
    Ok(operand)
}

fn operand_length(version: u8, type_id: [u8; 16], operand: &Operand) -> Option<u64> {
    let prefix = if version >= 28 {
        header_v28::LEN
    } else {
        header::LEN
    };
    let stamps = match version {
        0..=22 => 0,
        23..=24 => 1,
        _ => 2,
    };
    let stamped_prefix = prefix + stamps * std::mem::size_of::<i32>();
    let len = match operand {
        Operand::Suppression { pairs } => stamped_prefix
            .checked_add(suppression::LEN)?
            .checked_add(if pairs.is_empty() {
                0
            } else {
                std::mem::size_of::<[u32; 2]>()
                    .checked_add(pairs.len().checked_mul(suppression_pair::LEN)?)?
            })?,
        Operand::SelectionControl { .. } => stamped_prefix + selection_control::LEN,
        Operand::Line { .. } => {
            prefix
                + if type_id == EDGE_DIRECTION_TYPE {
                    cached_edge::LEN
                } else {
                    cached_line::LEN
                }
        }
        Operand::Unresolved => return None,
    };
    Some(cadmpeg_core::decode::u64_from_index(len))
}

fn operand_type(operand: &Operand, type_id: [u8; 16]) -> bool {
    match operand {
        Operand::Suppression { .. } => type_id == SUPPRESSION_TYPE,
        Operand::SelectionControl { .. } => type_id == SELECTION_CONTROL_TYPE,
        Operand::Line { links, .. } => match type_id {
            EDGE_DIRECTION_TYPE => links.len() == 2,
            LINE_DIRECTION_TYPE => links.len() == 1,
            _ => false,
        },
        Operand::Unresolved => matches!(type_id, EDGE_DIRECTION_TYPE | LINE_DIRECTION_TYPE),
    }
}

fn hex_type(text: &str) -> Option<[u8; 16]> {
    let mut bytes = [0; 16];
    if text.len() != 32 {
        return None;
    }
    for (index, byte) in bytes.iter_mut().enumerate() {
        *byte = u8::from_str_radix(text.get(index * 2..index * 2 + 2)?, 16).ok()?;
    }
    Some(bytes)
}

fn slot(source: &PmDcPatternFeature, slot: usize) -> Option<crate::pmdc::PmDcReference> {
    source.property_slots.get(slot).copied()
}

fn property_at<'a>(
    source: &PmDcPatternFeature,
    slot_number: usize,
    index: &'a ProjectionIndex<'a>,
) -> Option<&'a PmDcFeatureProperty> {
    resolve_property(
        source.identity.segment_token.as_str(),
        slot(source, slot_number)?.index(),
        index,
    )
}

fn bool_at(
    source: &PmDcPatternFeature,
    slot_number: usize,
    index: &ProjectionIndex<'_>,
) -> Option<bool> {
    match property_at(source, slot_number, index)?.kind {
        PmDcFeaturePropertyKind::Boolean { value, .. } => Some(value),
        _ => None,
    }
}

fn count_at(
    source: &PmDcPatternFeature,
    slot_number: usize,
    index: &ProjectionIndex<'_>,
) -> Option<u32> {
    match property_at(source, slot_number, index)?.kind {
        PmDcFeaturePropertyKind::RdxVariable {
            nominal_value,
            model_value,
            ..
        } if model_value >= 1 && nominal_value == model_value => Some(model_value),
        _ => None,
    }
}

fn direction(
    source: &PmDcPatternFeature,
    slot_number: usize,
    index: &ProjectionIndex<'_>,
    operands: &Operands,
) -> Option<FeatureDirection3> {
    let token = source.identity.segment_token.as_str();
    let reference = slot(source, slot_number)?;
    if !operands.resolves(token, reference) {
        return None;
    }
    if let Some(record) = index
        .directions
        .get(&(token, reference.record_ordinal()?))
        .and_then(Option::as_ref)
    {
        if record.entity_flags != 0
            || record.parameter.get() != 0.0
            || record.extension.is_some_and(|value| value != 0)
        {
            return None;
        }
        return cadmpeg_ir::units::UnitVector3::normalized(Vector3::new(
            record.direction[0].get(),
            record.direction[1].get(),
            record.direction[2].get(),
        ))
        .map(FeatureDirection3::from);
    }
    let Operand::Line {
        state,
        reverse,
        geometry,
        links,
        span,
    } = operands.get_for(source, reference)?
    else {
        return None;
    };
    let type_id = operands
        .raw
        .get(&(token.to_owned(), reference.record_ordinal()?))?
        .as_ref()?;
    if !matches!(
        (*type_id, *state),
        (LINE_DIRECTION_TYPE, -1) | (EDGE_DIRECTION_TYPE, 1 | 2)
    ) || !operands.resolves(token, *geometry)
        || !links.iter().all(|link| operands.resolves(token, *link))
    {
        return None;
    }
    let mut vector = cadmpeg_ir::units::UnitVector3::normalized(*span)?;
    if operands
        .raw
        .get(&(token.to_owned(), geometry.record_ordinal()?))
        == Some(&Some(WORK_AXIS_TYPE))
    {
        let axis = axis_direction(
            resolve_property(token, geometry.index(), index)?,
            source.save_version_major,
        )?;
        if vector != axis {
            return None;
        }
    }
    match resolve_property(token, reverse.index(), index)?.kind {
        PmDcFeaturePropertyKind::Boolean { value: true, .. } => vector = vector.reversed(),
        PmDcFeaturePropertyKind::Boolean { value: false, .. } => {}
        _ => return None,
    }
    Some(vector.into())
}

fn axis_direction(
    record: &PmDcFeatureProperty,
    version: u8,
) -> Option<cadmpeg_ir::units::UnitVector3> {
    let PmDcFeaturePropertyKind::WorkAxis {
        entity_flags: 0,
        values,
        references,
        marker: false,
        span,
        state: 0 | 1,
        ..
    } = &record.kind
    else {
        return None;
    };
    if values.len() != if version > 21 { 3 } else { 2 }
        || values.iter().any(|value| *value != 0)
        || !references.references().is_empty()
    {
        return None;
    }
    cadmpeg_ir::units::UnitVector3::normalized(Vector3::new(
        span[0].get(),
        span[1].get(),
        span[2].get(),
    ))
}

fn axis(
    source: &PmDcPatternFeature,
    axis: usize,
    index: &ProjectionIndex<'_>,
    operands: &Operands,
) -> Option<LinearPatternDirection> {
    let base = if source.save_version_major >= 21 {
        18
    } else {
        12
    };
    let count = count_at(source, base + axis, index)?;
    let spacing = PositiveLength::try_from(length_reference(
        source.identity.segment_token.as_str(),
        slot(source, base + 2 + axis)?.index(),
        index,
    )?)
    .ok()?;
    let direction = direction(source, base + 4 + axis, index, operands)?;
    if [6, 8, 10, 12]
        .into_iter()
        .any(|offset| bool_at(source, base + offset + axis, index) != Some(false))
    {
        return None;
    }
    Some(LinearPatternDirection {
        direction,
        spacing,
        count,
    })
}

fn fixed_feature_header(source: &PmDcPatternFeature) -> bool {
    source.family == PmDcPatternFamily::Rectangular
        && crate::pmdc::record_version_supported(source.save_version_major)
        && source.property_slots.len()
            == if source.save_version_major >= 21 {
                32
            } else {
                26
            }
        && source.control == 0
        && source.properties.references().is_empty()
        && [3, 7, 8, 10]
            .into_iter()
            .all(|number| slot(source, number).is_some_and(|reference| reference.index() == 0))
        && (source.save_version_major < 21
            || (12..18)
                .all(|number| slot(source, number).is_some_and(|reference| reference.index() == 0)))
}

fn fixed_feature_properties<'a>(
    property: impl Fn(usize) -> Option<&'a PmDcFeatureProperty>,
) -> bool {
    [6, 11].into_iter().all(|number| {
        matches!(
            property(number).map(|record| &record.kind),
            Some(PmDcFeaturePropertyKind::Boolean { value: false, .. })
        )
    }) && matches!(
        property(2).map(|record| &record.kind),
        Some(PmDcFeaturePropertyKind::RdxVariable {
            nominal_value: 0,
            model_value: 0,
            ..
        })
    ) && matches!(
        property(5).map(|record| &record.kind),
        Some(PmDcFeaturePropertyKind::References {
            family: super::PmDcFeatureReferenceFamily::FeatureDimensions,
            ..
        })
    )
}

fn construction<'a>(
    source: &PmDcPatternFeature,
    index: &ProjectionIndex<'_>,
    operands: &'a Operands,
) -> Option<LinearConstruction<'a>> {
    if !fixed_feature_header(source)
        || !fixed_feature_properties(|number| property_at(source, number, index))
    {
        return None;
    }
    let token = source.identity.segment_token.as_str();
    let Operand::Suppression { pairs: suppressed } = operands.get_for(source, slot(source, 1)?)?
    else {
        return None;
    };
    let Operand::SelectionControl { references, value } =
        operands.get_for(source, slot(source, 4)?)?
    else {
        return None;
    };
    if *value != 0 || references.iter().any(|reference| reference.index() != 0) {
        return None;
    }
    let placement_transform = index
        .transforms
        .get(&(token, slot(source, 0)?.record_ordinal()?))?
        .as_ref()?;
    if placement_transform.matrix.rows() != IDENTITY_PLACEMENT {
        return None;
    }
    let first = axis(source, 0, index, operands)?;
    let base = if source.save_version_major >= 21 {
        18
    } else {
        12
    };
    let second = if slot(source, base + 1)?.index() == 0 {
        if [3, 5, 7, 9, 11, 13].into_iter().any(|offset| {
            slot(source, base + offset).is_none_or(|reference| reference.index() != 0)
        }) {
            return None;
        }
        None
    } else {
        Some(axis(source, 1, index, operands)?)
    };
    let count = first
        .count
        .checked_mul(second.as_ref().map_or(1, |axis| axis.count))?;
    if !fixed_extensions(source, count) || count < 2 {
        return None;
    }
    let mut distinct = HashSet::new();
    if suppressed.iter().any(|pair| {
        pair.contains(&0)
            || *pair == [1, 1]
            || pair[0] > first.count
            || pair[1] > second.as_ref().map_or(1, |axis| axis.count)
            || !distinct.insert(*pair)
    }) {
        return None;
    }
    Some(LinearConstruction {
        first,
        second,
        suppressed,
    })
}

struct LinearConstruction<'a> {
    first: LinearPatternDirection,
    second: Option<LinearPatternDirection>,
    suppressed: &'a [[u32; 2]],
}

impl LinearConstruction<'_> {
    fn pattern(self, ctx: &DecodeContext<'_>) -> Result<Option<PatternKind>, CodecError> {
        let mut omitted_instances = ctx.alloc_filled(
            self.suppressed.len(),
            [0; 2],
            "project Inventor pattern instance omissions",
        )?;
        for (position, pair) in omitted_instances.iter_mut().zip(self.suppressed) {
            let (Some(first), Some(second)) = (pair[0].checked_sub(1), pair[1].checked_sub(1))
            else {
                return Ok(None);
            };
            *position = [first, second];
        }
        Ok(PatternKind::new(PatternTransform::Linear {
            direction: Some(self.first.direction),
            spacing: self.first.spacing,
            count: self.first.count,
            second: self.second,
            omitted_instances,
        })
        .ok())
    }

    fn matches(&self, pattern: &PatternKind) -> bool {
        let PatternTransform::Linear {
            direction,
            spacing,
            count,
            second,
            omitted_instances,
        } = pattern.definition()
        else {
            return false;
        };
        direction.as_ref() == Some(&self.first.direction)
            && spacing == &self.first.spacing
            && count == &self.first.count
            && second == &self.second
            && omitted_instances.len() == self.suppressed.len()
            && omitted_instances
                .iter()
                .zip(self.suppressed)
                .all(|(position, pair)| {
                    pair[0].checked_sub(1) == Some(position[0])
                        && pair[1].checked_sub(1) == Some(position[1])
                })
    }
}

fn fixed_extensions(source: &PmDcPatternFeature, count: u32) -> bool {
    let expected_len = match source.save_version_major {
        0..=28 => 0,
        29 => 8,
        _ => 14,
    };
    source.extension_values.len() == expected_len
        && source
            .extension_values
            .iter()
            .enumerate()
            .all(|(index, value)| *value == if index == 5 { count } else { 0 })
}

fn seed_kind(
    source: &PmDcPatternFeature,
    reference: crate::pmdc::PmDcReference,
    operands: &Operands,
) -> Option<&'static str> {
    let key = (
        source.identity.segment_token.as_str().to_owned(),
        reference.record_ordinal()?,
    );
    match *operands.raw.get(&key)?.as_ref()? {
        super::FEATURE_TYPE => Some("feature"),
        super::RECTANGULAR_PATTERN_FEATURE_TYPE | super::MIRROR_FEATURE_TYPE => {
            Some("pattern-feature")
        }
        _ => None,
    }
}

fn seed_placement_is_admitted(
    seed_ordinal: u64,
    pattern_ordinal: u64,
    seed_model_history: bool,
    pattern_model_history: bool,
    seed_is_sketch: bool,
) -> bool {
    seed_model_history && pattern_model_history && !seed_is_sketch && seed_ordinal < pattern_ordinal
}

pub(super) fn project(
    ctx: &DecodeContext<'_>,
    source: &PmDcPatternFeature,
    label: Option<&PmDcFeatureLabel>,
    index: &ProjectionIndex<'_>,
    operands: &Operands,
    history: &history::History,
) -> Option<Result<(Feature, FeatureResultTopology), CodecError>> {
    let construction = construction(source, index, operands)?;
    let placement = history.placement(source.identity.record_ordinal)?;
    if !placement.model_history {
        return None;
    }
    let mut seeds = Vec::new();
    let mut distinct = HashSet::new();
    for reference in source.participants.references() {
        seed_kind(source, *reference, operands)?;
        let ordinal = reference.record_ordinal()?;
        let seed = history.placement(ordinal)?;
        if !distinct.insert(ordinal)
            || !seed_placement_is_admitted(
                seed.ordinal,
                placement.ordinal,
                seed.model_history,
                placement.model_history,
                seed.kind == history::HistoryNodeKind::Sketch,
            )
        {
            return None;
        }
        let mut identity = source.identity.clone();
        identity.record_ordinal = ordinal;
        let id = match history_feature_id(&identity) {
            Ok(id) => id,
            Err(error) => return Some(Err(error)),
        };
        seeds.push(PatternSeed::Feature(id));
    }
    if seeds.is_empty() {
        return None;
    }
    let collection = property_at(source, 9, index)?;
    let (id, result) = match result_from_collection(ctx, &source.identity, collection, index)? {
        Ok(result) => result,
        Err(error) => return Some(Err(error)),
    };
    let pattern = match construction.pattern(ctx) {
        Ok(Some(pattern)) => pattern,
        Ok(None) => return None,
        Err(error) => return Some(Err(error)),
    };
    let mut feature = match native_feature(ctx, &source.identity, label, source.id()) {
        Ok(feature) => feature,
        Err(error) => return Some(Err(error)),
    };
    feature.id = id;
    feature.source_tag = Some("rectangular_pattern".into());
    feature.evaluation = cadmpeg_ir::features::FeatureEvaluation::from_definition(
        FeatureDefinition::Operation(FeatureOperation::Pattern { seeds, pattern }),
    );
    Some(Ok((feature, result)))
}

pub(crate) fn is_linear_pattern(feature: &Feature) -> bool {
    matches!(
        feature.evaluation.definition(),
        FeatureDefinition::Operation(FeatureOperation::Pattern { pattern, .. })
            if matches!(pattern.definition(), PatternTransform::Linear { .. })
    )
}

#[derive(Clone, Copy)]
pub(crate) struct SourceRecords<'a> {
    pub(crate) properties: &'a [PmDcFeatureProperty],
    pub(crate) parameters: &'a [crate::design::PmDcParameter],
    pub(crate) values: &'a [cadmpeg_ir::features::DesignParameter],
    pub(crate) transforms: &'a [crate::sketch::PmDcTransform],
    pub(crate) directions: &'a [crate::sketch::PmDcDirection],
}

pub(crate) fn validation_index(records: SourceRecords<'_>) -> ProjectionIndex<'_> {
    use cadmpeg_core::decode::unique_index;
    ProjectionIndex {
        properties: unique_index(records.properties.iter().map(|r| {
            (
                (r.identity.segment_token.as_str(), r.identity.record_ordinal),
                r,
            )
        })),
        parameters: unique_index(records.parameters.iter().map(|r| {
            (
                (r.identity.segment_token.as_str(), r.identity.record_ordinal),
                r,
            )
        })),
        parameter_values: unique_index(
            records
                .values
                .iter()
                .filter_map(|r| Some((r.native_ref.as_deref()?, r))),
        ),
        transforms: unique_index(records.transforms.iter().map(|r| {
            (
                (r.identity.segment_token.as_str(), r.identity.record_ordinal),
                r,
            )
        })),
        directions: unique_index(records.directions.iter().map(|r| {
            (
                (r.identity.segment_token.as_str(), r.identity.record_ordinal),
                r,
            )
        })),
        ..ProjectionIndex::default()
    }
}

pub(crate) fn matches_source(
    source: &PmDcPatternFeature,
    feature: &Feature,
    index: &ProjectionIndex<'_>,
    operands: &Operands,
    results: &[FeatureResultTopology],
    features: &[Feature],
) -> bool {
    let FeatureDefinition::Operation(FeatureOperation::Pattern { seeds, pattern }) =
        feature.evaluation.definition()
    else {
        return false;
    };
    if construction(source, index, operands).is_none_or(|source| !source.matches(pattern))
        || seeds.is_empty()
        || seeds.len() != source.participants.references().len()
    {
        return false;
    }
    let mut distinct = HashSet::new();
    for (seed, reference) in seeds.iter().zip(source.participants.references()) {
        let Some(ordinal) = reference.record_ordinal() else {
            return false;
        };
        if !distinct.insert(ordinal) {
            return false;
        }
        let Some(kind) = seed_kind(source, *reference, operands) else {
            return false;
        };
        let expected_native = format!(
            "inventor:pmdc:{kind}#{}-{ordinal}",
            source.identity.segment_token
        );
        let PatternSeed::Feature(seed_id) = seed else {
            return false;
        };
        let mut targets = features.iter().filter(|target| target.id == *seed_id);
        let Some(target) = targets.next() else {
            return false;
        };
        if targets.next().is_some()
            || target.native_ref.as_deref() != Some(expected_native.as_str())
            || !seed_placement_is_admitted(
                target.ordinal,
                feature.ordinal,
                !target.source_properties.contains_key("history"),
                !feature.source_properties.contains_key("history"),
                matches!(
                    target.evaluation.definition(),
                    FeatureDefinition::Operation(FeatureOperation::Sketch { .. })
                ),
            )
        {
            return false;
        }
        let mut identity = source.identity.clone();
        identity.record_ordinal = ordinal;
        if history_feature_id(&identity)
            .ok()
            .as_ref()
            .is_none_or(|id| !matches!(seed, PatternSeed::Feature(actual) if actual == id))
        {
            return false;
        }
    }
    let Some(collection) = property_at(source, 9, index) else {
        return false;
    };
    if !matches!(
        collection.kind,
        PmDcFeaturePropertyKind::References {
            family: super::PmDcFeatureReferenceFamily::ObjectCollection,
            ..
        }
    ) {
        return false;
    }
    let mut feature_results = results
        .iter()
        .filter(|result| result.output_of == feature.id);
    let Some(result) = feature_results.next() else {
        return false;
    };
    feature_results.next().is_none()
        && result.native_ref.as_deref() == Some(collection.id().as_str())
}

pub(crate) fn native_losses<'a>(
    sources: impl Iterator<Item = &'a PmDcPatternFeature>,
    features: &[Feature],
) -> Vec<cadmpeg_ir::report::loss::LossNote> {
    let projected = features
        .iter()
        .filter(|feature| is_linear_pattern(feature))
        .filter_map(|feature| feature.native_ref.as_deref())
        .collect::<HashSet<_>>();
    sources
        .filter(|source| {
            source.family == PmDcPatternFamily::Rectangular
                && !projected.contains(source.id().as_str())
        })
        .map(|source| {
            crate::loss::InventorLossCode::FeatureRectangularPatternOperandsNative.note(format!(
                "Rectangular pattern {} retains unresolved construction operands.",
                source.id()
            ))
        })
        .collect()
}

#[cfg(test)]
mod tests;
