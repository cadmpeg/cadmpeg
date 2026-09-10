// SPDX-License-Identifier: Apache-2.0
//! The one route from decoded numbers to IR identities.
//!
//! An identity key must be non-empty and carry no `#` and no whitespace. A
//! [`Stem`] is built only from decoded numbers and the fixed [`Word`]s spelled
//! in this crate's source, so every key this module builds satisfies that
//! grammar and the minters below are total. No stem can be built from
//! file-derived text, so no decoded label can reach a minting failure.

use std::fmt;

use cadmpeg_ir::ids::IdentityError;
use cadmpeg_ir::ids::{
    AppearanceId, BodyId, CoedgeId, CurveId, EdgeId, FaceId, LoopId, PcurveId, PointId,
    ProceduralCurveId, ProceduralSurfaceId, RegionId, ShellId, SurfaceId, VertexId,
};

/// A decoded number an identity key may be spelled with.
pub(crate) trait Ordinal: Copy {
    /// The decimal spelling of this number.
    fn spelling(self) -> String;

    /// This number read as a Directory sequence, if it is one.
    fn sequence(self) -> Option<u32>;
}

impl Ordinal for u32 {
    fn spelling(self) -> String {
        self.to_string()
    }

    fn sequence(self) -> Option<u32> {
        Some(self)
    }
}

impl Ordinal for usize {
    fn spelling(self) -> String {
        self.to_string()
    }

    fn sequence(self) -> Option<u32> {
        u32::try_from(self).ok()
    }
}

impl Ordinal for i64 {
    fn spelling(self) -> String {
        self.to_string()
    }

    fn sequence(self) -> Option<u32> {
        u32::try_from(self).ok()
    }
}

/// A fixed word this crate spells identity keys with.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Word {
    BoundedPlane,
    End,
    FreeGeometry,
    ImplicitOuter,
    LegacySingleParent,
    PlacedDirectrix,
    PlacedGeneratrix,
    PlacedSource,
    Start,
}

impl Word {
    const ALL: [Self; 9] = [
        Self::BoundedPlane,
        Self::End,
        Self::FreeGeometry,
        Self::ImplicitOuter,
        Self::LegacySingleParent,
        Self::PlacedDirectrix,
        Self::PlacedGeneratrix,
        Self::PlacedSource,
        Self::Start,
    ];

    const fn text(self) -> &'static str {
        match self {
            Self::BoundedPlane => "bounded-plane",
            Self::End => "end",
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

const _: () = {
    let mut word = 0;
    while word < Word::ALL.len() {
        let bytes = Word::ALL[word].text().as_bytes();
        assert!(!bytes.is_empty());
        let mut index = 0;
        while index < bytes.len() {
            assert!(bytes[index] != b'#');
            assert!(bytes[index] > b' ' && bytes[index] < 0x7f);
            index += 1;
        }
        word += 1;
    }
};

/// An identity key built only from decoded numbers and [`Word`]s.
///
/// A key rooted at a Directory sequence carries that sequence as its origin,
/// and every derivation keeps it, so a reader that needs the entry a record
/// came from asks the stem instead of parsing the minted text.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Stem {
    key: String,
    origin: Option<u32>,
}

impl Stem {
    /// The key of one Directory entry: `D{sequence}`.
    pub(crate) fn directory(sequence: impl Ordinal) -> Self {
        Self {
            key: format!("D{}", sequence.spelling()),
            origin: sequence.sequence(),
        }
    }

    /// A key that is one fixed word.
    pub(crate) fn word(word: Word) -> Self {
        Self {
            key: word.text().to_owned(),
            origin: None,
        }
    }

    /// A fixed word qualified by a Directory sequence: `{word}-D{sequence}`.
    ///
    /// The key is rooted at the word, not at the sequence, so it has no origin:
    /// such a record is a derivation of the entry, not its whole neutral form.
    pub(crate) fn word_directory(word: Word, sequence: impl Ordinal) -> Self {
        Self {
            key: format!("{}-D{}", word.text(), sequence.spelling()),
            origin: None,
        }
    }

    /// A key that is one decoded number.
    pub(crate) fn number(value: impl Ordinal) -> Self {
        Self {
            key: value.spelling(),
            origin: None,
        }
    }

    /// The Directory entry this key is rooted at, if it is rooted at one.
    pub(crate) const fn origin(&self) -> Option<u32> {
        self.origin
    }

    /// A child keyed by a Directory sequence: `{self}:D{sequence}`.
    pub(crate) fn child(&self, sequence: impl Ordinal) -> Self {
        self.derive(format!("{}:D{}", self.key, sequence.spelling()))
    }

    /// A child keyed by an ordinal: `{self}:{index}`.
    pub(crate) fn slot(&self, index: impl Ordinal) -> Self {
        self.derive(format!("{}:{}", self.key, index.spelling()))
    }

    /// A named part of this key: `{self}:{word}`.
    pub(crate) fn part(&self, word: Word) -> Self {
        self.derive(format!("{}:{}", self.key, word.text()))
    }

    /// A named derivation of this key: `{self}-{word}`.
    pub(crate) fn tail(&self, word: Word) -> Self {
        self.derive(format!("{}-{}", self.key, word.text()))
    }

    /// A numbered derivation of this key: `{self}-{index}`.
    pub(crate) fn tail_index(&self, index: impl Ordinal) -> Self {
        self.derive(format!("{}-{}", self.key, index.spelling()))
    }

    fn derive(&self, key: String) -> Self {
        Self {
            key,
            origin: self.origin,
        }
    }
}

impl fmt::Display for Stem {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.key)
    }
}

/// The one place the identity grammar is proved, for every id this crate mints.
fn mint<T: TryFrom<String, Error = IdentityError>>(namespace: &str, stem: &Stem) -> T {
    T::try_from(format!("iges:{namespace}#{stem}")).expect("a stem is a valid identity key")
}

macro_rules! minter {
    ($(#[$meta:meta])* $name:ident, $ty:ty, $namespace:literal) => {
        $(#[$meta])*
        pub(crate) fn $name(stem: &Stem) -> $ty {
            mint($namespace, stem)
        }
    };
}

minter!(
    /// The body named by this key.
    body,
    BodyId,
    "model:body"
);
minter!(
    /// The coedge named by this key.
    coedge,
    CoedgeId,
    "model:coedge"
);
minter!(
    /// The curve named by this key.
    curve,
    CurveId,
    "model:curve"
);
minter!(
    /// The edge named by this key.
    edge,
    EdgeId,
    "model:edge"
);
minter!(
    /// The face named by this key.
    face,
    FaceId,
    "model:face"
);
minter!(
    /// The loop named by this key.
    r#loop,
    LoopId,
    "model:loop"
);
minter!(
    /// The pcurve named by this key.
    pcurve,
    PcurveId,
    "model:pcurve"
);
minter!(
    /// The point named by this key.
    point,
    PointId,
    "model:point"
);
minter!(
    /// The procedural curve named by this key.
    procedural_curve,
    ProceduralCurveId,
    "model:procedural-curve"
);
minter!(
    /// The procedural surface named by this key.
    procedural_surface,
    ProceduralSurfaceId,
    "model:procedural-surface"
);
minter!(
    /// The region named by this key.
    region,
    RegionId,
    "model:region"
);
minter!(
    /// The shell named by this key.
    shell,
    ShellId,
    "model:shell"
);
minter!(
    /// The surface named by this key.
    surface,
    SurfaceId,
    "model:surface"
);
minter!(
    /// The vertex named by this key.
    vertex,
    VertexId,
    "model:vertex"
);
minter!(
    /// The appearance named by this Directory colour definition key.
    appearance_color,
    AppearanceId,
    "appearance:color"
);
minter!(
    /// The appearance named by this standard colour number.
    appearance_standard,
    AppearanceId,
    "appearance:standard"
);
