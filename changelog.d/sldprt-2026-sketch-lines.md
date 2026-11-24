### Fixed

- Read the lines of a SolidWorks 2026 sketch. A 2026 sketch lists point records that carry no object index, so its curve records index the list by position; they were read as object indices and left unresolved or joined to the wrong points.
