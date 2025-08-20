use ratatui::{
    buffer::Buffer,
    layout::Rect,
    style::Style,
    symbols,
    widgets::{Block, Widget},
};
use std::collections::VecDeque;

pub struct TimeGraph<'a> {
    data: &'a VecDeque<u64>,
    max: u64,
    style: Style,
    block: Option<Block<'a>>,
}

impl<'a> TimeGraph<'a> {
    pub fn new(data: &'a VecDeque<u64>) -> Self {
        Self {
            data,
            max: 100,
            style: Style::default(),
            block: None,
        }
    }

    pub fn max(mut self, max: u64) -> Self {
        self.max = max;
        self
    }

    pub fn style(mut self, style: Style) -> Self {
        self.style = style;
        self
    }

    pub fn block(mut self, block: Block<'a>) -> Self {
        self.block = Some(block);
        self
    }
}

impl<'a> Widget for TimeGraph<'a> {
    fn render(self, area: Rect, buf: &mut Buffer) {
        // ==============================================================================
        // Render the block if provided
        // ==============================================================================
        let graph_area = if let Some(block) = self.block {
            let inner = block.inner(area);
            block.render(area, buf);
            inner
        } else {
            area
        };

        if graph_area.height < 1 || graph_area.width < 1 {
            return;
        }

        // ==============================================================================
        // Calculate graph dimensions and data points
        // ==============================================================================
        let max_width = graph_area.width as usize;
        let height = graph_area.height;

        // Get the data points to render (most recent on the right)
        let data_points: Vec<u64> = self.data.iter().take(max_width).rev().cloned().collect();

        if data_points.is_empty() {
            return;
        }

        // ==============================================================================
        // Render the graph using Braille characters
        // ==============================================================================

        // Each cell can display 2x4 dots using Braille characters
        let dots_per_cell = 4;
        let max_value = self.max as f64;

        for x in 0..graph_area.width.min(data_points.len() as u16) {
            let value = data_points[x as usize] as f64;
            let normalized = (value / max_value).min(1.0);

            // Calculate how many vertical positions to fill
            let filled_height = (normalized * (height * dots_per_cell) as f64) as u16;

            for y in 0..height {
                let cell_y = height - 1 - y;
                let cell_start = y * dots_per_cell;
                let cell_end = (y + 1) * dots_per_cell;

                // Determine which dots in this cell should be filled
                let mut dots = 0u8;
                for dot in cell_start..cell_end {
                    if dot < filled_height {
                        // Map dot position to Braille dot pattern
                        // Braille dots are numbered:
                        // 1 4
                        // 2 5
                        // 3 6
                        // 7 8
                        let dot_offset = dot - cell_start;
                        let braille_dot = match dot_offset {
                            0 => 0x40, // dot 7 (bottom)
                            1 => 0x04, // dot 3
                            2 => 0x02, // dot 2
                            3 => 0x01, // dot 1 (top)
                            _ => 0,
                        };
                        dots |= braille_dot;
                    }
                }

                if dots > 0 {
                    let symbol = symbols::braille::BLANK as u32 + dots as u32;
                    if let Some(ch) = char::from_u32(symbol) {
                        buf[(graph_area.left() + x, graph_area.top() + cell_y)]
                            .set_char(ch)
                            .set_style(self.style);
                    }
                }
            }
        }
    }
}
