# CATIA fixed-nine boundary-cycle witnesses

Date: 2026-08-26

This report records the source-scoped evidence for SN-37. It does not assign a
standard face identity to a fixed-nine cycle.

## Admitted cycle

The decoder admits a cycle only when a fixed-nine owner resolves four class
`0x5e` targets, each target resolves two class `0x5d` endpoint records, and the
four endpoint pairs form one simple four-vertex cycle. A preceding class
`0x5f` prelude is retained only when its bounded span and terminal controls
match the fixed-nine grammar. The retained relation keeps source index, owner
offset, face-node offset, target slots, target offsets, and endpoint offsets.

The release witness sweep covered 54 CATPart inputs. Fifty-three completed with
status 0 and one reached its per-input timeout. Exactly two successful inputs
contained one admitted closed four-edge cycle each.

| witness | owner | face node | class-`0x5e` target offsets | class-`0x5d` endpoint offsets |
| --- | ---: | ---: | --- | --- |
| A | 61352 | 61235 | 61248, 61283, 61318, 61335 | 61265, 61274, 61300, 61309 |
| B | 40482 | 40365 | 40378, 40413, 40448, 40465 | 40395, 40404, 40430, 40439 |

The offsets in the table are source-relative byte offsets. The cycle uses the
existing `b2 03 5d` and `b2 03 5e` records. The source-scoped prelude and
terminal controls use the admitted `b5 03 5f` and `27 03`/`27 05` forms.

## Source boundary

For both witnesses, the cycle references, owner references, and endpoint
network remain inside the bounded allocation source that contains the owner.
The standard support tags are decoded from a different consolidated record
source. The cycle-reference intersection with the standard support-tag set is
empty. B5 object-stream walks over the available logical streams produce no
reference from a cycle face, curve, edge, endpoint, or owner value to a
standard support tag.

The native records therefore establish a closed source-local boundary relation,
not a cross-source face or edge-support identity. The decoder retains that
relation and does not feed it into standard face-domain assignment.

## Class-`0x5b`/`0x5c` exhaustion

The corrected release audit covered 53 successful inputs and retained 824
complete control records: 557 class-`0x5b` records and 267 class-`0x5c`
records. Only the two closed-cycle witnesses contain a control pair in the
cycle's bounded neighborhood. In the first witness the pair has source offsets
`13718` and `13754` and file offsets `61085` and `61121`; in the second it has
source offsets `7580` and `7614` and file offsets `40217` and `40251`.

The first pair has class-`0x5b` length 36 and class-`0x5c` length 14. The
second pair has lengths 34 and 19. Each of those length/header shapes also
occurs outside a closed cycle. The retained payloads contain no admitted
allocation reference, owner position, face-node target, or standard support
tag. The adjacent cycle edge nodes retain source-local curve references and
source-local endpoint records only. This audit supplies no allocation-scoped
bridge and no additional solver constraint.

## Neutral comparison

An exact neutral sibling check confirms that the emitted face boundary
signatures agree when the two models use the same split cardinalities. A second
check has different face and edge split cardinalities. These checks validate
topology transfer and expose exporter split policy, but neither supplies the
allocation-scoped identity relation required by SN-37.

The cycle source also contains class-`0x5b` and class-`0x5c` records with
bounded control lanes. Their admitted grammar does not contain a demonstrated
reference to a standard support tag or to the cycle's owner identity. They
remain unclassified and are not used as a join key.

## Result

SN-37 remains open. The fixed-nine cycle parser is source-closed and complete
for the admitted role. No principled implementation change is made in this
slice. A future change requires a serialized relation that crosses the bounded
source boundary; geometry-only matching, local allocation values, and sibling
topology are insufficient.
