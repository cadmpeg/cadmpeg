// SPDX-License-Identifier: Apache-2.0
//! Output and input format identity: names, extension inference, and the
//! codec ids that an explicit `--input-format` bypass resolves to.

use std::path::Path;

use anyhow::{anyhow, Result};
use clap::ValueEnum;

/// A format `cadmpeg` can write.
#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
pub(crate) enum Format {
    /// Canonical CADIR JSON.
    #[value(alias = "json")]
    Cadir,
    /// ISO 10303-21 STEP AP214.
    Step,
    /// `FreeCAD` `.FCStd`.
    Fcstd,
    /// Autodesk Fusion `.f3d`.
    F3d,
    /// `SolidWorks` `.sldprt`.
    Sldprt,
    /// Rhino `.3dm`.
    #[value(alias = "3dm")]
    Rhino,
}

/// One output format's stable name and the input extensions that infer it.
///
/// This single table backs [`Format::name`], [`Format::from_extension`], and
/// the path-based inference in [`Format::from_path`], so the name and its
/// recognized extensions are defined in exactly one place.
struct FormatRow {
    format: Format,
    name: &'static str,
    extensions: &'static [&'static str],
}

const TABLE: &[FormatRow] = &[
    FormatRow {
        format: Format::Cadir,
        name: "cadir",
        extensions: &["cadir", "json"],
    },
    FormatRow {
        format: Format::Step,
        name: "step",
        extensions: &["step", "stp"],
    },
    FormatRow {
        format: Format::Fcstd,
        name: "fcstd",
        extensions: &["fcstd"],
    },
    FormatRow {
        format: Format::F3d,
        name: "f3d",
        extensions: &["f3d"],
    },
    FormatRow {
        format: Format::Sldprt,
        name: "sldprt",
        extensions: &["sldprt"],
    },
    FormatRow {
        format: Format::Rhino,
        name: "rhino",
        extensions: &["3dm"],
    },
];

impl Format {
    /// Stable lowercase identifier used in reports and encoder dispatch.
    pub(crate) fn name(self) -> &'static str {
        TABLE
            .iter()
            .find(|row| row.format == self)
            .expect("every Format variant has a TABLE row")
            .name
    }

    /// Infer a format from a bare file extension, case-insensitively.
    pub(crate) fn from_extension(extension: &str) -> Option<Self> {
        let extension = extension.to_ascii_lowercase();
        TABLE
            .iter()
            .find(|row| row.extensions.contains(&extension.as_str()))
            .map(|row| row.format)
    }

    /// Whether this format carries geometry rather than the neutral CADIR document.
    pub(crate) fn is_geometry_export(self) -> bool {
        self != Self::Cadir
    }

    /// Infer a format from a path's extension.
    pub(crate) fn from_path(path: Option<&Path>) -> Option<Self> {
        path.and_then(Path::extension)
            .and_then(|extension| extension.to_str())
            .and_then(Self::from_extension)
    }
}

/// A format the user may force with `--input-format`, bypassing content detection.
#[derive(Debug, Clone, Copy, ValueEnum)]
pub(crate) enum InputFormat {
    /// `FreeCAD` `.FCStd`.
    Fcstd,
    /// Autodesk Fusion `.f3d`.
    F3d,
    /// `SolidWorks` `.sldprt`.
    Sldprt,
    /// CATIA V5 `.CATPart`.
    #[value(alias = "catia")]
    Catpart,
    /// Siemens NX `.prt`.
    Nx,
    /// Creo Parametric `.prt`.
    Creo,
    /// Rhino `.3dm`.
    #[value(alias = "3dm")]
    Rhino,
    /// IGES `.igs` or `.iges`.
    #[value(alias = "igs")]
    Iges,
    /// Canonical CADIR JSON.
    Cadir,
}

/// The loader path an explicit input format selects.
#[derive(Debug, Clone, Copy)]
pub(crate) enum ForcedInput {
    /// Decode through the codec with this stable id.
    Codec(&'static str),
    /// Parse the input directly as a CADIR document.
    Cadir,
}

impl InputFormat {
    /// The codec id or CADIR path this forced input resolves to.
    ///
    /// This maps user-facing format names to codec ids, distinct knowledge from
    /// the output [`TABLE`]: an input format names a decoder, an output format
    /// names an encoder and a set of recognized extensions.
    pub(crate) fn resolution(self) -> ForcedInput {
        match self {
            Self::Fcstd => ForcedInput::Codec("fcstd"),
            Self::F3d => ForcedInput::Codec("f3d"),
            Self::Sldprt => ForcedInput::Codec("sldprt"),
            Self::Catpart => ForcedInput::Codec("catia"),
            Self::Nx => ForcedInput::Codec("nx"),
            Self::Creo => ForcedInput::Codec("creo"),
            Self::Rhino => ForcedInput::Codec("rhino"),
            Self::Iges => ForcedInput::Codec("iges"),
            Self::Cadir => ForcedInput::Cadir,
        }
    }
}

/// Resolve the effective output format from an explicit flag and the output path.
///
/// An explicit format wins; when it disagrees with a recognized output
/// extension the mismatch is warned about. Without an explicit format the
/// extension must infer one.
pub(crate) fn resolve_format(explicit: Option<Format>, out: Option<&Path>) -> Result<Format> {
    if let Some(format) = explicit {
        if let Some(inferred) = Format::from_path(out) {
            if inferred != format {
                eprintln!(
                    "warning: explicit format {} disagrees with output extension format {}; using {}",
                    format.name(),
                    inferred.name(),
                    format.name()
                );
            }
        }
        return Ok(format);
    }
    Format::from_path(out).ok_or_else(|| anyhow!("cannot infer format; pass -f"))
}
