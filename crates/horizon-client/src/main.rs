//! Horizon: native Rust client for Minecraft Java Edition 26.3 gameplay.
#![forbid(unsafe_code)]

#[cfg(not(feature = "system-allocator"))]
#[global_allocator]
static ALLOCATOR: horizon_threads::GameAllocator = horizon_threads::GAME_ALLOCATOR;

/// The mod API level this game serves, kept in the program for the launcher
/// to read without running it (`horizon_mods::api_level::MARK`). `main`
/// reads it, so the linker keeps it.
static MOD_API_LEVEL: [u8; 27] = horizon_mods::api_level::MARK;

fn main() {
    // The page starts the game once it has installed the game's files.
    #[cfg(target_arch = "wasm32")]
    horizon_os::web::on_start(start);
    #[cfg(not(target_arch = "wasm32"))]
    start();
}


fn read_enabled_mods() -> (bool, bool, bool) {
    let text = horizon_os::fs::read_to_string("config/xaero/mods_enabled.json")
        .or_else(|_| horizon_os::fs::read_to_string("/config/xaero/mods_enabled.json"));
    let Ok(raw) = text else {
        return (true, true, true);
    };
    let Ok(val) = serde_json::from_str::<serde_json::Value>(&raw) else {
        return (true, true, true);
    };
    let minimap = val.get("minimap").and_then(|v| v.as_bool()).unwrap_or(true);
    let worldmap = val.get("worldmap").and_then(|v| v.as_bool()).unwrap_or(true);
    let litematica = val.get("litematica").and_then(|v| v.as_bool()).unwrap_or(true);
    (minimap, worldmap, litematica)
}

fn start() {
    std::hint::black_box(&MOD_API_LEVEL);
    let (enable_minimap, enable_worldmap, enable_litematica) = read_enabled_mods();
    let has_xaero = enable_minimap || enable_worldmap;
    match (has_xaero, enable_litematica) {
        (true, true) => horizon_client::mod_api::name_mods(&[
            horizon_client::xaero_hooks::MOD_ID,
            horizon_client::litematica_hooks::MOD_ID,
        ]),
        (true, false) => {
            horizon_client::mod_api::name_mods(&[horizon_client::xaero_hooks::MOD_ID])
        }
        (false, true) => {
            horizon_client::mod_api::name_mods(&[horizon_client::litematica_hooks::MOD_ID])
        }
        (false, false) => horizon_client::mod_api::name_mods(&[]),
    }
    let mut mods: Vec<Box<dyn horizon_client::mod_api::Hooks>> = Vec::new();
    if has_xaero {
        mods.push(Box::new(horizon_client::xaero_hooks::XaeroMod::new(
            enable_minimap,
            enable_worldmap,
        )));
    }
    if enable_litematica {
        mods.push(Box::new(
            horizon_client::litematica_hooks::LitematicaMod::new(true),
        ));
    }
    horizon_client::entry::main_with(std::sync::Arc::from(mods));
}
