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

### DR-1015. Feature suppression, rollback and folders

**Question.** Which fields store a feature's suppressed state, the feature-tree rollback position, and the features a folder holds?

**Known.** The feature navigator object lists the features in order (§4.1). A folder is a `CLSID_JMFeatureGroup` entry of the navigator list. In documents without suppressed features and without a rollback, the navigator flag word is `0x08000003` or `0x08800013`.

**Need.** A document saved with one feature suppressed, one saved with the rollback position moved, and a no-edit resave of each.

### DR-1016. Feature navigator final word

**Question.** What does the final word of the feature navigator object count?

**Known.** For `n` listed features the word is `n` + 1 in most documents, `n` + 2 with a swept protrusion, and `n` with a lofted protrusion or a base tab followed by a flange.

**Need.** Documents that differ in one feature of each kind.

### DR-1020. Second side word of extruded features

**Question.** What does the `Double Value Object` 156 bytes after the function name of an extruded or revolved feature state?

**Known.** The slot holds no object in the first feature of a document. In the later features of controlled saves it holds `2` for protrusions, including one extended against its profile normal, and `1` for cutouts, including one extended along its profile normal. Four protrusions in other documents hold `1`.

**Need.** The protrusions that hold `1` rebuilt in controlled saves, and a protrusion and a cutout that differ only in the boolean operation.

### DR-1021. Topology proxy selections

**Question.** Which edges and faces does a `CLSID_JMTopologyProxy` object select, and which face of a two-setback chamfer is its reference face?

**Known.** Rounds and chamfers name their edges and reference faces by `CLSID_JMTopologyProxy` identifiers (§7.3, §7.4). A proxy object is 559 bytes for an edge selection and 827 bytes for a chamfer reference face in the read documents.

**Need.** Documents that differ only in the selected edge of a round, and a two-setback chamfer and its copy with the setbacks swapped.

### DR-1022. Tapped hole thread data

**Question.** At which offset does a tapped hole's `CLSID_JMHolePropData` object store its thread diameters and designation?

**Known.** A tapped hole's property data holds two `f64` diameters in millimetres, then a `u32` character count and the UTF-16LE thread designation. The block's offset varies with the object length. A tapped M6 hole with a 12.7 mm by 12.7 mm counterbore stores a tap drill diameter of 5 mm, and its solid removes about 200 mm³ more than a 5 mm bore with that counterbore.

**Need.** Tapped holes of two thread sizes, with and without a counterbore.

### DR-1023. Swept, lofted and thin-wall operands

**Question.** Where do swept, lofted and thin-wall features store their paths, sections, thicknesses and open faces?

**Known.** Swept and lofted features store their first profile at the §5.3 position. Extruded, revolved, hole, round, chamfer, pattern and mirror features store their parameters as §6 to §8 state.

**Need.** Documents of each of these feature kinds that differ in one parameter.

### DR-1024. Circular pattern sense and mirrors of several seeds

**Question.** Which sense does a circular pattern turn about its axis, and where does a mirror of more than one seed store its plane?

**Known.** Every read circular pattern fills a full turn. Every read mirror has one seed and stores its plane at object offset 329 (§8.2).

**Need.** A circular pattern of three instances 30 degrees apart, and a mirror of two features.

### DR-1025. Round set kinds

**Question.** What does round set kind `2` state, and where does such a set store its size?

**Known.** Set kind `1` is a constant radius with the layout of §7.3. One read round has set kind `2`. Its feature object is longer and holds two more `CLSID_JMTopologyProxy` identifiers and two `CLSID_JDim` identifiers after the kind.

**Need.** A variable-radius round and a round with a setback or other non-constant section, each with a copy that differs in one value.

### DR-1026. Occurrence visibility

**Question.** Where does an assembly store that an occurrence is hidden?

**Known.** Hiding an occurrence leaves its `CLSID_JAsmReference` object (§9.2), `JAsmRefAttr`, `JSitesList` and `PSMroots` unchanged. It changes the 512-byte `JVisibleData` stream throughout, and that stream does not read as plain records.

**Need.** The encoding of `JVisibleData`, or an assembly saved with one occurrence hidden and its copy with the occurrence shown, plus copies with two and three hidden occurrences.

### DR-1035. Occurrence suppression

**Question.** Where does an assembly store that an occurrence is suppressed?

**Known.** The state word of a `CLSID_JAsmReference` object (§9.2) states grounding. It is `4` for a non-grounded occurrence whether or not the occurrence is suppressed.

**Need.** An assembly saved with one occurrence suppressed and its copy with the occurrence active, and the same pair with the occurrence grounded.
