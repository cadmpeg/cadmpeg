# cadmpeg-tessellate

Display tessellation of the B-rep faces of a decoded cadmpeg model.

Each face's loops become polylines in the parameter plane of its surface:
from the coedge pcurves where they lie on their edges, otherwise from samples
of the edge curves inverted onto the surface. Faces that share an edge share
its samples, so adjacent face meshes meet at the same points. The trimmed
domain is triangulated and refined until the chordal deviation and
normal-angle limits hold.

A face mesh also carries the directed parameter-space trace of each coedge,
which a writer can store as a derived pcurve, and the largest distance
between a boundary sample and the surface.

```rust
use cadmpeg_tessellate::{Options, Tessellator};

let cube = cadmpeg_ir::examples::unit_cube().expect("fixture");
let mut tessellator = Tessellator::new(&cube, Options::default());
let face = &cube.model.faces[0].id;
let mesh = tessellator.face(face).expect("planar face");
assert_eq!(mesh.triangles.len(), 2);
```
