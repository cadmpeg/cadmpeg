# SolidWorks eDrawings package: Open Items

This document lists the parts of the eDrawings `.eprt` and `.easm` formats that we do not know. The specification `edrawings.md` gives the parts that we know.

Each item has these parts:

- **Question** — what we must find.
- **Known** — what the specification gives now.
- **Need** — why we must find the answer.
- **Conflict** — a disagreement between two documents, or between a document and the decoder. An item with this part needs a decision.
- **Note** — a defect in the item or in the specification.

When an item is resolved, delete it in the same change that writes the answer into the specification. Do not keep a Resolved part.

Each item has an identifier. Use the identifier in commit messages and in code comments.

This document uses ASD-STE100 Simplified Technical English. Record names, field names, and tag values are technical names.

## 1. Container and wrapper

### EDR-01. Wrapper record meaning

**Question.** What do the `u32` of the `I` record and the flag byte of the `B` record mean? Which other wrapper tags occur?

**Known.** `edrawings.md` §2.1 gives the layout. Both samples hold 4 and 0. The reader accepts only the tags `I`, `B`, and `Z`.

**Need.** We must know whether a file with another value or another tag changes how we read the model stream.

### EDR-02. Bounds unit and frame

**Question.** In which unit and which frame are the bounds of the `B` record?

**Known.** The six values form a small box near the origin in both samples.

**Need.** We must convert the lengths of the model stream to millimetres.

### EDR-03. Records with unknown meaning
**Question.** What do these fields mean: the constant byte after the `S` opcode of a shell, the first mask byte of the `~` record, the flag byte of the `&` record, the `u32` of the `0` record, the `0xff` flag of the `-` and `0x0d` records, the symbol code of the `@` record, and the content of the header block, the camera, and the texture matrix?

**Known.** `edrawings.md` §3.3 gives the length of each record. The decoder checks the length and ignores the values. The symbol code 9 occurs in `star` segments and the code 7 in `plus` segments.

**Need.** We must know whether a value other than the observed one changes how we read the geometry or the colour.

### EDR-04. Other stream versions

**Question.** Which stream versions other than `19.10`, `19.00`, and `18.00` occur, and do they change the wrapper or the model stream?

**Known.** `edrawings.md` §2.2 maps `19.10`, `19.00`, and `18.00` to a dialect. Any other version is read with the rules of `19.10` and reported as `edrawings/source.dialect-unverified`.

**Need.** We must know whether the dialect rows `edrawings:hsf-19-10`, `edrawings:hsf-19-00`, and `edrawings:hsf-18-00` cover other files.

### EDR-05. Tail byte

**Question.** What does the zero byte after the zlib stream mean, and can the tail be longer?

**Known.** `edrawings.md` §2.3 gives one zero byte in both samples.

**Need.** We must know whether the tail carries data that the codec must transfer.

### EDR-06. Auxiliary members

**Question.** What are the grammars of `scene.xml`, `materials.xml`, `FileProperties.xml`, and `OLE`, and how do they relate to the model stream?

**Known.** `edrawings.md` §1.2 names the members. The codec lists them and does not read them.

**Need.** We must know them to transfer materials, document properties, and lighting.

## 2. Model stream
### EDR-07. Attribute masks
**Question.** What do the bits of the mask words of the `H` and `R` records mean, and how many extra bytes does each bit add? What do the other forms of the `V` record hold?

**Known.** `edrawings.md` §3.4 and §3.11 give the lists and the forms that were seen. The reader refuses any other list or form. The `R` record with mask `0x00100000` holds 16 bytes and a run of the pairs `ff 01` or of the bytes `ff` of unknown meaning. The `R` list `0x80010000 0x80000000 0x00000002` has one extra byte of unknown meaning. The `H` records with masks `0x0002` and `0x0004` and the `V` record with the bytes `40 00` have unknown meaning.

**Need.** A file with another list is refused with `edrawings/model.malformed`. We must know the rule to read such a file.

### EDR-08. Exact geometry and annotations
**Question.** Does any file carry exact surfaces, curves, edges, 2D drawing content, or annotations in the model stream?

**Known.** The two samples hold display meshes only. The 2D segments are empty. The decoder rebuilds planar faces and reports `edrawings/geometry.reconstructed-from-mesh`.

**Need.** We must transfer exact geometry when a file holds it. Surface fitting on the meshes is a different work item.

### EDR-09. Sub-assembly matrices and display configurations
**Question.** Can a sub-assembly node carry a matrix, and how do the matrices of the instances below it combine with it? How do configurations other than the current display state change the tree?

**Known.** `edrawings.md` §3.8 gives the rule that the observed files follow. No sub-assembly carries a matrix. The decoder applies only the current display state, or the default state when the stream names no current state.

**Need.** We must place the instances of a file with such a matrix, and show the visibility of every state.

### EDR-10. Writer and version
**Question.** Does a stream that SolidWorks wrote with version `19.10`, or a stream that the eDrawings viewer wrote with version `18.00`, follow the rules of `19.10` or those of `18.00`?

**Known.** `edrawings.md` §3.11 and §3.12 give the rules of the files with versions `18.00` and `19.00`. SolidWorks wrote these files. The codec selects the rules from the declared version.

**Need.** We must know whether the version or the writer selects the rules. A file in the other combination can be refused with `edrawings/model.malformed` or read with wrong rules.

### EDR-11. Header length and trailer code
**Question.** Why does the closing bracket of the header block stand L + 7 bytes after the opcode in `edrawings:hsf-18-00` and L + 6 in `edrawings:hsf-19-10`? What does the code `be 2c` of the trailer mean, against `be 33`?

**Known.** `edrawings.md` §3.11 gives the end offset. The reader skips the content between the length and the bracket and does not check the trailer code.

**Need.** We must know the content of the header block to find a file where the end offset rule fails.

### EDR-12. Material extra bytes
**Question.** What do the seven extra bytes of a material with flags `01 09` hold, and which other flag values occur?

**Known.** `edrawings.md` §3.11 gives the rule: bit 3 of the second flag byte adds seven bytes. Two records show the bytes: a colour triple, a zero byte, and a second triple. The decoder takes no colour from such a record.

**Need.** We must know whether the first triple is a diffuse colour that overrides the colour of a sub-segment or of a part.

### EDR-13. Colour precedence and face colours
**Question.** Which colour wins when a part, a sub-segment, and a body each hold a material? Which faces does a sub-segment colour cover?

**Known.** `edrawings.md` §3.11 gives the three levels. The decoder uses the body material. It reports the sub-segment colours that differ as `edrawings/appearance.face-colour-reduced` and the part-level and display-state overrides as `edrawings/appearance.override-dropped`.

**Need.** We must give the faces and the occurrences their colours.

### EDR-14. Visibility inheritance
**Question.** Does a part below a sub-assembly with `vis=off` stay hidden when a state lists the part with `vis=on`? Does a sub-assembly with `vis=on` show a part with `defaultvisibility=off` that no state lists?

**Known.** `edrawings.md` §3.11 gives the rule that the decoder follows: the listing of the part decides, then `defaultvisibility=off`, then the nearest listed ancestor. The observed file has no case that separates the readings.

**Need.** We must show the visibility of every part as the source application shows it.

### EDR-15. Unexercised length forms
**Question.** What index width does a shell with 255 to 260 vertices use? Does a text object with the length byte `0xfe` occur in a SolidWorks stream?

**Known.** `edrawings.md` §3.6 gives width 8 up to 254 vertices and width 16 from 261. The reader takes the width from the face list and decodes both.

**Need.** We must know whether the face list of such a shell is read correctly.

### EDR-16. Range block meaning
**Question.** What do the fields of the range block of a shell mean: the byte `02`, the pairs `u0 v0 u1 v1`, and the K bytes?

**Known.** `edrawings.md` §3.6.1 gives the layout. The values `u0` and `u1` are 0, `v0` is less than `v1`, and the difference is the extent of the shell along one coordinate axis. K is twice the vertex count, and its bytes are `00` or `ff`. The decoder discards the block.

**Need.** We must know whether the block carries texture coordinates that a texture transfer needs. We must know whether the byte `02` is a `u8` or the low byte of a `u32`.

### EDR-17. Shells with a range block
**Question.** Which shells carry a range block and which shells carry a face tag?

**Known.** `edrawings.md` §3.6.1 gives the layout. The range block occurs on shells of textured sub-segments, and not every textured shell has it. The face tag `swfaceid` occurs on most shells of a stream and is absent from the others.

**Need.** We must know the rule to predict the optional parts, and we must know whether the face identifier links a shell to an exact face.

### EDR-18. Geometry object details
**Question.** What order do the `x`, `z`, and `y` arrays of a polyline set have, what do the two bytes `00 0c` mean, and how many items can one geometry object hold?

**Known.** `edrawings.md` §3.9.1 gives the layout. The reader accepts a run of any length of each item. The decoder skips the items.

**Need.** We must know them to transfer the 2D annotation of a file.

### EDR-19. Bodies that stay sheets
**Question.** Which sheet bodies of a stream are surface bodies of the source, and which are solids whose display mesh is incomplete?

**Known.** Every object of the stream belongs to a library body, so the decoder loses no shell. A library body is one body of the source: in a stream whose source also exists as an exchange file with a surface model, the open shells of the model and the sheet bodies correspond one to one, and the faces and the open edges have the same counts. Seen: sheet bodies that hold one face; two sheet bodies of one part whose boundary circles have the same centre and radius (the halves of a lens); flat pads of two coincident faces without thickness; bodies whose display mesh lacks one planar face (the boundary is one planar circle and no other body of the part closes it); one body whose triangulation has long fans of triangles with collinear vertices that the repair joins into more than two triangles at an edge. The decoder keeps every one of them a sheet. Display meshes of bodies that measure a few tens of micrometres have edges shorter than the weld distance of 1 micrometre; the first build welds them into degenerate triangles, and only the second build of a body that is not a solid uses a finer weld.

**Need.** We must know whether the source of a body with a missing planar face has the face. We must know whether a viewer joins the sheet bodies of one part that share their boundary. We must know a rule that keeps the geometry of micrometre-sized bodies without a separate weld per body.
