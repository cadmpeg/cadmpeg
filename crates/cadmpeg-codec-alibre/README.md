# cadmpeg-codec-alibre

Read-only codec for Alibre Design part files (`.AD_PRT`).

An `.AD_PRT` is a Microsoft Compound File. Its `SAB_DATA` stream holds the part
body as a Spatial ACIS binary stream, which the codec decodes through
`cadmpeg-asm` into the neutral B-rep.

<!-- generated: capability alibre -->

Support: L3 ([ladder](https://github.com/cadmpeg/cadmpeg/blob/main/docs/format-support.md#alibre-design-adprt)).

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
