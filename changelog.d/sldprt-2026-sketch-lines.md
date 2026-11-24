### Fixed

- Read the lines of a SolidWorks 2026 sketch. When its point records include a null object index and a line has endpoint values plus one that are not object indices of its point records, the endpoint values of all its lines are zero-based point-record positions. They were read as object indices, which left the sketch and the extrusion that names it unresolved. A spatial sweep path through such a sketch now resolves.
