// SPDX-License-Identifier: Apache-2.0
//! The compiled format registry.

use cadmpeg_ir::codec::{CadirEncoder, Codec, Encoder};

use crate::{ForcedInput, Format};

pub(crate) type DecoderConstructor = fn() -> Box<dyn Codec>;
pub(crate) type EncoderConstructor = fn() -> Box<dyn Encoder>;
/// One compiled format and all registration facts owned by the registry.
pub(crate) struct FormatDescriptor {
    pub id: &'static str,
    pub input_names: &'static [&'static str],
    pub input_extensions: &'static [&'static str],
    pub decoder: Option<DecoderConstructor>,
    pub output: Option<Format>,
    pub output_order: Option<u8>,
    pub output_names: &'static [&'static str],
    pub output_extensions: &'static [&'static str],
    pub encoder: Option<EncoderConstructor>,
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
            output_order: None,
            output_names: &[],
            output_extensions: &[],
            encoder: None,
        }
    };
    ($id:literal, $inputs:expr, $input_exts:expr, $decoder:expr, $format:expr, $order:expr, $outputs:expr, $output_exts:expr, $encoder:expr) => {
        FormatDescriptor {
            id: $id,
            input_names: $inputs,
            input_extensions: $input_exts,
            decoder: Some($decoder),
            output: Some($format),
            output_order: Some($order),
            output_names: $outputs,
            output_extensions: $output_exts,
            encoder: Some($encoder),
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
        &["fcstd"],
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
        &["f3d"],
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
        &["sldprt"],
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
        &["rhino", "3dm"],
        &["3dm"],
        || Box::new(cadmpeg_codec_rhino::RhinoEncoder)
    ),
    #[cfg(feature = "step")]
    descriptor!(
        "step",
        &["step"],
        &["step", "stp"],
        || Box::new(cadmpeg_codec_step::StepCodec::default()),
        Format::Step,
        1,
        &["step"],
        &["step", "stp"],
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
        &["iges", "igs"],
        || Box::new(cadmpeg_codec_iges::IgesEncoder)
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
        output: Some(Format::Cadir),
        output_order: Some(0),
        output_names: &["cadir", "json"],
        output_extensions: &["cadir", "json"],
        encoder: Some(|| Box::new(CadirEncoder)),
    },
];

pub(crate) fn by_output(format: Format) -> &'static FormatDescriptor {
    FORMAT_DESCRIPTORS
        .iter()
        .find(|descriptor| descriptor.output == Some(format))
        .expect("every Format has one descriptor")
}

pub(crate) fn writable() -> impl Iterator<Item = &'static FormatDescriptor> {
    let mut descriptors = FORMAT_DESCRIPTORS
        .iter()
        .filter(|descriptor| descriptor.encoder.is_some())
        .collect::<Vec<_>>();
    descriptors.sort_by_key(|descriptor| descriptor.output_order);
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
            assert_eq!(descriptor.output.is_some(), descriptor.encoder.is_some());
            assert_eq!(
                descriptor.output.is_some(),
                descriptor.output_order.is_some()
            );
            if let Some(format) = descriptor.output {
                assert_eq!(format.name(), descriptor.id);
                assert!(!descriptor.output_names.is_empty());
                assert!(!descriptor.output_extensions.is_empty());
                if format != Format::Cadir {
                    for name in descriptor.output_names {
                        assert!(
                            crate::registry::is_format_name(name),
                            "{} output spelling {name:?} is absent from docs/dialects.toml",
                            descriptor.id
                        );
                    }
                }
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
