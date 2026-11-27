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

### DR-1016. Feature navigator final word with failed features and repeated suppression

**Question.** Does a feature whose recompute failed lower the contribution of an earlier suppressed feature to the final word, and which sequence of suppress and unsuppress actions leaves a residue in the word?

**Known.** One suppressed feature adds the number of later non-folder items, except one failed mirror copy that follows a suppressed extruded protrusion. A document where a feature was suppressed and then unsuppressed has a final word above `n` + 1, and the same suppression saved again keeps it.

**Need.** Documents with one failed feature and no suppression, one failed feature after a suppressed feature, and the same suppression applied, removed and applied again, with the final word read at each step.

### DR-1017. Group feature kind and feature operands

**Question.** Which field separates a mirror copy from a pattern in a `CLSID_JMGroup` feature, and where does a feature store its profile, extent and direction?

**Known.** A `CLSID_JMGroup` object has a different layout from a `CLSID_JMPart` object after its frame. A `CLSID_JMPart` object stores its function name at the position in §4.2.

**Need.** Mirror and pattern documents that differ in one parameter, and extrude documents that differ only in depth or direction.
