### Fixed

- Retain Onshape history and readable geometry when an optional member or rollback topology table exceeds its size limit. Report each omitted part with a named loss.
- Retain readable Onshape history when the STEP export is missing or unreadable, and report missing final geometry.
- Retain Onshape history and STEP geometry when an individual member is unreadable or has a stale microversion. Preserve the first admitted feature annotation and restore design state when a thin scope fails.
