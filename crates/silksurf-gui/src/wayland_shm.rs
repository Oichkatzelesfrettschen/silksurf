use std::{
    fs::File,
    io::ErrorKind,
    os::fd::{AsFd, AsRawFd},
    rc::Rc,
    slice,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    time::{Duration, Instant},
};

use memmap2::MmapMut;
use raw_window_handle::{HasDisplayHandle, HasWindowHandle, RawDisplayHandle, RawWindowHandle};
use wayland_client::{
    Connection, Dispatch, Proxy, QueueHandle,
    backend::{Backend, ObjectId, WaylandError},
    globals::{GlobalListContents, registry_queue_init},
    protocol::{wl_buffer, wl_registry, wl_shm, wl_shm_pool, wl_surface},
};
use winit::window::Window;

use crate::{WinitDamageRect, WinitPresentDamage};

const WAYLAND_SHM_BUFFER_COUNT: usize = 4;
const WAYLAND_SHM_FULL_DAMAGE_PRESEED_COUNT: usize = 2;

#[allow(clippy::large_enum_variant)]
pub enum WaylandShmDrawOutcome {
    Presented {
        damage: WinitPresentDamage,
        buffer_age: u8,
        timings: WaylandShmPhaseTimings,
    },
    Busy,
}

#[derive(Clone, Copy, Debug, Default)]
pub struct WaylandShmPhaseTimings {
    pub pump: Duration,
    pub ensure: Duration,
    pub acquire: Duration,
    pub seed: Duration,
    pub render: Duration,
    pub attach_damage: Duration,
    pub flush: Duration,
    pub preseed: Duration,
}

#[derive(Clone, Copy, Debug, Default)]
struct WaylandShmPresentTimings {
    attach_damage: Duration,
    flush: Duration,
}

struct WaylandShmState;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct WaylandShmRetainedTag(u64);

impl WaylandShmRetainedTag {
    pub const fn new(value: u64) -> Self {
        Self(value)
    }
}

pub struct WaylandShmSurface {
    conn: Connection,
    event_queue: wayland_client::EventQueue<WaylandShmState>,
    qh: QueueHandle<WaylandShmState>,
    shm: wl_shm::WlShm,
    surface: Option<wl_surface::WlSurface>,
    buffers: Vec<WaylandShmBuffer>,
    width: u32,
    height: u32,
    buffer_width: u32,
    buffer_height: u32,
    last_presented_buffer: Option<usize>,
    window: Rc<Window>,
}

impl WaylandShmSurface {
    pub fn new(window: Rc<Window>) -> Result<Self, String> {
        let display_handle = window.display_handle().map_err(|e| e.to_string())?.as_raw();
        let RawDisplayHandle::Wayland(display_handle) = display_handle else {
            return Err("window is not backed by Wayland".to_string());
        };
        let window_handle = window.window_handle().map_err(|e| e.to_string())?.as_raw();
        let RawWindowHandle::Wayland(window_handle) = window_handle else {
            return Err("window does not expose a Wayland surface".to_string());
        };

        // SAFETY: winit exposes a live Wayland display handle for this window.
        let backend =
            unsafe { Backend::from_foreign_display(display_handle.display.as_ptr().cast()) };
        let conn = Connection::from_backend(backend);
        let (globals, event_queue) =
            registry_queue_init(&conn).map_err(|e| format!("registry init failed: {e}"))?;
        let qh = event_queue.handle();
        let shm: wl_shm::WlShm = globals
            .bind(&qh, 1..=1, ())
            .map_err(|e| format!("wl_shm bind failed: {e}"))?;
        // SAFETY: winit exposes a live Wayland surface handle for this window.
        let surface_id = unsafe {
            ObjectId::from_ptr(
                wl_surface::WlSurface::interface(),
                window_handle.surface.as_ptr().cast(),
            )
        }
        .map_err(|e| format!("surface import failed: {e}"))?;
        let surface = wl_surface::WlSurface::from_id(&conn, surface_id)
            .map_err(|e| format!("surface proxy failed: {e}"))?;

        Ok(Self {
            conn,
            event_queue,
            qh,
            shm,
            surface: Some(surface),
            buffers: Vec::new(),
            width: 0,
            height: 0,
            buffer_width: 0,
            buffer_height: 0,
            last_presented_buffer: None,
            window,
        })
    }

    pub fn draw_and_present(
        &mut self,
        width: u32,
        height: u32,
        trace_phase_timing: bool,
        render_fn: impl FnOnce(u8, &mut [u32]) -> WinitPresentDamage,
    ) -> Result<WaylandShmDrawOutcome, String> {
        let mut phase_start = phase_timer_start(trace_phase_timing);
        let mut timings = WaylandShmPhaseTimings::default();
        self.pump_events()?;
        record_phase(&mut timings.pump, &mut phase_start);
        self.ensure_buffers(width, height)?;
        record_phase(&mut timings.ensure, &mut phase_start);
        let Some(buffer_index) = self.acquire_released_buffer()? else {
            return Ok(WaylandShmDrawOutcome::Busy);
        };
        record_phase(&mut timings.acquire, &mut phase_start);
        self.seed_virgin_buffer(buffer_index, width, height);
        record_phase(&mut timings.seed, &mut phase_start);
        let buffer_age = self.buffers[buffer_index].age;
        let damage = {
            // SAFETY: ensure_buffers sizes the selected buffer for width * height pixels.
            let pixels = unsafe { self.buffers[buffer_index].mapped_mut() };
            render_fn(buffer_age, pixels)
        };
        record_phase(&mut timings.render, &mut phase_start);
        if damage == WinitPresentDamage::Clean {
            return Ok(WaylandShmDrawOutcome::Presented {
                damage,
                buffer_age,
                timings,
            });
        }
        let present_timings = self.present_buffer(buffer_index, damage, trace_phase_timing)?;
        timings.attach_damage = present_timings.attach_damage;
        timings.flush = present_timings.flush;
        phase_start = phase_timer_start(trace_phase_timing);
        if damage == WinitPresentDamage::Full {
            self.seed_released_virgin_buffers(buffer_index, width, height);
        }
        record_phase(&mut timings.preseed, &mut phase_start);
        Ok(WaylandShmDrawOutcome::Presented {
            damage,
            buffer_age,
            timings,
        })
    }

    pub fn warm_one_released_virgin_buffer(&mut self) -> bool {
        let Some(source_index) = self.last_presented_buffer else {
            return false;
        };
        let Some(pixel_count) = surface_pixel_count(self.width, self.height) else {
            return false;
        };
        for target_index in 0..self.buffers.len() {
            if !self.buffer_needs_seed(source_index, target_index) {
                continue;
            }
            if copy_buffer_prefix(&mut self.buffers, source_index, target_index, pixel_count) {
                self.buffers[target_index].age = 1;
                return true;
            }
        }
        false
    }

    pub fn write_released_retained_buffer(
        &mut self,
        tag: WaylandShmRetainedTag,
        width: u32,
        height: u32,
        pixels: &[u32],
    ) -> Result<bool, String> {
        self.pump_events()?;
        self.ensure_buffers(width, height)?;
        let Some(pixel_count) = surface_pixel_count(width, height) else {
            return Ok(false);
        };
        if pixels.len() < pixel_count {
            return Ok(false);
        }
        let Some(target_index) = self.released_unretained_buffer_index() else {
            return Ok(false);
        };
        // SAFETY: pixel_count is bounded by the target buffer dimensions above.
        let target_pixels = unsafe { self.buffers[target_index].mapped_prefix_mut(pixel_count) };
        target_pixels.copy_from_slice(&pixels[..pixel_count]);
        self.buffers[target_index].age = 1;
        self.buffers[target_index].retained_tag = Some(tag);
        Ok(true)
    }

    pub fn retained_buffer_available(&self, tag: WaylandShmRetainedTag) -> bool {
        self.released_retained_buffer_index(tag).is_some()
    }

    pub fn present_retained_buffer(
        &mut self,
        tag: WaylandShmRetainedTag,
        damage: WinitPresentDamage,
        trace_phase_timing: bool,
    ) -> Result<Option<WaylandShmDrawOutcome>, String> {
        let mut phase_start = phase_timer_start(trace_phase_timing);
        let mut timings = WaylandShmPhaseTimings::default();
        self.pump_events()?;
        record_phase(&mut timings.pump, &mut phase_start);
        let buffer_index = match self.released_retained_buffer_index(tag) {
            Some(index) => index,
            None => {
                self.read_and_dispatch_events()?;
                match self.released_retained_buffer_index(tag) {
                    Some(index) => index,
                    None => return Ok(None),
                }
            }
        };
        record_phase(&mut timings.acquire, &mut phase_start);
        let buffer_age = self.buffers[buffer_index].age;
        if damage == WinitPresentDamage::Clean {
            return Ok(Some(WaylandShmDrawOutcome::Presented {
                damage,
                buffer_age,
                timings,
            }));
        }
        let present_timings = self.present_buffer(buffer_index, damage, trace_phase_timing)?;
        timings.attach_damage = present_timings.attach_damage;
        timings.flush = present_timings.flush;
        Ok(Some(WaylandShmDrawOutcome::Presented {
            damage,
            buffer_age,
            timings,
        }))
    }

    fn pump_events(&mut self) -> Result<(), String> {
        self.event_queue
            .dispatch_pending(&mut WaylandShmState)
            .map_err(|e| format!("Wayland dispatch failed: {e}"))?;
        Ok(())
    }

    fn acquire_released_buffer(&mut self) -> Result<Option<usize>, String> {
        if let Some(index) = self.released_current_buffer_index() {
            return Ok(Some(index));
        }
        if let Some(index) = self.released_warm_buffer_index() {
            return Ok(Some(index));
        }
        if let Some(index) = self.released_buffer_index() {
            return Ok(Some(index));
        }
        self.read_and_dispatch_events()?;
        Ok(self
            .released_current_buffer_index()
            .or_else(|| self.released_warm_buffer_index())
            .or_else(|| self.released_buffer_index()))
    }

    fn released_current_buffer_index(&self) -> Option<usize> {
        let index = self.last_presented_buffer?;
        self.buffers.get(index)?.released().then_some(index)
    }

    fn released_warm_buffer_index(&self) -> Option<usize> {
        self.buffers
            .iter()
            .enumerate()
            .filter(|(_, buffer)| buffer.released() && buffer.age != 0)
            .max_by_key(|(_, buffer)| buffer.age)
            .map(|(index, _)| index)
    }

    fn released_buffer_index(&self) -> Option<usize> {
        self.buffers.iter().position(WaylandShmBuffer::released)
    }

    fn released_unretained_buffer_index(&self) -> Option<usize> {
        self.buffers
            .iter()
            .position(|buffer| buffer.released() && buffer.retained_tag.is_none())
    }

    fn released_retained_buffer_index(&self, tag: WaylandShmRetainedTag) -> Option<usize> {
        self.buffers
            .iter()
            .position(|buffer| buffer.released() && buffer.retained_tag == Some(tag))
    }

    fn read_and_dispatch_events(&mut self) -> Result<(), String> {
        let Some(guard) = self.event_queue.prepare_read() else {
            return self.pump_events();
        };
        match guard.read() {
            Ok(_) => self.pump_events(),
            Err(WaylandError::Io(err)) if err.kind() == ErrorKind::WouldBlock => Ok(()),
            Err(err) => Err(format!("Wayland read failed: {err}")),
        }
    }

    fn ensure_buffers(&mut self, width: u32, height: u32) -> Result<(), String> {
        if width == 0 || height == 0 {
            return Err("Wayland surface has zero size".to_string());
        }
        if buffers_cover_size(
            self.buffer_width,
            self.buffer_height,
            self.buffers.len(),
            width,
            height,
        ) {
            self.width = width;
            self.height = height;
            return Ok(());
        }

        self.buffers.clear();
        self.last_presented_buffer = None;
        let width_i32 = i32::try_from(width).map_err(|_| "surface width is too large")?;
        let height_i32 = i32::try_from(height).map_err(|_| "surface height is too large")?;
        for _ in 0..WAYLAND_SHM_BUFFER_COUNT {
            self.buffers.push(WaylandShmBuffer::new(
                &self.shm, width_i32, height_i32, &self.qh,
            )?);
        }
        self.width = width;
        self.height = height;
        self.buffer_width = width;
        self.buffer_height = height;
        Ok(())
    }

    fn present_buffer(
        &mut self,
        buffer_index: usize,
        damage: WinitPresentDamage,
        trace_phase_timing: bool,
    ) -> Result<WaylandShmPresentTimings, String> {
        let mut phase_start = phase_timer_start(trace_phase_timing);
        let mut timings = WaylandShmPresentTimings::default();
        let surface = self
            .surface
            .as_ref()
            .ok_or_else(|| "Wayland surface is already dropped".to_string())?;
        if self.last_presented_buffer == Some(buffer_index) {
            self.buffers[buffer_index].mark_busy();
        } else {
            self.buffers[buffer_index].attach(surface);
        }
        match damage {
            WinitPresentDamage::Clean => {}
            WinitPresentDamage::Full => damage_full(surface),
            WinitPresentDamage::Rect(rect) => damage_rect(surface, rect)?,
            WinitPresentDamage::Rects(rects) => {
                for rect in rects.as_slice() {
                    damage_rect(surface, *rect)?;
                }
            }
        }
        surface.commit();
        record_phase(&mut timings.attach_damage, &mut phase_start);
        self.conn
            .flush()
            .map_err(|e| format!("Wayland flush failed: {e}"))?;
        record_phase(&mut timings.flush, &mut phase_start);

        for (index, buffer) in self.buffers.iter_mut().enumerate() {
            if index == buffer_index {
                buffer.age = 1;
                buffer.retained_tag = None;
            } else if buffer.age != 0 {
                buffer.age = buffer.age.saturating_add(1);
            }
        }
        self.last_presented_buffer = Some(buffer_index);
        Ok(timings)
    }

    fn seed_virgin_buffer(&mut self, buffer_index: usize, width: u32, height: u32) {
        if self.buffers[buffer_index].age != 0 {
            return;
        }
        let Some(source_index) = self.last_presented_buffer else {
            return;
        };
        if source_index == buffer_index {
            return;
        }
        let Some(pixel_count) = surface_pixel_count(width, height) else {
            return;
        };
        if copy_buffer_prefix(&mut self.buffers, source_index, buffer_index, pixel_count) {
            self.buffers[buffer_index].age = 1;
            self.buffers[buffer_index].retained_tag = None;
        }
    }

    fn seed_released_virgin_buffers(&mut self, source_index: usize, width: u32, height: u32) {
        let Some(pixel_count) = surface_pixel_count(width, height) else {
            return;
        };
        let mut seeded_count = 0;
        for target_index in 0..self.buffers.len() {
            if seeded_count >= WAYLAND_SHM_FULL_DAMAGE_PRESEED_COUNT {
                return;
            }
            if !self.buffer_needs_seed(source_index, target_index) {
                continue;
            }
            if copy_buffer_prefix(&mut self.buffers, source_index, target_index, pixel_count) {
                self.buffers[target_index].age = 1;
                self.buffers[target_index].retained_tag = None;
                seeded_count += 1;
            }
        }
    }

    fn buffer_needs_seed(&self, source_index: usize, target_index: usize) -> bool {
        target_index != source_index
            && self.buffers[target_index].age == 0
            && self.buffers[target_index].released()
    }
}

impl Drop for WaylandShmSurface {
    fn drop(&mut self) {
        self.surface = None;
        drop(self.window.clone());
    }
}

struct WaylandShmBuffer {
    tempfile: File,
    map: MmapMut,
    pool: wl_shm_pool::WlShmPool,
    buffer: wl_buffer::WlBuffer,
    width: i32,
    height: i32,
    released: Arc<AtomicBool>,
    age: u8,
    retained_tag: Option<WaylandShmRetainedTag>,
}

impl WaylandShmBuffer {
    fn new(
        shm: &wl_shm::WlShm,
        width: i32,
        height: i32,
        qh: &QueueHandle<WaylandShmState>,
    ) -> Result<Self, String> {
        let pool_size = shm_pool_size(width, height)?;
        let tempfile = create_memfile()?;
        tempfile
            .set_len(u64::try_from(pool_size).map_err(|_| "SHM pool size is negative")?)
            .map_err(|e| format!("SHM file resize failed: {e}"))?;
        // SAFETY: tempfile is resized to the wl_shm pool size before mapping.
        let map = unsafe { map_file(&tempfile)? };
        let pool = shm.create_pool(tempfile.as_fd(), pool_size, qh, ());
        let released = Arc::new(AtomicBool::new(true));
        let buffer = pool.create_buffer(
            0,
            width,
            height,
            width.saturating_mul(4),
            wl_shm::Format::Xrgb8888,
            qh,
            released.clone(),
        );
        Ok(Self {
            tempfile,
            map,
            pool,
            buffer,
            width,
            height,
            released,
            age: 0,
            retained_tag: None,
        })
    }

    fn released(&self) -> bool {
        self.released.load(Ordering::Acquire)
    }

    fn attach(&self, surface: &wl_surface::WlSurface) {
        self.mark_busy();
        surface.attach(Some(&self.buffer), 0, 0);
    }

    fn mark_busy(&self) {
        self.released.store(false, Ordering::Release);
    }

    #[allow(clippy::cast_ptr_alignment)]
    unsafe fn mapped_mut(&mut self) -> &mut [u32] {
        let len = usize::try_from(self.width)
            .unwrap_or(0)
            .saturating_mul(usize::try_from(self.height).unwrap_or(0));
        // SAFETY: memfd mappings are page-aligned and len stays inside the mapped pool.
        unsafe { slice::from_raw_parts_mut(self.map.as_mut_ptr().cast::<u32>(), len) }
    }

    fn mapped_word_len(&self) -> usize {
        usize::try_from(self.width)
            .unwrap_or(0)
            .saturating_mul(usize::try_from(self.height).unwrap_or(0))
    }

    #[allow(clippy::cast_ptr_alignment)]
    unsafe fn mapped_prefix(&self, len: usize) -> &[u32] {
        // SAFETY: callers pass len values bounded by mapped_word_len.
        unsafe { slice::from_raw_parts(self.map.as_ptr().cast::<u32>(), len) }
    }

    #[allow(clippy::cast_ptr_alignment)]
    unsafe fn mapped_prefix_mut(&mut self, len: usize) -> &mut [u32] {
        // SAFETY: callers pass len values bounded by mapped_word_len.
        unsafe { slice::from_raw_parts_mut(self.map.as_mut_ptr().cast::<u32>(), len) }
    }
}

impl Drop for WaylandShmBuffer {
    fn drop(&mut self) {
        self.buffer.destroy();
        self.pool.destroy();
        let _ = self.tempfile.as_raw_fd();
    }
}

fn create_memfile() -> Result<File, String> {
    use rustix::fs::{MemfdFlags, SealFlags};

    let name = c"silksurf-wayland-shm";
    let fd = rustix::fs::memfd_create(name, MemfdFlags::CLOEXEC | MemfdFlags::ALLOW_SEALING)
        .map_err(|e| format!("memfd_create failed: {e}"))?;
    rustix::fs::fcntl_add_seals(&fd, SealFlags::SHRINK | SealFlags::SEAL)
        .map_err(|e| format!("memfd seal failed: {e}"))?;
    Ok(File::from(fd))
}

unsafe fn map_file(file: &File) -> Result<MmapMut, String> {
    // SAFETY: callers resize the memfd before mapping it for wl_shm use.
    unsafe { MmapMut::map_mut(file.as_raw_fd()).map_err(|e| format!("mmap failed: {e}")) }
}

fn shm_pool_size(width: i32, height: i32) -> Result<i32, String> {
    let pixel_count = width
        .checked_mul(height)
        .and_then(|pixels| pixels.checked_mul(4))
        .ok_or_else(|| "SHM pool size overflow".to_string())?;
    Ok(u32::try_from(pixel_count)
        .map_err(|_| "SHM pool size is negative".to_string())?
        .next_power_of_two() as i32)
}

fn damage_full(surface: &wl_surface::WlSurface) {
    if surface.version() < 4 {
        surface.damage(0, 0, i32::MAX, i32::MAX);
    } else {
        surface.damage_buffer(0, 0, i32::MAX, i32::MAX);
    }
}

fn damage_rect(surface: &wl_surface::WlSurface, rect: WinitDamageRect) -> Result<(), String> {
    let x = i32::try_from(rect.x).map_err(|_| "damage x is too large")?;
    let y = i32::try_from(rect.y).map_err(|_| "damage y is too large")?;
    let width = i32::try_from(rect.width).map_err(|_| "damage width is too large")?;
    let height = i32::try_from(rect.height).map_err(|_| "damage height is too large")?;
    if surface.version() < 4 {
        surface.damage(x, y, width, height);
    } else {
        surface.damage_buffer(x, y, width, height);
    }
    Ok(())
}

fn buffers_cover_size(
    buffer_width: u32,
    buffer_height: u32,
    buffer_count: usize,
    width: u32,
    height: u32,
) -> bool {
    buffer_count == WAYLAND_SHM_BUFFER_COUNT
        && buffer_width == width
        && buffer_height >= height
        && width != 0
        && height != 0
}

fn surface_pixel_count(width: u32, height: u32) -> Option<usize> {
    let width = usize::try_from(width).ok()?;
    let height = usize::try_from(height).ok()?;
    width.checked_mul(height)
}

fn copy_buffer_prefix(
    buffers: &mut [WaylandShmBuffer],
    source_index: usize,
    target_index: usize,
    pixel_count: usize,
) -> bool {
    if source_index >= buffers.len() || target_index >= buffers.len() {
        return false;
    }
    if source_index == target_index {
        return false;
    }
    if source_index < target_index {
        let (left, right) = buffers.split_at_mut(target_index);
        return copy_buffer_pair(&left[source_index], &mut right[0], pixel_count);
    }
    let (left, right) = buffers.split_at_mut(source_index);
    copy_buffer_pair(&right[0], &mut left[target_index], pixel_count)
}

fn copy_buffer_pair(
    source: &WaylandShmBuffer,
    target: &mut WaylandShmBuffer,
    pixel_count: usize,
) -> bool {
    if source.mapped_word_len() < pixel_count || target.mapped_word_len() < pixel_count {
        return false;
    }
    // SAFETY: source and target lengths are checked against pixel_count above.
    let source_pixels = unsafe { source.mapped_prefix(pixel_count) };
    // SAFETY: source and target lengths are checked against pixel_count above.
    let target_pixels = unsafe { target.mapped_prefix_mut(pixel_count) };
    target_pixels.copy_from_slice(source_pixels);
    true
}

fn phase_timer_start(enabled: bool) -> Option<Instant> {
    enabled.then(Instant::now)
}

fn record_phase(slot: &mut Duration, phase_start: &mut Option<Instant>) {
    if let Some(start) = phase_start.replace(Instant::now()) {
        *slot = start.elapsed();
    }
}

impl Dispatch<wl_registry::WlRegistry, GlobalListContents> for WaylandShmState {
    fn event(
        _state: &mut Self,
        _proxy: &wl_registry::WlRegistry,
        _event: wl_registry::Event,
        _data: &GlobalListContents,
        _conn: &Connection,
        _qh: &QueueHandle<Self>,
    ) {
    }
}

impl Dispatch<wl_shm::WlShm, ()> for WaylandShmState {
    fn event(
        _state: &mut Self,
        _proxy: &wl_shm::WlShm,
        _event: wl_shm::Event,
        _data: &(),
        _conn: &Connection,
        _qh: &QueueHandle<Self>,
    ) {
    }
}

impl Dispatch<wl_shm_pool::WlShmPool, ()> for WaylandShmState {
    fn event(
        _state: &mut Self,
        _proxy: &wl_shm_pool::WlShmPool,
        _event: wl_shm_pool::Event,
        _data: &(),
        _conn: &Connection,
        _qh: &QueueHandle<Self>,
    ) {
    }
}

impl Dispatch<wl_buffer::WlBuffer, Arc<AtomicBool>> for WaylandShmState {
    fn event(
        _state: &mut Self,
        _proxy: &wl_buffer::WlBuffer,
        event: wl_buffer::Event,
        released: &Arc<AtomicBool>,
        _conn: &Connection,
        _qh: &QueueHandle<Self>,
    ) {
        if let wl_buffer::Event::Release = event {
            released.store(true, Ordering::Release);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{WAYLAND_SHM_BUFFER_COUNT, buffers_cover_size};

    #[test]
    fn same_width_smaller_height_reuses_buffers() {
        assert!(buffers_cover_size(
            1280,
            320,
            WAYLAND_SHM_BUFFER_COUNT,
            1280,
            319
        ));
    }

    #[test]
    fn changed_width_recreates_buffers() {
        assert!(!buffers_cover_size(
            1280,
            320,
            WAYLAND_SHM_BUFFER_COUNT,
            1279,
            320
        ));
    }

    #[test]
    fn larger_height_recreates_buffers() {
        assert!(!buffers_cover_size(
            1280,
            320,
            WAYLAND_SHM_BUFFER_COUNT,
            1280,
            321
        ));
    }
}
