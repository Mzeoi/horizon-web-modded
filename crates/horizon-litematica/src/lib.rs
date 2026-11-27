#![forbid(unsafe_code)]

pub mod config;
pub mod placement;
pub mod schematic;
pub mod selection;
pub mod verifier;

use config::LitematicaConfig;
use placement::SchematicPlacement;
use schematic::Schematic;
use selection::AreaSelection;
use std::path::PathBuf;
use verifier::VerificationResult;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ActiveScreen {
    None,
    MainMenu,
    LoadSchematics,
    Placements,
    MaterialList,
    AreaSelection,
    Config,
}

pub struct LitematicaState {
    pub config: LitematicaConfig,
    pub config_dir: Option<PathBuf>,
    pub schematics: Vec<Schematic>,
    pub placements: Vec<SchematicPlacement>,
    pub active_placement: usize,
    pub selection: AreaSelection,
    pub verification: VerificationResult,
    pub verification_dirty: bool,
    pub active_screen: ActiveScreen,
    pub page: usize,
    pub status_message: Option<(String, u64)>,
    pub frame_counter: u64,
    pub last_player_block: [i32; 3],
    pub save_name_input: String,
    pub editing_save_name: bool,
    pub ghost_revision: u64,
}

impl LitematicaState {
    pub fn new() -> Self {
        Self {
            config: LitematicaConfig::default(),
            config_dir: None,
            schematics: Vec::new(),
            placements: Vec::new(),
            active_placement: 0,
            selection: AreaSelection::new("Selection"),
            verification: VerificationResult::default(),
            verification_dirty: true,
            active_screen: ActiveScreen::None,
            page: 0,
            status_message: None,
            frame_counter: 0,
            last_player_block: [i32::MIN, i32::MIN, i32::MIN],
            save_name_input: "my_build".to_string(),
            editing_save_name: false,
            ghost_revision: 1,
        }
    }

    pub fn bump_ghost_revision(&mut self) {
        self.ghost_revision = self.ghost_revision.wrapping_add(1).max(1);
    }

    pub fn ensure_config(&mut self, dir: PathBuf) {
        if self.config_dir.is_some() {
            return;
        }
        self.config = LitematicaConfig::load(&dir);
        self.config_dir = Some(dir.clone());
        self.reload_schematics_from_disk();
        self.load_placements();
    }

    pub fn schematics_dir(&self) -> Option<PathBuf> {
        self.config_dir
            .as_ref()
            .map(|d| d.join("litematica").join("schematics"))
    }

    pub fn save_config(&self) {
        if let Some(dir) = &self.config_dir {
            self.config.save(dir);
        }
    }

    pub fn set_status(&mut self, msg: impl Into<String>) {
        self.status_message = Some((msg.into(), self.frame_counter + 240));
    }

    pub fn save_placements(&self) {
        let Some(dir) = &self.config_dir else { return };
        let folder = dir.join("litematica");
        let _ = horizon_os::fs::create_dir_all(&folder);
        if let Ok(json) = serde_json::to_string_pretty(&self.placements) {
            let _ = horizon_os::fs::write(folder.join("placements.json"), json);
        }
    }

    pub fn load_placements(&mut self) {
        let Some(dir) = &self.config_dir else { return };
        let path = dir.join("litematica").join("placements.json");
        if let Ok(text) = horizon_os::fs::read_to_string(&path)
            && let Ok(list) = serde_json::from_str::<Vec<SchematicPlacement>>(&text)
        {
            self.placements = list;
            self.verification_dirty = true;
            self.bump_ghost_revision();
        }
    }

    pub fn reload_schematics_from_disk(&mut self) {
        let Some(sdir) = self.schematics_dir() else {
            return;
        };
        let _ = horizon_os::fs::create_dir_all(&sdir);
        let Ok(entries) = horizon_os::fs::read_dir(&sdir) else {
            return;
        };
        let mut loaded = Vec::new();
        let mut paths: Vec<PathBuf> = entries.flatten().map(|e| e.path()).collect();
        paths.sort();
        for path in paths {
            let ext = path.extension().and_then(|e| e.to_str()).unwrap_or("");
            if !matches!(ext, "litematic" | "nbt") {
                continue;
            }
            let fname = path
                .file_name()
                .map(|n| n.to_string_lossy().into_owned())
                .unwrap_or_default();
            if let Ok(bytes) = horizon_os::fs::read(&path)
                && let Ok(schem) = Schematic::parse_bytes(&bytes, &fname)
            {
                loaded.push(schem);
            }
        }
        self.schematics = loaded;
    }

    pub fn import_schematic_bytes(
        &mut self,
        file_name: &str,
        bytes: &[u8],
        place_at: Option<[i32; 3]>,
    ) -> Result<String, String> {
        let schem = Schematic::parse_bytes(bytes, file_name)?;
        let title = schem.name.clone();
        if let Some(sdir) = self.schematics_dir() {
            let _ = horizon_os::fs::create_dir_all(&sdir);
            let _ = horizon_os::fs::write(sdir.join(file_name), bytes);
        }
        if let Some(existing) = self
            .schematics
            .iter_mut()
            .find(|s| s.file_name == file_name)
        {
            *existing = schem;
        } else {
            self.schematics.push(schem);
        }
        if let Some(origin) = place_at {
            self.placements.push(SchematicPlacement::new(
                title.clone(),
                file_name.to_string(),
                origin,
            ));
            self.active_placement = self.placements.len() - 1;
            self.save_placements();
            self.verification_dirty = true;
            self.bump_ghost_revision();
        }
        Ok(title)
    }

    pub fn schematic_for_placement(&self, idx: usize) -> Option<(&SchematicPlacement, &Schematic)> {
        let pl = self.placements.get(idx)?;
        let schem = self.schematics.iter().find(|s| s.file_name == pl.file_name)?;
        Some((pl, schem))
    }

    pub fn delete_schematic(&mut self, idx: usize) {
        if idx >= self.schematics.len() {
            return;
        }
        let fname = self.schematics.remove(idx).file_name;
        if let Some(sdir) = self.schematics_dir() {
            let _ = horizon_os::fs::remove_file(sdir.join(&fname));
        }
        self.placements.retain(|p| p.file_name != fname);
        if self.active_placement >= self.placements.len() {
            self.active_placement = self.placements.len().saturating_sub(1);
        }
        self.save_placements();
        self.verification_dirty = true;
        self.bump_ghost_revision();
    }
}

impl Default for LitematicaState {
    fn default() -> Self {
        Self::new()
    }
}

/// Helper to parse a block state text like `minecraft:oak_stairs[facing=north,half=bottom]`
/// into `(name, properties)` for saving schematics from the world.
pub fn parse_state_text(state_text: &str) -> (String, std::collections::BTreeMap<String, String>) {
    let mut props = std::collections::BTreeMap::new();
    let Some((name, rest)) = state_text.split_once('[') else {
        return (state_text.to_string(), props);
    };
    let inner = rest.trim_end_matches(']');
    for pair in inner.split(',') {
        if let Some((k, v)) = pair.split_once('=') {
            props.insert(k.trim().to_string(), v.trim().to_string());
        }
    }
    (name.to_string(), props)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::placement::{Mirror, Rotation};
    use crate::schematic::{PaletteEntry, SchematicRegion};

    #[test]
    fn litematic_roundtrip_preserves_blocks_and_metadata() {
        let region = SchematicRegion {
            name: "Main".to_string(),
            position: [0, 0, 0],
            size: [3, 2, 2],
            palette: vec![
                PaletteEntry::air(),
                PaletteEntry {
                    name: "minecraft:stone".to_string(),
                    properties: Default::default(),
                },
                PaletteEntry {
                    name: "minecraft:oak_stairs".to_string(),
                    properties: [("facing".to_string(), "north".to_string())]
                        .into_iter()
                        .collect(),
                },
            ],
            blocks: vec![1, 0, 2, 1, 2, 0, 0, 1, 1, 2, 0, 1],
        };
        let schem = Schematic {
            name: "Test House".to_string(),
            author: "Tester".to_string(),
            description: "Roundtrip".to_string(),
            version: 7,
            data_version: 4784,
            total_blocks: 8,
            total_volume: 12,
            enclosing_size: [3, 2, 2],
            regions: vec![region],
            file_name: "test.litematic".to_string(),
        };
        let bytes = schem.encode_litematic_bytes().unwrap();
        let decoded = Schematic::parse_bytes(&bytes, "test.litematic").unwrap();
        assert_eq!(decoded.name, "Test House");
        assert_eq!(decoded.regions.len(), 1);
        assert_eq!(decoded.regions[0].size, [3, 2, 2]);
        assert_eq!(decoded.regions[0].blocks, schem.regions[0].blocks);
        assert_eq!(decoded.regions[0].palette, schem.regions[0].palette);
    }

    #[test]
    fn placement_rotation_and_mirror_transform_correctly() {
        let mut pl = SchematicPlacement::new("P".into(), "p.litematic".into(), [10, 64, 20]);
        pl.rotation = Rotation::Cw90;
        assert_eq!(pl.transform_pos([2, 1, 0]), [10, 65, 22]);
        pl.mirror = Mirror::LeftRight;
        assert_eq!(pl.transform_pos([2, 1, 3]), [13, 65, 22]);

        for rot in [Rotation::None, Rotation::Cw90, Rotation::Cw180, Rotation::Ccw90] {
            for mir in [Mirror::None, Mirror::LeftRight, Mirror::FrontBack] {
                pl.rotation = rot;
                pl.mirror = mir;
                for local in [[0, 0, 0], [3, 5, 7], [-2, 1, 4], [11, -3, -6]] {
                    let world = pl.transform_pos(local);
                    assert_eq!(pl.inverse_transform_pos(world), local, "rot={rot:?} mir={mir:?}");
                }
            }
        }
    }

    #[test]
    fn bit_packing_crosses_u64_boundaries_cleanly() {
        let mut palette = vec![PaletteEntry::air()];
        for i in 1..25 {
            palette.push(PaletteEntry {
                name: format!("minecraft:wool_{i}"),
                properties: Default::default(),
            });
        }
        let volume = 5 * 5 * 5;
        let blocks: Vec<u16> = (0..volume).map(|i| (i % 25) as u16).collect();
        let schem = Schematic {
            name: "CrossBoundary".into(),
            author: "Tester".into(),
            description: String::new(),
            version: 7,
            data_version: 4784,
            total_blocks: 120,
            total_volume: volume as u64,
            enclosing_size: [5, 5, 5],
            regions: vec![SchematicRegion {
                name: "R1".into(),
                position: [0, 0, 0],
                size: [5, 5, 5],
                palette,
                blocks: blocks.clone(),
            }],
            file_name: "cross.litematic".into(),
        };
        let bytes = schem.encode_litematic_bytes().unwrap();
        let decoded = Schematic::parse_bytes(&bytes, "cross.litematic").unwrap();
        assert_eq!(decoded.regions[0].blocks, blocks);
    }

    #[test]
    fn negative_region_sizes_and_vanilla_nbt_parse_correctly() {
        use horizon_world::nbt::{self, Compound, JavaString, Limits, NamedTag, Tag};

        let mut reg = Compound::new();
        let mut pos = Compound::new();
        pos.insert("x".into(), Tag::Int(5));
        pos.insert("y".into(), Tag::Int(10));
        pos.insert("z".into(), Tag::Int(5));
        reg.insert("Position".into(), Tag::Compound(pos));
        let mut size = Compound::new();
        size.insert("x".into(), Tag::Int(-2));
        size.insert("y".into(), Tag::Int(2));
        size.insert("z".into(), Tag::Int(-2));
        reg.insert("Size".into(), Tag::Compound(size));
        let mut stone_tag = Compound::new();
        stone_tag.insert("Name".into(), Tag::String("minecraft:stone".into()));
        let mut air_tag = Compound::new();
        air_tag.insert("Name".into(), Tag::String("minecraft:air".into()));
        reg.insert("BlockStatePalette".into(), Tag::List(vec![Tag::Compound(air_tag), Tag::Compound(stone_tag)]));
        reg.insert("BlockStates".into(), Tag::LongArray(vec![0x5555i64]));
        let mut regions = Compound::new();
        regions.insert("Neg".into(), Tag::Compound(reg));
        let mut root = Compound::new();
        root.insert("Version".into(), Tag::Int(6));
        root.insert("Regions".into(), Tag::Compound(regions));
        let nbt_bytes = nbt::write_named(
            &NamedTag { name: JavaString::default(), tag: Tag::Compound(root) },
            Limits::templates(),
        ).unwrap();
        let schem = Schematic::parse_bytes(&nbt_bytes, "neg.litematic").unwrap();
        assert_eq!(schem.regions[0].size, [2, 2, 2]);
        assert_eq!(schem.regions[0].position, [4, 10, 4]);
        assert_eq!(schem.total_blocks, 8);

        let mut sroot = Compound::new();
        sroot.insert("size".into(), Tag::List(vec![Tag::Int(2), Tag::Int(1), Tag::Int(2)]));
        let mut p0 = Compound::new();
        p0.insert("Name".into(), Tag::String("minecraft:air".into()));
        let mut p1 = Compound::new();
        p1.insert("Name".into(), Tag::String("minecraft:oak_planks".into()));
        sroot.insert("palette".into(), Tag::List(vec![Tag::Compound(p0), Tag::Compound(p1)]));
        let mut b1 = Compound::new();
        b1.insert("pos".into(), Tag::List(vec![Tag::Int(1), Tag::Int(0), Tag::Int(1)]));
        b1.insert("state".into(), Tag::Int(1));
        sroot.insert("blocks".into(), Tag::List(vec![Tag::Compound(b1)]));
        let snbt_bytes = nbt::write_named(
            &NamedTag { name: JavaString::default(), tag: Tag::Compound(sroot) },
            Limits::templates(),
        ).unwrap();
        let van = Schematic::parse_bytes(&snbt_bytes, "struct.nbt").unwrap();
        assert_eq!(van.total_blocks, 1);
        assert_eq!(van.regions[0].get(1, 0, 1).unwrap().name, "minecraft:oak_planks");
    }

    #[test]
    fn property_transforms_verifier_layers_and_selection_work() {
        use crate::config::LayerMode;
        use crate::placement::transform_properties;
        use crate::verifier::BlockMismatchKind;
        use std::collections::BTreeMap;

        let mut props = BTreeMap::new();
        props.insert("facing".into(), "north".into());
        props.insert("hinge".into(), "left".into());
        props.insert("north".into(), "true".into());
        props.insert("east".into(), "false".into());
        let rotated = transform_properties(&props, Rotation::Cw90, Mirror::None);
        assert_eq!(rotated.get("facing").unwrap(), "east");
        assert_eq!(rotated.get("east").unwrap(), "true");
        assert_eq!(rotated.get("south").unwrap(), "false");

        let mirrored = transform_properties(&props, Rotation::None, Mirror::LeftRight);
        assert_eq!(mirrored.get("facing").unwrap(), "south");
        assert_eq!(mirrored.get("hinge").unwrap(), "right");

        let mut sel = AreaSelection::new("Test");
        sel.pos1 = Some([10, 70, 20]);
        sel.pos2 = Some([8, 68, 22]);
        assert_eq!(sel.bounds(), Some(([8, 68, 20], [10, 70, 22])));
        assert_eq!(sel.dimensions(), Some([3, 3, 3]));
        assert_eq!(sel.volume(), 27);

        let schem = Schematic {
            name: "V".into(),
            author: "T".into(),
            description: String::new(),
            version: 7,
            data_version: 4784,
            total_blocks: 3,
            total_volume: 4,
            enclosing_size: [2, 2, 1],
            regions: vec![SchematicRegion {
                name: "R".into(),
                position: [0, 0, 0],
                size: [2, 2, 1],
                palette: vec![
                    PaletteEntry::air(),
                    PaletteEntry { name: "minecraft:stone".into(), properties: Default::default() },
                    PaletteEntry {
                        name: "minecraft:oak_stairs".into(),
                        properties: [("facing".into(), "north".into())].into_iter().collect(),
                    },
                ],
                blocks: vec![1, 2, 1, 0],
            }],
            file_name: "v.litematic".into(),
        };
        let pl = SchematicPlacement::new("V".into(), "v.litematic".into(), [0, 64, 0]);
        let res = VerificationResult::compute(&schem, &pl, LayerMode::All, 64, [0.0, 64.0, 0.0], 100, |pos| {
            match pos {
                [0, 64, 0] => Some(("minecraft:stone".into(), "minecraft:stone".into())),
                [1, 64, 0] => Some(("minecraft:oak_stairs".into(), "minecraft:oak_stairs[facing=south]".into())),
                [0, 65, 0] => Some(("minecraft:air".into(), "minecraft:air".into())),
                _ => None,
            }
        });
        assert_eq!(res.total_required, 3);
        assert_eq!(res.total_placed, 1);
        assert_eq!(res.total_wrong, 1);
        assert_eq!(res.total_missing, 1);
        assert!(res.blocks.iter().any(|b| b.pos == [1, 64, 0] && b.kind == BlockMismatchKind::WrongState));
        assert!(res.to_csv().contains("minecraft:stone,2,1,1,0"));

        let single = VerificationResult::compute(&schem, &pl, LayerMode::SingleLayer, 65, [0.0, 64.0, 0.0], 100, |_| None);
        assert_eq!(single.total_required, 1);
    }
}
