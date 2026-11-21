# cadmpeg-derive

`cadmpeg-derive` supplies the derive macros of the cadmpeg wire types.
`DeserializeName` reads an enum whose variants hold no data from the
variant's name alone, the one spelling of `docs/cadir-v11.md` §2.5.
`DeserializeTagged` reads an enum tagged by serde's `tag`, or by `tag` and
`content`, as serde's derive reads it, and reads each payload from JSON
text through `cadmpeg_core::tagged`.

Application code depends on `cadmpeg-ir`. This crate is for the wire crates.

## Documentation

- [API documentation][docs]
- [Architecture and crate map][architecture]
- [Repository][repo]

Requires Rust 1.88 or later. Licensed under Apache-2.0.

[architecture]: https://github.com/cadmpeg/cadmpeg/blob/main/docs/architecture.md
[docs]: https://docs.rs/cadmpeg-derive
[repo]: https://github.com/cadmpeg/cadmpeg
