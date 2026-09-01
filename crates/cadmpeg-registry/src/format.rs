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
    /// Whether `name` is a format word in the output grammar, independent of
    /// whether this build can write it.
    #[must_use]
    pub fn is_known_name(name: &str) -> bool {
        crate::registry::is_format_name(name)
    }

    /// Every output format this build carries, in registry order.
    pub fn all() -> impl Iterator<Item = Self> {
        crate::descriptors::writable().map(|(_, output)| output.format)
    }

    /// The format an output-format word names, by id or by accepted alias.
    ///
    /// Total over the `--to` format vocabulary, and the reason a bare `--to`
    /// value is unambiguous: a value this returns `Some` for is a format, and
    /// every other bare value is a dialect of the inferred output format.
    /// `registry::tests::compiled_write_catalogs_match_registry_policy` proves
    /// no compiled target alias lands here.
    #[must_use]
    pub fn from_name(name: &str) -> Option<Self> {
        let canonical = crate::registry::canonical_format_name(name)?;
        crate::descriptors::writable()
            .find(|(descriptor, _)| descriptor.id == canonical)
            .map(|(_, output)| output.format)
    }

    /// The output-format words this build accepts, for a refusal message.
    #[must_use]
    pub fn vocabulary() -> String {
        Self::all().map(Self::name).collect::<Vec<_>>().join(", ")
    }

    /// The format a filename extension names, case-insensitively.
    #[must_use]
    pub fn from_extension(extension: &str) -> Option<Self> {
        let extension = extension.to_ascii_lowercase();
        crate::descriptors::writable()
            .find(|(_, output)| output.extensions.contains(&extension.as_str()))
            .map(|(_, output)| output.format)
    }

    /// The stable format id, which is also its canonical `--to` spelling.
    #[must_use]
    pub fn name(self) -> &'static str {
        crate::descriptors::by_output(self).0.id
    }

    /// Whether this format's encoder emits a binary container.
    #[must_use]
    pub fn is_binary(self) -> bool {
        crate::descriptors::by_output(self).1.physics.is_binary()
    }

    /// Whether an export to this format requires transferred geometry.
    #[must_use]
    pub fn transfers_geometry(self) -> bool {
        crate::descriptors::by_output(self)
            .1
            .physics
            .transfers_geometry()
    }
}

#[cfg(test)]
mod tests {
    use super::Format;

    #[test]
    fn known_format_vocabulary_includes_canonical_names_and_output_aliases() {
        for name in [
            "cadir",
            "json",
            "step",
            "stp",
            "fcstd",
            "f3d",
            "sldprt",
            "rhino",
            "3dm",
            "iges",
            "igs",
            "inventor",
            "catia",
            "creo",
            "nx",
            "sat",
            "acis",
            "parasolid",
        ] {
            assert!(Format::is_known_name(name), "{name}");
        }
        assert!(!Format::is_known_name("5.1"));
    }

    #[cfg(feature = "step")]
    #[test]
    fn output_aliases_resolve_through_the_identity_registry() {
        assert_eq!(Format::from_name("stp"), Some(Format::Step));
        assert_eq!(Format::from_name("json"), Some(Format::Cadir));
        assert_eq!(Format::from_name("inventor"), None);
    }

    #[test]
    fn output_physics_come_from_each_writable_descriptor() {
        assert!(!Format::Cadir.is_binary());
        assert!(!Format::Cadir.transfers_geometry());
        #[cfg(feature = "step")]
        {
            assert!(!Format::Step.is_binary());
            assert!(Format::Step.transfers_geometry());
        }
        #[cfg(feature = "rhino")]
        {
            assert!(Format::Rhino.is_binary());
            assert!(Format::Rhino.transfers_geometry());
        }
    }
}
