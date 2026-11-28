# IronCAD reader

Detects IronCAD `.ics` scene files, lists their streams, inflates the framed
streams, and enumerates the ACIS and Parasolid kernel records of `Brep Data`.
ACIS and Parasolid part bodies decode into B-rep topology and geometry, and
each part is placed in the scene as a product occurrence.

<!-- generated: capability ironcad -->

Support: L3 ([ladder](https://github.com/cadmpeg/cadmpeg/blob/main/docs/format-support.md#ironcad-ics)).

<!-- /generated: capability ironcad -->
