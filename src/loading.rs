use ratatui::{buffer::Buffer, layout::{Constraint, Flex, Layout, Rect}, style::Stylize, text::Line, widgets::Widget};

pub struct Loading;

impl Widget for Loading {
    fn render(self, area: Rect, buf: &mut Buffer) {
        let [line] = Layout::vertical([Constraint::Length(1)])
            .flex(Flex::Center)
            .areas(area);

        Line::from("Loading map")
            .bold()
            .centered()
            .render(line, buf);
    }
}