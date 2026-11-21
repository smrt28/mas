//! Geometry of the complete printed plan, shared by rendering and mouse picking.
//! Source coordinates use the 1428×2000 PDF preview, with y increasing downwards.
use crate::{CELLS, COLS, ROWS, Rank};

pub const PAGE_WIDTH: f32 = 1428.;
pub const PAGE_HEIGHT: f32 = 2000.;
pub const WIDTH: u32 = 600;
pub const PDF_WIDTH: f32 = 841.68;
pub const PDF_HEIGHT: f32 = 1178.88;
pub const ASPECT: f32 = PDF_WIDTH / PDF_HEIGHT;
pub const HEIGHT: u32 = 841;
pub const BAR_HEIGHT: u32 = 84;
pub const WINDOW_HEIGHT: u32 = HEIGHT + BAR_HEIGHT;
pub const GRID_LEFT: f32 = 10.;
pub const GRID_TOP: f32 = 235.;
pub const STEP: f32 = 140.;
pub const STRIP: [Rank; 11] = [
    Rank::Flag,
    Rank::Marshal,
    Rank::General,
    Rank::Captain,
    Rank::Cadet,
    Rank::Corporal,
    Rank::Scout,
    Rank::Rifleman,
    Rank::Miner,
    Rank::Spy,
    Rank::Mine,
];

#[derive(Clone, Copy, Debug)]
pub struct Point {
    pub x: f32,
    pub y: f32,
}
#[derive(Clone, Copy, Debug)]
pub struct Rect {
    pub left: f32,
    pub top: f32,
    pub width: f32,
    pub height: f32,
}
impl Rect {
    pub fn center(self) -> Point {
        Point {
            x: self.left + self.width / 2.,
            y: self.top + self.height / 2.,
        }
    }
    pub fn contains(self, p: Point) -> bool {
        p.x >= self.left
            && p.x < self.left + self.width
            && p.y >= self.top
            && p.y < self.top + self.height
    }
}
/// A uniformly scaled sheet, centered below a fixed-height info bar. Extra
/// space is letterboxed; neither the artwork nor its interactive regions stretch.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Layout {
    pub width: f32,
    pub height: f32,
    pub board_width: f32,
    pub board_height: f32,
    pub scale: f32,
}
impl Default for Layout {
    fn default() -> Self {
        Self::new(WIDTH as f32, WINDOW_HEIGHT as f32)
    }
}
impl Layout {
    pub fn new(width: f32, height: f32) -> Self {
        let width = width.max(1.);
        let height = height.max(BAR_HEIGHT as f32 + 1.);
        let board_width = width.min((height - BAR_HEIGHT as f32) * ASPECT);
        Self {
            width,
            height,
            board_width,
            board_height: board_width / ASPECT,
            scale: board_width / WIDTH as f32,
        }
    }
    /// Rotate the complete printed sheet for commander two, including its legend.
    pub fn world(self, p: Point, you: usize) -> Point {
        let (x, y) = if you == 0 {
            (p.x, p.y)
        } else {
            (PAGE_WIDTH - p.x, PAGE_HEIGHT - p.y)
        };
        Point {
            x: (x / PAGE_WIDTH - 0.5) * self.board_width,
            y: -(BAR_HEIGHT as f32) / 2. + (0.5 - y / PAGE_HEIGHT) * self.board_height,
        }
    }
    pub fn source(self, p: Point, you: usize) -> Point {
        let x = (p.x / self.board_width + 0.5) * PAGE_WIDTH;
        let y = (0.5 - (p.y + BAR_HEIGHT as f32 / 2.) / self.board_height) * PAGE_HEIGHT;
        if you == 0 {
            Point { x, y }
        } else {
            Point {
                x: PAGE_WIDTH - x,
                y: PAGE_HEIGHT - y,
            }
        }
    }
    pub fn pick_cell(self, p: Point, you: usize) -> Option<usize> {
        pick_source_cell(self.source(p, you))
    }
    pub fn pick_rank(self, p: Point, you: usize) -> Option<Rank> {
        let p = self.source(p, you);
        STRIP.into_iter().find(|rank| card(*rank, you).contains(p))
    }
    /// Pointer coordinates are logical window pixels, independent of display DPI.
    pub fn cursor(self, x: f32, y: f32) -> Point {
        Point {
            x: x - self.width / 2.,
            y: self.height / 2. - y,
        }
    }
    pub fn in_bar(self, p: Point) -> bool {
        p.x >= -self.width / 2.
            && p.x < self.width / 2.
            && p.y >= self.height / 2. - BAR_HEIGHT as f32
            && p.y <= self.height / 2.
    }
}
pub fn world(p: Point, you: usize) -> Point {
    Layout::default().world(p, you)
}
pub fn source(p: Point, you: usize) -> Point {
    Layout::default().source(p, you)
}
pub fn cell(at: usize) -> Rect {
    Rect {
        left: GRID_LEFT + (at % COLS) as f32 * STEP,
        top: GRID_TOP + (at / COLS) as f32 * STEP,
        width: STEP,
        height: STEP,
    }
}
pub fn pick_cell(p: Point, you: usize) -> Option<usize> {
    Layout::default().pick_cell(p, you)
}
fn pick_source_cell(p: Point) -> Option<usize> {
    let grid = Rect {
        left: GRID_LEFT,
        top: GRID_TOP,
        width: COLS as f32 * STEP,
        height: ROWS as f32 * STEP,
    };
    if !grid.contains(p) {
        return None;
    }
    let col = ((p.x - GRID_LEFT) / STEP).floor() as usize;
    let row = ((p.y - GRID_TOP) / STEP).floor() as usize;
    let at = row * COLS + col;
    (at < CELLS && !crate::blocked(at)).then_some(at)
}
/// Card slots measured in the edited PNG (1059×1485). Both printed strips
/// share equal center spacing and balanced margins. Convert once into the
/// board coordinate system used by picking, borders and reserve counts.
pub fn card(rank: Rank, you: usize) -> Rect {
    let i = STRIP.iter().position(|r| *r == rank).unwrap();
    let left = 5. + 96. * i as f32;
    let (left, top) = if you == 0 {
        (left, 1344.)
    } else {
        (1059. - left - 88., 16.)
    };
    Rect {
        left: left / 1059. * PAGE_WIDTH,
        top: top / 1485. * PAGE_HEIGHT,
        width: 88. / 1059. * PAGE_WIDTH,
        height: 132. / 1485. * PAGE_HEIGHT,
    }
}
/// Counts occupy the blank area below the label in the viewer's orientation.
pub fn count_position(rank: Rank, you: usize) -> Point {
    let area = card(rank, you);
    Point {
        x: area.center().x,
        y: area.top + area.height * if you == 0 { 0.86 } else { 0.14 },
    }
}
pub fn pick_rank(p: Point, you: usize) -> Option<Rank> {
    Layout::default().pick_rank(p, you)
}
pub fn in_bar(p: Point) -> bool {
    Layout::default().in_bar(p)
}
