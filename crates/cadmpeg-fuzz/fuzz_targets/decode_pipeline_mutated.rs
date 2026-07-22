// SPDX-License-Identifier: Apache-2.0
//! Mutates container bytes, then runs detection, inspection, and decoding with
//! every binary CAD codec. The first input byte selects deterministic mutation
//! and truncation strategies. Codec errors are expected; panics are failures.

#![no_main]

use std::io::Cursor;

use cadmpeg_codec_catia::CatiaCodec;
use cadmpeg_codec_creo::CreoCodec;
use cadmpeg_codec_f3d::F3dCodec;
use cadmpeg_codec_nx::NxCodec;
use cadmpeg_codec_rhino::RhinoCodec;
use cadmpeg_codec_sldprt::SldprtCodec;
use cadmpeg_ir::codec::{Codec, CodecEntry, DecodeOptions};
use cadmpeg_ir::decode::InspectOptions;
use libfuzzer_sys::fuzz_target;

fn mutate_bytes(data: &[u8], seed: u8) -> Vec<u8> {
    let mut mutated = data.to_vec();
    if mutated.is_empty() {
        return mutated;
    }

    let num_mutations = (seed % 10) as usize + 1;
    for i in 0..num_mutations {
        let pos = ((seed as usize).wrapping_mul(i + 1)) % mutated.len();
        match seed % 5 {
            0 => mutated[pos] = mutated[pos].wrapping_add(1),
            1 => mutated[pos] = mutated[pos].wrapping_sub(1),
            2 => mutated[pos] = 0,
            3 => mutated[pos] = 0xff,
            4 => mutated[pos] ^= 0x80,
            _ => {}
        }
    }

    if seed % 3 == 0 && mutated.len() > 10 {
        let truncate_at = (seed as usize % (mutated.len() - 10)) + 10;
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
        Box::new(F3dCodec),
        Box::new(SldprtCodec),
        Box::new(CatiaCodec),
        Box::new(CreoCodec),
        Box::new(NxCodec),
        Box::new(RhinoCodec),
    ];

    for codec in codecs {
        let _ = codec.detect(&mutated);

        let mut inspect_cur = Cursor::new(&mutated);
        let _ = codec.inspect(&mut inspect_cur, &InspectOptions::default());

        let mut decode_cur = Cursor::new(&mutated);
        let _ = codec.decode(&mut decode_cur, &DecodeOptions::default());
    }
});
