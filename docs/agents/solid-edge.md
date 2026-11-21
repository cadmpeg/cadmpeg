# Solid Edge issue work

Apply the [general loop](README.md) to one of issues #1863 through #1874.
The parent [#1862](https://github.com/cadmpeg/cadmpeg/issues/1862) tracks the
`.par`, `.psm` and `.asm` reader. Each sub-issue has Known, Unknown and
How to verify sections and an attached CC0 repro set: a before/after native
pair and STEP exports. The attached files support analysis without a Solid
Edge licence. The skeleton PR is [#1875](https://github.com/cadmpeg/cadmpeg/pull/1875).

0. Claim the issue under [CONTRIBUTING.md's claim rules](../../CONTRIBUTING.md#claiming-an-issue).
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

4. Compare the extracted streams under [AGENTS.md's byte-work rules](../../AGENTS.md). Start with
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
   `crates/cadmpeg-codec-solidedge/src/` under [AGENTS.md's code rules](../../AGENTS.md).
   Recover a bad Solid Edge record at a validated next boundary and continue
   with the other records.

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

7. Update `docs/formats/solidedge.md`, `docs/layouts/solidedge.toml` and
   `docs/formats/solidedge-open-items.md` under [AGENTS.md's writing rules](../../AGENTS.md).
   Follow [the layout workflow](../layouts/README.md) for table regeneration.

8. Add synthetic tests for the settled Solid Edge record grammar and
   malformed-record recovery under [AGENTS.md's test rules](../../AGENTS.md).

9. Run the applicable repository checks from the root. These commands implement
   AGENTS.md and CONTRIBUTING.md's focused checks.

   ```sh
   cargo check -q -p cadmpeg-codec-solidedge --tests
   cargo test -q -p cadmpeg-codec-solidedge --lib
   cargo clippy -q -p cadmpeg-codec-solidedge --all-targets
   python3 scripts/check-doc-anchors.py
   python3 scripts/preflight.py
   ```

   Before opening the pull request, run the commands in
   [CONTRIBUTING.md's local CI gate](../../CONTRIBUTING.md#local-ci-gate)
   from the repository root, as its
   [checklist](../../CONTRIBUTING.md#pull-request-checklist) requires.

10. Complete the draft PR with a title that states the implemented behavior. Use
    [the PR template](../../.github/PULL_REQUEST_TEMPLATE.md), put `Part of #1862, fixes #N` in the
    body with the issue number, and include the attachment names and hashes,
    stream/record findings, expected and decoded counts, volume and box
    comparisons, loss codes and check results. Follow
    [CONTRIBUTING.md's checklist](../../CONTRIBUTING.md#pull-request-checklist)
    and [LEGAL.md](../../LEGAL.md).

    Save the body to `$scratch/pr.md` and set `title` to the change title.
    `gh repo fork --remote` makes your fork the `origin` remote (run it once):

    ```sh
    gh repo fork --remote
    git push -u origin HEAD
    gh pr edit --repo cadmpeg/cadmpeg --title "$title" --body-file "$scratch/pr.md"
    gh pr ready --repo cadmpeg/cadmpeg
    ```

## Stop conditions and new repro variants

Ask the human when a required repro variant is missing, two layouts fit the
evidence, the change needs a CADIR wire change, or a check fails for a reason
outside the change. State the exact evidence or decision needed.

For the standard recipe set, use the public
[Solid Edge Fixture Builder](https://fixtures.dutchmancad.xyz/builders/solidedge/).
Its `build_solidedge.ps1` uses PowerShell and
a licensed Solid Edge install (Community Edition counts) on Windows.
The builder is CC BY 4.0. It builds the project's standard recipes.
A per-issue repro script will be published next to the builder.

For an issue-specific pair, ask on the issue and state the exact before/after
edit, dimensions, units, save version and STEP exports needed to distinguish
the layouts. Have a donor build the pair by hand from a blank default
template and attach the native files and STEP exports under CC0-1.0 with the
declarations required by [the corpus donation rules](../../corpus/README.md).
