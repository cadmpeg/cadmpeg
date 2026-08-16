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

## 2. AP242 BO-Model sidecars

## 3. Containers and other encodings

## 4. Signatures

### SG-04. Signature verification result

**Question.** Which validation conditions make each signature valid, invalid, or indeterminate?

**Known.** Part 21 requires a detached CMS `SignedData` object and defines
the exact external content and alphabet projection. RFC 5652 supplies the
message-digest, signed-attribute, signature, certificate, and algorithm
processing rules. A valid result also needs a certificate-chain, key-usage,
revocation, time, and trust-anchor policy. An invalid result covers malformed
CMS, a digest mismatch, a signature mismatch, or a failed required policy;
indeterminate covers unavailable content, keys, certificates, or policy
evidence.

**Note.** Part 21 does not prescribe a trust store, revocation protocol,
clock policy, or caller authorization policy. Verification therefore needs a
caller-supplied CMS and trust-policy contract; structural CMS parsing alone
cannot produce a valid/invalid result.

**Need.** We must know the conditions to report a signature verification result.

## 5. Topology and pcurve decisions

## 6. Units and measures

## 7. Annotation, presentation, and tessellation
