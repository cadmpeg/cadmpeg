# Synthetic STEP fixture manifest

All fixtures in this directory are original minimal exchange structures
authored against `docs/formats/step.md`.

| File | Construct |
| --- | --- |
| `ap242_minimal.p21` | AP242 header, one DATA section, forward reference, multiline record, numeric forms, enum, omitted and derived values |
| `ap242_ed3_sections.p21` | edition-3 ANCHOR, REFERENCE, multiple DATA, and SIGNATURE sections |
| `complex_instance.p21` | external-mapped complex instance with sibling-supplied attributes |
| `strings.p21` | apostrophe, reverse-solidus, X, X2, X4, S, and page-selection string escapes |
| `ap242_geometry.p21` | millimetre unit context and placed point, line, and circle carriers |
| `ap214_sheet.p21` | connected triangular sheet B-rep with oriented edge uses and a planar face |
| `ap242_assembly.p21` | product-definition tree, NAUO identity, and item-defined occurrence placement |
