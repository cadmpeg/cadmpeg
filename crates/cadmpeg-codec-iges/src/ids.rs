// SPDX-License-Identifier: Apache-2.0
//! The one route from decoded numbers to IR identities.
//!
//! An identity key must be non-empty and carry no `#` and no whitespace. A
//! [`Stem`] is built only from decoded numbers and the fixed [`Word`]s spelled
//! in this crate's source, so every key this module builds satisfies that
//! grammar and the minters below are total. No stem can be built from
//! file-derived text, so no decoded label can reach a minting failure.

use std::fmt;

use cadmpeg_core::decode::DecodeContext;
use cadmpeg_core::CodecError;
use cadmpeg_ir::ids::{
    AppearanceBindingId, AppearanceId, BodyId, CoedgeId, CurveId, EdgeId, FaceId, IdentityKey,
    LoopId, PcurveId, PointId, ProceduralCurveId, ProceduralSurfaceId, RegionId, ShellId,
    SurfaceId, VertexId,
};

/// Format a directory-derived lookup key in caller-owned stack storage.
pub(crate) fn directory_lookup_key<'a>(
    prefix: &str,
    sequence: u32,
    storage: &'a mut [u8],
    ctx: &DecodeContext<'_>,
) -> Result<Option<&'a str>, CodecError> {
    let mut digits = [0_u8; 10];
    let mut value = sequence;
    let mut start = digits.len();
    loop {
        start -= 1;
        let Ok(digit) = u8::try_from(value % 10) else {
            return Ok(None);
        };
        digits[start] = b'0' + digit;
        value /= 10;
        if value == 0 {
            break;
        }
    }
    let Some(length) = prefix.len().checked_add(digits.len() - start) else {
        return Ok(None);
    };
    let Some(result) = storage.get_mut(..length) else {
        return Ok(None);
    };
    result[..prefix.len()].copy_from_slice(prefix.as_bytes());
    result[prefix.len()..].copy_from_slice(&digits[start..]);
    Ok(ctx.validate_utf8(result, "iges directory lookup text")?.ok())
}

/// A decoded number an identity key may be spelled with.
pub(crate) trait Ordinal: Copy {
    /// The number retained without allocating its decimal spelling.
    fn piece(self) -> Piece;

    /// This number read as a Directory sequence, if it is one.
    fn sequence(self) -> Option<u32>;
}

impl Ordinal for u32 {
    fn piece(self) -> Piece {
        Piece::Unsigned(u64::from(self))
    }

    fn sequence(self) -> Option<u32> {
        Some(self)
    }
}

impl Ordinal for usize {
    fn piece(self) -> Piece {
        Piece::Index(self)
    }

    fn sequence(self) -> Option<u32> {
        u32::try_from(self).ok()
    }
}

impl Ordinal for i64 {
    fn piece(self) -> Piece {
        Piece::Signed(self)
    }

    fn sequence(self) -> Option<u32> {
        u32::try_from(self).ok()
    }
}

/// A fixed word this crate spells identity keys with.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Word {
    Body,
    BoundedPlane,
    End,
    Face,
    FreeGeometry,
    ImplicitOuter,
    LegacySingleParent,
    PlacedDirectrix,
    PlacedGeneratrix,
    PlacedSource,
    Start,
}

impl Word {
    /// The fixed spelling of this word.
    const fn text(self) -> &'static str {
        match self {
            Self::Body => "body",
            Self::BoundedPlane => "bounded-plane",
            Self::End => "end",
            Self::Face => "face",
            Self::FreeGeometry => "free-geometry",
            Self::ImplicitOuter => "implicit-outer",
            Self::LegacySingleParent => "legacy-single-parent",
            Self::PlacedDirectrix => "placed-directrix",
            Self::PlacedGeneratrix => "placed-generatrix",
            Self::PlacedSource => "placed-source",
            Self::Start => "start",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Piece {
    Text(&'static str),
    Unsigned(u64),
    Index(usize),
    Signed(i64),
}

impl fmt::Display for Piece {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Text(text) => formatter.write_str(text),
            Self::Unsigned(value) => value.fmt(formatter),
            Self::Index(value) => value.fmt(formatter),
            Self::Signed(value) => value.fmt(formatter),
        }
    }
}

const INLINE_PIECES: usize = 10;

#[derive(Debug, Clone, PartialEq, Eq)]
enum StemPieces {
    Inline {
        items: [Piece; INLINE_PIECES],
        len: usize,
    },
    Overflow(Vec<Piece>),
}

impl StemPieces {
    fn new(first: Piece) -> Self {
        let mut items = [Piece::Text(""); INLINE_PIECES];
        items[0] = first;
        Self::Inline { items, len: 1 }
    }

    fn push(&mut self, piece: Piece) {
        match self {
            Self::Inline { items, len } if *len < INLINE_PIECES => {
                items[*len] = piece;
                *len += 1;
            }
            Self::Inline { items, len } => {
                let mut overflow = Vec::from(&items[..*len]);
                overflow.push(piece);
                *self = Self::Overflow(overflow);
            }
            Self::Overflow(items) => items.push(piece),
        }
    }

    fn iter(&self) -> impl Iterator<Item = &Piece> {
        let slice: &[Piece] = match self {
            Self::Inline { items, len } => &items[..*len],
            Self::Overflow(items) => items,
        };
        slice.iter()
    }
}

/// An identity key built only from decoded numbers and [`Word`]s.
///
/// A key rooted at a Directory sequence carries that sequence as its origin,
/// and every derivation keeps it, so a reader that needs the entry a record
/// came from asks the stem instead of parsing the minted text.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Stem {
    pieces: StemPieces,
    origin: Option<u32>,
}

impl Stem {
    /// The key of one Directory entry: `D{sequence}`.
    pub(crate) fn directory(sequence: impl Ordinal) -> Self {
        let mut stem = Self {
            pieces: StemPieces::new(Piece::Text("D")),
            origin: sequence.sequence(),
        };
        stem.pieces.push(sequence.piece());
        stem
    }

    /// A key that is one fixed word.
    pub(crate) fn word(word: Word) -> Self {
        Self {
            pieces: StemPieces::new(Piece::Text(word.text())),
            origin: None,
        }
    }

    /// A fixed word qualified by a Directory sequence: `{word}-D{sequence}`.
    ///
    /// The key is rooted at the word, not at the sequence, so it has no origin:
    /// such a record is a derivation of the entry, not its whole neutral form.
    pub(crate) fn word_directory(word: Word, sequence: impl Ordinal) -> Self {
        let mut stem = Self::word(word);
        stem.pieces.push(Piece::Text("-D"));
        stem.pieces.push(sequence.piece());
        stem
    }

    /// A key that is one decoded number.
    pub(crate) fn number(value: impl Ordinal) -> Self {
        Self {
            pieces: StemPieces::new(value.piece()),
            origin: None,
        }
    }

    /// The Directory entry this key is rooted at, if it is rooted at one.
    pub(crate) const fn origin(&self) -> Option<u32> {
        self.origin
    }

    /// This stem's identity key.
    fn key(&self) -> IdentityKey {
        IdentityKey::encode_key_text(&self.to_string())
    }

    /// A child keyed by a Directory sequence: `{self}:D{sequence}`.
    pub(crate) fn child(&self, sequence: impl Ordinal) -> Self {
        self.derive(Piece::Text(":D"), sequence.piece())
    }

    /// A child keyed by an ordinal: `{self}:{index}`.
    pub(crate) fn slot(&self, index: impl Ordinal) -> Self {
        self.derive(Piece::Text(":"), index.piece())
    }

    /// A named part of this key: `{self}:{word}`.
    pub(crate) fn part(&self, word: Word) -> Self {
        self.derive(Piece::Text(":"), Piece::Text(word.text()))
    }

    /// A named derivation of this key: `{self}-{word}`.
    pub(crate) fn tail(&self, word: Word) -> Self {
        self.derive(Piece::Text("-"), Piece::Text(word.text()))
    }

    /// A numbered derivation of this key: `{self}-{index}`.
    pub(crate) fn tail_index(&self, index: impl Ordinal) -> Self {
        self.derive(Piece::Text("-"), index.piece())
    }

    fn derive(&self, separator: Piece, value: Piece) -> Self {
        let mut result = self.clone();
        result.pieces.push(separator);
        result.pieces.push(value);
        result
    }
}

impl fmt::Display for Stem {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        for piece in self.pieces.iter() {
            piece.fmt(formatter)?;
        }
        Ok(())
    }
}

/// Declare one minter over a const-admitted `iges:{scope}:{kind}` namespace.
macro_rules! minter {
    ($(#[$meta:meta])* body, $admitted:ident, $ty:ty, $scope:literal, $kind:literal) => {
        $(#[$meta])*
        pub(crate) fn body(stem: &Stem) -> $ty {
            <$ty>::compose(
                &cadmpeg_ir::identity_namespace!("iges", $scope, $kind),
                stem.key(),
            )
        }

        pub(crate) fn $admitted(stem: &Stem, ctx: &DecodeContext<'_>) -> Result<$ty, CodecError> {
            let text = ctx.format_retained(
                format_args!("iges:{}:{}#{stem}", $scope, $kind),
                "iges generated identity",
            )?;
            <$ty>::try_from(text)
                .map_err(|_| CodecError::Malformed("IGES generated identity is invalid".into()))
        }
    };
    ($(#[$meta:meta])* $name:ident, $ty:ty, $scope:literal, $kind:literal) => {
        $(#[$meta])*
        pub(crate) fn $name(stem: &Stem) -> $ty {
            <$ty>::compose(
                &cadmpeg_ir::identity_namespace!("iges", $scope, $kind),
                stem.key(),
            )
        }
    };
    ($(#[$meta:meta])* $name:ident, $admitted:ident, $ty:ty, $scope:literal, $kind:literal) => {
        $(#[$meta])*
        #[cfg(test)]
        pub(crate) fn $name(stem: &Stem) -> $ty {
            <$ty>::compose(
                &cadmpeg_ir::identity_namespace!("iges", $scope, $kind),
                stem.key(),
            )
        }
            pub(crate) fn $admitted(
                stem: &Stem,
                ctx: &DecodeContext<'_>,
            ) -> Result<$ty, CodecError> {
                let text = ctx.format_retained(format_args!("iges:{}:{}#{stem}", $scope, $kind), "iges generated identity")?;
                <$ty>::try_from(text)
                    .map_err(|_| CodecError::Malformed("IGES generated identity is invalid".into()))
            }
    };
}

minter!(
    /// The body named by this key.
    body,
    body_admitted,
    BodyId,
    "model",
    "body"
);
minter!(
    /// The coedge named by this key.
    coedge,
    coedge_admitted,
    CoedgeId,
    "model",
    "coedge"
);
minter!(
    /// The curve named by this key.
    curve,
    curve_admitted,
    CurveId,
    "model",
    "curve"
);
minter!(
    /// The edge named by this key.
    edge,
    edge_admitted,
    EdgeId,
    "model",
    "edge"
);
minter!(
    /// The face named by this key.
    face,
    face_admitted,
    FaceId,
    "model",
    "face"
);
minter!(
    /// The loop named by this key.
    r#loop,
    loop_admitted,
    LoopId,
    "model",
    "loop"
);
minter!(
    /// The pcurve named by this key.
    pcurve,
    pcurve_admitted,
    PcurveId,
    "model",
    "pcurve"
);
minter!(
    /// The point named by this key.
    point,
    point_admitted,
    PointId,
    "model",
    "point"
);
minter!(
    /// The procedural curve named by this key.
    procedural_curve,
    procedural_curve_admitted,
    ProceduralCurveId,
    "model",
    "procedural-curve"
);
minter!(
    /// The procedural surface named by this key.
    procedural_surface,
    procedural_surface_admitted,
    ProceduralSurfaceId,
    "model",
    "procedural-surface"
);
minter!(
    /// The region named by this key.
    region,
    region_admitted,
    RegionId,
    "model",
    "region"
);
minter!(
    /// The shell named by this key.
    shell,
    shell_admitted,
    ShellId,
    "model",
    "shell"
);
minter!(
    /// The surface named by this key.
    surface,
    surface_admitted,
    SurfaceId,
    "model",
    "surface"
);
minter!(
    /// The vertex named by this key.
    vertex,
    vertex_admitted,
    VertexId,
    "model",
    "vertex"
);
minter!(
    /// The appearance named by this Directory colour definition key.
    appearance_color,
    appearance_color_admitted,
    AppearanceId,
    "appearance",
    "color"
);
minter!(
    /// The appearance named by this standard colour number.
    appearance_standard,
    appearance_standard_admitted,
    AppearanceId,
    "appearance",
    "standard"
);
minter!(
    /// The appearance binding named by this key.
    appearance_binding,
    appearance_binding_admitted,
    AppearanceBindingId,
    "model",
    "appearance-binding"
);

#[cfg(test)]
mod tests {
    use super::{directory_lookup_key, Stem, StemPieces, Word};
    use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
    use cadmpeg_core::CodecError;

    #[test]
    fn directory_lookup_key_uses_stack_storage_for_full_u32_range() {
        let policy = DecodePolicy::service();
        let arena = DecodeArena::new();
        let ctx = DecodeContext::new(&arena, &policy, false);
        let mut storage = [0_u8; 64];
        assert_eq!(
            directory_lookup_key("iges:model:surface#D", 0, &mut storage, &ctx).unwrap(),
            Some("iges:model:surface#D0")
        );
        assert_eq!(
            directory_lookup_key("iges:model:edge#D", u32::MAX, &mut storage, &ctx).unwrap(),
            Some("iges:model:edge#D4294967295")
        );
        let mut short = [0_u8; 3];
        assert_eq!(
            directory_lookup_key("iges:model:edge#D", 1, &mut short, &ctx).unwrap(),
            None
        );
    }

    #[test]
    fn generated_identity_minters_charge_before_rendering_each_kind() {
        let stem = Stem::directory(1_u32);
        let service_arena = DecodeArena::new();
        let service_policy = DecodePolicy::service();
        let (service_ctx, _) = DecodeContext::from_root_bytes(&[], &service_arena, &service_policy)
            .expect("test setup");
        macro_rules! check {
            ($old:ident, $admitted:ident) => {
                let error = cadmpeg_test_support::refusal::resource_limit_at(ResourceDimension::RetainedBytes, "iges generated identity", |cap| {
                    let mut policy = DecodePolicy::service();
                    policy.limits.max_retained_bytes = cap;
                    let arena = DecodeArena::new();
                    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy)?;
                    super::$admitted(&stem, &ctx)
                });
                assert!(matches!(error, CodecError::ResourceLimit(limit) if limit.dimension == ResourceDimension::RetainedBytes && limit.operation == "iges generated identity"));
                assert_eq!(super::$admitted(&stem, &service_ctx).expect("test setup").as_str(), super::$old(&stem).as_str());
            };
        }
        check!(body, body_admitted);
        check!(coedge, coedge_admitted);
        check!(curve, curve_admitted);
        check!(edge, edge_admitted);
        check!(face, face_admitted);
        check!(r#loop, loop_admitted);
        check!(pcurve, pcurve_admitted);
        check!(point, point_admitted);
        check!(procedural_curve, procedural_curve_admitted);
        check!(procedural_surface, procedural_surface_admitted);
        check!(region, region_admitted);
        check!(shell, shell_admitted);
        check!(surface, surface_admitted);
        check!(vertex, vertex_admitted);
        check!(appearance_color, appearance_color_admitted);
        check!(appearance_standard, appearance_standard_admitted);
        check!(appearance_binding, appearance_binding_admitted);
    }

    #[test]
    fn decoded_stem_derivations_use_inline_parts_without_changing_keys() {
        let stem = Stem::directory(u32::MAX)
            .child(u32::MAX)
            .slot(usize::MAX)
            .slot(usize::MAX);
        assert!(matches!(stem.pieces, StemPieces::Inline { .. }));
        assert_eq!(stem.origin(), Some(u32::MAX));
        assert_eq!(
            stem.to_string(),
            format!("D4294967295:D4294967295:{0}:{0}", usize::MAX)
        );
        assert!(matches!(
            Stem::directory(1_u32).tail(Word::End).pieces,
            StemPieces::Inline { .. }
        ));
        assert_eq!(
            Stem::word_directory(Word::Face, 1_u32).to_string(),
            "face-D1"
        );
        assert_eq!(Stem::number(-1_i64).to_string(), "-1");
    }
    #[test]
    fn decoded_brep_pcurve_stem_uses_inline_parts() {
        let stem = Stem::directory(u32::MAX)
            .child(u32::MAX)
            .child(u32::MAX)
            .slot(usize::MAX)
            .slot(usize::MAX);
        assert!(matches!(stem.pieces, StemPieces::Inline { .. }));
        assert_eq!(stem.origin(), Some(u32::MAX));
        assert_eq!(stem.to_string(), format!("D4294967295:D4294967295:D4294967295:{0}:{0}", usize::MAX));
    }
    #[test]
    fn directory_lookup_text_refuses_utf8_scan() {
        cadmpeg_test_support::refusal::resource_limit_at(ResourceDimension::WorkUnits, "iges directory lookup text", |cap| {
            let mut policy = DecodePolicy::service();
            policy.limits.max_work_units = cap;
            let arena = DecodeArena::new();
            let ctx = DecodeContext::new(&arena, &policy, false);
            let mut storage = [0_u8; 64];
            directory_lookup_key("D", 1, &mut storage, &ctx).map(|_| ())
        });
    }

}
