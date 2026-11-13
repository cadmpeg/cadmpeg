# cadmpeg-gpu

Multi-backend GPU offload and autotuning dispatcher for `cadmpeg` geometric evaluation, tessellation, and measurement primitives.

## Backends

- `Cpu`: Deterministic serial reference implementation.
- `CpuSpatialIndex`: Single-threaded 3D k-d tree and mesh AABB BVH spatial index.
- `CpuParallelSpatialIndex`: Multi-threaded Rayon execution across dense linear algebra, surface evaluation, and 3D k-d tree / mesh AABB BVH spatial queries.
- `VulkanCompute`: Native IEEE-754 `f64` WGSL compute shaders executed via `wgpu` (`Features::SHADER_F64`) across all 7 geometric operations.
- `VulkanRtCore`: Hardware RT Core acceleration via Vulkan (`wgpu::Features::EXPERIMENTAL_RAY_QUERY` + `wgpu::Features::SHADER_F64` with `enable wgpu_ray_query;`) combining hardware BLAS/TLAS traversal with exact IEEE-754 `f64` geometry verification inside unified WGSL compute shaders across all 4 spatial operations.
- `CudaCompute`: Custom FP64 SIMT PTX 7.1 kernels (`sm_50`) executed via `cudarc` across all 7 geometric operations.
- `PtxSm86`: Ampere `sm_86` PTX 7.1 kernels with shared-memory tiling and L2 residency hints across all 7 geometric operations.
- `RtxRtCore`: NVIDIA OptiX hardware RT Core acceleration (`cudarc::optix`) combining hardware BVH GAS (`OptixAccelBuild` + `optixTrace`) traversal with exact IEEE-754 `f64` PTX refinement across all 4 spatial operations.
- `RtxTensorCore`: Handcrafted PTX 7.1 `sm_80` Tensor Core kernels using `mma.sync.aligned.m8n8k4.row.col.f64.f64.f64.f64` on data-center GPUs (`sm_80`, `sm_90`, `sm_100`) for 4x4 homogeneous transforms and two-stage tensor-product rational Bézier surface patch evaluation.

## Kernel Coverage Matrix (33 Kernels Across 7 Operations × 9 Backends)

| Operation | `Cpu` | `CpuSpatialIndex` | `CpuParallelSpatialIndex` | `VulkanCompute` (`wgpu` `f64`) | `VulkanRtCore` (`wgpu_ray_query` + `f64`) | `CudaCompute` (`PTX sm_50`) | `PtxSm86` (`PTX sm_86`) | `RtxRtCore` (`cudarc::optix` + `f64`) | `RtxTensorCore` (`mma.m8n8k4.f64`) |
|:---|:---:|:---:|:---:|:---:|:---:|:---:|:---:|:---:|:---:|
| `BatchTransform4x4` | ✓ | — | ✓ | ✓ | — | ✓ | ✓ | — | ✓ |
| `PairwiseDistanceMatrix` | ✓ | — | ✓ | ✓ | — | ✓ | ✓ | — | — |
| `BezierSurfaceEval` | ✓ | — | ✓ | ✓ | — | ✓ | ✓ | — | ✓ |
| `RayMeshIntersection` | ✓ | ✓ | ✓ | ✓ | ✓ | ✓ | ✓ | ✓ | — |
| `PointInSolidParity` | ✓ | ✓ | ✓ | ✓ | ✓ | ✓ | ✓ | ✓ | — |
| `ClosestPointMesh` | ✓ | ✓ | ✓ | ✓ | ✓ | ✓ | ✓ | ✓ | — |
| `HausdorffDistance` | ✓ | ✓ | ✓ | ✓ | ✓ | ✓ | ✓ | ✓ | — |
