use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum Rotation {
    None,
    Cw90,
    Cw180,
    Ccw90,
}

impl Rotation {
    pub fn label(self) -> &'static str {
        match self {
            Self::None => "0°",
            Self::Cw90 => "90°",
            Self::Cw180 => "180°",
            Self::Ccw90 => "270°",
        }
    }

    pub fn next(self) -> Self {
        match self {
            Self::None => Self::Cw90,
            Self::Cw90 => Self::Cw180,
            Self::Cw180 => Self::Ccw90,
            Self::Ccw90 => Self::None,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum Mirror {
    None,
    LeftRight,
    FrontBack,
}

impl Mirror {
    pub fn label(self) -> &'static str {
        match self {
            Self::None => "None",
            Self::LeftRight => "Left-Right (Z)",
            Self::FrontBack => "Front-Back (X)",
        }
    }

    pub fn next(self) -> Self {
        match self {
            Self::None => Self::LeftRight,
            Self::LeftRight => Self::FrontBack,
            Self::FrontBack => Self::None,
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct SchematicPlacement {
    pub name: String,
    pub file_name: String,
    pub origin: [i32; 3],
    pub rotation: Rotation,
    pub mirror: Mirror,
    pub enabled: bool,
    pub locked: bool,
}

impl SchematicPlacement {
    pub fn new(name: String, file_name: String, origin: [i32; 3]) -> Self {
        Self {
            name,
            file_name,
            origin,
            rotation: Rotation::None,
            mirror: Mirror::None,
            enabled: true,
            locked: false,
        }
    }

    /// Transforms a schematic-local coordinate `(x, y, z)` (relative to placement origin)
    /// into world coordinates using this placement's mirror, rotation, and origin.
    pub fn transform_pos(&self, local: [i32; 3]) -> [i32; 3] {
        let [mut x, y, mut z] = local;
        match self.mirror {
            Mirror::None => {}
            Mirror::LeftRight => z = -z,
            Mirror::FrontBack => x = -x,
        }
        let (rx, rz) = match self.rotation {
            Rotation::None => (x, z),
            Rotation::Cw90 => (-z, x),
            Rotation::Cw180 => (-x, -z),
            Rotation::Ccw90 => (z, -x),
        };
        [self.origin[0] + rx, self.origin[1] + y, self.origin[2] + rz]
    }

    /// Inverse of `transform_pos`: maps a world coordinate back to schematic-local `(x, y, z)`.
    pub fn inverse_transform_pos(&self, world: [i32; 3]) -> [i32; 3] {
        let rx = world[0] - self.origin[0];
        let ry = world[1] - self.origin[1];
        let rz = world[2] - self.origin[2];
        let (mx, mz) = match self.rotation {
            Rotation::None => (rx, rz),
            Rotation::Cw90 => (rz, -rx),
            Rotation::Cw180 => (-rx, -rz),
            Rotation::Ccw90 => (-rz, rx),
        };
        let (lx, lz) = match self.mirror {
            Mirror::None => (mx, mz),
            Mirror::LeftRight => (mx, -mz),
            Mirror::FrontBack => (-mx, mz),
        };
        [lx, ry, lz]
    }
}

/// Rotates/mirrors directional block-state properties (facing, axis, connections, hinge, shape, rotation).
pub fn transform_properties(
    props: &BTreeMap<String, String>,
    rotation: Rotation,
    mirror: Mirror,
) -> BTreeMap<String, String> {
    if rotation == Rotation::None && mirror == Mirror::None {
        return props.clone();
    }
    let mut out = props.clone();
    if let Some(facing) = out.get("facing").cloned() {
        let mut f = facing.as_str();
        f = match (mirror, f) {
            (Mirror::LeftRight, "north") => "south",
            (Mirror::LeftRight, "south") => "north",
            (Mirror::FrontBack, "east") => "west",
            (Mirror::FrontBack, "west") => "east",
            _ => f,
        };
        f = match (rotation, f) {
            (Rotation::Cw90, "north") => "east",
            (Rotation::Cw90, "east") => "south",
            (Rotation::Cw90, "south") => "west",
            (Rotation::Cw90, "west") => "north",
            (Rotation::Cw180, "north") => "south",
            (Rotation::Cw180, "south") => "north",
            (Rotation::Cw180, "east") => "west",
            (Rotation::Cw180, "west") => "east",
            (Rotation::Ccw90, "north") => "west",
            (Rotation::Ccw90, "west") => "south",
            (Rotation::Ccw90, "south") => "east",
            (Rotation::Ccw90, "east") => "north",
            _ => f,
        };
        out.insert("facing".to_string(), f.to_string());
    }
    if let Some(axis) = out.get("axis").cloned()
        && matches!(rotation, Rotation::Cw90 | Rotation::Ccw90)
    {
        let next = match axis.as_str() {
            "x" => "z",
            "z" => "x",
            other => other,
        };
        out.insert("axis".to_string(), next.to_string());
    }
    if mirror != Mirror::None {
        if let Some(hinge) = out.get("hinge").cloned() {
            let next = match hinge.as_str() {
                "left" => "right",
                "right" => "left",
                other => other,
            };
            out.insert("hinge".to_string(), next.to_string());
        }
        if let Some(shape) = out.get("shape").cloned() {
            let next = match shape.as_str() {
                "inner_left" => "inner_right",
                "inner_right" => "inner_left",
                "outer_left" => "outer_right",
                "outer_right" => "outer_left",
                other => other,
            };
            out.insert("shape".to_string(), next.to_string());
        }
    }
    if let Some(rot_str) = out.get("rotation").cloned()
        && let Ok(mut rot) = rot_str.parse::<i32>()
        && (0..16).contains(&rot)
    {
        rot = match mirror {
            Mirror::None => rot,
            Mirror::LeftRight => (8 - rot).rem_euclid(16),
            Mirror::FrontBack => (16 - rot).rem_euclid(16),
        };
        rot = match rotation {
            Rotation::None => rot,
            Rotation::Cw90 => (rot + 4) % 16,
            Rotation::Cw180 => (rot + 8) % 16,
            Rotation::Ccw90 => (rot + 12) % 16,
        };
        out.insert("rotation".to_string(), rot.to_string());
    }
    let has_cardinals = ["north", "east", "south", "west"]
        .iter()
        .any(|k| props.contains_key(*k));
    if has_cardinals {
        let mut n = props.get("north").cloned();
        let mut e = props.get("east").cloned();
        let mut s = props.get("south").cloned();
        let mut w = props.get("west").cloned();
        match mirror {
            Mirror::None => {}
            Mirror::LeftRight => std::mem::swap(&mut n, &mut s),
            Mirror::FrontBack => std::mem::swap(&mut e, &mut w),
        }
        let (fn_, fe, fs, fw) = match rotation {
            Rotation::None => (n, e, s, w),
            Rotation::Cw90 => (w, n, e, s),
            Rotation::Cw180 => (s, w, n, e),
            Rotation::Ccw90 => (e, s, w, n),
        };
        if let Some(v) = fn_ {
            out.insert("north".to_string(), v);
        }
        if let Some(v) = fe {
            out.insert("east".to_string(), v);
        }
        if let Some(v) = fs {
            out.insert("south".to_string(), v);
        }
        if let Some(v) = fw {
            out.insert("west".to_string(), v);
        }
    }
    out
}
