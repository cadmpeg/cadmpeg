# cadmpeg-codec-sat

`cadmpeg-codec-sat` decodes bare Autodesk ShapeManager and admitted Spatial
ACIS B-rep streams outside any container. It accepts binary SAB streams in
`.smb`/`.smbh`-style files, text SAT/SMT streams, and the ACIS 217/218 binary
branch. It transfers supported records through [`cadmpeg-asm`][asm] into
[`cadmpeg-ir`][ir]. The stream content selects the decoder; file extensions do
not.

<!-- generated: capability -->

Support: depth none, breadth n/a ([ladder](https://github.com/cadmpeg/cadmpeg/blob/main/docs/format-support.md#asmacis-bare-satsmtsmbsab-streams)).

<!-- /generated: capability -->

The codec is read-only. It does not read `.f3d` or Inventor containers, and it
does not encode SAT/SAB output.

## Install

```sh
cargo add cadmpeg-codec-sat cadmpeg-ir
```

## Decode a stream

```rust,no_run
use std::fs::File;

use cadmpeg_codec_sat::SatCodec;
use cadmpeg_ir::{Codec, DecodeOptions};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut input = File::open("body.smb")?;
    let decoded = SatCodec.decode(&mut input, &DecodeOptions::default())?;

    println!(
        "{} bodies, {} faces, geometry_transferred={}",
        decoded.ir().model.bodies.len(),
        decoded.ir().model.faces.len(),
        decoded.report().geometry_transferred,
    );
    for loss in &decoded.report().losses {
        eprintln!("{:?}: {}", loss.severity, loss.message);
    }
    Ok(())
}
```

Read [`DecodeReport::losses`][decode-report] before trusting geometry. A
framed stream can still contain an unsupported kernel branch or records that
produce no transferred geometry. Such results retain source metadata and
unknown records while reporting a blocking `geometry_not_transferred` loss.

`SatCodec::inspect` reports the stream kind, kernel header facts, record count,
scale, and text terminator without building the neutral model. Detection is
content-based: binary ASM and ACIS magic produce high confidence, while a
structurally plausible text header produces medium confidence because numeric
text is not unique to SAT/SMT.

## Input branches

| Stream | Detection | Decode path |
| --- | --- | --- |
| `ASM BinaryFile4` | High | Fixed ASM header, SAB framing at the declared reference width, solved-record transfer. |
| `ASM BinaryFile8` | High | Fixed ASM header, 64-bit entity/reference fields, SAB framing, solved-record transfer. |
| `ACIS BinaryFile` save-format 217 or 218 | High | 32-bit ACIS header, four-byte SAB framing, solved-record transfer. |
| Text ASM SAT/SMT | Medium | Four-word ASCII header, counted strings and numeric records, `End-of-ASM-data` termination. |
| Text ACIS SAT/SMT | Medium | Four-word ASCII header, numeric records, `End-of-ACIS-data` termination. |

ASM headers select the integer/reference width and expose save format, entity
count, flags, product strings, save date, scale, and tolerance fields. A
history-bearing stream declares the boundary between solved records and
construction history; the model transfer reads the solved partition and keeps
the remaining record identity as native data where available.

ACIS 217/218 uses the supported 32-bit header and the same SAB record decoder.
Other ACIS binary save-format bands are identified during inspection but are
not decoded; they return a blocking geometry loss with the header facts in
source metadata.

Text streams use the line-oriented SAT/SMT grammar. Their header scale is
applied by the shared ASM transfer path, and the terminal line determines the
ASM or ACIS dialect. A numeric line that does not contain the complete SAT/SMT
opening is not accepted as a text stream.

## SAB framing and transfer

Binary streams frame records from the ASM header's record-stream boundary and
reference width. The framer recognizes nested subtype tokens, record names,
scalar values, strings, references, vectors, positions, enums, and integer
values. It stops at the declared solved-record limit or the correct stream
terminator; it does not search payload bytes for a record signature.

The shared kernel decoder builds neutral geometry and retains ASM-native
records under the `sat` namespace. Transfer covers the admitted analytic,
NURBS, topology, placement, and procedural carrier branches documented in
[the ASM format notes][spec]. Unsupported surfaces remain linked opaque
geometry where topology permits. Unknown SAB records retain their source
range, digest, and kernel-qualified identity through `SourceFidelity`.

Model lengths and placements use the scale and unit semantics in the selected
stream header. Header tolerances populate the neutral IR when both linear and
angular values are available. The codec does not infer units from file names or
from record text.

## Documentation

- [API documentation][docs]
- [Format support][support]
- [ASM/SAT format notes][spec]
- [Architecture and crate map][architecture]
- [Clean-room and legal policy][legal]
- [Repository][repo]

Requires Rust 1.88 or later. Licensed under Apache-2.0.

Autodesk, ShapeManager, ACIS, and other product names are trademarks of their
respective owners. cadmpeg uses them only to identify the file formats this
codec targets and is not affiliated with, endorsed by, or sponsored by any CAD
vendor. See the [clean-room and legal policy][legal].

[architecture]: https://github.com/cadmpeg/cadmpeg/blob/main/docs/architecture.md
[asm]: https://docs.rs/cadmpeg-asm
[decode-report]: https://docs.rs/cadmpeg-ir/latest/cadmpeg_ir/report/struct.DecodeReport.html
[docs]: https://docs.rs/cadmpeg-codec-sat
[ir]: https://docs.rs/cadmpeg-ir
[legal]: https://github.com/cadmpeg/cadmpeg/blob/main/LEGAL.md
[repo]: https://github.com/cadmpeg/cadmpeg
[spec]: https://github.com/cadmpeg/cadmpeg/blob/main/docs/formats/asm.md
[support]: https://github.com/cadmpeg/cadmpeg/blob/main/docs/format-support.md
