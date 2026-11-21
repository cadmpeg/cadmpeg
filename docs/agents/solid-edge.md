# Solid Edge issue work

Apply the [general loop](README.md) to one of issues #1863 through #1874.
The parent [#1862](https://github.com/cadmpeg/cadmpeg/issues/1862) tracks the
`.par`, `.psm` and `.asm` reader. Each sub-issue has Known, Unknown and
How to verify sections and an attached CC0 repro set: a before/after native
pair and STEP exports. The attached files support analysis without a Solid
Edge licence. The skeleton PR is [#1875](https://github.com/cadmpeg/cadmpeg/pull/1875).

0. Claim the issue: comment /claim on it, then open a draft pull request linked to the issue within 24 hours, even a rough one, and keep it moving; CONTRIBUTING.md gives the claim rules.
1. Read the issue, #1862, #1875 and the policies named in the general loop.
   Run these commands from your clone of the repository. Set `issue` to
   the sub-issue number.

   ```sh
   issue=1863
   gh issue view "$issue" --repo cadmpeg/cadmpeg --comments
   gh issue view 1862 --repo cadmpeg/cadmpeg --comments
   gh pr view 1875 --repo cadmpeg/cadmpeg
   ```

   Read `crates/cadmpeg-codec-solidedge/` if it exists. If it is absent, start
   from the skeleton pull request #1875. Its branch lives in a fork, so
   fetch it through the pull request ref. Set `branch` to a branch name of
   your choice.

   ```sh
   branch=solidedge-issue-1863
   git fetch https://github.com/cadmpeg/cadmpeg pull/1875/head
   git switch -c "$branch" FETCH_HEAD
   ```

   With the skeleton merged, create a branch from `main`.
   Read the crate's `src/lib.rs`, `src/loss.rs`, owning parser modules and
   existing tests. Use `crates/cadmpeg-codec-sldprt/` as a structure example.
   Read `docs/formats/solidedge.md`, its open-items file and
   `docs/layouts/solidedge.toml` when present.

2. Download the issue attachments into scratch storage outside the repository.
   Set `TMPDIR` to an external writable directory. Take each URL from the
   issue and assign `before_url`, `after_url`, `before_step_url` and
   `after_step_url`. Set `ext` to `par`, `psm` or `asm` for this pair.

   ```sh
   scratch=$(mktemp -d "${TMPDIR:-/tmp}/cadmpeg-solidedge-$issue.XXXXXX")
   ext=par
   curl --fail --location "$before_url" --output "$scratch/before.$ext"
   curl --fail --location "$after_url" --output "$scratch/after.$ext"
   curl --fail --location "$before_step_url" --output "$scratch/before.step"
   curl --fail --location "$after_step_url" --output "$scratch/after.step"
   ```

   Keep the attachment URLs, author declarations and file hashes in scratch
   notes. Use `sha256sum "$scratch/before.$ext"` and repeat for each file.
   Keep all downloads outside the commit unless the issue explicitly requests
   corpus fixtures; apply [corpus/README.md](../../corpus/README.md) in that case.
   For a ZIP attachment, list it with `cadmpeg inspect container` and extract
   each named member with `cadmpeg inspect extract`, as in step 3.

3. Build the CLI before a batch run. This command builds the current default
   codecs, including STEP. Check the skeleton's CLI feature registration for
   Solid Edge and enable its declared feature if required.

   ```sh
   cargo build -q -p cadmpeg
   ```

   Use the resulting `cadmpeg` binary for every command below. The local Cargo
   default path is `target/debug/cadmpeg`; a build service can return a
   different artifact path. Put its directory on `PATH`. List each native
   file's streams, including assembly companion files attached to the issue.

   ```sh
   for file in "$scratch"/*."$ext"; do
     timeout 30 cadmpeg inspect container "$file"
   done
   ```

   Copy a stream path byte for byte from the listing into `member`. Preserve
   case, punctuation and control characters. Repeat extraction for each stream
   needed by the issue, using distinct output names.

   ```sh
   cadmpeg inspect extract "$scratch/before.$ext" "$member" -o "$scratch/before.stream"
   cadmpeg inspect extract "$scratch/after.$ext" "$member" -o "$scratch/after.stream"
   ```

4. Compare the extracted streams under CLAUDE.md's byte-work rules. Start with
   `cmp`, then inspect each changed span. Exit 1 from `inspect cmp` records a
   byte difference. The following offset, pattern and layout show command
   syntax; set them from the changed span and the proposed record grammar.

   ```sh
   cadmpeg inspect cmp "$scratch/before.stream" "$scratch/after.stream"
   offset=0
   cadmpeg inspect hex "$scratch/before.stream" --offset "$offset" --len 0x80
   cadmpeg inspect hex "$scratch/after.stream" --offset "$offset" --len 0x80
   cadmpeg inspect find "$scratch/after.stream" --encoding hex '01 00 ?? 00' --context 16
   cadmpeg inspect strings "$scratch/after.stream" --encoding both --min 4
   cadmpeg inspect read "$scratch/after.stream" --type u32 --offset "$offset" --endian le
   cadmpeg inspect struct "$scratch/after.stream" --offset "$offset" --count 1 \
     --layout 'u32le:id,pad4,f64le:x,f64le:y,f64le:z'
   ```

   Use cadmpeg's byte tools for byte work. State a hypothesis with stream name, record
   boundary, tag, width, byte order, count, reference scope and unit. Check
   each claim against both native files and all attached variants. Record
   evidence and rejected readings in scratch notes or the PR body. If two
   layouts fit, ask the human for a distinguishing variant.

5. Implement the settled reading in the owning module of
   `crates/cadmpeg-codec-solidedge/src/`. Keep `src/lib.rs` as the facade and
   match `docs/codec-facade.toml`. Read bytes through
   `cadmpeg_core::decode::View`, including typed `req_*` and offset readers.
   Keep the `View` where its `SpaceId` must survive. Propagate `ParseError`
   through the codec-local `From<ParseError>` with `?`. Validate file-derived
   counts with `View::counted` or `bounded_len`, then allocate through
   `DecodeContext::alloc_filled`.

   At the record recovery boundary, turn a bad record into a loss and continue
   with other records. Resume at a validated next boundary; a bad record must
   preserve the usable file result. Preserve container errors and session
   resource refusals. Add a `SolidEdgeLossCode` variant in `src/loss.rs` for
   each new drop path. Construct notes with `code.note(message)` and keep the
   `family.detail` strings stable. Decode each value supported by the most
   likely reading and charge an assumption loss where needed. Keep measurement
   in verification; select the reading from byte semantics. Ask the human
   before a CADIR wire change; retain V11 for this work.

6. Rebuild the CLI after the code change. Decode and check both native files
   and their STEP exports. The commands below use fresh scratch output names;
   repeat with new names after each iteration.

   ```sh
   cargo build -q -p cadmpeg
   for side in before after; do
     timeout 60 cadmpeg dump "$scratch/$side.$ext" -o "$scratch/$side.native.cadir.json"
     timeout 60 cadmpeg dump "$scratch/$side.step" -o "$scratch/$side.step.cadir.json"
     timeout 60 cadmpeg check "$scratch/$side.$ext" -o "$scratch/$side.native.check.json"
     timeout 60 cadmpeg check "$scratch/$side.step" -o "$scratch/$side.step.check.json"
     cadmpeg query summary "$scratch/$side.native.check.json"
     cadmpeg query losses "$scratch/$side.native.check.json"
     cadmpeg query findings "$scratch/$side.native.check.json"
     cadmpeg query counts "$scratch/$side.native.cadir.json"
     cadmpeg query counts "$scratch/$side.step.cadir.json"
     timeout 60 cadmpeg measure "$scratch/$side.native.cadir.json"
     timeout 60 cadmpeg measure "$scratch/$side.step.cadir.json"
     timeout 60 cadmpeg diff "$scratch/$side.native.cadir.json" "$scratch/$side.step.cadir.json" --geometry
   done
   ```

   Compare body and face counts from `query counts`, volume in mm³, and the
   bounding-box minimum and maximum in mm for each pair. Use
   [docs/measurement.md](../measurement.md) for numeric error, model uncertainty,
   unavailable values and the geometry comparison allowance. Record actual
   and expected counts and each `agree`, `disagree` or `inconclusive` verdict.
   Explain topology differences supported by the issue. For sheet bodies,
   record the volume's applicability and the available measurements. For
   assemblies, check the issue's occurrence placements as well as body geometry.
   Run the issue's How to verify procedure and record its result. A count or
   measurement mismatch requires another iteration or a stated evidence gap.

7. Update the format model. Put settled byte semantics and invariants in
   `docs/formats/solidedge.md`. Put fixed offsets, widths and byte order in
   `docs/layouts/solidedge.toml`; follow [the layout rules](../layouts/README.md).
   Keep evidence, implementation status and provenance in the issue or PR.
   Put genuine unknowns in `docs/formats/solidedge-open-items.md`, with fields
   Question, Known, Need, Conflict and Note. Write a settled answer into the
   specification and delete its open item in the same change.

   `scripts/preflight_open_items.py` checks duplicate IDs; preflight compares
   new IDs with upstream and reports the next free number on a collision.
   Use the document's existing prefix and an unused number in headings of the
   form `### PREFIX-01. Title`. Check the working document and upstream before
   assigning an ID. If that tooling is absent, apply CLAUDE.md's rule: keep
   genuine unknowns in the companion open-items file with the five fields
   above, and delete settled items when their answers enter the specification.

8. Add synthetic tests built from the settled specification. Construct bytes
   from the record grammar. Keep attachment bytes in scratch storage. Assert
   decoded values, reference scope, units, exactness and the relevant loss
   codes. Include a malformed record followed by a valid record to test
   recovery. Take expectations from the specification or approximate equality.
   Put unit tests in the owning module's inline `mod tests`, `foo/tests.rs` or
   `foo/tests/mod.rs`. Put codec-wide composition tests in
   `src/integration_tests.rs` or `src/integration_tests/`, and support in
   `src/test_support.rs` or `src/test_support/`. Follow AGENTS.md for golden
   placement and separate test moves from feature work.

9. Run the applicable repository checks from the root. These commands implement
   AGENTS.md and CONTRIBUTING.md's focused checks.

   ```sh
   cargo check -q -p cadmpeg-codec-solidedge --tests
   cargo test -q -p cadmpeg-codec-solidedge --lib
   cargo clippy -q -p cadmpeg-codec-solidedge --all-targets
   cargo fmt --all --check
   python3 scripts/check-source-policy.py
   python3 scripts/check-codec-facade.py
   python3 scripts/check-doc-anchors.py
   python3 scripts/preflight_open_items.py
   python3 scripts/preflight.py
   ```

   Add `-- owning_module::tests` to the codec test command when the change is
   confined to that unit-test module. For a changed layout table, regenerate,
   review the diff, then validate:

   ```sh
   UPDATE_LAYOUT_DOCS=1 UPDATE_LAYOUT_CODE=1 cargo test -q -p cadmpeg --test layout_tables
   cargo test -q -p cadmpeg --test layout_tables
   ```

   Follow `docs/layouts/README.md` for the generator mapping if the skeleton
   needs one. Regenerate schema and Annex A after native record type changes:

   ```sh
   UPDATE_GOLDEN=1 cargo test -q -p cadmpeg-ir --features schema --lib
   python3 scripts/generate-annex-a.py --write
   ```

   Review every generated diff. Leave workspace gates to CI after the push:
   `cargo clippy --workspace --all-targets`, `cargo build --workspace`,
   `cargo test-fast` and `cargo test --workspace --doc`, as CONTRIBUTING.md
   lists. If an unrelated check
   fails, give the human its command, exit status and failure reason.

10. Prepare a PR with a title that states the implemented behavior. Use
    `.github/PULL_REQUEST_TEMPLATE.md`, put `Part of #1862, fixes #N` in the
    body with the issue number, and include the attachment names and hashes,
    stream/record findings, expected and decoded counts, volume and box
    comparisons, loss codes and check results. Include the contributor's
    truthful DCO certification as a ticked `Signed off` box and this verbatim
    declaration from LEGAL.md:

    > I derived this contribution solely by analyzing CAD files I am legally entitled to possess and from publicly available information. I did not use vendor SDKs, decompiled binaries, confidential or NDA-covered material, or any source listed as forbidden in LEGAL.md.

    Save the body to `$scratch/pr.md` and set `title` to the change title.
    `gh repo fork --remote` makes your fork the `origin` remote (run it once):

    ```sh
    gh repo fork --remote
    git push -u origin HEAD
    gh pr create --repo cadmpeg/cadmpeg --title "$title" --body-file "$scratch/pr.md"
    ```

    CI runs the full gate on the pull request.

## Stop conditions and new repro variants

Ask the human when a required repro variant is missing, two layouts fit the
evidence, the change needs a CADIR wire change, or a check fails for a reason
outside the change. State the exact evidence or decision needed.

For the standard recipe set, use the public
[Solid Edge Fixture Builder](https://fixtures.dutchmancad.xyz/builders/solidedge/).
Its `build_solidedge.ps1` uses PowerShell and Solid Edge Community Edition on
Windows. The builder is CC BY 4.0. It builds the project's standard recipes.
A per-issue repro script will be published next to the builder.

For an issue-specific pair, ask on the issue and state the exact before/after
edit, dimensions, units, save version and STEP exports needed to distinguish
the layouts. Anyone with Solid Edge, including the free Community Edition on
Windows, may build the pair by hand from a blank default template. The donor
must confirm that every included part, material and appearance is their own
work, that their licence permits public sharing, and that they dedicate the
attached native files and exports under CC0-1.0. Apply the corpus rules if
the issue requests committed fixtures.

## Clean-room sources

Use legally held file bytes and permitted public information under
[LEGAL.md](../../LEGAL.md). Exclude vendor SDKs and API documentation,
decompiled vendor code, confidential or NDA material, incompatible licensed
sources and access-control bypasses. Apply the conflict-of-knowledge rule to
prior SDK or NDA access. Keep the provenance declaration truthful.
