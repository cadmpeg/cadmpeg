# Rhino 3DM open items

This file contains only payload semantics that are outside the settled built-in
format model.

## Third-party plug-ins

- Payload layouts for class UUIDs registered by third-party plug-ins.
- Payload layouts for third-party class userdata, including plug-in-specific
  anonymous chunks, dictionaries, and application records.
- Meaning of plug-in-defined attribute or layer extensions whose item payload
  is not one of the built-in items defined by the main format specification.

## Future extensions

- Payload semantics for future class UUIDs not in the built-in registry.
- Payload semantics for future attribute or layer item IDs whose widths and
  layouts are not defined by the corresponding built-in version gate.
- Payload semantics for future major payload versions.
- Semantics of future minor-version suffixes after the last settled field when
  their bounded layout is not defined.

## Out-of-scope legacy geometry

- V1 geometry payload layouts.
- V2 geometry payload layouts.

V1 flat-chunk framing, V2 and later table framing, checksums, strings,
userdata framing, attributes, settings, layers, object identity, and all
settled V3 through V8 built-in payloads are defined in
`docs/formats/rhino_3dm.md` and are not open items.
