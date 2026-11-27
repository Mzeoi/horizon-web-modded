use crate::config::LayerMode;
use crate::placement::{SchematicPlacement, transform_properties};
use crate::schematic::Schematic;
use std::collections::HashMap;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BlockMismatchKind {
    Match,
    Missing,
    WrongBlock,
    WrongState,
}

#[derive(Clone, Debug)]
pub struct RenderedBlock {
    pub pos: [i32; 3],
    pub kind: BlockMismatchKind,
    pub expected_name: String,
}

#[derive(Clone, Debug)]
pub struct MaterialEntry {
    pub block_name: String,
    pub total: u32,
    pub placed: u32,
    pub missing: u32,
    pub wrong: u32,
}

#[derive(Clone, Debug, Default)]
pub struct VerificationResult {
    pub blocks: Vec<RenderedBlock>,
    pub materials: Vec<MaterialEntry>,
    pub total_required: u32,
    pub total_placed: u32,
    pub total_missing: u32,
    pub total_wrong: u32,
}

impl VerificationResult {
    /// Compares a schematic placement against the live world using `world_block_at`
    /// which returns `(block_name, full_state_text)` for a world `[x, y, z]` (or `None` if unloaded).
    pub fn compute<F>(
        schematic: &Schematic,
        placement: &SchematicPlacement,
        layer_mode: LayerMode,
        layer_y: i32,
        camera: [f64; 3],
        max_render_blocks: usize,
        mut world_block_at: F,
    ) -> Self
    where
        F: FnMut([i32; 3]) -> Option<(String, String)>,
    {
        let mut mat_map: HashMap<String, (u32, u32, u32, u32)> = HashMap::new();
        let mut rendered = Vec::new();
        let mut total_required = 0u32;
        let mut total_placed = 0u32;
        let mut total_missing = 0u32;
        let mut total_wrong = 0u32;

        for region in &schematic.regions {
            let [sx, sy, sz] = region.size;
            for y in 0..sy {
                for z in 0..sz {
                    for x in 0..sx {
                        let Some(entry) = region.get(x, y, z) else {
                            continue;
                        };
                        if entry.is_air() {
                            continue;
                        }
                        let local = [
                            region.position[0] + x,
                            region.position[1] + y,
                            region.position[2] + z,
                        ];
                        let world_pos = placement.transform_pos(local);
                        if !layer_mode.matches_y(world_pos[1], layer_y) {
                            continue;
                        }

                        total_required += 1;
                        let counts = mat_map.entry(entry.name.clone()).or_insert((0, 0, 0, 0));
                        counts.0 += 1;

                        let kind = match world_block_at(world_pos) {
                            None => {
                                total_missing += 1;
                                counts.2 += 1;
                                BlockMismatchKind::Missing
                            }
                            Some((w_name, w_state)) => {
                                if is_air_name(&w_name) {
                                    total_missing += 1;
                                    counts.2 += 1;
                                    BlockMismatchKind::Missing
                                } else if w_name != entry.name {
                                    total_wrong += 1;
                                    counts.3 += 1;
                                    BlockMismatchKind::WrongBlock
                                } else if !entry.properties.is_empty()
                                    && !properties_match(
                                        &w_state,
                                        &transform_properties(
                                            &entry.properties,
                                            placement.rotation,
                                            placement.mirror,
                                        ),
                                    )
                                {
                                    total_wrong += 1;
                                    counts.3 += 1;
                                    BlockMismatchKind::WrongState
                                } else {
                                    total_placed += 1;
                                    counts.1 += 1;
                                    BlockMismatchKind::Match
                                }
                            }
                        };

                        rendered.push(RenderedBlock {
                            pos: world_pos,
                            kind,
                            expected_name: entry.name.clone(),
                        });
                    }
                }
            }
        }

        // Prioritize mismatches (WrongBlock, WrongState) over Missing/Match, then sort by distance.
        rendered.sort_by(|a, b| {
            let pa = priority(a.kind);
            let pb = priority(b.kind);
            pa.cmp(&pb).then_with(|| {
                let da = dist_sq(a.pos, camera);
                let db = dist_sq(b.pos, camera);
                da.partial_cmp(&db).unwrap_or(std::cmp::Ordering::Equal)
            })
        });
        if rendered.len() > max_render_blocks {
            rendered.truncate(max_render_blocks);
        }

        let mut materials: Vec<MaterialEntry> = mat_map
            .into_iter()
            .map(|(block_name, (total, placed, missing, wrong))| MaterialEntry {
                block_name,
                total,
                placed,
                missing,
                wrong,
            })
            .collect();
        materials.sort_by(|a, b| {
            b.missing
                .cmp(&a.missing)
                .then_with(|| b.total.cmp(&a.total))
                .then_with(|| a.block_name.cmp(&b.block_name))
        });

        Self {
            blocks: rendered,
            materials,
            total_required,
            total_placed,
            total_missing,
            total_wrong,
        }
    }

    pub fn to_csv(&self) -> String {
        let mut out = String::from("Item,Total,Placed,Missing,Wrong\n");
        for m in &self.materials {
            out.push_str(&format!(
                "{},{},{},{},{}\n",
                m.block_name, m.total, m.placed, m.missing, m.wrong
            ));
        }
        out
    }
}

fn dist_sq(p: [i32; 3], cam: [f64; 3]) -> f64 {
    let dx = p[0] as f64 + 0.5 - cam[0];
    let dy = p[1] as f64 + 0.5 - cam[1];
    let dz = p[2] as f64 + 0.5 - cam[2];
    dx * dx + dy * dy + dz * dz
}

fn is_air_name(name: &str) -> bool {
    matches!(
        name,
        "minecraft:air" | "minecraft:cave_air" | "minecraft:void_air" | ""
    )
}

fn priority(kind: BlockMismatchKind) -> u8 {
    match kind {
        BlockMismatchKind::WrongBlock => 0,
        BlockMismatchKind::WrongState => 1,
        BlockMismatchKind::Missing => 2,
        BlockMismatchKind::Match => 3,
    }
}

fn properties_match(
    state_text: &str,
    expected: &std::collections::BTreeMap<String, String>,
) -> bool {
    let Some((_, props_part)) = state_text.split_once('[') else {
        return expected.is_empty();
    };
    let props_str = props_part.trim_end_matches(']');
    let mut world_props = HashMap::new();
    for pair in props_str.split(',') {
        if let Some((k, v)) = pair.split_once('=') {
            world_props.insert(k.trim(), v.trim());
        }
    }
    expected.iter().all(|(k, v)| {
        match world_props.get(k.as_str()) {
            Some(wv) => *wv == v.as_str(),
            // Ignore foreign/modded properties (e.g. ichorlogged) absent from vanilla 26.3.
            None => true,
        }
    })
}
