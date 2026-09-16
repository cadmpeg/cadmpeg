// SPDX-License-Identifier: Apache-2.0
//! The one route from decoded numbers to IR identities.
//!
//! An identity key must be non-empty and carry no `#` and no whitespace. A
//! [`Stem`] is built only from decoded numbers and the fixed [`Word`]s spelled
//! in this crate's source, so every key this module builds satisfies that
//! grammar and the minters below are total. No stem can be built from
//! file-derived text, so no decoded label can reach a minting failure.

use std::fmt;

use cadmpeg_ir::identity_key;
use cadmpeg_ir::ids::{
    AppearanceBindingId, AppearanceId, BodyId, CoedgeId, CurveId, EdgeId, FaceId, IdentityKey,
    LoopId, PcurveId, PointId, ProceduralCurveId, ProceduralSurfaceId, RegionId, ShellId,
    SurfaceId, VertexId,
};

/// A decoded number an identity key may be spelled with.
///
/// A decimal spelling is an identity key by construction, so the conversion
/// answers [`IdentityKey`] and no minter has a failing branch.
pub(crate) trait Ordinal: Copy {
    /// The decimal spelling of this number, as an identity key.
    fn key(self) -> IdentityKey;

    /// This number read as a Directory sequence, if it is one.
    fn sequence(self) -> Option<u32>;
}

impl Ordinal for u32 {
    fn key(self) -> IdentityKey {
        IdentityKey::from(self)
    }

    fn sequence(self) -> Option<u32> {
        Some(self)
    }
}

impl Ordinal for usize {
    fn key(self) -> IdentityKey {
        IdentityKey::from(self)
    }

    fn sequence(self) -> Option<u32> {
        u32::try_from(self).ok()
    }
}

impl Ordinal for i64 {
    fn key(self) -> IdentityKey {
        IdentityKey::from(self)
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
    /// This word as an identity key.
    ///
    /// Every arm is a literal the `identity_key!` macro admits during const
    /// evaluation, so a word that breaks the key grammar fails `cargo check`.
    fn key(self) -> IdentityKey {
        match self {
            Self::Body => identity_key!("body"),
            Self::BoundedPlane => identity_key!("bounded-plane"),
            Self::End => identity_key!("end"),
            Self::Face => identity_key!("face"),
            Self::FreeGeometry => identity_key!("free-geometry"),
            Self::ImplicitOuter => identity_key!("implicit-outer"),
            Self::LegacySingleParent => identity_key!("legacy-single-parent"),
            Self::PlacedDirectrix => identity_key!("placed-directrix"),
            Self::PlacedGeneratrix => identity_key!("placed-generatrix"),
            Self::PlacedSource => identity_key!("placed-source"),
            Self::Start => identity_key!("start"),
        }
    }
}

/// An identity key built only from decoded numbers and [`Word`]s.
///
/// A key rooted at a Directory sequence carries that sequence as its origin,
/// and every derivation keeps it, so a reader that needs the entry a record
/// came from asks the stem instead of parsing the minted text.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Stem {
    key: IdentityKey,
    origin: Option<u32>,
}

impl Stem {
    /// The key of one Directory entry: `D{sequence}`.
    pub(crate) fn directory(sequence: impl Ordinal) -> Self {
        Self {
            key: identity_key!("D").then(sequence.key()),
            origin: sequence.sequence(),
        }
    }

    /// A key that is one fixed word.
    pub(crate) fn word(word: Word) -> Self {
        Self {
            key: word.key(),
            origin: None,
        }
    }

    /// A fixed word qualified by a Directory sequence: `{word}-D{sequence}`.
    ///
    /// The key is rooted at the word, not at the sequence, so it has no origin:
    /// such a record is a derivation of the entry, not its whole neutral form.
    pub(crate) fn word_directory(word: Word, sequence: impl Ordinal) -> Self {
        Self {
            key: word.key().dash(identity_key!("D").then(sequence.key())),
            origin: None,
        }
    }

    /// A key that is one decoded number.
    pub(crate) fn number(value: impl Ordinal) -> Self {
        Self {
            key: value.key(),
            origin: None,
        }
    }

    /// The Directory entry this key is rooted at, if it is rooted at one.
    pub(crate) const fn origin(&self) -> Option<u32> {
        self.origin
    }

    /// This stem's identity key.
    pub(crate) fn key(&self) -> IdentityKey {
        self.key.clone()
    }

    /// A child keyed by a Directory sequence: `{self}:D{sequence}`.
    pub(crate) fn child(&self, sequence: impl Ordinal) -> Self {
        self.derive(
            self.key
                .clone()
                .colon(identity_key!("D").then(sequence.key())),
        )
    }

    /// A child keyed by an ordinal: `{self}:{index}`.
    pub(crate) fn slot(&self, index: impl Ordinal) -> Self {
        self.derive(self.key.clone().colon(index.key()))
    }

    /// A named part of this key: `{self}:{word}`.
    pub(crate) fn part(&self, word: Word) -> Self {
        self.derive(self.key.clone().colon(word.key()))
    }

    /// A named derivation of this key: `{self}-{word}`.
    pub(crate) fn tail(&self, word: Word) -> Self {
        self.derive(self.key.clone().dash(word.key()))
    }

    /// A numbered derivation of this key: `{self}-{index}`.
    pub(crate) fn tail_index(&self, index: impl Ordinal) -> Self {
        self.derive(self.key.clone().dash(index.key()))
    }

    fn derive(&self, key: IdentityKey) -> Self {
        Self {
            key,
            origin: self.origin,
        }
    }
}

impl fmt::Display for Stem {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.key.as_str())
    }
}

/// Declare one minter over a const-admitted `iges:{scope}:{kind}` namespace.
macro_rules! minter {
    ($(#[$meta:meta])* $name:ident, $ty:ty, $scope:literal, $kind:literal) => {
        $(#[$meta])*
        pub(crate) fn $name(stem: &Stem) -> $ty {
            <$ty>::compose(
                &cadmpeg_ir::identity_namespace!("iges", $scope, $kind),
                stem.key(),
            )
        }
    };
}

minter!(
    /// The body named by this key.
    body,
    BodyId,
    "model",
    "body"
);
minter!(
    /// The coedge named by this key.
    coedge,
    CoedgeId,
    "model",
    "coedge"
);
minter!(
    /// The curve named by this key.
    curve,
    CurveId,
    "model",
    "curve"
);
minter!(
    /// The edge named by this key.
    edge,
    EdgeId,
    "model",
    "edge"
);
minter!(
    /// The face named by this key.
    face,
    FaceId,
    "model",
    "face"
);
minter!(
    /// The loop named by this key.
    r#loop,
    LoopId,
    "model",
    "loop"
);
minter!(
    /// The pcurve named by this key.
    pcurve,
    PcurveId,
    "model",
    "pcurve"
);
minter!(
    /// The point named by this key.
    point,
    PointId,
    "model",
    "point"
);
minter!(
    /// The procedural curve named by this key.
    procedural_curve,
    ProceduralCurveId,
    "model",
    "procedural-curve"
);
minter!(
    /// The procedural surface named by this key.
    procedural_surface,
    ProceduralSurfaceId,
    "model",
    "procedural-surface"
);
minter!(
    /// The region named by this key.
    region,
    RegionId,
    "model",
    "region"
);
minter!(
    /// The shell named by this key.
    shell,
    ShellId,
    "model",
    "shell"
);
minter!(
    /// The surface named by this key.
    surface,
    SurfaceId,
    "model",
    "surface"
);
minter!(
    /// The vertex named by this key.
    vertex,
    VertexId,
    "model",
    "vertex"
);
minter!(
    /// The appearance named by this Directory colour definition key.
    appearance_color,
    AppearanceId,
    "appearance",
    "color"
);
minter!(
    /// The appearance named by this standard colour number.
    appearance_standard,
    AppearanceId,
    "appearance",
    "standard"
);
minter!(
    /// The appearance binding named by this key.
    appearance_binding,
    AppearanceBindingId,
    "model",
    "appearance-binding"
);
