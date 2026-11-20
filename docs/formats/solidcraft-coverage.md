# SolidCraft coverage

This page states what the codec writes and how that is checked. The file format is in [solidcraft.md](solidcraft.md); what stays unresolved is in [solidcraft-open-items.md](solidcraft-open-items.md).

## Writing

The writer has three paths, chosen by the target request and by whether the
source is still the retained image.

**Replay.** Decode keeps the whole file as a source image and records the
`document_local_sha256` of the neutral document. An inherited target, or the
source's own dialect, writes the image byte for byte while the document still
digests to that value. This holds for every dialect the decoder reads.

**Patch.** A document that changed is written by patching the retained
document. The one edit family is a parameter's expression, set in the
`params` record of the same name. The writer proves that nothing else
changed: it sets the baseline expression of each edited parameter back in a
copy of the model and digests the copy against the decode baseline. A new
parameter, a parameter that is not a `params` record, and any other edit are
refused by name. A container is packed again without `cache/`, which is keyed
to the unedited document, and the dropped members are charged as
`solidcraft/cache.model-stale`. Plain JSON stays indented JSON.

**Generation.** A model with no source, or an explicit target other than the
source's dialect, is written from the neutral model alone as `solidcraft/57`,
either a container (the default) or plain JSON. The generator writes the
parameters and the primitive features (box, cylinder, cone, sphere, torus) in
ordinal order, with dimensions as millimetre expressions, and never a
`cache/`: SolidCraft evaluates the history on open. A feature of another
family, a partial or rotated primitive, and a model that holds geometry but no
history are refused by name. Arenas the generator does not write are charged
by the export report.

The catalog lists `solidcraft:container-1-v57` and `solidcraft:json-v57`.
`scripts/verify-solidcraft-cli.py` writes an edited and a generated design,
opens each in `solidcraft-cli`, which rebuilds the model from the history,
and compares the extent and the volume.

The checked-in `solidcraft-profile.json` is generated, not edited: `SOLIDCRAFT_CLI=<path to solidcraft-cli> cargo run -p cadmpeg-codec-solidcraft --bin solidcraft-profile -- crates/cadmpeg-codec-solidcraft/tests/golden/fixtures docs/formats/solidcraft-profile.json`.
It lists the write assertions beside the cumulative gates, so a passing writer shows even while L6 to L8 keep the ladder at L5.
