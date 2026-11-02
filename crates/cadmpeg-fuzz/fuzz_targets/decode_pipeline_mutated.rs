// SPDX-License-Identifier: Apache-2.0
//! Mutates container bytes, then runs detection, inspection, and decoding with
//! the registered container codecs, including STEP, IGES, SAT, and USD. The first
//! input byte selects deterministic mutation and truncation strategies. Codec
//! errors are expected; panics are failures.

#![no_main]

use std::io::Cursor;

#[cfg(feature = "catia")]
use cadmpeg_codec_catia::CatiaCodec;
#[cfg(feature = "creo")]
use cadmpeg_codec_creo::CreoCodec;
#[cfg(feature = "f3d")]
use cadmpeg_codec_f3d::F3dCodec;
#[cfg(feature = "freecad")]
use cadmpeg_codec_freecad::FcstdCodec;
#[cfg(feature = "iges")]
use cadmpeg_codec_iges::IgesCodec;
#[cfg(feature = "inventor")]
use cadmpeg_codec_inventor::InventorCodec;
#[cfg(feature = "nx")]
use cadmpeg_codec_nx::NxCodec;
#[cfg(feature = "pvz")]
use cadmpeg_codec_pvz::PvzCodec;
#[cfg(feature = "rhino")]
use cadmpeg_codec_rhino::RhinoCodec;
#[cfg(feature = "sat")]
use cadmpeg_codec_sat::SatCodec;
#[cfg(feature = "sldprt")]
use cadmpeg_codec_sldprt::SldprtCodec;
#[cfg(feature = "step")]
use cadmpeg_codec_step::StepCodec;
#[cfg(feature = "usd")]
use cadmpeg_codec_usd::UsdCodec;
use cadmpeg_ir::codec::{Codec, DecodeOptions};

use cadmpeg_core::decode::InspectOptions;
use libfuzzer_sys::fuzz_target;

fn mutate_bytes(data: &[u8], seed: u8) -> Vec<u8> {
    let mut mutated = data.to_vec();
    if mutated.is_empty() {
        return mutated;
    }

    let num_mutations = usize::from(seed % 10) + 1;
    for i in 0..num_mutations {
        let pos = (usize::from(seed).wrapping_mul(i + 1)) % mutated.len();
        match seed % 5 {
            0 => mutated[pos] = mutated[pos].wrapping_add(1),
            1 => mutated[pos] = mutated[pos].wrapping_sub(1),
            2 => mutated[pos] = 0,
            3 => mutated[pos] = 0xff,
            4 => mutated[pos] ^= 0x80,
            _ => {}
        }
    }

    if seed.is_multiple_of(3) && mutated.len() > 10 {
        let truncate_at = (usize::from(seed) % (mutated.len() - 10)) + 10;
        mutated.truncate(truncate_at);
    }

    mutated
}

fuzz_target!(|data: &[u8]| {
    if data.len() < 16 {
        return;
    }

    let seed = data[0];
    let file_data = &data[1..];
    let mutated = mutate_bytes(file_data, seed);

    let codecs: Vec<Box<dyn Codec>> = vec![
        #[cfg(feature = "f3d")]
        Box::new(F3dCodec),
        #[cfg(feature = "inventor")]
        Box::new(InventorCodec),
        #[cfg(feature = "freecad")]
        Box::new(FcstdCodec),
        #[cfg(feature = "sldprt")]
        Box::new(SldprtCodec),
        #[cfg(feature = "catia")]
        Box::new(CatiaCodec),
        #[cfg(feature = "creo")]
        Box::new(CreoCodec),
        #[cfg(feature = "nx")]
        Box::new(NxCodec),
        #[cfg(feature = "rhino")]
        Box::new(RhinoCodec),
        #[cfg(feature = "iges")]
        Box::new(IgesCodec),
        #[cfg(feature = "sat")]
        Box::new(SatCodec),
        #[cfg(feature = "pvz")]
        Box::new(PvzCodec),
        #[cfg(feature = "usd")]
        Box::new(UsdCodec),
        #[cfg(feature = "step")]
        Box::new(StepCodec::default()),
    ];

    for codec in codecs {
        let arena = cadmpeg_core::decode::DecodeArena::new();
        if let Ok((ctx, root)) = cadmpeg_core::decode::DecodeContext::from_root_bytes(
            &mutated,
            &arena,
            &cadmpeg_core::decode::DecodePolicy::service(),
        ) {
            let _ = codec.detect(&ctx, root);
            let _ = ctx.finish_session();
        }

        let mut inspect_cur = Cursor::new(&mutated);
        let _ = codec.inspect(&mut inspect_cur, &InspectOptions::default());

        let mut decode_cur = Cursor::new(&mutated);
        let _ = codec.decode(&mut decode_cur, &DecodeOptions::default());
    }
});
