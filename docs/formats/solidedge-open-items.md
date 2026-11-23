# Solid Edge open items

### DR-1009. Document identity and kind

**Question.** Which stored field distinguishes part, sheet-metal, and assembly documents?

**Known.** The root streams `PSMroots` and `JSitesList` identify the compound document family. Part and sheet-metal documents carry the same primary partition path pattern. Assembly sites carry `JAsmRefAttr` streams.

**Need.** Establish a document-kind discriminator and its value domain.

### DR-1010. Active partition stream and framing

**Question.** How does the document select active partition indices and apply paired delta streams?

**Known.** Primary partitions use `PARASOLID/STREAM<n>.P_B`. Paired `.D_B` streams contain Parasolid delta transmit data. The reader projects primary node sequences and reports its primary-state assumption.

**Need.** Establish the site-to-partition reference and the saved delta-state selector.

### DR-1011. Governing versions and length units

**Question.** Which outer version fields govern document streams, and which field declares document display units?

**Known.** The root contains `Versions`, `DocVersion2`, and `BuildVersions`. The embedded kernel token selects the Parasolid grammar. Kernel geometry uses metres, which the reader converts to CADIR millimetres.

**Need.** Establish the outer version record fields, version admission rules, and display-unit declaration.

### DR-1013. Intersection direction without a chart

**Question.** Which native rule fixes intersection-curve parameter direction when the chart supplies no usable direction?

**Known.** An intersection record references two support surfaces and a chart, and carries a sense byte. A chart carries base-parameter, scale, and point fields. The reader uses chart order for a resolved direction and ordered oriented support normals for a resolved analytic section otherwise.

**Need.** Establish chart-null semantics, the effects of support order and support sense, and the native parameter gauge.

### DR-1053. Swept surface with a stated scale

**Question.** What does a stated `scale` of a swept surface (type 67) do to the sweep parameter?

**Known.** A swept surface stores a `section` curve, a unit `sweep` direction, and `scale`. With a null `scale` the sweep parameter is the distance along the direction. The reader keeps a swept surface with a stated `scale` as an unsupported carrier.

**Need.** A part whose swept face stores a non-null `scale`, with its edge or pcurve parameters along the sweep.
