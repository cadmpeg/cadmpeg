// SPDX-License-Identifier: Apache-2.0
//! The compiled format registry.

use cadmpeg_ir::codec::write::{CadirEncoder, Encoder};
use cadmpeg_ir::codec::{Codec, FormatId};

use crate::{ForcedInput, Format};

type DecoderConstructor = fn() -> Box<dyn Codec>;
type EncoderConstructor = fn() -> Box<dyn Encoder>;
type ScanConstructor = fn(
    cadmpeg_core::decode::View<'_>,
    &cadmpeg_core::scan::ScanBudget<'_>,
) -> cadmpeg_core::scan::DesignScan;
type CompoundScanProbe = fn(
    &[cadmpeg_container::compound::scan::ScanCompoundEntry],
    &cadmpeg_core::scan::ScanBudget<'_>,
) -> Result<bool, cadmpeg_core::scan::StopReason>;

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
    feature = "sat",
    feature = "pvz",
    feature = "usd",
    feature = "solidedge",
    feature = "edrawings",
    feature = "onshape"
))]
macro_rules! scan_backend {
    (F3D) => {
        Some(cadmpeg_codec_f3d::F3dCodec::scan_design_with_budget)
    };
    (SLDPRT) => {
        Some(cadmpeg_codec_sldprt::SldprtCodec::scan_design_with_budget)
    };
    (INVENTOR) => {
        Some(cadmpeg_codec_inventor::InventorCodec::scan_design_with_budget)
    };
    ($name:ident) => {
        None
    };
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
    feature = "sat",
    feature = "pvz",
    feature = "usd",
    feature = "solidedge",
    feature = "edrawings",
    feature = "onshape"
))]
macro_rules! compound_scan_probe {
    (SLDPRT) => {
        Some(cadmpeg_codec_sldprt::SldprtCodec::scan_detect_compound)
    };
    (INVENTOR) => {
        Some(cadmpeg_codec_inventor::InventorCodec::scan_detect_compound)
    };
    ($name:ident) => {
        None
    };
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
    feature = "sat",
    feature = "pvz",
    feature = "usd",
    feature = "solidedge",
    feature = "edrawings",
    feature = "onshape"
))]
macro_rules! zip_scan_probe {
    (FCSTD) => {
        Some(|name: &str| name == "Document.xml")
    };
    (F3D) => {
        Some(|name: &str| {
            [
                "Breps.BlobParts",
                "FusionAssetName",
                "FusionDocType",
                ".smbh",
                "Manifest.json",
                "DesignDescription.json",
                ".f3d",
            ]
            .iter()
            .any(|marker| name.contains(marker))
        })
    };
    (PVZ) => {
        Some(|name: &str| {
            name.rsplit_once('.')
                .is_some_and(|(_, extension)| extension.eq_ignore_ascii_case("pvs"))
        })
    };
    (USD) => {
        Some(|name: &str| {
            name.rsplit_once('.').is_some_and(|(_, extension)| {
                ["usd", "usda", "usdc"]
                    .iter()
                    .any(|kind| extension.eq_ignore_ascii_case(kind))
            })
        })
    };
    ($name:ident) => {
        None
    };
}

/// Opaque witness that a compiled format has a native decoder.
#[derive(Debug, Clone, Copy)]
pub struct NativeDescriptor {
    id: FormatId,
    input_extensions: &'static [&'static str],
    pub(crate) decoder: DecoderConstructor,
    pub(crate) scanner: Option<ScanConstructor>,
    pub(crate) zip_scan_probe: Option<fn(&str) -> bool>,
    pub(crate) compound_scan_probe: Option<CompoundScanProbe>,
}

impl NativeDescriptor {
    /// Stable native format identifier.
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
    #[cfg(any(
        feature = "fcstd",
        feature = "f3d",
        feature = "solidedge",
        feature = "inventor",
        feature = "sldprt",
        feature = "catia",
        feature = "creo",
        feature = "nx",
        feature = "rhino",
        feature = "step",
        feature = "iges",
        feature = "sat",
        feature = "pvz",
        feature = "edrawings",
        feature = "usd",
        feature = "onshape"
    ))]
    Native(NativeDescriptor),
}

/// Delivery form of a writable format.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum OutputPhysics {
    /// Text output.
    Text,
    /// A binary container.
    #[cfg(any(
        feature = "fcstd",
        feature = "f3d",
        feature = "sldprt",
        feature = "rhino"
    ))]
    Binary,
}

impl OutputPhysics {
    pub(crate) const fn is_binary(self) -> bool {
        match self {
            Self::Text => false,
            #[cfg(any(
                feature = "fcstd",
                feature = "f3d",
                feature = "sldprt",
                feature = "rhino"
            ))]
            Self::Binary => true,
        }
    }
}

/// Facts that exist together only when a format is writable.
#[derive(Debug)]
pub(crate) struct OutputDescriptor {
    pub(crate) extensions: &'static [&'static str],
    /// The dialect that an output extension names, for the extensions that
    /// name one.
    pub(crate) extension_dialects: &'static [(&'static str, &'static str)],
    pub(crate) physics: OutputPhysics,
    pub(crate) encoder: EncoderConstructor,
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
            #[cfg(any(
                feature = "fcstd",
                feature = "f3d",
                feature = "solidedge",
                feature = "inventor",
                feature = "sldprt",
                feature = "catia",
                feature = "creo",
                feature = "nx",
                feature = "rhino",
                feature = "step",
                feature = "iges",
                feature = "sat",
                feature = "pvz",
                feature = "edrawings",
                feature = "usd",
                feature = "onshape"
            ))]
            FormatKind::Native(native) => native.id,
        }
    }

    /// Recognized lowercase filename extensions.
    pub const fn input_extensions(&self) -> &'static [&'static str] {
        match &self.kind {
            FormatKind::Neutral {
                input_extensions, ..
            } => input_extensions,
            #[cfg(any(
                feature = "fcstd",
                feature = "f3d",
                feature = "solidedge",
                feature = "inventor",
                feature = "sldprt",
                feature = "catia",
                feature = "creo",
                feature = "nx",
                feature = "rhino",
                feature = "step",
                feature = "iges",
                feature = "sat",
                feature = "pvz",
                feature = "edrawings",
                feature = "usd",
                feature = "onshape"
            ))]
            FormatKind::Native(native) => native.input_extensions,
        }
    }

    fn forced_input(&'static self) -> ForcedInput {
        match &self.kind {
            FormatKind::Neutral { .. } => ForcedInput::Cadir,
            #[cfg(any(
                feature = "fcstd",
                feature = "f3d",
                feature = "solidedge",
                feature = "inventor",
                feature = "sldprt",
                feature = "catia",
                feature = "creo",
                feature = "nx",
                feature = "rhino",
                feature = "step",
                feature = "iges",
                feature = "sat",
                feature = "pvz",
                feature = "edrawings",
                feature = "usd",
                feature = "onshape"
            ))]
            FormatKind::Native(native) => ForcedInput::Codec(native),
        }
    }
}

#[cfg(any(
    feature = "solidedge",
    feature = "inventor",
    feature = "catia",
    feature = "creo",
    feature = "nx",
    feature = "sat",
    feature = "pvz",
    feature = "edrawings",
    feature = "onshape"
))]
macro_rules! reader {
    ($name:ident, $id:expr, $input_exts:expr, $decoder:expr) => {
        static $name: FormatDescriptor = FormatDescriptor {
            kind: FormatKind::Native(NativeDescriptor {
                id: $id,
                input_extensions: $input_exts,
                decoder: $decoder,
                scanner: scan_backend!($name),
                zip_scan_probe: zip_scan_probe!($name),
                compound_scan_probe: compound_scan_probe!($name),
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
    feature = "iges",
    feature = "usd"
))]
macro_rules! writable {
    ($name:ident, $output:ident, $id:expr, $input_exts:expr, $decoder:expr, $output_exts:expr, $physics:expr, $encoder:expr) => {
        writable!(
            $name,
            $output,
            $id,
            $input_exts,
            $decoder,
            $output_exts,
            $physics,
            $encoder,
            &[]
        );
    };
    ($name:ident, $output:ident, $id:expr, $input_exts:expr, $decoder:expr, $output_exts:expr, $physics:expr, $encoder:expr, $extension_dialects:expr) => {
        static $output: OutputDescriptor = OutputDescriptor {
            extensions: $output_exts,
            extension_dialects: $extension_dialects,
            physics: $physics,
            encoder: $encoder,
        };
        static $name: FormatDescriptor = FormatDescriptor {
            kind: FormatKind::Native(NativeDescriptor {
                id: $id,
                input_extensions: $input_exts,
                decoder: $decoder,
                scanner: scan_backend!($name),
                zip_scan_probe: zip_scan_probe!($name),
                compound_scan_probe: compound_scan_probe!($name),
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
    OutputPhysics::Binary,
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
    OutputPhysics::Binary,
    || Box::new(cadmpeg_codec_f3d::F3dCodec)
);
#[cfg(feature = "solidedge")]
reader!(
    SOLIDEDGE,
    FormatId::new("solidedge"),
    &["par", "psm", "asm"],
    || Box::new(cadmpeg_codec_solidedge::SolidEdgeCodec)
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
    OutputPhysics::Binary,
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
    OutputPhysics::Binary,
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
    OutputPhysics::Text,
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
    OutputPhysics::Text,
    || Box::new(cadmpeg_codec_iges::IgesCodec)
);
#[cfg(feature = "sat")]
reader!(
    SAT,
    FormatId::new("sat"),
    &["sat", "sab", "smt", "smb"],
    || Box::new(cadmpeg_codec_sat::SatCodec)
);
#[cfg(feature = "pvz")]
reader!(PVZ, FormatId::new("pvz"), &["pvz"], || Box::new(
    cadmpeg_codec_pvz::PvzCodec
));
#[cfg(feature = "edrawings")]
reader!(
    EDRAWINGS,
    FormatId::new("edrawings"),
    &["easm", "eprt"],
    || Box::new(cadmpeg_codec_edrawings::EdrawingsCodec)
);
#[cfg(feature = "onshape")]
reader!(
    ONSHAPE,
    FormatId::new("onshape"),
    &["onshape-bundle"],
    || Box::new(cadmpeg_codec_onshape::OnshapeCodec::new(|ctx, view| {
        cadmpeg_ir::codec::CodecBackend::decode_impl(
            &cadmpeg_codec_step::StepCodec::default(),
            ctx,
            view,
        )
    }))
);
#[cfg(feature = "usd")]
writable!(
    USD,
    USD_OUTPUT,
    FormatId::new("usd"),
    &["usd", "usda", "usdc", "usdz"],
    || Box::new(cadmpeg_codec_usd::UsdCodec),
    &["usda", "usd", "usdz", "usdc"],
    OutputPhysics::Text,
    || Box::new(cadmpeg_codec_usd::UsdCodec),
    &[("usdz", "usdz"), ("usdc", "usdc")]
);
static CADIR_OUTPUT: OutputDescriptor = OutputDescriptor {
    extensions: &["cadir", "json"],
    extension_dialects: &[],
    physics: OutputPhysics::Text,
    encoder: || Box::new(CadirEncoder),
};
static CADIR: FormatDescriptor = FormatDescriptor {
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
    #[cfg(feature = "solidedge")]
    &SOLIDEDGE,
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
    #[cfg(feature = "pvz")]
    &PVZ,
    #[cfg(feature = "edrawings")]
    &EDRAWINGS,
    #[cfg(feature = "usd")]
    &USD,
    #[cfg(feature = "onshape")]
    &ONSHAPE,
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
            #[cfg(feature = "usd")]
            Self::Usd => (&USD, &USD_OUTPUT),
        }
    }
}

/// Resolves the CLI's forced-input vocabulary from the compiled descriptors.
pub fn forced_input(name: &str) -> Result<Option<ForcedInput>, crate::registry::RegistryLoadError> {
    let canonical = crate::registry::canonical_format_name(name)?;
    Ok(canonical.and_then(|canonical| {
        FORMAT_DESCRIPTORS
            .iter()
            .find(|descriptor| canonical == descriptor.id().as_str())
            .map(|descriptor| descriptor.forced_input())
    }))
}

/// Every forced-input spelling accepted by this build.
pub fn input_names() -> Result<Vec<&'static str>, crate::registry::RegistryLoadError> {
    let mut names = Vec::new();
    for descriptor in FORMAT_DESCRIPTORS {
        names.extend(crate::registry::format_words(descriptor.id().as_str())?);
    }
    Ok(names)
}

#[cfg(test)]
mod tests {
    use super::{forced_input, input_names, FORMAT_DESCRIPTORS};
    use crate::Format;
    use std::collections::BTreeSet;

    #[test]
    fn compiled_descriptors_have_unique_ids_and_consistent_capabilities() {
        for format in Format::all() {
            assert!(
                FORMAT_DESCRIPTORS
                    .iter()
                    .any(|descriptor| std::ptr::eq(*descriptor, format.descriptor().0)),
                "{format:?} is absent from FORMAT_DESCRIPTORS"
            );
        }
        let mut ids = BTreeSet::new();
        for descriptor in FORMAT_DESCRIPTORS {
            assert!(
                ids.insert(descriptor.id()),
                "duplicate {} descriptor",
                descriptor.id()
            );
            assert!(
                crate::registry::format_words(descriptor.id().as_str())
                    .expect("embedded registry loads")
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
            if let Some(format) =
                Format::from_name(descriptor.id().as_str()).expect("embedded registry loads")
            {
                let output = format.descriptor().1;
                assert!(!output.extensions.is_empty());
                assert_eq!(
                    crate::registry::canonical_format_name(descriptor.id().as_str())
                        .expect("embedded registry loads"),
                    Some(descriptor.id().as_str()),
                    "{} output format is absent from crates/cadmpeg-registry/docs/dialects.toml",
                    descriptor.id()
                );
            }
        }
    }

    #[test]
    fn every_registry_word_resolves_through_its_descriptor() {
        for descriptor in FORMAT_DESCRIPTORS {
            let expected = descriptor.forced_input();
            for name in crate::registry::format_words(descriptor.id().as_str())
                .expect("embedded registry loads")
            {
                assert_eq!(
                    forced_input(name).expect("embedded registry loads"),
                    Some(expected),
                    "{name}"
                );
            }
        }
    }

    #[test]
    fn input_names_are_unique() {
        let mut names = BTreeSet::new();
        for name in input_names().expect("embedded registry loads") {
            assert!(names.insert(name), "duplicate input format word {name:?}");
        }
    }
}
