// SPDX-License-Identifier: Apache-2.0
//! Text annotation entities.

use super::geometry::{curve_geometry_coplanar, resolve_transform, ProjectionOutcome};
use super::presentation::{
    general_note_font_valid_for_global_table, new_general_note_charset_valid,
    new_general_note_font_valid,
};
use super::{mirror_flag_valid, vertical_text_flag_valid};
use crate::directory::{DirectoryEntry, UseFlag};
use crate::global::{GlobalTable, ProjectedGlobal};
use crate::parameter::{DefaultTailCount, ParameterRecord};
use cadmpeg_core::decode::DecodeContext;
use cadmpeg_core::CodecError;
use cadmpeg_ir::index::ModelIndex;
use cadmpeg_ir::math::{Point3, Vector3};
use cadmpeg_ir::transform::Transform;
use cadmpeg_ir::CadIr;
use std::collections::{BTreeMap, BTreeSet};

/// One admitted annotation shape per variant.
///
/// [`classify`] is the single owner of annotation admission: the native
/// retention pass and the semantic projection both dispatch on its result,
/// so a new form is admitted by adding one `classify` arm and handling the
/// variant everywhere the compiler then requires.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum AnnotationKind {
    AngularDimension,
    CurveDimension,
    DiameterDimension,
    FlagNote,
    GeneralLabel,
    GeneralNote,
    NewGeneralNote,
    Leader,
    LinearDimension,
    OrdinateDimension,
    PointDimension,
    RadiusDimension,
    GeneralSymbol,
    SectionedArea,
}

// Directory entries, records and the Global table stay fixed for this projection.
// Sequence keys therefore identify reusable primary grammar and width results.
struct AnnotationValidation<'ctx, 'policy> {
    ctx: &'ctx DecodeContext<'policy>,
    primary: BTreeMap<u32, bool>,
    width_sums: BTreeMap<u32, Option<i64>>,
    storage: cadmpeg_core::decode::ScopedReservation<'ctx>,
}

impl<'ctx, 'policy> AnnotationValidation<'ctx, 'policy> {
    fn new(ctx: &'ctx DecodeContext<'policy>) -> Result<Self, CodecError> {
        Ok(Self {
            ctx,
            primary: BTreeMap::new(),
            width_sums: BTreeMap::new(),
            storage: ctx.reserve_scoped(0, "iges annotation validation scratch")?,
        })
    }

    fn primary(
        &mut self,
        entry: &DirectoryEntry,
        record: &ParameterRecord,
        entries: &BTreeMap<u32, &DirectoryEntry>,
        global_table: GlobalTable,
    ) -> Result<bool, CodecError> {
        if let Some(valid) = self.cached_primary(entry.sequence)? {
            return Ok(valid);
        }
        self.validate_primary(entry, record, entries, global_table)
    }

    fn primary_from_records(
        &mut self,
        sequence: u32,
        entry: &DirectoryEntry,
        entries: &BTreeMap<u32, &DirectoryEntry>,
        records: &BTreeMap<u32, &ParameterRecord>,
        global_table: GlobalTable,
    ) -> Result<bool, CodecError> {
        if let Some(valid) = self.cached_primary(entry.sequence)? {
            return Ok(valid);
        }
        let Some(record) = self.ctx.get_btree_map(
            records,
            &sequence,
            "iges annotation child Parameter Data lookup",
        )?
        else {
            return Ok(false);
        };
        self.validate_primary(entry, record, entries, global_table)
    }

    fn cached_primary(&self, sequence: u32) -> Result<Option<bool>, CodecError> {
        Ok(self
            .ctx
            .get_btree_map(
                &self.primary,
                &sequence,
                "iges annotation primary validation cache lookup",
            )?
            .copied())
    }

    fn validate_primary(
        &mut self,
        entry: &DirectoryEntry,
        record: &ParameterRecord,
        entries: &BTreeMap<u32, &DirectoryEntry>,
        global_table: GlobalTable,
    ) -> Result<bool, CodecError> {
        let valid = match entry.entity_type {
            212 => general_note_valid_for_global_table(
                record,
                entries,
                global_table,
                entry.form,
                self.ctx,
            )?,
            214 => leader_valid_for_global_table(entry, record, global_table, self.ctx)?,
            106 => witness_valid(record, self.ctx)?,
            _ => false,
        };
        self.storage.with_storage(|| {
            self.ctx.insert_btree_map(
                &mut self.primary,
                entry.sequence,
                valid,
                "iges annotation primary validation cache",
            )
        })?;
        Ok(valid)
    }

    fn width_sum_from_records(
        &mut self,
        sequence: u32,
        records: &BTreeMap<u32, &ParameterRecord>,
    ) -> Result<Option<i64>, CodecError> {
        if let Some(total) = self.ctx.get_btree_map(
            &self.width_sums,
            &sequence,
            "iges annotation width cache lookup",
        )? {
            return Ok(*total);
        }
        let Some(note) =
            self.ctx
                .get_btree_map(records, &sequence, "iges flag note width source lookup")?
        else {
            return Ok(None);
        };
        let mut total = Some(0_i64);
        if let Some(strings) = note.count(1) {
            let mut offsets = 0..strings;
            while !offsets.is_empty() || self.ctx.resource_refusal().is_some() {
                let Some(offset) = self
                    .ctx
                    .next_charged(&mut offsets, "iges flag note width sum")?
                else {
                    break;
                };
                total = total.and_then(|total| {
                    total.checked_add(note.integer(2 + offset * 12).unwrap_or_default())
                });
                if total.is_none() {
                    break;
                }
            }
        } else {
            total = None;
        }
        self.storage.with_storage(|| {
            self.ctx.insert_btree_map(
                &mut self.width_sums,
                sequence,
                total,
                "iges annotation width sum cache",
            )
        })?;
        Ok(total)
    }
}

fn sectioned_area_pattern_plane(
    record: &ParameterRecord,
    transform: Transform,
    length_factor: f64,
) -> Option<(Point3, Vector3)> {
    let z = record.number_or(5, 0.0)? * length_factor;
    if !z.is_finite() || !length_factor.is_finite() {
        return None;
    }
    let point = transform.apply_point(Point3::new(0.0, 0.0, z))?.get();
    let normal = transform
        .apply_normal(Vector3::new(0.0, 0.0, 1.0))
        .and_then(|normal| cadmpeg_ir::features::FiniteVector3::from(normal).unit_nonzero())?;
    Some((point, normal))
}

type SectionCoplanarityKey = (u32, super::CoplanarityPlaneKey);

struct SectionedAreaGeometryCache<'ctx, 'ir> {
    ir: &'ir CadIr,
    index: Option<cadmpeg_ir::index::DecodeModelIndex<'ctx, 'ir>>,
    proofs: BTreeMap<SectionCoplanarityKey, super::CoplanarityProof<'ir, bool>>,
    storage: cadmpeg_core::decode::ScopedReservation<'ctx>,
}

impl<'ctx, 'ir> SectionedAreaGeometryCache<'ctx, 'ir> {
    fn new(ir: &'ir CadIr, ctx: &'ctx DecodeContext<'_>) -> Result<Self, CodecError> {
        Ok(Self {
            ir,
            index: None,
            proofs: BTreeMap::new(),
            storage: ctx.reserve_scoped(0, "iges section coplanarity cache scratch")?,
        })
    }

    fn curve_coplanar(
        &mut self,
        sequence: u32,
        pattern_plane: (Point3, Vector3),
        resolution: f64,
        ctx: &'ctx DecodeContext<'_>,
    ) -> Result<bool, CodecError> {
        if let Some(refusal) = ctx.resource_refusal() {
            return Err(refusal.into());
        }
        if !resolution.is_finite() || resolution < 0.0 {
            return Ok(false);
        }
        let plane_key = super::coplanarity_plane_key(pattern_plane, resolution);
        let mut key = (sequence, plane_key);
        if let Some(proof) =
            ctx.get_btree_map(&self.proofs, &key, "iges section coplanarity proof lookup")?
        {
            if proof.matches(plane_key, pattern_plane, resolution) {
                return Ok(proof.result);
            }
            key = (
                sequence,
                super::coplanarity_placement_key(pattern_plane, resolution),
            );
            if let Some(proof) =
                ctx.get_btree_map(&self.proofs, &key, "iges section coplanarity proof lookup")?
            {
                return Ok(proof.result);
            }
        }
        if self.index.is_none() {
            self.index = Some(ModelIndex::new_model_only(self.ir, ctx)?);
        }
        let Some(index) = self.index.as_ref() else {
            return Ok(false);
        };
        let identity = Transform::identity();
        let mut active_storage = ctx.reserve_scoped(0, "iges section curve scratch")?;
        let mut active = BTreeSet::new();
        let curve_id = active_storage.with_storage(|| {
            crate::ids::curve_admitted(&crate::ids::Stem::directory(sequence), ctx)
        })?;
        let mut proven_geometry = None;
        let coplanar = if let Some(curve) = index.curves(curve_id.as_str(), ctx)? {
            if let Some(geometry) = curve.geometry.solved() {
                proven_geometry = Some(geometry);
                active_storage.with_storage(|| {
                    ctx.insert_btree_set(&mut active, curve_id, "iges section active curves")
                })?;
                active_storage.with_storage(|| {
                    curve_geometry_coplanar(
                        geometry,
                        index,
                        identity,
                        pattern_plane,
                        resolution,
                        &mut active,
                        ctx,
                    )
                })?
            } else {
                false
            }
        } else {
            false
        };
        self.storage.with_storage(|| {
            ctx.insert_btree_map(
                &mut self.proofs,
                key,
                super::CoplanarityProof {
                    origin: pattern_plane.0,
                    geometry: proven_geometry,
                    result: coplanar,
                },
                "iges section coplanarity proof cache",
            )
        })?;
        Ok(coplanar)
    }
}

/// Maps a directory entry's type and form to its annotation kind, or `None`
/// for every shape the annotation concern does not admit.
pub(crate) fn classify(entity_type: i64, form: i64) -> Option<AnnotationKind> {
    match (entity_type, form) {
        (202, 0) => Some(AnnotationKind::AngularDimension),
        (204, 0) => Some(AnnotationKind::CurveDimension),
        (206, 0) => Some(AnnotationKind::DiameterDimension),
        (208, 0) => Some(AnnotationKind::FlagNote),
        (210, 0) => Some(AnnotationKind::GeneralLabel),
        (212, form) if crate::profile::general_note_form_admitted(form) => {
            Some(AnnotationKind::GeneralNote)
        }
        (213, 0) => Some(AnnotationKind::NewGeneralNote),
        (214, 1..=12) => Some(AnnotationKind::Leader),
        (216, 0..=2) => Some(AnnotationKind::LinearDimension),
        (218, 0..=1) => Some(AnnotationKind::OrdinateDimension),
        (220, 0) => Some(AnnotationKind::PointDimension),
        (222, 0..=1) => Some(AnnotationKind::RadiusDimension),
        (228, 0..=3 | 5001..=9999) => Some(AnnotationKind::GeneralSymbol),
        (230, 0..=1) => Some(AnnotationKind::SectionedArea),
        _ => None,
    }
}

fn finite(record: &ParameterRecord, index: usize) -> bool {
    record.number(index).is_some()
}

fn exact_parameter_count(record: &ParameterRecord, expected: usize) -> bool {
    record.parameter_end() == expected
}

fn justification_valid(value: i64) -> bool {
    matches!(value, 0..=3)
}

fn fixed_or_variable_valid(value: i64) -> bool {
    matches!(value, 0..=1)
}

fn hexadecimal_byte(bytes: &[u8]) -> Option<u8> {
    let [high, low] = bytes else {
        return None;
    };
    let high = match high {
        b'0'..=b'9' => high - b'0',
        b'A'..=b'F' => high - b'A' + 10,
        b'a'..=b'f' => high - b'a' + 10,
        _ => return None,
    };
    let low = match low {
        b'0'..=b'9' => low - b'0',
        b'A'..=b'F' => low - b'A' + 10,
        b'a'..=b'f' => low - b'a' + 10,
        _ => return None,
    };
    Some((high << 4) | low)
}

fn general_note_text_valid_for_global_table(
    text: &[u8],
    font: i64,
    global_table: GlobalTable,
    is_v5_null_string: bool,
    ctx: &DecodeContext<'_>,
) -> Result<bool, CodecError> {
    if let Some(refusal) = ctx.resource_refusal() {
        return Err(refusal.into());
    }
    if font != 2001 {
        return Ok(true);
    }
    if matches!(global_table, GlobalTable::V4_0) {
        return Ok(false);
    }
    if is_v5_null_string && matches!(global_table, GlobalTable::V5_0) {
        return Ok(true);
    }
    Ok(text.len().is_multiple_of(4)
        && ctx.all_by(
            text.chunks_exact(4),
            |character| {
                Ok(hexadecimal_byte(&character[..2])
                    .zip(hexadecimal_byte(&character[2..]))
                    .is_some_and(|(row, column)| {
                        (0x21..=0x7e).contains(&row) && (0x21..=0x7e).contains(&column)
                    }))
            },
            "iges annotation validation traversal",
        )?)
}

fn general_note_valid_for_global_table(
    record: &ParameterRecord,
    entries: &BTreeMap<u32, &DirectoryEntry>,
    global_table: GlobalTable,
    form: i64,
    ctx: &DecodeContext<'_>,
) -> Result<bool, CodecError> {
    if let Some(refusal) = ctx.resource_refusal() {
        return Err(refusal.into());
    }
    let parameter_end = crate::parameter::general_note_layout_end(record, form)
        .unwrap_or_else(|| record.parameter_end());
    if !general_note_suffix_structurally_valid(record, parameter_end) {
        return Ok(false);
    }
    let count = match record.count_with_stride_before_default_tail(1, 12, parameter_end) {
        DefaultTailCount::Held(count)
            if crate::profile::general_note_form_admitted(form)
                && general_note_string_count_valid(form, count) =>
        {
            count
        }
        _ => return Ok(false),
    };
    Ok(parameter_end <= 2 + count * 12
        && ctx.all_by(
            0..count,
            |index| {
                let start = 2 + index * 12;
                let text = record.string_or_empty(start + 11);
                Ok(record
                    .integer(start)
                    .and_then(|value| usize::try_from(value).ok())
                    .zip(text)
                    .is_some_and(|(declared, text)| declared == text.len())
                    && (start + 1..=start + 2).all(|field| {
                        record
                            .number_or(field, 0.0)
                            .is_some_and(|value| value.is_finite() && value >= 0.0)
                    })
                    && record
                        .integer_or(start + 3, 1)
                        .zip(text)
                        .map(|(font, text)| -> Result<bool, CodecError> {
                            Ok(general_note_font_valid_for_global_table(
                                font,
                                entries,
                                global_table,
                                ctx,
                            )? && general_note_text_valid_for_global_table(
                                text,
                                font,
                                global_table,
                                record.integer(1) == Some(1) && text == b" ",
                                ctx,
                            )?)
                        })
                        .transpose()?
                        .unwrap_or(false)
                    && record
                        .number_or(start + 4, std::f64::consts::FRAC_PI_2)
                        .is_some()
                    && record.number_or(start + 5, 0.0).is_some()
                    && record
                        .integer_or(start + 6, 0)
                        .is_some_and(mirror_flag_valid)
                    && record
                        .integer_or(start + 7, 0)
                        .is_some_and(vertical_text_flag_valid)
                    && (start + 8..=start + 10).all(|field| record.number_or(field, 0.0).is_some()))
            },
            "iges annotation validation traversal",
        )?)
}

fn general_note_suffix_structurally_valid(record: &ParameterRecord, primary_end: usize) -> bool {
    // A malformed pointer target must not invalidate the note's own primary
    // fields, but arbitrary tokens after that primary span are not a suffix.
    // Check the two counted group shapes without requiring their targets to
    // resolve; reference validation owns that separate decision.
    if record.tokens().len() == primary_end || record.parameter_end() == primary_end {
        return true;
    }
    let Some(first_count) = record
        .tokens()
        .get(primary_end)
        .and_then(|token| match &token.value {
            crate::parameter::TokenValue::Integer(value) => usize::try_from(*value).ok(),
            crate::parameter::TokenValue::Omitted
            | crate::parameter::TokenValue::Real(_)
            | crate::parameter::TokenValue::String(_) => None,
        })
    else {
        return false;
    };
    let Some(first_end) = primary_end
        .checked_add(1)
        .and_then(|end| end.checked_add(first_count))
    else {
        return false;
    };
    if first_end > record.tokens().len() {
        return false;
    }
    if first_end == record.tokens().len() {
        return true;
    }
    let Some(second_count) = record
        .tokens()
        .get(first_end)
        .and_then(|token| match &token.value {
            crate::parameter::TokenValue::Integer(value) => usize::try_from(*value).ok(),
            crate::parameter::TokenValue::Omitted
            | crate::parameter::TokenValue::Real(_)
            | crate::parameter::TokenValue::String(_) => None,
        })
    else {
        return false;
    };
    first_end
        .checked_add(1)
        .and_then(|end| end.checked_add(second_count))
        .is_some_and(|second_end| second_end == record.tokens().len())
}

fn general_note_string_count_valid(form: i64, count: usize) -> bool {
    let minimum = match form {
        0 | 6..=8 => 1,
        1..=4 => 2,
        5 => 3,
        100 => 4,
        101 => 8,
        102 => 9,
        105 => 12,
        _ => return false,
    };
    count >= minimum
}

fn new_general_note_valid(
    record: &ParameterRecord,
    entries: &BTreeMap<u32, &DirectoryEntry>,
    ctx: &DecodeContext<'_>,
) -> Result<bool, CodecError> {
    if let Some(refusal) = ctx.resource_refusal() {
        return Err(refusal.into());
    }
    let parameter_end = record.parameter_end();
    let count = match record.count_with_stride_before_default_tail(12, 20, parameter_end) {
        DefaultTailCount::Held(count) if count > 0 => count,
        _ => return Ok(false),
    };
    Ok(parameter_end <= 13 + count * 20
        && (1..=2).all(|index| {
            record
                .number_or(index, 0.0)
                .is_some_and(|value| value.is_finite() && value >= 0.0)
        })
        && record.integer_or(3, 0).is_some_and(justification_valid)
        && (4..=11).all(|index| record.number_or(index, 0.0).is_some())
        && ctx.all_by(
            0..count,
            |index| {
                let start = 13 + index * 20;
                let fixed = record.integer_or(start, 0);
                let character_width = record.number_or(start + 1, 0.0);
                let character_height = record.number_or(start + 2, 0.0);
                // PS-01: variable-width CSPACE has an explicit default of one;
                // fixed-width CSPACE uses the generic real default of zero.
                let spacing_default = if fixed == Some(1) { 1.0 } else { 0.0 };
                let spacing = record.number_or(start + 3, spacing_default);
                let text = record.string_or_empty(start + 19);
                // PS-01: Type 213 FONT has no explicit default; the generic
                // integer default is zero.
                let font_style = record.integer_or(start + 5, 0);
                // PS-04: CHRSET has an entity-specific default of standard ASCII.
                let character_set = record.integer_or(start + 11, 1);
                let metrics_valid = character_width
                    .zip(character_height)
                    .zip(spacing)
                    .is_some_and(|((width, height), spacing)| {
                        width.is_finite()
                            && width > 0.0
                            && height.is_finite()
                            && height > 0.0
                            && spacing.is_finite()
                            && match fixed {
                                Some(0) => spacing >= -width,
                                Some(1) => spacing >= 0.0,
                                _ => false,
                            }
                    })
                    && fixed.is_some_and(fixed_or_variable_valid);
                let character_set_prefix_valid = metrics_valid
                    && record.number_or(start + 4, 0.0).is_some()
                    && record.number_or(start + 6, 0.0).is_some_and(|value| {
                        value.is_finite() && (0.0..=std::f64::consts::TAU).contains(&value)
                    })
                    && record.string_or_empty(start + 7).is_some()
                    && record
                        .integer(start + 8)
                        .and_then(|value| usize::try_from(value).ok())
                        .zip(text)
                        .is_some_and(|(declared, text)| declared == text.len())
                    && (start + 9..=start + 10).all(|field| {
                        record
                            .number_or(field, 0.0)
                            .is_some_and(|value| value.is_finite() && value >= 0.0)
                    })
                    && font_style.is_some_and(new_general_note_font_valid);
                let character_set_valid = character_set_prefix_valid
                    && match character_set {
                        Some(value) => new_general_note_charset_valid(value, entries, ctx)?,
                        None => false,
                    };
                Ok(character_set_valid
                    && record
                        .number_or(start + 12, std::f64::consts::FRAC_PI_2)
                        .is_some()
                    && record.number_or(start + 13, 0.0).is_some()
                    && record
                        .integer_or(start + 14, 0)
                        .is_some_and(mirror_flag_valid)
                    && record
                        .integer_or(start + 15, 0)
                        .is_some_and(vertical_text_flag_valid)
                    && (start + 16..=start + 18)
                        .all(|field| record.number_or(field, 0.0).is_some()))
            },
            "iges annotation validation traversal",
        )?)
}

fn leader_valid_for_global_table(
    entry: &DirectoryEntry,
    record: &ParameterRecord,
    global_table: GlobalTable,
    ctx: &DecodeContext<'_>,
) -> Result<bool, CodecError> {
    if let Some(refusal) = ctx.resource_refusal() {
        return Err(refusal.into());
    }
    let Some(count) = record
        .count_with_stride_at(1, 7, 2, record.parameter_end())
        .filter(|count| *count > 0)
    else {
        return Ok(false);
    };
    let dimensions_valid = record
        .number(2)
        .zip(record.number(3))
        .is_some_and(|(height, width)| {
            height.is_finite()
                && width.is_finite()
                && match global_table {
                    GlobalTable::V4_0 => matches!(entry.form, 1..=12),
                    _ => match entry.form {
                        4 => height == 0.0 && width == 0.0,
                        5 | 6 | 12 => height > 0.0 && height == width,
                        1..=3 | 7..=11 => height > 0.0 && width > 0.0,
                        _ => false,
                    },
                }
        });
    Ok(exact_parameter_count(record, 7 + count * 2)
        && dimensions_valid
        && ctx.all_by(
            4..=6 + count * 2,
            |index| Ok(finite(record, index)),
            "iges annotation validation traversal",
        )?)
}

fn pointer(
    record: &ParameterRecord,
    index: usize,
    entries: &BTreeMap<u32, &DirectoryEntry>,
    ctx: &DecodeContext<'_>,
) -> Result<Option<u32>, CodecError> {
    if let Some(refusal) = ctx.resource_refusal() {
        return Err(refusal.into());
    }
    let Some(sequence) = record
        .integer(index)
        .and_then(|value| u32::try_from(value).ok())
        .filter(|sequence| sequence % 2 == 1)
    else {
        return Ok(None);
    };
    Ok(ctx
        .contains_key_btree_map(
            entries,
            &sequence,
            "iges annotation directory pointer lookup",
        )?
        .then_some(sequence))
}

fn child_valid(
    sequence: u32,
    entity_type: i64,
    forms: impl Fn(i64) -> bool,
    entries: &BTreeMap<u32, &DirectoryEntry>,
    records: &BTreeMap<u32, &ParameterRecord>,
    global_table: GlobalTable,
    validation: &mut AnnotationValidation<'_, '_>,
) -> Result<bool, CodecError> {
    let Some(entry) = validation.ctx.get_btree_map(
        entries,
        &sequence,
        "iges annotation child Directory lookup",
    )?
    else {
        return Ok(false);
    };
    if entry.entity_type != entity_type
        || !forms(entry.form)
        || !entry.status.is_physically_dependent()
        || entry.status.use_flag(global_table) != Some(UseFlag::Annotation)
    {
        return Ok(false);
    }
    validation.primary_from_records(sequence, entry, entries, records, global_table)
}

fn general_note_child_valid(
    sequence: u32,
    entries: &BTreeMap<u32, &DirectoryEntry>,
    records: &BTreeMap<u32, &ParameterRecord>,
    global_table: GlobalTable,
    validation: &mut AnnotationValidation<'_, '_>,
) -> Result<bool, CodecError> {
    child_valid(
        sequence,
        212,
        crate::profile::general_note_form_admitted,
        entries,
        records,
        global_table,
        validation,
    )
}

fn general_symbol_note_valid(
    record: &ParameterRecord,
    entries: &BTreeMap<u32, &DirectoryEntry>,
    records: &BTreeMap<u32, &ParameterRecord>,
    form: i64,
    global_table: GlobalTable,
    validation: &mut AnnotationValidation<'_, '_>,
) -> Result<bool, CodecError> {
    if let Some(refusal) = validation.ctx.resource_refusal() {
        return Err(refusal.into());
    }
    Ok(match record.integer(1) {
        Some(0) => form == 0 && !matches!(global_table, GlobalTable::V4_0),
        Some(_) => pointer(record, 1, entries, validation.ctx)?
            .map(|sequence| -> Result<bool, CodecError> {
                general_note_child_valid(sequence, entries, records, global_table, validation)
            })
            .transpose()?
            .unwrap_or(false),
        None => false,
    })
}

fn dimension_enclosure_type_allowed(
    entity_type: i64,
    form: i64,
    global_table: GlobalTable,
) -> bool {
    matches!((entity_type, form), (100 | 102, 0))
        || (!matches!(global_table, GlobalTable::V4_0) && (entity_type, form) == (106, 63))
}

fn dimension_children_valid(
    parent: &DirectoryEntry,
    mut children: impl Iterator<Item = Result<Option<u32>, CodecError>>,
    entries: &BTreeMap<u32, &DirectoryEntry>,
    ctx: &DecodeContext<'_>,
) -> Result<bool, CodecError> {
    if let Some(refusal) = ctx.resource_refusal() {
        return Err(refusal.into());
    }
    let first_transform = loop {
        let Some(sequence) = children.next() else {
            return Ok(false);
        };
        let Some(sequence) = sequence? else {
            continue;
        };
        let Some(entry) =
            ctx.get_btree_map(entries, &sequence, "iges annotation child transform lookup")?
        else {
            return Ok(false);
        };
        break entry.transform;
    };
    if parent.transform != 0 && first_transform != 0 {
        return Ok(false);
    }
    for sequence in children {
        let Some(sequence) = sequence? else {
            continue;
        };
        let Some(entry) =
            ctx.get_btree_map(entries, &sequence, "iges annotation child transform lookup")?
        else {
            return Ok(false);
        };
        if entry.transform != first_transform {
            return Ok(false);
        }
    }
    Ok(true)
}

fn witness_valid(record: &ParameterRecord, ctx: &DecodeContext<'_>) -> Result<bool, CodecError> {
    if let Some(refusal) = ctx.resource_refusal() {
        return Err(refusal.into());
    }
    let Some(count) = record
        .count_with_stride_at(2, 4, 2, record.parameter_end())
        .filter(|count| *count >= 3 && *count % 2 == 1)
    else {
        return Ok(false);
    };
    Ok(record.integer(1) == Some(1)
        && exact_parameter_count(record, 4 + count * 2)
        && ctx.all_by(
            3..4 + count * 2,
            |index| Ok(finite(record, index)),
            "iges annotation validation traversal",
        )?)
}

pub(crate) fn parameterized_curve_type(entry: &DirectoryEntry) -> bool {
    matches!(
        entry.entity_type,
        100 | 102 | 104 | 106 | 110 | 112 | 126 | 130 | 142
    )
}

fn dimension_valid(
    entry: &DirectoryEntry,
    record: &ParameterRecord,
    entries: &BTreeMap<u32, &DirectoryEntry>,
    records: &BTreeMap<u32, &ParameterRecord>,
    global_table: GlobalTable,
    validation: &mut AnnotationValidation<'_, '_>,
) -> Result<bool, CodecError> {
    let ctx = validation.ctx;
    let note = pointer(record, 1, entries, ctx)?;
    let note_valid = note
        .map(|sequence| -> Result<bool, CodecError> {
            general_note_child_valid(sequence, entries, records, global_table, validation)
        })
        .transpose()?
        .unwrap_or(false);
    let fields_valid = match (entry.entity_type, entry.form) {
        (202, 0) => {
            let witnesses = [record.integer(2), record.integer(3)];
            let leaders = [
                pointer(record, 7, entries, ctx)?,
                pointer(record, 8, entries, ctx)?,
            ];
            let witnesses_valid = witnesses.iter().enumerate().try_fold(
                true,
                |valid, (offset, raw)| -> Result<bool, CodecError> {
                    Ok(valid
                        && match raw {
                            Some(0) => true,
                            Some(_) => pointer(record, 2 + offset, entries, ctx)?
                                .map(|sequence| -> Result<bool, CodecError> {
                                    child_valid(
                                        sequence,
                                        106,
                                        |form| form == 40,
                                        entries,
                                        records,
                                        global_table,
                                        validation,
                                    )
                                })
                                .transpose()?
                                .unwrap_or(false),
                            None => false,
                        })
                },
            )?;
            let leaders_valid =
                leaders
                    .iter()
                    .try_fold(true, |valid, leader| -> Result<bool, CodecError> {
                        Ok(valid && {
                            leader
                                .map(|sequence| -> Result<bool, CodecError> {
                                    child_valid(
                                        sequence,
                                        214,
                                        |form| matches!(form, 1..=12),
                                        entries,
                                        records,
                                        global_table,
                                        validation,
                                    )
                                })
                                .transpose()?
                                .unwrap_or(false)
                        })
                    })?;
            exact_parameter_count(record, 9)
                && witnesses_valid
                && (4..=5).all(|index| finite(record, index))
                && record
                    .number(6)
                    .is_some_and(|value| value.is_finite() && value > 0.0)
                && leaders_valid
        }
        (204, 0) => {
            let curves = [
                pointer(record, 2, entries, ctx)?,
                pointer(record, 3, entries, ctx)?,
            ];
            let curve_entries = [
                match curves[0] {
                    Some(sequence) => ctx
                        .get_btree_map(
                            entries,
                            &sequence,
                            "iges annotation dimension curve lookup",
                        )?
                        .copied(),
                    None => None,
                },
                match curves[1] {
                    Some(sequence) => ctx
                        .get_btree_map(
                            entries,
                            &sequence,
                            "iges annotation dimension curve lookup",
                        )?
                        .copied(),
                    None => None,
                },
            ];
            let curves_valid = curve_entries[0].is_some_and(|curve| {
                parameterized_curve_type(curve)
                    && curve.status.is_physically_dependent()
                    && curve.status.use_flag(global_table) == Some(UseFlag::Annotation)
            }) && match record.integer(3) {
                Some(0) => true,
                Some(_) => curve_entries[1].is_some_and(|curve| {
                    parameterized_curve_type(curve)
                        && curve.status.is_physically_dependent()
                        && curve.status.use_flag(global_table) == Some(UseFlag::Annotation)
                        && !(curve.entity_type == 110
                            && curve_entries[0].is_some_and(|first| first.entity_type == 110))
                }),
                None => false,
            };
            let leaders = [
                pointer(record, 4, entries, ctx)?,
                pointer(record, 5, entries, ctx)?,
            ];
            let leaders_valid =
                leaders
                    .iter()
                    .try_fold(true, |valid, leader| -> Result<bool, CodecError> {
                        Ok(valid && {
                            leader
                                .map(|sequence| -> Result<bool, CodecError> {
                                    child_valid(
                                        sequence,
                                        214,
                                        |form| matches!(form, 1..=12),
                                        entries,
                                        records,
                                        global_table,
                                        validation,
                                    )
                                })
                                .transpose()?
                                .unwrap_or(false)
                        })
                    })?;
            let witnesses_valid =
                (6..=7).try_fold(true, |valid, index| -> Result<bool, CodecError> {
                    Ok(valid
                        && match record.integer(index) {
                            Some(0) => true,
                            Some(_) => pointer(record, index, entries, ctx)?
                                .map(|sequence| -> Result<bool, CodecError> {
                                    child_valid(
                                        sequence,
                                        106,
                                        |form| form == 40,
                                        entries,
                                        records,
                                        global_table,
                                        validation,
                                    )
                                })
                                .transpose()?
                                .unwrap_or(false),
                            None => false,
                        })
                })?;
            exact_parameter_count(record, 8) && curves_valid && leaders_valid && witnesses_valid
        }
        (206, 0) => {
            let first = pointer(record, 2, entries, ctx)?;
            let second = pointer(record, 3, entries, ctx)?;
            let leaders_valid = first
                .map(|sequence| -> Result<bool, CodecError> {
                    child_valid(
                        sequence,
                        214,
                        |form| matches!(form, 1..=12),
                        entries,
                        records,
                        global_table,
                        validation,
                    )
                })
                .transpose()?
                .unwrap_or(false)
                && match record.integer(3) {
                    Some(0) => true,
                    Some(_) => second
                        .map(|sequence| -> Result<bool, CodecError> {
                            child_valid(
                                sequence,
                                214,
                                |form| matches!(form, 1..=12),
                                entries,
                                records,
                                global_table,
                                validation,
                            )
                        })
                        .transpose()?
                        .unwrap_or(false),
                    None => false,
                };
            exact_parameter_count(record, 6)
                && leaders_valid
                && (4..=5).all(|index| finite(record, index))
        }
        (216, 0..=2) => {
            let leaders = [
                pointer(record, 2, entries, ctx)?,
                pointer(record, 3, entries, ctx)?,
            ];
            let witnesses = [record.integer(4), record.integer(5)];
            let leaders_valid =
                leaders
                    .iter()
                    .try_fold(true, |valid, sequence| -> Result<bool, CodecError> {
                        Ok(valid && {
                            sequence
                                .map(|sequence| -> Result<bool, CodecError> {
                                    child_valid(
                                        sequence,
                                        214,
                                        |form| matches!(form, 1..=12),
                                        entries,
                                        records,
                                        global_table,
                                        validation,
                                    )
                                })
                                .transpose()?
                                .unwrap_or(false)
                        })
                    })?;
            let witnesses_valid = witnesses.iter().enumerate().try_fold(
                true,
                |valid, (offset, raw)| -> Result<bool, CodecError> {
                    Ok(valid
                        && match raw {
                            Some(0) => true,
                            Some(_) => pointer(record, 4 + offset, entries, ctx)?
                                .map(|sequence| -> Result<bool, CodecError> {
                                    child_valid(
                                        sequence,
                                        106,
                                        |form| form == 40,
                                        entries,
                                        records,
                                        global_table,
                                        validation,
                                    )
                                })
                                .transpose()?
                                .unwrap_or(false),
                            None => false,
                        })
                },
            )?;
            exact_parameter_count(record, 6) && leaders_valid && witnesses_valid
        }
        (218, 0) => {
            let ordinate = pointer(record, 2, entries, ctx)?;
            let valid = ordinate
                .map(|sequence| -> Result<bool, CodecError> {
                    Ok(child_valid(
                        sequence,
                        106,
                        |form| form == 40,
                        entries,
                        records,
                        global_table,
                        validation,
                    )? || child_valid(
                        sequence,
                        214,
                        |form| matches!(form, 1..=12),
                        entries,
                        records,
                        global_table,
                        validation,
                    )?)
                })
                .transpose()?
                .unwrap_or(false);
            exact_parameter_count(record, 3) && valid
        }
        (218, 1) => {
            let witness = pointer(record, 2, entries, ctx)?;
            let leader = pointer(record, 3, entries, ctx)?;
            let valid = witness
                .map(|sequence| -> Result<bool, CodecError> {
                    child_valid(
                        sequence,
                        106,
                        |form| form == 40,
                        entries,
                        records,
                        global_table,
                        validation,
                    )
                })
                .transpose()?
                .unwrap_or(false)
                && leader
                    .map(|sequence| -> Result<bool, CodecError> {
                        child_valid(
                            sequence,
                            214,
                            |form| matches!(form, 1..=12),
                            entries,
                            records,
                            global_table,
                            validation,
                        )
                    })
                    .transpose()?
                    .unwrap_or(false);
            exact_parameter_count(record, 4) && valid
        }
        (220, 0) => {
            let leader = pointer(record, 2, entries, ctx)?;
            let enclosure_raw = record.integer(3);
            let enclosure = pointer(record, 3, entries, ctx)?;
            let leader_valid = leader
                .map(|sequence| -> Result<bool, CodecError> {
                    Ok(child_valid(
                        sequence,
                        214,
                        |form| matches!(form, 1..=12),
                        entries,
                        records,
                        global_table,
                        validation,
                    )? && ctx
                        .get_btree_map(
                            records,
                            &sequence,
                            "iges annotation dimension leader lookup",
                        )?
                        .and_then(|record| record.integer(1))
                        == Some(3))
                })
                .transpose()?
                .unwrap_or(false);
            let enclosure_valid = match enclosure_raw {
                Some(0) => true,
                Some(_) => match enclosure {
                    Some(sequence) => ctx
                        .get_btree_map(entries, &sequence, "iges annotation enclosure lookup")?
                        .is_some_and(|entry| {
                            dimension_enclosure_type_allowed(
                                entry.entity_type,
                                entry.form,
                                global_table,
                            ) && entry.status.is_physically_dependent()
                                && entry.status.use_flag(global_table) == Some(UseFlag::Annotation)
                        }),
                    None => false,
                },
                None => false,
            };
            exact_parameter_count(record, 4) && leader_valid && enclosure_valid
        }
        (222, 0..=1) => {
            let first = pointer(record, 2, entries, ctx)?;
            let first_valid = first
                .map(|sequence| -> Result<bool, CodecError> {
                    child_valid(
                        sequence,
                        214,
                        |form| matches!(form, 1..=12),
                        entries,
                        records,
                        global_table,
                        validation,
                    )
                })
                .transpose()?
                .unwrap_or(false);
            let center_valid = finite(record, 3) && finite(record, 4);
            let second_raw = (entry.form == 1).then(|| record.integer(5)).flatten();
            let second = (entry.form == 1)
                .then(|| pointer(record, 5, entries, ctx))
                .transpose()?
                .flatten();
            let second_valid = entry.form == 0
                || match second_raw {
                    Some(0) => true,
                    Some(_) => second
                        .map(|sequence| -> Result<bool, CodecError> {
                            child_valid(
                                sequence,
                                214,
                                |form| matches!(global_table, GlobalTable::V4_0) || form == 4,
                                entries,
                                records,
                                global_table,
                                validation,
                            )
                        })
                        .transpose()?
                        .unwrap_or(false),
                    None => false,
                };
            exact_parameter_count(record, if entry.form == 0 { 5 } else { 6 })
                && first_valid
                && center_valid
                && second_valid
        }
        _ => false,
    };
    let child_indexes: &[usize] = match (entry.entity_type, entry.form) {
        (202, 0) => &[2, 3, 7, 8],
        (204, 0) => &[2, 3, 4, 5, 6, 7],
        (206 | 220, 0) | (218, 1) => &[2, 3],
        (216, 0..=2) => &[2, 3, 4, 5],
        (218 | 222, 0) => &[2],
        (222, 1) => &[2, 5],
        _ => &[],
    };
    let children = note
        .into_iter()
        .map(|sequence| Ok::<_, CodecError>(Some(sequence)))
        .chain(
            child_indexes
                .iter()
                .map(|index| pointer(record, *index, entries, ctx)),
        );
    Ok(note_valid && fields_valid && dimension_children_valid(entry, children, entries, ctx)?)
}

fn flag_or_label_valid(
    entry: &DirectoryEntry,
    record: &ParameterRecord,
    entries: &BTreeMap<u32, &DirectoryEntry>,
    records: &BTreeMap<u32, &ParameterRecord>,
    global_table: GlobalTable,
    validation: &mut AnnotationValidation<'_, '_>,
) -> Result<bool, CodecError> {
    let ctx = validation.ctx;
    let (note_index, count_index, leader_start) = if entry.entity_type == 208 {
        (5, 6, 7)
    } else {
        (1, 2, 3)
    };
    let note = pointer(record, note_index, entries, ctx)?;
    let note_valid = note
        .map(|sequence| -> Result<bool, CodecError> {
            general_note_child_valid(sequence, entries, records, global_table, validation)
        })
        .transpose()?
        .unwrap_or(false);
    let count = record.count(count_index);
    let leaders_valid = count
        .map(|count| -> Result<bool, CodecError> {
            ctx.all_by(
                0..count,
                |offset| {
                    Ok(pointer(record, leader_start + offset, entries, ctx)?
                        .map(|sequence| -> Result<bool, CodecError> {
                            child_valid(
                                sequence,
                                214,
                                |form| matches!(form, 1..=12),
                                entries,
                                records,
                                global_table,
                                validation,
                            )
                        })
                        .transpose()?
                        .unwrap_or(false))
                },
                "iges annotation validation traversal",
            )
        })
        .transpose()?
        .unwrap_or(false);
    let shape_valid = if entry.entity_type == 208 {
        count.is_some_and(|count| exact_parameter_count(record, 7 + count))
            && (1..=4).all(|index| finite(record, index))
            && match note {
                Some(sequence) => validation
                    .width_sum_from_records(sequence, records)?
                    .is_some_and(|total| total <= 10),
                None => false,
            }
    } else {
        count.is_some_and(|count| count > 0 && exact_parameter_count(record, 3 + count))
    };
    Ok(note_valid && leaders_valid && shape_valid)
}

fn general_symbol_valid(
    record: &ParameterRecord,
    entries: &BTreeMap<u32, &DirectoryEntry>,
    records: &BTreeMap<u32, &ParameterRecord>,
    form: i64,
    global_table: GlobalTable,
    validation: &mut AnnotationValidation<'_, '_>,
) -> Result<bool, CodecError> {
    let ctx = validation.ctx;
    let note_valid =
        general_symbol_note_valid(record, entries, records, form, global_table, validation)?;
    let Some(geometry_count) = record.count(2).filter(|count| *count > 0) else {
        return Ok(false);
    };
    let geometry_valid = ctx.all_by(
        0..geometry_count,
        |offset| {
            let Some(sequence) = pointer(record, 3 + offset, entries, ctx)? else {
                return Ok(false);
            };
            Ok(ctx
                .get_btree_map(entries, &sequence, "iges annotation symbol geometry lookup")?
                .is_some_and(|target| {
                    target.status.is_physically_dependent()
                        && target.status.use_flag(global_table) == Some(UseFlag::Annotation)
                }))
        },
        "iges annotation validation traversal",
    )?;
    let leader_count_index = 3 + geometry_count;
    let Some(leader_count) = record.count(leader_count_index) else {
        return Ok(false);
    };
    let leaders_valid = ctx.all_by(
        0..leader_count,
        |offset| {
            Ok(
                pointer(record, leader_count_index + 1 + offset, entries, ctx)?
                    .map(|sequence| -> Result<bool, CodecError> {
                        child_valid(
                            sequence,
                            214,
                            |form| matches!(form, 1..=12),
                            entries,
                            records,
                            global_table,
                            validation,
                        )
                    })
                    .transpose()?
                    .unwrap_or(false),
            )
        },
        "iges annotation validation traversal",
    )?;
    Ok(note_valid
        && geometry_valid
        && leaders_valid
        && exact_parameter_count(record, leader_count_index + 1 + leader_count))
}

pub(crate) fn section_boundary_type(entry: &DirectoryEntry) -> bool {
    matches!(
        (entry.entity_type, entry.form),
        (100 | 102 | 112 | 126, 0) | (104, 1) | (106, 63)
    )
}

fn fill_pattern_valid_for_global_table(pattern: i64, global_table: GlobalTable) -> bool {
    if matches!(global_table, GlobalTable::V4_0) {
        return (0..=19).contains(&pattern);
    }
    matches!(
        pattern,
        0..=20 | 22 | 26 | 28..=29 | 32 | 34 | 36 | 38 | 40..=42 | 46 | 50 | 60
            | 70 | 72 | 80 | 82 | 84 | 86 | 90 | 92 | 94 | 110 | 124 | 134 | 136
            | 140 | 142 | 152 | 154 | 156..=159 | 172 | 174 | 178 | 210 | 220 | 224
            | 226 | 234 | 236 | 240 | 244 | 246 | 252 | 254 | 256 | 262 | 264..=266
            | 268
    )
}

fn zero_or_omitted(record: &ParameterRecord, index: usize) -> bool {
    match record.value(index) {
        None | Some(crate::parameter::TokenValue::Omitted) => true,
        _ => record.number(index) == Some(0.0),
    }
}

fn finite_or_omitted(record: &ParameterRecord, index: usize) -> bool {
    match record.value(index) {
        None | Some(crate::parameter::TokenValue::Omitted) => true,
        _ => finite(record, index),
    }
}

#[derive(Clone, Copy)]
struct SectionedAreaContext {
    global_table: GlobalTable,
    transform: Transform,
    length_factor: f64,
    resolution: f64,
}

fn sectioned_area_valid<'ctx>(
    geometry: &mut SectionedAreaGeometryCache<'ctx, '_>,
    record: &ParameterRecord,
    entries: &BTreeMap<u32, &DirectoryEntry>,
    form: i64,
    context: SectionedAreaContext,
    ctx: &'ctx DecodeContext<'_>,
) -> Result<bool, CodecError> {
    if let Some(refusal) = ctx.resource_refusal() {
        return Err(refusal.into());
    }
    let SectionedAreaContext {
        global_table,
        transform,
        length_factor,
        resolution,
    } = context;
    if !matches!(form, 0 | 1) {
        return Ok(false);
    }
    let boundary_sequence = match record.integer(1) {
        Some(0) if form == 1 => Some(None),
        Some(_) => pointer(record, 1, entries, ctx)?.map(Some),
        None => None,
    };
    let boundary_valid = match boundary_sequence {
        Some(Some(sequence)) => ctx
            .get_btree_map(entries, &sequence, "iges section boundary lookup")?
            .is_some_and(|entry| section_boundary_type(*entry)),
        Some(None) => true,
        None => false,
    };
    let Some(island_count) = record.count(8) else {
        return Ok(false);
    };
    if form == 1 && island_count == 0 {
        return Ok(false);
    }
    let islands_valid = ctx.all_by(
        0..island_count,
        |offset| {
            let Some(sequence) = pointer(record, 9 + offset, entries, ctx)? else {
                return Ok(false);
            };
            Ok(ctx
                .get_btree_map(entries, &sequence, "iges section island boundary lookup")?
                .is_some_and(|entry| section_boundary_type(entry)))
        },
        "iges annotation validation traversal",
    )?;
    let coplanarity_valid = if matches!(global_table, GlobalTable::V4_0) {
        true
    } else if let Some(pattern_plane) =
        sectioned_area_pattern_plane(record, transform, length_factor)
    {
        let boundary_coplanar = match boundary_sequence {
            Some(Some(sequence)) => {
                geometry.curve_coplanar(sequence, pattern_plane, resolution, ctx)?
            }
            Some(None) => true,
            None => false,
        };
        boundary_coplanar
            && ctx.all_by(
                0..island_count,
                |offset| {
                    let Some(sequence) = pointer(record, 9 + offset, entries, ctx)? else {
                        return Ok(false);
                    };
                    geometry.curve_coplanar(sequence, pattern_plane, resolution, ctx)
                },
                "iges section island reference traversal",
            )?
    } else {
        false
    };
    let pattern = record
        .integer(2)
        .filter(|value| fill_pattern_valid_for_global_table(*value, global_table));
    let pattern_parameters_valid = pattern.is_some_and(|pattern| {
        if matches!(pattern, 0 | 19) || pattern > 19 {
            (3..=7).all(|index| zero_or_omitted(record, index))
        } else {
            finite_or_omitted(record, 3)
                && finite_or_omitted(record, 4)
                && finite(record, 5)
                && record
                    .number(6)
                    .is_some_and(|distance| distance.is_finite() && distance > 0.0)
                && finite_or_omitted(record, 7)
        }
    });
    Ok(boundary_valid
        && pattern_parameters_valid
        && islands_valid
        && coplanarity_valid
        && exact_parameter_count(record, 9 + island_count))
}

pub(super) fn project<'ctx>(
    ir: &CadIr,
    directory: &[DirectoryEntry],
    (entries, records): (
        &BTreeMap<u32, &DirectoryEntry>,
        &BTreeMap<u32, &ParameterRecord>,
    ),
    global: &ProjectedGlobal,
    ctx: &'ctx DecodeContext<'_>,
) -> Result<ProjectionOutcome<'ctx>, CodecError> {
    let mut validation = AnnotationValidation::new(ctx)?;
    let mut section_geometry = SectionedAreaGeometryCache::new(ir, ctx)?;
    let mut decoded_storage = ctx.reserve_scoped(0, "iges annotation decoded sequences")?;
    let mut decoded = BTreeSet::new();
    let mut loss_slots_storage = ctx.reserve_scoped(0, "iges entity loss slots")?;
    let mut losses = Vec::new();

    let mut directory_entries = directory.iter();
    while !directory_entries.as_slice().is_empty() {
        let Some(entry) = ctx.next_charged(
            &mut directory_entries,
            "iges annotation directory traversal",
        )?
        else {
            break;
        };
        let Some(kind) = classify(entry.entity_type, entry.form) else {
            continue;
        };
        let valid = ctx
            .get_btree_map(
                records,
                &entry.sequence,
                "iges annotation Parameter Data lookup",
            )?
            .map(|record| -> Result<bool, CodecError> {
                let mut transform_storage =
                    ctx.reserve_scoped(0, "iges annotation transform scratch")?;
                let resolved_transform = match transform_storage.with_storage(|| {
                    resolve_transform(
                        entry.transform,
                        entries,
                        records,
                        global.length_factor_mm(),
                        global.real_precision(),
                        &mut BTreeSet::new(),
                        ctx,
                    )
                }) {
                    Ok(transform) => Some(transform),
                    Err(error) => {
                        error.non_resource()?;
                        None
                    }
                };
                let transform_valid = resolved_transform.is_some();
                Ok(
                    entry.status.use_flag(global.global_table()) == Some(UseFlag::Annotation)
                        && transform_valid
                        && match kind {
                            AnnotationKind::AngularDimension
                            | AnnotationKind::CurveDimension
                            | AnnotationKind::DiameterDimension
                            | AnnotationKind::LinearDimension
                            | AnnotationKind::OrdinateDimension
                            | AnnotationKind::PointDimension
                            | AnnotationKind::RadiusDimension => dimension_valid(
                                entry,
                                record,
                                entries,
                                records,
                                global.global_table(),
                                &mut validation,
                            )?,
                            AnnotationKind::FlagNote | AnnotationKind::GeneralLabel => {
                                flag_or_label_valid(
                                    entry,
                                    record,
                                    entries,
                                    records,
                                    global.global_table(),
                                    &mut validation,
                                )?
                            }
                            AnnotationKind::GeneralNote | AnnotationKind::Leader => {
                                validation.primary(entry, record, entries, global.global_table())?
                            }
                            AnnotationKind::NewGeneralNote => {
                                new_general_note_valid(record, entries, ctx)?
                            }
                            AnnotationKind::GeneralSymbol => general_symbol_valid(
                                record,
                                entries,
                                records,
                                entry.form,
                                global.global_table(),
                                &mut validation,
                            )?,
                            AnnotationKind::SectionedArea => {
                                if let Some(transform) = resolved_transform {
                                    sectioned_area_valid(
                                        &mut section_geometry,
                                        record,
                                        entries,
                                        entry.form,
                                        SectionedAreaContext {
                                            global_table: global.global_table(),
                                            transform,
                                            length_factor: global.length_factor_mm(),
                                            resolution: global.minimum_resolution_mm(),
                                        },
                                        ctx,
                                    )?
                                } else {
                                    false
                                }
                            }
                        },
                )
            })
            .transpose()?
            .unwrap_or(false);
        if valid {
            ctx.insert_scoped_btree_set(
                &mut decoded_storage,
                &mut decoded,
                entry.sequence,
                "iges annotation decoded sequences",
                "iges annotation decoded sequences",
            )?;
        } else {
            let message = match kind {
                AnnotationKind::AngularDimension
                | AnnotationKind::CurveDimension
                | AnnotationKind::DiameterDimension
                | AnnotationKind::LinearDimension
                | AnnotationKind::OrdinateDimension
                | AnnotationKind::PointDimension
                | AnnotationKind::RadiusDimension => {
                    "dimension components, role types, transforms, or Directory status are invalid"
                }
                AnnotationKind::GeneralSymbol => {
                    "symbol note, defining geometry, or leader list is invalid"
                }
                AnnotationKind::SectionedArea => {
                    "section boundary, fill pattern, hatch geometry, or island list is invalid"
                }
                AnnotationKind::FlagNote
                | AnnotationKind::GeneralLabel
                | AnnotationKind::GeneralNote
                | AnnotationKind::NewGeneralNote
                | AnnotationKind::Leader => "text count, presentation metrics, encoding, placement, or Directory use flag is invalid",
            };
            super::push_entity_loss_with_scoped_slots(
                ctx,
                &mut loss_slots_storage,
                &mut losses,
                entry,
                format_args!("{message}"),
            )?;
        }
    }

    Ok(ProjectionOutcome {
        decoded,
        decoded_storage,
        losses,
        loss_slots_storage,
    })
}

#[cfg(test)]
mod tests;
