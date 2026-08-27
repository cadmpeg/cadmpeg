// SPDX-License-Identifier: Apache-2.0
//! The output-format vocabulary this build carries.

/// An output format this build can write.
///
/// Not a `ValueEnum`: `--to` takes `FORMAT[:DIALECT]`, and clap cannot parse
/// the dialect half. [`Format::from_name`] is the whole output-format
/// vocabulary, aliases included.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Format {
    /// CADIR JSON. Also spelled `json`.
    Cadir,
    /// ISO 10303-21 STEP.
    #[cfg(feature = "step")]
    Step,
    /// `FreeCAD` `.FCStd`.
    #[cfg(feature = "fcstd")]
    Fcstd,
    /// Autodesk Fusion `.f3d`.
    #[cfg(feature = "f3d")]
    F3d,
    /// `SolidWorks` `.sldprt`.
    #[cfg(feature = "sldprt")]
    Sldprt,
    /// Rhino `.3dm`. Also spelled `3dm`.
    #[cfg(feature = "rhino")]
    Rhino,
    /// IGES `.igs` or `.iges`. Also spelled `igs`.
    #[cfg(feature = "iges")]
    Iges,
}

impl Format {
    /// Every output format this build carries, in help and listing order.
    pub const ALL: &'static [Self] = &[
        Self::Cadir,
        #[cfg(feature = "step")]
        Self::Step,
        #[cfg(feature = "fcstd")]
        Self::Fcstd,
        #[cfg(feature = "f3d")]
        Self::F3d,
        #[cfg(feature = "sldprt")]
        Self::Sldprt,
        #[cfg(feature = "rhino")]
        Self::Rhino,
        #[cfg(feature = "iges")]
        Self::Iges,
    ];

    /// The format an output-format word names, by id or by accepted alias.
    ///
    /// Total over the `--to` format vocabulary, and the reason a bare `--to`
    /// value is unambiguous: a value this returns `Some` for is a format, and
    /// every other bare value is a dialect of the inferred output format.
    /// `scripts/check-dialect-support.py` proves no target alias lands here.
    #[must_use]
    pub fn from_name(name: &str) -> Option<Self> {
        match name {
            "cadir" | "json" => Some(Self::Cadir),
            #[cfg(feature = "step")]
            "step" => Some(Self::Step),
            #[cfg(feature = "fcstd")]
            "fcstd" => Some(Self::Fcstd),
            #[cfg(feature = "f3d")]
            "f3d" => Some(Self::F3d),
            #[cfg(feature = "sldprt")]
            "sldprt" => Some(Self::Sldprt),
            #[cfg(feature = "rhino")]
            "rhino" | "3dm" => Some(Self::Rhino),
            #[cfg(feature = "iges")]
            "iges" | "igs" => Some(Self::Iges),
            _ => None,
        }
    }

    /// The output-format words this build accepts, for a refusal message.
    #[must_use]
    pub fn vocabulary() -> String {
        Self::ALL
            .iter()
            .map(|format| format.name())
            .collect::<Vec<_>>()
            .join(", ")
    }

    /// The format a filename extension names, case-insensitively.
    #[must_use]
    pub fn from_extension(extension: &str) -> Option<Self> {
        match extension.to_ascii_lowercase().as_str() {
            "cadir" | "json" => Some(Self::Cadir),
            #[cfg(feature = "step")]
            "step" | "stp" => Some(Self::Step),
            #[cfg(feature = "fcstd")]
            "fcstd" => Some(Self::Fcstd),
            #[cfg(feature = "f3d")]
            "f3d" => Some(Self::F3d),
            #[cfg(feature = "sldprt")]
            "sldprt" => Some(Self::Sldprt),
            #[cfg(feature = "rhino")]
            "3dm" => Some(Self::Rhino),
            #[cfg(feature = "iges")]
            "iges" | "igs" => Some(Self::Iges),
            _ => None,
        }
    }

    /// Whether writing this format transfers geometry rather than the neutral
    /// document.
    #[must_use]
    pub fn is_geometry_export(self) -> bool {
        match self {
            Self::Cadir => false,
            #[cfg(feature = "step")]
            Self::Step => true,
            #[cfg(feature = "fcstd")]
            Self::Fcstd => true,
            #[cfg(feature = "f3d")]
            Self::F3d => true,
            #[cfg(feature = "sldprt")]
            Self::Sldprt => true,
            #[cfg(feature = "rhino")]
            Self::Rhino => true,
            #[cfg(feature = "iges")]
            Self::Iges => true,
        }
    }

    /// Whether this output format is a binary container, which is unsafe to
    /// stream to a terminal or a JSON-expecting pipe by accident.
    #[must_use]
    pub fn is_binary_container(self) -> bool {
        match self {
            #[cfg(feature = "fcstd")]
            Self::Fcstd => true,
            #[cfg(feature = "f3d")]
            Self::F3d => true,
            #[cfg(feature = "sldprt")]
            Self::Sldprt => true,
            #[cfg(feature = "rhino")]
            Self::Rhino => true,
            _ => false,
        }
    }

    /// The stable format id, which is also its canonical `--to` spelling.
    #[must_use]
    pub fn name(self) -> &'static str {
        match self {
            Self::Cadir => "cadir",
            #[cfg(feature = "step")]
            Self::Step => "step",
            #[cfg(feature = "fcstd")]
            Self::Fcstd => "fcstd",
            #[cfg(feature = "f3d")]
            Self::F3d => "f3d",
            #[cfg(feature = "sldprt")]
            Self::Sldprt => "sldprt",
            #[cfg(feature = "rhino")]
            Self::Rhino => "rhino",
            #[cfg(feature = "iges")]
            Self::Iges => "iges",
        }
    }
}
