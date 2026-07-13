// SPDX-License-Identifier: Apache-2.0
//! Built-in native codecs and content-based format detection.

use cadmpeg_codec_catia::CatiaCodec;
use cadmpeg_codec_creo::CreoCodec;
use cadmpeg_codec_f3d::F3dCodec;
use cadmpeg_codec_iges::IgesCodec;
use cadmpeg_codec_nx::NxCodec;
use cadmpeg_codec_rhino::RhinoCodec;
use cadmpeg_codec_sldprt::SldprtCodec;
use cadmpeg_ir::codec::{CadirEncoder, Codec, Confidence, Encoder};
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
                Box::new(F3dCodec),
                Box::new(SldprtCodec),
                Box::new(CatiaCodec),
                Box::new(CreoCodec),
                Box::new(NxCodec),
                Box::new(RhinoCodec),
                Box::new(IgesCodec),
            ],
            encoders: vec![
                Box::new(F3dCodec),
                Box::new(SldprtCodec),
                Box::new(StepCodec::default()),
                Box::new(CadirEncoder),
            ],
        }
    }

    /// Return the strongest codec match above [`Confidence::No`].
    pub fn detect<'a>(&'a self, prefix: &[u8]) -> Option<(&'a dyn Codec, Confidence)> {
        // `max_by_key` selects the last registered codec on a tie. Built-in
        // magic values do not overlap; keep tie resolution deterministic.
        self.codecs
            .iter()
            .map(|c| (c.as_ref(), c.detect(prefix)))
            .filter(|(_, conf)| *conf > Confidence::No)
            .max_by_key(|(_, conf)| *conf)
    }

    /// Return the codec with the given stable format identifier.
    pub fn by_id(&self, id: &str) -> Option<&dyn Codec> {
        self.codecs
            .iter()
            .find(|codec| codec.id() == id)
            .map(Box::as_ref)
    }

    /// Return the encoder with the given stable output-format identifier.
    pub fn encoder_by_id(&self, id: &str) -> Option<&dyn Encoder> {
        self.encoders
            .iter()
            .find(|encoder| encoder.id() == id)
            .map(Box::as_ref)
    }
}

#[cfg(test)]
mod tests {
    use super::Registry;
    use crate::Format;

    #[test]
    fn every_exportable_format_has_an_encoder() {
        let registry = Registry::with_builtins();
        for format in [Format::Cadir, Format::Step, Format::F3d, Format::Sldprt] {
            assert!(
                registry.encoder_by_id(format.name()).is_some(),
                "{}",
                format.name()
            );
        }
    }
}
