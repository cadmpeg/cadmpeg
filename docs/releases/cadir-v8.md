# CADIR wire version 8

CADIR documents, decode sidecars, and command reports now use wire version 8. Readers accept exactly 8 and refuse every earlier artifact. Releases 0.6.0–0.6.1 and 0.6.2 emitted different version-6 shapes. The version-6 and released version-7 documents lack content scope. There is no reader migration.

The document field `scope` replaces recorded decode depth. Its independent `model` and `design` booleans declare which neutral classes are held, and participate in the content digest. A populated class cannot be excluded. Decode transfer reports carry the delivered scope and a geometry-transfer boolean. `DecodeDepth` remains the caller's request vocabulary.

F3D design failure and NX attachment failure now deliver reduced scope and report the missing requested classes. Failure events remain in reports. F3D no longer stores a design failure arena in the document.

Writers synthesize from the document's neutral content and their own typed native records. Retained bytes can select layout and identities, but cannot supply content. `SourceFidelity.unrepresented` identifies source records that have no typed representation. Patch and replay are guarded; declined patches fall back to synthesis and report their reason.

FreeCAD has a source-less FCStd writer, including OCCT BRep text output. FreeCAD, IGES, Rhino, F3D, and SLDPRT reduce or omit unsupported content with losses instead of refusing otherwise writable documents. Feature evaluation staleness is stored in `needs_recompute`; writers preserve the target format's recompute flag or report `export:evaluation.stale`. FreeCAD dumb-solid `Part::Feature` wrappers do not create neutral design-history nodes.

`--reject-lossy=decode` judges this invocation's native decode or CADIR parse. A restored sidecar is displayed as historical decode evidence. Its hash binding is unchanged; editing CADIR requires re-binding or removing the sidecar. `--reject-lossy-floor=info|warning|error|blocking` selects the minimum rejected severity and defaults to `info`. Excluded neutral scope produces a note rather than a loss rejection.

A non-empty `unrepresented` makes patch and replay ineligible. The writer synthesizes with `SourceUnrepresented { classes }` as its cause. A patch that cannot express an edit synthesizes with `PatchDeclined { reason }`. The export report carries the synthesis cause.

See [the document contract](../document-contract.md) for codec, writer, and editor requirements.
