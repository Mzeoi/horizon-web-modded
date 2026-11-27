use flate2::{Compression, read::GzDecoder, write::GzEncoder};
use horizon_world::nbt::{self, Compound, JavaString, Limits, NamedTag, Tag};
use std::collections::BTreeMap;
use std::io::{Read, Write};

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PaletteEntry {
    pub name: String,
    pub properties: BTreeMap<String, String>,
}

impl PaletteEntry {
    pub fn air() -> Self {
        Self {
            name: "minecraft:air".to_string(),
            properties: BTreeMap::new(),
        }
    }

    pub fn is_air(&self) -> bool {
        matches!(
            self.name.as_str(),
            "minecraft:air" | "minecraft:cave_air" | "minecraft:void_air" | "minecraft:structure_void"
        )
    }
}

#[derive(Clone, Debug)]
pub struct SchematicRegion {
    pub name: String,
    /// Offset relative to schematic origin.
    pub position: [i32; 3],
    /// Positive dimensions `[sx, sy, sz]`.
    pub size: [i32; 3],
    pub palette: Vec<PaletteEntry>,
    /// Flat indices `[y * (sx * sz) + z * sx + x]` into `palette`.
    pub blocks: Vec<u16>,
}

impl SchematicRegion {
    pub fn get(&self, x: i32, y: i32, z: i32) -> Option<&PaletteEntry> {
        let [sx, sy, sz] = self.size;
        if x < 0 || y < 0 || z < 0 || x >= sx || y >= sy || z >= sz {
            return None;
        }
        let idx = (y as usize) * (sx as usize * sz as usize) + (z as usize) * (sx as usize) + (x as usize);
        let pal_idx = *self.blocks.get(idx)? as usize;
        self.palette.get(pal_idx)
    }
}

#[derive(Clone, Debug)]
pub struct Schematic {
    pub name: String,
    pub author: String,
    pub description: String,
    pub version: i32,
    pub data_version: i32,
    pub total_blocks: u64,
    pub total_volume: u64,
    pub enclosing_size: [i32; 3],
    pub regions: Vec<SchematicRegion>,
    pub file_name: String,
}

impl Schematic {
    pub fn parse_bytes(bytes: &[u8], file_name: &str) -> Result<Self, String> {
        let raw = maybe_gunzip(bytes)?;
        let (named, _) = nbt::read_named(&raw, Limits::templates())
            .map_err(|e| format!("Failed to parse NBT: {e}"))?;
        let Tag::Compound(root) = &named.tag else {
            return Err("Schematic root is not an NBT compound".to_string());
        };

        if root_get(root, "Regions").is_some() && root_get(root, "Version").is_some() {
            parse_litematic(root, file_name)
        } else if root_get(root, "palette").is_some() && root_get(root, "blocks").is_some() {
            parse_vanilla_structure(root, file_name)
        } else {
            Err("Unrecognized schematic format (expected .litematic or vanilla .nbt structure)".to_string())
        }
    }

    pub fn encode_litematic_bytes(&self) -> Result<Vec<u8>, String> {
        let mut regions_map = Compound::new();
        for reg in &self.regions {
            let mut reg_map = Compound::new();
            reg_map.insert("Position".into(), make_xyz_compound(reg.position));
            reg_map.insert("Size".into(), make_xyz_compound(reg.size));

            let pal_tags: Vec<Tag> = reg
                .palette
                .iter()
                .map(|entry| {
                    let mut c = Compound::new();
                    c.insert("Name".into(), Tag::String(entry.name.as_str().into()));
                    if !entry.properties.is_empty() {
                        let mut props = Compound::new();
                        for (k, v) in &entry.properties {
                            props.insert(k.as_str().into(), Tag::String(v.as_str().into()));
                        }
                        c.insert("Properties".into(), Tag::Compound(props));
                    }
                    Tag::Compound(c)
                })
                .collect();
            reg_map.insert("BlockStatePalette".into(), Tag::List(pal_tags));

            let bits = bits_for_palette(reg.palette.len());
            let packed = pack_litematica_bits(&reg.blocks, bits);
            reg_map.insert("BlockStates".into(), Tag::LongArray(packed));
            reg_map.insert("TileEntities".into(), Tag::List(Vec::new()));
            reg_map.insert("Entities".into(), Tag::List(Vec::new()));

            regions_map.insert(reg.name.as_str().into(), Tag::Compound(reg_map));
        }

        let mut meta = Compound::new();
        meta.insert("Name".into(), Tag::String(self.name.as_str().into()));
        meta.insert("Author".into(), Tag::String(self.author.as_str().into()));
        meta.insert("Description".into(), Tag::String(self.description.as_str().into()));
        meta.insert("RegionCount".into(), Tag::Int(self.regions.len() as i32));
        meta.insert("TotalBlocks".into(), Tag::Int(self.total_blocks as i32));
        meta.insert("TotalVolume".into(), Tag::Int(self.total_volume as i32));
        meta.insert("EnclosingSize".into(), make_xyz_compound(self.enclosing_size));
        meta.insert("TimeCreated".into(), Tag::Long(0));
        meta.insert("TimeModified".into(), Tag::Long(0));

        let mut root = Compound::new();
        root.insert("Version".into(), Tag::Int(7));
        root.insert("SubVersion".into(), Tag::Int(1));
        root.insert("MinecraftDataVersion".into(), Tag::Int(4784));
        root.insert("Metadata".into(), Tag::Compound(meta));
        root.insert("Regions".into(), Tag::Compound(regions_map));

        let nbt_bytes = nbt::write_named(
            &NamedTag {
                name: JavaString::default(),
                tag: Tag::Compound(root),
            },
            Limits::templates(),
        )
        .map_err(|e| format!("Failed to encode NBT: {e}"))?;

        let mut encoder = GzEncoder::new(Vec::new(), Compression::default());
        encoder
            .write_all(&nbt_bytes)
            .map_err(|e| format!("Gzip compress failed: {e}"))?;
        encoder
            .finish()
            .map_err(|e| format!("Gzip finish failed: {e}"))
    }
}

fn maybe_gunzip(bytes: &[u8]) -> Result<Vec<u8>, String> {
    if bytes.len() >= 2 && bytes[0] == 0x1f && bytes[1] == 0x8b {
        let mut decoder = GzDecoder::new(bytes);
        let mut out = Vec::new();
        decoder
            .read_to_end(&mut out)
            .map_err(|e| format!("Gzip decompress failed: {e}"))?;
        Ok(out)
    } else {
        Ok(bytes.to_vec())
    }
}

fn root_get<'a>(map: &'a Compound, key: &str) -> Option<&'a Tag> {
    map.get(&JavaString::from(key))
}

fn tag_int(tag: Option<&Tag>, default: i32) -> i32 {
    match tag {
        Some(Tag::Int(v)) => *v,
        Some(Tag::Short(v)) => *v as i32,
        Some(Tag::Byte(v)) => *v as i32,
        Some(Tag::Long(v)) => *v as i32,
        _ => default,
    }
}

fn tag_str(tag: Option<&Tag>) -> Option<String> {
    match tag {
        Some(Tag::String(s)) => Some(s.to_string_lossy()),
        _ => None,
    }
}

fn read_xyz_compound(tag: Option<&Tag>) -> [i32; 3] {
    let Some(Tag::Compound(c)) = tag else {
        return [0, 0, 0];
    };
    [
        tag_int(root_get(c, "x"), 0),
        tag_int(root_get(c, "y"), 0),
        tag_int(root_get(c, "z"), 0),
    ]
}

fn make_xyz_compound(v: [i32; 3]) -> Tag {
    let mut c = Compound::new();
    c.insert("x".into(), Tag::Int(v[0]));
    c.insert("y".into(), Tag::Int(v[1]));
    c.insert("z".into(), Tag::Int(v[2]));
    Tag::Compound(c)
}

fn parse_palette_entry(tag: &Tag) -> PaletteEntry {
    let Tag::Compound(c) = tag else {
        return PaletteEntry::air();
    };
    let name = tag_str(root_get(c, "Name")).unwrap_or_else(|| "minecraft:air".to_string());
    let mut properties = BTreeMap::new();
    if let Some(Tag::Compound(props)) = root_get(c, "Properties") {
        for (k, v) in props {
            if let Some(vs) = tag_str(Some(v)) {
                properties.insert(k.to_string_lossy(), vs);
            }
        }
    }
    PaletteEntry { name, properties }
}

fn bits_for_palette(palette_len: usize) -> usize {
    if palette_len <= 1 {
        return 2;
    }
    let highest_bit = (usize::BITS - (palette_len - 1).leading_zeros()) as usize;
    highest_bit.max(2)
}

fn unpack_litematica_bits(longs: &[i64], bits: usize, volume: usize) -> Vec<u16> {
    let mut out = Vec::with_capacity(volume);
    if longs.is_empty() || bits == 0 {
        out.resize(volume, 0);
        return out;
    }
    let mask: u64 = (1u64 << bits) - 1;
    for idx in 0..volume {
        let start_offset = idx * bits;
        let start_arr = start_offset >> 6;
        let end_arr = ((idx + 1) * bits - 1) >> 6;
        let start_bit = start_offset & 0x3F;
        if start_arr >= longs.len() {
            out.push(0);
            continue;
        }
        let val = if start_arr == end_arr || end_arr >= longs.len() {
            ((longs[start_arr] as u64) >> start_bit) & mask
        } else {
            let end_offset = 64 - start_bit;
            (((longs[start_arr] as u64) >> start_bit) | ((longs[end_arr] as u64) << end_offset))
                & mask
        };
        out.push(val as u16);
    }
    out
}

fn pack_litematica_bits(indices: &[u16], bits: usize) -> Vec<i64> {
    let total_bits = indices.len() * bits;
    let long_len = total_bits.div_ceil(64);
    let mut longs = vec![0u64; long_len.max(1)];
    let mask = (1u64 << bits) - 1;
    for (idx, &val) in indices.iter().enumerate() {
        let v = (val as u64) & mask;
        let start_offset = idx * bits;
        let start_arr = start_offset >> 6;
        let end_arr = ((idx + 1) * bits - 1) >> 6;
        let start_bit = start_offset & 0x3F;
        longs[start_arr] |= v << start_bit;
        if start_arr != end_arr {
            let end_offset = 64 - start_bit;
            longs[end_arr] |= v >> end_offset;
        }
    }
    longs.into_iter().map(|u| u as i64).collect()
}

fn parse_litematic(root: &Compound, file_name: &str) -> Result<Schematic, String> {
    let version = tag_int(root_get(root, "Version"), 0);
    if !(1..=8).contains(&version) {
        return Err(format!("Unsupported .litematic version: {version}"));
    }
    let data_version = tag_int(root_get(root, "MinecraftDataVersion"), 0);

    let (name, author, description, enclosing_size) =
        if let Some(Tag::Compound(meta)) = root_get(root, "Metadata") {
            (
                tag_str(root_get(meta, "Name")).unwrap_or_else(|| file_name.to_string()),
                tag_str(root_get(meta, "Author")).unwrap_or_else(|| "Unknown".to_string()),
                tag_str(root_get(meta, "Description")).unwrap_or_default(),
                read_xyz_compound(root_get(meta, "EnclosingSize")),
            )
        } else {
            (file_name.to_string(), "Unknown".to_string(), String::new(), [0, 0, 0])
        };

    let Some(Tag::Compound(regions_nbt)) = root_get(root, "Regions") else {
        return Err("Missing Regions compound".to_string());
    };

    let mut regions = Vec::new();
    let mut total_blocks = 0u64;
    let mut total_volume = 0u64;

    for (rname, rtag) in regions_nbt {
        let Tag::Compound(rc) = rtag else { continue };
        let raw_pos = read_xyz_compound(root_get(rc, "Position"));
        let raw_size = read_xyz_compound(root_get(rc, "Size"));
        let abs_size = [raw_size[0].abs(), raw_size[1].abs(), raw_size[2].abs()];
        if abs_size[0] <= 0 || abs_size[1] <= 0 || abs_size[2] <= 0 {
            continue;
        }
        // Normalize negative sizes to min-corner origin.
        let min_pos = [
            if raw_size[0] < 0 { raw_pos[0] + raw_size[0] + 1 } else { raw_pos[0] },
            if raw_size[1] < 0 { raw_pos[1] + raw_size[1] + 1 } else { raw_pos[1] },
            if raw_size[2] < 0 { raw_pos[2] + raw_size[2] + 1 } else { raw_pos[2] },
        ];

        let palette: Vec<PaletteEntry> = match root_get(rc, "BlockStatePalette") {
            Some(Tag::List(list)) => list.iter().map(parse_palette_entry).collect(),
            _ => vec![PaletteEntry::air()],
        };
        let bits = bits_for_palette(palette.len());
        let volume = abs_size[0] as usize * abs_size[1] as usize * abs_size[2] as usize;
        let longs = match root_get(rc, "BlockStates") {
            Some(Tag::LongArray(arr)) => arr.as_slice(),
            _ => &[],
        };
        let blocks = unpack_litematica_bits(longs, bits, volume);

        for &pal_idx in &blocks {
            if let Some(entry) = palette.get(pal_idx as usize)
                && !entry.is_air()
            {
                total_blocks += 1;
            }
        }
        total_volume += volume as u64;

        regions.push(SchematicRegion {
            name: rname.to_string_lossy(),
            position: min_pos,
            size: abs_size,
            palette,
            blocks,
        });
    }

    if regions.is_empty() {
        return Err("Schematic contains no valid regions".to_string());
    }

    let computed_enclosing = if enclosing_size.iter().all(|&v| v > 0) {
        enclosing_size
    } else {
        regions.iter().fold([0, 0, 0], |acc, r| {
            [acc[0].max(r.size[0]), acc[1].max(r.size[1]), acc[2].max(r.size[2])]
        })
    };

    Ok(Schematic {
        name,
        author,
        description,
        version,
        data_version,
        total_blocks,
        total_volume,
        enclosing_size: computed_enclosing,
        regions,
        file_name: file_name.to_string(),
    })
}

fn parse_vanilla_structure(root: &Compound, file_name: &str) -> Result<Schematic, String> {
    let size = match root_get(root, "size") {
        Some(Tag::List(l)) if l.len() >= 3 => [
            tag_int(l.first(), 0),
            tag_int(l.get(1), 0),
            tag_int(l.get(2), 0),
        ],
        _ => return Err("Missing structure size".to_string()),
    };
    if size[0] <= 0 || size[1] <= 0 || size[2] <= 0 {
        return Err("Invalid structure size".to_string());
    }
    let palette: Vec<PaletteEntry> = match root_get(root, "palette") {
        Some(Tag::List(l)) => l.iter().map(parse_palette_entry).collect(),
        _ => return Err("Missing structure palette".to_string()),
    };
    let volume = size[0] as usize * size[1] as usize * size[2] as usize;
    let mut blocks = vec![0u16; volume];
    let mut total_blocks = 0u64;
    if let Some(Tag::List(block_list)) = root_get(root, "blocks") {
        for b in block_list {
            let Tag::Compound(bc) = b else { continue };
            let state = tag_int(root_get(bc, "state"), 0) as u16;
            if let Some(Tag::List(pos)) = root_get(bc, "pos")
                && pos.len() >= 3
            {
                let x = tag_int(pos.first(), -1);
                let y = tag_int(pos.get(1), -1);
                let z = tag_int(pos.get(2), -1);
                if x >= 0 && y >= 0 && z >= 0 && x < size[0] && y < size[1] && z < size[2] {
                    let idx = (y as usize) * (size[0] as usize * size[2] as usize)
                        + (z as usize) * (size[0] as usize)
                        + (x as usize);
                    blocks[idx] = state;
                    if palette.get(state as usize).is_some_and(|e| !e.is_air()) {
                        total_blocks += 1;
                    }
                }
            }
        }
    }

    let author = tag_str(root_get(root, "author")).unwrap_or_else(|| "Unknown".to_string());
    let data_version = tag_int(root_get(root, "DataVersion"), 0);

    Ok(Schematic {
        name: file_name.to_string(),
        author,
        description: String::new(),
        version: 0,
        data_version,
        total_blocks,
        total_volume: volume as u64,
        enclosing_size: size,
        regions: vec![SchematicRegion {
            name: "Structure".to_string(),
            position: [0, 0, 0],
            size,
            palette,
            blocks,
        }],
        file_name: file_name.to_string(),
    })
}
