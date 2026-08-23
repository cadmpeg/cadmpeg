# Autodesk Protein asset package: Format Specification

> **License:** This document is released under [CC-BY-4.0](https://creativecommons.org/licenses/by/4.0/). Attribute to the cadmpeg project.

---

Record offsets, field widths, and endianness are also maintained as a machine-checked table in [`docs/layouts/protein.md`](../layouts/protein.md), generated from `docs/layouts/protein.toml`. That table is the canonical source for the numbers; the prose below carries the semantics. `cargo test -p cadmpeg --test layout_tables` proves the two agree.

A Protein package is a ZIP archive of XML schemas and paged instance-property streams. Fusion `.f3d` containers carry nested `.protein` members. Inventor IPT/IAM documents wrap one Protein ZIP in a compound `Protein` stream whose four-byte length prefix is specified in [`inventor.md`](inventor.md) §7.

## 1. Package

The package is a ZIP archive. A schema entry is a file whose path begins with `Schemas/` or contains `/Schemas/` and ends with `Schema.xml`. Every schema declares its identifier in the root `UID` element. Duplicate ZIP entry names and duplicate schema identifiers are malformed.

Schema XML, ZIP entry order, and schema inheritance are a text grammar over named members. They have no fixed byte offsets.

## 2. Instance stream header

`InstanceProperties.bin` and `DefinitionIteratorProperties.bin` start with a 16-byte stream header. The first header word is the little-endian u32 page size and equals `0x88`. The remaining twelve header bytes are retained; the specification states no field in that region.

The bytes after the header are an exact multiple of the page size. A header without at least one complete page is malformed.

## 3. Pages

Each page is 136 bytes. Bytes 8 through 135 are the 128-byte page body. Record extents come from page framing; scanning the concatenated payload for a record-start marker does not recover them.

| Page         | Marker                      | Contribution                                                                                                      |
| ------------ | --------------------------- | ----------------------------------------------------------------------------------------------------------------- |
| Record start | `80 00 01 00` at bytes 4..8 | Opens a logical record and contributes the complete body at bytes 8..136.                                         |
| Continuation | `80 00 00 00` at bytes 4..8 | Extends the current record with its complete body.                                                                |
| Terminal     | `ff ff ff ff` at bytes 0..4 | Closes the current record; bytes 8..`8 + used` contribute, where `used` is the little-endian `u16` at bytes 4..6. |

A record-start page or continuation page stores its four-byte marker at offset 4. Bytes 0 through 3 are a prefix; the specification states no field there.

A terminal page stores `ff ff ff ff` at offset 0 and the used payload length as a little-endian u16 at offset 4. Bytes 6 through 7 are a suffix; the specification states no field there. `used` is at most 128.

A new start page, continuation page, or terminal page without the required current-record state is invalid. A stream ending with an open record is valid: the last open record is complete. A record-start page also ends the record before it.

## 4. Instance logical record

A valid logical record begins with four length-prefixed UTF-8 strings: schema identifier, asset GUID, base asset identifier, and `AssetLibID`. The value block that follows is the remaining members of the schema inheritance closure. Property order, carriers, connection blocks, and omitted inherited members are schema-driven and have no fixed byte offsets.

## 5. Definition-iterator logical record

A `DefinitionIteratorProperties.bin` logical record has this sequence:

1. record marker `80 00 01 00`;
2. length-prefixed UTF-8 schema identifier, followed by one zero byte;
3. length-prefixed UTF-8 asset identifier;
4. length-prefixed UTF-8 base asset identifier;
5. little-endian u32 format version, equal to `2`;
6. length-prefixed UTF-8 category, group, and description;
7. little-endian u32 tag count and that many length-prefixed UTF-8 tags;
8. little-endian u32 preview-path count and that many length-prefixed UTF-8 paths.

The record has no further typed members after the last preview path. If page framing contributes bytes through the end of a start or continuation page, the remaining bytes are zero padding. The asset identifier joins the definition to the base asset identifier of an instance record. The joined definition supplies the instance schema identifier and category.

A definition stream can repeat one asset identifier with the same schema, base asset identifier, category, group, description, tags, and preview paths. Equal definitions denote one catalog entry. Repeated asset identifiers with different definition fields are invalid.
