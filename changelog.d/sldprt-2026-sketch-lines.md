### Fixed

- Read the lines of a SolidWorks 2026 sketch. Its point records hold null object indices beside numbered ones, and its curve records index the list by position where their values plus one are not object indices of its point records; they were read as object indices and left unresolved or joined to the wrong points. The sketch, its extrusion and a spatial sweep path through such a sketch now resolve.
