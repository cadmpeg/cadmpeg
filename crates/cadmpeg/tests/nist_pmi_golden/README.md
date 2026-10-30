# SPDX-License-Identifier: Apache-2.0
# NIST MBE PMI inspect and decode goldens

Snapshots for the producer-evidence files in `corpus/nist-pmi/`. PR CI checks
that the names match `corpus/manifest.toml`. The scheduled workflow
`.github/workflows/nist-pmi-goldens.yml` compares the bytes.

Regenerate with:

```
UPDATE_GOLDEN=1 cargo test -p cadmpeg --test nist_pmi_goldens -- --ignored
```
