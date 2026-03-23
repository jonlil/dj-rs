use iced::Color;

// Background layers
pub const BG_BASE:    Color = Color { r: 0.055, g: 0.055, b: 0.055, a: 1.0 }; // #0e0e0e
pub const BG_PANEL:   Color = Color { r: 0.082, g: 0.082, b: 0.082, a: 1.0 }; // #151515
pub const BG_ROW:     Color = Color { r: 0.094, g: 0.094, b: 0.094, a: 1.0 }; // #181818
pub const BG_HOVER:   Color = Color { r: 0.122, g: 0.122, b: 0.122, a: 1.0 }; // #1f1f1f
pub const BG_ACTIVE:  Color = Color { r: 0.098, g: 0.157, b: 0.235, a: 1.0 }; // #192840
pub const BG_ICON:    Color = Color { r: 0.063, g: 0.063, b: 0.063, a: 1.0 }; // #101010

// Text
pub const TEXT_PRIMARY:   Color = Color { r: 0.878, g: 0.878, b: 0.878, a: 1.0 }; // #e0e0e0
pub const TEXT_SECONDARY: Color = Color { r: 0.533, g: 0.533, b: 0.533, a: 1.0 }; // #888888
pub const TEXT_DIM:       Color = Color { r: 0.310, g: 0.310, b: 0.310, a: 1.0 }; // #4f4f4f

// Accent
pub const ACCENT_GREEN:  Color = Color { r: 0.114, g: 0.729, b: 0.333, a: 1.0 }; // #1DB954
pub const ACCENT_BLUE:   Color = Color { r: 0.337, g: 0.502, b: 1.000, a: 1.0 }; // #5580ff
pub const ACCENT_BORDER: Color = Color { r: 0.188, g: 0.188, b: 0.188, a: 1.0 }; // #303030

// Separator
pub const SEPARATOR: Color = Color { r: 0.149, g: 0.149, b: 0.149, a: 1.0 }; // #262626

// Icon bar widths
pub const ICON_BAR_W: f32 = 52.0;
pub const TREE_PANEL_W: f32 = 230.0;

// Row heights
pub const TRACK_ROW_H: f32 = 29.0;
pub const TREE_ROW_H: f32  = 29.0;
pub const HEADER_H: f32    = 24.0;
