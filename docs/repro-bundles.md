# Reproduction bundles

A reproduction bundle is a CADIR document with `fragment: true` and a `repro`
object. A consumer can deserialize the bundle as `cadmpeg_ir::CadIr` and pass it
to its mapper. The `repro` object carries the signature, root identities, opaque
application context, and arena manifest. The current `IR_VERSION` applies.

The library entry points are in `cadmpeg_registry::repro`:

```rust,ignore
pub fn cut(document: &Document, finding: &Finding, options: &CutOptions)
    -> Result<Bundle, CodecError>;
pub fn check(bundle: &Bundle) -> Vec<Finding>;
pub fn shrink(bundle: &Bundle, signature: &Signature, budget: Duration,
    cancel: &dyn Fn() -> bool) -> Bundle;
```

`Document` is `cadmpeg_ir::CadIr`. `Finding` is the existing check finding.
`CutOptions::context` is a JSON object. Its values pass through unchanged. The
caller controls the information in that object. Row and serialized-byte limits
bound cut projection. The defaults admit 1,000,000 rows and 64 MiB of compact
JSON. The registry enables the IR schema feature used by the privacy projection.

The registry composes neutral validation, source-fidelity validation when a
sidecar is supplied, registered native validation, and measurement findings.
The CLI calls this composition. The IR owns closure and signature operations
because these operations use the CADIR graph and finding contract. The reference
visitor also serves `query graph`.

The cut starts at the finding entity and related identities. It follows model
and native references to a fixed point. Incoming coedges, loops, faces, shells,
regions, and bodies retain topology context. A native root, or a feature that
reaches a native scope, retains the native namespace and linked neutral
features. This keeps numeric record-index joins and type-table rows together.
The header retains the wire version, units, tolerances, and source codec id.
Codec-owned replacements retain fixed stream-member suffixes in anonymous
addresses. Native type GUIDs and encoded numeric payloads retain their values.

The privacy projection replaces identity scopes and nonnumeric keys consistently.
It replaces free text with opaque tokens. Source classification reduces to the
codec id. Metadata strings and path strings are removed. Numeric geometry stays
in the fragment. The manifest gives each retained arena's row count, compact
JSON array size, and one-line description. Context is caller-owned data.

A signature records its normalization version, pipeline stage, codec id, check
id, severity, entity kind, normalized message, numeric observations, and hash.
Entity identities become `{entity}`. Quoted text and path tokens become `{text}`.
Decimal numbers, exponents, and hexadecimal integers become `{number}`. Each numeric
observation has an inclusive band of ten percent of its absolute value. Zero has
an exact band. The hash is the first eight SHA-256 bytes of the compact JSON array
`[signature_version, stage, codec, check, severity, entity_kind, message]`, encoded
as 16 lowercase hexadecimal characters. Numeric bands are separate from the
hash. Matching requires the hash and every numeric band to agree.

Fragment validation emits `fragment_skipped` information findings for checks
that require complete document context. Carrier reachability and Fusion ACT root
cardinality and ordinal coverage use this rule. Local identity uniqueness,
reference resolution, geometry, and topology checks remain active.

Shrinking removes row groups and their incoming dependents. Native namespaces
stay together during removal. Each accepted candidate reproduces the signature.
The manifest is rebuilt after each accepted reduction. The budget and cancel
callback are checked between candidate operations. One composed validation pass
uses the service resource policy. Cancellation returns the last accepted bundle.

Write a check report before cutting its zero-based finding index:

```sh
cadmpeg check input.cadir.json --report report.json
cadmpeg repro --from report.json --finding 0 -o bundle.json
cadmpeg check --fragment bundle.json --json
```

A persisted check report includes the absolute `input_path`. Keep the input
available and unchanged until the cut completes. The repro command refuses to
write a cut that fails to reproduce its selected signature. The output path must
be new. Report and bundle input files have a 256 MiB limit. Check exits with its
usual error status when the fragment reproduces an
error finding.

Native discriminator classification uses codec-owned typed readers. It permits
100,000 probes and 512 MiB of cumulative probe input. A probe operates on a copy
of one native record. A cap refusal returns `CodecError` before bundle delivery.
