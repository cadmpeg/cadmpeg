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

**Question.** Which faces does a `CLSID_JMTopologyProxy` object select, how does an edge proxy select a curved edge, what do the `f64` values after the axis of a circular-edge proxy store, which face of a two-setback chamfer is its reference face, where does a thin wall with several open faces store the faces after the first, and how does a proxy name the topology of its creating feature?

**Known.** The geometry records of straight-edge, planar-face, vertex and circular-edge proxies are in §7.6. A 559-byte proxy selects one straight edge. It holds `16`, `0x80000000` and the identifier of a `CLSID_JMPart` feature at object offset 360. In every read proxy that feature is the only feature before the selecting feature, so whether it is the feature that created the edge is not established. A byte at object offset 70 differs between edges of one feature. An 827-byte proxy selects the reference face of a chamfer or the open faces of a thin wall. A circular-edge proxy holds the center and the axis of the circle and then five `f64` values, the first of which is the radius in one read document. A thin wall stores a proxy of 881 bytes 152 bytes after the end of its function name, whatever number of faces the thin wall opens.

**Need.** A round on a circular edge, a round on a circular edge of a second radius, a two-setback chamfer and its copy with the setbacks swapped, a thin wall with two open faces whose volume shows both, and a chamfer on an edge that a cut created.

### DR-1025. Round set kinds

**Question.** What do the blend shape and the setback of a round set store?

**Known.** Set kind `1` is a constant radius and set kind `2` is a variable radius (§7.3). A corner round with unequal radii holds one set of kind `1` for each edge.

**Need.** A round with a conic or G2 blend shape and a round with a setback, each with a copy that differs in one value, saved from the Solid Edge user interface.

### DR-1027. Tapped hole with counterbore

**Question.** What does the thread depth of a tapped hole store, and which stored value sizes the solid of a hole of type `3` that has a counterbore?

**Known.** A tapped M6 hole of type `3` with a 12.7 mm by 12.7 mm counterbore stores a tap drill diameter of 5 mm and the thread values of §7.2. Its solid removes about 200 mm³ more than a 5 mm bore with that counterbore. A plain hole with a 5 mm drill and M6 thread values removes exactly a 5 mm bore. A tapped hole with no selected thread and a thread depth of 8 mm stores `8` at offset 92 of its properties object in four documents with different counterbores.

**Need.** A tapped hole with a selected thread and a counterbore, and two copies of it that differ in the thread depth only.

### DR-1028. Lofted feature operands

**Question.** Where does a lofted feature store its guide curves, its centreline and its closed and ruled options?

**Known.** A lofted feature stores its section profiles at the §5.3 positions. After the section list it stores 12 bytes of reference words and then the identifier of a `CLSID_JMVertexMap` object.

**Need.** A loft with one guide curve, a loft with a centreline, and a closed loft, each with a copy that moves the curve or changes the option.
