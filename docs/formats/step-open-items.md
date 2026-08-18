# STEP Open Items

This document lists the parts of STEP exchange formats that we do not know. The specification `step.md` gives the parts that we know.

Each item has these parts:

- **Question** — what we must find.
- **Known** — what the specification gives now.
- **Need** — why we must find the answer.
- **Conflict** — a disagreement between two documents, or between a document and the decoder. An item with this part needs a decision.
- **Note** — a defect in the item or in the specification.

When an item is resolved, delete it in the same change that writes the answer into the specification. Do not keep a Resolved part.

Each item has an identifier. Use the identifier in commit messages and in code comments.

This document uses ASD-STE100 Simplified Technical English. Record names, field names, and token values are technical names. They keep their source spelling.

## 1. External resources

## 3. Containers and other encodings

### CE-08. Header schema object identifier

**Question.** Which `FILE_SCHEMA` object identifier forms must the header
parser accept?

**Known.** ISO 10303-21 gives a schema name with an optional object
identifier. The parser refuses the complete file when one component of that
identifier is negative. The `Idf/Idflibs/VC0603_SMD.stp` sample that installs
with FreeCAD declares `AUTOMOTIVE_DESIGN_CC2 { 1 2 10303 214 -1 1 5 4 }`. Its
SHA-256 is
`ddb12bc22a94b3ce2837f4a3e75fa37b47824363c54f491c853b80d6cc6b06a0`. `cadmpeg
inspect` stops with `malformed container: FILE_SCHEMA has invalid or duplicate
schema identifiers at byte 358`. A copy that changes only that one component
from `-1` to `1` decodes, transfers 418 pcurve uses, and passes `cadmpeg
check`.

**Need.** Decide if a negative object identifier component is a container
error or a recoverable header loss. Open CASCADE 6.3 wrote this file in 2010,
so the form occurs in exchange. A refusal in the header stops all later work
on the file, and the geometry after the header is good.

**Note.** Operator verification of commit `ba62cfb87` found this. That commit
used the sample as independent evidence. It normalized the header in a scratch
copy, recorded both hashes in its notebook, and filed no item.


## 4. Signatures

## 5. Topology and pcurve decisions

### TP-11. Strict mode and unproved pcurve fidelity

**Question.** Must strict decode refuse a file that carries a pcurve which the
finite admission witness accepted?

**Known.** Commit `ba62cfb87` reports `topology.pcurve-global-fidelity-unproved`
for each admitted non-seam pcurve, and pins the strict floor of that code to
warning. `cadmpeg dump --no-salvage` therefore stops at the first admitted
pcurve. The `Idf/Idflibs/VC0603_SMD.stp` sample admits 418 pcurves, and strict
decode refuses it. A FreeCAD 1.1.1 export of two solids holds no `PCURVE`, so
strict decode accepts it: the refusal follows an admitted pcurve, not a
producer.

**Need.** Decide what strict mode means. One reading refuses every unproved
fidelity claim. A different reading refuses only salvage substitutions and
malformed input. Record the decision in the specification. The present rule
makes strict decode refuse each real file that carries pcurves, so the mode
gives no usable result for STEP.

**Note.** The refusal class is also wrong. `CodecError::Malformed` gives the
text `malformed container`, but a fidelity warning does not make the container
malformed.

### TP-12. Report granularity for unproved admission

**Question.** At which granularity must the decoder report an unproved pcurve
admission?

**Known.** `crates/cadmpeg-codec-step/src/reader/topology.rs:2339-2359` pushes
one `LossNote` for each admitted pcurve, and each note holds a formatted
message that names the curve, the surface, and the coedge use. The
`Idf/Idflibs/VC0603_SMD.stp` sample gives 418 such notes in a 187 kB report
for a 632 kB source. `cadmpeg check` prints one line for each note.

**Need.** Decide between one note for each relation and one note for each
document. The note count grows with the model, so a large assembly gives a
report that is large and hard to read. If the per-relation form stays, give
the rule in the specification, because a reader cannot see from the note count
how many distinct problems exist.


## 6. Units and measures

## 7. Annotation, presentation, and tessellation

## 8. Product structure and placement
