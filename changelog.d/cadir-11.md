### Breaking

- No tool converts a CADIR 10 document to CADIR 11: keep the source CAD files. [Upgrading from CADIR 10](https://github.com/cadmpeg/cadmpeg/blob/main/docs/upgrading-to-cadir-11.md) lists the layout changes and the new arena paths.
- A CADIR 11 document holds its topology under `geometry` and its feature history under `design`: `.model.faces` is `.geometry.faces`, and `.model.features` is `.design.features`. `cadmpeg query` names the arenas the same way. `scope` replaces `decode_depth`.
- A SolidWorks or Fusion write of a document read from a `.cadir.json` writes an unedited document from its retained source and refuses an edited one.
- Keep the `.fidelity.json` decode sidecar beside its `.cadir.json`. A FreeCAD or SolidWorks write reads it to find the edited records, and reports `export:edit.unverified` without it.
- `convert` writes the output when the check finds errors, and reports the findings. `--allow-errors` is removed: remove it from scripts. `--reject-errors` refuses the write (exit 1).
- Writers produce a file and report losses for many documents that they refused before. A script that used the refusal as its signal reads the losses, or passes `--reject-lossy=export`.
- `cadmpeg check` applies the validation rules of the CADIR 11 specification, also to the design and process layers (`check` values `design` and `process`). It can report errors on a decode that an earlier release passed. In `check --json`, every finding writes `related`.
- Loss codes and severities change for several formats: for example, an unreadable record is `decode/record.unreadable`, and a defective STEP statement is an `Error`. A script that matches loss codes, or `--reject-lossy` with an `Error` floor, can give a different result. The list below names each change.
- The Rust API of the `cadmpeg-*` crates changes. The list below names each change.
