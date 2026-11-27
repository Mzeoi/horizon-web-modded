#![forbid(unsafe_code)]

use crate::mod_api::{
    ClientLevelLoadEvent, Corner, Flow, FrameEvent, Hooks, Host, HudEvent, KeyEvent,
    MapTileEvent, MinimapEvent, ModsEvent, NativeMinimap, NativeMod, NativeScreenKind,
    NativeWorldId, PrimitiveShape, ScreenCharTypedPreEvent, ScreenKeyPressedPreEvent,
    ScreenMouseButtonPressedPreEvent, ScreenMouseDraggedPreEvent, ScreenMouseScrolledPreEvent,
    ScreenRenderPostEvent, SettingsClickEvent, SettingsEvent, WorldPrimitivesEvent,
};
use horizon_os::sync::lock;
use horizon_xaero_map::{
    ActiveScreen, ContextMenu, WaypointDraft, XaeroState,
    config::MapCorner,
    minimap::MinimapHudData,
    radar::{RadarBlip, classify_entity},
    waypoints::{Waypoint, WaypointColor},
};
use std::sync::{Arc, Mutex};

pub const MOD_ID: &str = "xaerominimap";
pub const WORLDMAP_MOD_ID: &str = "xaeroworldmap";

#[derive(Clone, Debug)]
enum PendingAction {
    OpenScreen(ActiveScreen),
    CloseScreen,
    SendChat(String),
}

pub struct XaeroMod {
    enable_minimap: bool,
    enable_worldmap: bool,
    state: Mutex<XaeroState>,
    pending: Mutex<Vec<PendingAction>>,
    deferred_open: Mutex<Option<ActiveScreen>>,
    waypoint_return: Mutex<ActiveScreen>,
}

impl XaeroMod {
    pub fn new(enable_minimap: bool, enable_worldmap: bool) -> Self {
        Self {
            enable_minimap,
            enable_worldmap,
            state: Mutex::new(XaeroState::new()),
            pending: Mutex::new(Vec::new()),
            deferred_open: Mutex::new(None),
            waypoint_return: Mutex::new(ActiveScreen::None),
        }
    }

    pub fn mods_list(enable_minimap: bool, enable_worldmap: bool) -> Arc<[Box<dyn Hooks>]> {
        if !enable_minimap && !enable_worldmap {
            return Arc::from(Vec::new());
        }
        Arc::from(vec![Box::new(Self::new(enable_minimap, enable_worldmap)) as Box<dyn Hooks>])
    }

    fn queue(&self, action: PendingAction) {
        if let Ok(mut q) = lock(&self.pending) {
            q.push(action);
        }
    }
}

impl Default for XaeroMod {
    fn default() -> Self {
        Self::new(true, true)
    }
}

fn to_horizon_corner(c: MapCorner) -> Corner {
    match c {
        MapCorner::TopLeft => Corner::TopLeft,
        MapCorner::TopRight => Corner::TopRight,
        MapCorner::BottomLeft => Corner::BottomLeft,
        MapCorner::BottomRight => Corner::BottomRight,
    }
}

fn world_id_string(w: &NativeWorldId) -> String {
    match w {
        NativeWorldId::Singleplayer(s) => format!("sp_{s}"),
        NativeWorldId::Server(s) => format!("mp_{s}"),
    }
}

fn collect_radar_blips(host: &dyn Host, cfg: &horizon_xaero_map::config::XaeroConfig) -> Vec<RadarBlip> {
    if !cfg.entity_radar {
        return Vec::new();
    }
    let mut out = Vec::new();
    for e in host.client_entities() {
        if !e.alive || e.invisible || e.spectator {
            continue;
        }
        let cat = classify_entity(&e.kind, e.player_uuid.is_some());
        let allowed = match cat {
            horizon_xaero_map::radar::RadarCategory::Player => cfg.show_players,
            horizon_xaero_map::radar::RadarCategory::Hostile => cfg.show_hostile,
            horizon_xaero_map::radar::RadarCategory::Passive => cfg.show_passive,
            horizon_xaero_map::radar::RadarCategory::Item => cfg.show_items,
            horizon_xaero_map::radar::RadarCategory::Other => cfg.show_passive,
        };
        if !allowed {
            continue;
        }
        out.push(RadarBlip {
            x: e.position[0],
            y: e.position[1],
            z: e.position[2],
            yaw: e.yaw,
            label: e.player_name.clone(),
            category: cat,
        });
    }
    out
}

fn in_rect(mx: f32, my: f32, r: [f32; 4]) -> bool {
    mx >= r[0] && mx <= r[0] + r[2] && my >= r[1] && my <= r[1] + r[3]
}

fn draw_button(
    host: &dyn Host,
    batch: &mut crate::mod_api::HudBatch,
    rect: [f32; 4],
    label: &str,
    mouse: [f32; 2],
    bg: u32,
) {
    let hovered = in_rect(mouse[0], mouse[1], rect);
    let fill = if hovered { 0xE044_5066 } else { bg };
    host.fill(batch, rect, fill);
    // 1px border
    let border = if hovered { 0xFFAA_CCFF } else { 0xFF55_5E70 };
    host.fill(batch, [rect[0], rect[1], rect[2], 1.0], border);
    host.fill(batch, [rect[0], rect[1] + rect[3] - 1.0, rect[2], 1.0], border);
    host.fill(batch, [rect[0], rect[1], 1.0, rect[3]], border);
    host.fill(batch, [rect[0] + rect[2] - 1.0, rect[1], 1.0, rect[3]], border);

    let tw = host.text_width(label);
    let tx = rect[0] + (rect[2] - tw) * 0.5;
    let ty = rect[1] + (rect[3] - 8.0) * 0.5;
    host.text(batch, label, [tx.round(), ty.round()], 0xFFFF_FFFF, true);
}

impl Hooks for XaeroMod {
    fn mods(&self, _host: &mut dyn Host, event: &mut ModsEvent<'_>) {
        if self.enable_minimap {
            event.mods.push(NativeMod {
                id: MOD_ID.to_string(),
                title: "Xaero's Minimap 26.6.2".to_string(),
                description: "Port of Xaero's Minimap 26.6.2: minimap, entity radar, slime chunks & waypoints.".to_string(),
                keys: vec![
                    ["new_waypoint".into(), "New Waypoint".into(), "key.keyboard.b".into()],
                    ["waypoints_menu".into(), "Waypoints Menu".into(), "key.keyboard.u".into()],
                    ["zoom_minimap".into(), "Zoom Minimap".into(), "key.keyboard.z".into()],
                    ["toggle_minimap".into(), "Toggle Minimap".into(), "key.keyboard.y".into()],
                ],
            });
        }
        if self.enable_worldmap {
            event.mods.push(NativeMod {
                id: "xaeroworldmap".to_string(),
                title: "Xaero's World Map 1.47.2".to_string(),
                description: "Port of Xaero's World Map 1.47.2: fullscreen interactive map & region cache.".to_string(),
                keys: vec![
                    ["open_worldmap".into(), "Open World Map".into(), "key.keyboard.m".into()],
                ],
            });
        }
    }

    fn settings(&self, _host: &mut dyn Host, event: &mut SettingsEvent<'_>) {
        let Ok(st) = lock(&self.state) else { return };
        let on_off = |b: bool| if b { "ON" } else { "OFF" };
        if event.mod_id == MOD_ID {
            event.buttons.extend([
                format!("Minimap: {}", on_off(st.config.minimap_enabled)),
                format!("Corner: {}", st.config.corner.label()),
                format!("Orientation: {}", if st.config.lock_north { "North Locked" } else { "Rotated" }),
                format!("Shape: {}", if st.config.round_minimap { "Circle" } else { "Square" }),
                format!("Entity Radar: {}", on_off(st.config.entity_radar)),
                format!("Slime Chunks: {}", on_off(st.config.show_slime_chunks)),
                format!("3D Waypoint Beams: {}", on_off(st.config.show_waypoints_in_world)),
                "Open Xaero Settings...".to_string(),
            ]);
        } else if event.mod_id == WORLDMAP_MOD_ID {
            event.buttons.extend([
                format!("World Map Waypoints: {}", on_off(st.config.wm_show_waypoints)),
                format!("World Map Radar: {}", on_off(st.config.wm_show_radar)),
                format!("World Map Grid: {}", on_off(st.config.wm_show_grid)),
                format!("World Map Slime Chunks: {}", on_off(st.config.wm_show_slime_chunks)),
                "Open World Map...".to_string(),
                "Open Xaero Settings...".to_string(),
            ]);
        }
    }

    fn settings_click(&self, host: &mut dyn Host, event: &mut SettingsClickEvent<'_>) {
        if event.mod_id != MOD_ID && event.mod_id != WORLDMAP_MOD_ID {
            return;
        }
        let mut open_screen = None;
        if let Ok(mut st) = lock(&self.state) {
            if let Ok(dir) = host.config_dir() {
                st.ensure_config(dir);
            }
            if event.mod_id == MOD_ID {
                match event.button {
                    0 => st.config.minimap_enabled = !st.config.minimap_enabled,
                    1 => st.config.corner = st.config.corner.next(),
                    2 => st.config.lock_north = !st.config.lock_north,
                    3 => st.config.round_minimap = !st.config.round_minimap,
                    4 => st.config.entity_radar = !st.config.entity_radar,
                    5 => st.config.show_slime_chunks = !st.config.show_slime_chunks,
                    6 => st.config.show_waypoints_in_world = !st.config.show_waypoints_in_world,
                    7 => open_screen = Some(ActiveScreen::Settings),
                    _ => {}
                }
            } else {
                match event.button {
                    0 => st.config.wm_show_waypoints = !st.config.wm_show_waypoints,
                    1 => st.config.wm_show_radar = !st.config.wm_show_radar,
                    2 => {
                        st.config.wm_show_grid = !st.config.wm_show_grid;
                        st.map_dirty = true;
                    }
                    3 => {
                        st.config.wm_show_slime_chunks = !st.config.wm_show_slime_chunks;
                        st.map_dirty = true;
                    }
                    4 => open_screen = Some(ActiveScreen::WorldMap),
                    5 => open_screen = Some(ActiveScreen::Settings),
                    _ => {}
                }
            }
            st.save_config();
        }
        if let Some(scr) = open_screen {
            self.queue(PendingAction::OpenScreen(scr));
        }
    }

    fn client_level_load(&self, host: &mut dyn Host, event: &mut ClientLevelLoadEvent<'_>) {
        if let Ok(mut st) = lock(&self.state) {
            if let Ok(dir) = host.config_dir() {
                st.ensure_config(dir);
            }
            if let Some(world_id) = host
                .client_state()
                .and_then(|s| s.world.as_ref().map(world_id_string))
            {
                st.sync_world(&world_id, event.dimension);
            } else {
                st.dimension = event.dimension.to_string();
            }
        }
    }

    fn map_tile(&self, host: &dyn Host, event: &mut MapTileEvent<'_>) {
        if !self.enable_worldmap { return; }
        if let Ok(mut st) = lock(&self.state) {
            if let Ok(dir) = host.config_dir() {
                st.ensure_config(dir.clone());
                if let Some(world_id) = host
                    .client_state()
                    .and_then(|s| s.world.as_ref().map(world_id_string))
                {
                    st.sync_world(&world_id, event.dimension);
                }
                st.worldmap.ingest_tile(&dir, event.dimension, event.chunk, event.colors);
                if st.active_screen == ActiveScreen::WorldMap {
                    st.map_dirty = true;
                }
            }
        }
    }

    fn key(&self, host: &mut dyn Host, event: &mut KeyEvent<'_>) {
        if !event.pressed {
            return;
        }
        let Some(cs) = host.client_state() else { return };
        if cs.screen != NativeScreenKind::Playing {
            return;
        }
        let k_worldmap = host.mod_key(WORLDMAP_MOD_ID, "open_worldmap", "key.keyboard.m");
        let k_new_wp = host.mod_key(MOD_ID, "new_waypoint", "key.keyboard.b");
        let k_wp_list = host.mod_key(MOD_ID, "waypoints_menu", "key.keyboard.u");
        let k_zoom = host.mod_key(MOD_ID, "zoom_minimap", "key.keyboard.z");
        let k_toggle = host.mod_key(MOD_ID, "toggle_minimap", "key.keyboard.y");

        if self.enable_worldmap && event.key == k_worldmap {
            if let Ok(mut st) = lock(&self.state) {
                if let Some(eye) = cs.eye_position {
                    st.cam_x = eye[0];
                    st.cam_z = eye[2];
                }
                st.active_screen = ActiveScreen::WorldMap;
                st.map_dirty = true;
                st.context_menu = None;
            }
            host.mod_screen_open(MOD_ID);
        } else if self.enable_minimap && event.key == k_new_wp {
            if let Ok(mut ret) = lock(&self.waypoint_return) {
                *ret = ActiveScreen::None;
            }
            if let Some(eye) = cs.eye_position
                && let Ok(mut st) = lock(&self.state)
            {
                st.waypoint_draft = Some(WaypointDraft::from_coords("New Waypoint".to_string(), eye[0].floor() as i32, (eye[1] - 1.62).floor() as i32, eye[2].floor() as i32, WaypointColor::Aqua, None));
                st.active_screen = ActiveScreen::WaypointEdit;
            }
            host.mod_screen_open(MOD_ID);
        } else if self.enable_minimap && event.key == k_wp_list {
            if let Ok(mut st) = lock(&self.state) {
                st.active_screen = ActiveScreen::WaypointList;
            }
            host.mod_screen_open(MOD_ID);
        } else if self.enable_minimap && event.key == k_zoom {
            if let Ok(mut st) = lock(&self.state) {
                st.config.cycle_zoom();
                st.save_config();
            }
        } else if self.enable_minimap && event.key == k_toggle {
            if let Ok(mut st) = lock(&self.state) {
                st.config.minimap_enabled = !st.config.minimap_enabled;
                st.save_config();
            }
        }
    }

    fn minimap(&self, _host: &dyn Host, _event: &mut MinimapEvent) -> Option<NativeMinimap> {
        if !self.enable_minimap { return None; }
        let st = lock(&self.state).ok()?;
        if !st.config.minimap_enabled {
            return None;
        }
        Some(NativeMinimap {
            corner: to_horizon_corner(st.config.corner),
            size: st.config.minimap_size,
            blocks_per_pixel: st.config.blocks_per_pixel(),
            rotate: !st.config.lock_north,
            round: st.config.round_minimap,
        })
    }

    fn frame(&self, host: &mut dyn Host, _event: &mut FrameEvent) {
        // 1. Execute queued screen/chat actions that need `&mut dyn Host`
        let actions: Vec<PendingAction> = if let Ok(mut q) = lock(&self.pending) {
            q.drain(..).collect()
        } else {
            Vec::new()
        };
        for act in actions {
            match act {
                PendingAction::OpenScreen(scr) => {
                    if host.mod_screen_open(MOD_ID) {
                        if let Ok(mut st) = lock(&self.state) {
                            if scr == ActiveScreen::WorldMap
                                && let Some(eye) = host.client_state().and_then(|c| c.eye_position)
                            {
                                st.cam_x = eye[0];
                                st.cam_z = eye[2];
                            }
                            st.active_screen = scr;
                            st.map_dirty = true;
                            st.context_menu = None;
                        }
                    } else if host.client_state().is_some_and(|c| c.world.is_some())
                        && let Ok(mut d) = lock(&self.deferred_open)
                    {
                        *d = Some(scr);
                    }
                }
                PendingAction::CloseScreen => {
                    if let Ok(mut st) = lock(&self.state) {
                        st.active_screen = ActiveScreen::None;
                        st.context_menu = None;
                    }
                    host.mod_screen_close(MOD_ID);
                }
                PendingAction::SendChat(msg) => {
                    let _ = host.send_chat(&msg);
                }
            }
        }

        // 2. Sync world state, deathpoints, biome, and World Map viewport upload
        let Some(cs) = host.client_state() else { return };
        if cs.screen == NativeScreenKind::Playing
            && let Some(scr) = lock(&self.deferred_open).ok().and_then(|mut d| d.take())
            && host.mod_screen_open(MOD_ID)
        {
            if let Ok(mut st) = lock(&self.state) {
                if scr == ActiveScreen::WorldMap
                    && let Some(eye) = cs.eye_position
                {
                    st.cam_x = eye[0];
                    st.cam_z = eye[2];
                }
                st.active_screen = scr;
                st.map_dirty = true;
                st.context_menu = None;
            }
        }
        let Ok(mut st) = lock(&self.state) else { return };
        st.frame_counter = st.frame_counter.wrapping_add(1);

        if let Ok(dir) = host.config_dir() {
            st.ensure_config(dir);
        }

        if let Some(world) = &cs.world {
            let wid = world_id_string(world);
            if st.world_id != wid {
                let dim = st.dimension.clone();
                st.sync_world(&wid, &dim);
            }
        }

        // Deathpoint detection
        let dead_now = cs.screen == NativeScreenKind::Death;
        if dead_now && !st.was_dead && st.config.auto_deathpoint {
            if let Some(eye) = cs.eye_position
                && let Some(dir) = st.config_dir.clone()
            {
                st.waypoints.record_death(
                    &dir,
                    eye[0].floor() as i32,
                    (eye[1] - 1.62).floor() as i32,
                    eye[2].floor() as i32,
                );
            }
        }
        st.was_dead = dead_now;

        // Sample current biome every 15 frames
        if st.frame_counter % 15 == 0
            && let Some(eye) = cs.eye_position
            && let Some(biomes) = host.client_biomes()
        {
            let bx = eye[0].floor() as i32;
            let by = eye[1].floor() as i32;
            let bz = eye[2].floor() as i32;
            let c = [bx >> 4, bz >> 4];
            if let Ok(chunks) = host.client_chunks(c, c)
                && let Some(bid) = chunks.biome([bx, by, bz])
            {
                st.last_biome = Some(biomes.name(bid).to_string());
            }
        }

        // Flush dirty World Map regions to OPFS every ~300 frames
        if st.frame_counter % 300 == 0
            && let Some(dir) = st.config_dir.clone()
        {
            st.worldmap.flush_dirty(&dir);
        }

        // Sync ActiveScreen::None if user closed mod screen via Escape
        if st.active_screen != ActiveScreen::None && host.mod_screen() != Some(MOD_ID) {
            st.active_screen = ActiveScreen::None;
            st.context_menu = None;
        }

        // Re-render World Map texture into `horizon/modimage` when dirty
        if st.active_screen == ActiveScreen::WorldMap && st.map_dirty {
            let tex_w = (cs.gui_size[0].round() as u32).clamp(256, 1024);
            let tex_h = (cs.gui_size[1].round() as u32).clamp(256, 1024);
            let pos = cs.eye_position.unwrap_or([0.0, 64.0, 0.0]);
            let cfg = st.config.clone();
            let dim = st.dimension.clone();
            let cam_x = st.cam_x;
            let cam_z = st.cam_z;
            let zoom = st.zoom;
            let wps = st.waypoints.current_list().to_vec();
            let mut rgba: Vec<u8> = std::mem::take(&mut (*st).viewport_rgba);
            if let Some(dir) = st.config_dir.clone() {
                st.worldmap.render_viewport(
                    &dir,
                    &dim,
                    cam_x,
                    cam_z,
                    zoom,
                    tex_w,
                    tex_h,
                    &cfg,
                    pos,
                    &wps,
                    &[],
                    &mut rgba,
                );
            }
            if host.mod_image(tex_w, tex_h, &rgba) {
                st.last_tex_size = [tex_w, tex_h];
                st.map_dirty = false;
            }
            st.viewport_rgba = rgba;
        }
    }

    fn hud(&self, host: &mut dyn Host, event: &mut HudEvent<'_>) {
        if !self.enable_minimap { return; }
        let Some(cs) = host.client_state() else { return };
        if cs.gui_hidden || cs.debug_screen {
            return;
        }
        let Some(eye) = cs.eye_position else { return };
        let Ok(st) = lock(&self.state) else { return };
        if !st.config.minimap_enabled {
            return;
        }

        // Compute the minimap's exact position (mirrors `extras_hud::Corners` placement)
        let ms = st.config.minimap_size;
        let margin = 2.0f32;
        let (mx, my) = match st.config.corner {
            MapCorner::TopLeft => (margin, margin),
            MapCorner::TopRight => (event.batch.width - margin - ms, margin + 26.0),
            MapCorner::BottomLeft => (margin, event.batch.height - margin - ms),
            MapCorner::BottomRight => (
                event.batch.width - margin - ms,
                event.batch.height - margin - ms,
            ),
        };
        let map_rect = [mx, my, ms, ms];
        let blips = collect_radar_blips(host, &st.config);
        let hud_data = MinimapHudData::build(
            &st.config,
            eye,
            cs.yaw,
            st.last_biome.as_deref(),
            &st.dimension,
            st.waypoints.current_list(),
            &blips,
            map_rect,
        );

        // Xaero border frame around the minimap
        let border_col = if hud_data.chunk_is_slime {
            0xFF44_CC44
        } else {
            0xFF2B_2F3A
        };
        host.fill(event.batch, [mx - 1.0, my - 1.0, ms + 2.0, 1.0], border_col);
        host.fill(event.batch, [mx - 1.0, my + ms, ms + 2.0, 1.0], border_col);
        host.fill(event.batch, [mx - 1.0, my, 1.0, ms], border_col);
        host.fill(event.batch, [mx + ms, my, 1.0, ms], border_col);

        // Radar dots
        for (px, py, col) in hud_data.radar_dots {
            host.fill(event.batch, [px - 1.5, py - 1.5, 3.0, 3.0], 0xFF00_0000);
            host.fill(event.batch, [px - 1.0, py - 1.0, 2.0, 2.0], col);
        }

        // Waypoint markers
        for (px, py, col, initials) in hud_data.waypoint_markers {
            host.fill(event.batch, [px - 4.0, py - 4.0, 8.0, 8.0], 0xCC00_0000);
            host.fill(event.batch, [px - 3.0, py - 3.0, 6.0, 6.0], col);
            let tw = host.text_width(&initials);
            host.text(
                event.batch,
                &initials,
                [(px - tw * 0.5).round(), (py - 4.0).round()],
                0xFFFF_FFFF,
                true,
            );
        }

        // Compass N/E/S/W
        for (lbl, cx, cy, col) in hud_data.compass {
            let tw = host.text_width(lbl);
            host.text(
                event.batch,
                lbl,
                [(cx - tw * 0.5).round(), (cy - 4.0).round()],
                col,
                true,
            );
        }

        // Info lines below (or above if bottom corner)
        let info_y0 = if matches!(st.config.corner, MapCorner::TopLeft | MapCorner::TopRight) {
            my + ms + 3.0
        } else {
            my - 3.0 - (hud_data.info_lines.len() as f32 * 10.0)
        };
        for (i, (line, col)) in hud_data.info_lines.iter().enumerate() {
            let tw = host.text_width(line);
            let lx = mx + (ms - tw) * 0.5;
            let ly = info_y0 + i as f32 * 10.0;
            host.fill(event.batch, [lx - 2.0, ly - 1.0, tw + 4.0, 9.0], 0x7800_0000);
            host.text(event.batch, line, [lx.round(), ly.round()], *col, true);
        }
    }

    fn world_primitives(&self, _host: &dyn Host, event: &mut WorldPrimitivesEvent<'_>) {
        if !self.enable_minimap { return; }
        let Ok(st) = lock(&self.state) else { return };
        if !st.config.show_waypoints_in_world {
            return;
        }
        let max_dist_sq = (st.config.waypoint_beacon_distance as f64).powi(2);
        for wp in st.waypoints.current_list() {
            if wp.disabled {
                continue;
            }
            let wx = wp.x as f64 + 0.5;
            let wy = wp.y as f64;
            let wz = wp.z as f64 + 0.5;
            let dx = wx - event.camera[0];
            let dy = wy - event.camera[1];
            let dz = wz - event.camera[2];
            let dist_sq = dx * dx + dy * dy + dz * dz;
            if dist_sq > max_dist_sq {
                continue;
            }
            let dist = dist_sq.sqrt().round() as i32;
            let col = wp.color.argb();
            // Beacon beam
            event
                .primitives
                .beam([wx, wy - 8.0, wz], 96, (col & 0x00FF_FFFF) | 0x8800_0000)
                .through_walls();
            // Floating label
            event
                .primitives
                .push(PrimitiveShape::Text {
                    at: [wx, wy + 2.2, wz],
                    text: format!("{} ({}m)", wp.name, dist),
                    color: col,
                    scale: 1.0,
                    scale_with_distance: true,
                    background: true,
                })
                .through_walls();
        }
    }

    fn screen_render_post(&self, host: &dyn Host, event: &mut ScreenRenderPostEvent<'_>) {
        if event.screen.id != "mod" || host.mod_screen() != Some(MOD_ID) {
            return;
        }
        let Ok(st) = lock(&self.state) else { return };
        let w = event.batch.width;
        let h = event.batch.height;
        let mouse = event.screen.mouse;

        match st.active_screen {
            ActiveScreen::WorldMap => {
                // Draw World Map texture from `horizon/modimage`
                let tw = st.last_tex_size[0].max(1) as f32;
                let th = st.last_tex_size[1].max(1) as f32;
                host.sprite(
                    event.batch,
                    "horizon/modimage",
                    [0.0, 0.0, tw, th],
                    [0.0, 0.0, w, h],
                );

                // Overlay Waypoints onto World Map
                let half_w = w * 0.5;
                let half_h = h * 0.5;
                if st.config.wm_show_waypoints {
                    for wp in st.waypoints.current_list() {
                        if wp.disabled {
                            continue;
                        }
                        let sx = ((wp.x as f64 + 0.5 - st.cam_x) * st.zoom as f64) as f32 + half_w;
                        let sy = ((wp.z as f64 + 0.5 - st.cam_z) * st.zoom as f64) as f32 + half_h;
                        if sx < -20.0 || sx > w + 20.0 || sy < -20.0 || sy > h + 20.0 {
                            continue;
                        }
                        host.fill(event.batch, [sx - 5.0, sy - 5.0, 10.0, 10.0], 0xDD00_0000);
                        host.fill(event.batch, [sx - 4.0, sy - 4.0, 8.0, 8.0], wp.color.argb());
                        let label = format!("{} ({}, {})", wp.name, wp.x, wp.z);
                        let lw = host.text_width(&label);
                        host.fill(event.batch, [sx - lw * 0.5 - 2.0, sy + 7.0, lw + 4.0, 10.0], 0xB000_0000);
                        host.text(
                            event.batch,
                            &label,
                            [(sx - lw * 0.5).round(), (sy + 8.0).round()],
                            0xFFFF_FFFF,
                            true,
                        );
                    }
                }

                // Overlay Entity Radar onto World Map
                if st.config.wm_show_radar {
                    for blip in collect_radar_blips(host, &st.config) {
                        let sx = ((blip.x - st.cam_x) * st.zoom as f64) as f32 + half_w;
                        let sy = ((blip.z - st.cam_z) * st.zoom as f64) as f32 + half_h;
                        if sx < 0.0 || sx > w || sy < 0.0 || sy > h {
                            continue;
                        }
                        host.fill(event.batch, [sx - 2.0, sy - 2.0, 4.0, 4.0], 0xFF00_0000);
                        host.fill(event.batch, [sx - 1.5, sy - 1.5, 3.0, 3.0], blip.category.argb());
                    }
                }

                // Top header bar with cursor world coords
                let cursor_wx = (st.cam_x + ((mouse[0] - half_w) / st.zoom) as f64).floor() as i32;
                let cursor_wz = (st.cam_z + ((mouse[1] - half_h) / st.zoom) as f64).floor() as i32;
                let header = format!(
                    "Xaero's World Map | Cursor: X={}, Z={} | Zoom: {:.2}x | R-Click: Menu | Drag: Pan | Scroll: Zoom",
                    cursor_wx, cursor_wz, st.zoom
                );
                host.fill(event.batch, [0.0, 0.0, w, 18.0], 0xC010_141C);
                host.text(event.batch, &header, [8.0, 5.0], 0xFFFF_FFFF, true);

                // Left toolbar buttons (Xaero World Map style)
                let buttons: [(&str, [f32; 4]); 10] = [
                    ("+", [8.0, 26.0, 22.0, 20.0]),
                    ("-", [8.0, 50.0, 22.0, 20.0]),
                    ("Center", [8.0, 74.0, 58.0, 20.0]),
                    ("Waypoints", [8.0, 98.0, 58.0, 20.0]),
                    ("WP List", [8.0, 122.0, 58.0, 20.0]),
                    ("Radar", [8.0, 146.0, 58.0, 20.0]),
                    ("Grid", [8.0, 170.0, 58.0, 20.0]),
                    ("Slime", [8.0, 194.0, 58.0, 20.0]),
                    ("Settings", [8.0, 218.0, 58.0, 20.0]),
                    ("Close", [8.0, 242.0, 58.0, 20.0]),
                ];
                for (lbl, rect) in buttons {
                    draw_button(host, event.batch, rect, lbl, mouse, 0xCC1E_2430);
                }

                // Right-click context menu
                if let Some(cm) = &st.context_menu {
                    let mw = 132.0;
                    let mh = 72.0;
                    let cx = cm.screen_x.min(w - mw - 4.0);
                    let cy = cm.screen_y.min(h - mh - 4.0);
                    host.fill(event.batch, [cx, cy, mw, mh], 0xEE18_1C24);
                    draw_button(host, event.batch, [cx + 4.0, cy + 4.0, mw - 8.0, 20.0], "New Waypoint Here", mouse, 0xCC28_3040);
                    draw_button(host, event.batch, [cx + 4.0, cy + 26.0, mw - 8.0, 20.0], "Teleport Here (/tp)", mouse, 0xCC28_3040);
                    draw_button(host, event.batch, [cx + 4.0, cy + 48.0, mw - 8.0, 20.0], "Close Menu", mouse, 0xCC28_3040);
                }
            }
            ActiveScreen::WaypointList => {
                let pw = 380.0f32.min(w - 20.0);
                let ph = 240.0f32.min(h - 20.0);
                let px = (w - pw) * 0.5;
                let py = (h - ph) * 0.5;
                host.fill(event.batch, [0.0, 0.0, w, h], 0x9000_0000);
                host.fill(event.batch, [px, py, pw, ph], 0xF018_1C24);
                let list = st.waypoints.current_list();
                let total_pages = (list.len().max(1) + 6) / 7;
                let page = st.waypoint_page.min(total_pages.saturating_sub(1));
                let title = format!("Xaero Waypoints ({}/{} - {} total)", page + 1, total_pages, list.len());
                host.text(event.batch, &title, [px + 10.0, py + 8.0], 0xFFFF_FFFF, true);

                let start = page * 7;
                for (row, wp) in list.iter().skip(start).take(7).enumerate() {
                    let ry = py + 28.0 + row as f32 * 24.0;
                    host.fill(event.batch, [px + 10.0, ry, pw - 20.0, 21.0], 0xB023_2834);
                    host.fill(event.batch, [px + 14.0, ry + 5.0, 10.0, 10.0], wp.color.argb());
                    let state_str = if wp.disabled { "[OFF]" } else { "" };
                    let info = format!("{} {} ({}, {}, {})", wp.name, state_str, wp.x, wp.y, wp.z);
                    host.text(event.batch, &info, [px + 28.0, ry + 6.0], 0xFFFF_FFFF, true);
                    draw_button(host, event.batch, [px + pw - 160.0, ry + 2.0, 34.0, 17.0], "Edit", mouse, 0xCC2E_3F58);
                    draw_button(host, event.batch, [px + pw - 122.0, ry + 2.0, 42.0, 17.0], "Toggle", mouse, 0xCC2E_3748);
                    draw_button(host, event.batch, [px + pw - 76.0, ry + 2.0, 30.0, 17.0], "TP", mouse, 0xCC2E_4837);
                    draw_button(host, event.batch, [px + pw - 42.0, ry + 2.0, 30.0, 17.0], "Del", mouse, 0xCC5A_2828);
                }

                draw_button(host, event.batch, [px + 10.0, py + ph - 28.0, 96.0, 20.0], "+ New Waypoint", mouse, 0xCC28_4A38);
                draw_button(host, event.batch, [px + 114.0, py + ph - 28.0, 48.0, 20.0], "Prev", mouse, 0xCC28_3040);
                draw_button(host, event.batch, [px + 168.0, py + ph - 28.0, 48.0, 20.0], "Next", mouse, 0xCC28_3040);
                draw_button(host, event.batch, [px + pw - 76.0, py + ph - 28.0, 66.0, 20.0], "Close", mouse, 0xCC30_3848);
            }
            ActiveScreen::WaypointEdit => {
                if let Some(draft) = &st.waypoint_draft {
                    let pw = 290.0;
                    let ph = 172.0;
                    let px = (w - pw) * 0.5;
                    let py = (h - ph) * 0.5;
                    host.fill(event.batch, [0.0, 0.0, w, h], 0x9000_0000);
                    host.fill(event.batch, [px, py, pw, ph], 0xF018_1C24);
                    host.text(event.batch, "New / Edit Waypoint (Click field to type)", [px + 10.0, py + 8.0], 0xFFFF_FFFF, true);
                    // Name field
                    let name_bg = if draft.active_field == 0 { 0xFF1A_2433 } else { 0xFF0E_1116 };
                    let name_cur = if draft.active_field == 0 { "_" } else { "" };
                    host.fill(event.batch, [px + 10.0, py + 24.0, pw - 20.0, 20.0], name_bg);
                    host.text(event.batch, &format!("Name: {}{}", draft.name, name_cur), [px + 14.0, py + 30.0], 0xFFFF_FFFF, true);
                    // X, Y, Z coordinate fields
                    let coord_w = (pw - 32.0) / 3.0;
                    let coords = [
                        (1u8, "X", &draft.x_input),
                        (2u8, "Y", &draft.y_input),
                        (3u8, "Z", &draft.z_input),
                    ];
                    for (idx, lbl, val) in coords {
                        let cx = px + 10.0 + (idx as f32 - 1.0) * (coord_w + 6.0);
                        let bg = if draft.active_field == idx { 0xFF1A_2433 } else { 0xFF0E_1116 };
                        let cur = if draft.active_field == idx { "_" } else { "" };
                        host.fill(event.batch, [cx, py + 50.0, coord_w, 20.0], bg);
                        host.text(event.batch, &format!("{}:{}{}", lbl, val, cur), [cx + 4.0, py + 56.0], 0xFFDD_EEFF, true);
                    }
                    // Color selector
                    host.fill(event.batch, [px + 10.0, py + 80.0, 14.0, 14.0], draft.color.argb());
                    draw_button(
                        host,
                        event.batch,
                        [px + 30.0, py + 78.0, pw - 40.0, 18.0],
                        &format!("Color: {} (Click to Cycle)", draft.color.display_name()),
                        mouse,
                        0xCC28_3040,
                    );
                    draw_button(host, event.batch, [px + 10.0, py + 130.0, 120.0, 24.0], "Save", mouse, 0xCC28_5A38);
                    draw_button(host, event.batch, [px + pw - 130.0, py + 130.0, 120.0, 24.0], "Cancel", mouse, 0xCC4A_2828);
                }
            }
            ActiveScreen::Settings => {
                let pw = 320.0;
                let ph = 276.0;
                let px = (w - pw) * 0.5;
                let py = (h - ph) * 0.5;
                host.fill(event.batch, [0.0, 0.0, w, h], 0x9000_0000);
                host.fill(event.batch, [px, py, pw, ph], 0xF018_1C24);
                host.text(event.batch, "Xaero's Minimap & World Map Settings", [px + 10.0, py + 8.0], 0xFFFF_FFFF, true);
                let on_off = |b: bool| if b { "ON" } else { "OFF" };
                let seed_label = if st.editing_seed {
                    format!("World Seed (Slime): {}_", st.seed_input)
                } else {
                    format!("World Seed (Slime): {}", st.seed_input)
                };
                let items = [
                    format!("Minimap: {}", on_off(st.config.minimap_enabled)),
                    format!("Corner: {}", st.config.corner.label()),
                    format!("Size: {:.0}px (Click)", st.config.minimap_size),
                    format!("North Locked: {}", on_off(st.config.lock_north)),
                    format!("Entity Radar: {}", on_off(st.config.entity_radar)),
                    format!("Cave Mode: {}", on_off(st.config.cave_mode)),
                    format!("Slime Chunks: {}", on_off(st.config.show_slime_chunks)),
                    format!("3D Beams: {}", on_off(st.config.show_waypoints_in_world)),
                    format!("Auto Deathpoint: {}", on_off(st.config.auto_deathpoint)),
                    seed_label,
                ];
                for (i, label) in items.iter().enumerate() {
                    let ry = py + 24.0 + i as f32 * 19.0;
                    let bg = if i == 9 && st.editing_seed { 0xCC28_3E5A } else { 0xCC24_2B38 };
                    draw_button(host, event.batch, [px + 10.0, ry, pw - 20.0, 17.0], label, mouse, bg);
                }
                draw_button(host, event.batch, [px + 10.0, py + ph - 50.0, 146.0, 18.0], "Manage Waypoints...", mouse, 0xCC2E_3F58);
                draw_button(host, event.batch, [px + 164.0, py + ph - 50.0, 146.0, 18.0], "+ New Waypoint...", mouse, 0xCC28_4A38);
                draw_button(host, event.batch, [px + pw - 78.0, py + ph - 26.0, 68.0, 18.0], "Done", mouse, 0xCC30_3848);
            }
            ActiveScreen::None => {}
        }
    }

    fn screen_mouse_button_pressed_pre(
        &self,
        host: &dyn Host,
        event: &mut ScreenMouseButtonPressedPreEvent<'_>,
    ) -> Flow {
        if event.screen.id != "mod" || host.mod_screen() != Some(MOD_ID) {
            return Flow::Continue;
        }
        let mx = event.screen.mouse[0];
        let my = event.screen.mouse[1];
        let w = event.screen.rect[2];
        let h = event.screen.rect[3];

        let Ok(mut st) = lock(&self.state) else {
            return Flow::Cancel;
        };
        match st.active_screen {
            ActiveScreen::WorldMap => {
                // Right click -> open context menu
                if event.button == 1 {
                    let half_w = event.screen.rect[2] * 0.5;
                    let half_h = event.screen.rect[3] * 0.5;
                    let wx = (st.cam_x + ((mx - half_w) / st.zoom) as f64).floor() as i32;
                    let wz = (st.cam_z + ((my - half_h) / st.zoom) as f64).floor() as i32;
                    st.context_menu = Some(ContextMenu {
                        screen_x: mx,
                        screen_y: my,
                        world_x: wx,
                        world_z: wz,
                    });
                    return Flow::Cancel;
                }
                if event.button == 0 {
                    // Check context menu clicks first
                    if let Some(cm) = st.context_menu.clone() {
                        let mw = 132.0;
                        let mh = 72.0;
                        let cx = cm.screen_x.min(w - mw - 4.0);
                        let cy = cm.screen_y.min(h - mh - 4.0);
                        if in_rect(mx, my, [cx + 4.0, cy + 4.0, mw - 8.0, 20.0]) {
                            st.context_menu = None;
                            if let Ok(mut ret) = lock(&self.waypoint_return) {
                                *ret = ActiveScreen::WorldMap;
                            }
                            st.waypoint_draft = Some(WaypointDraft::from_coords(
                                format!("WP {},{}", cm.world_x, cm.world_z),
                                cm.world_x,
                                64,
                                cm.world_z,
                                WaypointColor::Gold,
                                None,
                            ));
                            st.active_screen = ActiveScreen::WaypointEdit;
                            return Flow::Cancel;
                        }
                        if in_rect(mx, my, [cx + 4.0, cy + 26.0, mw - 8.0, 20.0]) {
                            st.context_menu = None;
                            self.queue(PendingAction::SendChat(format!(
                                "/tp @s {} ~ {}",
                                cm.world_x, cm.world_z
                            )));
                            self.queue(PendingAction::CloseScreen);
                            return Flow::Cancel;
                        }
                        st.context_menu = None;
                        return Flow::Cancel;
                    }

                    // Toolbar clicks
                    let rects: [(usize, [f32; 4]); 10] = [
                        (0, [8.0, 26.0, 22.0, 20.0]),
                        (1, [8.0, 50.0, 22.0, 20.0]),
                        (2, [8.0, 74.0, 58.0, 20.0]),
                        (3, [8.0, 98.0, 58.0, 20.0]),
                        (4, [8.0, 122.0, 58.0, 20.0]),
                        (5, [8.0, 146.0, 58.0, 20.0]),
                        (6, [8.0, 170.0, 58.0, 20.0]),
                        (7, [8.0, 194.0, 58.0, 20.0]),
                        (8, [8.0, 218.0, 58.0, 20.0]),
                        (9, [8.0, 242.0, 58.0, 20.0]),
                    ];
                    for (idx, r) in rects {
                        if in_rect(mx, my, r) {
                            match idx {
                                0 => {
                                    st.zoom = (st.zoom * 1.4).min(8.0);
                                    st.map_dirty = true;
                                }
                                1 => {
                                    st.zoom = (st.zoom / 1.4).max(0.125);
                                    st.map_dirty = true;
                                }
                                2 => {
                                    if let Some(cs) = host.client_state()
                                        && let Some(eye) = cs.eye_position
                                    {
                                        st.cam_x = eye[0];
                                        st.cam_z = eye[2];
                                        st.map_dirty = true;
                                    }
                                }
                                3 => {
                                    st.config.wm_show_waypoints = !st.config.wm_show_waypoints;
                                    st.save_config();
                                }
                                4 => {
                                    st.waypoint_page = 0;
                                    st.active_screen = ActiveScreen::WaypointList;
                                }
                                5 => {
                                    st.config.wm_show_radar = !st.config.wm_show_radar;
                                    st.save_config();
                                }
                                6 => {
                                    st.config.wm_show_grid = !st.config.wm_show_grid;
                                    st.map_dirty = true;
                                    st.save_config();
                                }
                                7 => {
                                    st.config.wm_show_slime_chunks = !st.config.wm_show_slime_chunks;
                                    st.map_dirty = true;
                                    st.save_config();
                                }
                                8 => st.active_screen = ActiveScreen::Settings,
                                9 => self.queue(PendingAction::CloseScreen),
                                _ => {}
                            }
                            return Flow::Cancel;
                        }
                    }
                }
            }
            ActiveScreen::WaypointList => {
                let pw = 380.0f32.min(event.screen.rect[2] - 20.0);
                let ph = 240.0f32.min(event.screen.rect[3] - 20.0);
                let px = (event.screen.rect[2] - pw) * 0.5;
                let py = (event.screen.rect[3] - ph) * 0.5;
                let total = st.waypoints.current_list().len();
                let total_pages = (total.max(1) + 6) / 7;
                let page = st.waypoint_page.min(total_pages.saturating_sub(1));

                if in_rect(mx, my, [px + 10.0, py + ph - 28.0, 96.0, 20.0]) {
                    if let Some(cs) = host.client_state()
                        && let Some(eye) = cs.eye_position
                    {
                        if let Ok(mut ret) = lock(&self.waypoint_return) {
                            *ret = ActiveScreen::WaypointList;
                        }
                        st.waypoint_draft = Some(WaypointDraft::from_coords("New Waypoint".to_string(), eye[0].floor() as i32, (eye[1] - 1.62).floor() as i32, eye[2].floor() as i32, WaypointColor::Aqua, None));
                        st.active_screen = ActiveScreen::WaypointEdit;
                    }
                    return Flow::Cancel;
                }
                if in_rect(mx, my, [px + 114.0, py + ph - 28.0, 48.0, 20.0]) {
                    st.waypoint_page = page.saturating_sub(1);
                    return Flow::Cancel;
                }
                if in_rect(mx, my, [px + 168.0, py + ph - 28.0, 48.0, 20.0]) {
                    if page + 1 < total_pages {
                        st.waypoint_page = page + 1;
                    }
                    return Flow::Cancel;
                }
                if in_rect(mx, my, [px + pw - 76.0, py + ph - 28.0, 66.0, 20.0]) {
                    self.queue(PendingAction::CloseScreen);
                    return Flow::Cancel;
                }

                let start = page * 7;
                let visible = total.saturating_sub(start).min(7);
                for row in 0..visible {
                    let i = start + row;
                    let ry = py + 28.0 + row as f32 * 24.0;
                    if in_rect(mx, my, [px + pw - 160.0, ry + 2.0, 34.0, 17.0]) {
                        if let Some(wp) = st.waypoints.current_list().get(i).cloned() {
                            if let Ok(mut ret) = lock(&self.waypoint_return) {
                                *ret = ActiveScreen::WaypointList;
                            }
                            st.waypoint_draft = Some(WaypointDraft::from_coords(wp.name, wp.x, wp.y, wp.z, wp.color, Some(i)));
                            st.active_screen = ActiveScreen::WaypointEdit;
                        }
                        return Flow::Cancel;
                    }
                    if in_rect(mx, my, [px + pw - 122.0, ry + 2.0, 42.0, 17.0]) {
                        if let Some(dir) = st.config_dir.clone() {
                            st.waypoints.toggle_at(&dir, i);
                        }
                        return Flow::Cancel;
                    }
                    if in_rect(mx, my, [px + pw - 76.0, ry + 2.0, 30.0, 17.0]) {
                        if let Some(wp) = st.waypoints.current_list().get(i).cloned() {
                            self.queue(PendingAction::SendChat(format!(
                                "/tp @s {} {} {}",
                                wp.x, wp.y, wp.z
                            )));
                            self.queue(PendingAction::CloseScreen);
                        }
                        return Flow::Cancel;
                    }
                    if in_rect(mx, my, [px + pw - 42.0, ry + 2.0, 30.0, 17.0]) {
                        if let Some(dir) = st.config_dir.clone() {
                            st.waypoints.remove_at(&dir, i);
                        }
                        return Flow::Cancel;
                    }
                }
            }
            ActiveScreen::WaypointEdit => {
                let pw = 290.0;
                let ph = 172.0;
                let px = (event.screen.rect[2] - pw) * 0.5;
                let py = (event.screen.rect[3] - ph) * 0.5;
                if in_rect(mx, my, [px + 10.0, py + 24.0, pw - 20.0, 20.0]) {
                    if let Some(draft) = st.waypoint_draft.as_mut() {
                        draft.active_field = 0;
                    }
                    return Flow::Cancel;
                }
                let coord_w = (pw - 32.0) / 3.0;
                for idx in 1u8..=3 {
                    let cx = px + 10.0 + (idx as f32 - 1.0) * (coord_w + 6.0);
                    if in_rect(mx, my, [cx, py + 50.0, coord_w, 20.0]) {
                        if let Some(draft) = st.waypoint_draft.as_mut() {
                            draft.active_field = idx;
                        }
                        return Flow::Cancel;
                    }
                }
                if in_rect(mx, my, [px + 30.0, py + 78.0, pw - 40.0, 18.0]) {
                    if let Some(draft) = st.waypoint_draft.as_mut() {
                        draft.color = draft.color.next();
                    }
                    return Flow::Cancel;
                }
                if in_rect(mx, my, [px + 10.0, py + 130.0, 120.0, 24.0]) {
                    let ret = lock(&self.waypoint_return).map(|r| *r).unwrap_or(ActiveScreen::None);
                    if let Some(draft) = st.waypoint_draft.take()
                        && let Some(dir) = st.config_dir.clone()
                    {
                        let (wx, wy, wz) = draft.parsed_coords();
                        let wp = Waypoint::new(draft.name, wx, wy, wz, draft.color);
                        if let Some(idx) = draft.edit_index {
                            st.waypoints.update_at(&dir, idx, wp);
                            st.active_screen = ActiveScreen::WaypointList;
                            return Flow::Cancel;
                        } else {
                            st.waypoints.add_waypoint(&dir, wp);
                        }
                    }
                    if ret != ActiveScreen::None {
                        st.active_screen = ret;
                        st.map_dirty = true;
                    } else {
                        self.queue(PendingAction::CloseScreen);
                    }
                    return Flow::Cancel;
                }
                if in_rect(mx, my, [px + pw - 130.0, py + 130.0, 120.0, 24.0]) {
                    let was_edit = st.waypoint_draft.as_ref().is_some_and(|d| d.edit_index.is_some());
                    let ret = lock(&self.waypoint_return).map(|r| *r).unwrap_or(ActiveScreen::None);
                    st.waypoint_draft = None;
                    if was_edit {
                        st.active_screen = ActiveScreen::WaypointList;
                    } else if ret != ActiveScreen::None {
                        st.active_screen = ret;
                        st.map_dirty = true;
                    } else {
                        self.queue(PendingAction::CloseScreen);
                    }
                    return Flow::Cancel;
                }
            }
            ActiveScreen::Settings => {
                let pw = 320.0;
                let ph = 276.0;
                let px = (event.screen.rect[2] - pw) * 0.5;
                let py = (event.screen.rect[3] - ph) * 0.5;
                if in_rect(mx, my, [px + 10.0, py + ph - 50.0, 146.0, 18.0]) {
                    st.editing_seed = false;
                    st.waypoint_page = 0;
                    st.active_screen = ActiveScreen::WaypointList;
                    return Flow::Cancel;
                }
                if in_rect(mx, my, [px + 164.0, py + ph - 50.0, 146.0, 18.0]) {
                    st.editing_seed = false;
                    if let Some(cs) = host.client_state()
                        && let Some(eye) = cs.eye_position
                    {
                        if let Ok(mut ret) = lock(&self.waypoint_return) {
                            *ret = ActiveScreen::Settings;
                        }
                        st.waypoint_draft = Some(WaypointDraft::from_coords("New Waypoint".to_string(), eye[0].floor() as i32, (eye[1] - 1.62).floor() as i32, eye[2].floor() as i32, WaypointColor::Aqua, None));
                        st.active_screen = ActiveScreen::WaypointEdit;
                    }
                    return Flow::Cancel;
                }
                if in_rect(mx, my, [px + pw - 78.0, py + ph - 26.0, 68.0, 18.0]) {
                    st.editing_seed = false;
                    self.queue(PendingAction::CloseScreen);
                    return Flow::Cancel;
                }
                for i in 0..10 {
                    let ry = py + 24.0 + i as f32 * 19.0;
                    if in_rect(mx, my, [px + 10.0, ry, pw - 20.0, 17.0]) {
                        st.editing_seed = i == 9;
                        match i {
                            0 => st.config.minimap_enabled = !st.config.minimap_enabled,
                            1 => st.config.corner = st.config.corner.next(),
                            2 => st.config.cycle_size(),
                            3 => st.config.lock_north = !st.config.lock_north,
                            4 => st.config.entity_radar = !st.config.entity_radar,
                            5 => st.config.cave_mode = !st.config.cave_mode,
                            6 => st.config.show_slime_chunks = !st.config.show_slime_chunks,
                            7 => st.config.show_waypoints_in_world = !st.config.show_waypoints_in_world,
                            8 => st.config.auto_deathpoint = !st.config.auto_deathpoint,
                            _ => {}
                        }
                        st.save_config();
                        return Flow::Cancel;
                    }
                }
            }
            ActiveScreen::None => {}
        }
        Flow::Cancel
    }

    fn screen_mouse_scrolled_pre(
        &self,
        host: &dyn Host,
        event: &mut ScreenMouseScrolledPreEvent<'_>,
    ) -> Flow {
        if event.screen.id != "mod" || host.mod_screen() != Some(MOD_ID) {
            return Flow::Continue;
        }
        if let Ok(mut st) = lock(&self.state)
            && st.active_screen == ActiveScreen::WorldMap
        {
            let factor = if event.scroll[1] > 0.0 { 1.2 } else { 1.0 / 1.2 };
            st.zoom = (st.zoom * factor).clamp(0.125, 8.0);
            st.map_dirty = true;
        }
        Flow::Cancel
    }

    fn screen_mouse_dragged_pre(
        &self,
        host: &dyn Host,
        event: &mut ScreenMouseDraggedPreEvent<'_>,
    ) -> Flow {
        if event.screen.id != "mod" || host.mod_screen() != Some(MOD_ID) {
            return Flow::Continue;
        }
        if event.button == 0
            && let Ok(mut st) = lock(&self.state)
            && st.active_screen == ActiveScreen::WorldMap
        {
            st.cam_x -= (event.delta[0] / st.zoom) as f64;
            st.cam_z -= (event.delta[1] / st.zoom) as f64;
            st.map_dirty = true;
        }
        Flow::Cancel
    }

    fn screen_key_pressed_pre(
        &self,
        host: &dyn Host,
        event: &mut ScreenKeyPressedPreEvent<'_>,
    ) -> Flow {
        if event.screen.id != "mod" || host.mod_screen() != Some(MOD_ID) {
            return Flow::Continue;
        }
        let Ok(mut st) = lock(&self.state) else {
            return Flow::Continue;
        };
        if event.key == "key.keyboard.escape" {
            st.active_screen = ActiveScreen::None;
            st.context_menu = None;
            st.editing_seed = false;
            return Flow::Continue;
        }
        match st.active_screen {
            ActiveScreen::WaypointEdit => {
                if event.key == "key.keyboard.tab"
                    && let Some(draft) = st.waypoint_draft.as_mut()
                {
                    draft.active_field = (draft.active_field + 1) % 4;
                    return Flow::Cancel;
                }
                if event.key == "key.keyboard.backspace"
                    && let Some(draft) = st.waypoint_draft.as_mut()
                {
                    match draft.active_field {
                        0 => { draft.name.pop(); }
                        1 => { draft.x_input.pop(); }
                        2 => { draft.y_input.pop(); }
                        3 => { draft.z_input.pop(); }
                        _ => {}
                    }
                    return Flow::Cancel;
                }
                if event.key == "key.keyboard.enter" {
                    let ret = lock(&self.waypoint_return).map(|r| *r).unwrap_or(ActiveScreen::None);
                    if let Some(draft) = st.waypoint_draft.take()
                        && let Some(dir) = st.config_dir.clone()
                    {
                        let (wx, wy, wz) = draft.parsed_coords();
                        let wp = Waypoint::new(draft.name, wx, wy, wz, draft.color);
                        if let Some(idx) = draft.edit_index {
                            st.waypoints.update_at(&dir, idx, wp);
                            st.active_screen = ActiveScreen::WaypointList;
                            return Flow::Cancel;
                        } else {
                            st.waypoints.add_waypoint(&dir, wp);
                        }
                    }
                    if ret != ActiveScreen::None {
                        st.active_screen = ret;
                        st.map_dirty = true;
                    } else {
                        self.queue(PendingAction::CloseScreen);
                    }
                    return Flow::Cancel;
                }
            }
            ActiveScreen::Settings if st.editing_seed => {
                if event.key == "key.keyboard.enter" {
                    st.editing_seed = false;
                    st.config.world_seed = st.seed_input.parse().unwrap_or(0);
                    st.save_config();
                    return Flow::Cancel;
                }
                if event.key == "key.keyboard.backspace" {
                    st.seed_input.pop();
                    st.config.world_seed = st.seed_input.parse().unwrap_or(0);
                    st.save_config();
                    return Flow::Cancel;
                }
            }
            ActiveScreen::WorldMap => {
                let k_worldmap = host.mod_key(WORLDMAP_MOD_ID, "open_worldmap", "key.keyboard.m");
                if event.key == k_worldmap {
                    self.queue(PendingAction::CloseScreen);
                    return Flow::Cancel;
                }
            }
            _ => {}
        }
        Flow::Cancel
    }

    fn screen_char_typed_pre(
        &self,
        host: &dyn Host,
        event: &mut ScreenCharTypedPreEvent<'_>,
    ) -> Flow {
        if event.screen.id != "mod" || host.mod_screen() != Some(MOD_ID) {
            return Flow::Continue;
        }
        if let Ok(mut st) = lock(&self.state) {
            if st.active_screen == ActiveScreen::WaypointEdit
                && let Some(draft) = st.waypoint_draft.as_mut()
                && !event.c.is_control()
            {
                match draft.active_field {
                    0 if draft.name.chars().count() < 24 => {
                        draft.name.push(event.c);
                    }
                    1 | 2 | 3 => {
                        let target = match draft.active_field {
                            1 => &mut draft.x_input,
                            2 => &mut draft.y_input,
                            _ => &mut draft.z_input,
                        };
                        if (event.c.is_ascii_digit() || (event.c == '-' && target.is_empty()))
                            && target.len() < 10
                        {
                            target.push(event.c);
                        }
                    }
                    _ => {}
                }
            } else if st.active_screen == ActiveScreen::Settings
                && st.editing_seed
                && (event.c.is_ascii_digit() || (event.c == '-' && st.seed_input.is_empty()))
                && st.seed_input.len() < 20
            {
                st.seed_input.push(event.c);
                st.config.world_seed = st.seed_input.parse().unwrap_or(0);
                st.save_config();
            }
        }
        Flow::Cancel
    }
}
