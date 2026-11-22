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

### DR-1012. Base of the decimal length options

**Question.** Which length unit do the tenth, hundredth and thousandth options (length codes 68, 69 and 70) divide?

**Known.** The three options are length unit options and store codes 68, 69 and 70 in `DefaultDistancePrimary`.

**Need.** A part with a known edge length saved under each option, with the displayed value of that length.

### DR-1015. Group feature suppression and folder names

**Question.** Which field stores the suppressed state of a `CLSID_JMGroup` feature, and where is the name of a feature-tree folder stored?

**Known.** A `CLSID_JMPart` feature stores its suppressed state in bit `0x10` of its state word (§4.3). The `u32` at object offset 8 of a `CLSID_JMGroup` object is `1` in documents without suppressed features. A `CLSID_JMFeatureGroup` object lists the folder members (§4.3).

**Need.** A document with one suppressed mirror copy or pattern and its resave without suppression; a document with one renamed folder and its resave with the default name.

### DR-1016. Feature navigator final word

**Question.** What does the final word of the feature navigator object count?

**Known.** For `n` listed features the word is `n` + 1 in most documents, `n` + 2 with a swept protrusion, and `n` with a lofted protrusion or a base tab followed by a flange.

**Need.** Documents that differ in one feature of each kind.

### DR-1017. Group feature kind and feature operands

**Question.** Which field separates a mirror copy from a pattern in a `CLSID_JMGroup` feature, and where does a feature store its profile, extent and direction?

**Known.** A `CLSID_JMGroup` object has a different layout from a `CLSID_JMPart` object after its frame. A `CLSID_JMPart` object stores its function name at the position in §4.2.

**Need.** Mirror and pattern documents that differ in one parameter, and extrude documents that differ only in depth or direction.
