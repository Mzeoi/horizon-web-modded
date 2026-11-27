use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct AreaSelection {
    pub name: String,
    pub pos1: Option<[i32; 3]>,
    pub pos2: Option<[i32; 3]>,
}

impl AreaSelection {
    pub fn new(name: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            pos1: None,
            pos2: None,
        }
    }

    pub fn bounds(&self) -> Option<([i32; 3], [i32; 3])> {
        let (a, b) = (self.pos1?, self.pos2?);
        let min = [a[0].min(b[0]), a[1].min(b[1]), a[2].min(b[2])];
        let max = [a[0].max(b[0]), a[1].max(b[1]), a[2].max(b[2])];
        Some((min, max))
    }

    pub fn dimensions(&self) -> Option<[i32; 3]> {
        let (min, max) = self.bounds()?;
        Some([
            max[0] - min[0] + 1,
            max[1] - min[1] + 1,
            max[2] - min[2] + 1,
        ])
    }

    pub fn volume(&self) -> u64 {
        self.dimensions()
            .map(|[x, y, z]| x as u64 * y as u64 * z as u64)
            .unwrap_or(0)
    }

    pub fn clear(&mut self) {
        self.pos1 = None;
        self.pos2 = None;
    }
}
