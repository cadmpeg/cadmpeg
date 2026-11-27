# IronCAD open items

### DR-1063. Kernel record roles in the record tree

**Question.** Which part, feature, or result body does each kernel record of `Brep Data` hold, and how does `Shape Data` refer to it?

**Known.** The `Brep Data` payload is a tree of length-prefixed nodes and kernel records (`ironcad.md` §3). The root node holds a sequence of pairs, each a child node followed by a record, then twelve empty nodes. The child node of a pair holds one or more groups of a 16-byte node (which holds one empty node) and a record; a child node with several groups ends with one empty node. A pair can also be a node that holds further pairs and no trailing empty nodes. The record inside a child node and the record that follows the child node hold different Parasolid or ACIS streams of the same kernel. A scene with one block part holds one pair.

The two ACIS streams of a one-block pair hold the same closed body. The body of the record inside the child node has box corners `(0, 0, 0)` and `(10, 5, 2)` cm and no transform. The body of the record that follows the child node has vertices from `(-5, -2.5, 1.2424559257230936e-27)` to `(5, 2.5, 2)` cm and an identity transform. A one-cylinder pair stores the same relation: corners `(0, 0, 0)` and `(5, 5, 5)` cm inside the child node, and corners `(-2.5, -2.5, 0)` and `(2.5, 2.5, 5)` cm after it. A part whose child node holds three groups is followed by one record. An assembly is a node that holds two pairs. A one-box Parasolid pair stores the same relation: corners `(0, 0, 0)` and `(0.05, 0.04, 0.025)` m inside the child node, and corners `(-0.025, -0.02, 0)` and `(0.025, 0.02, 0.025)` m after it. In a part whose child node holds three groups, the three inner records hold a 50 × 40 × 25 mm block, a cylinder of height 50 mm, and a cylinder of diameter 24 mm and height 40 mm. The record after that node holds one body of ten faces: seven planes, two cylinders, and one torus. `Shape Data` stores one matrix for each feature (`ironcad.md` §5). Its translation is the offset from the body of the feature record to the part frame: a 50 × 40 × 25 mm block feature whose body spans `(0, 0, 0)` to `(0.05, 0.04, 0.025)` m stores the identity rotation and the translation `(-0.025, -0.02, 0)` m, and a hole feature stores the rows `(1, 0, 0)`, `(0, 0, -1)`, and `(0, 1, 0)`. The decoder reads the record that follows a child node as the part result in the part frame and transfers its body. It reads each record inside a child node as a feature shape of that part, retains the kernel record, and charges `ironcad.body.feature-shape-retained`.

**Need.** Save a part with one block feature, then the same part after a second block is added, then after a hole is added. Compare the record trees and the `Shape Data` object graph to bind each record to a feature or to the part result.

### DR-1064. Native binary Parasolid transmit lead byte

**Question.** What does the lead byte `0x41` of a native binary transmit mean, and is the description length always a `u16`?

**Known.** A native binary transmit starts with `0x41`, a little-endian `u16` description length, the description, a little-endian `u32` schema-token length, and the token (`parasolid.md` §5). The neutral-binary header of `parasolid.md` §1 starts with `PS` and a big-endian `u32` description length. Each Parasolid kernel record of `Brep Data` stores a native binary transmit whose description is 51 bytes long.

**Need.** Save a scene whose kernel transmit has a description longer than 255 bytes, or a scene from another IronCAD release, and compare the leading bytes.

### DR-1066. Kernel record header words

**Question.** What do the record `marker`, the three `words`, and the `reserved` word of a kernel record header encode?

**Known.** The low byte of the marker is `0x01` (`ironcad.md` §3). The marker takes the values `0x00077c01`, `0x00086801`, `0x00087801`, `0x00088001`, `0x00088801`, `0x0008e801`, `0x00094c01`, and `0x0009b401`; one payload can hold records with different values. In the Parasolid records, a higher marker comes with an equal or later modeller version in the transmit description: `0x00077c01` with modeller version 2601178, `0x00086801` with 3101244, `0x00087801`, `0x00088001`, and `0x00088801` with 3400200, and `0x0008e801`, `0x00094c01`, and `0x0009b401` with 3401176. ACIS records store `0x0009b401`. The three words are each `1`. The reserved word is `0`, `1`, or `2`. The decoder charges `ironcad.record.header-unrecognized` for a record whose marker low byte differs from `0x01` or whose words differ from `1, 1, 1, 0`.

**Need.** Save the same part from two IronCAD releases and compare the markers. Save a part with several bodies, and a part whose reserved word is nonzero, then compare the record headers with the scene browser.

### DR-1067. Correspondence of `Public Access` records to `Shape Contents` objects

**Question.** Which field ties a `Public Access` record to its `Shape Contents` object (`ironcad.md` §§5, 6)? Which object does an object marker open when the first member of an assembly follows it? What do the copy paths after the first one in a copy record name, and which field marks a scene object that a STEP export omits?

**Known.** The `end` of a `Shape Contents` object binds the object to its part result record or assembly node of `Brep Data` (`ironcad.md` §6). The bytes `ff ff ff ff e1 ff` and a `u32` identifier come before the records of a scene object. The first marker after a part or assembly record carries the identifier of the next object, or of the assembly whose first member follows; the first member of an assembly has no marker of its own. A record with an empty name and a version `2` part copy header can be the first member of an assembly. Identifiers repeat between objects of different parents. A copy record holds up to four copy paths before its name; the first names the object whose body the copy places, and a later path can end in the identifier of a feature of that object. The records of the members of an assembly copy repeat the records of the source assembly, with the source identifiers. `Shape Contents` can list a part, after its feature records, whose body `Brep Data` stores and that a STEP export of the scene omits. The decoder binds each record to an unused object of its name and identifier, and of the kind that its role prefers; it binds by name alone when no object of that kind, name, and identifier exists, takes the first candidate after the last bound object, and reads a record whose objects are all bound as a repeat. It charges `ironcad.placement.bound-by-identity`. A record with an empty name and no candidate of its identifier takes the next unused object.

**Need.** Save a scene with two assemblies whose first members carry the same identifier, then an assembly copy, then a hidden part and a suppressed part. Compare the object markers of `Public Access` with the `Shape Contents` objects, and the hidden and suppressed parts with a visible part.

### DR-1068. Role of the second length entry of `Document Data`

**Question.** What does the second length entry of `Document Data` select?

**Known.** `Document Data` stores a length entry, an angle entry, a second length entry, and a mass entry (`ironcad.md` §4). The first length entry is the scene length unit. The second length entry stores the f64 `1.0` and the name `Centimeters` in a millimetre scene and in an inch scene; the SI scale of a centimetre is `0.01`. ACIS kernel lengths are centimetres (`ironcad.md` §3.2).

**Need.** Change each length unit setting of the scene that does not change the first entry, save after each change, and compare the second length entry of `Document Data`.

### DR-1069. Bytes before the scene record matrix

**Question.** What selects the short or the long form of the bytes between the `ModelGUID` text and the record matrix, and what do those bytes encode (`ironcad.md` §5)?

**Known.** The 32 bytes after the `ModelGUID` text are `00 00 02 00 00 00 00 00 00 0e 00 00 d8 13 00 00 d8 13 00 00` and three `u32` words `1`. In the short form, the flag byte `1` follows them and the matrix starts at offset 33. In the long form, the `u32` words `0` and `1` follow them, then the flag, and the matrix starts at offset 41. All part and assembly records of one `Public Access` payload that place the matrix at a fixed offset use the same form. In a version `3` part record, the `u32` words `0` and `1` follow the 32 bytes, then a property set of class `0x3de50020` whose properties `MassDensity`, `PartMass`, `PartName`, and `PartVolume` store strings, then the 17 bytes `00 02 00 00 00 00 00 00 0e 00 00 d8 13 00 00 d8 13`, 18 zero bytes, and the flag. The `PartName` of that set is `Body<n>`. The decoder names the body of a part that has one body by that `PartName` and charges `ironcad.property.unread` for the other properties of the set. In a version `2` part copy record, the part property set comes after the 32 bytes or after the 32 bytes and the words `0` and `1`, and the flag follows it. The decoder reads the matrix of the short and the long form at its fixed offset. For any other form, it reads the first rigid matrix after a flag byte as the record matrix and charges `ironcad.placement.matrix-searched`.

**Need.** Save the same one-part scene from two IronCAD releases, then save it again after a change of the part material, and compare the bytes between the `ModelGUID` text and the matrix.
