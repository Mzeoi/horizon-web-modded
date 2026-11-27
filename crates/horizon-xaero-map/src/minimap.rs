#![forbid(unsafe_code)]

use crate::config::XaeroConfig;
use crate::radar::RadarBlip;
use crate::waypoints::Waypoint;

/// Exact port of `xaero.common.minimap.region.MinimapTile.isSlimeChunk(long seed, int x, int z)`.
/// Uses Minecraft's `java.util.Random` LCG (`multiplier = 0x5DEECE66DL`, `addend = 0xBL`).
pub fn is_slime_chunk(seed: i64, chunk_x: i32, chunk_z: i32) -> bool {
    if seed == 0 {
        return false;
    }
    let cx = i64::from(chunk_x);
    let cz = i64::from(chunk_z);
    let s = seed
        .wrapping_add(cx.wrapping_mul(cx).wrapping_mul(0x4c_1906))
        .wrapping_add(cx.wrapping_mul(0x5a_c0db))
        .wrapping_add(cz.wrapping_mul(cz).wrapping_mul(0x43_07a7))
        .wrapping_add(cz.wrapping_mul(0x5f_24f))
        ^ 0x3ad8_025f;
    let mut state = (s ^ 0x5_DEEC_E66D) & ((1i64 << 48) - 1);
    loop {
        state = (state.wrapping_mul(0x5_DEEC_E66D).wrapping_add(0xB)) & ((1i64 << 48) - 1);
        let bits = ((state as u64) >> 17) as i32;
        let val = bits.rem_euclid(10);
        if bits - val + 9 >= 0 {
            return val == 0;
        }
    }
}

/// Project a world `(wx, wz)` position onto the minimap box `(mx, my, size)` in GUI pixels.
/// Matches Xaero's north-up and player-rotated projection, clamping out-of-range waypoints to rim.
pub fn project_onto_minimap(
    player_x: f64,
    player_z: f64,
    player_yaw_deg: f32,
    wx: f64,
    wz: f64,
    map_x: f32,
    map_y: f32,
    map_size: f32,
    blocks_per_pixel: f32,
    lock_north: bool,
    round: bool,
    clamp_to_edge: bool,
) -> Option<[f32; 2]> {
    let scale = 1.0 / blocks_per_pixel.max(0.1);
    let dx = (wx - player_x) as f32 * scale;
    let dz = (wz - player_z) as f32 * scale;

    let (sx, sy) = if lock_north {
        (dx, dz)
    } else {
        // Horizon minimap rotation: `180 + yaw` aligns player's facing direction with +up (-Y).
        let rad = (180.0 + player_yaw_deg).to_radians();
        let (s, c) = rad.sin_cos();
        (dx * c + dz * s, -dx * s + dz * c)
    };

    let half = map_size * 0.5 - 4.0;
    let cx = map_x + map_size * 0.5;
    let cy = map_y + map_size * 0.5;

    if round {
        let dist_sq = sx * sx + sy * sy;
        if dist_sq > half * half {
            if !clamp_to_edge {
                return None;
            }
            let dist = dist_sq.sqrt().max(0.001);
            return Some([cx + sx / dist * half, cy + sy / dist * half]);
        }
    } else if sx.abs() > half || sy.abs() > half {
        if !clamp_to_edge {
            return None;
        }
        let max_axis = sx.abs().max(sy.abs()).max(0.001);
        let k = half / max_axis;
        return Some([cx + sx * k, cy + sy * k]);
    }

    Some([cx + sx, cy + sy])
}

#[derive(Clone, Debug)]
pub struct MinimapHudData {
    pub map_rect: [f32; 4],
    pub info_lines: Vec<(String, u32)>,
    pub compass: Vec<(&'static str, f32, f32, u32)>,
    pub radar_dots: Vec<(f32, f32, u32)>,
    pub waypoint_markers: Vec<(f32, f32, u32, String)>,
    pub chunk_is_slime: bool,
}

impl MinimapHudData {
    pub fn build(
        cfg: &XaeroConfig,
        player_pos: [f64; 3],
        yaw: f32,
        biome_name: Option<&str>,
        dimension: &str,
        waypoints: &[Waypoint],
        blips: &[RadarBlip],
        map_rect: [f32; 4],
    ) -> Self {
        let [mx, my, ms, _] = map_rect;
        let bpp = cfg.blocks_per_pixel();
        let (bx, by, bz) = (
            player_pos[0].floor() as i32,
            player_pos[1].floor() as i32,
            player_pos[2].floor() as i32,
        );
        let chunk_is_slime = cfg.show_slime_chunks
            && dimension == "minecraft:overworld"
            && is_slime_chunk(cfg.world_seed, bx >> 4, bz >> 4);

        // Compass N/E/S/W around the minimap rim
        let mut compass = Vec::with_capacity(4);
        let cx = mx + ms * 0.5;
        let cy = my + ms * 0.5;
        let reach = ms * 0.5 - 5.0;
        let dirs: [(&str, f32, f32, u32); 4] = [
            ("N", 0.0, -1.0, 0xFFFF_5555),
            ("E", 1.0, 0.0, 0xFFFF_FFFF),
            ("S", 0.0, 1.0, 0xFFFF_FFFF),
            ("W", -1.0, 0.0, 0xFFFF_FFFF),
        ];
        for (lbl, dx, dz, col) in dirs {
            let (rx, ry) = if cfg.lock_north {
                (dx, dz)
            } else {
                let rad = (180.0 + yaw).to_radians();
                let (s, c) = rad.sin_cos();
                (dx * c + dz * s, -dx * s + dz * c)
            };
            compass.push((lbl, cx + rx * reach, cy + ry * reach, col));
        }

        // Entity radar dots
        let mut radar_dots = Vec::new();
        if cfg.entity_radar {
            for blip in blips {
                if let Some([px, py]) = project_onto_minimap(
                    player_pos[0],
                    player_pos[2],
                    yaw,
                    blip.x,
                    blip.z,
                    mx,
                    my,
                    ms,
                    bpp,
                    cfg.lock_north,
                    cfg.round_minimap,
                    false,
                ) {
                    radar_dots.push((px, py, blip.category.argb()));
                }
            }
        }

        // Waypoint markers
        let mut waypoint_markers = Vec::new();
        if cfg.show_waypoints_on_map {
            for wp in waypoints {
                if wp.disabled {
                    continue;
                }
                if let Some([px, py]) = project_onto_minimap(
                    player_pos[0],
                    player_pos[2],
                    yaw,
                    wp.x as f64 + 0.5,
                    wp.z as f64 + 0.5,
                    mx,
                    my,
                    ms,
                    bpp,
                    cfg.lock_north,
                    cfg.round_minimap,
                    true,
                ) {
                    waypoint_markers.push((px, py, wp.color.argb(), wp.initials.clone()));
                }
            }
        }

        // Info lines below the minimap (Coordinates, Biome, Slime Chunk)
        let mut info_lines = Vec::new();
        if cfg.show_coordinates {
            info_lines.push((format!("{bx}, {by}, {bz}"), 0xFFFF_FFFF));
        }
        if cfg.show_biome
            && let Some(b) = biome_name
        {
            let short = b.strip_prefix("minecraft:").unwrap_or(b);
            info_lines.push((short.to_string(), 0xFFAA_AAAA));
        }
        if chunk_is_slime {
            info_lines.push(("Slime Chunk".to_string(), 0xFF55_FF55));
        }

        Self {
            map_rect,
            info_lines,
            compass,
            radar_dots,
            waypoint_markers,
            chunk_is_slime,
        }
    }
}
