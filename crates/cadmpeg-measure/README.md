# cadmpeg-measure

Geometric measurement of decoded cadmpeg models: carrier parameters, trimmed
areas, boundary lengths, boxes and orientation readings of bodies, faces and
edges, in model space.

Every value is measured within its reported error under its stated method,
or unavailable with a pinned reason and a closed category. The output
contract is in `docs/measurement.md` of the repository.

Measurement is an oracle on decoded output. No decoder or encoder depends on
this crate outside its tests.
