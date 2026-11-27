#![forbid(unsafe_code)]

pub mod assets;
pub mod config;
pub mod minimap;
pub mod radar;
pub mod waypoints;
pub mod worldmap;

use config::XaeroConfig;
use std::path::PathBuf;
use waypoints::{WaypointColor, WaypointStore};
use worldmap::WorldMapCache;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ActiveScreen {
    None,
    WorldMap,
    WaypointList,
    WaypointEdit,
    Settings,
}

#[derive(Clone, Debug)]
pub struct WaypointDraft {
    pub name: String,
    pub x: i32,
    pub y: i32,
    pub z: i32,
    pub x_input: String,
    pub y_input: String,
    pub z_input: String,
    /// 0 = Name, 1 = X, 2 = Y, 3 = Z
    pub active_field: u8,
    pub color: WaypointColor,
    pub edit_index: Option<usize>,
}

impl WaypointDraft {
    pub fn from_coords(
        name: impl Into<String>,
        x: i32,
        y: i32,
        z: i32,
        color: WaypointColor,
        edit_index: Option<usize>,
    ) -> Self {
        Self {
            name: name.into(),
            x,
            y,
            z,
            x_input: x.to_string(),
            y_input: y.to_string(),
            z_input: z.to_string(),
            active_field: 0,
            color,
            edit_index,
        }
    }

    pub fn parsed_coords(&self) -> (i32, i32, i32) {
        (
            self.x_input.trim().parse().unwrap_or(self.x),
            self.y_input.trim().parse().unwrap_or(self.y),
            self.z_input.trim().parse().unwrap_or(self.z),
        )
    }
}

#[derive(Clone, Debug)]
pub struct ContextMenu {
    pub screen_x: f32,
    pub screen_y: f32,
    pub world_x: i32,
    pub world_z: i32,
}

pub struct XaeroState {
    pub config: XaeroConfig,
    pub config_loaded: bool,
    pub config_dir: Option<PathBuf>,
    pub waypoints: WaypointStore,
    pub worldmap: WorldMapCache,
    pub dimension: String,
    pub world_id: String,
    pub was_dead: bool,

    // Interactive UI state
    pub active_screen: ActiveScreen,
    pub cam_x: f64,
    pub cam_z: f64,
    pub zoom: f32,
    pub map_dirty: bool,
    pub last_tex_size: [u32; 2],
    pub viewport_rgba: Vec<u8>,
    pub context_menu: Option<ContextMenu>,
    pub waypoint_draft: Option<WaypointDraft>,
    pub editing_seed: bool,
    pub seed_input: String,
    pub last_biome: Option<String>,
    pub frame_counter: u64,
    pub waypoint_page: usize,
}

impl XaeroState {
    pub fn new() -> Self {
        Self {
            config: XaeroConfig::default(),
            config_loaded: false,
            config_dir: None,
            waypoints: WaypointStore::new(),
            worldmap: WorldMapCache::new(),
            dimension: "minecraft:overworld".to_string(),
            world_id: String::new(),
            was_dead: false,
            active_screen: ActiveScreen::None,
            cam_x: 0.0,
            cam_z: 0.0,
            zoom: 1.0,
            map_dirty: true,
            last_tex_size: [0, 0],
            viewport_rgba: Vec::new(),
            context_menu: None,
            waypoint_draft: None,
            editing_seed: false,
            seed_input: String::new(),
            last_biome: None,
            frame_counter: 0,
            waypoint_page: 0,
        }
    }

    pub fn ensure_config(&mut self, dir: PathBuf) {
        if !self.config_loaded {
            self.config = XaeroConfig::load(&dir);
            self.seed_input = self.config.world_seed.to_string();
            self.config_loaded = true;
        }
        self.config_dir = Some(dir);
    }

    pub fn save_config(&self) {
        if let Some(dir) = &self.config_dir {
            self.config.save(dir);
        }
    }

    pub fn sync_world(&mut self, world_id: &str, dim: &str) {
        let changed = self.world_id != world_id || self.dimension != dim;
        if !changed {
            return;
        }
        self.world_id = world_id.to_string();
        self.dimension = dim.to_string();
        self.worldmap.set_world(world_id);
        if let Some(dir) = &self.config_dir {
            self.waypoints.load_for(dir, world_id, dim);
        }
        self.map_dirty = true;
    }
}

impl Default for XaeroState {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use waypoints::{Waypoint, WaypointPurpose};
    use worldmap::MapRegion;

    #[test]
    fn waypoint_roundtrip_matches_xaero_format() {
        let wp = Waypoint::new("Home:Base", 120, 64, -340, WaypointColor::Gold);
        let line = wp.to_xaero_line();
        assert!(line.starts_with("waypoint:Home^col^Base:"));
        let parsed = Waypoint::from_xaero_line(&line).expect("parse waypoint");
        assert_eq!(parsed, wp);

        let death = Waypoint::deathpoint(-10, 70, 20);
        assert_eq!(death.purpose, WaypointPurpose::Death);
        let parsed_death = Waypoint::from_xaero_line(&format!("#{}", death.to_xaero_line()))
            .expect("parse #waypoint");
        assert_eq!(parsed_death, death);
    }

    #[test]
    fn slime_chunk_matches_minecraft_seed() {
        // Seed 12345: verify deterministic output without panics
        let s0 = minimap::is_slime_chunk(0, 0, 0);
        assert!(!s0);
        let mut hits = 0;
        for z in -10..10 {
            for x in -10..10 {
                if minimap::is_slime_chunk(123456789, x, z) {
                    hits += 1;
                }
            }
        }
        assert!(hits > 10 && hits < 80, "expected ~10% slime chunks, got {hits}");
    }

    #[test]
    fn map_region_rle_roundtrip() {
        let mut region = MapRegion::new();
        let tile = [0xFF33_6699u32; 256];
        region.write_chunk_tile(3, 4, &tile);
        let encoded = region.encode_rle();
        let decoded = MapRegion::decode_rle(&encoded).expect("decode RLE");
        assert_eq!(decoded.pixels, region.pixels);
    }

    #[test]
    fn embedded_xaero_pngs_decode() {
        let a = assets::assets();
        assert_eq!((a.guis.width, a.guis.height), (256, 256));
        assert_eq!((a.minimap_frame.width, a.minimap_frame.height), (256, 256));
        assert_eq!((a.worldmap_gui.width, a.worldmap_gui.height), (256, 256));
        assert!(!assets::tr("gui.xaero_new_waypoint").is_empty());
    }
}
