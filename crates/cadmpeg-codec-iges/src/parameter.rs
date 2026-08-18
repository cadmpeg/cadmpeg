// SPDX-License-Identifier: Apache-2.0
//! Parameter Data assembly and count-driven token spans.

use crate::card::{CardScan, PhysicalLine, Section};
use crate::directory::DirectoryEntry;
use crate::global::{Global, RealPrecision};
use cadmpeg_core::decode::DecodeContext;
use cadmpeg_core::CodecError;
use std::collections::BTreeMap;
use std::ops::Range;

/// One typed lexical value in an entity parameter record.
#[derive(Debug, Clone, PartialEq)]
pub(crate) enum TokenValue {
    Omitted,
    Integer(i64),
    Real(f64),
    String(Vec<u8>),
}

/// Typed value and its half-open offset in the assembled 64-column stream.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct Token {
    pub(crate) value: TokenValue,
    pub(crate) span: Range<usize>,
}

/// One entity's assembled Parameter Data.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct ParameterRecord {
    pub(crate) directory_sequence: u32,
    pub(crate) line_range: Range<u32>,
    pub(crate) bytes: Vec<u8>,
    pub(crate) tokens: Vec<Token>,
    /// Exclusive end of the entity-specific Parameter Data sequence.
    ///
    /// `tokens` retains the complete record so native preservation and
    /// relationship analysis can inspect trailing pointer groups. Entity
    /// accessors stop at this boundary.
    pub(crate) parameter_end: usize,
    pub(crate) comment: Vec<u8>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct TrailingPointerGroups {
    pub(crate) token_start: usize,
    pub(crate) associations: Vec<u32>,
    pub(crate) properties: Vec<u32>,
    pub(crate) association_pointers: Vec<TrailingPointer>,
    pub(crate) property_pointers: Vec<TrailingPointer>,
    pub(crate) fully_valid: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct TrailingPointerAnalysis {
    pub(crate) candidate_count: usize,
    pub(crate) valid_candidate_count: usize,
    pub(crate) groups: Option<TrailingPointerGroups>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct TrailingPointer {
    pub(crate) token_index: usize,
    pub(crate) raw_pointer: i64,
}

impl ParameterRecord {
    pub(crate) fn parameter_end(&self) -> usize {
        self.parameter_end.min(self.tokens.len())
    }

    pub(crate) fn token(&self, index: usize) -> Option<&Token> {
        (index < self.parameter_end()).then(|| self.tokens.get(index))?
    }

    pub(crate) fn value(&self, index: usize) -> Option<&TokenValue> {
        Some(&self.token(index)?.value)
    }

    fn raw_value(&self, index: usize) -> Option<&TokenValue> {
        self.tokens.get(index).map(|token| &token.value)
    }

    fn raw_integer(&self, index: usize) -> Option<i64> {
        match self.raw_value(index)? {
            TokenValue::Integer(value) => Some(*value),
            TokenValue::Omitted | TokenValue::Real(_) | TokenValue::String(_) => None,
        }
    }

    pub(crate) fn integer(&self, index: usize) -> Option<i64> {
        match self.value(index)? {
            TokenValue::Integer(value) => Some(*value),
            TokenValue::Omitted | TokenValue::Real(_) | TokenValue::String(_) => None,
        }
    }

    pub(crate) fn integer_or(&self, index: usize, default: i64) -> Option<i64> {
        let token = match self.tokens.get(index) {
            None => return Some(default),
            Some(_) if index >= self.parameter_end() => return None,
            Some(token) => token,
        };
        match &token.value {
            TokenValue::Omitted => Some(default),
            TokenValue::Integer(value) => Some(*value),
            TokenValue::Real(_) | TokenValue::String(_) => None,
        }
    }

    pub(crate) fn number(&self, index: usize) -> Option<f64> {
        match self.value(index)? {
            TokenValue::Integer(value) => Some(*value as f64),
            TokenValue::Real(value) => Some(*value),
            TokenValue::Omitted | TokenValue::String(_) => None,
        }
    }

    pub(crate) fn number_or(&self, index: usize, default: f64) -> Option<f64> {
        let token = match self.tokens.get(index) {
            None => return Some(default),
            Some(_) if index >= self.parameter_end() => return None,
            Some(token) => token,
        };
        match &token.value {
            TokenValue::Omitted => Some(default),
            TokenValue::Integer(value) => Some(*value as f64),
            TokenValue::Real(value) => Some(*value),
            TokenValue::String(_) => None,
        }
    }

    /// Return the sending-system significance for a real token. A `D`
    /// exponent selects double precision; every other real syntax selects
    /// single precision. Integer tokens are exact and have no such bound.
    pub(crate) fn number_uncertainty(
        &self,
        index: usize,
        value: f64,
        precision: RealPrecision,
    ) -> f64 {
        self.number_significance_with(index, precision)
            .map_or(0.0, |digits| {
                if value == 0.0 {
                    0.0
                } else {
                    0.5 * 10.0_f64.powf(value.abs().log10().floor() - f64::from(digits) + 1.0)
                }
            })
    }

    fn number_significance_with(&self, index: usize, precision: RealPrecision) -> Option<u32> {
        let token = self.token(index)?;
        if !matches!(token.value, TokenValue::Real(_)) {
            return None;
        }
        let bytes = self.bytes.get(token.span.clone())?;
        if bytes.iter().any(|byte| matches!(byte, b'D' | b'd')) {
            Some(precision.double_significance)
        } else {
            Some(precision.single_significance)
        }
    }

    pub(crate) fn string(&self, index: usize) -> Option<&[u8]> {
        match self.value(index)? {
            TokenValue::String(value) => Some(value),
            TokenValue::Omitted | TokenValue::Integer(_) | TokenValue::Real(_) => None,
        }
    }

    pub(crate) fn string_or_empty(&self, index: usize) -> Option<&[u8]> {
        let token = match self.tokens.get(index) {
            None => return Some(&[]),
            Some(_) if index >= self.parameter_end() => return None,
            Some(token) => token,
        };
        match &token.value {
            TokenValue::Omitted => Some(&[]),
            TokenValue::String(value) => Some(value),
            TokenValue::Integer(_) | TokenValue::Real(_) => None,
        }
    }

    /// Return a nonnegative declared list count only when at least that many
    /// tokens remain in this record. Each list item consumes one or more
    /// tokens, so this is a format-derived upper bound for every count-driven
    /// loop before its entity-specific stride is validated.
    pub(crate) fn count(&self, index: usize) -> Option<usize> {
        self.count_with_stride(index, 1)
    }

    /// Return a nonnegative declared count only when all fixed-width items fit.
    pub(crate) fn count_with_stride(&self, index: usize, stride: usize) -> Option<usize> {
        self.count_with_stride_before(index, stride, self.parameter_end())
    }

    /// Return a nonnegative declared count only when all fixed-width items fit
    /// before the entity-specific end of the parameter sequence.
    pub(crate) fn count_with_stride_before(
        &self,
        index: usize,
        stride: usize,
        end: usize,
    ) -> Option<usize> {
        let item_start = index.checked_add(1)?;
        self.count_with_stride_at(index, item_start, stride, end)
    }

    /// Return a nonnegative declared count only when all fixed-width items fit
    /// from an explicit item start before the entity-specific end.
    pub(crate) fn count_with_stride_at(
        &self,
        index: usize,
        item_start: usize,
        stride: usize,
        end: usize,
    ) -> Option<usize> {
        let count = self
            .integer(index)
            .and_then(|value| usize::try_from(value).ok())?;
        let required = count.checked_mul(stride)?;
        let end = end.min(self.parameter_end());
        (required <= end.saturating_sub(item_start)).then_some(count)
    }

    /// Return a nonnegative declared count before the entity-specific end when
    /// the final item may omit trailing defaulted fields at the record delimiter.
    pub(crate) fn count_with_stride_before_default_tail(
        &self,
        index: usize,
        stride: usize,
        end: usize,
    ) -> Option<usize> {
        if stride == 0 {
            return None;
        }
        let end = end.min(self.parameter_end());
        if end < self.tokens.len() {
            return self.count_with_stride_before(index, stride, end);
        }
        let item_start = index.checked_add(1)?;
        let count = self
            .integer(index)
            .and_then(|value| usize::try_from(value).ok())?;
        let available = end.saturating_sub(item_start);
        let max_count = available.checked_add(stride - 1)?.checked_div(stride)?;
        (count <= max_count).then_some(count)
    }
}

pub(crate) fn trailing_pointer_groups(
    record: &ParameterRecord,
    directory: &BTreeMap<u32, &DirectoryEntry>,
) -> Option<TrailingPointerGroups> {
    analyze_trailing_pointer_groups(record, directory)
        .groups
        .filter(|groups| groups.fully_valid)
}

pub(crate) fn analyze_trailing_pointer_groups(
    record: &ParameterRecord,
    directory: &BTreeMap<u32, &DirectoryEntry>,
) -> TrailingPointerAnalysis {
    // IGES defines the group order and pointer classes, but the entity table
    // supplies NV when it defines the primary layout. Use that table boundary
    // before applying the generic CADIR recovery for an entity without a
    // registered layout.
    let candidates = match entity_primary_end(record, directory) {
        Some(start) => {
            let prefix = non_integer_prefix(record);
            pointer_group_candidate_with_prefix(record, start, &prefix)
                .into_iter()
                .collect()
        }
        None => structural_pointer_group_candidates(record),
    };
    let valid_groups = candidates
        .iter()
        .filter_map(|candidate| groups_for_candidate(record, directory, *candidate))
        .filter(|groups| groups.fully_valid);
    let valid_groups = valid_groups.collect::<Vec<_>>();
    let groups = match valid_groups.as_slice() {
        [groups] => Some(groups.clone()),
        [] if candidates.len() == 1 => groups_for_candidate(record, directory, candidates[0]),
        _ => None,
    };
    TrailingPointerAnalysis {
        candidate_count: candidates.len(),
        valid_candidate_count: valid_groups.len(),
        groups,
    }
}

/// Return `NV + 1`, the token index at which the trailing pointer groups
/// start, for an entity layout established in the supported format tables.
///
/// The entity type token is index zero. Type 102 §4.4 has `N` at index 1 and
/// `N` constituent pointers at indexes 2 through `N + 1`, so its groups start
/// at token `N + 2`. Type 106 uses the IP-dependent tuple spans in §§4.6–4.11:
/// IP 1 starts at token 4 with width 2, IP 2 starts at token 3 with width 3,
/// and IP 3 starts at token 3 with width 6. Type 110 §4.13 has six primary
/// coordinates in Forms 0–2, so its additional-pointer groups start at token
/// seven. Type 116 §4.16 has three coordinates and a display pointer, so its
/// groups start at token five even when that pointer is zero or defaulted.
/// Type 123 §4.20 has three primary values, so its groups start at token four.
/// Type 402 Forms 1, 7, 14, and 15 use `N` plus `N` member pointers, so their
/// groups start at token `N + 2`.
/// Type 402 Form 5 puts `N` at index 1 and seven fields per label placement,
/// so its groups start at token `2 + 7*N`.
/// Type 402 Form 6 fixes index 1 to one, puts the visible-entity count `N1` at
/// index 2, and lists the view plus `N1` entities, so its groups start at
/// token `4 + N1`.
/// Type 402 Form 9 requires `NP=1`, puts `NC` at index 2, the parent at index 3,
/// and `NC` child pointers at indexes 4 through `3 + NC`, so its groups start
/// at token `4 + NC`.
/// Type 230 Form 0 puts the island count at index 8 and consumes one pointer
/// per island, so its groups start at token `9 + N`; zero islands is valid.
/// Type 320 Form 0 puts `NA` at index 3 and `NC` after the fixed `TF`, `PRD`,
/// and `DPTR` fields, so its groups start at token `8 + NA + NC` after the
/// entity type token. Both counts may be zero when their lists fit.
/// Type 184 Forms 0 and 1 put `N` at index 1, followed by `N` item pointers
/// and `N` transformation pointers, so their groups start at token `2 + 2*N`.
/// Type 412 Form 0 puts `LC` at index 11, followed by the fixed `DDF` flag and
/// `LC` position numbers, so its groups start at token `13 + LC`.
/// Type 414 Form 0 puts `LC` at index 9, followed by the fixed `DDF` flag and
/// `LC` position numbers, so its groups start at token `11 + LC`.
/// Type 114 Form 0 puts `M` and `N` at indexes 3 and 4 and stores a complete
/// `(M + 1) * (N + 1)` grid of 48-value patch and placeholder blocks, so its
/// groups start at token `7 + M + N + 48*(M + 1)*(N + 1)`.
/// Type 128 Forms 0 through 9 define `K1`, `K2`, `M1`, `M2`, `A`, `B`, and `C`,
/// so their groups start at token `16 + A + B + 4*C`, with
/// `A = 1 + K1 + M1`, `B = 1 + K2 + M2`, and `C = (K1 + 1) * (K2 + 1)`.
/// Type 144 Form 0 puts the inner-boundary count at index 3, so its groups
/// start at token `5 + N2` after the entity type token.
/// Type 143 Form 0 puts the boundary count at index 3, so its groups start at
/// token `4 + N` after the entity type token.
/// Type 141 Form 0 puts the model-curve count at index 4. Each item consumes
/// three fields plus its `K` parameter-curve pointers, so its groups start at
/// token `5 + 3*N + sum(K(i))` after the entity type token.
/// Type 126 Forms 0 through 5 define `K`, `M`, and `A = 1 + K + M`, so their
/// groups start at token `18 + 5*K + M`.
/// Type 112 Form 0 puts `N` at index 4 and stores thirteen primary tokens per
/// segment after the first breakpoint, so its groups start at token
/// `18 + 13*N`.
/// Layouts not represented here use generic CADIR recovery. A malformed known
/// layout returns the record end as a sentinel and never enables generic
/// recovery.
pub(crate) fn entity_primary_end(
    record: &ParameterRecord,
    directory: &BTreeMap<u32, &DirectoryEntry>,
) -> Option<usize> {
    let entry = directory.get(&record.directory_sequence)?;
    match (entry.entity_type, entry.form) {
        (102, 0) | (402, 1 | 7 | 14 | 15) => Some(counted_primary_end(record)),
        (402, 5) => Some(label_display_primary_end(record)),
        (402, 6) => Some(view_list_primary_end(record)),
        (402, 9) => Some(single_parent_primary_end(record)),
        (230, 0) => Some(sectioned_area_primary_end(record)),
        (320, 0) => Some(network_subfigure_primary_end(record)),
        (184, 0 | 1) => Some(solid_assembly_primary_end(record)),
        (412, 0) => Some(rectangular_array_primary_end(record)),
        (414, 0) => Some(circular_array_primary_end(record)),
        (106, form) if copious_expected_interpretation(form).is_some() => {
            Some(copious_primary_end(record, form))
        }
        (110, 0..=2) => Some(7),
        (112, 0) => Some(parametric_spline_curve_primary_end(record)),
        (114, 0) => Some(parametric_spline_surface_primary_end(record)),
        (116, 0) => Some(5),
        (123, 0) => Some(4),
        (126, 0..=5) => Some(rational_bspline_curve_primary_end(record)),
        (128, 0..=9) => Some(rational_bspline_surface_primary_end(record)),
        (141, 0) => Some(boundary_primary_end(record)),
        (143, 0) => Some(bounded_surface_primary_end(record)),
        (144, 0) => Some(trimmed_surface_primary_end(record)),
        _ => None,
    }
}

fn counted_primary_end(record: &ParameterRecord) -> usize {
    record
        .integer(1)
        .and_then(|value| usize::try_from(value).ok())
        .filter(|count| *count > 0)
        .and_then(|count| count.checked_add(2))
        .unwrap_or(record.tokens.len())
}

fn label_display_primary_end(record: &ParameterRecord) -> usize {
    record
        .integer(1)
        .and_then(|value| usize::try_from(value).ok())
        .filter(|count| *count > 0)
        .and_then(|count| count.checked_mul(7))
        .and_then(|span| span.checked_add(2))
        .filter(|end| *end <= record.tokens.len())
        .unwrap_or(record.tokens.len())
}

fn view_list_primary_end(record: &ParameterRecord) -> usize {
    if record.integer(1) != Some(1) {
        return record.tokens.len();
    }
    record
        .integer(2)
        .and_then(|value| usize::try_from(value).ok())
        .and_then(|count| count.checked_add(4))
        .filter(|end| *end <= record.tokens.len())
        .unwrap_or(record.tokens.len())
}

fn single_parent_primary_end(record: &ParameterRecord) -> usize {
    if record.integer(1) != Some(1) {
        return record.tokens.len();
    }
    record
        .integer(2)
        .and_then(|value| usize::try_from(value).ok())
        .filter(|count| *count > 0)
        .and_then(|count| count.checked_add(4))
        .unwrap_or(record.tokens.len())
}

fn sectioned_area_primary_end(record: &ParameterRecord) -> usize {
    record
        .integer(8)
        .and_then(|value| usize::try_from(value).ok())
        .and_then(|count| count.checked_add(9))
        .filter(|end| *end <= record.tokens.len())
        .unwrap_or(record.tokens.len())
}

fn network_subfigure_primary_end(record: &ParameterRecord) -> usize {
    let Some(member_count) = record
        .integer(3)
        .and_then(|value| usize::try_from(value).ok())
    else {
        return record.tokens.len();
    };
    let Some(connect_count_index) = member_count.checked_add(7) else {
        return record.tokens.len();
    };
    let Some(connect_count) = record
        .integer(connect_count_index)
        .and_then(|value| usize::try_from(value).ok())
    else {
        return record.tokens.len();
    };
    member_count
        .checked_add(connect_count)
        .and_then(|count| count.checked_add(8))
        .filter(|end| *end <= record.tokens.len())
        .unwrap_or(record.tokens.len())
}

fn solid_assembly_primary_end(record: &ParameterRecord) -> usize {
    let Some(item_count) = record
        .integer(1)
        .and_then(|value| usize::try_from(value).ok())
        .filter(|count| *count > 0)
    else {
        return record.tokens.len();
    };
    item_count
        .checked_mul(2)
        .and_then(|span| span.checked_add(2))
        .filter(|end| *end <= record.tokens.len())
        .unwrap_or(record.tokens.len())
}

fn rectangular_array_primary_end(record: &ParameterRecord) -> usize {
    record
        .integer(11)
        .and_then(|value| usize::try_from(value).ok())
        .and_then(|list_count| list_count.checked_add(13))
        .filter(|end| *end <= record.tokens.len())
        .unwrap_or(record.tokens.len())
}

fn circular_array_primary_end(record: &ParameterRecord) -> usize {
    record
        .integer(9)
        .and_then(|value| usize::try_from(value).ok())
        .and_then(|list_count| list_count.checked_add(11))
        .filter(|end| *end <= record.tokens.len())
        .unwrap_or(record.tokens.len())
}

fn rational_bspline_curve_primary_end(record: &ParameterRecord) -> usize {
    let Some(k) = record
        .integer(1)
        .and_then(|value| usize::try_from(value).ok())
    else {
        return record.tokens.len();
    };
    let Some(degree) = record
        .integer(2)
        .and_then(|value| usize::try_from(value).ok())
    else {
        return record.tokens.len();
    };
    if k < degree {
        return record.tokens.len();
    }
    k.checked_mul(5)
        .and_then(|span| span.checked_add(degree))
        .and_then(|span| span.checked_add(18))
        .unwrap_or(record.tokens.len())
}

fn parametric_spline_curve_primary_end(record: &ParameterRecord) -> usize {
    let Some(segment_count) = record
        .integer(4)
        .and_then(|value| usize::try_from(value).ok())
        .filter(|count| *count > 0)
    else {
        return record.tokens.len();
    };
    segment_count
        .checked_mul(13)
        .and_then(|span| span.checked_add(18))
        .unwrap_or(record.tokens.len())
}

fn parametric_spline_surface_primary_end(record: &ParameterRecord) -> usize {
    let Some(u_segments) = record
        .integer(3)
        .and_then(|value| usize::try_from(value).ok())
        .filter(|count| *count > 0)
    else {
        return record.tokens.len();
    };
    let Some(v_segments) = record
        .integer(4)
        .and_then(|value| usize::try_from(value).ok())
        .filter(|count| *count > 0)
    else {
        return record.tokens.len();
    };
    let Some(u_blocks) = u_segments.checked_add(1) else {
        return record.tokens.len();
    };
    let Some(v_blocks) = v_segments.checked_add(1) else {
        return record.tokens.len();
    };
    u_blocks
        .checked_mul(v_blocks)
        .and_then(|block_count| block_count.checked_mul(48))
        .and_then(|block_span| block_span.checked_add(7))
        .and_then(|start| start.checked_add(u_segments))
        .and_then(|start| start.checked_add(v_segments))
        .unwrap_or(record.tokens.len())
}

fn rational_bspline_surface_primary_end(record: &ParameterRecord) -> usize {
    let Some(k1) = record
        .integer(1)
        .and_then(|value| usize::try_from(value).ok())
    else {
        return record.tokens.len();
    };
    let Some(k2) = record
        .integer(2)
        .and_then(|value| usize::try_from(value).ok())
    else {
        return record.tokens.len();
    };
    let Some(m1) = record
        .integer(3)
        .and_then(|value| usize::try_from(value).ok())
    else {
        return record.tokens.len();
    };
    let Some(m2) = record
        .integer(4)
        .and_then(|value| usize::try_from(value).ok())
    else {
        return record.tokens.len();
    };
    if k1 < m1 || k2 < m2 {
        return record.tokens.len();
    }
    let Some(a) = k1.checked_add(m1).and_then(|value| value.checked_add(1)) else {
        return record.tokens.len();
    };
    let Some(b) = k2.checked_add(m2).and_then(|value| value.checked_add(1)) else {
        return record.tokens.len();
    };
    let Some(c) = k1.checked_add(1).and_then(|u_count| {
        k2.checked_add(1)
            .and_then(|v_count| u_count.checked_mul(v_count))
    }) else {
        return record.tokens.len();
    };
    c.checked_mul(4)
        .and_then(|span| span.checked_add(16))
        .and_then(|span| span.checked_add(a))
        .and_then(|span| span.checked_add(b))
        .unwrap_or(record.tokens.len())
}

fn bounded_surface_primary_end(record: &ParameterRecord) -> usize {
    record
        .integer(3)
        .and_then(|value| usize::try_from(value).ok())
        .and_then(|boundary_count| boundary_count.checked_add(4))
        .unwrap_or(record.tokens.len())
}

fn boundary_primary_end(record: &ParameterRecord) -> usize {
    let Some(segment_count) = record
        .integer(4)
        .and_then(|value| usize::try_from(value).ok())
        .filter(|count| *count > 0)
    else {
        return record.tokens.len();
    };
    let mut index = 5;
    for _ in 0..segment_count {
        let Some(pcurve_count) = record
            .integer(index + 2)
            .and_then(|value| usize::try_from(value).ok())
        else {
            return record.tokens.len();
        };
        let Some(next_index) = index
            .checked_add(3)
            .and_then(|start| start.checked_add(pcurve_count))
        else {
            return record.tokens.len();
        };
        index = next_index;
    }
    index
}

fn trimmed_surface_primary_end(record: &ParameterRecord) -> usize {
    record
        .integer(3)
        .and_then(|value| usize::try_from(value).ok())
        .and_then(|inner_count| inner_count.checked_add(5))
        .unwrap_or(record.tokens.len())
}

fn copious_expected_interpretation(form: i64) -> Option<i64> {
    match form {
        1 | 11 | 20 | 21 | 31..=38 | 40 | 63 => Some(1),
        2 | 12 => Some(2),
        3 | 13 => Some(3),
        _ => None,
    }
}

fn copious_primary_end(record: &ParameterRecord, form: i64) -> usize {
    let Some(expected_interpretation) = copious_expected_interpretation(form) else {
        return record.tokens.len();
    };
    let Some(interpretation) = record.integer(1) else {
        return record.tokens.len();
    };
    if interpretation != expected_interpretation {
        return record.tokens.len();
    }
    let Some(tuple_count) = record
        .integer(2)
        .and_then(|value| usize::try_from(value).ok())
        .filter(|count| *count > 0)
    else {
        return record.tokens.len();
    };
    let (tuple_start, tuple_width): (usize, usize) = match interpretation {
        1 => (4, 2),
        2 => (3, 3),
        3 => (3, 6),
        _ => return record.tokens.len(),
    };
    tuple_count
        .checked_mul(tuple_width)
        .and_then(|span| tuple_start.checked_add(span))
        .unwrap_or(record.tokens.len())
}

#[derive(Clone, Copy)]
struct PointerGroupCandidate {
    token_start: usize,
    association_start: usize,
    property_count_index: usize,
    property_count: usize,
}

fn pointer_group_candidate_with_prefix(
    record: &ParameterRecord,
    association_count_index: usize,
    non_integer_prefix: &[usize],
) -> Option<PointerGroupCandidate> {
    let association_count = record
        .raw_integer(association_count_index)
        .and_then(|value| usize::try_from(value).ok())?;
    let association_start = association_count_index.checked_add(1)?;
    let property_count_index = association_start.checked_add(association_count)?;
    let property_count = record
        .raw_integer(property_count_index)
        .and_then(|value| usize::try_from(value).ok())?;
    if association_count == 0 && property_count == 0 {
        return None;
    }
    let property_start = property_count_index.checked_add(1)?;
    let end = property_start.checked_add(property_count)?;
    if end != record.tokens.len()
        || non_integer_prefix[property_count_index] != non_integer_prefix[association_start]
        || non_integer_prefix[end] != non_integer_prefix[property_start]
    {
        return None;
    }
    Some(PointerGroupCandidate {
        token_start: association_count_index,
        association_start,
        property_count_index,
        property_count,
    })
}

fn structural_pointer_group_candidates(record: &ParameterRecord) -> Vec<PointerGroupCandidate> {
    let non_integer_prefix = non_integer_prefix(record);
    (1..record.tokens.len())
        .filter_map(|association_count_index| {
            pointer_group_candidate_with_prefix(
                record,
                association_count_index,
                &non_integer_prefix,
            )
        })
        .collect()
}

fn non_integer_prefix(record: &ParameterRecord) -> Vec<usize> {
    let mut prefix = Vec::with_capacity(record.tokens.len() + 1);
    prefix.push(0);
    for index in 0..record.tokens.len() {
        prefix.push(prefix[index] + usize::from(record.raw_integer(index).is_none()));
    }
    prefix
}

fn pointer_is_valid(
    record: &ParameterRecord,
    index: usize,
    directory: &BTreeMap<u32, &DirectoryEntry>,
    association: bool,
) -> bool {
    let Some(sequence) = record
        .raw_integer(index)
        .and_then(|value| u32::try_from(value).ok())
        .filter(|sequence| sequence % 2 == 1)
    else {
        return false;
    };
    directory.get(&sequence).is_some_and(|entry| {
        if association {
            matches!(entry.entity_type, 212 | 312 | 402)
        } else {
            matches!(entry.entity_type, 316 | 322 | 406 | 422)
        }
    })
}

fn groups_for_candidate(
    record: &ParameterRecord,
    directory: &BTreeMap<u32, &DirectoryEntry>,
    candidate: PointerGroupCandidate,
) -> Option<TrailingPointerGroups> {
    let association_pointers = (candidate.association_start..candidate.property_count_index)
        .map(|token_index| {
            Some(TrailingPointer {
                token_index,
                raw_pointer: record.raw_integer(token_index)?,
            })
        })
        .collect::<Option<Vec<_>>>()?;
    let property_start = candidate.property_count_index.checked_add(1)?;
    let property_end = property_start.checked_add(candidate.property_count)?;
    let property_pointers = (property_start..property_end)
        .map(|token_index| {
            Some(TrailingPointer {
                token_index,
                raw_pointer: record.raw_integer(token_index)?,
            })
        })
        .collect::<Option<Vec<_>>>()?;
    let associations = association_pointers
        .iter()
        .filter_map(|pointer| {
            u32::try_from(pointer.raw_pointer)
                .ok()
                .filter(|sequence| sequence % 2 == 1)
                .filter(|sequence| {
                    directory
                        .get(sequence)
                        .is_some_and(|entry| matches!(entry.entity_type, 212 | 312 | 402))
                })
        })
        .collect();
    let properties = property_pointers
        .iter()
        .filter_map(|pointer| {
            u32::try_from(pointer.raw_pointer)
                .ok()
                .filter(|sequence| sequence % 2 == 1)
                .filter(|sequence| {
                    directory
                        .get(sequence)
                        .is_some_and(|entry| matches!(entry.entity_type, 316 | 322 | 406 | 422))
                })
        })
        .collect();
    let fully_valid = association_pointers
        .iter()
        .all(|pointer| pointer_is_valid(record, pointer.token_index, directory, true))
        && property_pointers
            .iter()
            .all(|pointer| pointer_is_valid(record, pointer.token_index, directory, false));
    Some(TrailingPointerGroups {
        token_start: candidate.token_start,
        associations,
        properties,
        association_pointers,
        property_pointers,
        fully_valid,
    })
}

fn malformed(sequence: u32, message: impl Into<String>) -> CodecError {
    crate::error::malformed(format!(
        "IGES parameters for D{sequence}: {}",
        message.into()
    ))
}

fn positive_u32(value: i64, sequence: u32, name: &str) -> Result<u32, CodecError> {
    u32::try_from(value).map_err(|_| malformed(sequence, format!("{name} is not a positive u32")))
}

fn back_pointer(line: &PhysicalLine) -> Result<u32, CodecError> {
    let field = line.payload.get(64..72).ok_or_else(|| {
        CodecError::Malformed(format!(
            "IGES Parameter Data card P{} is shorter than 72 bytes",
            line.sequence.unwrap_or_default()
        ))
    })?;
    let text = std::str::from_utf8(field)
        .map_err(|_| CodecError::Malformed("IGES Parameter Data back-pointer is not ASCII".into()))?
        .trim();
    text.parse::<u32>()
        .map_err(|_| CodecError::Malformed("IGES Parameter Data back-pointer is not a u32".into()))
}

fn hollerith(
    bytes: &[u8],
    start: usize,
    sequence: u32,
) -> Result<Option<(Token, usize)>, CodecError> {
    let mut cursor = start;
    while bytes.get(cursor).is_some_and(u8::is_ascii_digit) {
        cursor += 1;
    }
    if cursor == start || !matches!(bytes.get(cursor), Some(b'H' | b'h')) {
        return Ok(None);
    }
    let count = std::str::from_utf8(&bytes[start..cursor])
        .map_err(|_| malformed(sequence, "Hollerith count is not ASCII"))?
        .parse::<usize>()
        .map_err(|_| malformed(sequence, "Hollerith count is out of range"))?;
    let payload_start = cursor
        .checked_add(1)
        .ok_or_else(|| malformed(sequence, "Hollerith offset overflow"))?;
    let end = payload_start
        .checked_add(count)
        .ok_or_else(|| malformed(sequence, "Hollerith length overflow"))?;
    let payload = bytes
        .get(payload_start..end)
        .ok_or_else(|| malformed(sequence, "Hollerith payload is truncated"))?;
    Ok(Some((
        Token {
            value: TokenValue::String(payload.to_vec()),
            span: start..end,
        },
        end,
    )))
}

fn numeric(bytes: &[u8], span: Range<usize>, sequence: u32) -> Result<Token, CodecError> {
    let text = std::str::from_utf8(&bytes[span.clone()])
        .map_err(|_| malformed(sequence, "numeric token is not ASCII"))?
        .trim();
    if text.is_empty() {
        return Ok(Token {
            value: TokenValue::Omitted,
            span,
        });
    }
    let real = text
        .bytes()
        .any(|byte| matches!(byte, b'.' | b'E' | b'e' | b'D' | b'd'));
    let value = if real {
        let normalized = text.replace(['D', 'd'], "E");
        TokenValue::Real(
            normalized
                .parse::<f64>()
                .map_err(|_| malformed(sequence, format!("invalid real token {text:?}")))?,
        )
    } else {
        TokenValue::Integer(
            text.parse::<i64>()
                .map_err(|_| malformed(sequence, format!("invalid integer token {text:?}")))?,
        )
    };
    Ok(Token { value, span })
}

fn tokenize(
    bytes: &[u8],
    parameter_delimiter: u8,
    record_delimiter: u8,
    sequence: u32,
    ctx: Option<&DecodeContext<'_>>,
) -> Result<(Vec<Token>, usize), CodecError> {
    let mut tokens = Vec::new();
    let mut cursor = 0_usize;
    loop {
        if bytes.get(cursor) == Some(&record_delimiter) {
            return Ok((tokens, cursor + 1));
        }
        if bytes.get(cursor) == Some(&parameter_delimiter) {
            charge_token(ctx)?;
            tokens.push(Token {
                value: TokenValue::Omitted,
                span: cursor..cursor,
            });
            cursor += 1;
            continue;
        }
        let (token, end) = if let Some(value) = hollerith(bytes, cursor, sequence)? {
            value
        } else {
            let end = bytes[cursor..]
                .iter()
                .position(|byte| {
                    matches!(*byte, value if value == parameter_delimiter || value == record_delimiter)
                })
                .and_then(|relative| cursor.checked_add(relative))
                .ok_or_else(|| malformed(sequence, "record delimiter is missing"))?;
            if end == cursor {
                return Err(malformed(sequence, "empty token has no delimiter"));
            }
            (numeric(bytes, cursor..end, sequence)?, end)
        };
        charge_token(ctx)?;
        tokens.push(token);
        match bytes.get(end).copied() {
            Some(value) if value == parameter_delimiter => cursor = end + 1,
            Some(value) if value == record_delimiter => return Ok((tokens, end + 1)),
            _ => return Err(malformed(sequence, "token is not followed by a delimiter")),
        }
    }
}

pub(crate) fn assemble_with_context(
    scan: &CardScan,
    directory: &[DirectoryEntry],
    global: &Global,
    ctx: Option<&DecodeContext<'_>>,
) -> Result<Vec<ParameterRecord>, CodecError> {
    let lines = scan
        .lines
        .iter()
        .filter(|line| line.section == Some(Section::Parameter))
        .map(|line| (line.sequence.unwrap_or_default(), line))
        .collect::<BTreeMap<_, _>>();
    let entries = directory
        .iter()
        .filter(|entry| !(entry.entity_type == 0 && entry.parameter_line_count == 0))
        .map(|entry| (entry.sequence, entry))
        .collect::<BTreeMap<_, _>>();
    let mut owned_by_entry = BTreeMap::<u32, Vec<u32>>::new();
    for (sequence, line) in &lines {
        let pointer = back_pointer(line)?;
        if pointer == 0 || pointer % 2 == 0 || !entries.contains_key(&pointer) {
            return Err(CodecError::Malformed(format!(
                "IGES Parameter Data card P{sequence} back-pointer {pointer} is not an owning odd Directory Entry sequence"
            )));
        }
        owned_by_entry.entry(pointer).or_default().push(*sequence);
    }
    let mut records = Vec::new();
    for entry in directory {
        if entry.parameter_line_count == 0 && entry.entity_type == 0 {
            continue;
        }
        let start = positive_u32(
            entry.parameter_start,
            entry.sequence,
            "Parameter Data start",
        )?;
        let count = positive_u32(
            entry.parameter_line_count,
            entry.sequence,
            "Parameter Data line count",
        )?;
        if count == 0 {
            return Err(malformed(
                entry.sequence,
                "Parameter Data line count is zero",
            ));
        }
        let owned = owned_by_entry
            .get(&entry.sequence)
            .map_or(&[][..], Vec::as_slice);
        let actual_start = owned.first().copied().ok_or_else(|| {
            malformed(
                entry.sequence,
                "no Parameter Data card points to this Directory Entry",
            )
        })?;
        let actual_end = owned
            .last()
            .copied()
            .and_then(|sequence| sequence.checked_add(1))
            .ok_or_else(|| malformed(entry.sequence, "Parameter Data range overflow"))?;
        if actual_start != start || owned.windows(2).any(|pair| pair[1] != pair[0] + 1) {
            return Err(malformed(
                entry.sequence,
                "Parameter Data back-pointer range is not contiguous at the declared start",
            ));
        }
        let declared_count = usize::try_from(count)
            .map_err(|_| malformed(entry.sequence, "Parameter Data count overflows usize"))?;
        if owned.len() != declared_count {
            return Err(malformed(
                entry.sequence,
                format!(
                    "declares {declared_count} Parameter Data cards but owns {} by back-pointer",
                    owned.len()
                ),
            ));
        }
        let mut bytes = Vec::new();
        for sequence in actual_start..actual_end {
            let line = lines.get(&sequence).ok_or_else(|| {
                malformed(
                    entry.sequence,
                    format!("Parameter Data card P{sequence} is missing"),
                )
            })?;
            bytes.extend_from_slice(&line.payload[..64]);
        }
        let (tokens, record_end) = tokenize(
            &bytes,
            global.parameter_delimiter,
            global.record_delimiter,
            entry.sequence,
            ctx,
        )?;
        if !matches!(tokens.first().map(|token| &token.value), Some(TokenValue::Integer(value)) if *value == entry.entity_type)
        {
            return Err(malformed(
                entry.sequence,
                "first parameter does not match the Directory Entry entity type",
            ));
        }
        let mut record = ParameterRecord {
            directory_sequence: entry.sequence,
            line_range: actual_start..actual_end,
            comment: bytes[record_end..].to_vec(),
            bytes,
            tokens,
            parameter_end: 0,
        };
        record.parameter_end = trailing_pointer_groups(&record, &entries)
            .map_or(record.tokens.len(), |groups| groups.token_start);
        records.push(record);
    }
    Ok(records)
}

fn charge_token(ctx: Option<&DecodeContext<'_>>) -> Result<(), CodecError> {
    ctx.map_or(Ok(()), |ctx| {
        ctx.charge_collection_items(1, "iges_parameter_tokens")
    })
}

pub(crate) fn summary_notes(records: &[ParameterRecord]) -> Vec<String> {
    vec![
        format!("parameter_records={}", records.len()),
        format!(
            "parameter_tokens={}",
            records
                .iter()
                .map(|record| record.tokens.len())
                .sum::<usize>()
        ),
        format!(
            "external_references={}",
            records
                .iter()
                .filter(|record| record.integer(0) == Some(416))
                .count()
        ),
    ]
}

#[cfg(test)]
mod tests;
