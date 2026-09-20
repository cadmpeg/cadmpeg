// SPDX-License-Identifier: Apache-2.0
//! Token-space cursor and subtype walkers over framed [`Token`] payloads.
//!
//! These walkers mirror the byte readers in [`crate::nurbs::reader`] and the
//! subtype walkers in [`crate::nurbs::subtypes`]. The framer resolves integer
//! width and retains payload identifiers, so the walkers use token names and
//! positions. Token positions identify fields within a record payload without
//! depending on serialized byte offsets.

#[cfg(any(test, feature = "test-support"))]
use crate::kernel_header::RefWidth;
use crate::nurbs::reader::{checked_knot_layout, BsplineMarker, Nullable};
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
    pub(super) fn set_pos(&mut self, pos: usize) {
        self.pos = pos;
    }

    /// The full token slice the cursor walks.
    pub(super) fn toks(&self) -> &'a [Token] {
        self.toks
    }

    /// The token at the cursor, without consuming it.
    pub(super) fn peek(&self) -> Option<&'a Token> {
        self.toks.get(self.pos)
    }

    /// Whether the cursor is at the closing token of this complete subtype
    /// span, with no unconsumed field before or token after it.
    pub(super) fn at_scope_end(&self) -> bool {
        self.pos + 1 == self.toks.len()
            && matches!(self.toks.get(self.pos), Some(Token::SubtypeClose))
    }

    /// Consume one token of any kind.
    pub(super) fn bump(&mut self) -> Option<&'a Token> {
        let token = self.toks.get(self.pos)?;
        self.pos += 1;
        Some(token)
    }

    /// The remaining tokens from the cursor onward.
    pub(super) fn rest(&self) -> &'a [Token] {
        self.toks.get(self.pos..).unwrap_or(&[])
    }

    pub(super) fn take_f64(&mut self) -> Option<f64> {
        match self.peek()? {
            Token::Double(value) => {
                self.pos += 1;
                Some(*value)
            }
            _ => None,
        }
    }

    pub(super) fn take_long(&mut self) -> Option<i64> {
        match self.peek()? {
            Token::Long(value) => {
                self.pos += 1;
                Some(*value)
            }
            _ => None,
        }
    }

    pub(super) fn take_enum(&mut self) -> Option<i64> {
        match self.peek()? {
            Token::Enum(value) => {
                self.pos += 1;
                Some(*value)
            }
            _ => None,
        }
    }

    pub(super) fn take_bool(&mut self) -> Option<bool> {
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

    pub(super) fn take_str(&mut self) -> Option<&'a str> {
        match self.peek()? {
            Token::Str(value) => {
                self.pos += 1;
                Some(value)
            }
            _ => None,
        }
    }

    /// Consume one payload identifier (`Ident` or `SubIdent`).
    pub(super) fn take_ident(&mut self) -> Option<&'a str> {
        match self.peek()? {
            Token::Ident(value) | Token::SubIdent(value) => {
                self.pos += 1;
                Some(value)
            }
            _ => None,
        }
    }

    /// Consume one `0x13` position triple.
    pub(super) fn take_position(&mut self) -> Option<[f64; 3]> {
        match self.peek()? {
            Token::Position(value) => {
                self.pos += 1;
                Some(*value)
            }
            _ => None,
        }
    }

    /// Consume one `0x14` vector triple.
    pub(super) fn take_vector3(&mut self) -> Option<[f64; 3]> {
        match self.peek()? {
            Token::Vector3(value) => {
                self.pos += 1;
                Some(*value)
            }
            _ => None,
        }
    }

    /// Consume a `Long` count followed by that many `Double`s.
    pub(super) fn take_float_array(&mut self) -> Option<Vec<f64>> {
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
    pub(super) fn take_range_value(&mut self) -> Option<f64> {
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
    pub(super) fn take_optional_range_value(&mut self) -> Option<Nullable<f64>> {
        let mark = self.pos;
        match self.peek()? {
            Token::True => {
                self.pos += 1;
                let Some(value) = self.take_f64() else {
                    self.pos = mark;
                    return None;
                };
                Some(Nullable::Value(value))
            }
            Token::False => {
                self.pos += 1;
                Some(Nullable::Null)
            }
            Token::Double(_) => self.take_f64().map(Nullable::Value),
            _ => None,
        }
    }
}

/// Read a knot table of `n` `(knot, multiplicity)` pairs from the cursor.
///
/// Expansion adds one to each endpoint multiplicity. The pole count is
/// `sum(mult) - (degree - 1)`.
pub(super) fn take_knot_table(
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
    let expansion = checked_knot_layout(&mults, degree)?;
    let mut expanded = Vec::with_capacity(expansion.expanded_len());
    for (value, &run_length) in values.iter().zip(&expansion.expanded_run_lengths) {
        for _ in 0..run_length {
            expanded.push(*value);
        }
    }
    Some((expanded, expansion.n_poles))
}

/// The B-spline marker at token `pos`, if any.
pub(super) fn marker_at(toks: &[Token], pos: usize) -> Option<BsplineMarker> {
    match toks.get(pos)? {
        Token::Ident(name) if name == "nubs" => Some(BsplineMarker::Nubs),
        Token::Ident(name) if name == "nurbs" => Some(BsplineMarker::Nurbs),
        _ => None,
    }
}

/// Token indices of the `nubs`/`nurbs` markers `toks` itself owns: those
/// outside every construction nested within it. The span's outer
/// `SubtypeOpen` sets the initial nesting depth.
///
/// A `SubtypeClose` with no open scope is a malformed token stream and is
/// refused: pinning the depth at zero would make every later marker read as one
/// this span owns.
pub(super) fn owned_marker_positions(toks: &[Token]) -> Option<Vec<usize>> {
    let (out, balanced) = walk_owned_markers(toks);
    balanced.then_some(out)
}

/// The owned-marker walk, with the balance it observed.
///
/// The second element is `false` when the walk met a `SubtypeClose` that no
/// open in `toks` matches. A balanced stream always answers `true`, so a caller
/// holding a [`SubtypeScope`] reads the first element alone.
fn walk_owned_markers(toks: &[Token]) -> (Vec<usize>, bool) {
    let mut out = Vec::new();
    let mut depth = 0usize;
    // The span's own leading `SubtypeOpen` is skipped, so the close that
    // matches it is the one close this walk admits at depth zero.
    let mut outer = usize::from(matches!(toks.first(), Some(Token::SubtypeOpen)));
    for (pos, token) in toks.iter().enumerate().skip(outer) {
        match token {
            Token::SubtypeOpen => depth += 1,
            Token::SubtypeClose => match depth.checked_sub(1) {
                Some(next) => depth = next,
                None => match outer.checked_sub(1) {
                    Some(next) => outer = next,
                    None => return (out, false),
                },
            },
            _ => {
                if depth == 0 && marker_at(toks, pos).is_some() {
                    out.push(pos);
                }
            }
        }
    }
    (out, true)
}

/// Token indices and names of the subtype definitions `toks` itself owns: the
/// `SubtypeOpen`s at the outermost nesting level whose next token is an
/// identifier, in order, `ref` included. A definition inside a nested scope
/// belongs to that scope's construction, not to `toks`.
///
/// A `SubtypeClose` with no open scope is a malformed token stream and is
/// refused.
pub(super) fn owned_subtype_defs(toks: &[Token]) -> Option<Vec<(usize, &str)>> {
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
            Token::SubtypeClose => depth = depth.checked_sub(1)?,
            _ => {}
        }
    }
    Some(owned)
}

/// Token index of the first subtype definition `toks` owns whose name matches
/// one of `names`, with the matched name. Names are tried in order; the first
/// name with a hit wins.
///
/// A construction owns a record through its own definition. Nested definitions
/// belong to their enclosing construction, so this function ignores matching
/// markers in nested scopes.
pub(super) fn find_owned_subtype_marker<'n>(
    toks: &[Token],
    names: &[&'n str],
) -> Option<(usize, &'n str)> {
    let owned = owned_subtype_defs(toks)?;
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
    owned_subtype_defs(toks)?
        .into_iter()
        .map(|(_, name)| name)
        .find(|name| *name != "ref")
        .map(|name| canonical_intcurve_kind(name).into())
}

/// The token span that carries `toks`'s cache, or `None` when the record
/// states none.
///
/// `docs/formats/asm.md`: "the outer non-`ref` procedural subtype owns the
/// record's solved curve or surface cache. A B-spline block inside a subtype
/// nested by that construction belongs to the nested support, source, guide,
/// or child field and is not a candidate for the outer construction's cache."
/// A scope is cache-bearing when it directly owns at least one B-spline
/// marker. The sentence states a construction's ownership, so it decides where
/// the record owns a construction:
///
/// * Exactly one non-`ref` scope the record owns directly is cache-bearing:
///   that scope is the cache span.
/// * More than one is cache-bearing: the record names no single owner, so it
///   states no cache span.
/// * The record owns non-`ref` scopes and none is cache-bearing: the
///   construction's cache is absent. Every remaining block sits in a nested
///   support, source, guide or child scope, which the sentence excludes, so
///   the record states no cache span.
/// * The record owns no non-`ref` scope: it states no construction, so no
///   construction owns its blocks and the record's own stream is the cache
///   span. This last case is the decoder's decision, not the sentence's. The
///   sentence states what a construction owns, so a record that states no
///   construction is outside it. The nearest sentence that bears on it is
///   `docs/formats/asm.md:403`, `exact_int_cur`: "the solved `nubs`/`nurbs`
///   curve cache is the authoritative exact construction payload" — the cache
///   is the record's own payload there — but that sentence names one subtype,
///   and this case covers every record that states no owned construction,
///   including the `ref` forms whose subtype is named in the subtype table. No
///   sentence in `asm.md` states the general case. What states it is the
///   routes that exercise it: the four f3d writer round trips
///   `writer::tests::source_less_nurbs::
///   generated_source_less_face_writes_nurbs_surface_carrier`,
///   `..._writes_rational_nurbs_surface_carrier`,
///   `..._writes_rational_nurbs_edge_curve` and
///   `generated_source_less_multi_face_writes_nurbs_carriers_and_pcurve` in
///   `cadmpeg-codec-f3d`, which encode a source-less NURBS body and decode it
///   back. The written record owns no procedural subtype at all, so its
///   B-spline block is the record's geometry and not a construction's cache.
///   Removing this arm fails all four on `solved carrier`.
///
/// This is the one answer; every reader of a record cache calls it and none
/// states a fallback of its own.
///
/// A malformed token stream is refused, not worked around: `owned_subtype_defs`
/// answers `None` rather than passing over the scope that stated it.
pub(super) fn cache_scope(toks: &[Token]) -> Option<&[Token]> {
    let mut constructions = 0usize;
    let mut cache_bearing = Vec::new();
    for (start, _) in owned_subtype_defs(toks)?
        .into_iter()
        .filter(|(_, name)| *name != "ref")
    {
        constructions += 1;
        let Some(scope) = subtype_span(toks, start) else {
            continue;
        };
        if !scope.owned_marker_positions().is_empty() {
            cache_bearing.push(scope.tokens());
        }
    }
    match (cache_bearing.as_slice(), constructions) {
        ([scope], _) => Some(scope),
        ([], 0) => Some(toks),
        _ => None,
    }
}

fn canonical_intcurve_kind(name: &str) -> &str {
    super::subtypes::INTCURVE_ALIASES
        .iter()
        .find_map(|(modern, legacy)| (*legacy == name).then_some(*modern))
        .unwrap_or(name)
}

/// Token index of the `intcurve` subtype definition `toks` owns, given the
/// subtype's modern name. The legacy spelling of the same construction is
/// accepted as a second candidate.
pub(super) fn find_owned_intcurve_subtype(toks: &[Token], modern: &str) -> Option<usize> {
    if modern.is_empty() {
        return None;
    }
    let legacy = super::subtypes::INTCURVE_ALIASES
        .iter()
        .find_map(|(name, alias)| (*name == modern).then_some(*alias));
    let found = match legacy {
        Some(legacy) => find_owned_subtype_marker(toks, &[modern, legacy]),
        None => find_owned_subtype_marker(toks, &[modern]),
    };
    found.map(|(marker, _)| marker)
}

/// A balanced subtype scope in token space.
///
/// [`subtype_span`] is the only constructor: the field is private and the type
/// has no `From` and no `Deref`. Every value therefore states one scope whose
/// every `SubtypeClose` has a matching open within the span, and whose final
/// token is the close that balances it. The walks that refuse an unbalanced
/// stream are total over this type.
///
/// The fields are not reachable from another module:
///
/// ```compile_fail
/// use cadmpeg_asm::nurbs::toks::SubtypeScope;
/// use cadmpeg_asm::sab::Token;
///
/// let toks = [Token::SubtypeOpen, Token::SubtypeClose];
/// let scope = SubtypeScope { tokens: &toks[..] };
/// ```
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct SubtypeScope<'a> {
    tokens: &'a [Token],
}

impl<'a> SubtypeScope<'a> {
    /// The scope's tokens, both delimiters included.
    pub fn tokens(&self) -> &'a [Token] {
        self.tokens
    }

    /// The tokens the scope encloses: everything between its opening token and
    /// the close that balances it. For a scope that names a construction the
    /// first of these is the identifier that names it, and the rest are the
    /// fields the construction states.
    ///
    /// Total: [`subtype_span`] is the only constructor. It refuses unless the
    /// token at `start` is the scope's own `SubtypeOpen`, and it builds
    /// `tokens` as `toks[start..=pos]` with `pos > start`, so the slice holds
    /// at least the opening and the closing delimiter and this range is in
    /// bounds. The proof is the constructor's, not the caller's.
    pub fn interior(&self) -> &'a [Token] {
        &self.tokens[1..self.tokens.len() - 1]
    }

    /// Token indices of the `nubs`/`nurbs` markers the scope itself owns: those
    /// outside every construction nested within it. The scope's outer
    /// `SubtypeOpen` sets the initial nesting depth.
    ///
    /// Direct ownership is the format's rule. `docs/formats/asm.md:395`: "the
    /// outer non-`ref` procedural subtype owns the record's solved curve or
    /// surface cache. A B-spline block inside a subtype nested by that
    /// construction belongs to the nested support, source, guide, or child
    /// field and is not a candidate for the outer construction's cache", and
    /// the owning block's ordinal is fixed "among the B-spline blocks directly
    /// owned by the construction".
    ///
    /// The walk skips one leading `SubtypeOpen`, which over these tokens is the
    /// scope's own. A caller that passed the slice after the scope's name token
    /// instead made the walk skip the open of a *first nested* scope and
    /// collect that nested construction's markers as owned; that shape was the
    /// defect, and no caller states it now.
    ///
    /// Total: the unbalanced stream that [`owned_marker_positions`] refuses is
    /// a state this type cannot hold.
    pub fn owned_marker_positions(&self) -> Vec<usize> {
        walk_owned_markers(self.tokens).0
    }
}

/// The balanced subtype scope opening at `start`, inclusive of both delimiters.
///
/// `None` unless the token at `start` is a `SubtypeOpen`. A scope opens at its
/// own opening delimiter, so a `start` that names another token names no
/// scope, and the span is then at least two tokens.
pub(super) fn subtype_span(toks: &[Token], start: usize) -> Option<SubtypeScope<'_>> {
    matches!(toks.get(start), Some(Token::SubtypeOpen)).then_some(())?;
    let mut depth = 0usize;
    for (pos, token) in toks.iter().enumerate().skip(start) {
        match token {
            Token::SubtypeOpen => depth += 1,
            Token::SubtypeClose => {
                depth = depth.checked_sub(1)?;
                if depth == 0 {
                    // `pos > start`: reaching depth one needs a `SubtypeOpen`
                    // at or after `start`, so the close that returns depth to
                    // zero is never the token at `start` itself. Both slices
                    // are therefore in range.
                    return Some(SubtypeScope {
                        tokens: toks.get(start..=pos)?,
                    });
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
pub(super) fn subtype_refs(toks: &[Token]) -> Vec<usize> {
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

/// The subtype scope at payload chunk `chunk_index` when its immediately
/// following identifier is `expected`. Token-space counterpart of
/// [`crate::sab::payload_subtype_span`].
///
/// The scope carries its own balance proof, so a caller that walks it needs no
/// walk of its own to establish one. The identifier this function matched is
/// the first token of [`SubtypeScope::interior`].
pub fn payload_subtype_toks<'r>(
    record: &'r crate::sab::Record,
    chunk_index: usize,
    expected: &str,
) -> Option<SubtypeScope<'r>> {
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
    subtype_span(&record.tokens, open)
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
    pub(super) fn save_format_version(&self) -> Option<u32> {
        self.save_format_version
    }

    /// The balanced scope of definition `index`, sliced from its owning record.
    pub(super) fn span(&self, index: usize) -> Option<SubtypeScope<'_>> {
        let (tokens, token_pos) = self.defs.get(index)?;
        subtype_span(tokens, *token_pos)
    }
}

/// Lex a bare byte span (a subtype scope or block without a record name or
/// terminator) into payload tokens, for tests that build byte fixtures.
///
/// # Errors
///
/// Refuses malformed tokens and bytes that frame as more than one record.
#[cfg(any(test, feature = "test-support"))]
pub fn lex_test_span(
    bytes: &[u8],
    ref_width: RefWidth,
) -> Result<std::sync::Arc<[Token]>, crate::stream_error::StreamError> {
    let mut wrapped = vec![0x0d, 1, b'x'];
    wrapped.extend_from_slice(bytes);
    wrapped.push(0x11);
    let records = crate::sab::frame(&wrapped, 0, wrapped.len(), ref_width)?;
    let [record]: [crate::sab::Record; 1] =
        records
            .try_into()
            .map_err(|records: Vec<_>| crate::stream_error::StreamError {
                format: crate::stream_error::StreamFormat::Binary,
                offset: 0,
                reason: format!(
                    "bare payload frames as {} records, expected one",
                    records.len()
                ),
            })?;
    Ok(record.tokens)
}

/// Build a [`SubtypeTable`] over a bare byte span, for tests.
#[cfg(any(test, feature = "test-support"))]
pub fn test_table(
    bytes: &[u8],
    ref_width: RefWidth,
) -> Result<SubtypeTable, crate::stream_error::StreamError> {
    let record = crate::sab::Record {
        index: 0,
        name: String::new(),

        tokens: lex_test_span(bytes, ref_width)?,
        offset: 0,
        len: 0,
    };
    Ok(SubtypeTable::from_records(&[record]))
}

#[cfg(test)]
mod tests {
    use super::{
        cache_scope, lex_test_span, marker_at, owned_construction_subtype, owned_marker_positions,
        owned_subtype_defs, subtype_refs, subtype_span, test_table, Cur,
    };
    use crate::kernel_header::RefWidth;
    use crate::nurbs::reader::BsplineMarker;
    use crate::sab::Token;

    fn ident(name: &str) -> Token {
        Token::Ident(name.to_string())
    }

    #[test]
    fn bare_payload_admission_preserves_empty_and_single_token_payloads() {
        for width in [RefWidth::Four, RefWidth::Eight] {
            assert!(lex_test_span(&[], width).unwrap().is_empty());
            assert_eq!(&*lex_test_span(&[0x0a], width).unwrap(), &[Token::True]);
        }
    }

    #[test]
    fn bare_payload_admission_refuses_truncated_tokens() {
        for width in [RefWidth::Four, RefWidth::Eight] {
            assert!(lex_test_span(&[0x06], width).is_err());
            assert!(test_table(&[0x06], width).is_err());
        }
    }

    #[test]
    fn bare_payload_admission_refuses_an_extra_record() {
        for width in [RefWidth::Four, RefWidth::Eight] {
            let bytes = [0x11, 0x0d, 1, b'y'];
            let error = lex_test_span(&bytes, width).unwrap_err();
            assert!(error.reason.contains("2 records"), "{error}");
            assert!(test_table(&bytes, width).is_err());
        }
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
        assert_eq!(owned_subtype_defs(&toks), Some(vec![(0, "exactcur")]));
        assert_eq!(subtype_refs(&toks), vec![3]);
        assert_eq!(
            owned_construction_subtype(&toks),
            Some("exact_int_cur".to_string())
        );
        assert_eq!(
            subtype_span(&toks, 2).map(|scope| scope.tokens()),
            Some(&toks[2..=5])
        );
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
        assert_eq!(owned_marker_positions(&toks), Some(vec![1]));
        assert_eq!(marker_at(&toks, 1), Some(BsplineMarker::Nubs));
        assert_eq!(marker_at(&toks, 3), Some(BsplineMarker::Nurbs));
    }

    #[test]
    fn a_scope_yields_its_owned_markers_with_no_refusal_to_answer() {
        // Two nested constructions inside one scope. The call binds a
        // `Vec<usize>` directly: `SubtypeScope::owned_marker_positions` states
        // no `Option`, because the type has already proven the balance the
        // raw-stream walk refuses.
        let toks = [
            Token::SubtypeOpen,
            ident("exactcur"),
            ident("nubs"),
            Token::SubtypeOpen,
            ident("ref"),
            Token::SubtypeOpen,
            ident("nurbs"),
            Token::SubtypeClose,
            Token::SubtypeClose,
            ident("nurbs"),
            Token::SubtypeClose,
        ];
        let scope = subtype_span(&toks, 0).expect("balanced scope");
        let owned: Vec<usize> = scope.owned_marker_positions();
        assert_eq!(owned, vec![2, 9]);
        assert_eq!(scope.tokens(), &toks[..]);
    }

    /// A scope opens at its own `SubtypeOpen`. An index that names another
    /// token names no scope, so `interior()` is total by construction: the
    /// constructor never hands back a span whose first token is not the open.
    #[test]
    fn a_span_that_does_not_open_at_start_is_not_a_scope() {
        let toks = [
            ident("x"),
            Token::SubtypeOpen,
            ident("exactcur"),
            Token::SubtypeClose,
        ];

        assert_eq!(subtype_span(&toks, 0), None);
        assert_eq!(subtype_span(&toks, 2), None);
        assert_eq!(subtype_span(&toks, 3), None);
        assert_eq!(subtype_span(&toks, 4), None);
        let scope = subtype_span(&toks, 1).expect("balanced scope");
        assert_eq!(scope.tokens(), &toks[1..=3]);
        assert_eq!(scope.interior(), &toks[2..3]);
    }

    #[test]
    fn a_first_nested_scope_does_not_own_the_outer_scopes_markers() {
        // The interior opens with a nested marker-bearing construction. Only
        // the outer scope's own `nurbs` is directly owned; the nested `nubs`
        // belongs to the nested construction (`docs/formats/asm.md:395`).
        let toks = [
            Token::SubtypeOpen,
            ident("exactcur"),
            Token::SubtypeOpen,
            ident("support"),
            ident("nubs"),
            Token::SubtypeClose,
            ident("nurbs"),
            Token::SubtypeClose,
        ];
        let scope = subtype_span(&toks, 0).expect("balanced scope");
        assert_eq!(scope.owned_marker_positions(), vec![6]);
        assert_eq!(scope.interior(), &toks[1..7]);

        // The slice after the scope's name token opens with the nested scope,
        // so the free walk skips that nested open and reports the nested
        // marker as owned. That is the shape the scope type replaces.
        assert_eq!(owned_marker_positions(&toks[2..7]), Some(vec![2, 4]));
    }

    #[test]
    fn one_cache_bearing_scope_is_the_cache_scope_and_two_are_ambiguous() {
        // `exactcur` owns a `nubs` marker; the `ref` scope is skipped by name.
        let one = [
            Token::SubtypeOpen,
            ident("exactcur"),
            ident("nubs"),
            Token::SubtypeClose,
            Token::SubtypeOpen,
            ident("ref"),
            Token::Long(3),
            Token::SubtypeClose,
        ];
        assert_eq!(cache_scope(&one), Some(&one[0..=3]));

        // A second scope owning a marker leaves no unique carrier.
        let two = [
            Token::SubtypeOpen,
            ident("exactcur"),
            ident("nubs"),
            Token::SubtypeClose,
            Token::SubtypeOpen,
            ident("exactcur"),
            ident("nurbs"),
            Token::SubtypeClose,
        ];
        assert_eq!(cache_scope(&two), None);

        // A scope owning no marker is not a candidate, and leaves the one
        // that does as the answer.
        let bare = [
            Token::SubtypeOpen,
            ident("exactcur"),
            Token::SubtypeClose,
            Token::SubtypeOpen,
            ident("exactcur"),
            ident("nubs"),
            Token::SubtypeClose,
        ];
        assert_eq!(cache_scope(&bare), Some(&bare[3..=6]));
    }

    #[test]
    fn an_unbalanced_close_refuses_the_cache_scope_rather_than_moving_to_another() {
        // The stream states one close more than it opens, so no scope is
        // chosen at all.
        let toks = [
            Token::SubtypeOpen,
            ident("exactcur"),
            ident("nubs"),
            Token::SubtypeClose,
            Token::SubtypeClose,
            Token::SubtypeOpen,
            ident("exactcur"),
            ident("nurbs"),
            Token::SubtypeClose,
        ];
        assert_eq!(owned_subtype_defs(&toks), None);
        assert_eq!(cache_scope(&toks), None);
    }

    #[test]
    fn one_unbalanced_close_refuses_the_owned_walks() {
        // One close more than the stream opens. Everything after it sits
        // outside every scope, so a walk that pinned the depth at zero
        // reported the trailing `nurbs` as a marker this span owns.
        let toks = [
            Token::SubtypeOpen,
            ident("exactcur"),
            Token::SubtypeOpen,
            ident("ref"),
            Token::SubtypeClose,
            Token::SubtypeClose,
            Token::SubtypeClose,
            ident("nurbs"),
        ];
        assert_eq!(owned_marker_positions(&toks), None);
        assert_eq!(owned_subtype_defs(&toks), None);
    }
    #[test]
    fn subtype_table_walks_wide_strings_at_the_stream_ref_width() {
        fn t_ident(b: &mut Vec<u8>, s: &str) {
            b.push(0x0d);
            b.push(s.len() as u8);
            b.extend_from_slice(s.as_bytes());
        }

        for ref_width in [
            crate::kernel_header::RefWidth::Four,
            crate::kernel_header::RefWidth::Eight,
        ] {
            // The last four payload bytes spell a definition opening. Only a walker
            // that consumes the length prefix at `ref_width` steps past them.
            let payload = [b'0', b'1', b'2', b'3', 0x0f, 0x0d, 0x01, b'x'];

            let mut active = Vec::new();
            t_ident(&mut active, "tspl");
            active.push(0x09);
            active.extend_from_slice(&payload.len().to_le_bytes()[..ref_width.bytes()]);
            active.extend_from_slice(&payload);
            let definition = active.len();
            active.extend_from_slice(b"\x0f\x0d\x08real_def\x10");
            active.push(0x11);

            let tables = crate::nurbs::subtypes::SubtypeTables::from_stream(&active);
            assert_eq!(tables.for_width(ref_width), [definition]);
        }
    }
}
