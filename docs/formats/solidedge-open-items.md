# Solid Edge open items

### DR-1010. Active partition stream and framing

**Question.** How does the document select active partition indices and apply paired delta streams?

**Known.** Primary partitions use `PARASOLID/STREAM<n>.P_B`. Paired `.D_B` streams contain Parasolid delta transmit data.

**Need.** Establish the site-to-partition reference and the saved delta-state selector.

### DR-1011. Governing versions and length units

**Question.** Which outer version fields govern document streams, and which field declares document display units?

**Known.** The root contains `Versions`, `DocVersion2`, and `BuildVersions`. The embedded kernel token selects the Parasolid grammar. Kernel geometry uses metres, which the reader converts to CADIR millimetres.

**Need.** Establish the outer version record fields, version admission rules, and display-unit declaration.

### DR-1013. Intersection direction without a chart

**Question.** Which native rule fixes intersection-curve parameter direction when the chart supplies no usable direction?

**Known.** An intersection record references two support surfaces and a chart, and carries a sense byte. A chart carries base-parameter, scale, and point fields.

**Need.** Establish chart-null semantics, the effects of support order and support sense, and the native parameter gauge.

### DR-1053. Swept surface with a stated scale

**Question.** What does a stated `scale` of a swept surface (type 67) do to the sweep parameter?

**Known.** A swept surface stores a `section` curve, a unit `sweep` direction, and `scale`. With a null `scale` the sweep parameter is the distance along the direction.

**Need.** A part whose swept face stores a non-null `scale`, with its edge or pcurve parameters along the sweep.

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

### DR-1073. Offset end of extent kind 138

**Question.** From which end of the edge does the offset of extent kind `138` (§10.3) count, and do the bend method, bend deduction and bend allowance slots lie 44 bytes later than for the extent kind `134`, as they do for extent kind `140`?

**Known.** The offset and the width are two `CLSID_JDim` slots at offsets 492 and 512. The relief type is at offset 540. The offset equals the remaining edge length less the width in every stored flange of this kind, so both ends have the same setback.

**Need.** A flange of extent kind `138` whose offset differs from the remaining edge length less the width, on an edge of known direction.

### DR-1034. Flange constants and options

**Question.** Which options set the flange object slots that hold the same value in every stored flange, and which values can the offset kind, the dimension side and the corner relief take?

**Known.** The `Double Value Object` slots at offsets 268, 352 and 460 of §10.3, the constants that follow the relief type, and the zero-valued `CLSID_JDim` that follows them hold the same value in every stored flange. The field that selects the 8-byte header extension of a 563-byte edge proxy (§7.6) is not established.

**Need.** A flange with each offset type, the dimension side, the corner relief option, and a flange of each kind created by the Flange command.

### DR-1043. Member record fields and activation in larger families

**Question.** What do the 12 bytes after the first list, the class identifiers of the two blocks, the bytes after the second list before the state word, and the last 3 bytes of a second-list entry state (§9.5)? Which member is active in a family of three or more members, and what do the bits of the state word other than bit 1 state?

**Known.** Each block starts with `u32` `1` and ends with `u32` `0`. The 12 bytes are zero in every record. The 112 bytes after the second list, the state word excluded, are equal in every record of every family. The last 3 bytes of an entry are zero. The state word of the second record is 3 in both active states of a family of two members.

**Need.** Families of three or more members with each member active in turn, a family whose `FamilyInfo` names a member other than the first, and members that differ in replaced and moved components, with the stored member table of each state.
