# Rhino 3DM Open Items

Settled format rules remain in
[`rhino_3dm.md`](rhino_3dm.md). OpenNURBS transfer evidence remains in
[`rhino_3dm-opennurbs-comparison.md`](rhino_3dm-opennurbs-comparison.md).

## Remaining items

### FV-02. Layer table item 37

**Question.** What is the wire grammar and CADIR field for the current
OpenNURBS layer extension item 37?

**Known.** `ON_Layer::Write` in `origin/9.x/opennurbs_layer.cpp` emits item
byte 37 and a UTF-16 string when `Description()` is nonempty, after the
packed layer 1.15 fields and before item zero. `ON_Layer::Read` consumes the
same string and then the next item byte. The packed layer version remains
1.15; item 37 is a tagged extension, not a new minor gate. The current codec
stops typed parsing at this item and retains the complete layer record.

**Need.** The specification grammar, source default and normalization rule,
native field type, and decoder transfer for item 37, gated by a producer
witness and byte inspection.

**Note.** Reopened 2026-08-17 after the post-closure `origin/8.x..origin/9.x`
source diff found a current writer branch omitted from the prior table-record
inventory. The differential witness and OpenNURBS readback establish the
field meaning; implementation and specification alignment remain.
