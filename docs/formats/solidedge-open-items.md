# Solid Edge open items

### DR-1009. Document identity and kind

**Question.** Which directory entries and stored fields identify a Solid Edge document and distinguish part, sheet-metal, and assembly documents?

**Known.** The codec requires the root paths `JGlobal` and `JSitesList` in a CFB directory. Detection applies the same predicate to each input extension.

**Need.** Establish the directory signature and the document-kind field from native document bytes.

### DR-1010. Active partition stream and framing

**Question.** Which streams contain the active Parasolid partitions, and how does each stream bound its partition bytes?

**Known.** `cadmpeg-parasolid` reads neutral-binary transmit headers, embedded schema edits, and primary node sequences.

**Need.** Establish the stream paths, framing fields, partition boundaries, and active-state selection from native document bytes.

### DR-1011. Governing versions and length units

**Question.** Which stored version fields govern the compound stream grammars, and which fields set the length unit of each partition?

**Known.** `cadmpeg-parasolid` reads a schema token from the transmit header and converts its supported model-space geometry from metres to millimetres.

**Need.** Establish the outer version declarations and the relationship between document display units and kernel coordinates.
