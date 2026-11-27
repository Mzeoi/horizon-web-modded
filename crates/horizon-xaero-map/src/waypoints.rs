#![forbid(unsafe_code)]

use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::Path;

/// Exact port of `xaero.hud.minimap.waypoint.WaypointColor` (21 colors).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[repr(u8)]
pub enum WaypointColor {
    Black = 0,
    DarkBlue = 1,
    DarkGreen = 2,
    DarkAqua = 3,
    DarkRed = 4,
    DarkPurple = 5,
    Gold = 6,
    Gray = 7,
    DarkGray = 8,
    Blue = 9,
    Green = 10,
    Aqua = 11,
    Red = 12,
    Purple = 13,
    Yellow = 14,
    White = 15,
    Magenta = 16,
    LightBlue = 17,
    Lime = 18,
    Pink = 19,
    Brown = 20,
}

impl WaypointColor {
    pub const ALL: [Self; 21] = [
        Self::Black,
        Self::DarkBlue,
        Self::DarkGreen,
        Self::DarkAqua,
        Self::DarkRed,
        Self::DarkPurple,
        Self::Gold,
        Self::Gray,
        Self::DarkGray,
        Self::Blue,
        Self::Green,
        Self::Aqua,
        Self::Red,
        Self::Purple,
        Self::Yellow,
        Self::White,
        Self::Magenta,
        Self::LightBlue,
        Self::Lime,
        Self::Pink,
        Self::Brown,
    ];

    pub fn from_index(idx: usize) -> Self {
        Self::ALL.get(idx).copied().unwrap_or(Self::White)
    }

    pub fn next(self) -> Self {
        Self::from_index((self as usize + 1) % Self::ALL.len())
    }

    pub fn argb(self) -> u32 {
        match self {
            Self::Black => 0xFF00_0000,
            Self::DarkBlue => 0xFF00_00AA,
            Self::DarkGreen => 0xFF00_AA00,
            Self::DarkAqua => 0xFF00_AAAA,
            Self::DarkRed => 0xFFAA_0000,
            Self::DarkPurple => 0xFFAA_00AA,
            Self::Gold => 0xFFFF_AA00,
            Self::Gray => 0xFFAA_AAAA,
            Self::DarkGray => 0xFF55_5555,
            Self::Blue => 0xFF55_55FF,
            Self::Green => 0xFF55_FF55,
            Self::Aqua => 0xFF55_FFFF,
            Self::Red => 0xFFFF_0000,
            Self::Purple => 0xFFFF_55FF,
            Self::Yellow => 0xFFFF_FF55,
            Self::White => 0xFFFF_FFFF,
            Self::Magenta => 0xFFFF_00FF,
            Self::LightBlue => 0xFF55_AAFF,
            Self::Lime => 0xFF80_FF00,
            Self::Pink => 0xFFFF_88AA,
            Self::Brown => 0xFF8B_4513,
        }
    }

    pub fn display_name(self) -> &'static str {
        match self {
            Self::Black => "Black",
            Self::DarkBlue => "Dark Blue",
            Self::DarkGreen => "Dark Green",
            Self::DarkAqua => "Dark Aqua",
            Self::DarkRed => "Dark Red",
            Self::DarkPurple => "Dark Purple",
            Self::Gold => "Gold",
            Self::Gray => "Gray",
            Self::DarkGray => "Dark Gray",
            Self::Blue => "Blue",
            Self::Green => "Green",
            Self::Aqua => "Aqua",
            Self::Red => "Red",
            Self::Purple => "Purple",
            Self::Yellow => "Yellow",
            Self::White => "White",
            Self::Magenta => "Magenta",
            Self::LightBlue => "Light Blue",
            Self::Lime => "Lime",
            Self::Pink => "Pink",
            Self::Brown => "Brown",
        }
    }
}

/// Exact port of `xaero.hud.minimap.waypoint.WaypointPurpose`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[repr(u8)]
pub enum WaypointPurpose {
    Normal = 0,
    Death = 1,
    OldDeath = 2,
    Destination = 3,
}

impl WaypointPurpose {
    pub fn from_index(idx: usize) -> Self {
        match idx {
            1 => Self::Death,
            2 => Self::OldDeath,
            3 => Self::Destination,
            _ => Self::Normal,
        }
    }

    pub fn is_death(self) -> bool {
        matches!(self, Self::Death | Self::OldDeath)
    }
}

/// Exact port of `xaero.hud.minimap.waypoint.WaypointVisibilityType`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[repr(u8)]
pub enum WaypointVisibilityType {
    Local = 0,
    Global = 1,
    WorldMapLocal = 2,
    WorldMapGlobal = 3,
}

impl WaypointVisibilityType {
    pub fn from_index(idx: usize) -> Self {
        match idx {
            1 => Self::Global,
            2 => Self::WorldMapLocal,
            3 => Self::WorldMapGlobal,
            _ => Self::Local,
        }
    }
}

/// Exact port of `xaero.common.minimap.waypoints.Waypoint`.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Waypoint {
    pub name: String,
    pub initials: String,
    pub x: i32,
    pub y: i32,
    pub z: i32,
    pub color: WaypointColor,
    pub disabled: bool,
    pub purpose: WaypointPurpose,
    pub set_name: String,
    pub rotate_on_tp: bool,
    pub tp_yaw: i32,
    pub visibility: WaypointVisibilityType,
    pub destination: bool,
}

impl Waypoint {
    pub fn new(name: impl Into<String>, x: i32, y: i32, z: i32, color: WaypointColor) -> Self {
        let name = name.into();
        let initials = default_initials(&name);
        Self {
            name,
            initials,
            x,
            y,
            z,
            color,
            disabled: false,
            purpose: WaypointPurpose::Normal,
            set_name: "gui.xaero_default".to_string(),
            rotate_on_tp: false,
            tp_yaw: 0,
            visibility: WaypointVisibilityType::Local,
            destination: false,
        }
    }

    pub fn deathpoint(x: i32, y: i32, z: i32) -> Self {
        Self {
            name: "Death".to_string(),
            initials: "X".to_string(),
            x,
            y,
            z,
            color: WaypointColor::Red,
            disabled: false,
            purpose: WaypointPurpose::Death,
            set_name: "gui.xaero_default".to_string(),
            rotate_on_tp: false,
            tp_yaw: 0,
            visibility: WaypointVisibilityType::Local,
            destination: false,
        }
    }

    /// Serialize in exact Xaero `WaypointIO` `.txt` format:
    /// `waypoint:name:initials:x:y:z:color:disabled:type:set:rotate_on_tp:tp_yaw:visibility_type:destination`
    pub fn to_xaero_line(&self) -> String {
        format!(
            "waypoint:{}:{}:{}:{}:{}:{}:{}:{}:{}:{}:{}:{}:{}",
            escape_colons(&self.name),
            escape_colons(&self.initials),
            self.x,
            self.y,
            self.z,
            self.color as u8,
            self.disabled,
            self.purpose as u8,
            escape_colons(&self.set_name),
            self.rotate_on_tp,
            self.tp_yaw,
            self.visibility as u8,
            self.destination,
        )
    }

    /// Parse from Xaero `WaypointIO` `.txt` line (`waypoint:...` or `#waypoint:...`).
    pub fn from_xaero_line(line: &str) -> Option<Self> {
        let body = line
            .strip_prefix("#waypoint:")
            .or_else(|| line.strip_prefix("waypoint:"))?;
        let parts: Vec<&str> = body.split(':').collect();
        if parts.len() < 7 {
            return None;
        }
        let name = unescape_colons(parts[0]);
        let initials = unescape_colons(parts[1]);
        let x = parts[2].parse::<i32>().ok()?;
        let y = parts[3].parse::<i32>().ok()?;
        let z = parts[4].parse::<i32>().ok()?;
        let color = WaypointColor::from_index(parts[5].parse::<usize>().unwrap_or(15));
        let disabled = parts[6] == "true";
        let purpose = parts
            .get(7)
            .and_then(|s| s.parse::<usize>().ok())
            .map(WaypointPurpose::from_index)
            .unwrap_or(WaypointPurpose::Normal);
        let set_name = parts
            .get(8)
            .map(|s| unescape_colons(s))
            .unwrap_or_else(|| "gui.xaero_default".to_string());
        let rotate_on_tp = parts.get(9).is_some_and(|s| *s == "true");
        let tp_yaw = parts.get(10).and_then(|s| s.parse().ok()).unwrap_or(0);
        let visibility = parts
            .get(11)
            .and_then(|s| s.parse::<usize>().ok())
            .map(WaypointVisibilityType::from_index)
            .unwrap_or(WaypointVisibilityType::Local);
        let destination = parts.get(12).is_some_and(|s| *s == "true");
        Some(Self {
            name,
            initials,
            x,
            y,
            z,
            color,
            disabled,
            purpose,
            set_name,
            rotate_on_tp,
            tp_yaw,
            visibility,
            destination,
        })
    }
}

fn default_initials(name: &str) -> String {
    let trimmed = name.trim();
    if trimmed.is_empty() {
        return "W".to_string();
    }
    trimmed.chars().take(2).collect()
}

fn escape_colons(s: &str) -> String {
    s.replace(':', "^col^")
}

fn unescape_colons(s: &str) -> String {
    s.replace("^col^", ":")
}

pub fn sanitize_id(raw: &str) -> String {
    raw.chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '-' || c == '_' || c == '.' {
                c
            } else {
                '_'
            }
        })
        .collect()
}

#[derive(Clone, Debug, Default)]
pub struct WaypointStore {
    pub active_world: String,
    pub active_dim: String,
    /// Map from `(world_id, dim_id)` to list of waypoints.
    pub by_dim: HashMap<(String, String), Vec<Waypoint>>,
    pub active_set: String,
}

impl WaypointStore {
    pub fn new() -> Self {
        Self {
            active_world: String::new(),
            active_dim: "minecraft:overworld".to_string(),
            by_dim: HashMap::new(),
            active_set: "gui.xaero_default".to_string(),
        }
    }

    pub fn current_list(&self) -> &[Waypoint] {
        self.by_dim
            .get(&(self.active_world.clone(), self.active_dim.clone()))
            .map(Vec::as_slice)
            .unwrap_or(&[])
    }

    pub fn current_list_mut(&mut self) -> &mut Vec<Waypoint> {
        self.by_dim
            .entry((self.active_world.clone(), self.active_dim.clone()))
            .or_default()
    }

    pub fn load_for(&mut self, config_dir: &Path, world_id: &str, dim: &str) {
        self.active_world = world_id.to_string();
        self.active_dim = dim.to_string();
        let key = (self.active_world.clone(), self.active_dim.clone());
        if self.by_dim.contains_key(&key) {
            return;
        }
        let path = Self::file_path(config_dir, world_id, dim);
        let mut loaded = Vec::new();
        if let Ok(text) = horizon_os::fs::read_to_string(&path) {
            for line in text.lines() {
                if let Some(rest) = line.strip_prefix("sets:") {
                    if let Some(first) = rest.split(':').next()
                        && !first.is_empty()
                    {
                        self.active_set = first.to_string();
                    }
                } else if let Some(wp) = Waypoint::from_xaero_line(line) {
                    loaded.push(wp);
                }
            }
        }
        self.by_dim.insert(key, loaded);
    }

    pub fn save_current(&self, config_dir: &Path) {
        if self.active_world.is_empty() {
            return;
        }
        let path = Self::file_path(config_dir, &self.active_world, &self.active_dim);
        if let Some(parent) = path.parent() {
            let _ = horizon_os::fs::create_dir_all(parent);
        }
        let mut out = String::from(
            "#\n#waypoint:name:initials:x:y:z:color:disabled:type:set:rotate_on_tp:tp_yaw:visibility_type:destination\n#\n",
        );
        out.push_str(&format!("sets:{}:{}\n", self.active_set, self.active_set));
        for wp in self.current_list() {
            out.push_str(&wp.to_xaero_line());
            out.push('\n');
        }
        let _ = horizon_os::fs::write(&path, out);
    }

    pub fn add_waypoint(&mut self, config_dir: &Path, wp: Waypoint) {
        self.current_list_mut().push(wp);
        self.save_current(config_dir);
    }

    pub fn record_death(&mut self, config_dir: &Path, x: i32, y: i32, z: i32) {
        let list = self.current_list_mut();
        for wp in list.iter_mut() {
            if wp.purpose == WaypointPurpose::Death {
                wp.purpose = WaypointPurpose::OldDeath;
                wp.name = "Old Death".to_string();
            }
        }
        list.push(Waypoint::deathpoint(x, y, z));
        self.save_current(config_dir);
    }

    pub fn remove_at(&mut self, config_dir: &Path, idx: usize) {
        let list = self.current_list_mut();
        if idx < list.len() {
            list.remove(idx);
            self.save_current(config_dir);
        }
    }

    pub fn toggle_at(&mut self, config_dir: &Path, idx: usize) {
        let list = self.current_list_mut();
        if let Some(wp) = list.get_mut(idx) {
            wp.disabled = !wp.disabled;
            self.save_current(config_dir);
        }
    }

    pub fn update_at(&mut self, config_dir: &Path, idx: usize, wp: Waypoint) {
        let list = self.current_list_mut();
        if let Some(slot) = list.get_mut(idx) {
            *slot = wp;
            self.save_current(config_dir);
        }
    }

    fn file_path(config_dir: &Path, world_id: &str, dim: &str) -> std::path::PathBuf {
        config_dir
            .join("xaero")
            .join("waypoints")
            .join(sanitize_id(world_id))
            .join(format!("{}.txt", sanitize_id(dim)))
    }
}
