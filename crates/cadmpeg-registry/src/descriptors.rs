// SPDX-License-Identifier: Apache-2.0
//! The compiled format registry.

use cadmpeg_ir::codec::{CadirEncoder, Codec, Encoder};

use crate::{ForcedInput, Format};

pub(crate) type DecoderConstructor = fn() -> Box<dyn Codec>;
pub(crate) type EncoderConstructor = fn() -> Box<dyn Encoder>;

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
pub(crate) struct OutputDescriptor {
    pub format: Format,
    pub order: u8,
    pub extensions: &'static [&'static str],
    pub physics: OutputPhysics,
    pub encoder: EncoderConstructor,
}

/// One compiled format and all registration facts owned by the registry.
pub(crate) struct FormatDescriptor {
    pub id: &'static str,
    pub input_names: &'static [&'static str],
    pub input_extensions: &'static [&'static str],
    pub decoder: Option<DecoderConstructor>,
    pub output: Option<OutputDescriptor>,
}

#[cfg(any(
    feature = "fcstd",
    feature = "f3d",
    feature = "inventor",
    feature = "sldprt",
    feature = "catia",
    feature = "creo",
    feature = "nx",
    feature = "rhino",
    feature = "step",
    feature = "iges",
    feature = "sat"
))]
macro_rules! descriptor {
    ($id:literal, $inputs:expr, $input_exts:expr, $decoder:expr) => {
        FormatDescriptor {
            id: $id,
            input_names: $inputs,
            input_extensions: $input_exts,
            decoder: Some($decoder),
            output: None,
        }
    };
    ($id:literal, $inputs:expr, $input_exts:expr, $decoder:expr, $format:expr, $order:expr, $output_exts:expr, $physics:expr, $encoder:expr) => {
        FormatDescriptor {
            id: $id,
            input_names: $inputs,
            input_extensions: $input_exts,
            decoder: Some($decoder),
            output: Some(OutputDescriptor {
                format: $format,
                order: $order,
                extensions: $output_exts,
                physics: $physics,
                encoder: $encoder,
            }),
        }
    };
}

pub(crate) static FORMAT_DESCRIPTORS: &[FormatDescriptor] = &[
    #[cfg(feature = "fcstd")]
    descriptor!(
        "fcstd",
        &["fcstd"],
        &["fcstd"],
        || Box::new(cadmpeg_codec_freecad::FcstdCodec),
        Format::Fcstd,
        2,
        &["fcstd"],
        OutputPhysics::GeometryBinary,
        || Box::new(cadmpeg_codec_freecad::FcstdCodec)
    ),
    #[cfg(feature = "f3d")]
    descriptor!(
        "f3d",
        &["f3d"],
        &["f3d", "f3z"],
        || Box::new(cadmpeg_codec_f3d::F3dCodec),
        Format::F3d,
        3,
        &["f3d"],
        OutputPhysics::GeometryBinary,
        || Box::new(cadmpeg_codec_f3d::F3dCodec)
    ),
    #[cfg(feature = "inventor")]
    descriptor!(
        "inventor",
        &["inventor", "ipt", "iam"],
        &["ipt", "iam"],
        || Box::new(cadmpeg_codec_inventor::InventorCodec)
    ),
    #[cfg(feature = "sldprt")]
    descriptor!(
        "sldprt",
        &["sldprt"],
        &["sldprt"],
        || Box::new(cadmpeg_codec_sldprt::SldprtCodec),
        Format::Sldprt,
        4,
        &["sldprt"],
        OutputPhysics::GeometryBinary,
        || Box::new(cadmpeg_codec_sldprt::SldprtCodec)
    ),
    #[cfg(feature = "catia")]
    descriptor!("catia", &["catpart", "catia"], &["catpart"], || Box::new(
        cadmpeg_codec_catia::CatiaCodec
    )),
    #[cfg(feature = "creo")]
    descriptor!("creo", &["creo"], &["prt"], || Box::new(
        cadmpeg_codec_creo::CreoCodec
    )),
    #[cfg(feature = "nx")]
    descriptor!("nx", &["nx"], &["prt"], || Box::new(
        cadmpeg_codec_nx::NxCodec
    )),
    #[cfg(feature = "rhino")]
    descriptor!(
        "rhino",
        &["rhino", "3dm"],
        &["3dm"],
        || Box::new(cadmpeg_codec_rhino::RhinoCodec),
        Format::Rhino,
        5,
        &["3dm"],
        OutputPhysics::GeometryBinary,
        || Box::new(cadmpeg_codec_rhino::RhinoCodec)
    ),
    #[cfg(feature = "step")]
    descriptor!(
        "step",
        &["step"],
        &["step", "stp"],
        || Box::new(cadmpeg_codec_step::StepCodec::default()),
        Format::Step,
        1,
        &["step", "stp"],
        OutputPhysics::GeometryText,
        || Box::new(cadmpeg_codec_step::StepCodec::default())
    ),
    #[cfg(feature = "iges")]
    descriptor!(
        "iges",
        &["iges", "igs"],
        &["iges", "igs"],
        || Box::new(cadmpeg_codec_iges::IgesCodec),
        Format::Iges,
        6,
        &["iges", "igs"],
        OutputPhysics::GeometryText,
        || Box::new(cadmpeg_codec_iges::IgesCodec)
    ),
    #[cfg(feature = "sat")]
    descriptor!(
        "sat",
        &["sat", "smt", "smb", "sab"],
        &["sat", "sab", "smt", "smb"],
        || Box::new(cadmpeg_codec_sat::SatCodec)
    ),
    FormatDescriptor {
        id: "cadir",
        input_names: &["cadir"],
        input_extensions: &["cadir", "json"],
        decoder: None,
        output: Some(OutputDescriptor {
            format: Format::Cadir,
            order: 0,
            extensions: &["cadir", "json"],
            physics: OutputPhysics::NeutralText,
            encoder: || Box::new(CadirEncoder),
        }),
    },
];

pub(crate) fn by_output(format: Format) -> (&'static FormatDescriptor, &'static OutputDescriptor) {
    FORMAT_DESCRIPTORS
        .iter()
        .find_map(|descriptor| {
            descriptor
                .output
                .as_ref()
                .filter(|output| output.format == format)
                .map(|output| (descriptor, output))
        })
        .expect("every Format has one descriptor")
}

pub(crate) fn writable(
) -> impl Iterator<Item = (&'static FormatDescriptor, &'static OutputDescriptor)> {
    let mut descriptors = FORMAT_DESCRIPTORS
        .iter()
        .filter_map(|descriptor| {
            descriptor
                .output
                .as_ref()
                .map(|output| (descriptor, output))
        })
        .collect::<Vec<_>>();
    descriptors.sort_by_key(|(_, output)| output.order);
    descriptors.into_iter()
}

/// Resolves the CLI's forced-input vocabulary from the compiled descriptors.
#[must_use]
pub fn forced_input(name: &str) -> Option<ForcedInput> {
    let descriptor = FORMAT_DESCRIPTORS
        .iter()
        .find(|descriptor| descriptor.input_names.contains(&name))?;
    Some(if descriptor.decoder.is_some() {
        ForcedInput::Codec(descriptor.id)
    } else {
        ForcedInput::Cadir
    })
}

/// Every forced-input spelling accepted by this build.
pub fn input_names() -> impl Iterator<Item = &'static str> {
    FORMAT_DESCRIPTORS
        .iter()
        .flat_map(|descriptor| descriptor.input_names.iter().copied())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeSet;

    #[test]
    fn compiled_descriptors_have_unique_ids_and_consistent_capabilities() {
        let mut ids = BTreeSet::new();
        for descriptor in FORMAT_DESCRIPTORS {
            assert!(
                ids.insert(descriptor.id),
                "duplicate {} descriptor",
                descriptor.id
            );
            assert!(
                !descriptor.input_names.is_empty(),
                "{} has no input name",
                descriptor.id
            );
            assert!(
                !descriptor.input_extensions.is_empty(),
                "{} has no input extension",
                descriptor.id
            );
            if let Some(output) = &descriptor.output {
                assert_eq!(output.format.name(), descriptor.id);
                assert!(!output.extensions.is_empty());
                assert_eq!(
                    crate::registry::canonical_format_name(descriptor.id),
                    Some(descriptor.id),
                    "{} output format is absent from docs/dialects.toml",
                    descriptor.id
                );
            }
        }
    }

    #[test]
    fn forced_input_names_resolve_through_their_own_descriptor() {
        for descriptor in FORMAT_DESCRIPTORS {
            let expected = if descriptor.decoder.is_some() {
                ForcedInput::Codec(descriptor.id)
            } else {
                ForcedInput::Cadir
            };
            for name in descriptor.input_names {
                assert_eq!(forced_input(name), Some(expected), "{name}");
            }
        }
    }
}
