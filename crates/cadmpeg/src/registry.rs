// SPDX-License-Identifier: Apache-2.0
//! Codec registry and format detection.

use cadmpeg_codec_catia::CatiaCodec;
use cadmpeg_codec_creo::CreoCodec;
use cadmpeg_codec_f3d::F3dCodec;
use cadmpeg_codec_nx::NxCodec;
use cadmpeg_codec_sldprt::SldprtCodec;
use cadmpeg_ir::codec::{Codec, Confidence};

/// The set of codecs the CLI knows about.
pub struct Registry {
    codecs: Vec<Box<dyn Codec>>,
}

impl Registry {
    /// Registry with every built-in codec.
    pub fn with_builtins() -> Self {
        Registry {
            codecs: vec![
                Box::new(F3dCodec),
                Box::new(SldprtCodec),
                Box::new(CatiaCodec),
                Box::new(CreoCodec),
                Box::new(NxCodec),
            ],
        }
    }

    /// The codec with the highest detection confidence for `prefix`, if any
    /// codec is more than [`Confidence::No`].
    pub fn detect<'a>(&'a self, prefix: &[u8]) -> Option<(&'a dyn Codec, Confidence)> {
        // `max_by_key` selects the last registered codec on a tie. Built-in
        // magic values do not overlap; keep tie resolution deterministic.
        self.codecs
            .iter()
            .map(|c| (c.as_ref(), c.detect(prefix)))
            .filter(|(_, conf)| *conf > Confidence::No)
            .max_by_key(|(_, conf)| *conf)
    }

    /// Find a codec by its stable format id.
    pub fn by_id(&self, id: &str) -> Option<&dyn Codec> {
        self.codecs
            .iter()
            .find(|codec| codec.id() == id)
            .map(Box::as_ref)
    }
}
