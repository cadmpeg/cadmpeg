# STEP CADIR Decisions

This document records the decisions behind the `CADIR decision D-NN` clauses
in `step.md`. A decision is a rule that CADIR sets where the format does not
answer a question. The specification states the rule. This document states
why the rule exists and what reopens it.

Each record has these parts:

- **Question** — what the format does not answer.
- **Silence** — the clauses searched and what they leave open.
- **Rule** — the specification clause that states the decision.
- **Ground** — the CADIR goal the rule serves.
- **Cost** — what the rule gives up, and the loss or finding code that
  records it. A cost of none requires the reason.
- **Reopens** — the condition that reopens the decision, and the executable
  witness that pins it when one can exist.
- **From** — the open item the decision closed. Optional.

This file holds only rules in force. A change that supersedes a decision
rewrites or deletes its record in the same commit that changes the
specification clause. An audit refutes a record when it produces a
determination the Silence missed; it does not reopen a record for the
absence of format evidence, which is the record's premise.

This document uses ASD-STE100 Simplified Technical English. Record names,
field names, and token values are technical names. They keep their source
spelling.

## Containers and composition

### D-03. Part 26 to Part 21 composition

**Question.** What binds an HDF5 (Part 26) population to a Part 21 exchange
graph when both describe one exchange?

**Silence.** ISO/TS 10303-26:2011 Annex B.3 permits HDF5 data as part of an
exchange data set and names `externally_defined_item` as one possible
wrapper. ISO 10303-41:2021 §§14.2 and 14.4.1–14.4.5 assign the role and
meaning of external-item relationships to an annotated schema or an
agreement between exchange partners. No clause defines a Part 26 row to
Part 21 `#`-instance identity map.

**Rule.** `step.md` §1 "CADIR decision D-03: Part 26 and Part 21 are
separate resource graphs" and `step.md` §1 "The caller composition operand
is". A binding exists only through the caller's explicit identity map with
both resource graphs validated. Equal URIs, filenames, schema names,
timestamps, or numeric identifiers do not create a binding.

**Ground.** No fabricated bindings: a heuristic join asserts an
exchange-partner agreement that the file does not carry. Composition stays
caller-owned, behind the external-resource admission boundary.

**Cost.** An exchange that intends an implicit row-to-instance join decodes
as two unlinked resource graphs. A standalone HDF5 input with explicit STEP
selection is refused with `NotImplemented("STEP Part 26 binary/HDF5
encoding")` before Part 21 parsing; without explicit selection the signature
classifies as a medium-confidence alternate encoding. No loss code fires
inside the decoder because the decoder drops nothing: composition does not
start.

**Reopens.** A normative identity map — a Part 26 edition or an
annotated-schema convention that defines row-to-instance identity. A
demonstrated producer convention arrives as a new open item, not as a
silent bind. This condition is documentary; no executable witness can
watch for it.

**From.** CE-06.

## Topology

### D-01. Finite pcurve admission

**Question.** What admits the optional coedge-to-pcurve relation when the
required invariant is global and no finite evaluation can prove it?

**Silence.** ISO 10303-42:2021 §5.2.2 and §4.5.49 state the invariant:
every parametric space curve for one edge use describes the same
model-space point set, with the same sense as `curve_3d`. §5.2.2.1 and
function §5.6.4 give candidate matching, not verification. No part gives a
decision procedure, and a finite sample cannot prove a property of a
continuum. For a non-seam edge with multiple same-surface candidates,
Part 42 supplies no selector.

**Rule.** `step.md` §8 "CADIR decision D-01: a typed `SEAM_EDGE`" and the
two paragraphs that follow it. Admission is the finite directed endpoint
witness plus the finite model-space locus and direction witness. Admission
transfers the relation and reports that global fidelity is unproved. CADIR
claims no global pcurve fidelity.

**Ground.** Bounded, deterministic work for each relation. The finite
same-parameter check matches reference-implementation practice (Open
CASCADE `ShapeAnalysis_Edge` finite same-parameter and orientation checks).
The alternative routes fail the Need: unconditional admission asserts
fidelity with no witness; unconditional omission drops every pcurve
relation, because the invariant is provable for none of them.

**Cost.** An admitted relation can violate the global invariant on an
unsampled interval. Each admission charges
`topology.pcurve-global-fidelity-unproved`. Witness failures charge
`topology.pcurve-endpoints-discontinuous` or
`topology.pcurve-locus-discontinuous` and omit the relation. Multiple
same-surface candidates stay detached and charge
`topology.pcurve-association-ambiguous`. The strict floor and the note
granularity are open items TP-11 and TP-12.

**Reopens.** A validated numeric bound — interval or affine arithmetic over
each parameter interval — supersedes the finite witness. Witness:
`finite_pcurve_admission_marks_unsampled_global_divergence`
(`reader/topology` tests, fixture `tp09_unsampled_divergence.p21`) holds an
input the finite rule admits while it diverges on an unsampled interval;
the test pins the warning, not fidelity.

**From.** TP-09.

## Product structure and presentation

### D-02. Product-definition view identity and projection order

**Question.** What identity does each `PRODUCT_DEFINITION` view receive,
and what order may a consumer read from a multi-view layer expansion?

**Silence.** ISO 10303-41 resolves product-definition association as a
`SET`, and `PRESENTATION_LAYER_ASSIGNMENT.assigned_items` is a `SET`. A
`SET` is unordered by definition, so no source view order exists. ISO
10303-21:2016 §11.2 states that entity instances need not be ordered, so
DATA serialization order carries no meaning either.

**Rule.** `step.md` §8 "CADIR decision D-02: each linked
`PRODUCT_DEFINITION`". Each definition is one view with its own identity.
Layer expansion emits views in `PRODUCT_DEFINITION` DATA record order as
deterministic projection order. `PresentationLayer.items` has no CADIR
semantic order.

**Ground.** Deterministic output for reproducible encode, goldens, and
diff. No invented semantic: the order is stated as projection, so no
consumer contract forms on it.

**Cost.** None. A producer cannot state a view order in these `SET`s, so
no source order exists to lose. DATA serialization order survives only as
the deterministic projection.

**Reopens.** A normative rule that defines view order or layer precedence
— an application-protocol edition or a CAx-IF recommended practice. This
condition is documentary; no executable witness can watch for it.

**From.** PS-04.
