// SPDX-License-Identifier: Apache-2.0
//! The compiled format registry.

use cadmpeg_ir::codec::write::{CadirEncoder, Encoder};
use cadmpeg_ir::codec::{Codec, FormatId};

use crate::{ForcedInput, Format};

type DecoderConstructor = fn() -> Box<dyn Codec>;
pub(crate) type EncoderConstructor = fn() -> Box<dyn Encoder>;

/// Opaque witness that a compiled format has a native decoder.
#[derive(Debug, Clone, Copy)]
pub struct NativeDescriptor {
    id: FormatId,
    input_extensions: &'static [&'static str],
    pub(crate) decoder: DecoderConstructor,
}

impl NativeDescriptor {
    pub(crate) const fn id(&self) -> FormatId {
        self.id
    }

    pub(crate) const fn input_extensions(&self) -> &'static [&'static str] {
        self.input_extensions
    }
}

/// Whether a compiled input is neutral CADIR or has a native decoder.
#[derive(Debug, Clone, Copy)]
pub(crate) enum FormatKind {
    Neutral {
        id: FormatId,
        input_extensions: &'static [&'static str],
    },
    #[allow(dead_code)] // CADIR-only builds construct no native descriptors.
    Native(NativeDescriptor),
}

/// Delivery and semantic-admission physics of a writable format.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum OutputPhysics {
    /// Dialect-free textual CADIR; empty geometry is a valid document state.
    NeutralText,
    /// A textual native geometry format.
    #[cfg(any(feature = "step", feature = "iges"))]
    GeometryText,
    /// A binary native geometry container.
    #[cfg(any(
        feature = "fcstd",
        feature = "f3d",
        feature = "sldprt",
        feature = "rhino"
    ))]
    GeometryBinary,
}

impl OutputPhysics {
    pub(crate) const fn transfers_geometry(self) -> bool {
        match self {
            Self::NeutralText => false,
            #[cfg(any(feature = "step", feature = "iges"))]
            Self::GeometryText => true,
            #[cfg(any(
                feature = "fcstd",
                feature = "f3d",
                feature = "sldprt",
                feature = "rhino"
            ))]
            Self::GeometryBinary => true,
        }
    }

    pub(crate) const fn is_binary(self) -> bool {
        match self {
            Self::NeutralText => false,
            #[cfg(any(feature = "step", feature = "iges"))]
            Self::GeometryText => false,
            #[cfg(any(
                feature = "fcstd",
                feature = "f3d",
                feature = "sldprt",
                feature = "rhino"
            ))]
            Self::GeometryBinary => true,
        }
    }
}

/// Facts that exist together only when a format is writable.
#[derive(Debug)]
pub(crate) struct OutputDescriptor {
    pub extensions: &'static [&'static str],
    pub physics: OutputPhysics,
    pub encoder: EncoderConstructor,
}

/// One compiled format and all registration facts owned by the registry.
#[derive(Debug)]
pub struct FormatDescriptor {
    pub(crate) kind: FormatKind,
}

impl FormatDescriptor {
    /// Stable format identifier.
    pub const fn id(&self) -> FormatId {
        match &self.kind {
            FormatKind::Neutral { id, .. } => *id,
            FormatKind::Native(native) => native.id,
        }
    }

    /// Recognized lowercase filename extensions.
    pub const fn input_extensions(&self) -> &'static [&'static str] {
        match &self.kind {
            FormatKind::Neutral {
                input_extensions, ..
            } => input_extensions,
            FormatKind::Native(native) => native.input_extensions,
        }
    }

    fn forced_input(&'static self) -> ForcedInput {
        match &self.kind {
            FormatKind::Neutral { .. } => ForcedInput::Cadir,
            FormatKind::Native(native) => ForcedInput::Codec(native),
        }
    }
}

#[cfg(any(
    feature = "inventor",
    feature = "catia",
    feature = "creo",
    feature = "nx",
    feature = "sat"
))]
macro_rules! reader {
    ($name:ident, $id:expr, $input_exts:expr, $decoder:expr) => {
        static $name: FormatDescriptor = FormatDescriptor {
            kind: FormatKind::Native(NativeDescriptor {
                id: $id,
                input_extensions: $input_exts,
                decoder: $decoder,
            }),
        };
    };
}

#[cfg(any(
    feature = "fcstd",
    feature = "f3d",
    feature = "sldprt",
    feature = "rhino",
    feature = "step",
    feature = "iges"
))]
macro_rules! writable {
    ($name:ident, $output:ident, $id:expr, $input_exts:expr, $decoder:expr, $output_exts:expr, $physics:expr, $encoder:expr) => {
        static $output: OutputDescriptor = OutputDescriptor {
            extensions: $output_exts,
            physics: $physics,
            encoder: $encoder,
        };
        static $name: FormatDescriptor = FormatDescriptor {
            kind: FormatKind::Native(NativeDescriptor {
                id: $id,
                input_extensions: $input_exts,
                decoder: $decoder,
            }),
        };
    };
}

#[cfg(feature = "fcstd")]
writable!(
    FCSTD,
    FCSTD_OUTPUT,
    FormatId::new("fcstd"),
    &["fcstd"],
    || Box::new(cadmpeg_codec_freecad::FcstdCodec),
    &["fcstd"],
    OutputPhysics::GeometryBinary,
    || Box::new(cadmpeg_codec_freecad::FcstdCodec)
);
#[cfg(feature = "f3d")]
writable!(
    F3D,
    F3D_OUTPUT,
    FormatId::new("f3d"),
    &["f3d", "f3z"],
    || Box::new(cadmpeg_codec_f3d::F3dCodec),
    &["f3d"],
    OutputPhysics::GeometryBinary,
    || Box::new(cadmpeg_codec_f3d::F3dCodec)
);
#[cfg(feature = "inventor")]
reader!(INVENTOR, FormatId::new("inventor"), &["ipt", "iam"], || {
    Box::new(cadmpeg_codec_inventor::InventorCodec)
});
#[cfg(feature = "sldprt")]
writable!(
    SLDPRT,
    SLDPRT_OUTPUT,
    FormatId::new("sldprt"),
    &["sldprt"],
    || Box::new(cadmpeg_codec_sldprt::SldprtCodec),
    &["sldprt"],
    OutputPhysics::GeometryBinary,
    || Box::new(cadmpeg_codec_sldprt::SldprtCodec)
);
#[cfg(feature = "catia")]
reader!(CATIA, FormatId::new("catia"), &["catpart"], || Box::new(
    cadmpeg_codec_catia::CatiaCodec
));
#[cfg(feature = "creo")]
reader!(CREO, FormatId::new("creo"), &["prt"], || Box::new(
    cadmpeg_codec_creo::CreoCodec
));
#[cfg(feature = "nx")]
reader!(NX, FormatId::new("nx"), &["prt"], || Box::new(
    cadmpeg_codec_nx::NxCodec
));
#[cfg(feature = "rhino")]
writable!(
    RHINO,
    RHINO_OUTPUT,
    FormatId::new("rhino"),
    &["3dm"],
    || Box::new(cadmpeg_codec_rhino::RhinoCodec),
    &["3dm"],
    OutputPhysics::GeometryBinary,
    || Box::new(cadmpeg_codec_rhino::RhinoCodec)
);
#[cfg(feature = "step")]
writable!(
    STEP,
    STEP_OUTPUT,
    FormatId::new("step"),
    &["step", "stp"],
    || Box::new(cadmpeg_codec_step::StepCodec::default()),
    &["step", "stp"],
    OutputPhysics::GeometryText,
    || Box::new(cadmpeg_codec_step::StepCodec::default())
);
#[cfg(feature = "iges")]
writable!(
    IGES,
    IGES_OUTPUT,
    FormatId::new("iges"),
    &["iges", "igs"],
    || Box::new(cadmpeg_codec_iges::IgesCodec),
    &["iges", "igs"],
    OutputPhysics::GeometryText,
    || Box::new(cadmpeg_codec_iges::IgesCodec)
);
#[cfg(feature = "sat")]
reader!(
    SAT,
    FormatId::new("sat"),
    &["sat", "sab", "smt", "smb"],
    || Box::new(cadmpeg_codec_sat::SatCodec)
);
static CADIR_OUTPUT: OutputDescriptor = OutputDescriptor {
    extensions: &["cadir", "json"],
    physics: OutputPhysics::NeutralText,
    encoder: || Box::new(CadirEncoder),
};
pub(crate) static CADIR: FormatDescriptor = FormatDescriptor {
    kind: FormatKind::Neutral {
        id: FormatId::new("cadir"),
        input_extensions: &["cadir", "json"],
    },
};
pub(crate) static FORMAT_DESCRIPTORS: &[&FormatDescriptor] = &[
    #[cfg(feature = "fcstd")]
    &FCSTD,
    #[cfg(feature = "f3d")]
    &F3D,
    #[cfg(feature = "inventor")]
    &INVENTOR,
    #[cfg(feature = "sldprt")]
    &SLDPRT,
    #[cfg(feature = "catia")]
    &CATIA,
    #[cfg(feature = "creo")]
    &CREO,
    #[cfg(feature = "nx")]
    &NX,
    #[cfg(feature = "rhino")]
    &RHINO,
    #[cfg(feature = "step")]
    &STEP,
    #[cfg(feature = "iges")]
    &IGES,
    #[cfg(feature = "sat")]
    &SAT,
    &CADIR,
];

impl Format {
    /// The compiled descriptor pair behind a writable format. Total: every
    /// variant names its statics, so no lookup can miss.
    pub(crate) fn descriptor(self) -> (&'static FormatDescriptor, &'static OutputDescriptor) {
        match self {
            Self::Cadir => (&CADIR, &CADIR_OUTPUT),
            #[cfg(feature = "step")]
            Self::Step => (&STEP, &STEP_OUTPUT),
            #[cfg(feature = "fcstd")]
            Self::Fcstd => (&FCSTD, &FCSTD_OUTPUT),
            #[cfg(feature = "f3d")]
            Self::F3d => (&F3D, &F3D_OUTPUT),
            #[cfg(feature = "sldprt")]
            Self::Sldprt => (&SLDPRT, &SLDPRT_OUTPUT),
            #[cfg(feature = "rhino")]
            Self::Rhino => (&RHINO, &RHINO_OUTPUT),
            #[cfg(feature = "iges")]
            Self::Iges => (&IGES, &IGES_OUTPUT),
        }
    }
}

/// Resolves the CLI's forced-input vocabulary from the compiled descriptors.
#[must_use]
pub fn forced_input(name: &str) -> Option<ForcedInput> {
    let canonical = crate::registry::canonical_format_name(name)?;
    let descriptor = FORMAT_DESCRIPTORS
        .iter()
        .find(|descriptor| canonical == descriptor.id().as_str())?;
    Some(descriptor.forced_input())
}

/// Every forced-input spelling accepted by this build.
pub fn input_names() -> impl Iterator<Item = &'static str> {
    FORMAT_DESCRIPTORS
        .iter()
        .flat_map(|descriptor| crate::registry::format_words(descriptor.id().as_str()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeSet;

    #[test]
    fn compiled_descriptors_have_unique_ids_and_consistent_capabilities() {
        let mut ids = BTreeSet::new();
        for descriptor in FORMAT_DESCRIPTORS.iter() {
            assert!(
                ids.insert(descriptor.id()),
                "duplicate {} descriptor",
                descriptor.id()
            );
            assert!(
                crate::registry::format_words(descriptor.id().as_str())
                    .next()
                    .is_some(),
                "{} has no identity-registry format name",
                descriptor.id()
            );
            assert!(
                !descriptor.input_extensions().is_empty(),
                "{} has no input extension",
                descriptor.id()
            );
            if let Some(format) = Format::from_name(descriptor.id().as_str()) {
                let output = format.descriptor().1;
                assert!(!output.extensions.is_empty());
                assert_eq!(
                    crate::registry::canonical_format_name(descriptor.id().as_str()),
                    Some(descriptor.id().as_str()),
                    "{} output format is absent from docs/dialects.toml",
                    descriptor.id()
                );
            }
        }
    }

    #[test]
    fn every_registry_word_resolves_through_its_descriptor() {
        for descriptor in FORMAT_DESCRIPTORS.iter() {
            let expected = descriptor.forced_input();
            for name in crate::registry::format_words(descriptor.id().as_str()) {
                assert_eq!(forced_input(name), Some(expected), "{name}");
            }
        }
    }

    #[test]
    fn input_names_are_unique() {
        let mut names = BTreeSet::new();
        for name in input_names() {
            assert!(names.insert(name), "duplicate input format word {name:?}");
        }
    }
}
