// SPDX-License-Identifier: Apache-2.0
//! Init-time scaling microbenchmark and piecewise size-threshold autotuning dispatcher.

use std::time::Instant;

use cadmpeg_ir::math::{Point3, Vector3};

use crate::cuda::global_cuda;
use crate::vulkan::global_vulkan;
use crate::{
    cpu, spatial, BackendKind, ClosestPointResult, OperationKind, RayHit3, RayQuery3,
    TriangleMeshView,
};

/// Exponentially spaced calibration problem sizes measured at dispatcher initialization.
pub const CALIBRATION_SIZES: [usize; 5] = [16, 64, 256, 1024, 4096];

/// Latency measurement for a single `(OperationKind, BackendKind, problem_size)` calibration point.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct BenchmarkSample {
    /// Operation measured.
    pub operation: OperationKind,
    /// Backend measured.
    pub backend: BackendKind,
    /// Problem size $N$ (number of points, rays, or surface evaluation samples).
    pub problem_size: usize,
    /// Measured execution latency in nanoseconds.
    pub latency_ns: u64,
}

/// Half-open problem-size interval `[min_size, max_size)` routed to a specific [`BackendKind`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ThresholdInterval {
    /// Inclusive lower bound on problem size $N$.
    pub min_size: usize,
    /// Exclusive upper bound on problem size $N$ (`usize::MAX` for the unbounded tail).
    pub max_size: usize,
    /// Fastest backend selected for problem sizes in `[min_size, max_size)`.
    pub backend: BackendKind,
}

/// Piecewise size-threshold routing schedule for one [`OperationKind`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OperationSchedule {
    /// Operation governed by this schedule.
    pub operation: OperationKind,
    /// Contiguous, non-overlapping size intervals covering `0..usize::MAX`.
    pub intervals: Vec<ThresholdInterval>,
}

impl OperationSchedule {
    /// Selects the lowest-latency backend for the given `problem_size`.
    #[must_use]
    pub fn select_backend(&self, problem_size: usize) -> BackendKind {
        for interval in &self.intervals {
            if problem_size >= interval.min_size && problem_size < interval.max_size {
                return interval.backend;
            }
        }
        self.intervals
            .last()
            .map_or(BackendKind::Cpu, |last| last.backend)
    }
}

/// Full telemetry and routing report produced during [`GpuDispatcher`] calibration.
#[derive(Debug, Clone)]
pub struct CalibrationReport {
    /// Hardware backends available at runtime on this machine.
    pub available_backends: Vec<BackendKind>,
    /// Name of the active Vulkan `wgpu` adapter, if present.
    pub vulkan_device_name: Option<String>,
    /// Name of the active CUDA GPU, if present.
    pub cuda_device_name: Option<String>,
    /// All measured `(operation, backend, problem_size)` latency samples.
    pub samples: Vec<BenchmarkSample>,
    /// Piecewise size-threshold routing schedules per [`OperationKind`].
    pub schedules: [OperationSchedule; 7],
}

/// Autotuned multi-backend dispatcher that routes geometric and linear-algebraic calls
/// based on problem size $N$ and init-time microbenchmark latency profiles.
#[derive(Debug, Clone)]
pub struct GpuDispatcher {
    report: CalibrationReport,
}

impl GpuDispatcher {
    /// Probes all runtime-available hardware backends and runs the scaling microbenchmark
    /// across [`CALIBRATION_SIZES`].
    #[must_use]
    pub fn calibrate_default() -> Self {
        Self::calibrate_with_sizes(&CALIBRATION_SIZES)
    }

    /// Runs the scaling microbenchmark across a custom slice of ascending problem sizes.
    #[must_use]
    pub fn calibrate_with_sizes(sizes: &[usize]) -> Self {
        let mut available = vec![
            BackendKind::Cpu,
            BackendKind::CpuSpatialIndex,
            BackendKind::CpuParallelSpatialIndex,
        ];
        let mut vulkan_device_name = None;
        let mut cuda_device_name = None;

        if let Some(vk) = global_vulkan() {
            vulkan_device_name = Some(vk.device_name().to_owned());
            available.push(BackendKind::VulkanCompute);
            if vk.supports_ray_query() {
                available.push(BackendKind::VulkanRtCore);
            }
        }

        if let Some(cu) = global_cuda() {
            cuda_device_name = Some(cu.device_name().to_owned());
            available.push(BackendKind::CudaCompute);
            if cu.supports_sm86() {
                available.push(BackendKind::PtxSm86);
            }
            if cu.supports_optix_rt() {
                available.push(BackendKind::RtxRtCore);
            }
            if cu.supports_tensor_cores() {
                available.push(BackendKind::RtxTensorCore);
            }
        }

        let mut samples = Vec::new();
        let schedules = OperationKind::ALL.map(|op| {
            let candidates = candidate_backends_for_op(op, &available);
            let mut size_winners: Vec<(usize, BackendKind)> = Vec::with_capacity(sizes.len());
            for &n in sizes {
                let n = n.max(1);
                let mut best_backend = BackendKind::Cpu;
                let mut best_ns = u64::MAX;
                for &backend in &candidates {
                    if let Some(ns) = benchmark_single(op, backend, n) {
                        samples.push(BenchmarkSample {
                            operation: op,
                            backend,
                            problem_size: n,
                            latency_ns: ns,
                        });
                        if ns < best_ns {
                            best_ns = ns;
                            best_backend = backend;
                        }
                    }
                }
                size_winners.push((n, best_backend));
            }
            build_piecewise_schedule(op, &size_winners)
        });

        Self {
            report: CalibrationReport {
                available_backends: available,
                vulkan_device_name,
                cuda_device_name,
                samples,
                schedules,
            },
        }
    }

    /// Returns the calibration report and piecewise routing schedules.
    #[must_use]
    pub const fn report(&self) -> &CalibrationReport {
        &self.report
    }

    /// Returns the piecewise routing schedule for `operation`.
    #[must_use]
    pub const fn schedule(&self, operation: OperationKind) -> &OperationSchedule {
        match operation {
            OperationKind::BatchTransform4x4 => &self.report.schedules[0],
            OperationKind::PairwiseDistanceMatrix => &self.report.schedules[1],
            OperationKind::BezierSurfaceEval => &self.report.schedules[2],
            OperationKind::RayMeshIntersection => &self.report.schedules[3],
            OperationKind::PointInSolidParity => &self.report.schedules[4],
            OperationKind::ClosestPointMesh => &self.report.schedules[5],
            OperationKind::HausdorffDistance => &self.report.schedules[6],
        }
    }

    /// Selects the lowest-latency backend for `operation` at `problem_size`.
    #[must_use]
    pub fn select_backend(&self, operation: OperationKind, problem_size: usize) -> BackendKind {
        self.schedule(operation).select_backend(problem_size)
    }

    /// Executes a batch $4 \times 4$ homogeneous transform on a specific backend.
    ///
    /// Returns `None` when the backend does not implement the operation or hardware is unavailable.
    #[must_use]
    pub fn execute_transform_on(
        &self,
        backend: BackendKind,
        matrix: &[[f64; 4]; 4],
        points: &[[f64; 4]],
    ) -> Option<Vec<[f64; 4]>> {
        match backend {
            BackendKind::RtxTensorCore => {
                global_cuda().and_then(|cu| cu.tensor_core_batch_transform_4x4(matrix, points))
            }
            BackendKind::PtxSm86 => {
                global_cuda().and_then(|cu| cu.ptx_sm86_batch_transform_4x4(matrix, points))
            }
            BackendKind::CudaCompute => {
                global_cuda().and_then(|cu| cu.simt_batch_transform_4x4(matrix, points))
            }
            BackendKind::VulkanCompute => {
                global_vulkan().and_then(|vk| vk.compute_batch_transform_4x4(matrix, points))
            }
            BackendKind::CpuParallelSpatialIndex => {
                Some(cpu::rayon_batch_transform_4x4(matrix, points))
            }
            BackendKind::Cpu => Some(cpu::batch_transform_4x4(matrix, points)),
            BackendKind::CpuSpatialIndex | BackendKind::VulkanRtCore | BackendKind::RtxRtCore => {
                None
            }
        }
    }

    /// Executes an $N \times M$ pairwise squared distance matrix on a specific backend.
    ///
    /// Returns `None` when the backend does not implement the operation or hardware is unavailable.
    #[must_use]
    pub fn execute_pairwise_distance_on(
        &self,
        backend: BackendKind,
        points_a: &[Point3],
        points_b: &[Point3],
    ) -> Option<Vec<f64>> {
        match backend {
            BackendKind::PtxSm86 => global_cuda()
                .and_then(|cu| cu.ptx_sm86_pairwise_distance_squared(points_a, points_b)),
            BackendKind::CudaCompute => {
                global_cuda().and_then(|cu| cu.simt_pairwise_distance_squared(points_a, points_b))
            }
            BackendKind::VulkanCompute => global_vulkan()
                .and_then(|vk| vk.compute_pairwise_distance_squared(points_a, points_b)),
            BackendKind::CpuParallelSpatialIndex => {
                Some(cpu::rayon_pairwise_distance_squared(points_a, points_b))
            }
            BackendKind::Cpu => Some(cpu::pairwise_distance_squared(points_a, points_b)),
            BackendKind::RtxTensorCore
            | BackendKind::VulkanRtCore
            | BackendKind::RtxRtCore
            | BackendKind::CpuSpatialIndex => None,
        }
    }

    /// Executes bicubic rational Bézier surface grid evaluation on a specific backend.
    ///
    /// Returns `None` when the backend does not implement the operation or hardware is unavailable.
    #[must_use]
    pub fn execute_bezier_eval_on(
        &self,
        backend: BackendKind,
        control_points: &[[[f64; 4]; 4]; 4],
        u_params: &[f64],
        v_params: &[f64],
    ) -> Option<Vec<Point3>> {
        match backend {
            BackendKind::RtxTensorCore => global_cuda().and_then(|cu| {
                cu.tensor_core_eval_bicubic_bezier_grid(control_points, u_params, v_params)
            }),
            BackendKind::PtxSm86 => global_cuda().and_then(|cu| {
                cu.ptx_sm86_eval_bicubic_bezier_grid(control_points, u_params, v_params)
            }),
            BackendKind::CudaCompute => global_cuda().and_then(|cu| {
                cu.simt_eval_bicubic_bezier_grid(control_points, u_params, v_params)
            }),
            BackendKind::VulkanCompute => global_vulkan().and_then(|vk| {
                vk.compute_eval_bicubic_bezier_grid(control_points, u_params, v_params)
            }),
            BackendKind::CpuParallelSpatialIndex => Some(cpu::rayon_eval_bicubic_bezier_grid(
                control_points,
                u_params,
                v_params,
            )),
            BackendKind::Cpu => Some(cpu::eval_bicubic_bezier_grid(
                control_points,
                u_params,
                v_params,
            )),
            BackendKind::CpuSpatialIndex | BackendKind::VulkanRtCore | BackendKind::RtxRtCore => {
                None
            }
        }
    }

    /// Executes batch ray-mesh intersection on a specific backend.
    ///
    /// Returns `None` when the backend does not implement the operation or hardware is unavailable.
    #[must_use]
    pub fn execute_ray_mesh_on(
        &self,
        backend: BackendKind,
        rays: &[RayQuery3],
        mesh: TriangleMeshView<'_>,
    ) -> Option<Vec<RayHit3>> {
        match backend {
            BackendKind::RtxRtCore => {
                global_cuda().and_then(|cu| cu.optix_rt_ray_mesh_intersect(rays, mesh))
            }
            BackendKind::VulkanRtCore => {
                global_vulkan().and_then(|vk| vk.rt_core_ray_mesh_intersect(rays, mesh))
            }
            BackendKind::CpuParallelSpatialIndex => {
                Some(spatial::rayon_intersect_rays_mesh(rays, mesh))
            }
            BackendKind::CpuSpatialIndex => Some(spatial::intersect_rays_mesh(rays, mesh)),
            BackendKind::PtxSm86 => {
                global_cuda().and_then(|cu| cu.ptx_sm86_ray_mesh_intersect(rays, mesh))
            }
            BackendKind::CudaCompute => {
                global_cuda().and_then(|cu| cu.simt_ray_mesh_intersect(rays, mesh))
            }
            BackendKind::VulkanCompute => {
                global_vulkan().and_then(|vk| vk.compute_ray_mesh_intersect(rays, mesh))
            }
            BackendKind::Cpu => Some(cpu::ray_mesh_intersect(rays, mesh)),
            BackendKind::RtxTensorCore => None,
        }
    }

    /// Executes batch point-in-solid parity containment on a specific backend.
    ///
    /// Returns `None` when the backend does not implement the operation or hardware is unavailable.
    #[must_use]
    pub fn execute_point_in_solid_on(
        &self,
        backend: BackendKind,
        points: &[Point3],
        mesh: TriangleMeshView<'_>,
    ) -> Option<Vec<bool>> {
        match backend {
            BackendKind::RtxRtCore => {
                global_cuda().and_then(|cu| cu.optix_rt_point_in_solid_parity(points, mesh))
            }
            BackendKind::VulkanRtCore => {
                global_vulkan().and_then(|vk| vk.rt_core_point_in_solid_parity(points, mesh))
            }
            BackendKind::CpuParallelSpatialIndex => {
                Some(spatial::rayon_points_in_solid_parity(points, mesh))
            }
            BackendKind::CpuSpatialIndex => Some(spatial::points_in_solid_parity(points, mesh)),
            BackendKind::PtxSm86 => {
                global_cuda().and_then(|cu| cu.ptx_sm86_point_in_solid_parity(points, mesh))
            }
            BackendKind::CudaCompute => {
                global_cuda().and_then(|cu| cu.simt_point_in_solid_parity(points, mesh))
            }
            BackendKind::VulkanCompute => {
                global_vulkan().and_then(|vk| vk.compute_point_in_solid_parity(points, mesh))
            }
            BackendKind::Cpu => Some(cpu::point_in_solid_parity(points, mesh)),
            BackendKind::RtxTensorCore => None,
        }
    }

    /// Executes batch closest-point-on-mesh queries on a specific backend.
    ///
    /// Returns `None` when the backend does not implement the operation or hardware is unavailable.
    #[must_use]
    pub fn execute_closest_point_on(
        &self,
        backend: BackendKind,
        points: &[Point3],
        mesh: TriangleMeshView<'_>,
    ) -> Option<Vec<ClosestPointResult>> {
        match backend {
            BackendKind::RtxRtCore => {
                global_cuda().and_then(|cu| cu.optix_rt_closest_point_mesh(points, mesh))
            }
            BackendKind::VulkanRtCore => {
                global_vulkan().and_then(|vk| vk.rt_core_closest_point_mesh(points, mesh))
            }
            BackendKind::CpuParallelSpatialIndex => {
                Some(spatial::rayon_closest_points_on_mesh(points, mesh))
            }
            BackendKind::CpuSpatialIndex => Some(spatial::closest_points_on_mesh(points, mesh)),
            BackendKind::PtxSm86 => {
                global_cuda().and_then(|cu| cu.ptx_sm86_spatial_closest_point(points, mesh))
            }
            BackendKind::CudaCompute => {
                global_cuda().and_then(|cu| cu.simt_spatial_closest_point(points, mesh))
            }
            BackendKind::VulkanCompute => {
                global_vulkan().and_then(|vk| vk.compute_spatial_closest_point(points, mesh))
            }
            BackendKind::Cpu => Some(cpu::closest_point_mesh(points, mesh)),
            BackendKind::RtxTensorCore => None,
        }
    }

    /// Executes symmetric Hausdorff distance evaluation on a specific backend.
    ///
    /// Returns `None` when the backend does not implement the operation or hardware is unavailable.
    #[must_use]
    pub fn execute_hausdorff_on(
        &self,
        backend: BackendKind,
        points_a: &[Point3],
        points_b: &[Point3],
    ) -> Option<f64> {
        match backend {
            BackendKind::RtxRtCore => {
                global_cuda().and_then(|cu| cu.optix_rt_spatial_hausdorff(points_a, points_b))
            }
            BackendKind::VulkanRtCore => {
                global_vulkan().and_then(|vk| vk.rt_core_spatial_hausdorff(points_a, points_b))
            }
            BackendKind::CpuParallelSpatialIndex => {
                Some(spatial::rayon_hausdorff_distance(points_a, points_b))
            }
            BackendKind::CpuSpatialIndex => {
                Some(spatial::kd_hausdorff_distance(points_a, points_b))
            }
            BackendKind::PtxSm86 => {
                global_cuda().and_then(|cu| cu.ptx_sm86_spatial_hausdorff(points_a, points_b))
            }
            BackendKind::CudaCompute => {
                global_cuda().and_then(|cu| cu.simt_spatial_hausdorff(points_a, points_b))
            }
            BackendKind::VulkanCompute => {
                global_vulkan().and_then(|vk| vk.compute_spatial_hausdorff(points_a, points_b))
            }
            BackendKind::Cpu => {
                let dist_sq = cpu::pairwise_distance_squared(points_a, points_b);
                Some(cpu::hausdorff_from_distance_squared_matrix(
                    &dist_sq,
                    points_a.len(),
                    points_b.len(),
                ))
            }
            BackendKind::RtxTensorCore => None,
        }
    }

    /// Dispatches batch $4 \times 4$ homogeneous point transformation to the optimal backend for `points.len()`.
    #[must_use]
    pub fn batch_transform_4x4(
        &self,
        matrix: &[[f64; 4]; 4],
        points: &[[f64; 4]],
    ) -> Vec<[f64; 4]> {
        let backend = self.select_backend(OperationKind::BatchTransform4x4, points.len());
        self.execute_transform_on(backend, matrix, points)
            .unwrap_or_else(|| cpu::batch_transform_4x4(matrix, points))
    }

    /// Dispatches $N \times M$ pairwise squared Euclidean distance matrix computation to the optimal backend.
    #[must_use]
    pub fn pairwise_distance_squared(&self, points_a: &[Point3], points_b: &[Point3]) -> Vec<f64> {
        let work_size = points_a.len().max(points_b.len());
        let backend = self.select_backend(OperationKind::PairwiseDistanceMatrix, work_size);
        self.execute_pairwise_distance_on(backend, points_a, points_b)
            .unwrap_or_else(|| cpu::pairwise_distance_squared(points_a, points_b))
    }

    /// Computes the symmetric Hausdorff distance between two point sets using the optimal backend.
    #[must_use]
    pub fn hausdorff_distance(&self, points_a: &[Point3], points_b: &[Point3]) -> f64 {
        let work_size = points_a.len().max(points_b.len());
        let backend = self.select_backend(OperationKind::HausdorffDistance, work_size);
        self.execute_hausdorff_on(backend, points_a, points_b)
            .unwrap_or_else(|| spatial::kd_hausdorff_distance(points_a, points_b))
    }

    /// Dispatches bicubic rational Bézier surface patch grid evaluation to the optimal backend.
    #[must_use]
    pub fn eval_bicubic_bezier_grid(
        &self,
        control_points: &[[[f64; 4]; 4]; 4],
        u_params: &[f64],
        v_params: &[f64],
    ) -> Vec<Point3> {
        let total = u_params.len().checked_mul(v_params.len()).unwrap_or(0);
        let backend = self.select_backend(OperationKind::BezierSurfaceEval, total);
        self.execute_bezier_eval_on(backend, control_points, u_params, v_params)
            .unwrap_or_else(|| cpu::eval_bicubic_bezier_grid(control_points, u_params, v_params))
    }

    /// Dispatches batch ray-mesh intersection to the optimal backend.
    #[must_use]
    pub fn ray_mesh_intersect(
        &self,
        rays: &[RayQuery3],
        mesh: TriangleMeshView<'_>,
    ) -> Vec<RayHit3> {
        let backend = self.select_backend(OperationKind::RayMeshIntersection, rays.len());
        self.execute_ray_mesh_on(backend, rays, mesh)
            .unwrap_or_else(|| spatial::intersect_rays_mesh(rays, mesh))
    }

    /// Dispatches batch point-in-solid parity containment to the optimal backend.
    #[must_use]
    pub fn point_in_solid_parity(
        &self,
        points: &[Point3],
        mesh: TriangleMeshView<'_>,
    ) -> Vec<bool> {
        let backend = self.select_backend(OperationKind::PointInSolidParity, points.len());
        self.execute_point_in_solid_on(backend, points, mesh)
            .unwrap_or_else(|| spatial::points_in_solid_parity(points, mesh))
    }

    /// Dispatches batch closest-point-on-mesh queries to the optimal backend.
    #[must_use]
    pub fn closest_point_mesh(
        &self,
        points: &[Point3],
        mesh: TriangleMeshView<'_>,
    ) -> Vec<ClosestPointResult> {
        let backend = self.select_backend(OperationKind::ClosestPointMesh, points.len());
        self.execute_closest_point_on(backend, points, mesh)
            .unwrap_or_else(|| spatial::closest_points_on_mesh(points, mesh))
    }
}

fn candidate_backends_for_op(op: OperationKind, available: &[BackendKind]) -> Vec<BackendKind> {
    let mut out = Vec::new();
    for &b in available {
        let eligible = match op {
            OperationKind::BatchTransform4x4 | OperationKind::BezierSurfaceEval => matches!(
                b,
                BackendKind::Cpu
                    | BackendKind::CpuParallelSpatialIndex
                    | BackendKind::VulkanCompute
                    | BackendKind::CudaCompute
                    | BackendKind::PtxSm86
                    | BackendKind::RtxTensorCore
            ),
            OperationKind::PairwiseDistanceMatrix => matches!(
                b,
                BackendKind::Cpu
                    | BackendKind::CpuParallelSpatialIndex
                    | BackendKind::VulkanCompute
                    | BackendKind::CudaCompute
                    | BackendKind::PtxSm86
            ),
            OperationKind::RayMeshIntersection
            | OperationKind::PointInSolidParity
            | OperationKind::ClosestPointMesh
            | OperationKind::HausdorffDistance => matches!(
                b,
                BackendKind::Cpu
                    | BackendKind::CpuSpatialIndex
                    | BackendKind::CpuParallelSpatialIndex
                    | BackendKind::VulkanCompute
                    | BackendKind::VulkanRtCore
                    | BackendKind::CudaCompute
                    | BackendKind::PtxSm86
                    | BackendKind::RtxRtCore
            ),
        };
        if eligible {
            out.push(b);
        }
    }
    if out.is_empty() {
        out.push(BackendKind::Cpu);
    }
    out
}

/// Converts a sorted list of `(problem_size, winning_backend)` measurements into a merged
/// piecewise interval table covering `0..usize::MAX`.
#[must_use]
pub fn build_piecewise_schedule(
    operation: OperationKind,
    size_winners: &[(usize, BackendKind)],
) -> OperationSchedule {
    if size_winners.is_empty() {
        return OperationSchedule {
            operation,
            intervals: vec![ThresholdInterval {
                min_size: 0,
                max_size: usize::MAX,
                backend: BackendKind::Cpu,
            }],
        };
    }
    let mut intervals: Vec<ThresholdInterval> = Vec::new();
    let mut lower = 0_usize;
    for idx in 0..size_winners.len() {
        let (size_i, winner_i) = size_winners[idx];
        let upper = if idx + 1 < size_winners.len() {
            let next_size = size_winners[idx + 1].0;
            size_i.midpoint(next_size).max(lower + 1)
        } else {
            usize::MAX
        };
        if let Some(prev) = intervals.last_mut() {
            if prev.backend == winner_i {
                prev.max_size = upper;
                lower = upper;
                continue;
            }
        }
        intervals.push(ThresholdInterval {
            min_size: lower,
            max_size: upper,
            backend: winner_i,
        });
        lower = upper;
    }
    OperationSchedule {
        operation,
        intervals,
    }
}

fn benchmark_single(
    operation: OperationKind,
    backend: BackendKind,
    problem_size: usize,
) -> Option<u64> {
    match operation {
        OperationKind::BatchTransform4x4 => {
            let matrix = [
                [1.0, 0.25, -0.125, 2.0],
                [-0.25, 1.5, 0.5, -1.0],
                [0.125, -0.5, 1.25, 0.5],
                [0.0, 0.0, 0.0, 1.0],
            ];
            let pts: Vec<[f64; 4]> = (0..problem_size)
                .map(|i| {
                    let f = f64::from(u32::try_from(i).unwrap_or(0));
                    [f * 0.01, f * -0.02, f * 0.03, 1.0]
                })
                .collect();
            let t0 = Instant::now();
            match backend {
                BackendKind::Cpu => {
                    let out = cpu::batch_transform_4x4(&matrix, &pts);
                    std::hint::black_box(&out);
                }
                BackendKind::CpuParallelSpatialIndex => {
                    let out = cpu::rayon_batch_transform_4x4(&matrix, &pts);
                    std::hint::black_box(&out);
                }
                BackendKind::VulkanCompute => {
                    let vk = global_vulkan()?;
                    let out = vk.compute_batch_transform_4x4(&matrix, &pts)?;
                    std::hint::black_box(&out);
                }
                BackendKind::CudaCompute => {
                    let cu = global_cuda()?;
                    let out = cu.simt_batch_transform_4x4(&matrix, &pts)?;
                    std::hint::black_box(&out);
                }
                BackendKind::PtxSm86 => {
                    let cu = global_cuda()?;
                    let out = cu.ptx_sm86_batch_transform_4x4(&matrix, &pts)?;
                    std::hint::black_box(&out);
                }
                BackendKind::RtxTensorCore => {
                    let cu = global_cuda()?;
                    let out = cu.tensor_core_batch_transform_4x4(&matrix, &pts)?;
                    std::hint::black_box(&out);
                }
                BackendKind::CpuSpatialIndex
                | BackendKind::VulkanRtCore
                | BackendKind::RtxRtCore => return None,
            }
            u64::try_from(t0.elapsed().as_nanos()).ok()
        }
        OperationKind::PairwiseDistanceMatrix => {
            let side = isqrt_ceil(problem_size).clamp(4, 128);
            let pts_a: Vec<Point3> = (0..side)
                .map(|i| {
                    let f = f64::from(u32::try_from(i).unwrap_or(0));
                    Point3::new(f * 0.1, f * 0.2, f * -0.1)
                })
                .collect();
            let pts_b: Vec<Point3> = (0..side)
                .map(|j| {
                    let f = f64::from(u32::try_from(j).unwrap_or(0));
                    Point3::new(f * -0.15 + 1.0, f * 0.05, f * 0.25)
                })
                .collect();
            let t0 = Instant::now();
            match backend {
                BackendKind::Cpu => {
                    let out = cpu::pairwise_distance_squared(&pts_a, &pts_b);
                    std::hint::black_box(&out);
                }
                BackendKind::CpuParallelSpatialIndex => {
                    let out = cpu::rayon_pairwise_distance_squared(&pts_a, &pts_b);
                    std::hint::black_box(&out);
                }
                BackendKind::VulkanCompute => {
                    let vk = global_vulkan()?;
                    let out = vk.compute_pairwise_distance_squared(&pts_a, &pts_b)?;
                    std::hint::black_box(&out);
                }
                BackendKind::CudaCompute => {
                    let cu = global_cuda()?;
                    let out = cu.simt_pairwise_distance_squared(&pts_a, &pts_b)?;
                    std::hint::black_box(&out);
                }
                BackendKind::PtxSm86 => {
                    let cu = global_cuda()?;
                    let out = cu.ptx_sm86_pairwise_distance_squared(&pts_a, &pts_b)?;
                    std::hint::black_box(&out);
                }
                BackendKind::RtxTensorCore
                | BackendKind::VulkanRtCore
                | BackendKind::RtxRtCore
                | BackendKind::CpuSpatialIndex => return None,
            }
            u64::try_from(t0.elapsed().as_nanos()).ok()
        }
        OperationKind::BezierSurfaceEval => {
            let side = isqrt_ceil(problem_size).clamp(4, 128);
            let ctrl = sample_bezier_control_net();
            let u_params: Vec<f64> = (0..side)
                .map(|i| {
                    f64::from(u32::try_from(i).unwrap_or(0))
                        / f64::from(u32::try_from(side.max(2) - 1).unwrap_or(1))
                })
                .collect();
            let t0 = Instant::now();
            match backend {
                BackendKind::Cpu => {
                    let out = cpu::eval_bicubic_bezier_grid(&ctrl, &u_params, &u_params);
                    std::hint::black_box(&out);
                }
                BackendKind::CpuParallelSpatialIndex => {
                    let out = cpu::rayon_eval_bicubic_bezier_grid(&ctrl, &u_params, &u_params);
                    std::hint::black_box(&out);
                }
                BackendKind::VulkanCompute => {
                    let vk = global_vulkan()?;
                    let out = vk.compute_eval_bicubic_bezier_grid(&ctrl, &u_params, &u_params)?;
                    std::hint::black_box(&out);
                }
                BackendKind::CudaCompute => {
                    let cu = global_cuda()?;
                    let out = cu.simt_eval_bicubic_bezier_grid(&ctrl, &u_params, &u_params)?;
                    std::hint::black_box(&out);
                }
                BackendKind::PtxSm86 => {
                    let cu = global_cuda()?;
                    let out = cu.ptx_sm86_eval_bicubic_bezier_grid(&ctrl, &u_params, &u_params)?;
                    std::hint::black_box(&out);
                }
                BackendKind::RtxTensorCore => {
                    let cu = global_cuda()?;
                    let out =
                        cu.tensor_core_eval_bicubic_bezier_grid(&ctrl, &u_params, &u_params)?;
                    std::hint::black_box(&out);
                }
                BackendKind::CpuSpatialIndex
                | BackendKind::VulkanRtCore
                | BackendKind::RtxRtCore => return None,
            }
            u64::try_from(t0.elapsed().as_nanos()).ok()
        }
        OperationKind::RayMeshIntersection
        | OperationKind::PointInSolidParity
        | OperationKind::ClosestPointMesh => {
            let count = problem_size.max(4);
            let (verts, tris) = sample_benchmark_solid_mesh();
            let mesh = TriangleMeshView {
                vertices: verts,
                triangles: tris,
            };
            let rays: Vec<RayQuery3> = (0..count)
                .map(|i| {
                    let u = f64::from(u32::try_from(i % 64).unwrap_or(0)) / 63.0;
                    let v = f64::from(u32::try_from((i / 64) % 64).unwrap_or(0)) / 63.0;
                    RayQuery3 {
                        origin: Point3::new(-4.0 + u * 8.0, -4.0 + v * 8.0, -5.0),
                        direction: Vector3::new(0.0, 0.0, 1.0),
                        t_min: 0.0,
                        t_max: 20.0,
                    }
                })
                .collect();
            let pts: Vec<Point3> = (0..count)
                .map(|i| {
                    let u = f64::from(u32::try_from(i % 64).unwrap_or(0)) / 63.0;
                    let v = f64::from(u32::try_from((i / 64) % 64).unwrap_or(0)) / 63.0;
                    let x = -4.0 + u * 8.0;
                    let y = -4.0 + v * 8.0;
                    let r2 = (x * x + y * y) / 50.0;
                    let z_dome = 0.2 + 2.3 * (1.0 - r2).max(0.0);
                    let sign = if i % 2 == 0 { 1.0 } else { -1.0 };
                    Point3::new(x, y, z_dome + sign * 0.05)
                })
                .collect();

            match backend {
                BackendKind::VulkanRtCore => {
                    if let Some(vk) = global_vulkan() {
                        std::hint::black_box(vk.rt_core_ray_mesh_intersect(&rays[..1], mesh));
                    }
                }
                BackendKind::RtxRtCore => {
                    if let Some(cu) = global_cuda() {
                        std::hint::black_box(cu.optix_rt_ray_mesh_intersect(&rays[..1], mesh));
                    }
                }
                _ => {}
            }

            let t0 = Instant::now();
            match (operation, backend) {
                (OperationKind::RayMeshIntersection, BackendKind::Cpu) => {
                    let out = cpu::ray_mesh_intersect(&rays, mesh);
                    std::hint::black_box(&out);
                }
                (OperationKind::RayMeshIntersection, BackendKind::CpuSpatialIndex) => {
                    let out = spatial::intersect_rays_mesh(&rays, mesh);
                    std::hint::black_box(&out);
                }
                (OperationKind::RayMeshIntersection, BackendKind::CpuParallelSpatialIndex) => {
                    let out = spatial::rayon_intersect_rays_mesh(&rays, mesh);
                    std::hint::black_box(&out);
                }
                (OperationKind::RayMeshIntersection, BackendKind::VulkanCompute) => {
                    let vk = global_vulkan()?;
                    let out = vk.compute_ray_mesh_intersect(&rays, mesh)?;
                    std::hint::black_box(&out);
                }
                (OperationKind::RayMeshIntersection, BackendKind::VulkanRtCore) => {
                    let vk = global_vulkan()?;
                    let out = vk.rt_core_ray_mesh_intersect(&rays, mesh)?;
                    std::hint::black_box(&out);
                }
                (OperationKind::RayMeshIntersection, BackendKind::CudaCompute) => {
                    let cu = global_cuda()?;
                    let out = cu.simt_ray_mesh_intersect(&rays, mesh)?;
                    std::hint::black_box(&out);
                }
                (OperationKind::RayMeshIntersection, BackendKind::PtxSm86) => {
                    let cu = global_cuda()?;
                    let out = cu.ptx_sm86_ray_mesh_intersect(&rays, mesh)?;
                    std::hint::black_box(&out);
                }
                (OperationKind::RayMeshIntersection, BackendKind::RtxRtCore) => {
                    let cu = global_cuda()?;
                    let out = cu.optix_rt_ray_mesh_intersect(&rays, mesh)?;
                    std::hint::black_box(&out);
                }
                (OperationKind::PointInSolidParity, BackendKind::Cpu) => {
                    let out = cpu::point_in_solid_parity(&pts, mesh);
                    std::hint::black_box(&out);
                }
                (OperationKind::PointInSolidParity, BackendKind::CpuSpatialIndex) => {
                    let out = spatial::points_in_solid_parity(&pts, mesh);
                    std::hint::black_box(&out);
                }
                (OperationKind::PointInSolidParity, BackendKind::CpuParallelSpatialIndex) => {
                    let out = spatial::rayon_points_in_solid_parity(&pts, mesh);
                    std::hint::black_box(&out);
                }
                (OperationKind::PointInSolidParity, BackendKind::VulkanCompute) => {
                    let vk = global_vulkan()?;
                    let out = vk.compute_point_in_solid_parity(&pts, mesh)?;
                    std::hint::black_box(&out);
                }
                (OperationKind::PointInSolidParity, BackendKind::VulkanRtCore) => {
                    let vk = global_vulkan()?;
                    let out = vk.rt_core_point_in_solid_parity(&pts, mesh)?;
                    std::hint::black_box(&out);
                }
                (OperationKind::PointInSolidParity, BackendKind::CudaCompute) => {
                    let cu = global_cuda()?;
                    let out = cu.simt_point_in_solid_parity(&pts, mesh)?;
                    std::hint::black_box(&out);
                }
                (OperationKind::PointInSolidParity, BackendKind::PtxSm86) => {
                    let cu = global_cuda()?;
                    let out = cu.ptx_sm86_point_in_solid_parity(&pts, mesh)?;
                    std::hint::black_box(&out);
                }
                (OperationKind::PointInSolidParity, BackendKind::RtxRtCore) => {
                    let cu = global_cuda()?;
                    let out = cu.optix_rt_point_in_solid_parity(&pts, mesh)?;
                    std::hint::black_box(&out);
                }
                (OperationKind::ClosestPointMesh, BackendKind::Cpu) => {
                    let out = cpu::closest_point_mesh(&pts, mesh);
                    std::hint::black_box(&out);
                }
                (OperationKind::ClosestPointMesh, BackendKind::CpuSpatialIndex) => {
                    let out = spatial::closest_points_on_mesh(&pts, mesh);
                    std::hint::black_box(&out);
                }
                (OperationKind::ClosestPointMesh, BackendKind::CpuParallelSpatialIndex) => {
                    let out = spatial::rayon_closest_points_on_mesh(&pts, mesh);
                    std::hint::black_box(&out);
                }
                (OperationKind::ClosestPointMesh, BackendKind::VulkanCompute) => {
                    let vk = global_vulkan()?;
                    let out = vk.compute_spatial_closest_point(&pts, mesh)?;
                    std::hint::black_box(&out);
                }
                (OperationKind::ClosestPointMesh, BackendKind::VulkanRtCore) => {
                    let vk = global_vulkan()?;
                    let out = vk.rt_core_closest_point_mesh(&pts, mesh)?;
                    std::hint::black_box(&out);
                }
                (OperationKind::ClosestPointMesh, BackendKind::CudaCompute) => {
                    let cu = global_cuda()?;
                    let out = cu.simt_spatial_closest_point(&pts, mesh)?;
                    std::hint::black_box(&out);
                }
                (OperationKind::ClosestPointMesh, BackendKind::PtxSm86) => {
                    let cu = global_cuda()?;
                    let out = cu.ptx_sm86_spatial_closest_point(&pts, mesh)?;
                    std::hint::black_box(&out);
                }
                (OperationKind::ClosestPointMesh, BackendKind::RtxRtCore) => {
                    let cu = global_cuda()?;
                    let out = cu.optix_rt_closest_point_mesh(&pts, mesh)?;
                    std::hint::black_box(&out);
                }
                _ => return None,
            }
            u64::try_from(t0.elapsed().as_nanos()).ok()
        }
        OperationKind::HausdorffDistance => {
            let pts_a: Vec<Point3> = (0..problem_size)
                .map(|i| {
                    let f = f64::from(u32::try_from(i).unwrap_or(0));
                    Point3::new(
                        f * 0.05 - 10.0,
                        (f * 0.08).sin() * 5.0,
                        (f * 0.03).cos() * 4.0,
                    )
                })
                .collect();
            let pts_b: Vec<Point3> = (0..problem_size)
                .map(|j| {
                    let f = f64::from(u32::try_from(j).unwrap_or(0));
                    Point3::new(
                        f * 0.05 - 9.85,
                        (f * 0.08).sin() * 5.0 + 0.12,
                        (f * 0.03).cos() * 4.0 - 0.08,
                    )
                })
                .collect();
            match backend {
                BackendKind::VulkanRtCore => {
                    if let Some(vk) = global_vulkan() {
                        std::hint::black_box(vk.rt_core_spatial_hausdorff(&pts_a, &pts_b));
                    }
                }
                BackendKind::RtxRtCore => {
                    if let Some(cu) = global_cuda() {
                        std::hint::black_box(cu.optix_rt_spatial_hausdorff(&pts_a, &pts_b));
                    }
                }
                _ => {}
            }
            let t0 = Instant::now();
            match backend {
                BackendKind::Cpu => {
                    let dist_sq = cpu::pairwise_distance_squared(&pts_a, &pts_b);
                    let h = cpu::hausdorff_from_distance_squared_matrix(
                        &dist_sq,
                        pts_a.len(),
                        pts_b.len(),
                    );
                    std::hint::black_box(h);
                }
                BackendKind::CpuSpatialIndex => {
                    let h = spatial::kd_hausdorff_distance(&pts_a, &pts_b);
                    std::hint::black_box(h);
                }
                BackendKind::CpuParallelSpatialIndex => {
                    let h = spatial::rayon_hausdorff_distance(&pts_a, &pts_b);
                    std::hint::black_box(h);
                }
                BackendKind::VulkanCompute => {
                    let vk = global_vulkan()?;
                    let h = vk.compute_spatial_hausdorff(&pts_a, &pts_b)?;
                    std::hint::black_box(h);
                }
                BackendKind::VulkanRtCore => {
                    let vk = global_vulkan()?;
                    let h = vk.rt_core_spatial_hausdorff(&pts_a, &pts_b)?;
                    std::hint::black_box(h);
                }
                BackendKind::CudaCompute => {
                    let cu = global_cuda()?;
                    let h = cu.simt_spatial_hausdorff(&pts_a, &pts_b)?;
                    std::hint::black_box(h);
                }
                BackendKind::PtxSm86 => {
                    let cu = global_cuda()?;
                    let h = cu.ptx_sm86_spatial_hausdorff(&pts_a, &pts_b)?;
                    std::hint::black_box(h);
                }
                BackendKind::RtxRtCore => {
                    let cu = global_cuda()?;
                    let h = cu.optix_rt_spatial_hausdorff(&pts_a, &pts_b)?;
                    std::hint::black_box(h);
                }
                BackendKind::RtxTensorCore => return None,
            }
            u64::try_from(t0.elapsed().as_nanos()).ok()
        }
    }
}

fn isqrt_ceil(n: usize) -> usize {
    if n <= 1 {
        return n;
    }
    let mut x = 1_usize;
    while x.checked_mul(x).is_some_and(|sq| sq < n) {
        x += 1;
    }
    x
}

const fn sample_bezier_control_net() -> [[[f64; 4]; 4]; 4] {
    [
        [
            [0.0, 0.0, 0.0, 1.0],
            [0.0, 1.0, 0.2, 1.0],
            [0.0, 2.0, 0.2, 1.0],
            [0.0, 3.0, 0.0, 1.0],
        ],
        [
            [1.0, 0.0, 0.2, 1.0],
            [1.0, 1.0, 0.8, 1.2],
            [1.0, 2.0, 0.8, 1.2],
            [1.0, 3.0, 0.2, 1.0],
        ],
        [
            [2.0, 0.0, 0.2, 1.0],
            [2.0, 1.0, 0.8, 1.2],
            [2.0, 2.0, 0.8, 1.2],
            [2.0, 3.0, 0.2, 1.0],
        ],
        [
            [3.0, 0.0, 0.0, 1.0],
            [3.0, 1.0, 0.2, 1.0],
            [3.0, 2.0, 0.2, 1.0],
            [3.0, 3.0, 0.0, 1.0],
        ],
    ]
}

fn sample_benchmark_solid_mesh() -> &'static (Vec<Point3>, Vec<[u32; 3]>) {
    static BENCH_MESH: std::sync::OnceLock<(Vec<Point3>, Vec<[u32; 3]>)> =
        std::sync::OnceLock::new();
    BENCH_MESH.get_or_init(|| {
        let nx = 16_u32;
        let ny = 8_u32;
        let stride = nx + 1;
        let mut vertices = Vec::with_capacity(usize::try_from(stride * (ny + 1) * 2).unwrap_or(0));
        let mut triangles = Vec::with_capacity(usize::try_from(nx * ny * 4).unwrap_or(0));

        for half in 0..2_u32 {
            let sign = if half == 0 { 1.0 } else { -1.0 };
            for iy in 0..=ny {
                let v_frac = f64::from(iy) / f64::from(ny);
                let py = -5.0 + v_frac * 10.0;
                for ix in 0..=nx {
                    let u_frac = f64::from(ix) / f64::from(nx);
                    let px = -5.0 + u_frac * 10.0;
                    let r2 = (px * px + py * py) / 50.0;
                    let pz = sign * (0.2 + 2.3 * (1.0 - r2).max(0.0));
                    vertices.push(Point3::new(px, py, pz));
                }
            }
        }

        let top_base = 0_u32;
        let bot_base = stride * (ny + 1);
        for iy in 0..ny {
            for ix in 0..nx {
                let t00 = top_base + iy * stride + ix;
                let t10 = t00 + 1;
                let t01 = t00 + stride;
                let t11 = t01 + 1;
                triangles.push([t00, t10, t11]);
                triangles.push([t00, t11, t01]);

                let b00 = bot_base + iy * stride + ix;
                let b10 = b00 + 1;
                let b01 = b00 + stride;
                let b11 = b01 + 1;
                triangles.push([b00, b11, b10]);
                triangles.push([b00, b01, b11]);
            }
        }
        (vertices, triangles)
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    const DISPATCH_PARITY_TOLERANCE: f64 = 1.0e-11;

    #[test]
    fn piecewise_schedule_merges_adjacent_identical_backends() {
        let winners = [
            (16, BackendKind::Cpu),
            (64, BackendKind::Cpu),
            (256, BackendKind::RtxTensorCore),
            (1024, BackendKind::RtxTensorCore),
        ];
        let sched = build_piecewise_schedule(OperationKind::BatchTransform4x4, &winners);
        assert_eq!(sched.intervals.len(), 2);
        assert_eq!(sched.intervals[0].min_size, 0);
        assert_eq!(sched.intervals[0].max_size, 160);
        assert_eq!(sched.intervals[0].backend, BackendKind::Cpu);
        assert_eq!(sched.intervals[1].min_size, 160);
        assert_eq!(sched.intervals[1].max_size, usize::MAX);
        assert_eq!(sched.intervals[1].backend, BackendKind::RtxTensorCore);

        assert_eq!(sched.select_backend(1), BackendKind::Cpu);
        assert_eq!(sched.select_backend(159), BackendKind::Cpu);
        assert_eq!(sched.select_backend(160), BackendKind::RtxTensorCore);
        assert_eq!(sched.select_backend(100_000), BackendKind::RtxTensorCore);
    }

    #[test]
    fn candidate_backends_scope_all_operations_and_backends() {
        let all = BackendKind::ALL;
        assert!(
            candidate_backends_for_op(OperationKind::BatchTransform4x4, &all)
                .contains(&BackendKind::RtxTensorCore)
        );
        assert!(
            !candidate_backends_for_op(OperationKind::BatchTransform4x4, &all)
                .contains(&BackendKind::VulkanRtCore)
        );
        assert!(
            !candidate_backends_for_op(OperationKind::BatchTransform4x4, &all)
                .contains(&BackendKind::RtxRtCore)
        );
        assert!(
            candidate_backends_for_op(OperationKind::BatchTransform4x4, &all)
                .contains(&BackendKind::VulkanCompute)
        );
        assert!(
            candidate_backends_for_op(OperationKind::BezierSurfaceEval, &all)
                .contains(&BackendKind::RtxTensorCore)
        );
        assert!(
            candidate_backends_for_op(OperationKind::BezierSurfaceEval, &all)
                .contains(&BackendKind::CudaCompute)
        );
        assert!(
            candidate_backends_for_op(OperationKind::BezierSurfaceEval, &all)
                .contains(&BackendKind::PtxSm86)
        );
        assert!(
            !candidate_backends_for_op(OperationKind::HausdorffDistance, &all)
                .contains(&BackendKind::RtxTensorCore)
        );
        assert!(
            candidate_backends_for_op(OperationKind::HausdorffDistance, &all)
                .contains(&BackendKind::VulkanRtCore)
        );
        assert!(
            candidate_backends_for_op(OperationKind::HausdorffDistance, &all)
                .contains(&BackendKind::RtxRtCore)
        );
        assert!(
            !candidate_backends_for_op(OperationKind::ClosestPointMesh, &all)
                .contains(&BackendKind::RtxTensorCore)
        );
        assert!(
            candidate_backends_for_op(OperationKind::ClosestPointMesh, &all)
                .contains(&BackendKind::VulkanRtCore)
        );
        assert!(
            candidate_backends_for_op(OperationKind::ClosestPointMesh, &all)
                .contains(&BackendKind::RtxRtCore)
        );
        assert!(
            !candidate_backends_for_op(OperationKind::PairwiseDistanceMatrix, &all)
                .contains(&BackendKind::RtxTensorCore)
        );
        let parity_candidates = candidate_backends_for_op(OperationKind::PointInSolidParity, &all);
        assert!(parity_candidates.contains(&BackendKind::VulkanCompute));
        assert!(parity_candidates.contains(&BackendKind::VulkanRtCore));
        assert!(parity_candidates.contains(&BackendKind::CudaCompute));
        assert!(parity_candidates.contains(&BackendKind::PtxSm86));
        assert!(parity_candidates.contains(&BackendKind::RtxRtCore));
        assert!(!parity_candidates.contains(&BackendKind::RtxTensorCore));
        assert!(parity_candidates.contains(&BackendKind::CpuSpatialIndex));
        assert!(parity_candidates.contains(&BackendKind::CpuParallelSpatialIndex));

        let disp = crate::global_dispatcher();
        let ctrl = sample_bezier_control_net();
        let params = [0.0, 0.5, 1.0];
        let expected_bez = disp
            .execute_bezier_eval_on(BackendKind::Cpu, &ctrl, &params, &params)
            .expect("CPU Bezier eval");
        assert!(disp
            .execute_bezier_eval_on(BackendKind::CpuSpatialIndex, &ctrl, &params, &params)
            .is_none());
        assert!(disp
            .execute_bezier_eval_on(BackendKind::VulkanRtCore, &ctrl, &params, &params)
            .is_none());
        assert!(disp
            .execute_bezier_eval_on(BackendKind::RtxRtCore, &ctrl, &params, &params)
            .is_none());
        for backend in [
            BackendKind::CpuParallelSpatialIndex,
            BackendKind::VulkanCompute,
            BackendKind::CudaCompute,
            BackendKind::PtxSm86,
            BackendKind::RtxTensorCore,
        ] {
            if let Some(bez) = disp.execute_bezier_eval_on(backend, &ctrl, &params, &params) {
                for (e, t) in expected_bez.iter().zip(&bez) {
                    assert!((e.x - t.x).abs() < DISPATCH_PARITY_TOLERANCE);
                    assert!((e.y - t.y).abs() < DISPATCH_PARITY_TOLERANCE);
                    assert!((e.z - t.z).abs() < DISPATCH_PARITY_TOLERANCE);
                }
            }
        }
    }
}
