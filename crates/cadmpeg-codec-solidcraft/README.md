# cadmpeg-codec-solidcraft

`cadmpeg-codec-solidcraft` inspects and decodes SolidCraft `.sc` and `.solidcraft` designs for
[`cadmpeg-ir`][ir]. A design is a plain JSON document or a ZIP container that
holds the document beside content-addressed payload blobs and caches.

<!-- generated: capability solidcraft -->

Support: L5 ([ladder](https://github.com/cadmpeg/cadmpeg/blob/main/docs/format-support.md#solidcraft-sc-and-solidcraft)).

<!-- /generated: capability solidcraft -->

The codec detects a design by its content, never by its extension. It lists
every container member with its role, checks each blob against the SHA-256 in
its name, and classifies the file from its encoding, its container marker, and
the format number of its document. Decode reads the document header into the
source metadata and retains every member as a named native record. Every family
of content that is retained without decoding is named by a `solidcraft/*` loss.

## Install

```sh
cargo add cadmpeg-codec-solidcraft cadmpeg-ir cadmpeg-core
```

[ir]: https://docs.rs/cadmpeg-ir
