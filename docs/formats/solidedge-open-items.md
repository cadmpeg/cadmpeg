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

### DR-1016. Feature navigator final word after suppression

**Question.** Which state raises the final word of the feature navigator object above the number of feature-tree items that are not folders, plus one, and by how much?

**Known.** The word is that number plus one in every document without a suppressed feature. Suppressing an extruded cutout raises it by 2, and suppressing a mirror copy raises it by 1. Suppressing a pattern does not raise it.

**Need.** Documents that differ in one suppression, with the final word read before and after each change.

### DR-1021. Topology proxy selections

**Question.** What do the `f64` values after the axis of a circular-edge proxy store, which face of a two-setback chamfer is its reference face, and where does a thin wall with several open faces store the faces after the first?

**Known.** The geometry records of straight-edge, planar-face and vertex proxies are in §7.5. A circular-edge proxy holds the center and the axis of the circle and then five `f64` values, the first of which is the radius in one read document. A thin wall stores a proxy of 881 bytes 152 bytes after the end of its function name, whatever number of faces the thin wall opens.

**Need.** A round on a circular edge of a second radius, a two-setback chamfer and its copy with the setbacks swapped, and a thin wall whose volume shows two open faces.

### DR-1022. Tapped hole thread data

**Question.** At which offset does a tapped hole's `CLSID_JMHolePropData` object store its thread diameters and designation?

**Known.** A tapped hole's property data holds two `f64` diameters in millimetres, then a `u32` character count and the UTF-16LE thread designation. The block's offset varies with the object length. A tapped M6 hole with a 12.7 mm by 12.7 mm counterbore stores a tap drill diameter of 5 mm, and its solid removes about 200 mm³ more than a 5 mm bore with that counterbore.

**Need.** Tapped holes of two thread sizes, with and without a counterbore.

### DR-1025. Round set kinds

**Question.** What do the blend shape and the setback of a round set store?

**Known.** Set kind `1` is a constant radius and set kind `2` is a variable radius (§7.3). A corner round with unequal radii holds one set of kind `1` for each edge.

**Need.** A round with a conic or G2 blend shape and a round with a setback, each with a copy that differs in one value, saved from the Solid Edge user interface.

### DR-1023. Swept, lofted and thin-wall operands

**Question.** Where do swept, lofted and thin-wall features store their paths, sections, thicknesses and open faces?

**Known.** Swept and lofted features store their first profile at the §5.3 position. Extruded, revolved, hole, round, chamfer, pattern and mirror features store their parameters as §6 to §8 state.

**Need.** Documents of each of these feature kinds that differ in one parameter.

### DR-1024. Circular pattern sense and mirrors of several seeds

**Question.** Which sense does a circular pattern turn about its axis, and where does a mirror of more than one seed store its plane?

**Known.** Every read circular pattern fills a full turn. Every read mirror has one seed and stores its plane at object offset 329 (§8.2).

**Need.** A circular pattern of three instances 30 degrees apart, and a mirror of two features.
