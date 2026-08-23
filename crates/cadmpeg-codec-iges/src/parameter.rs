// SPDX-License-Identifier: Apache-2.0
//! Parameter Data assembly and count-driven token spans.

use crate::card::{CardScan, FramingDefect, FramingRecoveries, PhysicalLine, Section};
use crate::directory::{DirectoryEntry, QuarantinedDirectoryRecord};
use crate::global::{RealPrecision, ResolvedGlobal};
use crate::loss::IgesLossCode;
use cadmpeg_core::decode::{bounded_len, DecodeContext};
use cadmpeg_core::CodecError;
use cadmpeg_ir::report::LossNote;
use cadmpeg_ir::SourceProvenance;
use std::collections::{BTreeMap, BTreeSet};
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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum DefaultTailCount {
    Held(usize),
    Overdeclared { declared: usize, present: usize },
    Unreadable,
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

    /// Report the declared count before the entity-specific end when the final
    /// item may omit trailing defaulted fields at the record delimiter.
    pub(crate) fn count_with_stride_before_default_tail(
        &self,
        index: usize,
        stride: usize,
        end: usize,
    ) -> DefaultTailCount {
        match index.checked_add(1) {
            Some(item_start) => {
                self.count_with_stride_before_default_tail_at(index, item_start, stride, end)
            }
            None => DefaultTailCount::Unreadable,
        }
    }

    /// Report the declared count when the record delimiter defaults no field
    /// of the list, so every declared item must be present whole.
    pub(crate) fn count_with_stride_at_complete(
        &self,
        index: usize,
        item_start: usize,
        stride: usize,
        end: usize,
    ) -> DefaultTailCount {
        if stride == 0 {
            return DefaultTailCount::Unreadable;
        }
        let Some(count) = self
            .integer(index)
            .and_then(|value| usize::try_from(value).ok())
        else {
            return DefaultTailCount::Unreadable;
        };
        let present = end
            .min(self.parameter_end())
            .saturating_sub(item_start)
            .div_euclid(stride);
        if count <= present {
            DefaultTailCount::Held(count)
        } else {
            DefaultTailCount::Overdeclared {
                declared: count,
                present,
            }
        }
    }

    /// Report the declared count before the entity-specific end from an
    /// explicit item start.
    pub(crate) fn count_with_stride_before_default_tail_at(
        &self,
        index: usize,
        item_start: usize,
        stride: usize,
        end: usize,
    ) -> DefaultTailCount {
        let Some(count) = self
            .integer(index)
            .and_then(|value| usize::try_from(value).ok())
        else {
            return DefaultTailCount::Unreadable;
        };
        let Some(present) = self.items_before_default_tail_at(item_start, stride, end) else {
            return DefaultTailCount::Unreadable;
        };
        if count <= present {
            DefaultTailCount::Held(count)
        } else {
            DefaultTailCount::Overdeclared {
                declared: count,
                present,
            }
        }
    }

    /// Return the fixed-width items the record holds from `item_start` and
    /// before the entity-specific end. A list that runs to the record end
    /// holds its final item in whole or in part, because the record delimiter
    /// supplies every remaining field under IGES 5.3 §2.2.3. A list that a
    /// trailing suffix follows holds only its complete items.
    pub(crate) fn items_before_default_tail_at(
        &self,
        item_start: usize,
        stride: usize,
        end: usize,
    ) -> Option<usize> {
        if stride == 0 {
            return None;
        }
        let end = end.min(self.parameter_end());
        let available = end.saturating_sub(item_start);
        Some(if end < self.tokens.len() {
            available / stride
        } else {
            available.div_ceil(stride)
        })
    }
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
/// Type 402 Form 3 puts positive `N1` and nonnegative `N2` at indexes 1 and 2,
/// followed by `N1` view pointers and `N2` entity pointers, so its groups start
/// at token `3 + N1 + N2`. Form 4 uses five fields per view block, so its groups
/// start at token `3 + 5*N1 + N2`.
/// Type 402 Forms 2 and 12 put the positive entry count `N` at index 1 and
/// store a name/pointer pair per entry, so their groups start at token
/// `2 + 2*N`.
/// Type 406 Form 1 puts positive `NP` at index 1 and stores `NP` level numbers,
/// so its groups start at token `2 + NP`.
/// Type 406 Form 2 fixes `NP=3` at index 1 and stores the three restriction
/// values at indexes 2 through 4, so its groups start at token five.
/// Type 406 Form 3 fixes `NP=2` at index 1 and stores its function code and
/// description at indexes 2 and 3, so its groups start at token four.
/// Type 406 Form 6 fixes `NP=5` at index 1 and stores five fixed values, so its
/// groups start at token seven.
/// Type 406 Forms 18, 19, 20, and 21 fix `NP=1` at index 1 and store one
/// fixed value at index 2, so their groups start at token three. Form 22 fixes
/// `NP=9` and stores its nine fixed values at indexes 2 through 10, so its
/// groups start at token eleven. Form 23 fixes `NP=2` and stores `TYPE` and
/// `NAME` at indexes 2 and 3, so its groups start at token four.
/// Type 406 Form 8 fixes `NP=1` at index 1 and stores its pin number at index
/// 2, so its groups start at token three.
/// Type 406 Form 9 fixes `NP=4` at index 1 and stores four part-number strings
/// at indexes 2 through 5, so its groups start at token six.
/// Type 406 Form 10 fixes `NP=6` at index 1 and stores six hierarchy values
/// at indexes 2 through 7, so its groups start at token eight.
/// Type 406 Form 12 puts positive `NP` at index 1 and stores `NP` external
/// reference file-name strings, so its groups start at token `2 + NP`.
/// Type 406 Form 13 permits `NP=2` or `NP=3` at index 1 and stores the
/// nominal size and name at indexes 2 and 3, with the optional standard at
/// index 4 for `NP=3`, so its groups start at token four or five.
/// Type 406 Form 14 puts positive `NP` at index 1 and stores `NP` flow-line
/// specification strings, so its groups start at token `2 + NP`.
/// Type 406 Form 15 fixes `NP=1` at index 1 and stores the name at index 2,
/// so its groups start at token three.
/// Type 406 Forms 16 and 17 fix `NP=2` at index 1 and store two drawing
/// property fields at indexes 2 and 3, so their groups start at token four.
/// Type 406 Form 24 puts `NLD` at index 2 and stores four fields per level
/// definition, so its groups start at token `3 + 4*NLD`; `NP` is `1 + 4*NLD`.
/// Type 406 Form 25 puts `NV` at index 3 and stores one level number per value,
/// so its groups start at token `4 + NV`; `NP` is `2 + NV`.
/// Type 406 Form 26 fixes `NP=3` at index 1 and stores three fixed values,
/// so its groups start at token five.
/// Type 406 Form 28 fixes `NP=6` at index 1 and stores `SPOS`, `UI`, `CHRSET`,
/// `USTRING`, `FFLAG`, and `PREC` at indexes 2 through 7, so its groups start
/// at token eight. `CHRSET` may be empty and then defaults to standard ASCII.
/// Type 406 Form 29 fixes `NP=8` at index 1 and stores `SFLAG`, `TYP`, `TPFLAG`,
/// `UTOL`, `LTOL`, `SSPFLG`, `FFLAG`, and `PREC` at indexes 2 through 9, so its
/// groups start at token ten. `TPFLAG` may be omitted and then defaults to 2.
/// Type 406 Form 31 fixes `NP=8` at index 1 and stores four two-coordinate box
/// corners at indexes 2 through 9, so its groups start at token ten.
/// Type 406 Form 32 fixes `NP=3` at index 1 and stores `NAME`, `ORG`, and
/// `DATE` at indexes 2 through 4, so its groups start at token five.
/// Type 406 Form 33 fixes `NP=2` at index 1 and stores `SNUM` and `SID` at
/// indexes 2 and 3, so its groups start at token four.
/// Type 406 Form 36 permits `NP=1` for a curve or `NP=2` for a surface and
/// stores `CLOSEDU` and, for a surface, `CLOSEDV`, so its groups start at
/// token `2 + NP`.
/// Type 402 Form 13 fixes `ND` to one, puts the positive geometry count `NG` at
/// index 2, and lists the dimension plus `NG` geometry pointers, so its groups
/// start at token `4 + NG`.
/// Type 402 Form 18 fixes `NCF` to two, puts the six class counts at indexes
/// 2 through 7, and stores the six class lists after the two flags at indexes
/// 8 and 9, so its groups start at token
/// `10 + NF + NC + NJ + NN + NT + NP`. Zero class counts are valid.
/// Type 402 Form 20 fixes `NCF` to one, puts the six class counts at indexes
/// 2 through 7, and stores the six class lists after the type flag at index 8,
/// so its groups start at token
/// `9 + NF + NC + NJ + NN + NT + NP`. Zero class counts are valid.
/// Type 402 Form 8 puts `NS`, `N1`, `N2`, and `N3` at indexes 1 through 4,
/// followed by the four counted classes, so its groups start at token
/// `5 + NS + N1 + N2 + N3`.
/// Type 402 Form 10 puts `NP` and `NTD` at indexes 1 and 2, `NP` point
/// pointers at index 3, and one seven-field text description, so its groups
/// start at token `10 + NP`.
/// Type 402 Form 11 puts `NC` and `NP` at indexes 1 and 2, followed by `NC`
/// point pointers and `NP` data values, so its groups start at token
/// `3 + NC + NP`.
/// Type 406 Forms 34 and 35 put `NP = 1 + 3*ND` at index 1, `ND` at index 2,
/// and one three-integer text-score range per specification, so their groups
/// start at token `3 + 3*ND`.
/// Type 406 Form 30 fixes `NP = 14`, puts `K` at index 13, and appends three
/// supplemental-note fields per `K`, so its groups start at token `14 + 3*K`.
/// Type 406 Form 27 puts `NV` at index 3 and stores `NV` `(TYP, VAL)` pairs
/// from index 4, so its groups start at token `4 + 2*NV`; `NP` must equal
/// `2 + 2*NV`.
/// Type 406 Form 11 puts `ND` and `NI` at indexes 3 and 4, followed by `NI`
/// type slots, `NI` count slots, the concatenated independent values, and
/// `ND` values at every Cartesian point, so its groups start after that
/// complete nested span.
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
/// Type 214 Forms 1 through 12 put `N` at index 1 and store `N` pairs of
/// segment-tail coordinates after the fixed fields, so their groups start at
/// token `7 + 2*N`.
/// Type 218 Form 0 stores two fixed pointers and starts its groups at token 3;
/// Form 1 stores three fixed pointers and starts its groups at token 4.
/// Type 412 Form 0 puts `LC` at index 11, followed by the fixed `DDF` flag and
/// `LC` position numbers, so its groups start at token `13 + LC`.
/// Type 414 Form 0 puts `LC` at index 9, followed by the fixed `DDF` flag and
/// `LC` position numbers, so its groups start at token `11 + LC`.
/// Type 420 Form 0 puts `NC` at index 11 and stores one connect-point pointer
/// per count, so its groups start at token `12 + NC`.
/// Type 430 Forms 0 and 1 put the solid pointer at index 1, so their groups
/// start at token 2.
/// Type 408 Form 0 stores a definition pointer, three translation values, and
/// an optional scale at indexes 1 through 5, so its groups start at token 6.
/// Type 410 Form 0 stores eight view fields at indexes 1 through 8, so its
/// groups start at token 9; Form 1 stores twenty-two perspective fields, so
/// its groups start at token 23.
/// Type 416 Forms 0, 2, and 4 store two identifier strings at indexes 1 and 2,
/// so their groups start at token 3; Forms 1 and 3 store one identifier string
/// at index 1, so their groups start at token 2.
/// Type 132 Form 0 stores fourteen fixed primary fields, so its groups start
/// at token 15.
/// Type 402 Form 19 puts the block count at index 1 and stores six fields per
/// view/segment block, so its groups start at token `2 + 6*N`.
/// Type 406 Form 4 stores three fixed primary fields, so its groups start at
/// token 4.
/// Type 202 Form 0 stores eight fixed primary fields, so its groups start at
/// token nine.
/// Type 204 Form 0 stores seven fixed primary fields, so its groups start at
/// token eight.
/// Type 206 Form 0 stores five fixed primary fields, so its groups start at
/// token six.
/// Type 216 Forms 0 through 2 store five fixed primary fields, so their groups
/// start at token six.
/// Type 220 Form 0 stores three fixed primary fields, so its groups start at
/// token four.
/// Type 222 Form 0 stores four fixed primary fields, so its groups start at
/// token five. Form 1 stores one additional fixed leader pointer, so its
/// groups start at token six.
/// Type 100 Form 0 stores seven fixed primary fields, so its groups start at
/// token eight.
/// Type 104 Forms 0 through 3 store eleven fixed primary fields, so their
/// groups start at token twelve.
/// Type 108 Forms -1 through 1 store nine fixed primary fields, so their
/// groups start at token ten.
/// Type 312 Forms 0 and 1 store ten fixed primary fields, so their groups
/// start at token eleven.
/// Type 314 Form 0 stores three color coordinates and an optional color name,
/// so its groups start at token five after the explicit name slot.
/// Type 304 Form 1 stores four fixed values, so its groups start at token five;
/// Form 2 stores `M` segment lengths and one hexadecimal pattern, so its groups
/// start at token `M + 3`.
/// Type 310 Form 0 puts `N` at index 5. Each character adds four fields and
/// three fields per pen motion, so its groups start after the complete nested
/// character and motion span.
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
/// Type 142 Form 0 stores five fixed primary fields, so its groups start at
/// token 6.
/// Type 208 Form 0 puts the nonnegative leader count `N` at index 6 and the
/// `N` leader pointers at indexes 7 through `6 + N`, so its groups start at
/// token `7 + N`; zero leaders are valid.
/// Type 210 Form 0 puts the positive leader count `N` at index 2 and the `N`
/// leader pointers at indexes 3 through `2 + N`, so its groups start at token
/// `3 + N`.
/// Type 212 Forms 0 through 8, 100 through 102, and 105 put the positive
/// string count `NS` at index 1 and store twelve tokens per text string, so
/// their groups start at token `2 + 12*NS`.
/// Type 213 Form 0 puts the positive string count `NS` at index 12 and stores
/// twenty tokens per text string, so its groups start at token `13 + 20*NS`.
/// Type 228 Form 0 puts the positive geometry count `N` at index 2 and the
/// nonnegative leader count `L` after the geometry pointers, so its groups
/// start at token `4 + N + L`.
/// Type 126 Forms 0 through 5 define `K`, `M`, and `A = 1 + K + M`, so their
/// groups start at token `18 + 5*K + M`.
/// Type 112 Form 0 puts `N` at index 4 and stores thirteen primary tokens per
/// segment after the first breakpoint, so its groups start at token
/// `18 + 13*N`.
/// Type 130 Form 0 has fourteen fixed primary fields, so its groups start at
/// token 15.
/// Type 150 Form 0 has twelve fixed primary fields, so its groups start at
/// token 13.
/// Type 152 Form 0 has thirteen fixed primary fields, so its groups start at
/// token 14.
/// Type 154 Form 0 has eight fixed primary fields, so its groups start at
/// token 9.
/// Type 156 Form 0 has nine fixed primary fields, so its groups start at
/// token 10.
/// Type 158 Form 0 has four fixed primary fields, so its groups start at
/// token 5.
/// Type 160 Form 0 has eight fixed primary fields, so its groups start at
/// token 9.
/// Type 168 Form 0 has twelve fixed primary fields, so its groups start at
/// token 13.
/// Type 162 Forms 0 and 1 have eight fixed primary fields, so their groups
/// start at token 9.
/// Type 164 Form 0 has five fixed primary fields, so its groups start at
/// token 6.
/// Type 124 Forms 0, 1, 10, 11, and 12 have twelve fixed primary fields, so
/// their groups start at token 13.
/// Type 118 Forms 0 and 1 have four fixed primary fields, so their groups
/// start at token 5.
/// Type 120 Form 0 has four fixed primary fields, so its groups start at token
/// 5.
/// Type 122 Form 0 has four fixed primary fields, so its groups start at token
/// 5.
/// Type 182 Form 0 has four fixed primary fields, so its groups start at token
/// 5.
/// Type 186 Form 0 puts the void-shell count at index 3 and stores one
/// `(VOID, VOF)` pair per void shell, so its groups start at token `4 + 2*N`.
/// Type 180 Forms 0 and 1 put the postorder length `N` at index 1 and store
/// `N` operation-or-operand terms, so their groups start at token `N + 2`.
/// Layouts not represented here use generic CADIR recovery. A malformed known
/// layout returns the record end as a sentinel and never enables generic
/// recovery.
#[derive(Debug, Clone, Copy)]
pub(crate) struct SignalStringLayout {
    pub(crate) signal_name_count: usize,
    pub(crate) connection_count: usize,
    pub(crate) schematic_count: usize,
    pub(crate) physical_count: usize,
    pub(crate) signal_names_start: usize,
    pub(crate) connections_start: usize,
    pub(crate) schematic_start: usize,
    pub(crate) physical_start: usize,
    pub(crate) primary_end: usize,
}

#[derive(Debug, Clone, Copy)]
pub(crate) struct TextNodeLayout {
    pub(crate) geometry_count: usize,
    pub(crate) geometry_start: usize,
    pub(crate) description_start: usize,
    pub(crate) primary_end: usize,
}

#[derive(Debug, Clone, Copy)]
pub(crate) struct ConnectNodeLayout {
    pub(crate) point_count: usize,
    pub(crate) data_count: usize,
    pub(crate) points_start: usize,
    pub(crate) data_start: usize,
    pub(crate) primary_end: usize,
}

fn legacy_count(record: &ParameterRecord, index: usize) -> Option<usize> {
    record
        .integer(index)
        .and_then(|value| usize::try_from(value).ok())
}

pub(crate) fn signal_string_layout(record: &ParameterRecord) -> Option<SignalStringLayout> {
    let signal_name_count = legacy_count(record, 1)?;
    let connection_count = legacy_count(record, 2)?;
    let schematic_count = legacy_count(record, 3)?;
    let physical_count = legacy_count(record, 4)?;
    let signal_names_start: usize = 5;
    let connections_start = signal_names_start.checked_add(signal_name_count)?;
    let schematic_start = connections_start.checked_add(connection_count)?;
    let physical_start = schematic_start.checked_add(schematic_count)?;
    let primary_end = physical_start.checked_add(physical_count)?;
    (primary_end <= record.parameter_end()).then_some(SignalStringLayout {
        signal_name_count,
        connection_count,
        schematic_count,
        physical_count,
        signal_names_start,
        connections_start,
        schematic_start,
        physical_start,
        primary_end,
    })
}

pub(crate) fn text_node_layout(record: &ParameterRecord) -> Option<TextNodeLayout> {
    let geometry_count = legacy_count(record, 1)?;
    let text_description_count = legacy_count(record, 2)?;
    let geometry_start: usize = 3;
    let description_start = geometry_start.checked_add(geometry_count)?;
    let primary_end = description_start.checked_add(7)?;
    (text_description_count == 1 && primary_end <= record.parameter_end()).then_some(
        TextNodeLayout {
            geometry_count,
            geometry_start,
            description_start,
            primary_end,
        },
    )
}

pub(crate) fn connect_node_layout(record: &ParameterRecord) -> Option<ConnectNodeLayout> {
    let point_count = legacy_count(record, 1)?;
    let data_count = legacy_count(record, 2)?;
    let points_start: usize = 3;
    let data_start = points_start.checked_add(point_count)?;
    let primary_end = data_start.checked_add(data_count)?;
    (primary_end <= record.parameter_end()).then_some(ConnectNodeLayout {
        point_count,
        data_count,
        points_start,
        data_start,
        primary_end,
    })
}

fn signal_string_primary_end(record: &ParameterRecord) -> usize {
    signal_string_layout(record).map_or(record.tokens.len(), |layout| layout.primary_end)
}

fn text_node_primary_end(record: &ParameterRecord) -> usize {
    text_node_layout(record).map_or(record.tokens.len(), |layout| layout.primary_end)
}

fn connect_node_primary_end(record: &ParameterRecord) -> usize {
    connect_node_layout(record).map_or(record.tokens.len(), |layout| layout.primary_end)
}

pub(crate) fn entity_primary_end(
    record: &ParameterRecord,
    directory: &BTreeMap<u32, &DirectoryEntry>,
) -> Option<usize> {
    let entry = directory.get(&record.directory_sequence)?;
    match (entry.entity_type, entry.form) {
        (102, 0) | (402, 1 | 7 | 14 | 15) => Some(counted_primary_end(record)),
        (402, 5) => Some(label_display_primary_end(record)),
        (402, 6) => Some(view_list_primary_end(record)),
        (402, 3) => Some(view_visibility_primary_end(record, 1)),
        (402, 4) => Some(view_visibility_primary_end(record, 5)),
        (402, 2 | 12) => Some(external_reference_index_primary_end(record)),
        (402, 8) => Some(signal_string_primary_end(record)),
        (402, 10) => Some(text_node_primary_end(record)),
        (402, 11) => Some(connect_node_primary_end(record)),
        (402, 13) => Some(dimensioned_geometry_primary_end(record)),
        (402, 18) => Some(flow_associativity_primary_end(record, 2)),
        (402, 19) => Some(segmented_visibility_primary_end(record)),
        (402, 20) => Some(flow_associativity_primary_end(record, 1)),
        (406, 1 | 14) => Some(counted_primary_end(record)),
        (406, 2) => Some(region_restriction_primary_end(record)),
        (406, 3) => Some(level_function_primary_end(record)),
        (406, 4) => Some(fixed_primary_end(record, 4)),
        (406, 6) => Some(fixed_primary_end(record, 7)),
        (406, 18..=21) => Some(fixed_primary_end(record, 3)),
        (406, 22) => Some(fixed_primary_end(record, 11)),
        (406, 23) => Some(fixed_primary_end(record, 4)),
        (406, 8) => Some(pin_number_primary_end(record)),
        (406, 9) => Some(part_number_primary_end(record)),
        (406, 10) => Some(hierarchy_primary_end(record)),
        (406, 11) => Some(tabular_data_primary_end(record)),
        (406, 12) => Some(external_reference_file_list_primary_end(record)),
        (406, 13) => Some(nominal_size_primary_end(record)),
        (406, 15) => Some(name_property_primary_end(record)),
        (406, 16 | 17 | 33) => Some(drawing_property_primary_end(record)),
        (406, 24) => Some(level_to_lep_layer_map_primary_end(record)),
        (406, 25) => Some(lep_artwork_stackup_primary_end(record)),
        (406, 26) => Some(lep_drilled_hole_primary_end(record)),
        (406, 28) => Some(dimension_units_primary_end(record)),
        (406, 29) => Some(dimension_tolerance_primary_end(record)),
        (406, 31) => Some(basic_dimension_primary_end(record)),
        (406, 32) => Some(drawing_sheet_approval_primary_end(record)),
        (406, 36) => Some(closure_primary_end(record)),
        (406, 30) => Some(dimension_display_primary_end(record)),
        (406, 34 | 35) => Some(text_score_primary_end(record)),
        (406, 27) => Some(generic_data_primary_end(record)),
        (402, 9) => Some(single_parent_primary_end(record)),
        (230, 0) => Some(sectioned_area_primary_end(record)),
        (228, 0) => Some(general_symbol_primary_end(record)),
        (132, 0) => Some(fixed_primary_end(record, 15)),
        (202, 0) => Some(fixed_primary_end(record, 9)),
        (204, 0) => Some(fixed_primary_end(record, 8)),
        (206, 0) => Some(fixed_primary_end(record, 6)),
        (216, 0..=2) => Some(fixed_primary_end(record, 6)),
        (220, 0) => Some(fixed_primary_end(record, 4)),
        (222, 0) => Some(fixed_primary_end(record, 5)),
        (222, 1) => Some(fixed_primary_end(record, 6)),
        (104, 0..=3) => Some(fixed_primary_end(record, 12)),
        (108, -1..=1) => Some(fixed_primary_end(record, 10)),
        (150, 0) => Some(fixed_primary_end(record, 13)),
        (152, 0) => Some(fixed_primary_end(record, 14)),
        (154, 0) => Some(fixed_primary_end(record, 9)),
        (156, 0) => Some(fixed_primary_end(record, 10)),
        (158, 0) => Some(fixed_primary_end(record, 5)),
        (160, 0) => Some(fixed_primary_end(record, 9)),
        (168, 0) => Some(fixed_primary_end(record, 13)),
        (162, 0 | 1) => Some(fixed_primary_end(record, 9)),
        (164, 0) => Some(fixed_primary_end(record, 6)),
        (124, 0 | 1 | 10 | 11 | 12) => Some(fixed_primary_end(record, 13)),
        (118, 0 | 1) => Some(fixed_primary_end(record, 5)),
        (120, 0) => Some(fixed_primary_end(record, 5)),
        (122, 0) => Some(fixed_primary_end(record, 5)),
        (182, 0) => Some(fixed_primary_end(record, 5)),
        (186, 0) => Some(manifold_solid_primary_end(record)),
        (312, 0..=1) => Some(fixed_primary_end(record, 11)),
        (314, 0) => Some(fixed_primary_end(record, 5)),
        (304, 1) => Some(fixed_primary_end(record, 5)),
        (304, 2) => Some(line_font_pattern_primary_end(record)),
        (310, 0) => Some(text_font_primary_end(record)),
        (320, 0) => Some(network_subfigure_primary_end(record)),
        (184, 0 | 1) => Some(solid_assembly_primary_end(record)),
        (214, 1..=12) => Some(leader_primary_end(record)),
        (218, 0) => Some(fixed_primary_end(record, 3)),
        (218, 1) => Some(fixed_primary_end(record, 4)),
        (412, 0) => Some(rectangular_array_primary_end(record)),
        (414, 0) => Some(circular_array_primary_end(record)),
        (408, 0) => Some(fixed_primary_end(record, 6)),
        (420, 0) => Some(network_instance_primary_end(record)),
        (430, 0 | 1) => Some(fixed_primary_end(record, 2)),
        (410, 0) => Some(fixed_primary_end(record, 9)),
        (410, 1) => Some(fixed_primary_end(record, 23)),
        (416, 0 | 2 | 4) => Some(fixed_primary_end(record, 3)),
        (416, 1 | 3) => Some(fixed_primary_end(record, 2)),
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
        (130, 0) => Some(fixed_primary_end(record, 15)),
        (190, 0) => Some(fixed_primary_end(record, 3)),
        (190, 1) => Some(fixed_primary_end(record, 4)),
        (192, 0) => Some(fixed_primary_end(record, 4)),
        (192, 1) => Some(fixed_primary_end(record, 5)),
        (194, 0) => Some(fixed_primary_end(record, 5)),
        (194, 1) => Some(fixed_primary_end(record, 6)),
        (196, 0) => Some(fixed_primary_end(record, 3)),
        (196, 1) => Some(fixed_primary_end(record, 5)),
        (198, 0) => Some(fixed_primary_end(record, 5)),
        (198, 1) => Some(fixed_primary_end(record, 6)),
        (180, 0 | 1) => Some(boolean_tree_primary_end(record)),
        (141, 0) => Some(boundary_primary_end(record)),
        (142, 0) => Some(fixed_primary_end(record, 6)),
        (100, 0) => Some(fixed_primary_end(record, 8)),
        (208, 0) => Some(flag_note_primary_end(record)),
        (210, 0) => Some(general_label_primary_end(record)),
        (212, 0..=8 | 100..=102 | 105) => Some(general_note_primary_end(record)),
        (213, 0) => Some(new_general_note_primary_end(record)),
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

fn fixed_primary_end(record: &ParameterRecord, end: usize) -> usize {
    if record.tokens.len() >= end {
        end
    } else {
        record.tokens.len()
    }
}

fn line_font_pattern_primary_end(record: &ParameterRecord) -> usize {
    record
        .integer(1)
        .and_then(|value| usize::try_from(value).ok())
        .filter(|count| *count > 0)
        .and_then(|count| count.checked_add(3))
        .filter(|end| *end <= record.tokens.len())
        .unwrap_or(record.tokens.len())
}

fn text_font_primary_end(record: &ParameterRecord) -> usize {
    let Some(character_count) = record
        .integer(5)
        .and_then(|value| usize::try_from(value).ok())
        .filter(|count| *count > 0)
    else {
        return record.tokens.len();
    };

    let mut cursor = 6_usize;
    for _ in 0..character_count {
        let Some(motion_count) = cursor
            .checked_add(3)
            .and_then(|index| record.integer(index))
            .and_then(|value| usize::try_from(value).ok())
        else {
            return record.tokens.len();
        };
        let Some(next) = motion_count
            .checked_mul(3)
            .and_then(|motion_span| cursor.checked_add(4)?.checked_add(motion_span))
            .filter(|end| *end <= record.tokens.len())
        else {
            return record.tokens.len();
        };
        cursor = next;
    }
    cursor
}

fn segmented_visibility_primary_end(record: &ParameterRecord) -> usize {
    record
        .integer(1)
        .and_then(|value| usize::try_from(value).ok())
        .filter(|count| *count > 0)
        .and_then(|count| count.checked_mul(6))
        .and_then(|span| span.checked_add(2))
        .filter(|end| *end <= record.tokens.len())
        .unwrap_or(record.tokens.len())
}

fn boolean_tree_primary_end(record: &ParameterRecord) -> usize {
    record
        .integer(1)
        .and_then(|value| usize::try_from(value).ok())
        .filter(|count| *count > 2)
        .and_then(|count| count.checked_add(2))
        .filter(|end| *end <= record.tokens.len())
        .unwrap_or(record.tokens.len())
}

fn generic_data_primary_end(record: &ParameterRecord) -> usize {
    let Some(value_count) = record
        .integer(3)
        .and_then(|value| usize::try_from(value).ok())
        .filter(|count| *count > 0)
    else {
        return record.tokens.len();
    };
    let Some(pair_span) = value_count.checked_mul(2) else {
        return record.tokens.len();
    };
    let Some(expected_np) = pair_span.checked_add(2) else {
        return record.tokens.len();
    };
    if record
        .integer(1)
        .and_then(|value| usize::try_from(value).ok())
        != Some(expected_np)
    {
        return record.tokens.len();
    }
    pair_span
        .checked_add(4)
        .filter(|end| *end <= record.tokens.len())
        .unwrap_or(record.tokens.len())
}

fn tabular_data_primary_end(record: &ParameterRecord) -> usize {
    let Some(dependent_count) = record
        .integer(3)
        .and_then(|value| usize::try_from(value).ok())
        .filter(|count| *count > 0)
    else {
        return record.tokens.len();
    };
    let Some(independent_count) = record
        .integer(4)
        .and_then(|value| usize::try_from(value).ok())
    else {
        return record.tokens.len();
    };
    let Some(count_start) = 5usize.checked_add(independent_count) else {
        return record.tokens.len();
    };
    let Some(value_start) = count_start.checked_add(independent_count) else {
        return record.tokens.len();
    };
    if value_start > record.tokens.len() {
        return record.tokens.len();
    }

    let mut independent_value_count = 0_usize;
    let mut point_count = 1_usize;
    for offset in 0..independent_count {
        let Some(count_index) = count_start.checked_add(offset) else {
            return record.tokens.len();
        };
        let Some(value_count) = record
            .integer(count_index)
            .and_then(|value| usize::try_from(value).ok())
            .filter(|count| *count > 0)
        else {
            return record.tokens.len();
        };
        let Some(next_count) = independent_value_count.checked_add(value_count) else {
            return record.tokens.len();
        };
        independent_value_count = next_count;
        let Some(next_point_count) = point_count.checked_mul(value_count) else {
            return record.tokens.len();
        };
        point_count = next_point_count;
    }
    let Some(dependent_value_start) = value_start.checked_add(independent_value_count) else {
        return record.tokens.len();
    };
    let Some(dependent_value_count) = dependent_count.checked_mul(point_count) else {
        return record.tokens.len();
    };
    let Some(end) = dependent_value_start
        .checked_add(dependent_value_count)
        .filter(|end| *end <= record.tokens.len())
    else {
        return record.tokens.len();
    };
    if record.integer(1) != i64::try_from(end.saturating_sub(2)).ok() {
        return record.tokens.len();
    }
    end
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

fn view_visibility_primary_end(record: &ParameterRecord, block_width: usize) -> usize {
    let view_count = record
        .integer(1)
        .and_then(|value| usize::try_from(value).ok())
        .filter(|count| *count > 0);
    let entity_count = record
        .integer(2)
        .and_then(|value| usize::try_from(value).ok());
    view_count
        .zip(entity_count)
        .and_then(|(view_count, entity_count)| {
            view_count
                .checked_mul(block_width)?
                .checked_add(3)?
                .checked_add(entity_count)
        })
        .filter(|end| *end <= record.tokens.len())
        .unwrap_or(record.tokens.len())
}

fn external_reference_index_primary_end(record: &ParameterRecord) -> usize {
    record
        .integer(1)
        .and_then(|value| usize::try_from(value).ok())
        .filter(|count| *count > 0)
        .and_then(|count| count.checked_mul(2))
        .and_then(|span| span.checked_add(2))
        .filter(|end| *end <= record.tokens.len())
        .unwrap_or(record.tokens.len())
}

fn region_restriction_primary_end(record: &ParameterRecord) -> usize {
    if record.integer(1) == Some(3) && record.tokens.len() >= 5 {
        5
    } else {
        record.tokens.len()
    }
}

fn level_function_primary_end(record: &ParameterRecord) -> usize {
    if record.integer(1) == Some(2) && record.tokens.len() >= 4 {
        4
    } else {
        record.tokens.len()
    }
}

fn pin_number_primary_end(record: &ParameterRecord) -> usize {
    if record.integer(1) == Some(1) && record.tokens.len() >= 3 {
        3
    } else {
        record.tokens.len()
    }
}

fn part_number_primary_end(record: &ParameterRecord) -> usize {
    if record.integer(1) == Some(4) && record.tokens.len() >= 6 {
        6
    } else {
        record.tokens.len()
    }
}

fn hierarchy_primary_end(record: &ParameterRecord) -> usize {
    if record.integer(1) == Some(6) && record.tokens.len() >= 8 {
        8
    } else {
        record.tokens.len()
    }
}

fn nominal_size_primary_end(record: &ParameterRecord) -> usize {
    match record.integer(1) {
        Some(2) if record.tokens.len() >= 4 => 4,
        Some(3) if record.tokens.len() >= 5 => 5,
        _ => record.tokens.len(),
    }
}

fn name_property_primary_end(record: &ParameterRecord) -> usize {
    if record.integer(1) == Some(1) && record.tokens.len() >= 3 {
        3
    } else {
        record.tokens.len()
    }
}

fn drawing_property_primary_end(record: &ParameterRecord) -> usize {
    if record.integer(1) == Some(2) && record.tokens.len() >= 4 {
        4
    } else {
        record.tokens.len()
    }
}

fn level_to_lep_layer_map_primary_end(record: &ParameterRecord) -> usize {
    let Some(definition_count) = record
        .integer(2)
        .and_then(|value| usize::try_from(value).ok())
        .filter(|count| *count > 0)
    else {
        return record.tokens.len();
    };
    let Some(end) = definition_count
        .checked_mul(4)
        .and_then(|span| span.checked_add(3))
        .filter(|end| *end <= record.tokens.len())
    else {
        return record.tokens.len();
    };
    if record.integer(1) != i64::try_from(end.saturating_sub(2)).ok() {
        return record.tokens.len();
    }
    end
}

fn lep_artwork_stackup_primary_end(record: &ParameterRecord) -> usize {
    let Some(level_count) = record
        .integer(3)
        .and_then(|value| usize::try_from(value).ok())
        .filter(|count| *count > 0)
    else {
        return record.tokens.len();
    };
    let Some(end) = level_count
        .checked_add(4)
        .filter(|end| *end <= record.tokens.len())
    else {
        return record.tokens.len();
    };
    if record.integer(1) != i64::try_from(end.saturating_sub(2)).ok() {
        return record.tokens.len();
    }
    end
}

fn lep_drilled_hole_primary_end(record: &ParameterRecord) -> usize {
    if record.integer(1) == Some(3) && record.tokens.len() >= 5 {
        5
    } else {
        record.tokens.len()
    }
}

fn dimension_units_primary_end(record: &ParameterRecord) -> usize {
    if record.integer(1) == Some(6) && record.tokens.len() >= 8 {
        8
    } else {
        record.tokens.len()
    }
}

fn dimension_tolerance_primary_end(record: &ParameterRecord) -> usize {
    if record.integer(1) == Some(8) && record.tokens.len() >= 10 {
        10
    } else {
        record.tokens.len()
    }
}

fn basic_dimension_primary_end(record: &ParameterRecord) -> usize {
    if record.integer(1) == Some(8) && record.tokens.len() >= 10 {
        10
    } else {
        record.tokens.len()
    }
}

fn drawing_sheet_approval_primary_end(record: &ParameterRecord) -> usize {
    if record.integer(1) == Some(3) && record.tokens.len() >= 5 {
        5
    } else {
        record.tokens.len()
    }
}

fn closure_primary_end(record: &ParameterRecord) -> usize {
    record
        .integer(1)
        .and_then(|value| matches!(value, 1 | 2).then_some(value as usize))
        .and_then(|count| count.checked_add(2))
        .filter(|end| *end <= record.tokens.len())
        .unwrap_or(record.tokens.len())
}

fn external_reference_file_list_primary_end(record: &ParameterRecord) -> usize {
    record
        .integer(1)
        .and_then(|value| usize::try_from(value).ok())
        .filter(|count| *count > 0)
        .and_then(|count| count.checked_add(2))
        .filter(|end| *end <= record.tokens.len())
        .unwrap_or(record.tokens.len())
}

fn dimensioned_geometry_primary_end(record: &ParameterRecord) -> usize {
    if record.integer(1) != Some(1) {
        return record.tokens.len();
    }
    record
        .integer(2)
        .and_then(|value| usize::try_from(value).ok())
        .filter(|count| *count > 0)
        .and_then(|count| count.checked_add(4))
        .filter(|end| *end <= record.tokens.len())
        .unwrap_or(record.tokens.len())
}

fn flow_associativity_primary_end(record: &ParameterRecord, context_count: i64) -> usize {
    if record.integer(1) != Some(context_count) {
        return record.tokens.len();
    }
    let list_start: usize = if context_count == 2 { 10 } else { 9 };
    let list_tokens = (2..=7).try_fold(0_usize, |total, index| {
        let count = record
            .integer(index)
            .and_then(|value| usize::try_from(value).ok())?;
        total.checked_add(count)
    });
    list_tokens
        .and_then(|count| list_start.checked_add(count))
        .filter(|end| *end <= record.tokens.len())
        .unwrap_or(record.tokens.len())
}

fn text_score_primary_end(record: &ParameterRecord) -> usize {
    let Some(range_count) = record
        .integer(2)
        .and_then(|value| usize::try_from(value).ok())
        .filter(|count| *count > 0)
    else {
        return record.tokens.len();
    };
    let Some(expected_property_count) = range_count
        .checked_mul(3)
        .and_then(|span| span.checked_add(1))
    else {
        return record.tokens.len();
    };
    if record.integer(1) != i64::try_from(expected_property_count).ok() {
        return record.tokens.len();
    }
    range_count
        .checked_mul(3)
        .and_then(|span| span.checked_add(3))
        .filter(|end| *end <= record.tokens.len())
        .unwrap_or(record.tokens.len())
}

fn dimension_display_primary_end(record: &ParameterRecord) -> usize {
    if record.integer(1) != Some(14) {
        return record.tokens.len();
    }
    let Some(note_count) = record
        .integer(13)
        .and_then(|value| usize::try_from(value).ok())
    else {
        return record.tokens.len();
    };
    note_count
        .checked_mul(3)
        .and_then(|span| span.checked_add(14))
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

fn general_symbol_primary_end(record: &ParameterRecord) -> usize {
    let Some(geometry_count) = record
        .integer(2)
        .and_then(|value| usize::try_from(value).ok())
        .filter(|count| *count > 0)
    else {
        return record.tokens.len();
    };
    let Some(leader_count_index) = geometry_count.checked_add(3) else {
        return record.tokens.len();
    };
    let Some(leader_count) = record
        .integer(leader_count_index)
        .and_then(|value| usize::try_from(value).ok())
    else {
        return record.tokens.len();
    };
    leader_count_index
        .checked_add(1)
        .and_then(|start| start.checked_add(leader_count))
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

fn network_instance_primary_end(record: &ParameterRecord) -> usize {
    record
        .integer(11)
        .and_then(|value| usize::try_from(value).ok())
        .and_then(|count| count.checked_add(12))
        .filter(|end| *end <= record.tokens.len())
        .unwrap_or(record.tokens.len())
}

fn leader_primary_end(record: &ParameterRecord) -> usize {
    let Some(segment_count) = record
        .integer(1)
        .and_then(|value| usize::try_from(value).ok())
        .filter(|count| *count > 0)
    else {
        return record.tokens.len();
    };
    segment_count
        .checked_mul(2)
        .and_then(|span| span.checked_add(7))
        .filter(|end| *end <= record.tokens.len())
        .unwrap_or(record.tokens.len())
}

fn flag_note_primary_end(record: &ParameterRecord) -> usize {
    record
        .integer(6)
        .and_then(|value| usize::try_from(value).ok())
        .and_then(|count| count.checked_add(7))
        .filter(|end| *end <= record.tokens.len())
        .unwrap_or(record.tokens.len())
}

fn general_label_primary_end(record: &ParameterRecord) -> usize {
    record
        .integer(2)
        .and_then(|value| usize::try_from(value).ok())
        .filter(|count| *count > 0)
        .and_then(|count| count.checked_add(3))
        .filter(|end| *end <= record.tokens.len())
        .unwrap_or(record.tokens.len())
}

fn general_note_primary_end(record: &ParameterRecord) -> usize {
    record
        .integer(1)
        .and_then(|value| usize::try_from(value).ok())
        .filter(|count| *count > 0)
        .and_then(|count| count.checked_mul(12))
        .and_then(|span| span.checked_add(2))
        .filter(|end| *end <= record.tokens.len())
        .unwrap_or(record.tokens.len())
}

fn new_general_note_primary_end(record: &ParameterRecord) -> usize {
    record
        .integer(12)
        .and_then(|value| usize::try_from(value).ok())
        .filter(|count| *count > 0)
        .and_then(|count| count.checked_mul(20))
        .and_then(|span| span.checked_add(13))
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

fn manifold_solid_primary_end(record: &ParameterRecord) -> usize {
    let Some(void_count) = record.count_with_stride(3, 2) else {
        return record.tokens.len();
    };
    void_count
        .checked_mul(2)
        .and_then(|span| span.checked_add(4))
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
    let associations: Vec<u32> = association_pointers
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
    let properties: Vec<u32> = property_pointers
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
    let fully_valid = associations.len() == association_pointers.len()
        && properties.len() == property_pointers.len();
    Some(TrailingPointerGroups {
        token_start: candidate.token_start,
        associations,
        properties,
        association_pointers,
        property_pointers,
        fully_valid,
    })
}

/// Why one entity's Parameter Data has no typed tokens.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ParameterDefect {
    HollerithCountUnreadable,
    HollerithPayloadTruncated,
    TokenNotAscii,
    TokenNotANumber,
    DelimiterMissing,
    EntityTypeTokenMismatch,
    DeclaredCardMissing,
    NoOwnedCards,
    DeclaredCountZero,
    OwnershipConflict,
}

impl ParameterDefect {
    pub(crate) fn key(self) -> &'static str {
        match self {
            Self::HollerithCountUnreadable => "hollerith-count-unreadable",
            Self::HollerithPayloadTruncated => "hollerith-payload-truncated",
            Self::TokenNotAscii => "token-not-ascii",
            Self::TokenNotANumber => "token-not-a-number",
            Self::DelimiterMissing => "delimiter-missing",
            Self::EntityTypeTokenMismatch => "entity-type-token-mismatch",
            Self::DeclaredCardMissing => "declared-card-missing",
            Self::NoOwnedCards => "no-owned-cards",
            Self::DeclaredCountZero => "declared-count-zero",
            Self::OwnershipConflict => "ownership-conflict",
        }
    }

    fn describe(self) -> &'static str {
        match self {
            Self::HollerithCountUnreadable => "a Hollerith byte count is unreadable",
            Self::HollerithPayloadTruncated => "a Hollerith payload is truncated",
            Self::TokenNotAscii => "a token is not ASCII",
            Self::TokenNotANumber => "a token is not a number",
            Self::DelimiterMissing => "a delimiter is missing",
            Self::EntityTypeTokenMismatch => {
                "the first token disagrees with the Directory Entry entity type"
            }
            Self::DeclaredCardMissing => "a declared Parameter Data card does not exist",
            Self::NoOwnedCards => "no Parameter Data card is owned",
            Self::DeclaredCountZero => "the Directory Entry declares zero Parameter Data cards",
            Self::OwnershipConflict => "Parameter Data card ownership conflicts",
        }
    }
}

/// One entity's Parameter Data whose tokens were not recovered.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct QuarantinedParameterRecord {
    pub(crate) sequence: u32,
    pub(crate) source_offset: u64,
    pub(crate) cards: usize,
    pub(crate) bytes: Vec<u8>,
    line_range: Option<Range<u32>>,
    provenance_offset: u64,
    pub(crate) defect: ParameterDefect,
}

impl QuarantinedParameterRecord {
    /// The stable native identity of this quarantined record.
    pub(crate) fn identity(&self) -> String {
        format!("iges:quarantine:parameter#{}", self.sequence)
    }

    pub(crate) fn loss_note(&self) -> LossNote {
        let owned = match &self.line_range {
            Some(range) => format!("P{} through P{}", range.start, range.end.saturating_sub(1)),
            None => "no owned Parameter Data card".to_owned(),
        };
        IgesLossCode::ParameterDataQuarantined
            .note(format!(
                "IGES Parameter Data of D{} ({owned}) is quarantined because {}; its {} raw card(s) are retained and no token was interpreted",
                self.sequence,
                self.defect.describe(),
                self.cards
            ))
            .with_provenance(SourceProvenance {
                format: "iges".into(),
                stream: "iges".into(),
                offset: self.provenance_offset,
                tag: Some(format!("D{}:parameter", self.sequence)),
            })
    }
}

/// A tokenizer stop: a source defect, or a resource refusal that is not one.
enum TokenizeFailure {
    Defect(ParameterDefect, usize),
    Refusal(CodecError),
}

/// Both parse results of the Parameter Data section.
pub(crate) struct ParameterAssembly {
    pub(crate) records: Vec<ParameterRecord>,
    pub(crate) trailing_pointer_analysis: BTreeMap<u32, TrailingPointerAnalysis>,
    pub(crate) quarantined: Vec<QuarantinedParameterRecord>,
    pub(crate) recoveries: FramingRecoveries,
}

fn back_pointer(line: &PhysicalLine) -> Option<u32> {
    let text = std::str::from_utf8(line.payload.get(64..72)?).ok()?.trim();
    text.parse::<u32>().ok()
}

fn hollerith(bytes: &[u8], start: usize) -> Result<Option<(Token, usize)>, TokenizeFailure> {
    let mut cursor = start;
    while bytes.get(cursor).is_some_and(u8::is_ascii_digit) {
        cursor += 1;
    }
    if cursor == start || !matches!(bytes.get(cursor), Some(b'H' | b'h')) {
        return Ok(None);
    }
    let unreadable_count =
        || TokenizeFailure::Defect(ParameterDefect::HollerithCountUnreadable, start);
    let count = std::str::from_utf8(&bytes[start..cursor])
        .map_err(|_| unreadable_count())?
        .parse::<usize>()
        .map_err(|_| unreadable_count())?;
    let end = cursor
        .checked_add(1)
        .and_then(|payload_start| payload_start.checked_add(count))
        .ok_or_else(unreadable_count)?;
    let payload = bytes.get(cursor + 1..end).ok_or(TokenizeFailure::Defect(
        ParameterDefect::HollerithPayloadTruncated,
        start,
    ))?;
    Ok(Some((
        Token {
            value: TokenValue::String(payload.to_vec()),
            span: start..end,
        },
        end,
    )))
}

fn numeric(bytes: &[u8], span: Range<usize>) -> Result<Token, TokenizeFailure> {
    let start = span.start;
    let text = std::str::from_utf8(&bytes[span.clone()])
        .map_err(|_| TokenizeFailure::Defect(ParameterDefect::TokenNotAscii, start))?
        .trim();
    if text.is_empty() {
        return Ok(Token {
            value: TokenValue::Omitted,
            span,
        });
    }
    let not_a_number = || TokenizeFailure::Defect(ParameterDefect::TokenNotANumber, start);
    let real = text
        .bytes()
        .any(|byte| matches!(byte, b'.' | b'E' | b'e' | b'D' | b'd'));
    let value = if real {
        let normalized = text.replace(['D', 'd'], "E");
        TokenValue::Real(normalized.parse::<f64>().map_err(|_| not_a_number())?)
    } else {
        TokenValue::Integer(text.parse::<i64>().map_err(|_| not_a_number())?)
    };
    Ok(Token { value, span })
}

fn tokenize(
    bytes: &[u8],
    parameter_delimiter: u8,
    record_delimiter: u8,
    ctx: Option<&DecodeContext<'_>>,
) -> Result<(Vec<Token>, usize), TokenizeFailure> {
    let charge = |ctx| charge_token(ctx).map_err(TokenizeFailure::Refusal);
    let mut tokens = Vec::new();
    let mut cursor = 0_usize;
    loop {
        if bytes.get(cursor) == Some(&record_delimiter) {
            return Ok((tokens, cursor + 1));
        }
        if bytes.get(cursor) == Some(&parameter_delimiter) {
            charge(ctx)?;
            tokens.push(Token {
                value: TokenValue::Omitted,
                span: cursor..cursor,
            });
            cursor += 1;
            continue;
        }
        let (token, end) = if let Some(value) = hollerith(bytes, cursor)? {
            value
        } else {
            let end = bytes[cursor..]
                .iter()
                .position(|byte| {
                    matches!(*byte, value if value == parameter_delimiter || value == record_delimiter)
                })
                .and_then(|relative| cursor.checked_add(relative))
                .ok_or(TokenizeFailure::Defect(
                    ParameterDefect::DelimiterMissing,
                    cursor,
                ))?;
            if end == cursor {
                return Err(TokenizeFailure::Defect(
                    ParameterDefect::DelimiterMissing,
                    cursor,
                ));
            }
            (numeric(bytes, cursor..end)?, end)
        };
        charge(ctx)?;
        tokens.push(token);
        match bytes.get(end).copied() {
            Some(value) if value == parameter_delimiter => cursor = end + 1,
            Some(value) if value == record_delimiter => return Ok((tokens, end + 1)),
            _ => {
                return Err(TokenizeFailure::Defect(
                    ParameterDefect::DelimiterMissing,
                    end,
                ))
            }
        }
    }
}

/// The Directory-declared Parameter Data range, when the declaration is usable.
enum DeclaredRange {
    Usable(Range<u32>),
    CardMissing,
    Unusable,
}

/// Positional sequences make the Parameter Data census contiguous, so a
/// declared range names existing cards exactly when it lies inside `census`.
fn declared_range(entry: &DirectoryEntry, census: &Range<u32>) -> DeclaredRange {
    let start = u32::try_from(entry.parameter_start)
        .ok()
        .filter(|value| *value > 0);
    let count = u32::try_from(entry.parameter_line_count)
        .ok()
        .filter(|value| *value > 0);
    let (Some(start), Some(count)) = (start, count) else {
        return DeclaredRange::Unusable;
    };
    let cards = usize::try_from(census.end.saturating_sub(census.start)).unwrap_or(usize::MAX);
    if bounded_len(u64::from(count), 1, cards).is_none() {
        return DeclaredRange::CardMissing;
    }
    let Some(end) = start.checked_add(count) else {
        return DeclaredRange::CardMissing;
    };
    if start < census.start || end > census.end {
        return DeclaredRange::CardMissing;
    }
    DeclaredRange::Usable(start..end)
}

/// The contiguous head of the run of cards whose back-pointer names one entry.
fn contiguous_run(cards: &[u32]) -> Vec<u32> {
    let mut run = Vec::<u32>::new();
    for sequence in cards {
        if run
            .last()
            .is_some_and(|last| last.checked_add(1) != Some(*sequence))
        {
            break;
        }
        run.push(*sequence);
    }
    run
}

/// The Directory Entries whose declared ranges claim a card in common.
///
/// A start-ordered sweep over the ranges marks every participant: an overlap
/// marks the later range and the range holding the highest end so far.
fn overlapping_ranges(declared: &BTreeMap<u32, Range<u32>>) -> BTreeSet<u32> {
    let mut ordered = declared
        .iter()
        .map(|(sequence, range)| (range.start, range.end, *sequence))
        .collect::<Vec<_>>();
    ordered.sort_unstable();
    let mut overlapping = BTreeSet::new();
    let mut highest_end = 0_u32;
    let mut highest_owner = None;
    for (start, end, sequence) in ordered {
        if start < highest_end {
            overlapping.insert(sequence);
            overlapping.extend(highest_owner);
        }
        if end >= highest_end {
            highest_end = end;
            highest_owner = Some(sequence);
        }
    }
    overlapping
}

fn owned_bytes(cards: &[u32], lines: &BTreeMap<u32, &PhysicalLine>) -> Vec<u8> {
    cards
        .iter()
        .filter_map(|sequence| lines.get(sequence))
        .flat_map(|line| line.payload.get(..64).unwrap_or_default().iter().copied())
        .collect()
}

/// The source offset of `offset` inside the assembled 64-column card stream.
fn stream_offset(
    offset: usize,
    cards: &[u32],
    lines: &BTreeMap<u32, &PhysicalLine>,
) -> Option<u64> {
    let line = lines.get(cards.get(offset / 64)?)?;
    line.offset.checked_add((offset % 64) as u64)
}

fn quarantine(
    entry: &DirectoryEntry,
    cards: &[u32],
    lines: &BTreeMap<u32, &PhysicalLine>,
    defect: ParameterDefect,
    failing_offset: Option<usize>,
) -> QuarantinedParameterRecord {
    let first = cards
        .first()
        .and_then(|sequence| lines.get(sequence))
        .map_or(entry.source_offset, |line| line.offset);
    QuarantinedParameterRecord {
        sequence: entry.sequence,
        source_offset: first,
        cards: cards.len(),
        bytes: cards
            .iter()
            .filter_map(|sequence| lines.get(sequence))
            .flat_map(|line| line.payload.iter().copied())
            .collect(),
        line_range: cards.first().zip(cards.last()).map(|(first, last)| {
            let end = last.saturating_add(1);
            *first..end
        }),
        provenance_offset: failing_offset
            .and_then(|offset| stream_offset(offset, cards, lines))
            .unwrap_or(first),
        defect,
    }
}

/// One entity's resolved Parameter Data ownership.
struct Ownership<'a> {
    entry: &'a DirectoryEntry,
    cards: Vec<u32>,
    quarantine: Option<ParameterDefect>,
}

/// Resolve which Parameter Data cards each Directory Entry owns.
///
/// The declared range applies first, then the back-pointer census, and a
/// conflict between the two statements quarantines both entities.
fn resolve_ownership<'a>(
    directory: &'a [DirectoryEntry],
    lines: &BTreeMap<u32, &PhysicalLine>,
    back_pointers: &BTreeMap<u32, Option<u32>>,
    recoveries: &mut FramingRecoveries,
    ctx: Option<&DecodeContext<'_>>,
) -> Result<Vec<Ownership<'a>>, CodecError> {
    let typed = directory
        .iter()
        .map(|entry| entry.sequence)
        .collect::<BTreeSet<_>>();
    let candidates = directory
        .iter()
        .filter(|entry| !(entry.entity_type == 0 && entry.parameter_line_count == 0))
        .collect::<Vec<_>>();
    let census = lines
        .keys()
        .next()
        .copied()
        .zip(lines.keys().next_back().copied())
        .map_or(0..0, |(first, last)| first..last.saturating_add(1));
    let mut named_by = BTreeMap::<u32, Vec<u32>>::new();
    for (sequence, pointer) in back_pointers {
        if let Some(owner) = pointer {
            named_by.entry(*owner).or_default().push(*sequence);
        }
    }
    let mut declared = BTreeMap::<u32, Range<u32>>::new();
    let mut card_missing = BTreeSet::<u32>::new();
    for entry in &candidates {
        match declared_range(entry, &census) {
            DeclaredRange::Usable(range) => {
                declared.insert(entry.sequence, range);
            }
            DeclaredRange::CardMissing => {
                card_missing.insert(entry.sequence);
            }
            DeclaredRange::Unusable => {}
        }
    }
    let mut conflicted = overlapping_ranges(&declared);
    let claimed = declared
        .iter()
        .filter(|(sequence, _)| !conflicted.contains(sequence))
        .flat_map(|(sequence, range)| range.clone().map(|card| (card, *sequence)))
        .collect::<BTreeMap<_, _>>();
    for (card, owner) in &claimed {
        match back_pointers.get(card).copied().flatten() {
            Some(pointer) if pointer == *owner => {}
            Some(pointer) if pointer % 2 == 1 && typed.contains(&pointer) => {
                conflicted.insert(*owner);
                conflicted.insert(pointer);
            }
            _ => {}
        }
    }
    for (card, owner) in &claimed {
        if conflicted.contains(owner) {
            continue;
        }
        let pointer = back_pointers.get(card).copied().flatten();
        if pointer != Some(*owner) {
            recoveries.record(
                Section::Parameter,
                FramingDefect::ParameterOwner,
                *card as usize,
                lines.get(card).map_or(0, |line| line.offset),
                pointer.map_or_else(
                    || "no readable back-pointer".to_owned(),
                    |value| format!("back-pointer {value}"),
                ),
                format!("the declared range of D{owner}"),
            );
        }
    }
    let mut resolved = Vec::new();
    for entry in candidates {
        let range = declared.get(&entry.sequence).cloned();
        if let Some(range) = &range {
            charge_owned_cards(ctx, u64::from(range.end.saturating_sub(range.start)))?;
        }
        let run = || contiguous_run(named_by.get(&entry.sequence).map_or(&[][..], Vec::as_slice));
        if conflicted.contains(&entry.sequence) {
            resolved.push(Ownership {
                entry,
                cards: range.map_or_else(run, Iterator::collect),
                quarantine: Some(ParameterDefect::OwnershipConflict),
            });
            continue;
        }
        if let Some(range) = range {
            resolved.push(Ownership {
                entry,
                cards: range.collect(),
                quarantine: None,
            });
            continue;
        }
        let run = run();
        if let Some(first) = run.first().copied() {
            recoveries.record(
                Section::Parameter,
                FramingDefect::ParameterOwner,
                first as usize,
                lines.get(&first).map_or(0, |line| line.offset),
                format!(
                    "an unusable declared range for D{} (start {}, count {})",
                    entry.sequence, entry.parameter_start, entry.parameter_line_count
                ),
                format!("the back-pointer census run of {} card(s)", run.len()),
            );
            resolved.push(Ownership {
                entry,
                cards: run,
                quarantine: None,
            });
            continue;
        }
        let defect = if entry.parameter_line_count == 0 {
            ParameterDefect::DeclaredCountZero
        } else if card_missing.contains(&entry.sequence) {
            ParameterDefect::DeclaredCardMissing
        } else {
            ParameterDefect::NoOwnedCards
        };
        resolved.push(Ownership {
            entry,
            cards: Vec::new(),
            quarantine: Some(defect),
        });
    }
    Ok(resolved)
}

fn charge_owned_cards(ctx: Option<&DecodeContext<'_>>, count: u64) -> Result<(), CodecError> {
    ctx.map_or(Ok(()), |ctx| {
        ctx.charge_collection_items(count, "iges_parameter_ownership")
    })
}

pub(crate) fn assemble_with_context(
    scan: &CardScan,
    directory: &[DirectoryEntry],
    quarantined_directory: &[QuarantinedDirectoryRecord],
    global: &ResolvedGlobal,
    ctx: Option<&DecodeContext<'_>>,
) -> Result<ParameterAssembly, CodecError> {
    let lines = scan
        .lines
        .iter()
        .filter(|line| line.section == Some(Section::Parameter))
        .map(|line| (line.sequence.unwrap_or_default(), line))
        .collect::<BTreeMap<_, _>>();
    let back_pointers = lines
        .iter()
        .map(|(sequence, line)| (*sequence, back_pointer(line)))
        .collect::<BTreeMap<_, _>>();
    let entries = directory
        .iter()
        .filter(|entry| !(entry.entity_type == 0 && entry.parameter_line_count == 0))
        .map(|entry| (entry.sequence, entry))
        .collect::<BTreeMap<_, _>>();
    let mut recoveries = FramingRecoveries::default();
    let ownership = resolve_ownership(directory, &lines, &back_pointers, &mut recoveries, ctx)?;
    let mut records = Vec::new();
    let mut trailing_pointer_analysis = BTreeMap::new();
    let mut quarantined = Vec::new();
    for owned in &ownership {
        let entry = owned.entry;
        if let Some(defect) = owned.quarantine {
            quarantined.push(quarantine(entry, &owned.cards, &lines, defect, None));
            continue;
        }
        let bytes = owned_bytes(&owned.cards, &lines);
        let (tokens, record_end) = match tokenize(
            &bytes,
            global.parameter_delimiter,
            global.record_delimiter,
            ctx,
        ) {
            Ok(value) => value,
            Err(TokenizeFailure::Refusal(error)) => return Err(error),
            Err(TokenizeFailure::Defect(defect, offset)) => {
                quarantined.push(quarantine(
                    entry,
                    &owned.cards,
                    &lines,
                    defect,
                    Some(offset),
                ));
                continue;
            }
        };
        if !matches!(tokens.first().map(|token| &token.value), Some(TokenValue::Integer(value)) if *value == entry.entity_type)
        {
            quarantined.push(quarantine(
                entry,
                &owned.cards,
                &lines,
                ParameterDefect::EntityTypeTokenMismatch,
                tokens.first().map(|token| token.span.start),
            ));
            continue;
        }
        let line_start = owned.cards.first().copied().unwrap_or_default();
        let line_end = owned
            .cards
            .last()
            .map_or(line_start, |last| last.saturating_add(1));
        let parameter_end = tokens.len();
        let mut record = ParameterRecord {
            directory_sequence: entry.sequence,
            line_range: line_start..line_end,
            comment: bytes.get(record_end..).unwrap_or_default().to_vec(),
            bytes,
            tokens,
            parameter_end,
        };
        let analysis = analyze_trailing_pointer_groups(&record, &entries);
        record.parameter_end = analysis
            .groups
            .as_ref()
            .filter(|groups| groups.fully_valid)
            .map_or(record.tokens.len(), |groups| groups.token_start);
        trailing_pointer_analysis.insert(entry.sequence, analysis);
        records.push(record);
    }
    let accounted = ownership
        .iter()
        .flat_map(|owned| owned.cards.iter().copied())
        .collect::<BTreeSet<_>>();
    let quarantined_sequences = quarantined_directory
        .iter()
        .map(|record| record.sequence)
        .collect::<BTreeSet<_>>();
    for (sequence, line) in &lines {
        let pointer = back_pointers.get(sequence).copied().flatten();
        if accounted.contains(sequence)
            || pointer.is_some_and(|value| quarantined_sequences.contains(&value))
        {
            continue;
        }
        recoveries.record(
            Section::Parameter,
            FramingDefect::UnclaimedParameterCard,
            *sequence as usize,
            line.offset,
            pointer.map_or_else(
                || "no readable back-pointer".to_owned(),
                |value| format!("back-pointer {value}"),
            ),
            "no owning Directory Entry",
        );
    }
    Ok(ParameterAssembly {
        records,
        trailing_pointer_analysis,
        quarantined,
        recoveries,
    })
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
mod counted_list_tests;
#[cfg(test)]
mod quarantine_tests;
#[cfg(test)]
mod tests;
