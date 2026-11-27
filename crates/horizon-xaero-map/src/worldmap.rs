#![forbid(unsafe_code)]

use crate::config::XaeroConfig;
use crate::minimap::is_slime_chunk;
use crate::radar::RadarBlip;
use crate::waypoints::{Waypoint, sanitize_id};
use std::collections::HashMap;
use std::path::Path;

/// Xaero World Map region size: 32x32 chunks = 512x512 blocks (`MapRegion`).
pub const REGION_CHUNKS: i32 = 32;
pub const REGION_BLOCKS: i32 = 512;

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct RegionKey {
    pub dim: String,
    pub rx: i32,
    pub rz: i32,
}

/// Port of `xaero.map.region.MapRegion`: 512x512 ARGB block colors.
#[derive(Clone)]
pub struct MapRegion {
    pub pixels: Vec<u32>,
    pub dirty: bool,
}

impl MapRegion {
    pub fn new() -> Self {
        Self {
            pixels: vec![0; (REGION_BLOCKS * REGION_BLOCKS) as usize],
            dirty: false,
        }
    }

    pub fn write_chunk_tile(&mut self, local_cx: i32, local_cz: i32, colors: &[u32; 256]) {
        let bx0 = local_cx * 16;
        let bz0 = local_cz * 16;
        for z in 0..16 {
            let dst_row = ((bz0 + z) * REGION_BLOCKS + bx0) as usize;
            let src_row = (z * 16) as usize;
            for x in 0..16 {
                let c = colors[src_row + x];
                if c != 0 {
                    self.pixels[dst_row + x] = c;
                }
            }
        }
        self.dirty = true;
    }

    /// Encode with simple RLE (`XWM1` magic + `(u16 run_len, u32 argb)` pairs) for compact OPFS storage.
    pub fn encode_rle(&self) -> Vec<u8> {
        let mut out = Vec::with_capacity(8192);
        out.extend_from_slice(b"XWM1");
        let mut i = 0;
        while i < self.pixels.len() {
            let val = self.pixels[i];
            let mut run = 1u16;
            while i + (run as usize) < self.pixels.len()
                && run < u16::MAX
                && self.pixels[i + (run as usize)] == val
            {
                run += 1;
            }
            out.extend_from_slice(&run.to_le_bytes());
            out.extend_from_slice(&val.to_le_bytes());
            i += run as usize;
        }
        out
    }

    pub fn decode_rle(bytes: &[u8]) -> Option<Self> {
        if bytes.len() < 4 || &bytes[..4] != b"XWM1" {
            return None;
        }
        let total = (REGION_BLOCKS * REGION_BLOCKS) as usize;
        let mut pixels = Vec::with_capacity(total);
        let mut pos = 4;
        while pos + 6 <= bytes.len() && pixels.len() < total {
            let run = u16::from_le_bytes([bytes[pos], bytes[pos + 1]]) as usize;
            let val = u32::from_le_bytes([
                bytes[pos + 2],
                bytes[pos + 3],
                bytes[pos + 4],
                bytes[pos + 5],
            ]);
            let count = run.min(total - pixels.len());
            pixels.resize(pixels.len() + count, val);
            pos += 6;
        }
        if pixels.len() != total {
            pixels.resize(total, 0);
        }
        Some(Self {
            pixels,
            dirty: false,
        })
    }
}

impl Default for MapRegion {
    fn default() -> Self {
        Self::new()
    }
}

pub struct WorldMapCache {
    pub world_id: String,
    pub regions: HashMap<RegionKey, MapRegion>,
    /// Regions that were attempted to load from disk/OPFS so we don't re-read missing files.
    probed: HashMap<RegionKey, bool>,
}

impl WorldMapCache {
    pub fn new() -> Self {
        Self {
            world_id: String::new(),
            regions: HashMap::new(),
            probed: HashMap::new(),
        }
    }

    pub fn set_world(&mut self, world_id: &str) {
        if self.world_id != world_id {
            self.world_id = world_id.to_string();
            self.regions.clear();
            self.probed.clear();
        }
    }

    pub fn ingest_tile(&mut self, config_dir: &Path, dim: &str, chunk: [i32; 2], colors: &[u32; 256]) {
        let rx = chunk[0].div_euclid(REGION_CHUNKS);
        let rz = chunk[1].div_euclid(REGION_CHUNKS);
        let lcx = chunk[0].rem_euclid(REGION_CHUNKS);
        let lcz = chunk[1].rem_euclid(REGION_CHUNKS);
        let key = RegionKey {
            dim: dim.to_string(),
            rx,
            rz,
        };
        if !self.regions.contains_key(&key) {
            self.try_load_region(config_dir, &key);
        }
        let region = self.regions.entry(key).or_default();
        region.write_chunk_tile(lcx, lcz, colors);
    }

    pub fn try_load_region(&mut self, config_dir: &Path, key: &RegionKey) {
        if self.world_id.is_empty() || self.probed.contains_key(key) {
            return;
        }
        self.probed.insert(key.clone(), true);
        let path = Self::region_path(config_dir, &self.world_id, key);
        if let Ok(bytes) = horizon_os::fs::read(&path)
            && let Some(region) = MapRegion::decode_rle(&bytes)
        {
            self.regions.insert(key.clone(), region);
        }
    }

    pub fn flush_dirty(&mut self, config_dir: &Path) {
        if self.world_id.is_empty() {
            return;
        }
        for (key, region) in &mut self.regions {
            if region.dirty {
                let path = Self::region_path(config_dir, &self.world_id, key);
                if let Some(parent) = path.parent() {
                    let _ = horizon_os::fs::create_dir_all(parent);
                }
                let _ = horizon_os::fs::write(&path, region.encode_rle());
                region.dirty = false;
            }
        }
    }

    fn region_path(config_dir: &Path, world_id: &str, key: &RegionKey) -> std::path::PathBuf {
        config_dir
            .join("xaero")
            .join("worldmap")
            .join(sanitize_id(world_id))
            .join(sanitize_id(&key.dim))
            .join(format!("r.{}.{}.xwm", key.rx, key.rz))
    }

    /// Sample a world block `(wx, wz)` in `dim` -> ARGB or `0`.
    pub fn sample_block(&self, dim: &str, wx: i32, wz: i32) -> u32 {
        let rx = wx.div_euclid(REGION_BLOCKS);
        let rz = wz.div_euclid(REGION_BLOCKS);
        let lx = wx.rem_euclid(REGION_BLOCKS) as usize;
        let lz = wz.rem_euclid(REGION_BLOCKS) as usize;
        let key = RegionKey {
            dim: dim.to_string(),
            rx,
            rz,
        };
        self.regions
            .get(&key)
            .map(|r| r.pixels[lz * REGION_BLOCKS as usize + lx])
            .unwrap_or(0)
    }

    /// Render the World Map viewport into an RGBA8 buffer (`tex_w x tex_h`, max 1024x1024).
    pub fn render_viewport(
        &mut self,
        config_dir: &Path,
        dim: &str,
        cam_x: f64,
        cam_z: f64,
        zoom: f32,
        tex_w: u32,
        tex_h: u32,
        cfg: &XaeroConfig,
        player_pos: [f64; 3],
        _waypoints: &[Waypoint],
        _blips: &[RadarBlip],
        out_rgba: &mut Vec<u8>,
    ) {
        let w = tex_w.clamp(64, 1024);
        let h = tex_h.clamp(64, 1024);
        out_rgba.resize((w * h * 4) as usize, 0);

        let inv_zoom = 1.0 / zoom.max(0.125) as f64;
        let half_w = w as f64 * 0.5;
        let half_h = h as f64 * 0.5;

        // Pre-probe visible regions
        let min_wx = (cam_x - half_w * inv_zoom).floor() as i32;
        let max_wx = (cam_x + half_w * inv_zoom).ceil() as i32;
        let min_wz = (cam_z - half_h * inv_zoom).floor() as i32;
        let max_wz = (cam_z + half_h * inv_zoom).ceil() as i32;
        for rz in min_wz.div_euclid(REGION_BLOCKS)..=max_wz.div_euclid(REGION_BLOCKS) {
            for rx in min_wx.div_euclid(REGION_BLOCKS)..=max_wx.div_euclid(REGION_BLOCKS) {
                let key = RegionKey {
                    dim: dim.to_string(),
                    rx,
                    rz,
                };
                self.try_load_region(config_dir, &key);
            }
        }

        for py in 0..h {
            let wz = (cam_z + (py as f64 - half_h) * inv_zoom).floor() as i32;
            let cz = wz.div_euclid(16);
            let grid_z = cfg.wm_show_grid && wz.rem_euclid(16) == 0;
            for px in 0..w {
                let wx = (cam_x + (px as f64 - half_w) * inv_zoom).floor() as i32;
                let cx = wx.div_euclid(16);
                let grid_x = cfg.wm_show_grid && wx.rem_euclid(16) == 0;

                let argb = self.sample_block(dim, wx, wz);
                let [mut r, mut g, mut b] = if argb != 0 {
                    let [_, rr, gg, bb] = argb.to_be_bytes();
                    [rr, gg, bb]
                } else {
                    // Xaero's dark unexplored checkerboard
                    let checker = ((wx >> 4) ^ (wz >> 4)) & 1 == 0;
                    if checker { [18, 20, 24] } else { [24, 26, 31] }
                };

                if cfg.wm_show_slime_chunks
                    && dim == "minecraft:overworld"
                    && is_slime_chunk(cfg.world_seed, cx, cz)
                {
                    g = g.saturating_add(45);
                }

                if grid_x || grid_z {
                    r = r.saturating_add(28);
                    g = g.saturating_add(28);
                    b = b.saturating_add(28);
                }

                let idx = ((py * w + px) * 4) as usize;
                out_rgba[idx] = r;
                out_rgba[idx + 1] = g;
                out_rgba[idx + 2] = b;
                out_rgba[idx + 3] = 255;
            }
        }

        // Draw small white dot at player's world position
        let ppx = ((player_pos[0] - cam_x) * zoom as f64 + half_w).round() as i32;
        let ppy = ((player_pos[2] - cam_z) * zoom as f64 + half_h).round() as i32;
        for dy in -3..=3 {
            for dx in -3..=3 {
                if dx * dx + dy * dy <= 9 {
                    let tx = ppx + dx;
                    let ty = ppy + dy;
                    if tx >= 0 && tx < w as i32 && ty >= 0 && ty < h as i32 {
                        let idx = ((ty as u32 * w + tx as u32) * 4) as usize;
                        let is_border = dx * dx + dy * dy >= 5;
                        out_rgba[idx] = if is_border { 20 } else { 255 };
                        out_rgba[idx + 1] = if is_border { 20 } else { 255 };
                        out_rgba[idx + 2] = if is_border { 20 } else { 255 };
                        out_rgba[idx + 3] = 255;
                    }
                }
            }
        }
    }
}

impl Default for WorldMapCache {
    fn default() -> Self {
        Self::new()
    }
}
