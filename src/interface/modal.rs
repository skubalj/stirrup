/*
stirrup: A TUI Mount Manager
Copyright (C) 2026 Joseph Skubal

This program is free software: you can redistribute it and/or modify
it under the terms of the GNU General Public License as published by
the Free Software Foundation, either version 3 of the License, or
any later version.

This program is distributed in the hope that it will be useful,
but WITHOUT ANY WARRANTY; without even the implied warranty of
MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE.  See the
GNU General Public License for more details.

You should have received a copy of the GNU General Public License
along with this program.  If not, see <https://www.gnu.org/licenses/>.
*/

use super::{MountAction, RunState, TableRow, style};
use anyhow::Result;
use crossterm::event::{self, Event, KeyCode};
use ratatui::{
    Frame,
    buffer::Buffer,
    layout::{Constraint, Layout, Position, Rect},
    text::{Line, Text},
    widgets::{
        Block, Clear, Padding, Paragraph, Scrollbar, ScrollbarOrientation, ScrollbarState,
        StatefulWidget, Widget, Wrap,
    },
};
use tui_input::{Input, backend::crossterm::EventHandler};

use crate::mount::MountConfiguration;

#[derive(Debug, Default)]
pub enum ModalState {
    #[default]
    None,
    EditModal(EditModal),
    DeleteConfirmModal(ConfirmModal),
    Notification(NotifyModal),
}

impl ModalState {
    pub fn set_cursor_position(&self, frame: &mut Frame) {
        if let ModalState::EditModal(edit_modal) = self
            && let Some(position) = edit_modal.cursor
        {
            frame.set_cursor_position(position);
        }
    }
}

#[derive(Debug, Default, Clone, Copy)]
pub struct Modal;

impl StatefulWidget for Modal {
    type State = ModalState;

    fn render(self, area: Rect, buf: &mut Buffer, state: &mut Self::State) {
        match state {
            ModalState::None => {}
            ModalState::EditModal(edit_modal) => edit_modal.draw(area, buf),
            ModalState::DeleteConfirmModal(confirm_modal) => confirm_modal.draw(area, buf),
            ModalState::Notification(notify_modal) => notify_modal.draw(area, buf),
        }
    }
}

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
enum EditSelection {
    #[default]
    Name,
    Device,
    MountPoint,
    LuksName,
    Filesystem,
    AcceptButton,
    DiscardButton,
}

impl EditSelection {
    pub fn is_button(self) -> bool {
        matches!(self, Self::AcceptButton | Self::DiscardButton)
    }

    pub fn up(self, is_mounted: bool) -> Self {
        match self {
            Self::Name | Self::Device => Self::Name,
            Self::MountPoint => Self::Device,
            Self::LuksName => Self::MountPoint,
            Self::Filesystem => Self::LuksName,
            Self::AcceptButton | Self::DiscardButton if is_mounted => Self::Name,
            Self::AcceptButton | Self::DiscardButton => Self::Filesystem,
        }
    }

    pub fn down(self, is_mounted: bool) -> Self {
        match self {
            Self::Name if is_mounted => Self::AcceptButton,
            Self::Name => Self::Device,
            Self::Device => Self::MountPoint,
            Self::MountPoint => Self::LuksName,
            Self::LuksName => Self::Filesystem,
            Self::Filesystem | Self::AcceptButton => Self::AcceptButton,
            Self::DiscardButton => Self::DiscardButton,
        }
    }

    pub fn left(self) -> Self {
        match self {
            Self::DiscardButton => Self::AcceptButton,
            x => x,
        }
    }

    pub fn right(self) -> Self {
        match self {
            Self::AcceptButton => Self::DiscardButton,
            x => x,
        }
    }

    pub fn next(self, is_mounted: bool) -> Self {
        match self {
            Self::Name if is_mounted => Self::AcceptButton,
            Self::Name => Self::Device,
            Self::Device => Self::MountPoint,
            Self::MountPoint => Self::LuksName,
            Self::LuksName => Self::Filesystem,
            Self::Filesystem => Self::AcceptButton,
            Self::AcceptButton | Self::DiscardButton => Self::DiscardButton,
        }
    }

    pub fn previous(self, is_mounted: bool) -> Self {
        match self {
            Self::Name | Self::Device => Self::Name,
            Self::MountPoint => Self::Device,
            Self::LuksName => Self::MountPoint,
            Self::Filesystem => Self::LuksName,
            Self::AcceptButton if is_mounted => Self::Name,
            Self::AcceptButton => Self::Filesystem,
            Self::DiscardButton => Self::AcceptButton,
        }
    }
}

#[derive(Debug, Clone)]
pub struct EditModal {
    name: Input,
    device: Input,
    mount_point: Input,
    luks_name: Input,
    filesystem: Input,

    is_mounted: bool,
    needs_mount: MountAction,
    selected: EditSelection,
    cursor: Option<Position>,
}

impl EditModal {
    pub fn new(row: &TableRow) -> Self {
        Self {
            name: Input::new(row.name.clone()),
            device: Input::new(row.config.device.clone()),
            mount_point: Input::new(row.config.mount_point.to_string_lossy().to_string()),
            luks_name: Input::new(row.config.luks_decrypt_name.clone().unwrap_or_default()),
            filesystem: Input::new(row.config.filesystem.clone().unwrap_or_default()),
            is_mounted: row.is_mounted,
            needs_mount: row.needs_mount,
            selected: Default::default(),
            cursor: Default::default(),
        }
    }

    pub fn handle_input(&mut self) -> Result<RunState<TableRow>> {
        let event = event::read()?;
        if let Event::Key(key) = event
            && key.kind.is_press()
        {
            match key.code {
                KeyCode::Esc => return Ok(RunState::Abort),
                KeyCode::Enter => match self.selected {
                    EditSelection::AcceptButton => {
                        return Ok(RunState::Complete(self.clone().into()));
                    }
                    EditSelection::DiscardButton => return Ok(RunState::Abort),
                    _ => {}
                },
                KeyCode::Up => self.selected = self.selected.up(self.is_mounted),
                KeyCode::Down => self.selected = self.selected.down(self.is_mounted),
                KeyCode::Left if self.selected.is_button() => {
                    self.selected = self.selected.left();
                }
                KeyCode::Right if self.selected.is_button() => {
                    self.selected = self.selected.right();
                }
                KeyCode::Tab => self.selected = self.selected.next(self.is_mounted),
                KeyCode::BackTab => self.selected = self.selected.previous(self.is_mounted),

                _ => match self.selected {
                    EditSelection::Name => {
                        self.name.handle_event(&event);
                    }
                    EditSelection::Device => {
                        self.device.handle_event(&event);
                    }
                    EditSelection::MountPoint => {
                        self.mount_point.handle_event(&event);
                    }
                    EditSelection::LuksName => {
                        self.luks_name.handle_event(&event);
                    }
                    EditSelection::Filesystem => {
                        self.filesystem.handle_event(&event);
                    }
                    EditSelection::AcceptButton | EditSelection::DiscardButton => {}
                },
            }
        }

        Ok(RunState::Running)
    }

    pub fn draw(&mut self, area: Rect, buf: &mut Buffer) {
        let area = area.centered(
            Constraint::Percentage(50),
            Constraint::Length(9), // top and bottom border + content
        );

        let border = Block::bordered()
            .padding(Padding::horizontal(1))
            .title(Line::from(" Edit Mount Record ").style(style::header_text()));
        let field_areas: [Rect; 7] = border
            .inner(area)
            .layout(&Layout::default().constraints([Constraint::Length(1); 7]));

        Clear.render(area, buf); // Clear space
        border.render(area, buf); // Draw the border of our modal

        let entry_layout = Layout::horizontal([Constraint::Length(13), Constraint::Fill(1)]);

        self.cursor = None;
        macro_rules! display_field {
            ($idx:expr, $label:expr, $variant:expr, $field:ident) => {{
                let [key_area, value_area] = field_areas[$idx].layout(&entry_layout);
                Text::from($label)
                    .style(style::header_text())
                    .render(key_area, buf);

                let scroll = self.$field.visual_scroll(value_area.width as usize);
                let style = if self.selected == $variant {
                    let x = self.$field.visual_cursor().max(scroll) - scroll;
                    self.cursor = Some(Position {
                        x: value_area.x + x as u16,
                        y: value_area.y,
                    });
                    style::highlight_text()
                } else if self.is_mounted && $variant != EditSelection::Name {
                    style::disabled_text()
                } else {
                    style::default_text()
                };

                Text::from(self.$field.value())
                    .style(style)
                    .render(value_area, buf);
            }};
        }

        display_field!(0, "Name:", EditSelection::Name, name);
        display_field!(1, "Device:", EditSelection::Device, device);
        display_field!(2, "Mount Point:", EditSelection::MountPoint, mount_point);
        display_field!(3, "LUKS Name:", EditSelection::LuksName, luks_name);
        display_field!(4, "Filesystem:", EditSelection::Filesystem, filesystem);

        let button_areas: [Rect; 3] = field_areas[6].layout(
            &Layout::horizontal([
                Constraint::Length(8),
                Constraint::Length(9),
                Constraint::Fill(1),
            ])
            .spacing(2),
        );

        Text::from("[Accept]")
            .style(style::button_style(
                self.selected == EditSelection::AcceptButton,
            ))
            .render(button_areas[0], buf);
        Text::from("[Discard]")
            .style(style::button_style(
                self.selected == EditSelection::DiscardButton,
            ))
            .render(button_areas[1], buf);
    }
}

impl From<EditModal> for TableRow {
    fn from(value: EditModal) -> Self {
        Self {
            name: value.name.value().into(),
            config: MountConfiguration {
                device: value.device.value().into(),
                mount_point: value.mount_point.value().into(),
                luks_decrypt_name: if value.luks_name.value().is_empty() {
                    None
                } else {
                    Some(value.luks_name.value().into())
                },
                filesystem: if value.filesystem.value().is_empty() {
                    None
                } else {
                    Some(value.filesystem.value().into())
                },
            },
            is_mounted: value.is_mounted,
            needs_mount: value.needs_mount,
        }
    }
}

#[derive(Debug)]
pub struct ConfirmModal {
    text: String,
    yes_selected: bool,
}

impl ConfirmModal {
    pub fn new(text: String) -> Self {
        Self {
            text,
            yes_selected: false,
        }
    }

    pub fn handle_input(&mut self) -> Result<RunState<bool>> {
        if let Event::Key(key) = event::read()?
            && key.kind.is_press()
        {
            match key.code {
                KeyCode::Esc => return Ok(RunState::Abort),
                KeyCode::Enter => return Ok(RunState::Complete(self.yes_selected)),
                KeyCode::Right => self.yes_selected = false,
                KeyCode::Left => self.yes_selected = true,
                _ => {}
            }
        }

        Ok(RunState::Running)
    }

    pub fn draw(&self, area: Rect, buf: &mut Buffer) {
        let area = area.centered(
            Constraint::Percentage(30),
            Constraint::Length(6), // top and bottom border + content
        );

        let border = Block::bordered()
            .padding(Padding::horizontal(1))
            .title(Line::from(" Confirm ").style(style::header_text()));

        let field_areas: [Rect; 2] = border.inner(area).layout(
            &Layout::default()
                .constraints([Constraint::Fill(1), Constraint::Length(1)])
                .spacing(1),
        );

        Clear.render(area, buf); // Clear space
        border.render(area, buf); // Draw the border of our modal

        Paragraph::new(self.text.as_str())
            .wrap(Wrap { trim: true })
            .render(field_areas[0], buf);

        let button_layout: [Rect; 3] = field_areas[1].layout(
            &Layout::horizontal([
                Constraint::Length(5),
                Constraint::Length(4),
                Constraint::Fill(1),
            ])
            .spacing(2),
        );

        Text::from("[Yes]")
            .style(style::button_style(self.yes_selected))
            .render(button_layout[0], buf);
        Text::from("[No]")
            .style(style::button_style(!self.yes_selected))
            .render(button_layout[1], buf);
    }
}

#[derive(Debug)]
pub struct NotifyModal {
    title: String,
    text: String,
    scroll_state: ScrollbarState,
}

impl NotifyModal {
    pub fn new<S: Into<String>>(title: &str, text: S) -> Self {
        let text = text.into();
        let num_lines = text.chars().filter(|&c| c == '\n').count();

        Self {
            title: title.to_owned(),
            text,
            scroll_state: ScrollbarState::new(num_lines),
        }
    }

    pub fn handle_input(&mut self) -> Result<RunState<()>> {
        if let Event::Key(key) = event::read()?
            && key.kind.is_press()
        {
            match key.code {
                KeyCode::Esc => return Ok(RunState::Abort),
                KeyCode::Enter => return Ok(RunState::Complete(())),
                KeyCode::Up => self.scroll_state.prev(),
                KeyCode::Down => self.scroll_state.next(),
                _ => {}
            }
        }

        Ok(RunState::Running)
    }

    pub fn draw(&mut self, area: Rect, buf: &mut Buffer) {
        let area = area.centered(
            Constraint::Length(75), // 70 character lines + padding, border, and scroll bar
            Constraint::Percentage(50), // top and bottom border + content
        );

        let border = Block::bordered()
            .padding(Padding::horizontal(1))
            .title(Line::from(format!(" {} ", self.title)).style(style::header_text()));

        let field_areas: [Rect; 2] = border
            .inner(area)
            .layout(&Layout::default().constraints([Constraint::Fill(1), Constraint::Length(1)]));

        Clear.render(area, buf); // Clear space
        border.render(area, buf); // Draw the border of our modal
        Paragraph::new(self.text.as_str())
            .style(style::default_text())
            .scroll((self.scroll_state.get_position() as u16, 0))
            .render(field_areas[0], buf);

        Scrollbar::new(ScrollbarOrientation::VerticalRight)
            .track_symbol(None)
            .render(field_areas[0], buf, &mut self.scroll_state);

        let button_area: [Rect; 2] = field_areas[1]
            .layout(&Layout::horizontal([Constraint::Length(4), Constraint::Fill(1)]).spacing(1));

        Text::from("[OK]")
            .style(style::button_selected_text())
            .render(button_area[0], buf);
    }
}
