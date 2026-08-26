# CATIA SN-17 owner numeric-lane audit

Date: 2026-08-26

This report records the release audit that exhausted the admitted fixed-nine
owner relations. It retains record offsets and decoded fields only. No source
bytes are copied.

## Release census

The release binary decoded 78 inputs successfully. The audit found 14,646
class-`0x62` owner packets:

| packet family | count |
| --- | ---: |
| fixed-nine | 951 |
| counted | 13,695 |
| fixed-nine, all-compact | 732 |
| fixed-nine, tagged-u16-strong | 112 |
| fixed-nine, width-coded-strong | 107 |

The fixed-nine relation presence matrix is:

| identity dialect | owner chart | face node | identity targets | boundary cycle |
| --- | ---: | ---: | ---: | ---: |
| all-compact | 15 | 703 | 464 | 0 |
| tagged-u16-strong | 0 | 0 | 112 | 2 |
| width-coded-strong | 30 | 62 | 46 | 0 |

The counts are independent fields. A packet can have identity targets without
having an owner chart or a face node. The chart relation therefore cannot be
inferred from the identity dialect or from target presence.

## Source-closed chart

The chart parser requires one contiguous seven-record source relation:

`carrier, bridge, B:18[05], B:18[09], B:18[0d], B:18[11], owner`.

One width-coded witness has record offsets:

| role | offset |
| --- | ---: |
| carrier | 602679 |
| bridge | 602884 |
| parameter point `05` | 602923 |
| parameter point `09` | 602954 |
| parameter point `0d` | 602985 |
| parameter point `11` | 603016 |
| owner | 603047 |

The owner stores lower `[19.949113350375008, 19.949113350374986]` and upper
`[139.64379345262492, 29.923670025562476]`. The four parameter records carry
the two constant-side tuples and the two orthogonal-side values required by
the chart grammar. The exact source relation closes the rectangle even though
the owner uses the width-coded identity dialect. The synthesized test
`owner_chart_applies_to_width_coded_identity_dialect` exercises the same rule
for `B:28`, `B:2b`, and `A:32` carriers.

This is the only general promotion justified by the audit: an admitted chart
establishes the rectangle from its carrier and side records. The identity
dialect does not establish the rectangle.

## Packets without a chart

The following fixed-nine width-coded packets have resolved source-local
identity targets but no chart relation:

| owner offset | lower | upper |
| ---: | --- | --- |
| 597097 | `[1.1926086126128234e-28, 35.92102448442021]` | `[314.1592653589792, 44.901280605525265]` |
| 599050 | `[-3.3128017017022767e-29, 206.54589078541738]` | `[314.1592653589792, 215.52614690652248]` |
| 656726 | `[0.0, 179.6051224221015]` | `[314.1592653589794, 193.0755066037591]` |

The tagged owner at offset `40482` has identity targets and a closed
source-local boundary cycle, but no chart relation. Its rectangle is
`[0.0, 0.0]` to `[49.53488615937236, 4.712388980384691]`. Numeric containment
against the NURBS carrier domains produces multiple candidates, so the
rectangle does not provide a unique carrier identity.

## Neutral topology falsification

Neutral sibling topology was used to test whether the binary32 triplets could
be promoted to face identity. The owner envelope at offset `40482` contains
seven face AABBs in the corresponding neutral model. Other tagged envelopes
in the same comparison contain six and five face AABBs. A separate comparison
has five width-coded envelopes, each containing three face AABBs. These are
valid envelopes, but they are not one-face witnesses.

The release reports agree with this boundary. A document with all-compact
owner witnesses binds 80 standard face surfaces through the existing
all-compact predicate. Representative tagged-heavy and width-only documents
bind zero through that predicate. This preserves evidence without assigning a
false face relation.

## Verdict

SN-17 is narrowed, not closed. The binary64 rectangle is settled for an owner
with an admitted source-closed chart, including width-coded owners. The
binary32 triplet is settled as a model-space face-boundary box only for
all-compact owners. Tagged and width-coded triplets remain non-identity
envelopes. Fixed-nine packets without a chart remain open until a serialized
carrier or allocation bridge crosses their source-local boundary. No
production admission change is justified by this audit.
