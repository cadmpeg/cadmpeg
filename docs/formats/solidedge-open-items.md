# Solid Edge open items

### DR-1010. Active partition stream and framing

**Question.** Which streams contain the active Parasolid partitions, and how does each stream bound its partition bytes?

**Known.** `cadmpeg-parasolid` reads neutral-binary transmit headers, embedded schema edits, and primary node sequences.

**Need.** Establish the stream paths, framing fields, partition boundaries, and active-state selection from native document bytes.

### DR-1011. Governing versions and length units

**Question.** Which stored version fields govern the compound stream grammars, and which fields set the length unit of each partition?

**Known.** `cadmpeg-parasolid` reads a schema token from the transmit header and converts its supported model-space geometry from metres to millimetres.

**Need.** Establish the outer version declarations and the relationship between document display units and kernel coordinates.

### DR-1012. Base of the decimal length options

**Question.** Which length unit do the tenth, hundredth and thousandth options (length codes 68, 69 and 70) divide?

**Known.** The three options are length unit options and store codes 68, 69 and 70 in `DefaultDistancePrimary`.

**Need.** A part with a known edge length saved under each option, with the displayed value of that length.
