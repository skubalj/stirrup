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

use crate::mount::{self, ConfigFile, MountConfiguration, probe_mtab};
use anyhow::{Context, Result};
use crossterm::event::{self, Event, KeyCode, KeyModifiers};
use ratatui::{
    Frame,
    layout::{Constraint, Layout, Margin, Rect},
    text::Line,
    widgets::{Block, Cell, Padding, Row, Table, TableState, Widget},
};

mod modal;
use modal::{ConfirmModal, EditModal, Modal, ModalState, NotifyModal};

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
    mounted_configs: Vec<MountConfiguration>,
    modal: ModalState,
}

impl MountTui {
    pub fn run(config: &ConfigFile) -> Result<Option<TuiActions>> {
        let mounted_configs = mount::probe_mtab().context("failed to probe /etc/mtab")?;

        let mut tui = Self {
            table_state: Default::default(),
            table_rows: make_table_rows(config, &mounted_configs),
            mounted_configs,
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
                    MountAction::Mount => actions.to_mount.push(row.config.name.clone()),
                    MountAction::Unmount => actions.to_unmount.push(row.config.name.clone()),
                    MountAction::None => {}
                }

                actions.configurations.push(row.config);
            }

            Ok(Some(actions))
        } else {
            Ok(None)
        }
    }

    fn handle_input(&mut self) -> Result<RunState<()>> {
        let event = event::read()?;

        // Ctrl+c abort handler
        if let Event::Key(key) = event
            && key.kind.is_press()
            && matches!(key.code, KeyCode::Char('c'))
            && key.modifiers.contains(KeyModifiers::CONTROL)
        {
            return Ok(RunState::Abort);
        }

        match &mut self.modal {
            ModalState::None => return self.handle_main_input(event),
            ModalState::EditModal(edit_modal) => match edit_modal.handle_input(event) {
                RunState::Running => {}
                RunState::Complete(row) => self.close_edit_modal(row),
                RunState::Abort => {
                    // If we were adding a new config, remove the empty entry from the table
                    let selected_idx = self.table_state.selected().unwrap();
                    if self.table_rows[selected_idx].is_empty() {
                        self.table_rows.remove(selected_idx);
                    }

                    self.modal = ModalState::None;
                }
            },
            ModalState::DeleteConfirmModal(confirm_modal) => {
                match confirm_modal.handle_input(event) {
                    RunState::Running => {}
                    RunState::Complete(true) => {
                        self.table_rows.remove(self.table_state.selected().unwrap());
                        self.modal = ModalState::None;
                    }
                    RunState::Complete(false) | RunState::Abort => self.modal = ModalState::None,
                }
            }
            ModalState::Notification(notification) => match notification.handle_input(event) {
                RunState::Running => {}
                RunState::Complete(()) | RunState::Abort => self.modal = ModalState::None,
            },
        }

        Ok(RunState::Running)
    }

    fn handle_main_input(&mut self, event: Event) -> Result<RunState<()>> {
        if let Event::Key(key) = event
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
                KeyCode::Char('r') => self.refresh_mounted_devices()?,
                KeyCode::Char('i') => self.open_info_modal(),
                _ => {}
            }
        }

        Ok(RunState::Running)
    }

    fn close_edit_modal(&mut self, mut row: TableRow) {
        row.update_is_mounted(&self.mounted_configs);

        let mut errors = row.validate();
        self.table_rows[self.table_state.selected().unwrap()] = row;
        self.table_rows
            .sort_by(|a, b| a.config.name.cmp(&b.config.name));

        if let Some(x) = self
            .table_rows
            .windows(2)
            .find(|window| window[0].config.name == window[1].config.name)
            .map(|x| x[0].config.name.as_str())
        {
            errors.push(format!(
                "Configuration '{x}' is duplicated.
            
                Configuration names must be unique. If you save the configurations
                without fixing this, one variant may overwrite another"
            ));
        }

        self.modal = if !errors.is_empty() {
            ModalState::Notification(NotifyModal::new("Error", errors.join("\n\n")))
        } else {
            ModalState::None
        };
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
                row.config.name
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
        self.table_rows.push(TableRow::default());
        self.table_state.select(Some(self.table_rows.len() - 1));
        self.modal = ModalState::EditModal(EditModal::new(self.table_rows.last().unwrap()));
    }

    fn edit_record(&mut self) {
        if let Some(idx) = self.table_state.selected()
            && let Some(row) = self.table_rows.get(idx)
        {
            self.modal = if row.is_mounted {
                ModalState::Notification(NotifyModal::new(
                    "Error",
                    "You cannnot edit a configuration while it is mounted",
                ))
            } else {
                ModalState::EditModal(EditModal::new(row))
            };
        }
    }

    fn refresh_mounted_devices(&mut self) -> Result<()> {
        self.mounted_configs = probe_mtab()?;
        for row in self.table_rows.iter_mut() {
            row.update_is_mounted(&self.mounted_configs);
        }

        Ok(())
    }

    fn draw(&mut self, frame: &mut Frame) {
        let help_text = KeyBindings::new(
            &[
                "Select: SPACEBAR",
                "Delete: DEL",
                "New: N",
                "Edit: E",
                "Refresh Mounted Devices: R",
                "Program Info: I",
                "Apply: ENTER",
                "Discard: ESCAPE",
            ],
            frame.area().width as usize - 2, // -2 for inset padding
        );

        let [table_area, help_area] = frame.area().layout(&Layout::default().constraints([
            Constraint::Fill(1),
            Constraint::Length(help_text.num_rows()),
        ]));

        self.draw_table(frame, table_area);

        frame.render_widget(
            help_text,
            help_area.inner(Margin {
                horizontal: 1,
                vertical: 0,
            }),
        );

        frame.render_stateful_widget(Modal, table_area, &mut self.modal);
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
                    Cell::from(item.config.name.as_str()).style(row_style),
                    Cell::from(item.config.device.as_str()).style(row_style),
                    Cell::from(item.config.mount_point.to_string_lossy()).style(row_style),
                    Cell::from(display_boolean(item.config.is_luks_encrypted)).style(row_style),
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
            Cell::from("Mount Point:").style(style::header_text()),
            Cell::from("LUKS Encrypted:").style(style::header_text()),
            Cell::from("Filesystem:").style(style::header_text()),
        ]);

        macro_rules! find_longest {
            ($mapping_fn:expr) => {
                self.table_rows
                    .iter()
                    .map($mapping_fn)
                    .max()
                    .and_then(|x| x.try_into().ok())
                    .unwrap_or(1)
            };
        }

        let col_constraints = [
            Constraint::Length(4),
            Constraint::Length(9),
            Constraint::Fill(find_longest!(|r| r.config.name.len())),
            Constraint::Fill(find_longest!(|r| r.config.device.len())),
            Constraint::Fill(find_longest!(|r| r.config.mount_point.as_os_str().len())),
            Constraint::Length(16),
            Constraint::Length(11),
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
    pub config: MountConfiguration,
    pub is_mounted: bool,
    pub needs_mount: MountAction,
}

impl TableRow {
    pub fn new(config: MountConfiguration, mounted: &[MountConfiguration]) -> Self {
        Self {
            config: config.clone(),
            is_mounted: Self::is_mounted(&config, mounted),
            needs_mount: MountAction::None,
        }
    }

    pub fn update_is_mounted(&mut self, mounted: &[MountConfiguration]) {
        let old_state = self.is_mounted;
        self.is_mounted = Self::is_mounted(&self.config, mounted);
        if self.is_mounted != old_state {
            self.needs_mount = MountAction::None;
        }
    }

    fn is_mounted(config: &MountConfiguration, mounted: &[MountConfiguration]) -> bool {
        mounted.iter().any(|m| m.mount_point == config.mount_point)
    }

    pub fn toggle_mount(&mut self) {
        self.needs_mount = match self.needs_mount {
            MountAction::None if self.is_mounted => MountAction::Unmount,
            MountAction::None => MountAction::Mount,
            MountAction::Mount | MountAction::Unmount => MountAction::None,
        }
    }

    /// Check whether the fields in this table make sense
    pub fn validate(&self) -> Vec<String> {
        let mut errors = Vec::new();
        if self.config.name.is_empty() {
            errors.push("The 'name' field cannot be empty.".into())
        }
        if self.config.device.is_empty() {
            errors.push("The 'device' field cannot be empty.".into())
        }
        if self.config.mount_point.as_os_str().is_empty() {
            errors.push("The 'mount_point' field cannot be empty.".into())
        }

        errors
    }

    pub fn is_empty(&self) -> bool {
        self.config.name.is_empty()
            && self.config.device.is_empty()
            && !self.config.is_luks_encrypted
            && self.config.mount_point.as_os_str().is_empty()
            && self.config.filesystem.is_none()
            && !self.is_mounted
            && self.needs_mount == MountAction::None
    }
}

impl Default for TableRow {
    fn default() -> Self {
        Self {
            config: MountConfiguration {
                name: Default::default(),
                device: Default::default(),
                is_luks_encrypted: Default::default(),
                mount_point: Default::default(),
                filesystem: Default::default(),
            },
            is_mounted: Default::default(),
            needs_mount: Default::default(),
        }
    }
}

fn make_table_rows(config: &ConfigFile, mounted: &[MountConfiguration]) -> Vec<TableRow> {
    let mut rows = Vec::new();
    for config in config.iter() {
        rows.push(TableRow::new(config.clone(), mounted));
    }
    rows
}

/// The set of actions that were specified on the TUI
#[derive(Debug, Default)]
pub struct TuiActions {
    pub configurations: Vec<MountConfiguration>,
    /// The names of configurations that need to be mounted
    pub to_mount: Vec<String>,
    /// The names of configurations that need to be unmounted
    pub to_unmount: Vec<String>,
}

#[derive(Debug)]
pub struct KeyBindings<'a> {
    num_rows: usize,
    bindings: &'a [&'a str],
    max_lengths: Vec<usize>,
}

impl<'a> KeyBindings<'a> {
    const PADDING: u16 = 6;

    fn new(bindings: &'a [&str], max_width: usize) -> Self {
        for num_rows in 1..bindings.len() {
            let num_columns = bindings.len().div_ceil(num_rows);
            let padding_length = Self::PADDING as usize * (num_columns - 1);
            let max_lengths = Self::calculate_max_lengths(bindings, num_columns);

            if max_lengths.iter().sum::<usize>() + padding_length < max_width {
                return Self {
                    num_rows,
                    bindings,
                    max_lengths,
                };
            }
        }

        // Default to every bindings on its own row
        Self {
            num_rows: bindings.len(),
            bindings,
            max_lengths: vec![bindings.iter().map(|x| x.len()).max().unwrap_or(1)],
        }
    }

    fn calculate_max_lengths(bindings: &[&str], num_cols: usize) -> Vec<usize> {
        bindings
            .chunks(num_cols)
            .fold(vec![0; num_cols], |mut a, n| {
                a.iter_mut()
                    .zip(n)
                    .for_each(|(width, binding)| *width = binding.len().max(*width));
                a
            })
    }

    pub fn num_rows(&self) -> u16 {
        self.num_rows.try_into().unwrap_or(1)
    }
}

impl<'a> Widget for KeyBindings<'a> {
    fn render(self, area: Rect, buf: &mut ratatui::prelude::Buffer)
    where
        Self: Sized,
    {
        let columns = Layout::horizontal(
            self.max_lengths
                .into_iter()
                .map(|x| Constraint::Length(x as u16)),
        )
        .spacing(Self::PADDING)
        .split(area);

        for (col_idx, col) in columns.iter().enumerate() {
            let rows = col.layout_vec(&Layout::vertical(std::iter::repeat_n(
                Constraint::Length(1),
                self.num_rows,
            )));

            for (row_idx, row) in rows.iter().enumerate() {
                if let Some(&b) = self.bindings.get((row_idx * columns.len()) + col_idx) {
                    Line::from(b).style(style::help_text()).render(*row, buf);
                }
            }
        }
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
