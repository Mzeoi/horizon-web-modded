use serde::{Deserialize, Serialize};
use std::path::Path;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum LayerMode {
    All,
    SingleLayer,
    AllBelow,
    AllAbove,
}

impl LayerMode {
    pub fn label(self) -> &'static str {
        match self {
            Self::All => "All",
            Self::SingleLayer => "Single Layer",
            Self::AllBelow => "All Below",
            Self::AllAbove => "All Above",
        }
    }

    pub fn next(self) -> Self {
        match self {
            Self::All => Self::SingleLayer,
            Self::SingleLayer => Self::AllBelow,
            Self::AllBelow => Self::AllAbove,
            Self::AllAbove => Self::All,
        }
    }

    pub fn matches_y(self, y: i32, layer_y: i32) -> bool {
        match self {
            Self::All => true,
            Self::SingleLayer => y == layer_y,
            Self::AllBelow => y <= layer_y,
            Self::AllAbove => y >= layer_y,
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(default)]
pub struct LitematicaConfig {
    pub render_enabled: bool,
    pub render_boxes: bool,
    pub render_blocks: bool,
    pub render_area_selection: bool,
    pub render_through_walls: bool,
    pub show_correct_blocks: bool,
    pub layer_mode: LayerMode,
    pub layer_y: i32,
    pub max_render_blocks: usize,
    pub show_hud: bool,
}

impl Default for LitematicaConfig {
    fn default() -> Self {
        Self {
            render_enabled: true,
            render_boxes: true,
            render_blocks: true,
            render_area_selection: true,
            render_through_walls: false,
            show_correct_blocks: false,
            layer_mode: LayerMode::All,
            layer_y: 64,
            max_render_blocks: 4000,
            show_hud: true,
        }
    }
}

impl LitematicaConfig {
    pub fn load(dir: &Path) -> Self {
        let path = dir.join("litematica").join("config.json");
        horizon_os::fs::read_to_string(&path)
            .ok()
            .and_then(|s| serde_json::from_str(&s).ok())
            .unwrap_or_default()
    }

    pub fn save(&self, dir: &Path) {
        let folder = dir.join("litematica");
        let _ = horizon_os::fs::create_dir_all(&folder);
        if let Ok(json) = serde_json::to_string_pretty(self) {
            let _ = horizon_os::fs::write(folder.join("config.json"), json);
        }
    }
}
