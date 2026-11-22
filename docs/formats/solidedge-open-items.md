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

### DR-1015. Feature suppression, rollback and folders

**Question.** Which fields store a feature's suppressed state, the feature-tree rollback position, and the features a folder holds?

**Known.** The feature navigator object lists the features in order (§4.1). A folder is a `CLSID_JMFeatureGroup` entry of the navigator list. In documents without suppressed features and without a rollback, the navigator flag word is `0x08000003` or `0x08800013`.

**Need.** A document saved with one feature suppressed, one saved with the rollback position moved, and a no-edit resave of each.

### DR-1016. Feature navigator final word

**Question.** What does the final word of the feature navigator object count?

**Known.** For `n` listed features the word is `n` + 1 in most documents, `n` + 2 with a swept protrusion, and `n` with a lofted protrusion or a base tab followed by a flange.

**Need.** Documents that differ in one feature of each kind.

### DR-1017. Group feature kind and feature operands

**Question.** Which field separates a mirror copy from a pattern in a `CLSID_JMGroup` feature, and where do swept, lofted, hole, round, chamfer and thin-wall features store their operands?

**Known.** A `CLSID_JMGroup` object has a different layout from a `CLSID_JMPart` object after its frame. Extruded and revolved features store their parameters in the slots of §6.1. Swept and lofted features store their first profile at the §5.3 position.

**Need.** Mirror and pattern documents that differ in one parameter, and documents of each other feature kind that differ in one parameter.


### DR-1020. Second side word of extruded features

**Question.** What does the `Double Value Object` 156 bytes after the function name of an extruded or revolved feature state?

**Known.** The slot holds no object in the first feature of a document. In the later features of controlled saves it holds `2` for protrusions, including one extended against its profile normal, and `1` for cutouts, including one extended along its profile normal. Four protrusions in other documents hold `1`.

**Need.** The protrusions that hold `1` rebuilt in controlled saves, and a protrusion and a cutout that differ only in the boolean operation.

