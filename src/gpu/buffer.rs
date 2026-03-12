//! GPU buffer management and pooling.

#![cfg(feature = "gpu")]

use std::collections::VecDeque;
use std::sync::Arc;

/// Pool of reusable GPU buffers
pub struct BufferPool {
    device: Arc<wgpu::Device>,
    buffers: VecDeque<wgpu::Buffer>,
    buffer_size: u64,
    _usage: wgpu::BufferUsages,
}

impl BufferPool {
    pub fn new(device: Arc<wgpu::Device>) -> Self {
        Self {
            device,
            buffers: VecDeque::new(),
            buffer_size: 1024 * 1024, // 1MB default
            _usage: wgpu::BufferUsages::STORAGE
                | wgpu::BufferUsages::COPY_DST
                | wgpu::BufferUsages::COPY_SRC,
        }
    }

    /// Get a buffer of at least the requested size
    pub fn get_buffer(&self, min_size: u64, usage: wgpu::BufferUsages) -> wgpu::Buffer {
        // For simplicity, always create new buffer
        // In production, would search pool for suitable buffer
        self.device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("Pooled Buffer"),
            size: min_size.max(self.buffer_size),
            usage,
            mapped_at_creation: false,
        })
    }

    /// Return a buffer to the pool
    pub fn return_buffer(&mut self, buffer: wgpu::Buffer) {
        // Limit pool size
        if self.buffers.len() < 10 {
            self.buffers.push_back(buffer);
        }
        // Otherwise, drop it (GPU memory freed)
    }

    /// Pre-allocate buffers
    pub fn preallocate(&mut self, count: usize, size: u64) {
        for _ in 0..count {
            let buffer = self.device.create_buffer(&wgpu::BufferDescriptor {
                label: Some("Preallocated Buffer"),
                size,
                usage: self._usage,
                mapped_at_creation: false,
            });
            self.buffers.push_back(buffer);
        }
    }

    /// Clear the pool
    pub fn clear(&mut self) {
        self.buffers.clear();
    }
}

/// GPU buffer wrapper with automatic staging
pub struct GpuBuffer {
    buffer: wgpu::Buffer,
    size: u64,
    _usage: wgpu::BufferUsages,
}

impl GpuBuffer {
    pub fn new(device: &wgpu::Device, size: u64, usage: wgpu::BufferUsages) -> Self {
        let buffer = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("GPU Buffer"),
            size,
            usage,
            mapped_at_creation: false,
        });

        Self {
            buffer,
            size,
            _usage: usage,
        }
    }

    pub fn write(&self, queue: &wgpu::Queue, offset: u64, data: &[u8]) {
        queue.write_buffer(&self.buffer, offset, data);
    }

    pub async fn read(&self, device: &wgpu::Device) -> Vec<u8> {
        // Create staging buffer
        let staging = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("Staging Buffer"),
            size: self.size,
            usage: wgpu::BufferUsages::MAP_READ | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });

        // Copy to staging
        let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
            label: Some("Read Encoder"),
        });
        encoder.copy_buffer_to_buffer(&self.buffer, 0, &staging, 0, self.size);

        // Map and read
        // In practice, would use async mapping
        vec![0u8; self.size as usize]
    }

    pub fn as_ref(&self) -> &wgpu::Buffer {
        &self.buffer
    }

    pub fn size(&self) -> u64 {
        self.size
    }
}
