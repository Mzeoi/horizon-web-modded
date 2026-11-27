#![forbid(unsafe_code)]

use serde::{Deserialize, Serialize};
use std::path::Path;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum MapCorner {
    TopLeft,
    TopRight,
    BottomLeft,
    BottomRight,
}

impl MapCorner {
    pub fn next(self) -> Self {
        match self {
            Self::TopLeft => Self::TopRight,
            Self::TopRight => Self::BottomRight,
            Self::BottomRight => Self::BottomLeft,
            Self::BottomLeft => Self::TopLeft,
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::TopLeft => "Top-Left",
            Self::TopRight => "Top-Right",
            Self::BottomLeft => "Bottom-Left",
            Self::BottomRight => "Bottom-Right",
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(default)]
pub struct XaeroConfig {
    pub minimap_enabled: bool,
    pub corner: MapCorner,
    pub minimap_size: f32,
    pub zoom_index: usize,
    pub lock_north: bool,
    pub round_minimap: bool,
    pub show_coordinates: bool,
    pub show_biome: bool,
    pub show_slime_chunks: bool,
    pub show_chunk_grid: bool,
    pub cave_mode: bool,
    pub entity_radar: bool,
    pub show_players: bool,
    pub show_hostile: bool,
    pub show_passive: bool,
    pub show_items: bool,
    pub show_waypoints_on_map: bool,
    pub show_waypoints_in_world: bool,
    pub auto_deathpoint: bool,
    pub waypoint_beacon_distance: f32,
    pub world_seed: i64,
    pub wm_show_waypoints: bool,
    pub wm_show_radar: bool,
    pub wm_show_grid: bool,
    pub wm_show_slime_chunks: bool,
}

pub const ZOOM_LEVELS: [f32; 5] = [0.5, 0.75, 1.0, 1.5, 2.0];

impl Default for XaeroConfig {
    fn default() -> Self {
        Self {
            minimap_enabled: true,
            corner: MapCorner::TopLeft,
            minimap_size: 120.0,
            zoom_index: 2, // 1.0 block per pixel
            lock_north: false,
            round_minimap: false,
            show_coordinates: true,
            show_biome: true,
            show_slime_chunks: false,
            show_chunk_grid: false,
            cave_mode: true,
            entity_radar: true,
            show_players: true,
            show_hostile: true,
            show_passive: true,
            show_items: true,
            show_waypoints_on_map: true,
            show_waypoints_in_world: true,
            auto_deathpoint: true,
            waypoint_beacon_distance: 2048.0,
            world_seed: 0,
            wm_show_waypoints: true,
            wm_show_radar: true,
            wm_show_grid: false,
            wm_show_slime_chunks: false,
        }
    }
}

impl XaeroConfig {
    pub fn blocks_per_pixel(&self) -> f32 {
        ZOOM_LEVELS
            .get(self.zoom_index)
            .copied()
            .unwrap_or(1.0)
    }

    pub fn cycle_zoom(&mut self) {
        self.zoom_index = (self.zoom_index + 1) % ZOOM_LEVELS.len();
    }

    pub fn cycle_size(&mut self) {
        self.minimap_size = match self.minimap_size as u32 {
            0..=95 => 100.0,
            96..=119 => 120.0,
            120..=149 => 150.0,
            150..=179 => 180.0,
            _ => 84.0,
        };
    }

    pub fn load(config_dir: &Path) -> Self {
        let path = config_dir.join("xaero").join("xaerominimap.json");
        if let Ok(text) = horizon_os::fs::read_to_string(&path)
            && let Ok(cfg) = serde_json::from_str::<Self>(&text)
        {
            return cfg;
        }
        Self::default()
    }

    pub fn save(&self, config_dir: &Path) {
        let dir = config_dir.join("xaero");
        let _ = horizon_os::fs::create_dir_all(&dir);
        let path = dir.join("xaerominimap.json");
        if let Ok(json) = serde_json::to_string_pretty(self) {
            let _ = horizon_os::fs::write(&path, json);
        }
    }
}
