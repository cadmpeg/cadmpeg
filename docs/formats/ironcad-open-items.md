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

### DR-1065. Payload larger than one block

**Question.** How does a compressed stream frame store a payload longer than `block_size`?

**Known.** A frame header holds `magic`, `block_size` (`0x40000`), `payload_length`, and `compressed_length`, and one zlib stream inflates to exactly `block_size` bytes (`ironcad.md` §2). `payload_length` takes values less than `block_size`.

**Need.** Save a scene whose `Brep Data` payload exceeds 256 KiB, for example a part with many fillets, and read the frame sequence of that stream.

### DR-1066. Kernel record header words

**Question.** What do the record `marker` and the three `words` of a kernel record header encode?

**Known.** The marker is `0x0009b401`, the three words are each `1`, and the reserved word is `0` in every ACIS and Parasolid record (`ironcad.md` §3).

**Need.** Save the same part from another IronCAD release, and save a part with several bodies, then compare the record headers.

### DR-1067. Assembly membership and part binding

**Question.** Which field states the part records that an assembly record holds, and how does an assembly record nest in another assembly? Which field binds a part record to its kernel records?

**Known.** The part records come in the stored order of the part result records of `Brep Data` (DR-1063). A part record stores the matrix of its part frame in its parent, and an assembly record stores the matrix of the assembly in the scene (`ironcad.md` §5). An assembly record comes after the part records of its parts. An assembly record that follows two part records holds both. An assembly record that follows five part records holds the last two; the first three are parts of the scene. After the matrix, a part record stores the bytes `ff ff ff ff 00 0b 00 00 00` and an assembly record stores `ff ff ff ff 00 02 00 00 00`. The decoder binds each part record to the part result record of the same rank and charges `ironcad.placement.bound-by-order`. It reads the part records between an assembly record and the previous assembly record as the parts of that assembly, composes each part matrix with the assembly matrix, and charges `ironcad.assembly.membership-by-order`. It places each part as an occurrence of the root definition and charges `ironcad.assembly.flattened`.

**Need.** Save a scene with one part, then an assembly of two parts, then a second part added after the assembly. Then put that assembly inside a second assembly. Compare the record order of `Public Access` and the records of `Public Access Data` to find the field that names the parts of each assembly.

### DR-1068. Role of the second length entry of `Document Data`

**Question.** What does the second length entry of `Document Data` select?

**Known.** `Document Data` stores a length entry, an angle entry, a second length entry, and a mass entry (`ironcad.md` §4). The first length entry is the scene length unit. The second length entry stores the f64 `1.0` and the name `Centimeters` in a millimetre scene and in an inch scene; the SI scale of a centimetre is `0.01`. ACIS kernel lengths are centimetres (`ironcad.md` §3.2).

**Need.** Change each length unit setting of the scene that does not change the first entry, save after each change, and compare the second length entry of `Document Data`.

