# SolidCraft golden fixtures

These fixtures are internal test inputs. They are not corpus specimens.

- They were written by cadmpeg's test code or retagged by a local script
  (`retag57.py`, which changes the format tag and rewrites the manifest's
  document digest). None was authored in SolidCraft by a person.
- They are not CC0 donations and are not listed in `corpus/manifest.toml`.
  The SolidCraft format key is not in the manifest's valid format keys.
- The goldens under `tests/golden/decode` and `tests/golden/inspect` pin the
  decoder's output for them. Regenerate with `UPDATE_GOLDEN=1`.

Third-party SolidCraft designs are never committed. Use them only for local
evaluation.

A public specimen needs a file authored in SolidCraft from a blank template,
donated under CC0-1.0 through the corpus process in `corpus/README.md`.
