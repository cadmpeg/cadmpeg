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

### DR-1066. Live copies of an assembly pattern

**Question.** Which `CLSID_JAsmReference` objects of an assembly with a pattern are live instances, and what does a copy whose indices repeat another copy's stand for?

**Known.** A pattern lists its copies as `CLSID_JAsmPatternOcc` objects (§9.5). Some read patterns have a copy that repeats another copy's indices, and a `CLSID_JAsmReference` object at a copy's placement that no copy object names. The references collection and the structure list (§9.4) list every such object.

**Need.** A pattern built at its final count and the same pattern with its count raised in a second edit, over the same seed and layout.

### DR-1067. Mirror plane of an assembly mirror

**Question.** Where does an assembly mirror store its plane, its seed and the reversal that makes the copy an occurrence rather than a reflection?

**Known.** A mirrored occurrence is a `CLSID_JAsmReference` object whose rotation has determinant `+1` (§9.6). A `CLSID_JAsmGroup` object stores its identifier at offset 17.

**Need.** Mirrors of one part about planes along each coordinate axis, and a mirror of a part that has no symmetry about the plane.

### DR-1068. Declaration link word

**Question.** What does the `u16` at offset 22 of a type table declaration state?

**Known.** The value is `0xFFFF` for every declaration except `ReferencePlaneObject` and `JAsmRefPlane` in a document that holds an assembly layout or a mirror group, where it is `0x000E`. The class rule (§3.3) does not use it.

**Need.** Documents that add a user reference plane to an assembly without a layout or a mirror.
