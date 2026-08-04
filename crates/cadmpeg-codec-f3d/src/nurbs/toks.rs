// SPDX-License-Identifier: Apache-2.0
//! Token-space cursor and subtype walkers over framed [`Token`] payloads.
//!
//! These are the encoding-independent counterparts of the byte-level readers
//! in [`crate::nurbs::reader`] and walkers in [`crate::nurbs::subtypes`]. The
//! framer resolves the stream's integer width and retains payload identifiers,
//! so a token walk needs no width probing and recognizes a subtype
//! definition's name as a token rather than a byte pattern. Positions are
//! token indices within one record's payload; they serve the same role the
//! byte offsets served — ordering and identity — without binding the decoder
//! to one serialization.

use crate::sab::Token;
use cadmpeg_ir::math::Vector3;

/// A cursor over one record's payload tokens.
///
/// `take_*` methods consume one token (or one counted group) and return its
/// value, or return `None` without advancing when the next token is not of the
/// requested kind — the same contract as the byte cursors they replace.
#[derive(Clone, Copy)]
pub(crate) struct Cur<'a> {
    toks: &'a [Token],
    pos: usize,
}

impl<'a> Cur<'a> {
    pub(crate) fn new(toks: &'a [Token]) -> Self {
        Self { toks, pos: 0 }
    }

    pub(crate) fn at(toks: &'a [Token], pos: usize) -> Self {
        Self { toks, pos }
    }

    /// Current token index.
    pub(crate) fn pos(&self) -> usize {
        self.pos
    }

    /// The token at the cursor, without consuming it.
    pub(crate) fn peek(&self) -> Option<&'a Token> {
        self.toks.get(self.pos)
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

    /// Consume one `0x16` `(u, v)` pair.
    pub(crate) fn take_vector2(&mut self) -> Option<[f64; 2]> {
        match self.peek()? {
            Token::Vector2(value) => {
                self.pos += 1;
                Some(*value)
            }
            _ => None,
        }
    }

    /// Consume one entity reference, `-1` included.
    pub(crate) fn take_ref(&mut self) -> Option<i64> {
        match self.peek()? {
            Token::Ref(value) => {
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

/// Return `v` normalized to unit length, or `None` when degenerate.
pub(crate) fn normalized(value: [f64; 3]) -> Option<Vector3> {
    crate::nurbs::reader::normalized(value)
}

/// The B-spline marker at token `pos`, if any: `(control-point dimension,
/// rational?)`. `nubs` introduces a non-rational block, `nurbs` a rational one.
pub(crate) fn marker_at(toks: &[Token], pos: usize) -> Option<(usize, bool)> {
    match toks.get(pos)? {
        Token::Ident(name) if name == "nubs" => Some((3, false)),
        Token::Ident(name) if name == "nurbs" => Some((4, true)),
        _ => None,
    }
}

/// Token indices of every `nubs`/`nurbs` marker in `toks`, in order.
pub(crate) fn marker_positions(toks: &[Token]) -> Vec<usize> {
    (0..toks.len())
        .filter(|&pos| marker_at(toks, pos).is_some())
        .collect()
}

/// Token indices of the `nubs`/`nurbs` markers `toks` itself owns: those
/// outside every construction nested within it. A leading `SubtypeOpen` is the
/// span's own scope opening and is not counted as nesting.
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
/// A construction claims a record only through the definition the record owns.
/// Records nest complete constructions as supports, so a decoder that accepted
/// any matching marker anywhere in the record would claim records belonging to
/// the construction that encloses it.
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
pub(crate) fn owned_construction_subtype(toks: &[Token]) -> Option<String> {
    owned_subtype_defs(toks)
        .into_iter()
        .map(|(_, name)| name)
        .find(|name| *name != "ref")
        .map(|name| canonical_intcurve_kind(name).into())
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

/// Token positions of the stream's subtype definitions, in stream order.
///
/// A subtype definition opens as `SubtypeOpen` followed by an identifier other
/// than `ref`, at any nesting depth; `{ref N}` references resolve to the `N`-th
/// entry. Entries are `(record index in the framed table, token index in that
/// record's payload)`.
pub(crate) struct SubtypeTable {
    defs: Vec<(usize, usize)>,
}

impl SubtypeTable {
    /// Build the table over each framed record's payload tokens, in order.
    pub(crate) fn from_records(records: &[crate::sab::Record]) -> Self {
        let mut defs = Vec::new();
        for (record_pos, record) in records.iter().enumerate() {
            for (pos, token) in record.tokens.iter().enumerate() {
                if matches!(token, Token::SubtypeOpen) {
                    if let Some(Token::Ident(name) | Token::SubIdent(name)) =
                        record.tokens.get(pos + 1)
                    {
                        if name != "ref" {
                            defs.push((record_pos, pos));
                        }
                    }
                }
            }
        }
        Self { defs }
    }

    /// The token span of definition `index`, sliced from its owning record.
    pub(crate) fn span<'r>(
        &self,
        records: &'r [crate::sab::Record],
        index: usize,
    ) -> Option<&'r [Token]> {
        let (record_pos, token_pos) = *self.defs.get(index)?;
        subtype_span(&records.get(record_pos)?.tokens, token_pos)
    }
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
        let mut cur = Cur::new(&toks);
        assert_eq!(cur.take_long(), None);
        assert_eq!(cur.take_f64(), Some(2.5));
        assert_eq!(cur.take_bool(), Some(true));
        assert_eq!(cur.take_long(), Some(7));
        assert_eq!(cur.bump(), None);
    }

    #[test]
    fn float_array_restores_position_on_a_truncated_body() {
        let toks = [Token::Long(2), Token::Double(1.0), Token::True];
        let mut cur = Cur::new(&toks);
        assert_eq!(cur.take_float_array(), None);
        assert_eq!(cur.pos(), 0);
    }

    #[test]
    fn owned_defs_skip_nested_constructions() {
        // { exactcur { ref 3 } } — the ref belongs to the nested scope.
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
        assert_eq!(marker_positions(&toks), vec![1, 3]);
        assert_eq!(marker_at(&toks, 1), Some((3, false)));
        assert_eq!(marker_at(&toks, 3), Some((4, true)));
    }
}
