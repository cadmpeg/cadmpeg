// SPDX-License-Identifier: Apache-2.0
//! Token-space cursor and subtype walkers over framed [`Token`] payloads.
//!
//! These walkers mirror the byte readers in [`crate::nurbs::reader`] and the
//! subtype walkers in [`crate::nurbs::subtypes`]. The framer resolves integer
//! width and retains payload identifiers, so the walkers use token names and
//! positions. Token positions identify fields within a record payload without
//! depending on serialized byte offsets.

use crate::sab::Token;

/// A cursor over one record's payload tokens.
///
/// `take_*` methods consume one token or counted group and return its value.
/// A failed type match leaves the cursor position unchanged.
#[derive(Clone, Copy)]
pub struct Cur<'a> {
    toks: &'a [Token],
    pos: usize,
}

impl<'a> Cur<'a> {
    /// A cursor over `toks` starting at token index `pos`.
    pub fn at(toks: &'a [Token], pos: usize) -> Self {
        Self { toks, pos }
    }

    /// Current token index.
    pub fn pos(&self) -> usize {
        self.pos
    }

    /// Move the cursor to token index `pos`.
    pub(crate) fn set_pos(&mut self, pos: usize) {
        self.pos = pos;
    }

    /// The full token slice the cursor walks.
    pub(crate) fn toks(&self) -> &'a [Token] {
        self.toks
    }

    /// The token at the cursor, without consuming it.
    pub(crate) fn peek(&self) -> Option<&'a Token> {
        self.toks.get(self.pos)
    }

    /// Whether the cursor is at the closing token of this complete subtype
    /// span, with no unconsumed field before or token after it.
    pub(crate) fn at_scope_end(&self) -> bool {
        self.pos + 1 == self.toks.len()
            && matches!(self.toks.get(self.pos), Some(Token::SubtypeClose))
    }

    /// Consume one token of any kind.
    pub(crate) fn bump(&mut self) -> Option<&'a Token> {
        let token = self.toks.get(self.pos)?;
        self.pos += 1;
        Some(token)
    }

    /// The remaining tokens from the cursor onward.
    pub(crate) fn rest(&self) -> &'a [Token] {
        self.toks.get(self.pos..).unwrap_or(&[])
    }

    pub(crate) fn take_f64(&mut self) -> Option<f64> {
        match self.peek()? {
            Token::Double(value) => {
                self.pos += 1;
                Some(*value)
            }
            _ => None,
        }
    }

    pub(crate) fn take_long(&mut self) -> Option<i64> {
        match self.peek()? {
            Token::Long(value) => {
                self.pos += 1;
                Some(*value)
            }
            _ => None,
        }
    }

    pub(crate) fn take_enum(&mut self) -> Option<i64> {
        match self.peek()? {
            Token::Enum(value) => {
                self.pos += 1;
                Some(*value)
            }
            _ => None,
        }
    }

    pub(crate) fn take_bool(&mut self) -> Option<bool> {
        match self.peek()? {
            Token::True => {
                self.pos += 1;
                Some(true)
            }
            Token::False => {
                self.pos += 1;
                Some(false)
            }
            _ => None,
        }
    }

    pub(crate) fn take_str(&mut self) -> Option<&'a str> {
        match self.peek()? {
            Token::Str(value) => {
                self.pos += 1;
                Some(value)
            }
            _ => None,
        }
    }

    /// Consume one payload identifier (`Ident` or `SubIdent`).
    pub(crate) fn take_ident(&mut self) -> Option<&'a str> {
        match self.peek()? {
            Token::Ident(value) | Token::SubIdent(value) => {
                self.pos += 1;
                Some(value)
            }
            _ => None,
        }
    }

    /// Consume one `0x13` position triple.
    pub(crate) fn take_position(&mut self) -> Option<[f64; 3]> {
        match self.peek()? {
            Token::Position(value) => {
                self.pos += 1;
                Some(*value)
            }
            _ => None,
        }
    }

    /// Consume one `0x14` vector triple.
    pub(crate) fn take_vector3(&mut self) -> Option<[f64; 3]> {
        match self.peek()? {
            Token::Vector3(value) => {
                self.pos += 1;
                Some(*value)
            }
            _ => None,
        }
    }

    /// Consume a `Long` count followed by that many `Double`s.
    pub(crate) fn take_float_array(&mut self) -> Option<Vec<f64>> {
        let mark = self.pos;
        let Some(count) = self.take_long().and_then(|c| usize::try_from(c).ok()) else {
            self.pos = mark;
            return None;
        };
        let mut values = Vec::new();
        for _ in 0..count {
            let Some(value) = self.take_f64() else {
                self.pos = mark;
                return None;
            };
            values.push(value);
        }
        Some(values)
    }

    /// Consume an optional leading boolean, then one `Double`: the range-bound
    /// form whose presence flag some releases serialize and some omit.
    pub(crate) fn take_range_value(&mut self) -> Option<f64> {
        let mark = self.pos;
        if matches!(self.peek(), Some(Token::True | Token::False)) {
            self.pos += 1;
        }
        let Some(value) = self.take_f64() else {
            self.pos = mark;
            return None;
        };
        Some(value)
    }

    /// Consume one optional range bound: `True` + `Double` or a bare `Double`
    /// is a present bound, `False` is an absent bound. The outer `None` is a
    /// parse failure.
    #[allow(clippy::option_option)] // Outer None is parse failure; inner None is an absent bound.
    pub(crate) fn take_optional_range_value(&mut self) -> Option<Option<f64>> {
        let mark = self.pos;
        match self.peek()? {
            Token::True => {
                self.pos += 1;
                let Some(value) = self.take_f64() else {
                    self.pos = mark;
                    return None;
                };
                Some(Some(value))
            }
            Token::False => {
                self.pos += 1;
                Some(None)
            }
            Token::Double(_) => self.take_f64().map(Some),
            _ => None,
        }
    }
}

/// Read a knot table of `n` `(knot, multiplicity)` pairs from the cursor.
///
/// Expansion adds one to each endpoint multiplicity. The pole count is
/// `sum(mult) - (degree - 1)`.
pub(crate) fn take_knot_table(
    cur: &mut Cur<'_>,
    n: usize,
    degree: i64,
) -> Option<(Vec<f64>, usize)> {
    let mut values = Vec::new();
    let mut mults = Vec::new();
    for _ in 0..n {
        values.push(cur.take_f64()?);
        mults.push(cur.take_long()?);
    }
    let sum: i64 = mults.iter().sum();
    let n_poles = sum - (degree - 1);
    if !(2..=100_000).contains(&n_poles) {
        return None;
    }
    let mut expanded = Vec::new();
    for (i, (value, mult)) in values.iter().zip(&mults).enumerate() {
        let extra = i64::from(i == 0 || i == n - 1);
        for _ in 0..usize::try_from((*mult + extra).max(0)).ok()? {
            expanded.push(*value);
        }
    }
    Some((expanded, n_poles as usize))
}

/// A B-spline block marker: `nubs` introduces a non-rational block, `nurbs` a
/// rational one whose poles carry a fourth weight component.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) enum BsplineMarker {
    /// Non-rational: three doubles per pole.
    Nubs,
    /// Rational: four doubles per pole, the fourth a homogeneous weight.
    Nurbs,
}

impl BsplineMarker {
    /// Doubles per control point.
    pub(crate) fn cp_dims(self) -> usize {
        match self {
            Self::Nubs => 3,
            Self::Nurbs => 4,
        }
    }

    /// Whether poles carry homogeneous weights.
    pub(crate) fn rational(self) -> bool {
        self == Self::Nurbs
    }
}

/// The B-spline marker at token `pos`, if any.
pub(crate) fn marker_at(toks: &[Token], pos: usize) -> Option<BsplineMarker> {
    match toks.get(pos)? {
        Token::Ident(name) if name == "nubs" => Some(BsplineMarker::Nubs),
        Token::Ident(name) if name == "nurbs" => Some(BsplineMarker::Nurbs),
        _ => None,
    }
}

/// Token indices of the `nubs`/`nurbs` markers `toks` itself owns: those
/// outside every construction nested within it. The span's outer
/// `SubtypeOpen` sets the initial nesting depth.
pub(crate) fn owned_marker_positions(toks: &[Token]) -> Vec<usize> {
    let mut out = Vec::new();
    let mut depth = 0usize;
    let start = usize::from(matches!(toks.first(), Some(Token::SubtypeOpen)));
    for (pos, token) in toks.iter().enumerate().skip(start) {
        match token {
            Token::SubtypeOpen => depth += 1,
            Token::SubtypeClose => depth = depth.saturating_sub(1),
            _ => {
                if depth == 0 && marker_at(toks, pos).is_some() {
                    out.push(pos);
                }
            }
        }
    }
    out
}

/// Token indices and names of the subtype definitions `toks` itself owns: the
/// `SubtypeOpen`s at the outermost nesting level whose next token is an
/// identifier, in order, `ref` included. A definition inside a nested scope
/// belongs to that scope's construction, not to `toks`.
pub(crate) fn owned_subtype_defs(toks: &[Token]) -> Vec<(usize, &str)> {
    let mut owned = Vec::new();
    let mut depth = 0usize;
    for (pos, token) in toks.iter().enumerate() {
        match token {
            Token::SubtypeOpen => {
                if depth == 0 {
                    if let Some(Token::Ident(name) | Token::SubIdent(name)) = toks.get(pos + 1) {
                        owned.push((pos, name.as_str()));
                    }
                }
                depth += 1;
            }
            Token::SubtypeClose => depth = depth.saturating_sub(1),
            _ => {}
        }
    }
    owned
}

/// Token index of the first subtype definition `toks` owns whose name matches
/// one of `names`, with the matched name. Names are tried in order; the first
/// name with a hit wins.
///
/// A construction owns a record through its own definition. Nested definitions
/// belong to their enclosing construction, so this function ignores matching
/// markers in nested scopes.
pub(crate) fn find_owned_subtype_marker<'n>(
    toks: &[Token],
    names: &[&'n str],
) -> Option<(usize, &'n str)> {
    let owned = owned_subtype_defs(toks);
    names.iter().copied().find_map(|name| {
        owned
            .iter()
            .find(|(_, owned_name)| *owned_name == name)
            .map(|(start, _)| (*start, name))
    })
}

/// The construction `toks` is, under its modern name: the first subtype
/// definition `toks` owns other than `ref`, canonicalized.
pub fn owned_construction_subtype(toks: &[Token]) -> Option<String> {
    owned_subtype_defs(toks)
        .into_iter()
        .map(|(_, name)| name)
        .find(|name| *name != "ref")
        .map(|name| canonical_intcurve_kind(name).into())
}

/// The unique cache-bearing non-reference scope owned directly by `toks`.
///
/// A record can own auxiliary outer definitions before its carrier. A scope is
/// cache-bearing when it directly owns at least one B-spline marker. Multiple
/// such scopes are ambiguous and are therefore rejected.
pub(crate) fn owned_cache_scope(toks: &[Token]) -> Option<&[Token]> {
    let mut candidates = owned_subtype_defs(toks)
        .into_iter()
        .filter(|(_, name)| *name != "ref")
        .filter_map(|(start, _)| subtype_span(toks, start))
        .filter(|scope| !owned_marker_positions(scope).is_empty());
    let scope = candidates.next()?;
    candidates.next().is_none().then_some(scope)
}

fn canonical_intcurve_kind(name: &str) -> &str {
    match name {
        "bldcur" => "blend_int_cur",
        "blndsprngcur" => "spring_int_cur",
        "exactcur" => "exact_int_cur",
        "lawintcur" => "law_int_cur",
        "offintcur" => "off_int_cur",
        "offsetintcur" => "offset_int_cur",
        "offsurfintcur" => "off_surf_int_cur",
        "parasil" => "para_silh_int_cur",
        "parcur" => "par_int_cur",
        "projcur" => "proj_int_cur",
        "surfcur" => "surf_int_cur",
        "surfintcur" => "int_int_cur",
        "d5c2_cur" => "skin_int_cur",
        "subsetintcur" => "subset_int_cur",
        _ => name,
    }
}

/// Token index of the `intcurve` subtype definition `toks` owns, given the
/// subtype's modern name. The legacy spelling of the same construction is
/// accepted as a second candidate.
pub(crate) fn find_owned_intcurve_subtype(toks: &[Token], modern: &str) -> Option<usize> {
    let legacy = match modern {
        "blend_int_cur" => "bldcur",
        "spring_int_cur" => "blndsprngcur",
        "exact_int_cur" => "exactcur",
        "law_int_cur" => "lawintcur",
        "off_int_cur" => "offintcur",
        "offset_int_cur" => "offsetintcur",
        "off_surf_int_cur" => "offsurfintcur",
        "para_silh_int_cur" => "parasil",
        "par_int_cur" => "parcur",
        "proj_int_cur" => "projcur",
        "surf_int_cur" => "surfcur",
        "int_int_cur" => "surfintcur",
        "skin_int_cur" => "d5c2_cur",
        "subset_int_cur" => "subsetintcur",
        _ => "",
    };
    let candidates: Vec<&str> = [modern, legacy]
        .into_iter()
        .filter(|name| !name.is_empty())
        .collect();
    find_owned_subtype_marker(toks, &candidates).map(|(marker, _)| marker)
}

/// The token span of the balanced subtype scope opening at `start`, inclusive
/// of both delimiters.
pub(crate) fn subtype_span(toks: &[Token], start: usize) -> Option<&[Token]> {
    let mut depth = 0usize;
    for (pos, token) in toks.iter().enumerate().skip(start) {
        match token {
            Token::SubtypeOpen => depth += 1,
            Token::SubtypeClose => {
                depth = depth.checked_sub(1)?;
                if depth == 0 {
                    return toks.get(start..=pos);
                }
            }
            _ => {}
        }
    }
    None
}

/// Subtype-table reference indices in `toks`, in token order: the
/// `{ref N}` form (`SubtypeOpen`, `Ident("ref")`, `Long(N)`) and the bare
/// index form (`SubtypeOpen`, `Long(N)`, `SubtypeClose`).
pub(crate) fn subtype_refs(toks: &[Token]) -> Vec<usize> {
    let mut refs = Vec::new();
    for (pos, token) in toks.iter().enumerate() {
        if !matches!(token, Token::SubtypeOpen) {
            continue;
        }
        match (toks.get(pos + 1), toks.get(pos + 2)) {
            (Some(Token::Ident(name)), Some(Token::Long(index)))
                if name == "ref" && *index >= 0 =>
            {
                refs.push(*index as usize);
            }
            (Some(Token::Long(index)), Some(Token::SubtypeClose)) if *index >= 0 => {
                refs.push(*index as usize);
            }
            _ => {}
        }
    }
    refs
}

/// The interior tokens of the subtype scope at payload chunk `chunk_index`
/// when its immediately following identifier is `expected`: everything after
/// that identifier up to (excluding) the matching close. Token-space
/// counterpart of [`crate::sab::payload_subtype_span`].
pub fn payload_subtype_toks<'r>(
    record: &'r crate::sab::Record,
    chunk_index: usize,
    expected: &str,
) -> Option<&'r [Token]> {
    let mut chunk = 0usize;
    let mut open = None;
    for (pos, token) in record.tokens.iter().enumerate() {
        if token.is_payload_ident() {
            continue;
        }
        if chunk == chunk_index {
            open = Some(pos);
            break;
        }
        chunk += 1;
    }
    let open = open?;
    if !matches!(record.tokens.get(open), Some(Token::SubtypeOpen)) {
        return None;
    }
    let (Token::Ident(name) | Token::SubIdent(name)) = record.tokens.get(open + 1)? else {
        return None;
    };
    if name != expected {
        return None;
    }
    let span = subtype_span(&record.tokens, open)?;
    span.get(2..span.len() - 1)
}

/// Token positions of the stream's subtype definitions, in stream order.
///
/// A subtype definition opens as `SubtypeOpen` followed by an identifier other
/// than `ref`, at any nesting depth; `{ref N}` references resolve to the `N`-th
/// entry. Each entry holds the owning record's shared payload tokens and the
/// definition's token index within them, so resolution needs no side channel
/// back to the record table.
pub struct SubtypeTable {
    defs: Vec<(std::sync::Arc<[Token]>, usize)>,
    /// The stream's ASM save format version from the `asmheader` record. Tokens
    /// omit this value, so the table carries it into token-space decoders.
    save_format_version: Option<u32>,
}

impl SubtypeTable {
    /// Build the table over each framed record's payload tokens, in order.
    pub fn from_records(records: &[crate::sab::Record]) -> Self {
        let mut defs = Vec::new();
        for record in records {
            for (pos, token) in record.tokens.iter().enumerate() {
                if matches!(token, Token::SubtypeOpen) {
                    if let Some(Token::Ident(name) | Token::SubIdent(name)) =
                        record.tokens.get(pos + 1)
                    {
                        if name != "ref" {
                            defs.push((record.tokens.clone(), pos));
                        }
                    }
                }
            }
        }
        Self {
            defs,
            save_format_version: None,
        }
    }

    /// Attach the stream's ASM save format version.
    #[must_use]
    pub fn with_save_format_version(mut self, version: Option<u32>) -> Self {
        self.save_format_version = version;
        self
    }

    /// The stream's ASM save format version, when known.
    pub(crate) fn save_format_version(&self) -> Option<u32> {
        self.save_format_version
    }

    /// The token span of definition `index`, sliced from its owning record.
    pub(crate) fn span(&self, index: usize) -> Option<&[Token]> {
        let (tokens, token_pos) = self.defs.get(index)?;
        subtype_span(tokens, *token_pos)
    }
}

/// Lex a bare byte span (a subtype scope or block without a record name or
/// terminator) into payload tokens, for tests that build byte fixtures.
///
/// # Panics
///
/// Panics when `bytes` fails to lex as one record payload.
pub fn lex_test_span(bytes: &[u8], ref_width: usize) -> std::sync::Arc<[Token]> {
    let mut wrapped = vec![0x0d, 1, b'x'];
    wrapped.extend_from_slice(bytes);
    wrapped.push(0x11);
    let records =
        crate::sab::frame(&wrapped, 0, wrapped.len(), ref_width).expect("test span lexes");
    records.into_iter().next().expect("one record").tokens
}

/// Build a [`SubtypeTable`] over a bare byte span, for tests.
pub fn test_table(bytes: &[u8], ref_width: usize) -> SubtypeTable {
    let record = crate::sab::Record {
        index: 0,
        name: String::new(),
        head: String::new(),
        tokens: lex_test_span(bytes, ref_width),
        offset: 0,
        len: 0,
    };
    SubtypeTable::from_records(&[record])
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ident(name: &str) -> Token {
        Token::Ident(name.to_string())
    }

    #[test]
    fn cursor_take_methods_do_not_advance_on_mismatch() {
        let toks = [Token::Double(2.5), Token::True, Token::Long(7)];
        let mut cur = Cur::at(&toks, 0);
        assert_eq!(cur.take_long(), None);
        assert_eq!(cur.take_f64(), Some(2.5));
        assert_eq!(cur.take_bool(), Some(true));
        assert_eq!(cur.take_long(), Some(7));
        assert_eq!(cur.bump(), None);
    }

    #[test]
    fn float_array_restores_position_on_a_truncated_body() {
        let toks = [Token::Long(2), Token::Double(1.0), Token::True];
        let mut cur = Cur::at(&toks, 0);
        assert_eq!(cur.take_float_array(), None);
        assert_eq!(cur.pos(), 0);
    }

    #[test]
    fn scope_end_requires_the_terminal_close() {
        let toks = [Token::SubtypeOpen, ident("x"), Token::SubtypeClose];
        assert!(Cur::at(&toks, 2).at_scope_end());
        assert!(!Cur::at(&toks, 1).at_scope_end());

        let trailing = [
            Token::SubtypeOpen,
            ident("x"),
            Token::SubtypeClose,
            Token::Double(1.0),
        ];
        assert!(!Cur::at(&trailing, 2).at_scope_end());
    }

    #[test]
    fn owned_defs_skip_nested_constructions() {
        // The ref belongs to the nested `ref` scope.
        let toks = [
            Token::SubtypeOpen,
            ident("exactcur"),
            Token::SubtypeOpen,
            ident("ref"),
            Token::Long(3),
            Token::SubtypeClose,
            Token::SubtypeClose,
        ];
        assert_eq!(owned_subtype_defs(&toks), vec![(0, "exactcur")]);
        assert_eq!(subtype_refs(&toks), vec![3]);
        assert_eq!(
            owned_construction_subtype(&toks),
            Some("exact_int_cur".to_string())
        );
        assert_eq!(subtype_span(&toks, 2), Some(&toks[2..=5]));
    }

    #[test]
    fn owned_markers_ignore_nested_scopes_and_a_leading_open() {
        let toks = [
            Token::SubtypeOpen,
            ident("nubs"),
            Token::SubtypeOpen,
            ident("nurbs"),
            Token::SubtypeClose,
        ];
        // Leading open is the span's own scope: the first `nubs` is owned, the
        // `nurbs` inside the nested scope is not.
        assert_eq!(owned_marker_positions(&toks), vec![1]);
        assert_eq!(marker_at(&toks, 1), Some(BsplineMarker::Nubs));
        assert_eq!(marker_at(&toks, 3), Some(BsplineMarker::Nurbs));
    }
}
