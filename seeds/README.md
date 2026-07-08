# Fuzz seeds

Every binary file under `seeds/` is **synthesized programmatically** by the seed generators in [`crates/cadmpeg-fuzz`](../crates/cadmpeg-fuzz) (`generate_seeds`, `generate_submodule_seeds`, `generate_comprehensive_seeds`). No seed is, or contains bytes carved from, a file produced by a CAD application.

These files exist only to give the fuzz targets structurally plausible starting inputs. They are not CAD files, carry no vendor content, and are not part of the test corpus. Openly-licensed real CAD files enter the repository exclusively through the [corpus donation pipeline](../corpus/README.md).

To regenerate the seeds, run the generator binaries from the `cadmpeg-fuzz` crate.
