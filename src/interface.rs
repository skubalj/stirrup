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

use anyhow::{Context, Result};
use crossterm::event::{self, Event, KeyCode};
use ratatui::{
    Frame,
    buffer::Buffer,
    layout::{Constraint, Layout, Position, Rect},
    text::{Line, Text},
    widgets::{
        Block, Cell, Clear, Padding, Paragraph, Row, Scrollbar, ScrollbarOrientation,
        ScrollbarState, StatefulWidget, Table, TableState, Widget, Wrap,
    },
};
use std::collections::BTreeMap;
use tui_input::{Input, backend::crossterm::EventHandler};

use crate::mount::{self, ConfigFile, MountConfiguration};

/// Describes the state of a currently running system
enum RunState<T> {
    Running,
    Complete(T),
    Abort,
}

#[derive(Default)]
pub struct MountTui {
    table_state: TableState,
    table_rows: Vec<TableRow>,
    modal: ModalState,
}

impl MountTui {
    pub fn run(config: &ConfigFile) -> Result<Option<TuiActions>> {
        let mounted = mount::probe_mtab().context("failed to probe /etc/mtab")?;

        let mut tui = Self {
            table_state: Default::default(),
            table_rows: make_table_rows(config, &mounted),
            modal: Default::default(),
        };

        // Run the UI loop
        let table_rows = ratatui::run::<_, anyhow::Result<_>>(|terminal| {
            loop {
                terminal.draw(|frame| tui.draw(frame))?;
                match tui.handle_input()? {
                    RunState::Running => {}
                    RunState::Complete(()) => return Ok(Some(tui.table_rows)),
                    RunState::Abort => return Ok(None),
                }
            }
        })?;

        // Box our table rows up into the configuration format
        if let Some(table_rows) = table_rows {
            let mut actions = TuiActions::default();
            for row in table_rows.into_iter() {
                match row.needs_mount {
                    MountAction::Mount => actions.to_mount.push(row.name.clone()),
                    MountAction::Unmount => actions.to_unmount.push(row.name.clone()),
                    MountAction::None => {}
                }

                actions.configurations.insert(row.name, row.config);
            }

            Ok(Some(actions))
        } else {
            Ok(None)
        }
    }

    fn handle_input(&mut self) -> Result<RunState<()>> {
        match &mut self.modal {
            ModalState::None => return self.handle_main_input(),
            ModalState::EditModal(edit_modal) => match edit_modal.handle_input()? {
                RunState::Running => {}
                RunState::Complete(row) => {
                    self.table_rows[self.table_state.selected().unwrap()] = row;
                    self.table_rows.sort_by(|a, b| a.name.cmp(&b.name));

                    self.modal = match self
                        .table_rows
                        .windows(2)
                        .find(|window| window[0].name == window[1].name)
                        .map(|x| x[0].name.as_str())
                    {
                        Some(x) => ModalState::Notification(NotifyModal::new(
                            "Error",
                            format!(
                                "Configuration '{x}' is duplicated.

Configuration names must be unique. If you save the configurations
without fixing this, one variant may overwrite another"
                            ),
                        )),
                        None => ModalState::None,
                    }
                }
                RunState::Abort => self.modal = ModalState::None,
            },
            ModalState::DeleteConfirmModal(confirm_modal) => match confirm_modal.handle_input()? {
                RunState::Running => {}
                RunState::Complete(true) => {
                    self.table_rows.remove(self.table_state.selected().unwrap());
                    self.modal = ModalState::None;
                }
                RunState::Complete(false) | RunState::Abort => self.modal = ModalState::None,
            },
            ModalState::Notification(notification) => match notification.handle_input()? {
                RunState::Running => {}
                RunState::Complete(()) | RunState::Abort => self.modal = ModalState::None,
            },
        }

        Ok(RunState::Running)
    }

    fn handle_main_input(&mut self) -> Result<RunState<()>> {
        if let Event::Key(key) = event::read()?
            && key.kind.is_press()
        {
            match key.code {
                KeyCode::Esc => return Ok(RunState::Abort),
                KeyCode::Enter => return Ok(RunState::Complete(())),
                KeyCode::Up => self.table_state.select_previous(),
                KeyCode::Down => self.table_state.select_next(),
                KeyCode::Char(' ') => self.toggle_mounted(),
                KeyCode::Char('-') | KeyCode::Delete => self.open_delete_modal(),
                KeyCode::Char('+') | KeyCode::Char('n') => self.add_record(),
                KeyCode::Char('e') => self.edit_record(),
                KeyCode::Char('i') => self.open_info_modal(),
                _ => {}
            }
        }

        Ok(RunState::Running)
    }

    fn toggle_mounted(&mut self) {
        if let Some(idx) = self.table_state.selected()
            && let Some(row) = self.table_rows.get_mut(idx)
        {
            row.toggle_mount();
        }
    }

    fn open_delete_modal(&mut self) {
        if let Some(idx) = self.table_state.selected()
            && let Some(row) = self.table_rows.get(idx)
        {
            self.modal = ModalState::DeleteConfirmModal(ConfirmModal::new(format!(
                "Are you sure you want to delete '{}'?",
                row.name
            )));
        }
    }

    fn open_info_modal(&mut self) {
        self.modal = ModalState::Notification(NotifyModal::new(
            "Stirrup 🐴",
            "Stirrup: A TUI Mount Manager

When you need to mount up, put your foot in the stirrup
            
Copyright (C) 2026 Joseph Skubal

This program is free software: you can redistribute it and/or modify
it under the terms of the GNU General Public License as published by
the Free Software Foundation, either version 3 of the License, or
any later version.

This program is distributed in the hope that it will be useful,
but WITHOUT ANY WARRANTY; without even the implied warranty of
MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE.  See the
GNU General Public License for more details.",
        ))
    }

    fn add_record(&mut self) {
        self.table_rows.push(TableRow::new());
        self.table_state.select(Some(self.table_rows.len() - 1));
        self.modal = ModalState::EditModal(EditModal::new(self.table_rows.last().unwrap()));
    }

    fn edit_record(&mut self) {
        if let Some(idx) = self.table_state.selected()
            && let Some(row) = self.table_rows.get(idx)
        {
            self.modal = ModalState::EditModal(EditModal::new(row))
        }
    }

    fn draw(&mut self, frame: &mut Frame) {
        let layout = Layout::default()
            .constraints([Constraint::Fill(1), Constraint::Length(1)])
            .split(frame.area());

        self.draw_table(frame, layout[0]);
        frame.render_widget(
            Text::from(
                " Mount/Unmount: SPACEBAR    Delete: DEL    New: N    Edit: E    Program Info: I    Apply: ENTER    Discard: ESCAPE",
            ).style(style::help_text()),
            layout[1],
        );

        frame.render_stateful_widget(Modal, layout[0], &mut self.modal);
        self.modal.set_cursor_position(frame);
    }

    fn draw_table(&mut self, frame: &mut Frame, area: Rect) {
        let table_rows: Vec<Row> = self
            .table_rows
            .iter()
            .enumerate()
            .map(|(idx, item)| {
                let mut is_mounted =
                    display_boolean(item.is_mounted ^ item.needs_mount.changed()).to_owned();

                let row_style = if item.needs_mount.changed() {
                    is_mounted += " *";
                    style::table_selected_row()
                } else {
                    style::default_text()
                };

                Row::from_iter(vec![
                    Cell::from(format!("{:3}", idx + 1)).style(row_style),
                    Cell::from(is_mounted).style(row_style),
                    Cell::from(item.name.as_str()).style(row_style),
                    Cell::from(item.config.device.as_str()).style(row_style),
                    Cell::from(item.config.luks_decrypt_name.as_deref().unwrap_or_default())
                        .style(row_style),
                    Cell::from(item.config.mount_point.to_string_lossy()).style(row_style),
                    Cell::from(item.config.filesystem.as_deref().unwrap_or_default())
                        .style(row_style),
                ])
            })
            .collect();

        let header = Row::from_iter([
            Cell::from("").style(style::header_text()),
            Cell::from("Mounted:").style(style::header_text()),
            Cell::from("Name:").style(style::header_text()),
            Cell::from("Device:").style(style::header_text()),
            Cell::from("LUKS Name:").style(style::header_text()),
            Cell::from("Mount Point:").style(style::header_text()),
            Cell::from("Filesystem:").style(style::header_text()),
        ]);

        let col_constraints = [
            Constraint::Length(4),
            Constraint::Length(9),
            Constraint::Fill(1),
            Constraint::Fill(1),
            Constraint::Fill(1),
            Constraint::Fill(1),
            Constraint::Fill(1),
        ];

        let table = Table::new(table_rows, col_constraints)
            .header(header)
            .row_highlight_style(style::highlight_text())
            .block(
                Block::bordered()
                    .padding(Padding::horizontal(1))
                    .title(Line::from(" Select Mounts ").style(style::header_text())),
            );

        frame.render_stateful_widget(table, area, &mut self.table_state);
    }
}

fn display_boolean(b: bool) -> &'static str {
    if b { "Yes" } else { "No" }
}

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
enum MountAction {
    #[default]
    None,
    Mount,
    Unmount,
}

impl MountAction {
    pub fn changed(self) -> bool {
        match self {
            Self::Mount | Self::Unmount => true,
            Self::None => false,
        }
    }
}

#[derive(Debug, Clone)]
struct TableRow {
    pub name: String,
    pub config: MountConfiguration,
    pub is_mounted: bool,
    pub needs_mount: MountAction,
}

impl TableRow {
    pub fn new() -> Self {
        Self {
            name: Default::default(),
            config: MountConfiguration {
                device: Default::default(),
                luks_decrypt_name: Default::default(),
                mount_point: Default::default(),
                filesystem: Default::default(),
            },
            is_mounted: Default::default(),
            needs_mount: Default::default(),
        }
    }

    pub fn toggle_mount(&mut self) {
        self.needs_mount = match self.needs_mount {
            MountAction::None if self.is_mounted => MountAction::Unmount,
            MountAction::None => MountAction::Mount,
            MountAction::Mount | MountAction::Unmount => MountAction::None,
        }
    }
}

fn make_table_rows(config: &ConfigFile, mounted: &[MountConfiguration]) -> Vec<TableRow> {
    let mut rows = Vec::new();
    for (name, config) in config.iter() {
        rows.push(TableRow {
            name: name.to_owned(),
            config: config.clone(),
            is_mounted: mounted
                .iter()
                .any(|m| &m.mount_point == &config.mount_point),
            needs_mount: MountAction::None,
        });
    }
    rows
}

/// The set of actions that were specified on the TUI
#[derive(Debug, Default)]
pub struct TuiActions {
    pub configurations: BTreeMap<String, MountConfiguration>,
    /// The names of configurations that need to be mounted
    pub to_mount: Vec<String>,
    /// The names of configurations that need to be unmounted
    pub to_unmount: Vec<String>,
}

#[derive(Debug, Default)]
enum ModalState {
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
struct Modal;

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
struct EditModal {
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
struct ConfirmModal {
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
struct NotifyModal {
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

/// Definitions of all the styles so that things stay consistent
mod style {
    use ratatui::style::Style;

    pub const fn default_text() -> Style {
        Style::new()
    }

    pub const fn disabled_text() -> Style {
        default_text().dark_gray()
    }

    pub const fn header_text() -> Style {
        default_text().blue().bold()
    }

    pub const fn table_selected_row() -> Style {
        default_text().green()
    }

    pub const fn highlight_text() -> Style {
        default_text().black().on_dark_gray().bold()
    }

    pub const fn button_style(selected: bool) -> Style {
        if selected {
            button_selected_text()
        } else {
            button_text()
        }
    }

    pub const fn button_text() -> Style {
        default_text().bold().dark_gray()
    }

    pub const fn button_selected_text() -> Style {
        default_text().black().on_dark_gray().bold()
    }

    pub const fn help_text() -> Style {
        default_text().dark_gray()
    }
}
