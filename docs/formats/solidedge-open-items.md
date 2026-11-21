# Solid Edge open items

### DR-1010. Active partition stream and framing

**Question.** Which streams contain the active Parasolid partitions, and how does each stream bound its partition bytes?

**Known.** `cadmpeg-parasolid` reads neutral-binary transmit headers, embedded schema edits, and primary node sequences.

**Need.** Establish the stream paths, framing fields, partition boundaries, and active-state selection from native document bytes.

### DR-1011. Governing versions

**Question.** Which stored version fields govern the compound stream grammars?

**Known.** `cadmpeg-parasolid` reads a schema token from the transmit header. Parasolid model-space lengths are in metres.

**Need.** Establish the outer version declarations and the stream grammars each declaration selects.

### DR-1012. Remaining unit codes

**Question.** Which `DefaultDistancePrimary` and `DefaultAnglePrimary` codes name the length and angle units outside the settled tables?

**Known.** Codes 59, 61, 62 and 64 name metre, millimetre, centimetre and inch. Codes 77 and 357 name radian and degree.

**Need.** One document per remaining length-unit and angle-unit setting, each differing from a millimetre and degree document only in that setting.
