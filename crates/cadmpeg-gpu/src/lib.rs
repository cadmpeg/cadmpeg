// SPDX-License-Identifier: Apache-2.0
//! Multi-backend GPU offload subsystem and init-time autotuning dispatcher.
//!
//! Provides deterministic CPU reference primitives, single-threaded and Rayon
//! spatial acceleration indices, custom `cudarc` CUDA SIMT and `sm_86` FP64 PTX
//! kernels, and handcrafted RTX Tensor Core (`mma.sync`) FP64 kernels. At
//! initialization, [`global_dispatcher`] benchmarks every available backend
//! across exponentially spaced problem sizes and constructs a piecewise
//! size-threshold routing schedule per [`OperationKind`].

pub mod cpu;
pub mod cuda;
pub mod dispatcher;
pub mod spatial;
pub mod vulkan;

use std::sync::OnceLock;

use cadmpeg_ir::math::{Point3, Vector3};

pub use crate::dispatcher::{
    BenchmarkSample, CalibrationReport, GpuDispatcher, OperationSchedule, ThresholdInterval,
};

/// Compute backend target for geometric and linear-algebraic offload.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum BackendKind {
    /// Deterministic CPU reference implementation.
    Cpu,
    /// Single-threaded CPU spatial acceleration index (3D k-d tree and mesh BVH).
    CpuSpatialIndex,
    /// Multi-threaded Rayon CPU parallel execution and spatial acceleration index.
    CpuParallelSpatialIndex,
    /// Cross-platform Vulkan `wgpu` native `SHADER_F64` compute shaders.
    VulkanCompute,
    /// Cross-platform Vulkan `wgpu` hardware RT Core BVH acceleration (`EXPERIMENTAL_RAY_QUERY` + `SHADER_F64`).
    VulkanRtCore,
    /// Custom CUDA Driver API FP64 SIMT compute kernels.
    CudaCompute,
    /// Custom Ampere `sm_86` PTX compute kernels.
    PtxSm86,
    /// NVIDIA `OptiX` hardware RT Core BVH acceleration (`cudarc::optix` + FP64 PTX refinement).
    RtxRtCore,
    /// Handcrafted RTX Tensor Core PTX kernel (`mma.sync.aligned.m8n8k4.row.col.f64`).
    RtxTensorCore,
}

impl BackendKind {
    /// All defined backend variants in canonical probe order.
    pub const ALL: [Self; 9] = [
        Self::Cpu,
        Self::CpuSpatialIndex,
        Self::CpuParallelSpatialIndex,
        Self::VulkanCompute,
        Self::VulkanRtCore,
        Self::CudaCompute,
        Self::PtxSm86,
        Self::RtxRtCore,
        Self::RtxTensorCore,
    ];

    /// Short identifier for diagnostics and benchmark tables.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Cpu => "cpu",
            Self::CpuSpatialIndex => "cpu-spatial-index",
            Self::CpuParallelSpatialIndex => "cpu-parallel-spatial-index",
            Self::VulkanCompute => "vulkan-wgpu-f64",
            Self::VulkanRtCore => "vulkan-rt-core",
            Self::CudaCompute => "cuda-simt",
            Self::PtxSm86 => "ptx-sm86",
            Self::RtxRtCore => "rtx-rt-core",
            Self::RtxTensorCore => "rtx-tensor-core",
        }
    }
}

/// High-leverage CAD geometry operation routed by [`GpuDispatcher`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum OperationKind {
    /// Batch $4 \times 4$ homogeneous affine/projective point transformation.
    BatchTransform4x4,
    /// Batch pairwise squared Euclidean distance matrix ($N \times M$).
    PairwiseDistanceMatrix,
    /// Tensor-product bicubic rational Bézier surface patch evaluation over a $(u, v)$ grid.
    BezierSurfaceEval,
    /// Batch ray–triangle mesh closest-hit intersection.
    RayMeshIntersection,
    /// Batch point-in-solid containment via ray-casting intersection parity.
    PointInSolidParity,
    /// Batch closest-point-on-mesh projection and squared distance query.
    ClosestPointMesh,
    /// Symmetric Hausdorff distance between two 3D point sets.
    HausdorffDistance,
}

impl OperationKind {
    /// All operation kinds calibrated at initialization.
    pub const ALL: [Self; 7] = [
        Self::BatchTransform4x4,
        Self::PairwiseDistanceMatrix,
        Self::BezierSurfaceEval,
        Self::RayMeshIntersection,
        Self::PointInSolidParity,
        Self::ClosestPointMesh,
        Self::HausdorffDistance,
    ];

    /// Short identifier for diagnostics and benchmark tables.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::BatchTransform4x4 => "batch_transform_4x4",
            Self::PairwiseDistanceMatrix => "pairwise_distance_matrix",
            Self::BezierSurfaceEval => "bezier_surface_eval",
            Self::RayMeshIntersection => "ray_mesh_intersection",
            Self::PointInSolidParity => "point_in_solid_parity",
            Self::ClosestPointMesh => "closest_point_mesh",
            Self::HausdorffDistance => "hausdorff_distance",
        }
    }
}

/// 3D ray query with parametric interval $[t_{\min}, t_{\max}]$.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct RayQuery3 {
    /// Ray origin in world space.
    pub origin: Point3,
    /// Ray direction vector.
    pub direction: Vector3,
    /// Minimum valid ray parameter $t_{\min} \ge 0$.
    pub t_min: f64,
    /// Maximum valid ray parameter $t_{\max} \ge t_{\min}$.
    pub t_max: f64,
}

/// Closest-hit result of a [`RayQuery3`] against a triangle mesh.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct RayHit3 {
    /// Whether the ray intersected a triangle within $[t_{\min}, t_{\max}]$.
    pub hit: bool,
    /// Ray parameter $t$ at the closest intersection, or `f64::INFINITY` on miss.
    pub t: f64,
    /// Index of the intersected triangle, or `u32::MAX` on miss.
    pub triangle_index: u32,
    /// Barycentric coordinate $u$ on the hit triangle.
    pub barycentric_u: f64,
    /// Barycentric coordinate $v$ on the hit triangle.
    pub barycentric_v: f64,
}

impl RayHit3 {
    /// Returns a miss record.
    #[must_use]
    pub const fn miss() -> Self {
        Self {
            hit: false,
            t: f64::INFINITY,
            triangle_index: u32::MAX,
            barycentric_u: 0.0,
            barycentric_v: 0.0,
        }
    }
}

/// Borrowed view of an indexed 3D triangle mesh.
#[derive(Debug, Clone, Copy)]
pub struct TriangleMeshView<'a> {
    /// Mesh vertex positions.
    pub vertices: &'a [Point3],
    /// Triangle vertex index triples into `vertices`.
    pub triangles: &'a [[u32; 3]],
}

/// Result of projecting a 3D query point onto the closest point of a triangle mesh.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ClosestPointResult {
    /// Closest point on the triangle mesh.
    pub closest_point: Point3,
    /// Squared Euclidean distance from the query point to `closest_point`.
    pub distance_sq: f64,
    /// Index of the triangle containing `closest_point`, or `u32::MAX` when the mesh is empty.
    pub triangle_index: u32,
}

static GLOBAL_DISPATCHER: OnceLock<GpuDispatcher> = OnceLock::new();

/// Returns the process-wide autotuned [`GpuDispatcher`], running the scaling
/// microbenchmark across all available hardware backends on first access.
#[must_use]
pub fn global_dispatcher() -> &'static GpuDispatcher {
    GLOBAL_DISPATCHER.get_or_init(GpuDispatcher::calibrate_default)
}

/// Transforms a batch of homogeneous 4D points by a $4 \times 4$ matrix using the optimal backend.
#[must_use]
pub fn transform_points_4x4(matrix: &[[f64; 4]; 4], points: &[[f64; 4]]) -> Vec<[f64; 4]> {
    global_dispatcher().batch_transform_4x4(matrix, points)
}

/// Computes the $N \times M$ pairwise squared Euclidean distance matrix using the optimal backend.
#[must_use]
pub fn pairwise_squared_distances(points_a: &[Point3], points_b: &[Point3]) -> Vec<f64> {
    global_dispatcher().pairwise_distance_squared(points_a, points_b)
}

/// Computes the symmetric Hausdorff distance between two non-empty 3D point sets.
#[must_use]
pub fn hausdorff_distance(points_a: &[Point3], points_b: &[Point3]) -> f64 {
    global_dispatcher().hausdorff_distance(points_a, points_b)
}

/// Evaluates a bicubic rational Bézier surface patch over a tensor-product $(u, v)$ grid.
#[must_use]
pub fn eval_bicubic_bezier_grid(
    control_points: &[[[f64; 4]; 4]; 4],
    u_params: &[f64],
    v_params: &[f64],
) -> Vec<Point3> {
    global_dispatcher().eval_bicubic_bezier_grid(control_points, u_params, v_params)
}

/// Intersects a batch of rays against an indexed triangle mesh using the optimal backend.
#[must_use]
pub fn intersect_rays_mesh(rays: &[RayQuery3], mesh: TriangleMeshView<'_>) -> Vec<RayHit3> {
    global_dispatcher().ray_mesh_intersect(rays, mesh)
}

/// Determines inside/outside solid containment for a batch of 3D query points against a closed mesh.
#[must_use]
pub fn points_in_solid_parity(points: &[Point3], mesh: TriangleMeshView<'_>) -> Vec<bool> {
    global_dispatcher().point_in_solid_parity(points, mesh)
}

/// Finds the closest point and squared distance on a triangle mesh for each query point.
#[must_use]
pub fn closest_points_on_mesh(
    points: &[Point3],
    mesh: TriangleMeshView<'_>,
) -> Vec<ClosestPointResult> {
    global_dispatcher().closest_point_mesh(points, mesh)
}

#[cfg(test)]
mod integration_tests;
