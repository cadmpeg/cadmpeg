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
use crate::nurbs::reader::{finish_knot_layout, BsplineMarker, Nullable};
use crate::sab::Token;
use cadmpeg_core::decode::admission::Admission;

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
    pub(super) fn take_float_array(
        &mut self,
        ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    ) -> Option<Result<Vec<f64>, cadmpeg_core::CodecError>> {
        if let Some(refusal) = ctx.resource_refusal() {
            return Some(Err(refusal.into()));
        }
        let mark = self.pos;
        let Some(count) = self.take_long().and_then(|c| usize::try_from(c).ok()) else {
            self.pos = mark;
            return None;
        };
        let mut scratch = match ctx.reserve_scoped(0, "ASM counted float array scratch") {
            Ok(scratch) => scratch,
            Err(error) => return Some(Err(error)),
        };
        let mut values =
            match scratch.with_storage(|| ctx.collection_vec(count, "ASM counted float array")) {
                Ok(values) => values,
                Err(error) => return Some(Err(error)),
            };
        let parsed = ctx.find_map(
            0..count,
            |_| {
                let Some(value) = self.take_f64() else {
                    self.pos = mark;
                    return Ok(Some(()));
                };
                values.push(value);
                Ok(None)
            },
            "scan ASM counted float array",
        );
        match parsed {
            Err(error) => Some(Err(error)),
            Ok(Some(())) => None,
            Ok(None) => Some(ctx.collect_retained_vec(values, "retain ASM counted float array")),
        }
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
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    cur: &mut Cur<'_>,
    n: usize,
    degree: i64,
) -> Option<Result<(Vec<f64>, usize), cadmpeg_core::CodecError>> {
    let mut scratch = match ctx.reserve_scoped(0, "ASM knot expansion inputs") {
        Ok(scratch) => scratch,
        Err(error) => return Some(Err(error)),
    };
    let mut values = match scratch.with_storage(|| ctx.collection_vec(n, "ASM unique knot values"))
    {
        Ok(values) => values,
        Err(error) => return Some(Err(error)),
    };
    let mut mults = match scratch.with_storage(|| ctx.collection_vec(n, "ASM knot multiplicities"))
    {
        Ok(mults) => mults,
        Err(error) => return Some(Err(error)),
    };
    let mut sum = Some(0usize);
    let mut expanded_len = Some(0usize);
    let parsed = ctx.find_map(
        0..n,
        |index| {
            let Some(value) = cur.take_f64() else {
                return Ok(Some(()));
            };
            let Some(multiplicity) = cur.take_long() else {
                return Ok(Some(()));
            };
            let count = usize::try_from(multiplicity).ok();
            sum = sum.and_then(|sum| sum.checked_add(count?));
            expanded_len = expanded_len.and_then(|length| {
                length.checked_add(count?.checked_add(usize::from(index == 0 || index + 1 == n))?)
            });
            values.push(value);
            mults.push(multiplicity);
            Ok(None)
        },
        "scan ASM knot pairs",
    );
    match parsed {
        Err(error) => return Some(Err(error)),
        Ok(Some(())) => return None,
        Ok(None) => {}
    }
    let expansion = finish_knot_layout(sum?, expanded_len?, degree)?;
    let mut expanded = match ctx.collection_vec(expansion.expanded_len(), "ASM expanded knots") {
        Ok(expanded) => expanded,
        Err(error) => return Some(Err(error)),
    };
    let values = match ctx.admit_iter(&values, "expand ASM unique knot values") {
        Ok(values) => values,
        Err(error) => return Some(Err(error.into())),
    };
    let multiplicities = match ctx.admit_iter(&mults, "expand ASM knot multiplicities") {
        Ok(values) => values,
        Err(error) => return Some(Err(error.into())),
    };
    for (index, (value, multiplicity)) in values.zip(multiplicities).enumerate() {
        let run_length = usize::try_from(*multiplicity).ok()?
            + usize::from(index == 0 || index + 1 == mults.len());
        let runs = match ctx.admit_iter(0..run_length, "write ASM expanded knots") {
            Ok(runs) => runs,
            Err(error) => return Some(Err(error.into())),
        };
        for _ in runs {
            expanded.push(*value);
        }
    }
    Some(Ok((expanded, expansion.n_poles)))
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
pub(super) fn owned_marker_positions(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    toks: &[Token],
) -> Option<Result<Vec<usize>, cadmpeg_core::CodecError>> {
    let (out, balanced) = match walk_owned_markers(ctx, toks) {
        Ok(value) => value,
        Err(error) => return Some(Err(error)),
    };
    balanced.then_some(Ok(out))
}

/// The owned-marker walk, with the balance it observed.
///
/// The second element is `false` when the walk met a `SubtypeClose` that no
/// open in `toks` matches. A balanced stream always answers `true`, so a caller
/// holding a [`SubtypeScope`] reads the first element alone.
fn walk_owned_markers(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    toks: &[Token],
) -> Result<(Vec<usize>, bool), cadmpeg_core::CodecError> {
    let mut out = Vec::new();
    let mut depth = 0usize;
    // The span's own leading `SubtypeOpen` is skipped, so the close that
    // matches it is the one close this walk admits at depth zero.
    let mut outer = usize::from(matches!(toks.first(), Some(Token::SubtypeOpen)));
    let start = outer;
    let malformed = ctx.find_map(
        toks[start..].iter().enumerate(),
        |(index, token)| {
            let pos = start + index;
            match token {
                Token::SubtypeOpen => depth += 1,
                Token::SubtypeClose => match depth.checked_sub(1) {
                    Some(next) => depth = next,
                    None => match outer.checked_sub(1) {
                        Some(next) => outer = next,
                        None => return Ok(Some(false)),
                    },
                },
                _ => {
                    if depth == 0 && marker_at(toks, pos).is_some() {
                        ctx.push_vec(&mut out, pos, "ASM owned spline markers")?;
                    }
                }
            }
            Ok(None)
        },
        "scan ASM owned spline markers",
    )?;
    Ok((out, malformed.is_none()))
}

/// Token index of the first subtype definition `toks` owns whose name matches
/// one of `names`, with the matched name. Names are tried in order; the first
/// name with a hit wins.
///
/// A construction owns a record through its own definition. Nested definitions
/// belong to their enclosing construction, so this function ignores matching
/// markers in nested scopes.
pub(super) fn find_owned_subtype_marker<'n>(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    toks: &[Token],
    names: &[&'n str],
) -> Option<Result<(usize, &'n str), cadmpeg_core::CodecError>> {
    if let Some(refusal) = ctx.resource_refusal() {
        return Some(Err(refusal.into()));
    }
    if names.is_empty() {
        return None;
    }
    let mut depth = 0usize;
    let mut best = None;
    let walked = ctx.find_map(
        toks.iter().enumerate(),
        |(pos, token)| {
            match token {
                Token::SubtypeOpen => {
                    if depth == 0 && best.is_none_or(|(rank, _, _)| rank != 0) {
                        let priorities = &names[..best.map_or(names.len(), |(rank, _, _)| rank)];
                        if let Some(Token::Ident(name) | Token::SubIdent(name)) = toks.get(pos + 1)
                        {
                            if let Some(rank) = ctx.position_by(
                                priorities,
                                |candidate| {
                                    ctx.equal_bytes(
                                        candidate.as_bytes(),
                                        name.as_bytes(),
                                        "match ASM construction name",
                                    )
                                },
                                "find ASM construction priority",
                            )? {
                                best = Some((rank, pos, names[rank]));
                            }
                        }
                    }
                    depth += 1;
                }
                Token::SubtypeClose => {
                    let Some(next) = depth.checked_sub(1) else {
                        return Ok(Some(false));
                    };
                    depth = next;
                }
                _ => {}
            }
            Ok(None)
        },
        "scan ASM owned construction markers",
    );
    match walked {
        Err(error) => Some(Err(error)),
        Ok(Some(false)) => None,
        Ok(_) => best.map(|(_, pos, name)| Ok((pos, name))),
    }
}

/// The construction `toks` is, under its modern name: the first subtype
/// definition `toks` owns other than `ref`, canonicalized.
pub fn owned_construction_subtype<'tokens>(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    toks: &'tokens [Token],
) -> Option<Result<&'tokens str, cadmpeg_core::CodecError>> {
    let mut depth = 0usize;
    let mut first = None;
    let walked = ctx.find_map(
        toks.iter().enumerate(),
        |(pos, token)| {
            match token {
                Token::SubtypeOpen => {
                    if depth == 0 && first.is_none() {
                        if let Some(Token::Ident(name) | Token::SubIdent(name)) = toks.get(pos + 1)
                        {
                            if name != "ref" {
                                first = Some(name.as_str());
                            }
                        }
                    }
                    depth += 1;
                }
                Token::SubtypeClose => {
                    let Some(next) = depth.checked_sub(1) else {
                        return Ok(Some(false));
                    };
                    depth = next;
                }
                _ => {}
            }
            Ok(None)
        },
        "scan ASM construction name",
    );
    match walked {
        Err(error) => Some(Err(error)),
        Ok(Some(false)) => None,
        Ok(_) => first.map(|name| Ok(canonical_intcurve_kind(name))),
    }
}

/// The record's unique directly owned cache-bearing construction scope.
/// Nested construction blocks belong to their own owners. More than one
/// cache-bearing outer scope is ambiguous. A record with no non-reference
/// construction owns its complete payload as its cache span. An unmatched
/// closing delimiter refuses the record.
pub(super) fn cache_scope<'a>(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    toks: &'a [Token],
) -> Option<Result<&'a [Token], cadmpeg_core::CodecError>> {
    let mut depth = 0usize;
    let mut constructions = 0usize;
    let mut start = 0usize;
    let mut construction = false;
    let mut marker = false;
    let mut cache = None;
    let mut caches = 0usize;
    let walked = ctx.find_map(toks.iter().enumerate(), |(pos, token)| {
        match token {
            Token::SubtypeOpen => {
                if depth == 0 {
                    start = pos;
                    construction = matches!(toks.get(pos + 1), Some(Token::Ident(name) | Token::SubIdent(name)) if name != "ref");
                    constructions += usize::from(construction);
                    marker = false;
                }
                depth += 1;
            }
            Token::SubtypeClose => {
                let Some(next) = depth.checked_sub(1) else { return Ok(Some(false)); };
                depth = next;
                if depth == 0 && construction && marker {
                    caches += 1;
                    cache = Some(&toks[start..=pos]);
                }
            }
            _ if depth == 1 && construction && !marker => marker = marker_at(toks, pos).is_some(),
            _ => {}
        }
        Ok(None)
    }, "scan ASM cache ownership");
    match walked {
        Err(error) => Some(Err(error)),
        Ok(Some(false)) => None,
        Ok(_) => match (caches, constructions) {
            (1, _) => cache.map(Ok),
            (0, 0) => Some(Ok(toks)),
            _ => None,
        },
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
pub(super) fn find_owned_intcurve_subtype(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    toks: &[Token],
    modern: &str,
) -> Option<Result<usize, cadmpeg_core::CodecError>> {
    if let Some(refusal) = ctx.resource_refusal() {
        return Some(Err(refusal.into()));
    }
    if modern.is_empty() {
        return None;
    }
    let legacy = super::subtypes::INTCURVE_ALIASES
        .iter()
        .find_map(|(name, alias)| (*name == modern).then_some(*alias));
    let found = match legacy {
        Some(legacy) => find_owned_subtype_marker(ctx, toks, &[modern, legacy]),
        None => find_owned_subtype_marker(ctx, toks, &[modern]),
    };
    found.map(|result| result.map(|(marker, _)| marker))
}

/// A balanced subtype scope in token space.
///
/// The scanners and `SubtypeTable` construct spans only from matching opening
/// and closing delimiters. The field is private and has no conversion or
/// dereference interface that can bypass that invariant.
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
    /// Both constructors include the opening delimiter and its matching close,
    /// so this interior slice is in range.
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
    /// The walk skips the scope's own opening token and counts the close that
    /// matches it at depth zero. Nested constructions retain their own depth.
    ///
    /// Total: the unbalanced stream that `owned_marker_positions` refuses is
    /// a state this type cannot hold.
    pub fn owned_marker_positions(
        &self,
        ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    ) -> Result<Vec<usize>, cadmpeg_core::CodecError> {
        Ok(walk_owned_markers(ctx, self.tokens)?.0)
    }
}

/// The balanced subtype scope opening at `start`, inclusive of both delimiters.
///
/// `None` unless the token at `start` is a `SubtypeOpen`. A scope opens at its
/// own opening delimiter, so a `start` that names another token names no
/// scope, and the span is then at least two tokens. An open scope is scanned
/// through its matching close, with work admitted before each token read.
/// Returns a resource refusal if the scan exceeds the context's work budget.
pub(super) fn subtype_span<'a>(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    toks: &'a [Token],
    start: usize,
) -> Result<Option<SubtypeScope<'a>>, cadmpeg_core::CodecError> {
    if !matches!(toks.get(start), Some(Token::SubtypeOpen)) {
        return match Admission::resource_refusal(ctx) {
            Some(refusal) => Err(refusal.into()),
            None => Ok(None),
        };
    }
    let mut depth = 0usize;
    let mut tokens = toks[start..].iter();
    for pos in start..toks.len() {
        let Some(token) = ctx.next_charged(&mut tokens, "ASM subtype span token scan")? else {
            return Ok(None);
        };
        match token {
            Token::SubtypeOpen => depth += 1,
            Token::SubtypeClose => {
                let Some(next_depth) = depth.checked_sub(1) else {
                    return Ok(None);
                };
                depth = next_depth;
                if depth == 0 {
                    // `pos > start`: reaching depth one needs a `SubtypeOpen`
                    // at or after `start`, so the close that returns depth to
                    // zero is never the token at `start` itself. Both slices
                    // are therefore in range.
                    let Some(tokens) = toks.get(start..=pos) else {
                        return Ok(None);
                    };
                    return Ok(Some(SubtypeScope { tokens }));
                }
            }
            _ => {}
        }
    }
    Ok(None)
}

/// The subtype scope at payload chunk `chunk_index` when its immediately
/// following identifier is `expected`.
///
/// The scope carries its own balance proof, so a caller that walks it needs no
/// walk of its own to establish one. The identifier this function matched is
/// the first token of [`SubtypeScope::interior`]. The chunk search and scope
/// scan admit work before each token read. Production callers pass one of the
/// fixed names `exp_par_cur` and `ref`, so the name comparison is bounded.
pub fn payload_subtype_toks<'r>(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    record: &'r crate::sab::Record,
    chunk_index: usize,
    expected: &str,
) -> Result<Option<SubtypeScope<'r>>, cadmpeg_core::CodecError> {
    if let Some(refusal) = Admission::resource_refusal(ctx) {
        return Err(refusal.into());
    }
    let mut chunk = 0usize;
    let mut open = None;
    let mut tokens = record.tokens.iter();
    for pos in 0..record.tokens.len() {
        let Some(token) = ctx.next_charged(&mut tokens, "ASM payload subtype token scan")? else {
            return Ok(None);
        };
        if token.is_payload_ident() {
            continue;
        }
        if chunk == chunk_index {
            if !matches!(token, Token::SubtypeOpen) {
                return Ok(None);
            }
            open = Some(pos);
            break;
        }
        chunk += 1;
    }
    let Some(open) = open else {
        return Ok(None);
    };
    let Some(_name_pos) = open.checked_add(1).filter(|pos| *pos < record.tokens.len()) else {
        return Ok(None);
    };
    let Some(Token::Ident(name) | Token::SubIdent(name)) =
        ctx.next_charged(&mut tokens, "ASM payload subtype token scan")?
    else {
        return Ok(None);
    };
    if name != expected {
        return Ok(None);
    }
    subtype_span(ctx, &record.tokens, open)
}

/// Token positions of the stream's subtype definitions, in stream order.
///
/// A subtype definition opens as `SubtypeOpen` followed by an identifier other
/// than `ref`, at any nesting depth; `{ref N}` references resolve to the `N`-th
/// entry. Each entry holds the owning record's shared payload tokens and the
/// definition's token index within them, so resolution needs no side channel
/// back to the record table.
pub struct SubtypeTable {
    defs: Vec<SubtypeDefinition>,
    /// The stream's ASM save format version from the `asmheader` record. Tokens
    /// omit this value, so the table carries it into token-space decoders.
    save_format_version: Option<u32>,
}

struct SubtypeDefinition {
    tokens: std::sync::Arc<[Token]>,
    start: usize,
    end: Option<usize>,
}

impl SubtypeTable {
    /// Build the table over each framed record's payload tokens, in order.
    pub fn from_records(
        ctx: &cadmpeg_core::decode::DecodeContext<'_>,
        records: &[crate::sab::Record],
    ) -> Result<Self, cadmpeg_core::CodecError> {
        if let Some(refusal) = ctx.resource_refusal() {
            return Err(cadmpeg_core::CodecError::from(refusal));
        }
        let mut defs: Vec<SubtypeDefinition> = Vec::new();
        let mut source_index_entries = records.iter();
        while source_index_entries.len() != 0 {
            let Some(record) =
                ctx.next_charged(&mut source_index_entries, "index ASM subtype records")?
            else {
                break;
            };
            let mut scratch = ctx.reserve_scoped(0, "index ASM subtype boundaries")?;
            let mut stack = Vec::new();
            let mut source_index_entries = record.tokens.as_ref().iter().enumerate();
            while source_index_entries.len() != 0 {
                let Some((pos, token)) =
                    ctx.next_charged(&mut source_index_entries, "index ASM subtype tokens")?
                else {
                    break;
                };
                match token {
                    Token::SubtypeOpen => {
                        let definition = match record.tokens.get(pos + 1) {
                            Some(Token::Ident(name) | Token::SubIdent(name)) if name != "ref" => {
                                let index = defs.len();
                                ctx.push_vec(
                                    &mut defs,
                                    SubtypeDefinition {
                                        tokens: record.tokens.clone(),
                                        start: pos,
                                        end: None,
                                    },
                                    "index ASM subtype definitions",
                                )?;
                                Some(index)
                            }
                            _ => None,
                        };
                        ctx.push_scoped_vec(
                            &mut scratch,
                            &mut stack,
                            definition,
                            "index ASM subtype boundary stack",
                        )?;
                    }
                    Token::SubtypeClose => {
                        if let Some(Some(index)) = stack.pop() {
                            defs[index].end = Some(pos + 1);
                        }
                    }
                    _ => {}
                }
            }
        }
        Ok(Self {
            defs,
            save_format_version: None,
        })
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
        let definition = self.defs.get(index)?;
        Some(SubtypeScope {
            tokens: &definition.tokens[definition.start..definition.end?],
        })
    }
}

/// Admit every subtype-reference walk before semantic candidates read the
/// table. The stack holds policy depth guards for the active reference path.
pub(crate) fn admit_subtype_references(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    records: &[crate::sab::Record],
    table: &SubtypeTable,
) -> Result<(), cadmpeg_core::CodecError> {
    if let Some(refusal) = ctx.resource_refusal() {
        return Err(refusal.into());
    }
    let mut source_values = IntoIterator::into_iter(records);
    while source_values.len() != 0 {
        let Some(record) = ctx.next_charged(&mut source_values, "walk ASM subtype records")? else {
            break;
        };
        let mut scratch = ctx.reserve_scoped(0, "walk ASM subtype references")?;
        let mut visited = std::collections::BTreeSet::new();
        let mut pending = Vec::new();
        let root = (record.tokens.as_ref(), 0usize, None);
        ctx.push_scoped_vec(&mut scratch, &mut pending, root, "walk ASM subtype stack")?;
        while let Some((tokens, position, _guard)) = pending.last_mut() {
            if *position < tokens.len() {
                ctx.charge_work(1, "scan ASM subtype references")?;
            }
            let Some(token) = tokens.get(*position) else {
                pending.pop();
                continue;
            };
            let pos = *position;
            *position += 1;
            if !matches!(token, Token::SubtypeOpen) {
                continue;
            }
            let index = match (tokens.get(pos + 1), tokens.get(pos + 2)) {
                (Some(Token::Ident(name)), Some(Token::Long(index)))
                    if name == "ref" && *index >= 0 =>
                {
                    usize::try_from(*index).ok()
                }
                (Some(Token::Long(index)), Some(Token::SubtypeClose)) if *index >= 0 => {
                    usize::try_from(*index).ok()
                }
                _ => None,
            };
            let Some(index) = index else {
                continue;
            };
            if !ctx.insert_scoped_btree_value(
                &mut scratch,
                &mut visited,
                index,
                "visit ASM subtype reference",
            )? {
                continue;
            }
            let Some(target) = table.span(index) else {
                continue;
            };
            let guard = ctx.enter_nested("follow ASM subtype reference")?;
            let frame = (target.tokens(), 0usize, Some(guard));
            ctx.push_scoped_vec(&mut scratch, &mut pending, frame, "walk ASM subtype stack")?;
        }
    }
    Ok(())
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
    let records = crate::test_support::sab::frame(&wrapped, 0, wrapped.len(), ref_width)?;
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
    let record = crate::test_support::sab::record(
0,
String::new(),
lex_test_span(bytes, ref_width)?,
0,
0
);
    let arena = cadmpeg_core::decode::DecodeArena::new();
    let policy = cadmpeg_core::decode::DecodePolicy::service();
    let (ctx, _) = cadmpeg_core::decode::DecodeContext::from_root_bytes(&[], &arena, &policy)
        .map_err(|error| crate::stream_error::StreamError {
            format: crate::stream_error::StreamFormat::Binary,
            offset: 0,
            reason: error.to_string(),
        })?;
    SubtypeTable::from_records(&ctx, &[record]).map_err(|error| crate::stream_error::StreamError {
        format: crate::stream_error::StreamFormat::Binary,
        offset: 0,
        reason: error.to_string(),
    })
}

#[cfg(test)]
mod tests {
    mod index_sources;
    mod entry_routes;
    use super::{
        cache_scope as cache_scope_ctx, lex_test_span, marker_at,
        owned_construction_subtype as owned_construction_subtype_ctx,
        owned_marker_positions as owned_marker_positions_ctx,
        subtype_span as subtype_span_ctx, test_table, Cur,
    };
    use crate::kernel_header::RefWidth;
    use crate::nurbs::reader::BsplineMarker;
    use crate::sab::Token;

    fn with_ctx<T>(f: impl FnOnce(&cadmpeg_core::decode::DecodeContext<'_>) -> T) -> T {
        let arena = cadmpeg_core::decode::DecodeArena::new();
        let policy = cadmpeg_core::decode::DecodePolicy::service();
        let (ctx, _) =
            cadmpeg_core::decode::DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
        f(&ctx)
    }

    fn subtype_span<'a>(toks: &'a [Token], start: usize) -> Option<super::SubtypeScope<'a>> {
        with_ctx(|ctx| subtype_span_ctx(ctx, toks, start).expect("decode work admission"))
    }

    fn owned_marker_positions(toks: &[Token]) -> Option<Vec<usize>> {
        with_ctx(|ctx| owned_marker_positions_ctx(ctx, toks).transpose().unwrap())
    }

    fn owned_construction_subtype(toks: &[Token]) -> Option<&str> {
        with_ctx(|ctx| {
            owned_construction_subtype_ctx(ctx, toks)
                .transpose()
                .unwrap()
        })
    }

    #[test]
    fn construction_name_inspection_borrows_with_zero_storage_and_preserves_scan_refusal() {
        use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
        use cadmpeg_core::CodecError;
        for (name, canonical) in [("exactcur", "exact_int_cur"), ("arbitrary", "arbitrary")] {
            let tokens = [Token::SubtypeOpen, Token::Ident(name.into()), Token::SubtypeClose];
            // Three tokens plus the original end probe. Inspection owns no backing.
            for cap in [3, 4] {
                let arena = DecodeArena::new();
                let mut policy = DecodePolicy::service();
                policy.limits.max_work_units = cap;
                policy.limits.max_materialized_bytes = 0;
                policy.limits.max_retained_bytes = 0;
                policy.limits.max_collection_items = 0;
                let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
                let result = owned_construction_subtype_ctx(&ctx, &tokens).unwrap();
                if cap == 3 {
                    let Err(CodecError::ResourceLimit(first)) = result else {
                        panic!("expected original construction-name end-probe refusal");
                    };
                    assert_eq!(first.dimension, ResourceDimension::WorkUnits);
                    assert_eq!(first.operation, "scan ASM construction name");
                    assert_eq!((first.limit, first.used, first.additional), (3, 3, 1));
                    assert!(matches!(ctx.finish_session(), Err(CodecError::ResourceLimit(last)) if last == first));
                } else {
                    let selected = result.unwrap();
                    assert_eq!(selected, canonical);
                    if name == "arbitrary" {
                        assert!(matches!(&tokens[1], Token::Ident(original)
                            if selected.as_ptr() == original.as_ptr()));
                    }
                    ctx.finish_session().unwrap();
                }
            }
        }
    }

    fn cache_scope(toks: &[Token]) -> Option<&[Token]> {
        with_ctx(|ctx| cache_scope_ctx(ctx, toks).transpose().unwrap())
    }

    #[test]
    fn owned_marker_vector_refuses_collection_limit() {
        use cadmpeg_core::decode::ResourceDimension;
        let tokens = [
            Token::SubtypeOpen,
            Token::Ident("nubs".into()),
            Token::SubtypeClose,
        ];
        let limit = crate::test_support::resource_limit_at(
            &[],
            ResourceDimension::CollectionItems,
            "ASM owned spline markers",
            |ctx| owned_marker_positions_ctx(ctx, &tokens).expect("owned markers"),
        );
        assert_eq!(limit.dimension, ResourceDimension::CollectionItems);
        assert_eq!(limit.operation, "ASM owned spline markers");
    }

    #[test]
    fn subtype_table_vector_refuses_collection_limit() {
        use cadmpeg_core::decode::ResourceDimension;
        let record = crate::test_support::sab::record(
0,
"spline".into(),
vec![
                Token::SubtypeOpen,
                Token::Ident("exactcur".into()),
                Token::SubtypeClose,
            ]
            .into(),
0,
0
);
        let limit = crate::test_support::resource_limit_at(
            &[],
            ResourceDimension::CollectionItems,
            "index ASM subtype definitions",
            |ctx| super::SubtypeTable::from_records(ctx, std::slice::from_ref(&record)),
        );
        assert_eq!(limit.dimension, ResourceDimension::CollectionItems);
        assert_eq!(limit.operation, "index ASM subtype definitions");
    }

    #[test]
    fn subtype_reference_walk_refuses_depth_before_following_next_definition() {
        use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
        use cadmpeg_core::CodecError;

        let records: Vec<crate::sab::Record> = (0..3)
            .map(|index| {
                let mut tokens = vec![Token::SubtypeOpen, Token::Ident("node".into())];
                if index < 2 {
                    tokens.extend([
                        Token::SubtypeOpen,
                        Token::Ident("ref".into()),
                        Token::Long(index + 1),
                        Token::SubtypeClose,
                    ]);
                }
                tokens.push(Token::SubtypeClose);
                crate::test_support::sab::record(
usize::try_from(index).expect("test value fits"),
"node".into(),
tokens.into(),
0,
0
)
            })
            .collect();
        let table = with_ctx(|ctx| super::SubtypeTable::from_records(ctx, &records).unwrap());
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        policy.limits.max_recursion_depth = 1;
        let (ctx, _) = DecodeContext::from_root_bytes(&[0], &arena, &policy).unwrap();
        let error = super::admit_subtype_references(&ctx, &records, &table)
            .expect_err("second followed reference exceeds depth one");
        let CodecError::ResourceLimit(limit) = error else {
            panic!("expected resource refusal, got {error:?}");
        };
        assert_eq!(limit.dimension, ResourceDimension::RecursionDepth);
        assert_eq!(limit.operation, "follow ASM subtype reference");

        let arena = DecodeArena::new();
        let policy = DecodePolicy::service();
        let (ctx, _) = DecodeContext::from_root_bytes(&[0], &arena, &policy).unwrap();
        super::admit_subtype_references(&ctx, &records, &table)
            .expect("service profile admits finite chain");
    }

    #[test]
    fn subtype_reference_walk_refuses_work_before_following_reference() {
        use cadmpeg_core::decode::ResourceDimension;

        let tokens: std::sync::Arc<[Token]> = vec![
            Token::SubtypeOpen,
            Token::Ident("node".into()),
            Token::SubtypeOpen,
            Token::Ident("ref".into()),
            Token::Long(0),
            Token::SubtypeClose,
            Token::SubtypeClose,
        ]
        .into();
        let record = crate::test_support::sab::record(
0,
"node".into(),
tokens,
0,
0
);
        let records = [record];
        let table = with_ctx(|ctx| super::SubtypeTable::from_records(ctx, &records).unwrap());
        let limit = crate::test_support::resource_limit_at(
            &[0],
            ResourceDimension::WorkUnits,
            "scan ASM subtype references",
            |ctx| super::admit_subtype_references(ctx, &records, &table),
        );
        assert_eq!(limit.dimension, ResourceDimension::WorkUnits);
        assert_eq!(limit.operation, "scan ASM subtype references");
    }

    #[test]
    fn indexed_spans_preserve_nested_and_incomplete_definition_boundaries() {
        let tokens = vec![
            Token::SubtypeClose,
            Token::SubtypeOpen,
            ident("outer"),
            Token::SubtypeOpen,
            ident("child"),
            Token::SubtypeClose,
            Token::SubtypeOpen,
            ident("ref"),
            Token::Long(1),
            Token::SubtypeClose,
        ];
        let records = [crate::test_support::sab::record(
0,
"spline".into(),
tokens.clone().into(),
0,
0
)];
        let table = with_ctx(|ctx| super::SubtypeTable::from_records(ctx, &records).unwrap());
        assert!(table.span(0).is_none());
        assert_eq!(table.span(1).unwrap().tokens(), &tokens[3..6]);
        assert!(table.span(2).is_none());
        let mut complete = tokens;
        complete.push(Token::SubtypeClose);
        let records = [crate::test_support::sab::record(
0,
"spline".into(),
complete.clone().into(),
0,
0
)];
        let table = with_ctx(|ctx| super::SubtypeTable::from_records(ctx, &records).unwrap());
        assert_eq!(table.span(0).unwrap().tokens(), &complete[1..]);
        assert_eq!(table.span(1).unwrap().interior(), &[ident("child")]);
    }

    #[test]
    fn an_empty_construction_name_query_needs_no_token_walk() {
        use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy};
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        policy.limits.max_work_units = 0;
        let ctx = DecodeContext::new(&arena, &policy, false);
        let tokens = [Token::SubtypeOpen, ident("exactcur"), Token::SubtypeClose];
        assert!(super::find_owned_subtype_marker(&ctx, &tokens, &[]).is_none());
    }

    #[test]
    fn malformed_ownership_scan_stops_before_unvisited_tail() {
        use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy};
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        policy.limits.max_work_units = 2;
        let ctx = DecodeContext::new(&arena, &policy, false);
        let mut tokens = vec![Token::SubtypeClose];
        tokens.extend((0..10_000).map(|_| Token::True));
        assert!(cache_scope_ctx(&ctx, &tokens).is_none());
    }

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
        let arena = cadmpeg_core::decode::DecodeArena::new();
        let (ctx, _) = cadmpeg_core::decode::DecodeContext::from_root_bytes(
            &[],
            &arena,
            &cadmpeg_core::decode::DecodePolicy::default(),
        )
        .expect("test decode context");
        let toks = [Token::Long(2), Token::Double(1.0), Token::True];
        let mut cur = Cur::at(&toks, 0);
        assert_eq!(
            cur.take_float_array(&ctx)
                .transpose()
                .expect("resource allocation"),
            None
        );
        assert_eq!(cur.pos(), 0);
    }

    #[test]
    fn truncated_counted_float_array_uses_only_scratch_storage() {
        use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy};
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        policy.limits.max_retained_bytes = 0;
        let ctx = DecodeContext::new(&arena, &policy, false);
        let toks = [Token::Long(2), Token::Double(1.0), Token::True];
        let mut cur = Cur::at(&toks, 0);
        assert!(cur.take_float_array(&ctx).is_none());
        assert_eq!(cur.pos(), 0);
    }

    #[test]
    fn counted_float_array_refuses_collection_limit_before_reading_values() {
        use cadmpeg_core::decode::ResourceDimension;
        let toks = [Token::Long(2), Token::Double(1.0), Token::Double(2.0)];
        let mut position = 0;
        let limit = crate::test_support::resource_limit_at(
            &[],
            ResourceDimension::CollectionItems,
            "ASM counted float array",
            |ctx| {
                let mut cur = Cur::at(&toks, 0);
                let result = cur.take_float_array(ctx).expect("counted float array");
                position = cur.pos();
                result
            },
        );
        assert_eq!(limit.dimension, ResourceDimension::CollectionItems);
        assert_eq!(position, 1);
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
        assert_eq!(
            owned_construction_subtype(&toks),
            Some("exact_int_cur")
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
        let owned: Vec<usize> = with_ctx(|ctx| scope.owned_marker_positions(ctx).unwrap());
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
    fn subtype_span_charges_only_the_balanced_prefix_and_fuses_its_first_refusal() {
        use cadmpeg_core::decode::{DecodeArena, DecodePolicy, ResourceDimension};

        let mut toks = vec![
            Token::SubtypeOpen,
            ident("exactcur"),
            Token::SubtypeOpen,
            ident("ref"),
            Token::Long(3),
            Token::SubtypeClose,
            Token::SubtypeClose,
        ];
        toks.extend((0..256).map(|_| Token::Double(0.0)));

        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        policy.limits.max_work_units = 7;
        let (ctx, _) = cadmpeg_core::decode::DecodeContext::from_root_bytes(&[], &arena, &policy)
            .expect("test decode context");
        let scope = subtype_span_ctx(&ctx, &toks, 0)
            .expect("scan work admission")
            .expect("balanced prefix");
        assert_eq!(scope.tokens(), &toks[..7]);
        ctx.finish_session().expect("the unused tail costs no work");

        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        policy.limits.max_work_units = 6;
        let (ctx, _) = cadmpeg_core::decode::DecodeContext::from_root_bytes(&[], &arena, &policy)
            .expect("test decode context");
        let error = subtype_span_ctx(&ctx, &toks, 0).expect_err("the closing token is admitted");
        let cadmpeg_core::CodecError::ResourceLimit(limit) = error else {
            panic!("expected work refusal: {error:?}");
        };
        assert_eq!(limit.dimension, ResourceDimension::WorkUnits);
        assert_eq!(limit.operation, "ASM subtype span token scan");
        assert_eq!(limit.used, 6);
        assert_eq!(limit.additional, 1);
        assert!(matches!(
            ctx.finish_session(),
            Err(cadmpeg_core::CodecError::ResourceLimit(actual)) if actual == limit
        ));
    }

    #[test]
    fn subtype_span_stops_at_an_unclosed_end_without_an_eof_charge() {
        use cadmpeg_core::decode::{DecodeArena, DecodePolicy, ResourceDimension};

        let toks = [
            Token::SubtypeOpen,
            ident("exactcur"),
            Token::SubtypeOpen,
            ident("ref"),
            Token::Long(3),
        ];
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        policy.limits.max_work_units =
            u64::try_from(toks.len()).expect("fixture length fits in u64");
        let (ctx, _) = cadmpeg_core::decode::DecodeContext::from_root_bytes(&[], &arena, &policy)
            .expect("test decode context");
        assert!(subtype_span_ctx(&ctx, &toks, 0)
            .expect("scan work admission")
            .is_none());
        ctx.finish_session().expect("there is no EOF probe");

        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        policy.limits.max_work_units =
            u64::try_from(toks.len() - 1).expect("fixture length fits in u64");
        let (ctx, _) = cadmpeg_core::decode::DecodeContext::from_root_bytes(&[], &arena, &policy)
            .expect("test decode context");
        let error = subtype_span_ctx(&ctx, &toks, 0).expect_err("the final token is admitted");
        let cadmpeg_core::CodecError::ResourceLimit(limit) = error else {
            panic!("expected work refusal: {error:?}");
        };
        assert_eq!(limit.dimension, ResourceDimension::WorkUnits);
        assert_eq!(limit.operation, "ASM subtype span token scan");
        assert_eq!(
            limit.used,
            u64::try_from(toks.len() - 1).expect("fixture length fits in u64")
        );
        assert_eq!(limit.additional, 1);
        assert!(matches!(
            ctx.finish_session(),
            Err(cadmpeg_core::CodecError::ResourceLimit(actual)) if actual == limit
        ));
    }

    #[test]
    fn subtype_span_no_open_is_fixed_work_and_does_not_hide_a_fused_refusal() {
        use cadmpeg_core::decode::{DecodeArena, DecodePolicy};

        let toks = [ident("x"), Token::SubtypeClose];
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        policy.limits.max_work_units = 0;
        let (ctx, _) = cadmpeg_core::decode::DecodeContext::from_root_bytes(&[], &arena, &policy)
            .expect("test decode context");
        assert!(subtype_span_ctx(&ctx, &toks, 0)
            .expect("no scan occurs")
            .is_none());
        ctx.finish_session().expect("no-open is a fixed O(1) check");

        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        policy.limits.max_work_units = 0;
        let (ctx, _) = cadmpeg_core::decode::DecodeContext::from_root_bytes(&[], &arena, &policy)
            .expect("test decode context");
        let error = ctx
            .charge_work(1, "ASM prior scan refusal")
            .expect_err("the first positive charge is refused");
        let cadmpeg_core::CodecError::ResourceLimit(limit) = error else {
            panic!("expected work refusal: {error:?}");
        };
        assert!(matches!(
            subtype_span_ctx(&ctx, &toks, 0),
            Err(cadmpeg_core::CodecError::ResourceLimit(actual)) if actual == limit
        ));
        assert!(matches!(
            ctx.finish_session(),
            Err(cadmpeg_core::CodecError::ResourceLimit(actual)) if actual == limit
        ));
    }

    #[test]
    fn payload_subtype_lookup_charges_payload_prefix_and_matching_scope() {
        use cadmpeg_core::decode::{DecodeArena, DecodePolicy, ResourceDimension};

        let mut tokens = vec![
            ident("preamble"),
            Token::Ref(-1),
            Token::Ref(-1),
            Token::Ref(-1),
            Token::Long(0),
            Token::True,
            Token::SubtypeOpen,
            ident("exp_par_cur"),
            Token::Long(3),
            Token::SubtypeClose,
        ];
        tokens.extend((0..256).map(|_| Token::Double(0.0)));
        let record = crate::test_support::sab::record(
0,
"pcurve".into(),
tokens.into(),
0,
0
);

        // Lookup scans the 7-token payload prefix, reads the matching name,
        // then validates the 4-token scope from its original base.
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        policy.limits.max_work_units = 12;
        let (ctx, _) = cadmpeg_core::decode::DecodeContext::from_root_bytes(&[], &arena, &policy)
            .expect("test decode context");
        let scope = super::payload_subtype_toks(&ctx, &record, 5, "exp_par_cur")
            .expect("lookup work admission")
            .expect("matching inline pcurve subtype");
        assert_eq!(scope.tokens(), &record.tokens[6..=9]);
        ctx.finish_session().expect("the trailing payload is unused");

        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        policy.limits.max_work_units = 8;
        let (ctx, _) = cadmpeg_core::decode::DecodeContext::from_root_bytes(&[], &arena, &policy)
            .expect("test decode context");
        assert!(super::payload_subtype_toks(&ctx, &record, 5, "different")
            .expect("lookup work admission")
            .is_none());
        ctx.finish_session().expect("name mismatch stops before the body");

        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        policy.limits.max_work_units = 11;
        let (ctx, _) = cadmpeg_core::decode::DecodeContext::from_root_bytes(&[], &arena, &policy)
            .expect("test decode context");
        let error = super::payload_subtype_toks(&ctx, &record, 5, "exp_par_cur")
            .expect_err("the matching close is admitted");
        let cadmpeg_core::CodecError::ResourceLimit(limit) = error else {
            panic!("expected work refusal: {error:?}");
        };
        assert_eq!(limit.dimension, ResourceDimension::WorkUnits);
        assert_eq!(limit.operation, "ASM subtype span token scan");
        assert_eq!(limit.used, 11);
        assert_eq!(limit.additional, 1);
        assert!(matches!(
            ctx.finish_session(),
            Err(cadmpeg_core::CodecError::ResourceLimit(actual)) if actual == limit
        ));
    }

    #[test]
    fn payload_subtype_lookup_preserves_a_fused_refusal_when_no_chunk_exists() {
        use cadmpeg_core::decode::{DecodeArena, DecodePolicy};

        let record = crate::test_support::sab::record(
0,
"pcurve".into(),
Vec::new().into(),
0,
0
);
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        policy.limits.max_work_units = 0;
        let (ctx, _) = cadmpeg_core::decode::DecodeContext::from_root_bytes(&[], &arena, &policy)
            .expect("test decode context");
        assert!(super::payload_subtype_toks(&ctx, &record, 0, "exp_par_cur")
            .expect("empty lookup does not scan")
            .is_none());
        ctx.finish_session().expect("empty lookup uses no work");

        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        policy.limits.max_work_units = 0;
        let (ctx, _) = cadmpeg_core::decode::DecodeContext::from_root_bytes(&[], &arena, &policy)
            .expect("test decode context");
        let error = ctx
            .charge_work(1, "ASM prior scan refusal")
            .expect_err("the first positive charge is refused");
        let cadmpeg_core::CodecError::ResourceLimit(limit) = error else {
            panic!("expected work refusal: {error:?}");
        };

        assert!(matches!(
            super::payload_subtype_toks(&ctx, &record, 0, "exp_par_cur"),
            Err(cadmpeg_core::CodecError::ResourceLimit(actual)) if actual == limit
        ));
        assert!(matches!(
            ctx.finish_session(),
            Err(cadmpeg_core::CodecError::ResourceLimit(actual)) if actual == limit
        ));
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
        assert_eq!(
            with_ctx(|ctx| scope.owned_marker_positions(ctx).unwrap()),
            vec![6]
        );
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
    }
    #[test]
    fn subtype_table_walks_wide_strings_at_the_stream_ref_width() {
        fn t_ident(b: &mut Vec<u8>, s: &str) {
            b.push(0x0d);
            b.push(u8::try_from(s.len()).expect("test value fits"));
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
            active.extend_from_slice(b"\x0f\x0d\x08real_def\x10");
            active.push(0x11);

            let records = crate::test_support::sab::frame(&active, 0, active.len(), ref_width)
                .expect("wide-string record frames at its declared width");
            let table = with_ctx(|ctx| super::SubtypeTable::from_records(ctx, &records).unwrap());
            assert_eq!(table.defs.len(), 1);
            assert!(matches!(table.span(0).expect("real definition").tokens(),
            [Token::SubtypeOpen, Token::Ident(name), Token::SubtypeClose] if name == "real_def"));
        }
    }
}
