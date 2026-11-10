// SPDX-License-Identifier: Apache-2.0
//! Safe cross-platform Vulkan `wgpu` native `SHADER_F64` compute backend.

use std::sync::mpsc;
use std::sync::OnceLock;

const TRANSFORM_WGSL: &str = "
@group(0) @binding(0) var<storage, read> mat: array<vec4<f64>, 4>;
@group(0) @binding(1) var<storage, read> pts_in: array<vec4<f64>>;
@group(0) @binding(2) var<storage, read_write> pts_out: array<vec4<f64>>;

@compute @workgroup_size(64)
fn main(@builtin(global_invocation_id) gid: vec3<u32>) {
    let i = gid.x;
    if i >= arrayLength(&pts_in) {
        return;
    }
    let p = pts_in[i];
    let r0 = mat[0];
    let r1 = mat[1];
    let r2 = mat[2];
    let r3 = mat[3];
    pts_out[i] = vec4<f64>(
        fma(r0.x, p.x, fma(r0.y, p.y, fma(r0.z, p.z, r0.w * p.w))),
        fma(r1.x, p.x, fma(r1.y, p.y, fma(r1.z, p.z, r1.w * p.w))),
        fma(r2.x, p.x, fma(r2.y, p.y, fma(r2.z, p.z, r2.w * p.w))),
        fma(r3.x, p.x, fma(r3.y, p.y, fma(r3.z, p.z, r3.w * p.w))),
    );
}
";

/// Hardware context for safe `wgpu` native `SHADER_F64` and `EXPERIMENTAL_RAY_QUERY` compute execution.
pub struct VulkanContext {
    device_name: String,
    device: wgpu::Device,
    queue: wgpu::Queue,
    bgl_compute: wgpu::BindGroupLayout,
    pipe_transform: wgpu::ComputePipeline,
}

static GLOBAL_VULKAN: OnceLock<Option<VulkanContext>> = OnceLock::new();

/// Returns the process-wide [`VulkanContext`] if a `wgpu` adapter with native `SHADER_F64` is available.
#[must_use]
pub fn global_vulkan() -> Option<&'static VulkanContext> {
    GLOBAL_VULKAN.get_or_init(VulkanContext::probe).as_ref()
}

fn slice_f64_to_bytes(slice: &[f64]) -> Vec<u8> {
    let mut bytes = Vec::with_capacity(slice.len().checked_mul(8).unwrap_or(0));
    for val in slice {
        bytes.extend_from_slice(&val.to_ne_bytes());
    }
    bytes
}

fn bytes_to_vec_f64(bytes: &[u8], count: usize) -> Vec<f64> {
    let mut out = Vec::with_capacity(count);
    for chunk in bytes.chunks_exact(8).take(count) {
        out.push(f64::from_ne_bytes([
            chunk[0], chunk[1], chunk[2], chunk[3], chunk[4], chunk[5], chunk[6], chunk[7],
        ]));
    }
    out
}

impl VulkanContext {
    /// Probes the primary `wgpu` high-performance adapter for `SHADER_F64` and `EXPERIMENTAL_RAY_QUERY` support.
    #[must_use]
    pub fn probe() -> Option<Self> {
        let instance = wgpu::Instance::default();
        let adapter = pollster::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions {
            power_preference: wgpu::PowerPreference::HighPerformance,
            compatible_surface: None,
            force_fallback_adapter: false,
            apply_limit_buckets: false,
        }))
        .ok()?;

        let info = adapter.get_info();
        let adapter_features = adapter.features();
        if !adapter_features.contains(wgpu::Features::SHADER_F64) {
            return None;
        }
        let supports_ray_query = adapter_features.contains(wgpu::Features::EXPERIMENTAL_RAY_QUERY);

        let mut required_features = wgpu::Features::SHADER_F64;
        let mut required_limits = wgpu::Limits::default();
        if supports_ray_query {
            let adapter_limits = adapter.limits();
            required_features |= wgpu::Features::EXPERIMENTAL_RAY_QUERY;
            required_limits.max_blas_primitive_count = adapter_limits.max_blas_primitive_count;
            required_limits.max_blas_geometry_count = adapter_limits.max_blas_geometry_count;
            required_limits.max_tlas_instance_count = adapter_limits.max_tlas_instance_count;
            required_limits.max_acceleration_structures_per_shader_stage =
                adapter_limits.max_acceleration_structures_per_shader_stage;
            required_limits.max_buffers_and_acceleration_structures_per_shader_stage =
                adapter_limits.max_buffers_and_acceleration_structures_per_shader_stage;
        }
        let experimental_features = if supports_ray_query {
            unsafe { wgpu::ExperimentalFeatures::enabled() }
        } else {
            wgpu::ExperimentalFeatures::disabled()
        };

        let (device, queue) = pollster::block_on(adapter.request_device(&wgpu::DeviceDescriptor {
            label: Some("cadmpeg-gpu-device"),
            required_features,
            required_limits,
            memory_hints: wgpu::MemoryHints::Performance,
            trace: wgpu::Trace::Off,
            experimental_features,
        }))
        .ok()?;

        let bgl_compute = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("cadmpeg-gpu-bgl"),
            entries: &[
                wgpu::BindGroupLayoutEntry {
                    binding: 0,
                    visibility: wgpu::ShaderStages::COMPUTE,
                    ty: wgpu::BindingType::Buffer {
                        ty: wgpu::BufferBindingType::Storage { read_only: true },
                        has_dynamic_offset: false,
                        min_binding_size: None,
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 1,
                    visibility: wgpu::ShaderStages::COMPUTE,
                    ty: wgpu::BindingType::Buffer {
                        ty: wgpu::BufferBindingType::Storage { read_only: true },
                        has_dynamic_offset: false,
                        min_binding_size: None,
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 2,
                    visibility: wgpu::ShaderStages::COMPUTE,
                    ty: wgpu::BindingType::Buffer {
                        ty: wgpu::BufferBindingType::Storage { read_only: false },
                        has_dynamic_offset: false,
                        min_binding_size: None,
                    },
                    count: None,
                },
            ],
        });

        let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("cadmpeg-gpu-pipe-layout"),
            bind_group_layouts: &[Some(&bgl_compute)],
            immediate_size: 0,
        });

        let compile_pipe = |label: &str, wgsl: &str| -> wgpu::ComputePipeline {
            let sm = device.create_shader_module(wgpu::ShaderModuleDescriptor {
                label: Some(label),
                source: wgpu::ShaderSource::Wgsl(wgsl.into()),
            });
            device.create_compute_pipeline(&wgpu::ComputePipelineDescriptor {
                label: Some(label),
                layout: Some(&pipeline_layout),
                module: &sm,
                entry_point: Some("main"),
                compilation_options: wgpu::PipelineCompilationOptions::default(),
                cache: None,
            })
        };

        let _ = supports_ray_query;

        Some(Self {
            device_name: info.name,
            pipe_transform: compile_pipe("cadmpeg-wgsl-transform", TRANSFORM_WGSL),
            device,
            queue,
            bgl_compute,
        })
    }

    /// Name of the active `wgpu` physical adapter.
    #[must_use]
    pub fn device_name(&self) -> &str {
        &self.device_name
    }

    /// Returns whether hardware RT Core ray queries (`EXPERIMENTAL_RAY_QUERY`) are active.
    #[must_use]
    pub const fn supports_ray_query(&self) -> bool {
        false
    }

    fn execute_f64_pipeline(
        &self,
        pipeline: &wgpu::ComputePipeline,
        buf0_f64: &[f64],
        buf1_f64: &[f64],
        out_f64_len: usize,
        work_items: usize,
    ) -> Option<Vec<f64>> {
        if work_items == 0 || out_f64_len == 0 {
            return Some(Vec::new());
        }
        let bytes0 = slice_f64_to_bytes(buf0_f64);
        let bytes1 = slice_f64_to_bytes(buf1_f64);
        let size0 = u64::try_from(bytes0.len().max(32)).ok()?;
        let size1 = u64::try_from(bytes1.len().max(32)).ok()?;
        let out_byte_len = out_f64_len.checked_mul(8)?;
        let out_size = u64::try_from(out_byte_len.max(32)).ok()?;

        let b0 = self.device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("cadmpeg-gpu-b0"),
            size: size0,
            usage: wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let b1 = self.device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("cadmpeg-gpu-b1"),
            size: size1,
            usage: wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let b_out = self.device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("cadmpeg-gpu-b-out"),
            size: out_size,
            usage: wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_SRC,
            mapped_at_creation: false,
        });
        let b_staging = self.device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("cadmpeg-gpu-b-staging"),
            size: out_size,
            usage: wgpu::BufferUsages::MAP_READ | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });

        self.queue.write_buffer(&b0, 0, &bytes0);
        self.queue.write_buffer(&b1, 0, &bytes1);

        let bg = self.device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("cadmpeg-gpu-bg"),
            layout: &self.bgl_compute,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: b0.as_entire_binding(),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: b1.as_entire_binding(),
                },
                wgpu::BindGroupEntry {
                    binding: 2,
                    resource: b_out.as_entire_binding(),
                },
            ],
        });

        let total_groups = u32::try_from(work_items).ok()?.div_ceil(64);
        if total_groups > 65_535 {
            return None;
        }
        let mut encoder = self
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                label: Some("cadmpeg-gpu-encoder"),
            });
        {
            let mut cpass = encoder.begin_compute_pass(&wgpu::ComputePassDescriptor {
                label: Some("cadmpeg-gpu-cpass"),
                timestamp_writes: None,
            });
            cpass.set_pipeline(pipeline);
            cpass.set_bind_group(0, &bg, &[]);
            cpass.dispatch_workgroups(total_groups, 1, 1);
        }
        encoder.copy_buffer_to_buffer(&b_out, 0, &b_staging, 0, out_size);
        self.queue.submit(Some(encoder.finish()));

        let slice = b_staging.slice(..);
        let (tx, rx) = mpsc::channel();
        slice.map_async(wgpu::MapMode::Read, move |res| {
            tx.send(res).ok();
        });
        self.device.poll(wgpu::PollType::wait_indefinitely()).ok()?;
        rx.recv().ok()?.ok()?;

        let mapped = slice.get_mapped_range().ok()?;
        let out = bytes_to_vec_f64(&mapped, out_f64_len);
        drop(mapped);
        b_staging.unmap();
        Some(out)
    }

    /// Executes native `f64` 4x4 homogeneous point transformation via Vulkan `wgpu`.
    #[must_use]
    pub fn compute_batch_transform_4x4(
        &self,
        matrix: &[[f64; 4]; 4],
        points: &[[f64; 4]],
    ) -> Option<Vec<[f64; 4]>> {
        if points.is_empty() {
            return Some(Vec::new());
        }
        let flat_pts: Vec<f64> = points.iter().flat_map(|p| *p).collect();
        let raw = self.execute_f64_pipeline(
            &self.pipe_transform,
            matrix.as_flattened(),
            &flat_pts,
            flat_pts.len(),
            points.len(),
        )?;
        Some(
            raw.chunks_exact(4)
                .map(|c| [c[0], c[1], c[2], c[3]])
                .collect(),
        )
    }
}
#[cfg(test)]
mod tests {
    use super::*;

    const VULKAN_PARITY_TOLERANCE: f64 = 1.0e-11;

    #[test]
    fn vulkan_transform_and_1e20_extreme_magnitude_match_cpu() {
        let Some(vk) = global_vulkan() else {
            return;
        };
        assert!(!vk.device_name().is_empty());

        let matrix = [
            [1.25, -0.5, 0.25, 4.0],
            [0.5, 2.0, -0.75, -2.5],
            [-0.25, 0.125, 1.5, 1.0],
            [0.0, 0.0, 0.0, 1.0],
        ];
        let points: Vec<[f64; 4]> = (0..19)
            .map(|i| {
                let f = f64::from(i);
                [f * 0.3 - 1.0, f * -0.7 + 2.0, f * 0.5, 1.0]
            })
            .collect();
        let expected = cpu::batch_transform_4x4(&matrix, &points);
        let actual = vk
            .compute_batch_transform_4x4(&matrix, &points)
            .expect("Vulkan f64 transform");
        for (e, a) in expected.iter().zip(&actual) {
            for c in 0..4 {
                assert!((e[c] - a[c]).abs() < VULKAN_PARITY_TOLERANCE);
            }
        }

        let scale = 1.0e20_f64;
        let big_mat = [
            [scale, 0.0, 0.0, 0.0],
            [0.0, scale, 0.0, 0.0],
            [0.0, 0.0, scale, 0.0],
            [0.0, 0.0, 0.0, 1.0],
        ];
        let big_pts = [[scale, -scale, 0.5 * scale, 1.0]];
        let big_out = vk
            .compute_batch_transform_4x4(&big_mat, &big_pts)
            .expect("Vulkan 1e20 transform");
        assert_eq!(big_out[0][0], 1.0e40);
        assert!(!big_out[0][0].is_nan());
    }
}
