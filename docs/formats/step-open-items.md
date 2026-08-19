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

### ER-05. Local form of a resource token in a value position

**Question.** Which spelling of a resource token in a value position binds to the
ANCHOR table of the same exchange?

**Known.** `step.md` §7 "the process working directory. A fragment-only non-UUID
is resolved against" gives two rules: a fragment-only non-UUID resolves against
the ANCHOR table of the current exchange, and a URI with a nonempty path, query,
or scheme stays the exact external dependency. The lexer removes the `<` and `>`
delimiters, so `<#name>` gives the resource text `#name` and `<name>` gives the
resource text `name`.
`crates/cadmpeg-codec-step/src/parse.rs:862-865` keys the anchor map by the
anchor name, which holds no `#`.
`crates/cadmpeg-codec-step/src/parse.rs:2253` matches a resource against that map
by its full text. The text `#name` matches no key, so a fragment-only resource
stays unresolved. The text `name` matches the key `name`, so a relative-path
resource takes the value of the anchor.
`crates/cadmpeg-codec-step/src/parse.rs:870-888` applies this to each anchor
value, each anchor tag value, and each parameter of each DATA record. Two other
functions use the opposite rule:
`crates/cadmpeg-codec-step/src/parse.rs:2431-2440` needs an empty path and then
finds the anchor by the fragment, and
`crates/cadmpeg-codec-step/src/archive.rs:158-171` removes the `#` prefix and
then finds the anchor by that name. The codec holds no external-resource loss
code, so neither result is reported.

**Conflict.** The specification and two of the three resolvers make the
fragment-only form local and the relative-path form external. The third resolver
makes the relative-path form local and the fragment-only form external.

**Need.** The two forms have opposite meanings, so one spelling always gives the
wrong result. A relative-path resource that takes an anchor value replaces an
external dependency with local data. A fragment-only resource that stays
unresolved keeps a local binding as an external dependency. The answer gives the
one rule that the three resolvers share. Add a witness with an anchor named
`part`, one parameter `<#part>`, and one parameter `<part>`, and show which
parameter takes the anchor value.

## 3. Containers and other encodings

### CE-09. Unregistered root name in a schema object identifier

**Question.** Must a schema object identifier whose root component is an
unregistered ASN.1 identifier stay a valid identifier?

**Known.** `step.md` §6 "The ASN.1 identifiers `itu-t` and `ccitt` are root `0`,
`iso` is root `1`, and" gives the five ASN.1 identifiers that name a root, and
gives no root number to each other ASN.1 identifier. `step.md` §6 "CADIR
decision: an object identifier component number outside the range that" makes a
root component number that is not `0`, `1`, or `2` a recoverable defect: the
decode charges `metadata.schema-object-identifier-out-of-range`, and the
identifier stays invalid for a DATA section schema name and for a
`FILE_POPULATION` governing schema.
`crates/cadmpeg-codec-step/src/parse/schema_identifier.rs:291-298` gives the
number of each of the five names, and gives no number to each other name.
`crates/cadmpeg-codec-step/src/parse/schema_identifier.rs:165-182` tests the
range of a component that holds a number only. A root component that is an
ASN.1 identifier has the `Unnumbered` form, so no rule tests it, and
`crates/cadmpeg-codec-step/src/parse/schema_identifier.rs:234-240` gives the
second component no rule because the root number is absent. The identifier
`{ foo 40 }` is therefore valid, and the identifier `{ 3 40 }` is a charged
defect.

**Conflict.** One defect has two dispositions. A root component that is not a
registered root gets a charged warning and stays unadmitted when it is a
number, and gets no warning and full admission when it is a name.

**Need.** A valid identifier is admitted as a DATA section schema name and as a
`FILE_POPULATION` governing schema, so the disposition changes which records the
decode accepts. The decoder holds the evidence that separates the two cases,
because it lists the five names that give a root number. Give the rule that an
unregistered root name obeys, or give the reason that a name and a number that
both fail the root rule must not agree.

### CE-10. Repeated forwarding of a root ANCHOR resource

**Question.** How many times must a root `ANCHOR` forward a URI-valued binding
before the archive member check?

**Known.** `step.md` §7 "item is the referenced entity or value, and a
URI-valued anchor forwards the" gives that a URI-valued anchor forwards the
resolution again, and `step.md` §7 "central directory. Root ANCHOR forwarding is
resolved before this member" puts the forwarding before the member check.
`crates/cadmpeg-codec-step/src/archive.rs:158-171` removes one `#` prefix, finds
that anchor one time, and gives the `Resource` value of the anchor. It does not
repeat the step, and it gives the source URI back when the anchor value is not a
resource. `crates/cadmpeg-codec-step/src/archive.rs:139-146` then resolves the
result and refuses the exchange when the archive holds no such member.
`crates/cadmpeg-codec-step/src/archive.rs:90-95` gives an internal target in the
root member when the path is empty. For the bindings `<a>` to `<#b>` and `<b>` to
`<parts/child.p21#target>`, one step gives `#b`. That result has an empty path,
so it names the root member, the archive holds the root member, and the check
passes. The absent member `parts/child.p21` does not stop the decode, and the
note names the root member.

**Conflict.** The specification repeats the forwarding. The decoder forwards one
step only, so a chain of two or more anchors defeats the member check that the
specification puts after the forwarding.

**Need.** A missing subsidiary member must refuse the exchange, because the
decode cannot give the target. The answer repeats the forwarding with a limit on
the number of steps and a guard against a cycle, or gives the rule that one step
is the complete forwarding. Add a witness with a chain of two anchors whose last
URI names a member that the archive does not hold.

### CE-11. Decode limits on the ZIP root member parse

**Question.** Which decode limits apply to the parse of the ZIP root member?

**Known.** `crates/cadmpeg-codec-step/src/archive.rs:130` parses the root member
with `crate::parse::parse`, which takes no decode context.
`crates/cadmpeg-codec-step/src/codec.rs:85` parses with
`parse::parse_with_context` and gives it the context. Without a context the
parser charges no work and no retained storage, and it counts no nested step, so
the `max_recursion_depth` limit of the caller has no effect. The parser then
uses its own limits only. `crates/cadmpeg-codec-step/src/archive.rs:126-155`
uses this parse for the resource notes, and the reader parses the same member a
second time with a context.

**Need.** A caller that lowers its limits must get the same protection from each
route into the parser. The ZIP route parses the complete root member one time
outside the limits, so a root member that the limits refuse is fully parsed
first. The answer gives the context to this parse, or gives the reason that the
resource-note parse needs no limit. Add a witness with a lowered recursion limit
and a ZIP root member that the limit refuses.

## 4. Signatures

## 5. Topology and pcurve decisions

### TP-14. Per-relation status of an admitted pcurve

**Question.** Which channel gives the verification status of one admitted
pcurve relation?

**Known.** `crates/cadmpeg-codec-step/src/reader/topology.rs:1580` collects the
admitted relations of each committed body,
`crates/cadmpeg-codec-step/src/reader/topology/admissions.rs:27-52` formats the
one document warning that names the first 8, and
`crates/cadmpeg-codec-step/src/reader/topology.rs:499` reports it. The document holds no other record of an admitted
relation. The exactness map (`crates/cadmpeg-ir/src/annotations.rs:13-28`) is
the per-entity trust channel, and it uses a globally unique entity identity as
its key. A pcurve relation is a `PcurveUse` field of a `Coedge`
(`crates/cadmpeg-ir/src/topology.rs:196-205`), not an entity with an identity,
so the map holds no key for one relation. `Exactness`
(`crates/cadmpeg-ir/src/provenance.rs:68-78`) holds `ByteExact`, `Derived`,
`Inferred`, and `Unknown`. An admitted relation transfers the source value
without transformation and its global invariant is unproved, which no value
gives. `ModelDraft::exactness` (`crates/cadmpeg-ir/src/draft.rs:262-275`)
writes an entity value only. The STEP reader writes no exactness entry:
`crates/cadmpeg-codec-step/src/reader/mod.rs:386` builds
`SourceFidelity::default()`.

**Need.** A consumer that repairs or reviews one coedge must know whether its
pcurve relation holds an unproved invariant. The document warning gives a count
and 8 examples, so a relation outside those 8 has no query. The answer adds a
key for one pcurve use and a status value that means transferred and unproved.
Both changes are in `cadmpeg-ir` and reach each codec that writes exactness.

## 6. Units and measures

### UM-06. Unresolved uncertainty measure with a transferred projection

**Question.** Which channel reports a linear uncertainty measure that does not
resolve, when the document projection transfers a value?

**Known.** `crates/cadmpeg-codec-step/src/reader/geometry.rs:3333-3387` counts
each `GLOBAL_UNCERTAINTY_ASSIGNED_CONTEXT` measure that does not resolve. A
measure does not resolve when its record is absent, when it holds no number,
when it holds no unit reference, when its unit is not a length unit and not a
plane-angle unit, or when its scaled value is not finite or is not more than
zero. `crates/cadmpeg-codec-step/src/reader/geometry.rs:3400-3427` adds these
counts across all contexts and gives `LinearUncertainty::Value`, `Empty`, or
`Ambiguous`. Only `Empty` and `Ambiguous` keep the count.
`crates/cadmpeg-codec-step/src/reader/geometry.rs:333-354` reports
`geometry.uncertainty-length-unresolved` for `Empty`, and names the count in the
`geometry.uncertainty-length-ambiguous` note for `Ambiguous`. The `Value` arm
writes `ir.tolerances.linear` and reports nothing. `step.md` §8 "CADIR decision:
`Tolerances.linear` is the document projection of the" gives the same rule: it
asks for a note only when no candidate resolves.

**Need.** A measure that does not resolve is a candidate that cannot compete.
One distinct resolved value therefore does not show that the contexts agree. A
document that declares one value in one context, and a measure that does not
resolve in a second context, transfers the first value and keeps no record of
the second measure. `step.md` §8 "Geometric-consistency checks use the selected
document tolerance as their" makes `Tolerances.linear` the baseline of the
consistency checks, so a caller that repairs or examines the document must know
that a declared measure was dropped. The answer gives the count of measures that
do not resolve to the transferred-value result, or gives the reason that a
dropped measure needs no record.

## 7. Annotation, presentation, and tessellation

### AP-14. Override precedence for a complex styled item

**Question.** How must the reader find the attributes of an
`OVER_RIDING_STYLED_ITEM` that is a complex instance?

**Known.** `step.md` §8 "`OVER_RIDING_STYLED_ITEM` is the explicit precedence
relation: its style takes" gives the override precedence. In a complex instance,
each simple record holds the attributes of its own entity type only.
`OVER_RIDING_STYLED_ITEM` gives one attribute of its own, so its partial holds
one parameter. `crates/cadmpeg-codec-step/src/reader/presentation.rs:916-931`
reads the target from position `len - 2` and the style set from position
`len - 3` of that partial. Both positions are absent when the partial holds one
parameter, and the `?` operator then gives `None` for the full function, so the
`STYLED_ITEM` branch at
`crates/cadmpeg-codec-step/src/reader/presentation.rs:932-943` does not run.
`crates/cadmpeg-codec-step/src/reader/presentation.rs:237-254` collects only the
records that give parts, and then makes the set of overridden records from that
collection. A complex `OVER_RIDING_STYLED_ITEM` is therefore absent from both
sets: its style does not apply, its `over_ridden_style` is not marked, and the
overridden style applies. No loss records this. The test
`complex_styled_item_decodes_color_and_owns_its_curve` in
`crates/cadmpeg-codec-step/src/reader/presentation/tests.rs` writes the
`OVER_RIDING_STYLED_ITEM` partial with three parameters, so it holds the
inherited attributes and does not exercise the one-parameter form.

**Conflict.** The specification gives precedence to the overriding style. For a
complex instance in the form that Part 21 gives, the decoder applies the
overridden style instead, and reports nothing.

**Need.** A color that a file overrides is a visible result. The answer reads
the `STYLED_ITEM` partial for the inherited style set and target, and reads the
`OVER_RIDING_STYLED_ITEM` partial for `over_ridden_style`. Add a witness that
writes each partial with its own attributes only, and that shows the overriding
color in the output.

### AP-15. Transparency value outside its permitted range

**Question.** What must the reader do with a `SURFACE_STYLE_TRANSPARENT` value
that is finite and outside `0..=1`?

**Known.** `step.md` §8 "CADIR decision: if a malformed rendering record contains
multiple finite" makes more than one finite value a conflict that omits
transparency and records `presentation.surface-transparency-conflict`.
`crates/cadmpeg-codec-step/src/reader/presentation.rs:1278-1283` keeps a finite
value, and then removes each value outside `0..=1` before the count.
`crates/cadmpeg-codec-step/src/reader/presentation.rs:1284-1298` counts what
remains. A record with two finite values, of which one is outside the range,
therefore keeps one candidate and applies it, and no conflict loss is reported. A
record with one finite value outside the range keeps no candidate, so the
rendering becomes opaque, and no loss is reported. The specification gives no
range rule for this value.

**Need.** A dropped transparency gives an appearance that a consumer cannot
separate from a file that declares no transparency. The two rules also disagree:
the specification counts finite values, and the decoder counts values inside the
range. Give the range rule and its loss, or count each finite value as the
specification gives. Add a witness with one out-of-range value, and a witness
with one value inside the range and one value outside it.

## 8. Product structure and placement
