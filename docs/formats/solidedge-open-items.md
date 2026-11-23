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

**Question.** Which faces does a `CLSID_JMTopologyProxy` object select, how does an edge proxy select a curved edge, and how does a proxy name the topology of its creating feature?

**Known.** A 559-byte proxy selects one straight edge by its start point, direction and length (§7.6), and holds `16`, `0x80000000` and the identifier of a `CLSID_JMPart` feature at object offset 360. In every read proxy that feature is the only feature before the selecting feature, so whether it is the feature that created the edge is not established. A byte at object offset 70 differs between edges of one feature. An 827-byte proxy selects the reference face of a chamfer or the open faces of a thin wall.

**Need.** A round on a circular edge, a thin wall with two open faces, and a chamfer on an edge that a cut created.

### DR-1025. Round set kinds

**Question.** What does round set kind `2` state, and where does such a set store its size?

**Known.** Set kind `1` is a constant radius with the layout of §7.3. One read round has set kind `2`. Its feature object is longer and holds two more `CLSID_JMTopologyProxy` identifiers and two `CLSID_JDim` identifiers after the kind.

**Need.** A variable-radius round and a round with a setback or other non-constant section, each with a copy that differs in one value.

### DR-1026. Occurrence visibility

**Question.** Where does an assembly store that an occurrence is hidden?

**Known.** Hiding an occurrence leaves its `CLSID_JAsmReference` object (§9.2), `JAsmRefAttr`, `JSitesList` and `PSMroots` unchanged. It changes the 512-byte `JVisibleData` stream throughout, and that stream does not read as plain records.

**Need.** The encoding of `JVisibleData`, or an assembly saved with one occurrence hidden and its copy with the occurrence shown, plus copies with two and three hidden occurrences.

### DR-1027. Tapped hole with counterbore

**Question.** Which stored value sizes the solid of a tapped hole that has a counterbore?

**Known.** A tapped M6 hole with a 12.7 mm by 12.7 mm counterbore stores a tap drill diameter of 5 mm and the thread values of §7.2. Its solid removes about 200 mm³ more than a 5 mm bore with that counterbore. A plain hole with a 5 mm drill and M6 thread values removes exactly a 5 mm bore.

**Need.** Tapped holes with counterbores of two depths and two diameters.

### DR-1028. Lofted feature operands

**Question.** Where does a lofted feature store its sections and guide curves?

**Known.** A lofted feature stores its first profile at the §5.3 position.

**Need.** Lofts between two and three sections, and a copy of each with one section moved.

### DR-1031. Bend deduction and other bend methods

**Question.** How does a bend take its flat length under bend method `1` (§10.1), and which other values can the bend method take?

**Known.** Bend method `1` is bend deduction. A document with bend method `1` keeps its neutral factor, bend deduction and bend allowance values unchanged. The bend deduction and bend allowance values do not change with the material thickness.

**Need.** Flanges at two angles under bend method `1`, and documents set to each other bend method the software offers.

### DR-1055. Bend relief length and shape

**Question.** Which dimension of a bend relief does the relief length (§10.1) set?

**Known.** The relief width and relief length variables hold the values set as the document defaults. A flange stores its own relief width, relief length and relief type (§10.3).

**Need.** A flange with a bend relief at two relief lengths, and the flat pattern of each.

### DR-1034. Flange constants and options

**Question.** Which options set the flange object slots that hold the same value in every stored flange, and which values can the side, the relief type and the object header flag take?

**Known.** The `Double Value Object` slots at offsets 212, 268, 352 and 460 of §10.3, the constants that follow the relief type, and the zero-valued `CLSID_JDim` that follows them hold the same value in every stored flange. The side is `2` in every stored flange. The relief type is `51`, `52` or `53`. The `u16` at object offset 18 of a flange object is `149` where the object has a relation and `157` where it has none. The field that selects the 8-byte header extension of a 563-byte edge proxy (§7.6) is not established.

**Need.** Flanges with the side reversed, each other relief type, each other measure-point and material-side option, and the corner relief option, and a flange of each kind with its radius, relief width and relief length set on the flange.

### DR-1059. Hem forms, length offset and side

**Question.** Which sheet dimension offsets the straight length of a hem from the entered hem length, which slots hold the dimensions of the double, teardrop and rolled forms, and which field states the side a hem folds to?

**Known.** The single hem (form code 208) stores its inner bend radius at offset 208 and its straight length at offset 236 of §10.4. The entered length exceeds the straight length by the inner radius and a further offset. The form codes 209, 210 and 212 store a value at each of offsets 208 and 236 that no field of the single hem explains. Reversing the side of a hem changes the stored edge selection and no other slot.

**Need.** Single hems on sheets of at least two other thicknesses, each other form saved with two different gaps and lengths, and the same hem folded to each side on one edge with the edge selections compared against the stored geometry.

### DR-1056. Base contour flange extent direction and fields

**Question.** Which of the slots at offsets 124 and 180 of §10.5 states the side of the extent, what do the remaining slots of the feature state, and where is the thickness side stored?

**Known.** The extent kind at offset 152 is 13. The slots at offsets 124 and 180 both hold 1. The slots from offset 292 on hold relief and corner codes, among them 57, 44, 76, 5, 79 and 80.

**Need.** Base contour flanges with the side reversed, the symmetric extent, each corner relief option, and the thickness side reversed.

### DR-1057. Unfold and refold operands

**Question.** Which `CLSID_JMTopologyProxy` of an unfold selects the stationary face and which selects the bends, and how does a proxy of a face or a bend store its selected entity?

**Known.** An unfold stores two face proxies of 1232 and 1410 bytes. A refold stores one proxy of 1232 bytes when it follows the unfold of the same bend, and one of 2046 bytes after a cut made in the flat state.

**Need.** Unfolds and refolds of several bends with the stationary face moved, and a topology proxy parser that reads a face selection.

### DR-1058. Close corner slots

**Question.** What do the closure type at offset 124, the slots at offsets 272, 300, 336, 364 and 392, and the relief form codes other than 106, 107 and 222 of §10.7 state?

**Known.** The slot at offset 124 holds 103, and the slot at offset 392 holds pi / 4 in every stored close corner. The corner gap, the relief form and the relief size change with the entered values.

**Need.** Close corners with each closure type, with each relief form and with a corner between flanges of an angle other than a right angle.

