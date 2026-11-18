//! Folderstorm: the session host's Economy gives the GPU culling buffers
//! back while its 3D is paused (`folderstorm/plans/gpu-memory.md`, "Economy,
//! second pass"). A child of `gpu_cull.rs` (one `#[path]` line there), so it
//! reaches the buffers it replaces.
//!
//! The indirect arguments (39 regions of 20 bytes per slot) and the masks
//! follow the largest scene seen, in powers of two, and never shrink: 51 MB
//! at 65k slots, 409 MB at 524k. Asked from the main thread
//! ([`folderstorm_release`]), the release is done by the render thread at
//! the start of its next frame (`Backend::render`, one line), before
//! `prepare`: a frame that draws the world grows them again at once, as
//! `prepare` always does, so it is asked only once the 3D is paused (empty
//! draw lists, no `prepare`, no `encode`). The bind groups are made at each
//! `encode` and the draws read `args()` and `cap()` of their own frame: no
//! handle of the old buffers outlives the frames already in flight.

use super::{BINS, CLASSIFY_GROUP, GpuCull, face_capacity};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};

static RELEASE: AtomicBool = AtomicBool::new(false);
static FREED: AtomicU64 = AtomicU64::new(0);

/// The culling buffers go back to their first size at the renderer's next
/// frame. Only while the 3D is paused: a frame that draws the world grows
/// them again.
pub fn folderstorm_release() {
    FREED.store(0, Ordering::Relaxed);
    RELEASE.store(true, Ordering::Relaxed);
}

/// The bytes the last asked release gave back (0: none, or not done yet).
pub fn folderstorm_freed() -> u64 {
    FREED.load(Ordering::Relaxed)
}

/// What [`GpuCull::new`] makes them with.
const FIRST_SPHERES: u32 = 4096;

impl GpuCull {
    /// At the start of a frame, before `prepare`: the release asked by
    /// [`folderstorm_release`], if any.
    pub(crate) fn folderstorm_release(&mut self, device: &wgpu::Device) {
        if !RELEASE.swap(false, Ordering::Relaxed) {
            return;
        }
        let held = |s: &Self| s.args.size() + s.masks.size() + s.group_data.size() + s.spheres.size();
        let before = held(self);
        let cap = 4096.min(face_capacity(&device.limits()));
        if self.cap > cap {
            self.args = Self::make_args(device, cap);
            self.masks = Self::make_storage(device, "cull masks", u64::from(cap) * 4);
            self.group_data = Self::make_storage(device, "cull groups", u64::from(cap / CLASSIFY_GROUP) * BINS as u64 * 4);
            self.cap = cap;
        }
        if self.sphere_cap > FIRST_SPHERES {
            self.spheres = Self::make_storage(device, "cull spheres", u64::from(FIRST_SPHERES) * 16);
            self.sphere_cap = FIRST_SPHERES;
        }
        self.faces = self.faces.min(self.cap);
        self.objects = self.objects.min(self.sphere_cap);
        self.groups = self.faces.div_ceil(CLASSIFY_GROUP).max(1);
        let freed = before.saturating_sub(held(self));
        FREED.store(freed, Ordering::Relaxed);
        log::info!(
            "GPU culling: {} MB of buffers given back while the 3D is paused ({} slots per bin)",
            freed >> 20,
            self.cap
        );
    }
}

/// The release and the growth after it, on a real device. Needs a GPU:
/// `cargo test --workspace --profile ci folderstorm_cull -- --ignored`.
#[cfg(test)]
mod tests {
    use super::super::CullFrame;
    use super::*;
    use bytemuck::Zeroable;

    #[test]
    #[ignore]
    fn folderstorm_cull_release_shrinks_then_grows_again() {
        let _gpu = crate::test_gpu::lock();
        let instance = wgpu::Instance::default();
        let Ok(adapter) = pollster::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions::default())) else {
            eprintln!("no GPU adapter: skipped");
            return;
        };
        let (device, queue) = pollster::block_on(adapter.request_device(&wgpu::DeviceDescriptor {
            required_limits: adapter.limits(),
            ..Default::default()
        }))
        .expect("GPU device");
        let mut cull = GpuCull::new(&device);
        let first = (cull.cap(), cull.args().size());
        let faces = (first.0 as usize * 4).min(face_capacity(&device.limits()) as usize);
        let validation = device.push_error_scope(wgpu::ErrorFilter::Validation);
        cull.prepare(&device, &queue, faces, 8192, u32::MAX, CullFrame::zeroed());
        let grown = cull.cap();
        // nothing asked: nothing changes
        cull.folderstorm_release(&device);
        assert_eq!(cull.cap(), grown);
        folderstorm_release();
        cull.folderstorm_release(&device);
        assert_eq!((cull.cap(), cull.args().size()), first);
        assert_eq!(cull.sphere_cap, FIRST_SPHERES);
        if grown > first.0 {
            assert!(folderstorm_freed() > 0);
        }
        // the next frame that draws the world grows them again
        cull.prepare(&device, &queue, faces, 8192, u32::MAX, CullFrame::zeroed());
        assert_eq!(cull.cap(), grown);
        assert!(pollster::block_on(validation.pop()).is_none());
    }
}
