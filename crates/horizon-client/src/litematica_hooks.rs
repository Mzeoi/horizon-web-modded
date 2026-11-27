#![forbid(unsafe_code)]

use crate::mod_api::{
    Flow, FrameEvent, Hooks, Host, HudEvent, KeyEvent, ModsEvent, NativeMod, NativeScreenKind,
    ScreenCharTypedPreEvent, ScreenKeyPressedPreEvent, ScreenMouseButtonPressedPreEvent,
    ScreenRenderPostEvent, SettingsClickEvent, SettingsEvent, WorldPrimitivesEvent,
};
use horizon_litematica::{
    ActiveScreen, LitematicaState, parse_state_text,
    placement::{SchematicPlacement, transform_properties},
    schematic::{PaletteEntry, Schematic, SchematicRegion},
    verifier::{BlockMismatchKind, VerificationResult},
};
use horizon_os::sync::lock;
use horizon_world::registry::{BlockRegistry, BlockStateId};
use std::collections::{BTreeMap, HashMap};
use std::sync::{Arc, Mutex};

pub const MOD_ID: &str = "litematica";

static ACTIVE_LITEMATICA: Mutex<Option<Arc<Mutex<LitematicaState>>>> = Mutex::new(None);
static SECTION_GHOST_REVISIONS: Mutex<Option<HashMap<[i32; 3], u64>>> = Mutex::new(None);

#[derive(Clone, Debug)]
enum PendingAction {
    OpenScreen(ActiveScreen),
    CloseScreen,
    DownloadBytes(String, Vec<u8>),
    SaveSelection,
}

pub struct LitematicaMod {
    enabled: bool,
    state: Arc<Mutex<LitematicaState>>,
    pending: Mutex<Vec<PendingAction>>,
    deferred_open: Mutex<Option<ActiveScreen>>,
}

impl LitematicaMod {
    pub fn new(enabled: bool) -> Self {
        let state = Arc::new(Mutex::new(LitematicaState::new()));
        if enabled && let Ok(mut slot) = lock(&ACTIVE_LITEMATICA) {
            *slot = Some(Arc::clone(&state));
        }
        Self {
            enabled,
            state,
            pending: Mutex::new(Vec::new()),
            deferred_open: Mutex::new(None),
        }
    }

    pub fn mods_list(enabled: bool) -> Vec<Box<dyn Hooks>> {
        if !enabled {
            return Vec::new();
        }
        vec![Box::new(Self::new(true)) as Box<dyn Hooks>]
    }

    pub fn mods_arc(enabled: bool) -> Arc<[Box<dyn Hooks>]> {
        Arc::from(Self::mods_list(enabled))
    }

    fn queue(&self, action: PendingAction) {
        if let Ok(mut q) = lock(&self.pending) {
            q.push(action);
        }
    }
}

impl Default for LitematicaMod {
    fn default() -> Self {
        Self::new(true)
    }
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
    let fill = if hovered { 0xE03A_506B } else { bg };
    host.fill(batch, rect, fill);
    let border = if hovered { 0xFF77_CCFF } else { 0xFF4A_5568 };
    host.fill(batch, [rect[0], rect[1], rect[2], 1.0], border);
    host.fill(batch, [rect[0], rect[1] + rect[3] - 1.0, rect[2], 1.0], border);
    host.fill(batch, [rect[0], rect[1], 1.0, rect[3]], border);
    host.fill(batch, [rect[0] + rect[2] - 1.0, rect[1], 1.0, rect[3]], border);

    let tw = host.text_width(label);
    let tx = rect[0] + (rect[2] - tw) * 0.5;
    let ty = rect[1] + (rect[3] - 8.0) * 0.5;
    host.text(batch, label, [tx.round(), ty.round()], 0xFFFF_FFFF, true);
}

fn player_block_pos(host: &dyn Host) -> Option<[i32; 3]> {
    let cs = host.client_state()?;
    let eye = cs.eye_position?;
    Some([
        eye[0].floor() as i32,
        (eye[1] - 1.62).floor() as i32,
        eye[2].floor() as i32,
    ])
}

fn format_state_with_props(
    host: &dyn Host,
    sid: horizon_world::registry::BlockStateId,
    bname: &str,
) -> String {
    const KEYS: [&str; 26] = [
        "facing",
        "half",
        "type",
        "axis",
        "open",
        "powered",
        "lit",
        "waterlogged",
        "shape",
        "part",
        "hinge",
        "rotation",
        "north",
        "east",
        "south",
        "west",
        "up",
        "down",
        "delay",
        "mode",
        "attachment",
        "face",
        "enabled",
        "extended",
        "conditional",
        "level",
    ];
    let mut parts = Vec::new();
    for k in KEYS {
        if let Some(v) = host.block_state_property(sid, k) {
            parts.push(format!("{k}={v}"));
        }
    }
    if parts.is_empty() {
        bname.to_string()
    } else {
        format!("{bname}[{}]", parts.join(","))
    }
}

fn recompute_verification(host: &dyn Host, st: &mut LitematicaState) {
    let Some((pl, schem)) = st.schematic_for_placement(st.active_placement).map(|(p, s)| (p.clone(), s.clone())) else {
        st.verification = VerificationResult::default();
        st.verification_dirty = false;
        return;
    };
    if !pl.enabled {
        st.verification = VerificationResult::default();
        st.verification_dirty = false;
        return;
    }

    // Compute chunk bounding box around the placement
    let mut min_x = i32::MAX;
    let mut min_z = i32::MAX;
    let mut max_x = i32::MIN;
    let mut max_z = i32::MIN;
    for reg in &schem.regions {
        let corners = [
            reg.position,
            [
                reg.position[0] + reg.size[0] - 1,
                reg.position[1] + reg.size[1] - 1,
                reg.position[2] + reg.size[2] - 1,
            ],
        ];
        for cx in [corners[0][0], corners[1][0]] {
            for cz in [corners[0][2], corners[1][2]] {
                let wp = pl.transform_pos([cx, reg.position[1], cz]);
                min_x = min_x.min(wp[0]);
                min_z = min_z.min(wp[2]);
                max_x = max_x.max(wp[0]);
                max_z = max_z.max(wp[2]);
            }
        }
    }

    let chunks = if min_x <= max_x && min_z <= max_z {
        host.client_chunks([min_x >> 4, min_z >> 4], [max_x >> 4, max_z >> 4]).ok()
    } else {
        None
    };

    let camera = host
        .client_state()
        .and_then(|cs| cs.eye_position)
        .unwrap_or([pl.origin[0] as f64, pl.origin[1] as f64, pl.origin[2] as f64]);

    let states = host.block_states();
    let result = VerificationResult::compute(
        &schem,
        &pl,
        st.config.layer_mode,
        st.config.layer_y,
        camera,
        st.config.max_render_blocks,
        |pos| {
            let snap = chunks.as_ref()?;
            if !snap.loaded([pos[0], pos[2]]) {
                return None;
            }
            let sid = snap.block(pos);
            let bname = states
                .get(sid)
                .map(|f| f.name.as_ref())
                .unwrap_or("minecraft:air")
                .to_string();
            let stext = format_state_with_props(host, sid, &bname);
            Some((bname, stext))
        },
    );
    let prev_counts = (
        st.verification.total_placed,
        st.verification.total_wrong,
        st.verification.total_missing,
    );
    let new_counts = (result.total_placed, result.total_wrong, result.total_missing);
    if prev_counts != new_counts {
        let kind_tag = |k: BlockMismatchKind| match k {
            BlockMismatchKind::Match => 0u8,
            BlockMismatchKind::Missing => 1,
            BlockMismatchKind::WrongBlock => 2,
            BlockMismatchKind::WrongState => 3,
        };
        let old_set: std::collections::HashSet<([i32; 3], u8)> =
            st.verification.blocks.iter().map(|b| (b.pos, kind_tag(b.kind))).collect();
        let new_set: std::collections::HashSet<([i32; 3], u8)> =
            result.blocks.iter().map(|b| (b.pos, kind_tag(b.kind))).collect();
        let mut changed_secs = std::collections::HashSet::new();
        for (pos, _) in old_set.symmetric_difference(&new_set) {
            changed_secs.insert([pos[0] >> 4, pos[1] >> 4, pos[2] >> 4]);
        }
        if changed_secs.is_empty() {
            st.bump_ghost_revision();
        } else if let Ok(mut map_opt) = lock(&SECTION_GHOST_REVISIONS) {
            let map = map_opt.get_or_insert_with(HashMap::new);
            for sec in changed_secs {
                let entry = map.entry(sec).or_insert(0);
                *entry = entry.wrapping_add(1);
            }
        }
    }
    st.verification = result;
    st.verification_dirty = false;
}

fn resolve_palette_state(
    registry: &BlockRegistry,
    name: &str,
    props: &BTreeMap<String, String>,
) -> Option<BlockStateId> {
    let default = registry.default_state(name).or_else(|| {
        if name.contains(':') {
            None
        } else {
            registry.default_state(&format!("minecraft:{name}"))
        }
    })?;
    if props.is_empty() {
        return Some(default);
    }
    let mut state = registry.state(default)?.clone();
    for (k, v) in props {
        if state.properties.contains_key(k) {
            state.properties.insert(k.clone(), v.clone());
        }
    }
    registry.id(&state).or(Some(default))
}

fn active_state_arc() -> Option<Arc<Mutex<LitematicaState>>> {
    lock(&ACTIVE_LITEMATICA).ok()?.clone()
}

/// Returns `(source, (min, max_exclusive), alpha, range)` for `Game::native_ghosts`.
pub fn ghost_config() -> Option<(u64, ([i32; 3], [i32; 3]), u8, u32)> {
    let arc = active_state_arc()?;
    let st = lock(&arc).ok()?;
    if !st.config.render_enabled || !st.config.render_blocks {
        return None;
    }
    let mut min = [i32::MAX; 3];
    let mut max_excl = [i32::MIN; 3];
    let mut any = false;
    for pl in &st.placements {
        if !pl.enabled {
            continue;
        }
        let Some(schem) = st.schematics.iter().find(|s| s.file_name == pl.file_name) else {
            continue;
        };
        for reg in &schem.regions {
            if reg.size[0] <= 0 || reg.size[1] <= 0 || reg.size[2] <= 0 {
                continue;
            }
            let p0 = pl.transform_pos(reg.position);
            let p1 = pl.transform_pos([
                reg.position[0] + reg.size[0] - 1,
                reg.position[1] + reg.size[1] - 1,
                reg.position[2] + reg.size[2] - 1,
            ]);
            for i in 0..3 {
                min[i] = min[i].min(p0[i].min(p1[i]));
                max_excl[i] = max_excl[i].max(p0[i].max(p1[i]) + 1);
            }
            any = true;
        }
    }
    if !any {
        return None;
    }
    match st.config.layer_mode {
        horizon_litematica::config::LayerMode::All => {}
        horizon_litematica::config::LayerMode::SingleLayer => {
            min[1] = min[1].max(st.config.layer_y);
            max_excl[1] = max_excl[1].min(st.config.layer_y + 1);
        }
        horizon_litematica::config::LayerMode::AllBelow => {
            max_excl[1] = max_excl[1].min(st.config.layer_y + 1);
        }
        horizon_litematica::config::LayerMode::AllAbove => {
            min[1] = min[1].max(st.config.layer_y);
        }
    }
    if min[0] >= max_excl[0] || min[1] >= max_excl[1] || min[2] >= max_excl[2] {
        return None;
    }
    Some((st.ghost_revision, (min, max_excl), 155, 96))
}

/// Returns the current ghost mesh revision for `Game::native_ghost_revision`.
pub fn ghost_revision() -> u64 {
    let Some(arc) = active_state_arc() else {
        return 0;
    };
    let Ok(st) = lock(&arc) else {
        return 0;
    };
    st.ghost_revision
}

/// Returns a section-granular ghost mesh revision for `Game::native_ghost_revision`.
pub fn ghost_revision_for_section(section: [i32; 3]) -> u64 {
    let base = ghost_revision();
    let sec_rev = lock(&SECTION_GHOST_REVISIONS)
        .ok()
        .and_then(|m| m.as_ref().and_then(|map| map.get(&section).copied()))
        .unwrap_or(0);
    base.wrapping_add(sec_rev)
}

/// Populates `out` with `(packed_local_at, BlockStateId)` for section `[sx, sy, sz]`.
pub fn collect_ghost_section(
    section: [i32; 3],
    registry: &BlockRegistry,
    is_world_same: impl Fn([i32; 3], BlockStateId) -> bool,
    out: &mut Vec<(u16, BlockStateId)>,
) {
    let Some(arc) = active_state_arc() else {
        return;
    };
    let Ok(st) = lock(&arc) else {
        return;
    };
    if !st.config.render_enabled || !st.config.render_blocks {
        return;
    }
    let sec_min = [section[0] << 4, section[1] << 4, section[2] << 4];
    let sec_max = [sec_min[0] + 16, sec_min[1] + 16, sec_min[2] + 16];

    for pl in &st.placements {
        if !pl.enabled {
            continue;
        }
        let Some(schem) = st.schematics.iter().find(|s| s.file_name == pl.file_name) else {
            continue;
        };
        for reg in &schem.regions {
            if reg.size[0] <= 0 || reg.size[1] <= 0 || reg.size[2] <= 0 {
                continue;
            }
            let p0 = pl.transform_pos(reg.position);
            let p1 = pl.transform_pos([
                reg.position[0] + reg.size[0] - 1,
                reg.position[1] + reg.size[1] - 1,
                reg.position[2] + reg.size[2] - 1,
            ]);
            let x0 = sec_min[0].max(p0[0].min(p1[0]));
            let y0 = sec_min[1].max(p0[1].min(p1[1]));
            let z0 = sec_min[2].max(p0[2].min(p1[2]));
            let x1 = sec_max[0].min(p0[0].max(p1[0]) + 1);
            let y1 = sec_max[1].min(p0[1].max(p1[1]) + 1);
            let z1 = sec_max[2].min(p0[2].max(p1[2]) + 1);
            if x0 >= x1 || y0 >= y1 || z0 >= z1 {
                continue;
            }

            let mut pal_cache: Vec<Option<BlockStateId>> = vec![None; reg.palette.len()];
            for (idx, entry) in reg.palette.iter().enumerate() {
                if entry.is_air() {
                    continue;
                }
                let props = transform_properties(&entry.properties, pl.rotation, pl.mirror);
                pal_cache[idx] = resolve_palette_state(registry, &entry.name, &props);
            }

            for wy in y0..y1 {
                if !st.config.layer_mode.matches_y(wy, st.config.layer_y) {
                    continue;
                }
                let ly = (wy - sec_min[1]) as u16;
                for wz in z0..z1 {
                    let lz = (wz - sec_min[2]) as u16;
                    for wx in x0..x1 {
                        let local = pl.inverse_transform_pos([wx, wy, wz]);
                        let rx = local[0] - reg.position[0];
                        let ry = local[1] - reg.position[1];
                        let rz = local[2] - reg.position[2];
                        if rx < 0 || ry < 0 || rz < 0 || rx >= reg.size[0] || ry >= reg.size[1] || rz >= reg.size[2] {
                            continue;
                        }
                        let flat = (ry as usize) * (reg.size[0] as usize * reg.size[2] as usize)
                            + (rz as usize) * (reg.size[0] as usize)
                            + (rx as usize);
                        let Some(&pal_idx) = reg.blocks.get(flat) else {
                            continue;
                        };
                        let Some(state) = pal_cache.get(pal_idx as usize).copied().flatten() else {
                            continue;
                        };
                        if is_world_same([wx, wy, wz], state) {
                            continue;
                        }
                        let lx = (wx - sec_min[0]) as u16;
                        let at = (ly << 8) | (lz << 4) | lx;
                        out.push((at, state));
                    }
                }
            }
        }
    }
}

fn capture_selection_to_schematic(host: &dyn Host, st: &mut LitematicaState) -> Result<(String, Vec<u8>), String> {
    let (min, max) = st
        .selection
        .bounds()
        .ok_or_else(|| "Set Pos1 and Pos2 first".to_string())?;
    let sx = max[0] - min[0] + 1;
    let sy = max[1] - min[1] + 1;
    let sz = max[2] - min[2] + 1;
    if sx <= 0 || sy <= 0 || sz <= 0 {
        return Err("Invalid selection bounds".to_string());
    }
    let volume = (sx as u64) * (sy as u64) * (sz as u64);
    if volume > 2_000_000 {
        return Err("Selection too large (>2M blocks)".to_string());
    }

    let chunks = host
        .client_chunks([min[0] >> 4, min[2] >> 4], [max[0] >> 4, max[2] >> 4])
        .map_err(|e| format!("Failed to read chunks: {e}"))?;

    let mut palette = vec![PaletteEntry::air()];
    let mut pal_map: HashMap<(String, BTreeMap<String, String>), u16> = HashMap::new();
    pal_map.insert(("minecraft:air".to_string(), BTreeMap::new()), 0);

    let mut blocks = Vec::with_capacity(volume as usize);
    let mut total_blocks = 0u64;

    let states = host.block_states();
    for y in 0..sy {
        for z in 0..sz {
            for x in 0..sx {
                let wx = min[0] + x;
                let wy = min[1] + y;
                let wz = min[2] + z;
                let sid = chunks.block([wx, wy, wz]);
                let bname = states
                    .get(sid)
                    .map(|f| f.name.as_ref())
                    .unwrap_or("minecraft:air")
                    .to_string();
                let stext = format_state_with_props(host, sid, &bname);
                let (bname, props) = parse_state_text(&stext);
                if matches!(
                    bname.as_str(),
                    "minecraft:air" | "minecraft:cave_air" | "minecraft:void_air" | ""
                ) {
                    blocks.push(0);
                    continue;
                }
                total_blocks += 1;
                let key = (bname.clone(), props.clone());
                let idx = if let Some(&existing) = pal_map.get(&key) {
                    existing
                } else {
                    let next_id = u16::try_from(palette.len())
                        .map_err(|_| "Palette exceeded 65535 block states".to_string())?;
                    palette.push(PaletteEntry {
                        name: bname,
                        properties: props,
                    });
                    pal_map.insert(key, next_id);
                    next_id
                };
                blocks.push(idx);
            }
        }
    }

    let name = if st.save_name_input.trim().is_empty() {
        "untitled".to_string()
    } else {
        st.save_name_input.trim().to_string()
    };
    let file_name = if name.ends_with(".litematic") {
        name.clone()
    } else {
        format!("{name}.litematic")
    };

    let schem = Schematic {
        name: name.clone(),
        author: "Horizon Player".to_string(),
        description: "Saved in Horizon Web Litematica".to_string(),
        version: 7,
        data_version: 4784,
        total_blocks,
        total_volume: volume,
        enclosing_size: [sx, sy, sz],
        regions: vec![SchematicRegion {
            name: "Main".to_string(),
            position: [0, 0, 0],
            size: [sx, sy, sz],
            palette,
            blocks,
        }],
        file_name: file_name.clone(),
    };

    let bytes = schem.encode_litematic_bytes()?;
    st.import_schematic_bytes(&file_name, &bytes, None)?;
    Ok((file_name, bytes))
}

impl Hooks for LitematicaMod {
    fn mods(&self, _host: &mut dyn Host, event: &mut ModsEvent<'_>) {
        if !self.enabled {
            return;
        }
        event.mods.push(NativeMod {
            id: MOD_ID.to_string(),
            title: "Litematica 0.26.3".to_string(),
            description: "Port of Litematica: .litematic/.nbt schematics, ghost block overlays, verifier & material list.".to_string(),
            keys: vec![
                ["open_menu".into(), "Open Litematica Menu".into(), "key.keyboard.l".into()],
                ["toggle_render".into(), "Toggle Schematic Render".into(), "key.keyboard.k".into()],
                ["layer_up".into(), "Layer Up".into(), "key.keyboard.page.up".into()],
                ["layer_down".into(), "Layer Down".into(), "key.keyboard.page.down".into()],
                ["set_pos1".into(), "Set Selection Pos1".into(), "key.keyboard.comma".into()],
                ["set_pos2".into(), "Set Selection Pos2".into(), "key.keyboard.period".into()],
            ],
        });
    }

    fn settings(&self, _host: &mut dyn Host, event: &mut SettingsEvent<'_>) {
        if event.mod_id != MOD_ID {
            return;
        }
        let Ok(st) = lock(&self.state) else { return };
        let on_off = |b: bool| if b { "ON" } else { "OFF" };
        event.buttons.extend([
            format!("Schematic Render: {}", on_off(st.config.render_enabled)),
            format!("Through Walls: {}", on_off(st.config.render_through_walls)),
            format!("Layer Mode: {}", st.config.layer_mode.label()),
            format!("Layer Y: {}", st.config.layer_y),
            format!("HUD Info: {}", on_off(st.config.show_hud)),
            "Open Litematica Menu...".to_string(),
        ]);
    }

    fn settings_click(&self, host: &mut dyn Host, event: &mut SettingsClickEvent<'_>) {
        if event.mod_id != MOD_ID {
            return;
        }
        let mut open_menu = false;
        if let Ok(mut st) = lock(&self.state) {
            if let Ok(dir) = host.config_dir() {
                st.ensure_config(dir);
            }
            match event.button {
                0 => {
                    st.config.render_enabled = !st.config.render_enabled;
                    st.bump_ghost_revision();
                }
                1 => st.config.render_through_walls = !st.config.render_through_walls,
                2 => {
                    st.config.layer_mode = st.config.layer_mode.next();
                    st.verification_dirty = true;
                    st.bump_ghost_revision();
                }
                3 => {
                    if let Some(p) = player_block_pos(host) {
                        st.config.layer_y = p[1];
                        st.verification_dirty = true;
                        st.bump_ghost_revision();
                    }
                }
                4 => st.config.show_hud = !st.config.show_hud,
                5 => open_menu = true,
                _ => {}
            }
            st.save_config();
        }
        if open_menu {
            self.queue(PendingAction::OpenScreen(ActiveScreen::MainMenu));
        }
    }

    fn key(&self, host: &mut dyn Host, event: &mut KeyEvent<'_>) {
        if !self.enabled || !event.pressed {
            return;
        }
        let Some(cs) = host.client_state() else { return };
        if cs.screen != NativeScreenKind::Playing {
            return;
        }

        let k_menu = host.mod_key(MOD_ID, "open_menu", "key.keyboard.l");
        let k_render = host.mod_key(MOD_ID, "toggle_render", "key.keyboard.k");
        let k_up = host.mod_key(MOD_ID, "layer_up", "key.keyboard.page.up");
        let k_down = host.mod_key(MOD_ID, "layer_down", "key.keyboard.page.down");
        let k_pos1 = host.mod_key(MOD_ID, "set_pos1", "key.keyboard.comma");
        let k_pos2 = host.mod_key(MOD_ID, "set_pos2", "key.keyboard.period");

        if event.key == k_menu {
            if let Ok(mut st) = lock(&self.state) {
                if let Ok(dir) = host.config_dir() {
                    st.ensure_config(dir);
                }
                st.active_screen = ActiveScreen::MainMenu;
                st.editing_save_name = false;
            }
            host.mod_screen_open(MOD_ID);
        } else if event.key == k_render {
            if let Ok(mut st) = lock(&self.state) {
                st.config.render_enabled = !st.config.render_enabled;
                let state_str = if st.config.render_enabled { "ON" } else { "OFF" };
                st.set_status(format!("Litematica Render: {state_str}"));
                st.bump_ghost_revision();
                st.save_config();
            }
        } else if event.key == k_up {
            if let Ok(mut st) = lock(&self.state) {
                st.config.layer_y += 1;
                let y = st.config.layer_y;
                st.verification_dirty = true;
                st.bump_ghost_revision();
                st.set_status(format!("Layer Y: {y}"));
                st.save_config();
            }
        } else if event.key == k_down {
            if let Ok(mut st) = lock(&self.state) {
                st.config.layer_y -= 1;
                let y = st.config.layer_y;
                st.verification_dirty = true;
                st.bump_ghost_revision();
                st.set_status(format!("Layer Y: {y}"));
                st.save_config();
            }
        } else if event.key == k_pos1 {
            if let Some(p) = player_block_pos(host)
                && let Ok(mut st) = lock(&self.state)
            {
                st.selection.pos1 = Some(p);
                st.set_status(format!("Pos1 set to ({}, {}, {})", p[0], p[1], p[2]));
            }
        } else if event.key == k_pos2 {
            if let Some(p) = player_block_pos(host)
                && let Ok(mut st) = lock(&self.state)
            {
                st.selection.pos2 = Some(p);
                st.set_status(format!("Pos2 set to ({}, {}, {})", p[0], p[1], p[2]));
            }
        }
    }

    fn frame(&self, host: &mut dyn Host, _event: &mut FrameEvent) {
        if !self.enabled {
            return;
        }

        // 1. Drain pending actions
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
                            st.active_screen = scr;
                            st.page = 0;
                            st.editing_save_name = false;
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
                        st.editing_save_name = false;
                    }
                    host.mod_screen_close(MOD_ID);
                }
                PendingAction::DownloadBytes(fname, bytes) => {
                    #[cfg(target_arch = "wasm32")]
                    horizon_os::web::download(&fname, &bytes);
                    #[cfg(not(target_arch = "wasm32"))]
                    {
                        let _ = (fname, bytes);
                    }
                }
                PendingAction::SaveSelection => {
                    if let Ok(mut st) = lock(&self.state) {
                        match capture_selection_to_schematic(host, &mut st) {
                            Ok((fname, bytes)) => {
                                st.set_status(format!("Saved {fname}"));
                                #[cfg(target_arch = "wasm32")]
                                horizon_os::web::download(&fname, &bytes);
                                #[cfg(not(target_arch = "wasm32"))]
                                let _ = bytes;
                            }
                            Err(e) => {
                                st.set_status(format!("Save failed: {e}"));
                            }
                        }
                    }
                }
            }
        }

        // 2. Poll browser file picker for imported .litematic / .nbt
        #[cfg(target_arch = "wasm32")]
        if let Some((fname, bytes)) = horizon_os::web::take_picked_named("litematic") {
            let place_at = player_block_pos(host);
            if let Ok(mut st) = lock(&self.state) {
                if let Ok(dir) = host.config_dir() {
                    st.ensure_config(dir);
                }
                match st.import_schematic_bytes(&fname, &bytes, place_at) {
                    Ok(title) => {
                        st.set_status(format!("Imported & placed: {title}"));
                    }
                    Err(e) => {
                        st.set_status(format!("Import error: {e}"));
                    }
                }
            }
        }

        // 3. Sync config, screen close, and periodic world verification
        if host.client_state().is_some_and(|cs| cs.screen == NativeScreenKind::Playing)
            && let Some(scr) = lock(&self.deferred_open).ok().and_then(|mut d| d.take())
            && host.mod_screen_open(MOD_ID)
        {
            if let Ok(mut st) = lock(&self.state) {
                st.active_screen = scr;
                st.page = 0;
                st.editing_save_name = false;
            }
        }
        let Ok(mut st) = lock(&self.state) else { return };
        st.frame_counter = st.frame_counter.wrapping_add(1);
        if let Ok(dir) = host.config_dir() {
            st.ensure_config(dir);
        }

        if st.active_screen != ActiveScreen::None && host.mod_screen() != Some(MOD_ID) {
            st.active_screen = ActiveScreen::None;
            st.editing_save_name = false;
        }

        if let Some(p) = player_block_pos(host)
            && p != st.last_player_block
        {
            st.last_player_block = p;
            if !st.placements.is_empty() {
                st.verification_dirty = true;
            }
        }

        if (st.verification_dirty || st.frame_counter % 25 == 0) && !st.placements.is_empty() {
            recompute_verification(host, &mut st);
        }
    }

    fn world_primitives(&self, _host: &dyn Host, event: &mut WorldPrimitivesEvent<'_>) {
        if !self.enabled {
            return;
        }
        let Ok(st) = lock(&self.state) else { return };
        if !st.config.render_enabled {
            return;
        }
        let through_walls = st.config.render_through_walls;

        // 1. Area selection box
        if st.config.render_area_selection {
            if let Some(p1) = st.selection.pos1 {
                let b = event.primitives.box_outline(
                    [p1[0] as f64, p1[1] as f64, p1[2] as f64],
                    [p1[0] as f64 + 1.0, p1[1] as f64 + 1.0, p1[2] as f64 + 1.0],
                    0xFF33_FF88,
                    2.5,
                );
                if through_walls {
                    b.through_walls();
                }
            }
            if let Some(p2) = st.selection.pos2 {
                let b = event.primitives.box_outline(
                    [p2[0] as f64, p2[1] as f64, p2[2] as f64],
                    [p2[0] as f64 + 1.0, p2[1] as f64 + 1.0, p2[2] as f64 + 1.0],
                    0xFF33_AAFF,
                    2.5,
                );
                if through_walls {
                    b.through_walls();
                }
            }
            if let (Some((min, max)), Some([sx, sy, sz])) =
                (st.selection.bounds(), st.selection.dimensions())
            {
                let b = event.primitives.box_outline(
                    [min[0] as f64, min[1] as f64, min[2] as f64],
                    [max[0] as f64 + 1.0, max[1] as f64 + 1.0, max[2] as f64 + 1.0],
                    0xFF44_FFAA,
                    2.0,
                );
                if through_walls {
                    b.through_walls();
                }
                event
                    .primitives
                    .text(
                        [
                            (min[0] + max[0]) as f64 * 0.5 + 0.5,
                            max[1] as f64 + 1.4,
                            (min[2] + max[2]) as f64 * 0.5 + 0.5,
                        ],
                        format!("{} ({}x{}x{})", st.selection.name, sx, sy, sz),
                        0xFF66_FFBB,
                        1.0,
                    )
                    .scale_with_distance()
                    .background();
            }
        }

        // 2. Placement bounding boxes
        if st.config.render_boxes {
            for (idx, pl) in st.placements.iter().enumerate() {
                if !pl.enabled {
                    continue;
                }
                let Some(schem) = st.schematics.iter().find(|s| s.file_name == pl.file_name) else {
                    continue;
                };
                let is_active = idx == st.active_placement;
                let color = if is_active { 0xFF66_CCFF } else { 0x88AA_BBCC };
                for reg in &schem.regions {
                    let p0 = pl.transform_pos(reg.position);
                    let p1 = pl.transform_pos([
                        reg.position[0] + reg.size[0] - 1,
                        reg.position[1] + reg.size[1] - 1,
                        reg.position[2] + reg.size[2] - 1,
                    ]);
                    let bmin = [
                        p0[0].min(p1[0]) as f64,
                        p0[1].min(p1[1]) as f64,
                        p0[2].min(p1[2]) as f64,
                    ];
                    let bmax = [
                        p0[0].max(p1[0]) as f64 + 1.0,
                        p0[1].max(p1[1]) as f64 + 1.0,
                        p0[2].max(p1[2]) as f64 + 1.0,
                    ];
                    let o = event.primitives.box_outline(bmin, bmax, color, 2.0);
                    if through_walls {
                        o.through_walls();
                    }
                }
                event
                    .primitives
                    .text(
                        [
                            pl.origin[0] as f64 + 0.5,
                            pl.origin[1] as f64 + 1.5,
                            pl.origin[2] as f64 + 0.5,
                        ],
                        format!("{} ({})", pl.name, pl.rotation.label()),
                        0xFF88_DDFF,
                        1.0,
                    )
                    .scale_with_distance()
                    .background();
            }
        }

        // 3. Verifier mismatch highlights (ghost block models are rendered via native_ghosts)
        if st.config.render_blocks {
            for rb in &st.verification.blocks {
                let (fill_col, outline_col) = match rb.kind {
                    BlockMismatchKind::Missing => continue,
                    BlockMismatchKind::WrongBlock => (0x66FF_2222, 0xFFFF_4422),
                    BlockMismatchKind::WrongState => (0x66FF_CC00, 0xFFFF_DD22),
                    BlockMismatchKind::Match => {
                        if !st.config.show_correct_blocks {
                            continue;
                        }
                        (0x1844_EE66, 0x6644_EE66)
                    }
                };
                // Slightly outside 0..1 so the tint wraps over existing solid blocks without z-fighting.
                let bmin = [
                    rb.pos[0] as f64 - 0.003,
                    rb.pos[1] as f64 - 0.003,
                    rb.pos[2] as f64 - 0.003,
                ];
                let bmax = [
                    rb.pos[0] as f64 + 1.003,
                    rb.pos[1] as f64 + 1.003,
                    rb.pos[2] as f64 + 1.003,
                ];
                let f = event.primitives.filled_box(bmin, bmax, fill_col);
                if through_walls {
                    f.through_walls();
                }
                let o = event.primitives.box_outline(bmin, bmax, outline_col, 2.0);
                if through_walls {
                    o.through_walls();
                }
            }
        }
    }

    fn hud(&self, host: &mut dyn Host, event: &mut HudEvent<'_>) {
        if !self.enabled {
            return;
        }
        let Some(cs) = host.client_state() else { return };
        if cs.gui_hidden || cs.debug_screen {
            return;
        }
        let Ok(st) = lock(&self.state) else { return };
        if !st.config.show_hud {
            return;
        }

        let mut lines: Vec<(String, u32)> = Vec::new();
        if let Some((msg, until)) = &st.status_message
            && st.frame_counter < *until
        {
            lines.push((msg.clone(), 0xFF77_EE99));
        }

        if let Some((pl, _)) = st.schematic_for_placement(st.active_placement)
            && pl.enabled
        {
            let v = &st.verification;
            let pct = if v.total_required > 0 {
                (v.total_placed as f32 / v.total_required as f32) * 100.0
            } else {
                0.0
            };
            lines.push((
                format!(
                    "Litematica: {} | Layer: {} (Y={})",
                    pl.name,
                    st.config.layer_mode.label(),
                    st.config.layer_y
                ),
                0xFF88_DDFF,
            ));
            lines.push((
                format!(
                    "Progress: {}/{} ({:.1}%) | Missing: {} | Wrong: {}",
                    v.total_placed, v.total_required, pct, v.total_missing, v.total_wrong
                ),
                0xFFFF_FFFF,
            ));
        } else if let Some([sx, sy, sz]) = st.selection.dimensions() {
            lines.push((
                format!("Selection: {}x{}x{} ({} blocks)", sx, sy, sz, st.selection.volume()),
                0xFF66_FFBB,
            ));
        }

        if lines.is_empty() {
            return;
        }

        let x = 6.0;
        let mut y = event.batch.height - 6.0 - (lines.len() as f32 * 11.0);
        for (line, col) in lines {
            let tw = host.text_width(&line);
            host.fill(event.batch, [x - 2.0, y - 1.0, tw + 4.0, 10.0], 0x8010_141C);
            host.text(event.batch, &line, [x, y], col, true);
            y += 11.0;
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

        let pw = 400.0f32.min(w - 20.0);
        let ph = 260.0f32.min(h - 20.0);
        let px = (w - pw) * 0.5;
        let py = (h - ph) * 0.5;
        host.fill(event.batch, [0.0, 0.0, w, h], 0x9000_0000);
        host.fill(event.batch, [px, py, pw, ph], 0xF014_1922);

        match st.active_screen {
            ActiveScreen::MainMenu => {
                host.text(
                    event.batch,
                    "Litematica (0.26.3 Web) - Main Menu",
                    [px + 12.0, py + 10.0],
                    0xFF88_DDFF,
                    true,
                );
                let bw = (pw - 32.0) * 0.5;
                let items = [
                    ("Import / Load Schematics", [px + 12.0, py + 34.0, bw, 24.0]),
                    ("Schematic Placements", [px + 20.0 + bw, py + 34.0, bw, 24.0]),
                    ("Material List", [px + 12.0, py + 66.0, bw, 24.0]),
                    ("Area Selection & Save", [px + 20.0 + bw, py + 66.0, bw, 24.0]),
                    ("Configuration", [px + 12.0, py + 98.0, bw, 24.0]),
                    (
                        if st.config.render_enabled {
                            "Render: ON (Click Toggle)"
                        } else {
                            "Render: OFF (Click Toggle)"
                        },
                        [px + 20.0 + bw, py + 98.0, bw, 24.0],
                    ),
                ];
                for (lbl, rect) in items {
                    draw_button(host, event.batch, rect, lbl, mouse, 0xCC1E_2836);
                }

                let info = format!(
                    "Loaded Schematics: {} | Active Placements: {} | Layer: {} (Y={})",
                    st.schematics.len(),
                    st.placements.len(),
                    st.config.layer_mode.label(),
                    st.config.layer_y
                );
                host.text(event.batch, &info, [px + 12.0, py + 142.0], 0xFFBB_CCDD, true);
                host.text(
                    event.batch,
                    "Keys: L=Menu | K=Render | PgUp/PgDn=Layer Y | ,/.=Pos1/Pos2",
                    [px + 12.0, py + 160.0],
                    0xFF88_99AA,
                    true,
                );
                draw_button(
                    host,
                    event.batch,
                    [px + pw - 80.0, py + ph - 30.0, 68.0, 20.0],
                    "Close",
                    mouse,
                    0xCC30_3848,
                );
            }
            ActiveScreen::LoadSchematics => {
                host.text(
                    event.batch,
                    "Schematics (Import .litematic / .nbt)",
                    [px + 12.0, py + 10.0],
                    0xFF88_DDFF,
                    true,
                );
                let total = st.schematics.len();
                let total_pages = (total.max(1) + 5) / 6;
                let page = st.page.min(total_pages.saturating_sub(1));
                let start = page * 6;
                for (row, schem) in st.schematics.iter().skip(start).take(6).enumerate() {
                    let ry = py + 32.0 + row as f32 * 28.0;
                    host.fill(event.batch, [px + 12.0, ry, pw - 24.0, 24.0], 0xB01D_2430);
                    let [sx, sy, sz] = schem.enclosing_size;
                    let label = format!("{} ({}x{}x{}, {} blk)", schem.name, sx, sy, sz, schem.total_blocks);
                    host.text(event.batch, &label, [px + 18.0, ry + 8.0], 0xFFFF_FFFF, true);
                    draw_button(
                        host,
                        event.batch,
                        [px + pw - 118.0, ry + 3.0, 62.0, 18.0],
                        "Place Here",
                        mouse,
                        0xCC28_5038,
                    );
                    draw_button(
                        host,
                        event.batch,
                        [px + pw - 52.0, ry + 3.0, 36.0, 18.0],
                        "Del",
                        mouse,
                        0xCC5A_2828,
                    );
                }
                draw_button(
                    host,
                    event.batch,
                    [px + 12.0, py + ph - 30.0, 128.0, 20.0],
                    "+ Import .litematic...",
                    mouse,
                    0xCC28_5A42,
                );
                draw_button(
                    host,
                    event.batch,
                    [px + 146.0, py + ph - 30.0, 44.0, 20.0],
                    "Prev",
                    mouse,
                    0xCC28_3040,
                );
                draw_button(
                    host,
                    event.batch,
                    [px + 194.0, py + ph - 30.0, 44.0, 20.0],
                    "Next",
                    mouse,
                    0xCC28_3040,
                );
                draw_button(
                    host,
                    event.batch,
                    [px + pw - 80.0, py + ph - 30.0, 68.0, 20.0],
                    "Back",
                    mouse,
                    0xCC30_3848,
                );
            }
            ActiveScreen::Placements => {
                host.text(
                    event.batch,
                    "Schematic Placements",
                    [px + 12.0, py + 10.0],
                    0xFF88_DDFF,
                    true,
                );
                let total = st.placements.len();
                let total_pages = (total.max(1) + 5) / 6;
                let page = st.page.min(total_pages.saturating_sub(1));
                let start = page * 6;
                for (row, pl) in st.placements.iter().skip(start).take(6).enumerate() {
                    let ry = py + 32.0 + row as f32 * 28.0;
                    let idx = start + row;
                    let bg = if idx == st.active_placement {
                        0xC024_344A
                    } else {
                        0xB01D_2430
                    };
                    host.fill(event.batch, [px + 12.0, ry, pw - 24.0, 24.0], bg);
                    let on_str = if pl.enabled { "ON" } else { "OFF" };
                    let info = format!(
                        "{} [{on_str}] ({},{},{}) {}",
                        pl.name,
                        pl.origin[0],
                        pl.origin[1],
                        pl.origin[2],
                        pl.rotation.label()
                    );
                    host.text(event.batch, &info, [px + 16.0, ry + 8.0], 0xFFFF_FFFF, true);
                    draw_button(host, event.batch, [px + pw - 190.0, ry + 3.0, 34.0, 18.0], "On/Off", mouse, 0xCC2E_3748);
                    draw_button(host, event.batch, [px + pw - 152.0, ry + 3.0, 34.0, 18.0], "Move", mouse, 0xCC2E_4458);
                    draw_button(host, event.batch, [px + pw - 114.0, ry + 3.0, 34.0, 18.0], "Rot", mouse, 0xCC38_3850);
                    draw_button(host, event.batch, [px + pw - 76.0, ry + 3.0, 34.0, 18.0], "Mir", mouse, 0xCC38_3850);
                    draw_button(host, event.batch, [px + pw - 38.0, ry + 3.0, 22.0, 18.0], "X", mouse, 0xCC5A_2828);
                }
                draw_button(
                    host,
                    event.batch,
                    [px + 12.0, py + ph - 30.0, 108.0, 20.0],
                    &format!("Layer: {}", st.config.layer_mode.label()),
                    mouse,
                    0xCC28_3040,
                );
                draw_button(
                    host,
                    event.batch,
                    [px + 126.0, py + ph - 30.0, 34.0, 20.0],
                    "Y-",
                    mouse,
                    0xCC28_3040,
                );
                draw_button(
                    host,
                    event.batch,
                    [px + 164.0, py + ph - 30.0, 34.0, 20.0],
                    "Y+",
                    mouse,
                    0xCC28_3040,
                );
                draw_button(
                    host,
                    event.batch,
                    [px + 204.0, py + ph - 30.0, 44.0, 20.0],
                    "Prev",
                    mouse,
                    0xCC28_3040,
                );
                draw_button(
                    host,
                    event.batch,
                    [px + 252.0, py + ph - 30.0, 44.0, 20.0],
                    "Next",
                    mouse,
                    0xCC28_3040,
                );
                draw_button(
                    host,
                    event.batch,
                    [px + pw - 80.0, py + ph - 30.0, 68.0, 20.0],
                    "Back",
                    mouse,
                    0xCC30_3848,
                );
            }
            ActiveScreen::MaterialList => {
                let v = &st.verification;
                let header = format!(
                    "Material List (Placed {}/{} | Missing {} | Wrong {})",
                    v.total_placed, v.total_required, v.total_missing, v.total_wrong
                );
                host.text(event.batch, &header, [px + 12.0, py + 10.0], 0xFF88_DDFF, true);
                let total = v.materials.len();
                let total_pages = (total.max(1) + 7) / 8;
                let page = st.page.min(total_pages.saturating_sub(1));
                for (row, mat) in v.materials.iter().skip(page * 8).take(8).enumerate() {
                    let ry = py + 30.0 + row as f32 * 22.0;
                    host.fill(event.batch, [px + 12.0, ry, pw - 24.0, 19.0], 0xB01D_2430);
                    let short_name = mat.block_name.strip_prefix("minecraft:").unwrap_or(&mat.block_name);
                    let line = format!(
                        "{:<24} Tot:{:<5} Ok:{:<5} Miss:{:<5} Bad:{}",
                        short_name, mat.total, mat.placed, mat.missing, mat.wrong
                    );
                    let col = if mat.missing > 0 { 0xFFFF_BB66 } else { 0xFF77_EE99 };
                    host.text(event.batch, &line, [px + 16.0, ry + 5.0], col, true);
                }
                draw_button(
                    host,
                    event.batch,
                    [px + 12.0, py + ph - 30.0, 96.0, 20.0],
                    "Download CSV",
                    mouse,
                    0xCC28_5038,
                );
                draw_button(
                    host,
                    event.batch,
                    [px + 114.0, py + ph - 30.0, 48.0, 20.0],
                    "Prev",
                    mouse,
                    0xCC28_3040,
                );
                draw_button(
                    host,
                    event.batch,
                    [px + 168.0, py + ph - 30.0, 48.0, 20.0],
                    "Next",
                    mouse,
                    0xCC28_3040,
                );
                draw_button(
                    host,
                    event.batch,
                    [px + pw - 80.0, py + ph - 30.0, 68.0, 20.0],
                    "Back",
                    mouse,
                    0xCC30_3848,
                );
            }
            ActiveScreen::AreaSelection => {
                host.text(
                    event.batch,
                    "Area Selection & Save .litematic",
                    [px + 12.0, py + 10.0],
                    0xFF88_DDFF,
                    true,
                );
                let p1s = st
                    .selection
                    .pos1
                    .map_or("Not set".into(), |[x, y, z]| format!("({}, {}, {})", x, y, z));
                let p2s = st
                    .selection
                    .pos2
                    .map_or("Not set".into(), |[x, y, z]| format!("({}, {}, {})", x, y, z));
                let dims = st
                    .selection
                    .dimensions()
                    .map_or("N/A".into(), |[x, y, z]| format!("{} x {} x {} ({} vol)", x, y, z, st.selection.volume()));
                host.text(event.batch, &format!("Pos1: {p1s}"), [px + 12.0, py + 34.0], 0xFF66_FFBB, true);
                host.text(event.batch, &format!("Pos2: {p2s}"), [px + 12.0, py + 50.0], 0xFF66_CCFF, true);
                host.text(event.batch, &format!("Size: {dims}"), [px + 12.0, py + 66.0], 0xFFFF_FFFF, true);

                draw_button(host, event.batch, [px + 12.0, py + 88.0, 112.0, 22.0], "Set Pos1 (Here)", mouse, 0xCC28_4A38);
                draw_button(host, event.batch, [px + 132.0, py + 88.0, 112.0, 22.0], "Set Pos2 (Here)", mouse, 0xCC28_3E58);
                draw_button(host, event.batch, [px + 252.0, py + 88.0, 80.0, 22.0], "Clear", mouse, 0xCC4A_2828);

                let name_bg = if st.editing_save_name { 0xFF1A_2638 } else { 0xFF0E_121A };
                let cur = if st.editing_save_name { "_" } else { "" };
                host.fill(event.batch, [px + 12.0, py + 126.0, pw - 24.0, 22.0], name_bg);
                host.text(
                    event.batch,
                    &format!("Name: {}{}", st.save_name_input, cur),
                    [px + 18.0, py + 133.0],
                    0xFFFF_FFFF,
                    true,
                );
                draw_button(
                    host,
                    event.batch,
                    [px + 12.0, py + 158.0, 180.0, 24.0],
                    "Save & Download .litematic",
                    mouse,
                    0xCC28_5A42,
                );
                draw_button(
                    host,
                    event.batch,
                    [px + pw - 80.0, py + ph - 30.0, 68.0, 20.0],
                    "Back",
                    mouse,
                    0xCC30_3848,
                );
            }
            ActiveScreen::Config => {
                host.text(
                    event.batch,
                    "Litematica Configuration",
                    [px + 12.0, py + 10.0],
                    0xFF88_DDFF,
                    true,
                );
                let on_off = |b: bool| if b { "ON" } else { "OFF" };
                let items = [
                    format!("Master Render: {}", on_off(st.config.render_enabled)),
                    format!("Render Bounding Boxes: {}", on_off(st.config.render_boxes)),
                    format!("Render Ghost/Overlay Blocks: {}", on_off(st.config.render_blocks)),
                    format!("Render Through Walls: {}", on_off(st.config.render_through_walls)),
                    format!("Show Correct Blocks: {}", on_off(st.config.show_correct_blocks)),
                    format!("Layer Mode: {}", st.config.layer_mode.label()),
                    format!("Show HUD Overlay: {}", on_off(st.config.show_hud)),
                    format!("Max Render Blocks: {}", st.config.max_render_blocks),
                ];
                for (i, lbl) in items.iter().enumerate() {
                    let ry = py + 32.0 + i as f32 * 22.0;
                    draw_button(host, event.batch, [px + 12.0, ry, pw - 24.0, 19.0], lbl, mouse, 0xCC1E_2836);
                }
                draw_button(
                    host,
                    event.batch,
                    [px + pw - 80.0, py + ph - 30.0, 68.0, 20.0],
                    "Back",
                    mouse,
                    0xCC30_3848,
                );
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
        if event.button != 0 {
            return Flow::Cancel;
        }
        let mx = event.screen.mouse[0];
        let my = event.screen.mouse[1];
        let w = event.screen.rect[2];
        let h = event.screen.rect[3];
        let pw = 400.0f32.min(w - 20.0);
        let ph = 260.0f32.min(h - 20.0);
        let px = (w - pw) * 0.5;
        let py = (h - ph) * 0.5;

        let Ok(mut st) = lock(&self.state) else {
            return Flow::Cancel;
        };

        match st.active_screen {
            ActiveScreen::MainMenu => {
                let bw = (pw - 32.0) * 0.5;
                if in_rect(mx, my, [px + 12.0, py + 34.0, bw, 24.0]) {
                    st.active_screen = ActiveScreen::LoadSchematics;
                    st.page = 0;
                } else if in_rect(mx, my, [px + 20.0 + bw, py + 34.0, bw, 24.0]) {
                    st.active_screen = ActiveScreen::Placements;
                    st.page = 0;
                } else if in_rect(mx, my, [px + 12.0, py + 66.0, bw, 24.0]) {
                    st.active_screen = ActiveScreen::MaterialList;
                    st.page = 0;
                } else if in_rect(mx, my, [px + 20.0 + bw, py + 66.0, bw, 24.0]) {
                    st.active_screen = ActiveScreen::AreaSelection;
                } else if in_rect(mx, my, [px + 12.0, py + 98.0, bw, 24.0]) {
                    st.active_screen = ActiveScreen::Config;
                } else if in_rect(mx, my, [px + 20.0 + bw, py + 98.0, bw, 24.0]) {
                    st.config.render_enabled = !st.config.render_enabled;
                    st.bump_ghost_revision();
                    st.save_config();
                } else if in_rect(mx, my, [px + pw - 80.0, py + ph - 30.0, 68.0, 20.0]) {
                    self.queue(PendingAction::CloseScreen);
                }
            }
            ActiveScreen::LoadSchematics => {
                if in_rect(mx, my, [px + 12.0, py + ph - 30.0, 128.0, 20.0]) {
                    #[cfg(target_arch = "wasm32")]
                    horizon_os::web::pick_file("litematic", ".litematic,.nbt");
                    return Flow::Cancel;
                }
                let total = st.schematics.len();
                let total_pages = (total.max(1) + 5) / 6;
                let page = st.page.min(total_pages.saturating_sub(1));
                if in_rect(mx, my, [px + 146.0, py + ph - 30.0, 44.0, 20.0]) {
                    st.page = page.saturating_sub(1);
                    return Flow::Cancel;
                }
                if in_rect(mx, my, [px + 194.0, py + ph - 30.0, 44.0, 20.0]) {
                    if page + 1 < total_pages {
                        st.page = page + 1;
                    }
                    return Flow::Cancel;
                }
                if in_rect(mx, my, [px + pw - 80.0, py + ph - 30.0, 68.0, 20.0]) {
                    st.active_screen = ActiveScreen::MainMenu;
                    return Flow::Cancel;
                }
                let start = page * 6;
                let visible = total.saturating_sub(start).min(6);
                for row in 0..visible {
                    let i = start + row;
                    let ry = py + 32.0 + row as f32 * 28.0;
                    if in_rect(mx, my, [px + pw - 118.0, ry + 3.0, 62.0, 18.0]) {
                        let origin = player_block_pos(host).unwrap_or([0, 64, 0]);
                        let s = &st.schematics[i];
                        let pl = SchematicPlacement::new(
                            s.name.clone(),
                            s.file_name.clone(),
                            origin,
                        );
                        st.placements.push(pl);
                        st.active_placement = st.placements.len() - 1;
                        st.verification_dirty = true;
                        st.bump_ghost_revision();
                        st.save_placements();
                        st.active_screen = ActiveScreen::Placements;
                        return Flow::Cancel;
                    }
                    if in_rect(mx, my, [px + pw - 52.0, ry + 3.0, 36.0, 18.0]) {
                        st.delete_schematic(i);
                        return Flow::Cancel;
                    }
                }
            }
            ActiveScreen::Placements => {
                if in_rect(mx, my, [px + 12.0, py + ph - 30.0, 108.0, 20.0]) {
                    st.config.layer_mode = st.config.layer_mode.next();
                    st.verification_dirty = true;
                    st.bump_ghost_revision();
                    st.save_config();
                    return Flow::Cancel;
                }
                if in_rect(mx, my, [px + 126.0, py + ph - 30.0, 34.0, 20.0]) {
                    st.config.layer_y -= 1;
                    st.verification_dirty = true;
                    st.bump_ghost_revision();
                    st.save_config();
                    return Flow::Cancel;
                }
                if in_rect(mx, my, [px + 164.0, py + ph - 30.0, 34.0, 20.0]) {
                    st.config.layer_y += 1;
                    st.verification_dirty = true;
                    st.bump_ghost_revision();
                    st.save_config();
                    return Flow::Cancel;
                }
                let total = st.placements.len();
                let total_pages = (total.max(1) + 5) / 6;
                let page = st.page.min(total_pages.saturating_sub(1));
                if in_rect(mx, my, [px + 204.0, py + ph - 30.0, 44.0, 20.0]) {
                    st.page = page.saturating_sub(1);
                    return Flow::Cancel;
                }
                if in_rect(mx, my, [px + 252.0, py + ph - 30.0, 44.0, 20.0]) {
                    if page + 1 < total_pages {
                        st.page = page + 1;
                    }
                    return Flow::Cancel;
                }
                if in_rect(mx, my, [px + pw - 80.0, py + ph - 30.0, 68.0, 20.0]) {
                    st.active_screen = ActiveScreen::MainMenu;
                    return Flow::Cancel;
                }
                let start = page * 6;
                let visible = total.saturating_sub(start).min(6);
                for row in 0..visible {
                    let i = start + row;
                    let ry = py + 32.0 + row as f32 * 28.0;
                    if in_rect(mx, my, [px + pw - 190.0, ry + 3.0, 34.0, 18.0]) {
                        st.placements[i].enabled = !st.placements[i].enabled;
                        st.active_placement = i;
                        st.verification_dirty = true;
                        st.bump_ghost_revision();
                        st.save_placements();
                        return Flow::Cancel;
                    }
                    if in_rect(mx, my, [px + pw - 152.0, ry + 3.0, 34.0, 18.0]) {
                        if let Some(p) = player_block_pos(host) {
                            st.placements[i].origin = p;
                            st.active_placement = i;
                            st.verification_dirty = true;
                            st.bump_ghost_revision();
                        st.save_placements();
                        }
                        return Flow::Cancel;
                    }
                    if in_rect(mx, my, [px + pw - 114.0, ry + 3.0, 34.0, 18.0]) {
                        st.placements[i].rotation = st.placements[i].rotation.next();
                        st.active_placement = i;
                        st.verification_dirty = true;
                        st.bump_ghost_revision();
                        st.save_placements();
                        return Flow::Cancel;
                    }
                    if in_rect(mx, my, [px + pw - 76.0, ry + 3.0, 34.0, 18.0]) {
                        st.placements[i].mirror = st.placements[i].mirror.next();
                        st.active_placement = i;
                        st.verification_dirty = true;
                        st.bump_ghost_revision();
                        st.save_placements();
                        return Flow::Cancel;
                    }
                    if in_rect(mx, my, [px + pw - 38.0, ry + 3.0, 22.0, 18.0]) {
                        st.placements.remove(i);
                        if st.active_placement >= st.placements.len() {
                            st.active_placement = st.placements.len().saturating_sub(1);
                        }
                        st.verification_dirty = true;
                        st.bump_ghost_revision();
                        st.save_placements();
                        return Flow::Cancel;
                    }
                    if in_rect(mx, my, [px + 12.0, ry, pw - 204.0, 24.0]) {
                        st.active_placement = i;
                        st.verification_dirty = true;
                        return Flow::Cancel;
                    }
                }
            }
            ActiveScreen::MaterialList => {
                if in_rect(mx, my, [px + 12.0, py + ph - 30.0, 96.0, 20.0]) {
                    let csv = st.verification.to_csv();
                    self.queue(PendingAction::DownloadBytes(
                        "material_list.csv".to_string(),
                        csv.into_bytes(),
                    ));
                    return Flow::Cancel;
                }
                let total = st.verification.materials.len();
                let total_pages = (total.max(1) + 7) / 8;
                let page = st.page.min(total_pages.saturating_sub(1));
                if in_rect(mx, my, [px + 114.0, py + ph - 30.0, 48.0, 20.0]) {
                    st.page = page.saturating_sub(1);
                } else if in_rect(mx, my, [px + 168.0, py + ph - 30.0, 48.0, 20.0]) {
                    if page + 1 < total_pages {
                        st.page = page + 1;
                    }
                } else if in_rect(mx, my, [px + pw - 80.0, py + ph - 30.0, 68.0, 20.0]) {
                    st.active_screen = ActiveScreen::MainMenu;
                }
            }
            ActiveScreen::AreaSelection => {
                st.editing_save_name = in_rect(mx, my, [px + 12.0, py + 126.0, pw - 24.0, 22.0]);
                if in_rect(mx, my, [px + 12.0, py + 88.0, 112.0, 22.0]) {
                    if let Some(p) = player_block_pos(host) {
                        st.selection.pos1 = Some(p);
                    }
                } else if in_rect(mx, my, [px + 132.0, py + 88.0, 112.0, 22.0]) {
                    if let Some(p) = player_block_pos(host) {
                        st.selection.pos2 = Some(p);
                    }
                } else if in_rect(mx, my, [px + 252.0, py + 88.0, 80.0, 22.0]) {
                    st.selection.clear();
                } else if in_rect(mx, my, [px + 12.0, py + 158.0, 180.0, 24.0]) {
                    self.queue(PendingAction::SaveSelection);
                } else if in_rect(mx, my, [px + pw - 80.0, py + ph - 30.0, 68.0, 20.0]) {
                    st.active_screen = ActiveScreen::MainMenu;
                }
            }
            ActiveScreen::Config => {
                if in_rect(mx, my, [px + pw - 80.0, py + ph - 30.0, 68.0, 20.0]) {
                    st.active_screen = ActiveScreen::MainMenu;
                    return Flow::Cancel;
                }
                for i in 0..8 {
                    let ry = py + 32.0 + i as f32 * 22.0;
                    if in_rect(mx, my, [px + 12.0, ry, pw - 24.0, 19.0]) {
                        match i {
                            0 => st.config.render_enabled = !st.config.render_enabled,
                            1 => st.config.render_boxes = !st.config.render_boxes,
                            2 => st.config.render_blocks = !st.config.render_blocks,
                            3 => st.config.render_through_walls = !st.config.render_through_walls,
                            4 => st.config.show_correct_blocks = !st.config.show_correct_blocks,
                            5 => {
                                st.config.layer_mode = st.config.layer_mode.next();
                                st.verification_dirty = true;
                            }
                            6 => st.config.show_hud = !st.config.show_hud,
                            7 => {
                                st.config.max_render_blocks = match st.config.max_render_blocks {
                                    1000 => 2000,
                                    2000 => 4000,
                                    4000 => 8000,
                                    _ => 1000,
                                };
                                st.verification_dirty = true;
                            }
                            _ => {}
                        }
                        st.bump_ghost_revision();
                    st.save_config();
                        return Flow::Cancel;
                    }
                }
            }
            ActiveScreen::None => {}
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
            if st.active_screen != ActiveScreen::MainMenu {
                st.active_screen = ActiveScreen::MainMenu;
                st.editing_save_name = false;
                return Flow::Cancel;
            }
            st.active_screen = ActiveScreen::None;
            st.editing_save_name = false;
            return Flow::Continue;
        }
        if st.active_screen == ActiveScreen::AreaSelection
            && st.editing_save_name
            && event.key == "key.keyboard.backspace"
        {
            st.save_name_input.pop();
            return Flow::Cancel;
        }
        let k_menu = host.mod_key(MOD_ID, "open_menu", "key.keyboard.l");
        if event.key == k_menu && !st.editing_save_name {
            self.queue(PendingAction::CloseScreen);
            return Flow::Cancel;
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
        if let Ok(mut st) = lock(&self.state)
            && st.active_screen == ActiveScreen::AreaSelection
            && st.editing_save_name
            && !event.c.is_control()
            && st.save_name_input.chars().count() < 32
            && (event.c.is_ascii_alphanumeric() || matches!(event.c, '_' | '-' | '.'))
        {
            st.save_name_input.push(event.c);
        }
        Flow::Cancel
    }
}
