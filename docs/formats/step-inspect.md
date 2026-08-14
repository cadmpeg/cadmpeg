# STEP inspect disposition (concluded)

STEP `inspect` is deep semantic analysis, not a cheap container census.

## Decision

Keep the Codec `inspect` entry point. Internally it calls `analyze_exchange`,
which runs the semantic decode path and discards the IR at the inspect
boundary. Attributes such as `unknown_entities` depend on that path; replacing
inspect with a syntactic census would drop that contract.

Do not silently substitute a cheap census. An optional cheap path may be added
later beside analysis if it does not replace the current inspect attributes.

## Why not rename the public command

`inspect` is the shared Codec/CLI surface for every format. Renaming only STEP
would fork the operator command. The disposition is recorded here and on
`StepCodec::inspect_impl` / `reader::analyze_exchange`.
