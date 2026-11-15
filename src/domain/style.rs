#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct RgbColor {
    pub r: u8,
    pub g: u8,
    pub b: u8,
}

impl RgbColor {
    pub const fn new(r: u8, g: u8, b: u8) -> Self {
        Self { r, g, b }
    }

    pub fn is_near_black(self) -> bool {
        self.r < 32 && self.g < 32 && self.b < 32
    }

    pub fn is_near_white(self) -> bool {
        self.r > 223 && self.g > 223 && self.b > 223
    }

    /// Perceived luminance test used to choose a readable foreground when a
    /// cell has a fill but no explicit usable font color.
    pub fn is_light(self) -> bool {
        let luminance = 299 * self.r as u32 + 587 * self.g as u32 + 114 * self.b as u32;
        luminance / 1000 >= 128
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum BorderStyle {
    #[default]
    None,
    Hair,
    Dotted,
    Dashed,
    Thin,
    Medium,
    Double,
    Thick,
}

impl BorderStyle {
    pub fn is_visible(self) -> bool {
        !matches!(self, Self::None)
    }

    pub fn weight(self) -> u8 {
        match self {
            Self::None => 0,
            Self::Hair | Self::Dotted => 1,
            Self::Dashed => 2,
            Self::Thin => 3,
            Self::Medium => 4,
            Self::Double => 5,
            Self::Thick => 6,
        }
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct BorderLine {
    pub style: BorderStyle,
    pub color: Option<RgbColor>,
}

impl BorderLine {
    pub const fn new(style: BorderStyle, color: Option<RgbColor>) -> Self {
        Self { style, color }
    }

    pub fn is_visible(&self) -> bool {
        self.style.is_visible()
    }

    /// Merge two border lines that describe the same boundary.
    ///
    /// The heavier style wins. When the weight is equal, a colored line is
    /// preferred over an uncolored one so that explicitly formatted borders
    /// keep their color.
    pub fn merge(self, other: Self) -> Self {
        match other.style.weight().cmp(&self.style.weight()) {
            std::cmp::Ordering::Greater => other,
            std::cmp::Ordering::Less => self,
            std::cmp::Ordering::Equal => {
                if self.color.is_none() && other.color.is_some() {
                    other
                } else {
                    self
                }
            }
        }
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct CellBorders {
    pub left: BorderLine,
    pub right: BorderLine,
    pub top: BorderLine,
    pub bottom: BorderLine,
}

impl CellBorders {
    pub fn any_visible(&self) -> bool {
        self.left.is_visible()
            || self.right.is_visible()
            || self.top.is_visible()
            || self.bottom.is_visible()
    }

    /// Combine two sets of borders. Used to aggregate the borders of all
    /// cells that belong to a merged range.
    pub fn merge(self, other: Self) -> Self {
        Self {
            left: self.left.merge(other.left),
            right: self.right.merge(other.right),
            top: self.top.merge(other.top),
            bottom: self.bottom.merge(other.bottom),
        }
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct CellStyle {
    pub bold: bool,
    pub color: Option<RgbColor>,
    pub fill: Option<RgbColor>,
    pub wrap_text: bool,
    pub borders: CellBorders,
}
