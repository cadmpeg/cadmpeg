// SPDX-License-Identifier: Apache-2.0
//! Built-in native codecs and content-based format detection.

use cadmpeg_codec_catia::CatiaCodec;
use cadmpeg_codec_creo::CreoCodec;
use cadmpeg_codec_f3d::F3dCodec;
use cadmpeg_codec_freecad::FcstdCodec;
use cadmpeg_codec_iges::IgesCodec;
use cadmpeg_codec_nx::NxCodec;
use cadmpeg_codec_rhino::RhinoCodec;
use cadmpeg_codec_sldprt::SldprtCodec;
use cadmpeg_ir::codec::{CadirEncoder, Codec, CodecError, Confidence, Encoder};
use cadmpeg_ir::document::CadIr;
use cadmpeg_ir::report::ExportReport;
use cadmpeg_ir::Finding;
use cadmpeg_ir::SourceFidelity;
use cadmpeg_step::StepCodec;

/// Native codecs available to the CLI.
pub struct Registry {
    codecs: Vec<Box<dyn Codec>>,
    encoders: Vec<Box<dyn Encoder>>,
}

impl Registry {
    /// Create a registry containing every native codec shipped with the CLI.
    pub fn with_builtins() -> Self {
        Registry {
            codecs: vec![
                Box::new(FcstdCodec),
                Box::new(F3dCodec),
                Box::new(SldprtCodec),
                Box::new(CatiaCodec),
                Box::new(CreoCodec),
                Box::new(NxCodec),
                Box::new(RhinoCodec),
                Box::new(StepCodec::default()),
                Box::new(IgesCodec),
            ],
            encoders: vec![
                Box::new(FcstdCodec),
                Box::new(F3dCodec),
                Box::new(SldprtCodec),
                Box::new(RhinoCodec),
                Box::new(StepCodec::default()),
                Box::new(CadirEncoder),
            ],
        }
    }

    /// Return the strongest codec match above [`Confidence::No`].
    pub fn detect<'a>(&'a self, prefix: &[u8]) -> Option<(&'a dyn Codec, Confidence)> {
        // Later codecs have explicit precedence when generic container
        // signatures tie. This preserves F3D routing for marker-less ZIP prefixes.
        self.codecs
            .iter()
            .map(|c| (c.as_ref(), c.detect(prefix)))
            .filter(|(_, conf)| *conf > Confidence::No)
            .max_by_key(|(_, confidence)| *confidence)
    }

    /// Return the codec with the given stable format identifier.
    pub fn by_id(&self, id: &str) -> Option<&dyn Codec> {
        self.codecs
            .iter()
            .find(|codec| codec.id() == id)
            .map(Box::as_ref)
    }

    /// Collect native-namespace findings from every registered codec.
    pub fn native_findings(&self, ir: &CadIr) -> Vec<Finding> {
        self.codecs
            .iter()
            .flat_map(|codec| codec.validate_native(ir))
            .collect()
    }

    /// Return the encoder with the given stable output-format identifier.
    pub fn encoder_by_id(&self, id: &str) -> Option<&dyn Encoder> {
        self.encoders
            .iter()
            .find(|encoder| encoder.id() == id)
            .map(Box::as_ref)
    }

    /// Replace the STEP encoder configuration used by subsequent exports.
    pub fn set_step_options(&mut self, options: cadmpeg_step::StepWriteOptions) {
        if let Some(encoder) = self
            .encoders
            .iter_mut()
            .find(|encoder| encoder.id() == "step")
        {
            *encoder = Box::new(StepCodec { options });
        }
    }

    /// Encode through the registered format path with optional target selection.
    pub fn encode_by_id(
        &self,
        id: &str,
        rhino_version: Option<cadmpeg_codec_rhino::RhinoArchiveVersion>,
        ir: &CadIr,
        source_fidelity: Option<&SourceFidelity>,
        output: &mut dyn std::io::Write,
    ) -> Option<Result<ExportReport, CodecError>> {
        if rhino_version.is_some() && id != "rhino" {
            return Some(Err(CodecError::Malformed(
                "Rhino archive version requires the Rhino encoder".into(),
            )));
        }
        if id == "rhino" {
            if let Some(version) = rhino_version {
                return Some(
                    cadmpeg_codec_rhino::RhinoEncoder::new(version).encode_with_source_fidelity(
                        ir,
                        source_fidelity,
                        output,
                    ),
                );
            }
        }
        self.encoder_by_id(id)
            .map(|encoder| encoder.encode_with_source_fidelity(ir, source_fidelity, output))
    }
}

#[cfg(test)]
mod tests {
    use super::Registry;
    use crate::Format;

    #[test]
    fn every_exportable_format_has_an_encoder() {
        let registry = Registry::with_builtins();
        for format in [
            Format::Cadir,
            Format::Step,
            Format::Fcstd,
            Format::F3d,
            Format::Sldprt,
            Format::Rhino,
        ] {
            assert!(
                registry.encoder_by_id(format.name()).is_some(),
                "{}",
                format.name()
            );
        }
    }

    #[test]
    fn ambiguous_zip_uses_last_registered_codec_precedence() {
        let registry = Registry::with_builtins();
        let (codec, confidence) = registry
            .detect(b"PK\x03\x04 markerless")
            .expect("required invariant");
        assert_eq!(codec.id(), "f3d");
        assert_eq!(confidence, cadmpeg_ir::codec::Confidence::Low);
    }

    #[test]
    fn step_is_registered_as_a_reader() {
        let registry = Registry::with_builtins();
        let (codec, confidence) = registry
            .detect(b"ISO-10303-21;HEADER;")
            .expect("STEP codec detection");
        assert_eq!(codec.id(), "step");
        assert_eq!(confidence, cadmpeg_ir::codec::Confidence::High);
        assert_eq!(registry.by_id("step").expect("STEP reader").id(), "step");
    }
}
