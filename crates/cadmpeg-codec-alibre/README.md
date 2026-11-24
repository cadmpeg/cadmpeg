# cadmpeg-codec-alibre

Read-only codec for Alibre Design part (`.AD_PRT`) and assembly (`.AD_ASM`) files.

An `.AD_PRT` is a Microsoft Compound File. Its `SAB_DATA` stream holds the part
body as a Spatial ACIS binary stream, which the codec decodes through
`cadmpeg-asm` into the neutral B-rep. An `.AD_ASM` assembly lists its components in
`STEP_DATA`; the codec decodes each component file beside the assembly and places
it as an occurrence.

<!-- generated: capability alibre -->

Support: L3 ([ladder](https://github.com/cadmpeg/cadmpeg/blob/main/docs/format-support.md#alibre-design-adprt-and-adasm)).

<!-- /generated: capability alibre -->

The format specification is `docs/formats/alibre.md` in the cadmpeg repository.

```no_run
use std::fs::File;

use cadmpeg_codec_alibre::AlibreCodec;
use cadmpeg_core::decode::InspectOptions;
use cadmpeg_ir::codec::Codec;

# fn main() -> Result<(), Box<dyn std::error::Error>> {
let mut input = File::open("part.AD_PRT")?;
let summary = AlibreCodec.inspect(&mut input, &InspectOptions::default())?;
for entry in &summary.entries {
    println!("{} {}", entry.role, entry.name);
}
# Ok(())
# }
```
