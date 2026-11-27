#![forbid(unsafe_code)]

use serde::Deserialize;
use std::collections::HashMap;
use std::sync::OnceLock;

/// Original Xaero entity icon definition JSONs embedded from `xaerominimap`.
const DEF_AXOLOTL: &str = include_str!("../assets/entity_icons/axolotl.json");
const DEF_CAT: &str = include_str!("../assets/entity_icons/cat.json");
const DEF_ENDERMAN: &str = include_str!("../assets/entity_icons/enderman.json");
const DEF_HORSE: &str = include_str!("../assets/entity_icons/horse.json");
const DEF_IRON_GOLEM: &str = include_str!("../assets/entity_icons/iron_golem.json");
const DEF_LLAMA: &str = include_str!("../assets/entity_icons/llama.json");
const DEF_PIG: &str = include_str!("../assets/entity_icons/pig.json");
const DEF_STRIDER: &str = include_str!("../assets/entity_icons/strider.json");
const DEF_TRADER_LLAMA: &str = include_str!("../assets/entity_icons/trader_llama.json");
const DEF_TROPICAL_FISH: &str = include_str!("../assets/entity_icons/tropical_fish.json");
const DEF_VILLAGER: &str = include_str!("../assets/entity_icons/villager.json");
const DEF_WOLF: &str = include_str!("../assets/entity_icons/wolf.json");
const DEF_ZOMBIE_VILLAGER: &str = include_str!("../assets/entity_icons/zombie_villager.json");

#[derive(Debug, Deserialize)]
struct EntityIconDef {
    #[serde(default)]
    variants: HashMap<String, String>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RadarCategory {
    Player,
    Hostile,
    Passive,
    Item,
    Other,
}

impl RadarCategory {
    pub fn argb(self) -> u32 {
        match self {
            Self::Player => 0xFFFF_FFFF,
            Self::Hostile => 0xFFFF_3B30,
            Self::Passive => 0xFFFF_E633,
            Self::Item => 0xFF55_FF55,
            Self::Other => 0xFF55_FFFF,
        }
    }
}

static ICON_DEFS: OnceLock<HashMap<&'static str, EntityIconDef>> = OnceLock::new();

fn icon_defs() -> &'static HashMap<&'static str, EntityIconDef> {
    ICON_DEFS.get_or_init(|| {
        let pairs = [
            ("minecraft:axolotl", DEF_AXOLOTL),
            ("minecraft:cat", DEF_CAT),
            ("minecraft:enderman", DEF_ENDERMAN),
            ("minecraft:horse", DEF_HORSE),
            ("minecraft:iron_golem", DEF_IRON_GOLEM),
            ("minecraft:llama", DEF_LLAMA),
            ("minecraft:pig", DEF_PIG),
            ("minecraft:strider", DEF_STRIDER),
            ("minecraft:trader_llama", DEF_TRADER_LLAMA),
            ("minecraft:tropical_fish", DEF_TROPICAL_FISH),
            ("minecraft:villager", DEF_VILLAGER),
            ("minecraft:wolf", DEF_WOLF),
            ("minecraft:zombie_villager", DEF_ZOMBIE_VILLAGER),
        ];
        let mut map = HashMap::new();
        for (id, raw) in pairs {
            if let Ok(parsed) = serde_json::from_str::<EntityIconDef>(raw) {
                let _ = parsed.variants.len();
                map.insert(id, parsed);
            }
        }
        map
    })
}

pub fn has_icon_definition(kind: &str) -> bool {
    icon_defs().contains_key(kind)
}

pub fn classify_entity(kind: &str, is_player: bool) -> RadarCategory {
    if is_player || kind == "minecraft:player" {
        return RadarCategory::Player;
    }
    if kind == "minecraft:item" {
        return RadarCategory::Item;
    }
    match kind {
        "minecraft:zombie"
        | "minecraft:zombie_villager"
        | "minecraft:husk"
        | "minecraft:drowned"
        | "minecraft:skeleton"
        | "minecraft:stray"
        | "minecraft:bogged"
        | "minecraft:wither_skeleton"
        | "minecraft:creeper"
        | "minecraft:spider"
        | "minecraft:cave_spider"
        | "minecraft:enderman"
        | "minecraft:witch"
        | "minecraft:slime"
        | "minecraft:magma_cube"
        | "minecraft:blaze"
        | "minecraft:ghast"
        | "minecraft:hoglin"
        | "minecraft:piglin"
        | "minecraft:piglin_brute"
        | "minecraft:zombified_piglin"
        | "minecraft:pillager"
        | "minecraft:vindicator"
        | "minecraft:evoker"
        | "minecraft:ravager"
        | "minecraft:vex"
        | "minecraft:phantom"
        | "minecraft:guardian"
        | "minecraft:elder_guardian"
        | "minecraft:shulker"
        | "minecraft:silverfish"
        | "minecraft:endermite"
        | "minecraft:warden"
        | "minecraft:breeze"
        | "minecraft:creaking"
        | "minecraft:wither"
        | "minecraft:ender_dragon" => RadarCategory::Hostile,
        "minecraft:cow"
        | "minecraft:mooshroom"
        | "minecraft:pig"
        | "minecraft:sheep"
        | "minecraft:chicken"
        | "minecraft:horse"
        | "minecraft:donkey"
        | "minecraft:mule"
        | "minecraft:llama"
        | "minecraft:trader_llama"
        | "minecraft:wolf"
        | "minecraft:cat"
        | "minecraft:ocelot"
        | "minecraft:fox"
        | "minecraft:rabbit"
        | "minecraft:parrot"
        | "minecraft:bee"
        | "minecraft:villager"
        | "minecraft:wandering_trader"
        | "minecraft:iron_golem"
        | "minecraft:snow_golem"
        | "minecraft:turtle"
        | "minecraft:axolotl"
        | "minecraft:frog"
        | "minecraft:goat"
        | "minecraft:camel"
        | "minecraft:sniffer"
        | "minecraft:armadillo"
        | "minecraft:squid"
        | "minecraft:glow_squid"
        | "minecraft:dolphin"
        | "minecraft:cod"
        | "minecraft:salmon"
        | "minecraft:tropical_fish"
        | "minecraft:pufferfish"
        | "minecraft:bat"
        | "minecraft:strider"
        | "minecraft:allay"
        | "minecraft:happy_ghast" => RadarCategory::Passive,
        _ => RadarCategory::Other,
    }
}

#[derive(Clone, Debug)]
pub struct RadarBlip {
    pub x: f64,
    pub y: f64,
    pub z: f64,
    pub yaw: f32,
    pub label: Option<String>,
    pub category: RadarCategory,
}
