# cadmpeg-protein

`cadmpeg-protein` is the bounded, schema-driven decoder for Autodesk Protein
asset packages. It reads the XML schemas and paged `InstanceProperties` stream
used by Autodesk Fusion `.f3d` assets and Inventor `.ipt`/`.iam` documents. It
returns typed asset-instance records and preserves connection identifiers for
the format codec that owns appearance or material binding.

This crate is not an outer-container reader. The calling codec selects and
opens the Protein package and supplies the package bytes and the
`InstanceProperties` entry bytes. This crate does not resolve library paths,
decode image files, or infer assignments to bodies and faces.

## Install

```sh
cargo add cadmpeg-protein
```

## Decode an instance stream

Use [`decode_detailed`][decode-detailed] when the caller must account for
records that have valid page framing but invalid headers or property values:

```rust,no_run
use std::fs;

use cadmpeg_protein::decode_detailed;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let protein = fs::read("appearance.protein")?;
    let instance_properties = fs::read("InstanceProperties.bin")?;
    let outcome = decode_detailed(&protein, &instance_properties)?;

    for record in &outcome.records {
        println!(
            "{}: {} properties from {}",
            record.guid,
            record.properties.len(),
            record.schema,
        );
    }
    for rejected in &outcome.rejected {
        eprintln!("record {} rejected: {}", rejected.ordinal, rejected.detail);
    }
    Ok(())
}
```

[`decode`][decode] is the compact form. It returns the valid records and
returns a `CodecError` for invalid package or page framing, but it does not
return the record-level rejection list. Both functions preserve serialized
record order. `DecodedRecord::ordinal` remains the zero-based position from
the paged stream, including rejected records.

Use [`has_schemas`][has-schemas] as a cheap non-throwing probe before decoding
an archive. It returns `false` for invalid ZIP bytes and for valid archives
that contain no recognized schema XML; it is a probe, not a replacement for
`decode` validation.

## Input framing

The `protein` argument is a ZIP archive containing schema XML entries. A schema
entry is a file whose path begins with `Schemas/` or contains `/Schemas/` and
ends with `Schema.xml`. Every schema declares its identifier in the root
`UID` element. Duplicate ZIP entry names and duplicate schema identifiers are
malformed.

The `instance` argument has a 16-byte stream header followed by fixed 136-byte
pages. The first header word is the page size (`0x88`); the remaining header
bytes are retained only by the caller. The numbers are tabulated in
[`docs/layouts/protein.md`][protein-layout]. Each page has a 128-byte body after its
8-byte page header:

| Page         | Marker                      | Contribution                                                                                                      |
| ------------ | --------------------------- | ----------------------------------------------------------------------------------------------------------------- |
| Record start | `80 00 01 00` at bytes 4..8 | Opens a logical record and contributes the complete body at bytes 8..136.                                         |
| Continuation | `80 00 00 00` at bytes 4..8 | Extends the current record with its complete body.                                                                |
| Terminal     | `ff ff ff ff` at bytes 0..4 | Closes the current record; bytes 8..`8 + used` contribute, where `used` is the little-endian `u16` at bytes 4..6. |

A new start page, continuation page, or terminal page without the required
current-record state is invalid. A stream ending with an open record is
valid: the last open record is complete. A valid logical record begins with four length-prefixed UTF-8 strings:
schema identifier, asset GUID, base asset identifier, and `AssetLibID`.

## Schema resolution and values

The decoder follows each schema's `Base` inheritance chain and builds the
effective property set by property identifier. Cycles and missing base
schemas are errors. `PropertyAlias` does not rename a serialized property.
`readonly="true"` and `definitionIteratorData="true"` properties are not part
of the value block. `public="false"` and `metadata` do not suppress a
serialized property. Effective properties are exposed in the
[`DecodedRecord::properties`][decoded-record] `BTreeMap`.

The fourth record-header string is `AssetLibID`, so it is exposed as
[`DecodedRecord::asset_lib_id`][decoded-record] rather than as a property.
The inherited `texture_MapChannel_ID_Advanced` slot is excluded from the
serialized property sequence because the format does not identify which of
two default-equivalent texture slots it represents.

Supported schema carriers map to [`PropertyValue`][property-value] as follows:

| Schema carrier          | Serialized value                            | Public value                                           |
| ----------------------- | ------------------------------------------- | ------------------------------------------------------ |
| `Boolean`               | one byte                                    | `Boolean(bool)`                                        |
| `Integer`, `Choice`     | little-endian `u32`                         | `Integer(u32)`                                         |
| `Float`                 | little-endian `f64`                         | `Float(f64)`                                           |
| unit-bearing `Float`    | a unit `u32`, then an `f64`                 | `Float(f64)`; the unit tag is consumed but not exposed |
| `Distance`              | a unit `u32`, then an `f64`                 | `Distance { unit, value }`                             |
| `String`, `Uuid`, `URL` | length-prefixed UTF-8                       | `String(String)`                                       |
| `Color`                 | four little-endian `f64` channels           | `Color([r, g, b, a])`                                  |
| `Reference`             | no value bytes                              | `Reference`                                            |
| `TextureURI`            | kind `0` counted paths or kind `1` one path | `TextureUri(Vec<String>)`                              |

Properties declared with `allowmultiplevalues="true"`, except `TextureURI`,
start with a `u32` value count and become `Multiple(Vec<PropertyValue>)`.
Connectable properties and `Reference` values are followed by a connection
block. Its form is a presence byte, kind byte `1`, a `u32` count, and that many
length-prefixed connected asset identifiers. The identifiers remain in
`DecodedProperty::connections`; the crate does not interpret their ownership.

Floating-point values must be finite. Length-prefixed strings are bounded, and
schema XML entries are bounded to 128 MiB. Implausible value and connection
counts, truncated values, invalid connection kinds, invalid texture kinds, and
trailing bytes in a logical record are rejected.

## Rejection and ownership model

Package ZIP errors, malformed schema XML, duplicate schema definitions, broken
inheritance, and invalid page framing return `CodecError::Malformed`.
Record-level failures are represented by [`RejectedRecord`][rejected-record]
and do not prevent later correctly framed records from decoding through
`decode_detailed`.

The format codec owns the higher-level joins:

- `cadmpeg-codec-f3d` joins decoded Protein records to Fusion appearance and
  material assignments.
- `cadmpeg-codec-inventor` projects decoded records into its document-local
  asset and appearance model.
- Neither codec treats a catalog entry or a connection identifier as proof of
  a body or face assignment without a separate typed source link.

The crate has no writer and does not reassemble or patch a Protein archive.

## Documentation

- [API documentation][docs]
- [Protein format specification][protein-spec]
- [F3D format notes][f3d]
- [Inventor format notes][inventor]
- [Format support][support]
- [Architecture and crate map][architecture]
- [Clean-room and legal policy][legal]
- [Repository][repo]

Requires Rust 1.88 or later. Licensed under Apache-2.0.

Autodesk, Autodesk Fusion, Inventor, and other product names are trademarks of
their respective owners. cadmpeg uses them only to identify the file formats
this crate supports and is not affiliated with, endorsed by, or sponsored by
any CAD vendor. See the [clean-room and legal policy][legal].

[architecture]: https://github.com/cadmpeg/cadmpeg/blob/main/docs/architecture.md
[decode]: https://docs.rs/cadmpeg-protein/latest/cadmpeg_protein/fn.decode.html
[decode-detailed]: https://docs.rs/cadmpeg-protein/latest/cadmpeg_protein/fn.decode_detailed.html
[decoded-record]: https://docs.rs/cadmpeg-protein/latest/cadmpeg_protein/struct.DecodedRecord.html
[docs]: https://docs.rs/cadmpeg-protein
[f3d]: https://github.com/cadmpeg/cadmpeg/blob/main/docs/formats/f3d.md
[has-schemas]: https://docs.rs/cadmpeg-protein/latest/cadmpeg_protein/fn.has_schemas.html
[inventor]: https://github.com/cadmpeg/cadmpeg/blob/main/docs/formats/inventor.md
[legal]: https://github.com/cadmpeg/cadmpeg/blob/main/LEGAL.md
[protein-layout]: https://github.com/cadmpeg/cadmpeg/blob/main/docs/layouts/protein.md
[protein-spec]: https://github.com/cadmpeg/cadmpeg/blob/main/docs/formats/protein.md
[property-value]: https://docs.rs/cadmpeg-protein/latest/cadmpeg_protein/enum.PropertyValue.html
[rejected-record]: https://docs.rs/cadmpeg-protein/latest/cadmpeg_protein/struct.RejectedRecord.html
[repo]: https://github.com/cadmpeg/cadmpeg
[support]: https://github.com/cadmpeg/cadmpeg/blob/main/docs/format-support.md
