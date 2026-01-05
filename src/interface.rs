use anyhow::Result;
use crossterm::event::{self, Event, KeyCode};
use ratatui::{
    Frame,
    layout::{Constraint, Layout, Rect},
    style::{Style, Stylize},
    text::{Line, Text},
    widgets::{Block, Cell, Clear, Padding, Row, Table, TableState},
};
use std::collections::HashMap;

use crate::mount::{self, ConfigFile, MountConfiguration};

#[derive(Default)]
pub struct MountTui {
    state: TableState,
    table_rows: Vec<TableRow>,
    modal: Option<EditModal>,
    exit: bool,
}

impl MountTui {
    pub fn run(config: &ConfigFile) -> Result<TuiActions> {
        let mounted = mount::probe_mtab()?;

        let mut tui = Self {
            state: Default::default(),
            table_rows: make_table_rows(&config, &mounted),
            modal: None,
            exit: false,
        };

        // Run the UI loop
        ratatui::run::<_, anyhow::Result<_>>(|terminal| {
            while !tui.exit {
                // Render
                terminal.draw(|frame| {
                    tui.draw(frame);
                    if let Some(ref modal) = tui.modal {
                        modal.draw(frame);
                    }
                })?;

                // Handle input
                if let Some(ref mut modal) = tui.modal {
                    modal.handle_input()?;
                    if modal.exit {
                        tui.modal = None;
                    }
                } else {
                    tui.handle_input()?;
                }
            }
            Ok(())
        })?;

        // Unpack our rows into actions that the backend needs to perform
        let mut actions = TuiActions::default();
        for row in tui.table_rows.into_iter() {
            match row.add_remove {
                AddRemove::Add => {
                    actions.to_create.insert(row.name.clone(), row.config);
                }
                AddRemove::Remove => actions.to_remove.push(row.name.clone()),
                AddRemove::None => {}
            }

            match row.needs_mount {
                MountAction::Mount => actions.to_mount.push(row.name),
                MountAction::Unmount => actions.to_unmount.push(row.name),
                MountAction::None => {}
            }
        }

        Ok(actions)
    }

    fn handle_input(&mut self) -> Result<()> {
        if let Event::Key(key) = event::read()?
            && key.kind.is_press()
        {
            match key.code {
                KeyCode::Esc => {
                    self.exit = true;
                    self.table_rows.clear();
                }
                KeyCode::Enter => self.exit = true,
                KeyCode::Up => self.previous_row(),
                KeyCode::Down => self.next_row(),
                KeyCode::Char(' ') => self.toggle_mounted(),
                KeyCode::Char('-') | KeyCode::Delete => self.toggle_delete(),
                KeyCode::Char('+') | KeyCode::Char('n') => self.add_record(),
                KeyCode::Char('e') => self.edit_record(),
                _ => {}
            }
        }

        Ok(())
    }

    fn next_row(&mut self) {
        self.state.select(Some(
            self.state
                .selected()
                .map(|x| (x + 1) % self.table_rows.len())
                .unwrap_or_default(),
        ));
    }

    fn previous_row(&mut self) {
        let num_items = self.table_rows.len();
        self.state.select(Some(
            self.state
                .selected()
                .map(|x| (x + num_items - 1) % num_items)
                .unwrap_or_default(),
        ));
    }

    fn toggle_mounted(&mut self) {
        if let Some(idx) = self.state.selected() {
            if let Some(row) = self.table_rows.get_mut(idx) {
                row.toggle_mount();
            }
        }
    }

    fn toggle_delete(&mut self) {
        if let Some(idx) = self.state.selected() {
            if let Some(row) = self.table_rows.get_mut(idx) {
                match row.add_remove {
                    AddRemove::None => row.add_remove = AddRemove::Remove,
                    AddRemove::Add => {
                        self.table_rows.remove(idx);
                    }
                    AddRemove::Remove => row.add_remove = AddRemove::None,
                }
            }
        }
    }

    fn add_record(&mut self) {
        let record = TableRow::new();
        self.modal = Some(EditModal::new(self.table_rows.len(), &record));
        self.table_rows.push(record);
    }

    fn edit_record(&mut self) {
        if let Some(idx) = self.state.selected() {
            if let Some(row) = self.table_rows.get_mut(idx) {
                self.modal = Some(EditModal::new(idx, row))
            }
        }
    }

    fn draw(&mut self, frame: &mut Frame) {
        let layout = Layout::default()
            .constraints([Constraint::Fill(1), Constraint::Length(2)])
            .split(frame.area());

        self.draw_table(frame, layout[0]);
        frame.render_widget(
            Text::from(
                "Mount/Unmount: SPACEBAR    Delete: DEL    New: N    Edit: E    Apply: ENTER    Discard: ESCAPE",
            ).dark_gray(),
            layout[1],
        );
    }

    fn draw_table(&mut self, frame: &mut Frame, area: Rect) {
        let table_rows: Vec<Row> = self
            .table_rows
            .iter()
            .enumerate()
            .map(|(idx, item)| {
                let mut is_mounted =
                    display_boolean(item.is_mounted ^ item.needs_mount.changed()).to_owned();
                if item.needs_mount.changed() {
                    is_mounted += " (!)";
                }

                let (row_style, bullet) = match (item.add_remove, item.needs_mount) {
                    (AddRemove::Add, _) => (Style::default().green(), '+'),
                    (AddRemove::Remove, _) => (Style::default().red(), '-'),
                    (_, MountAction::Mount | MountAction::Unmount) => {
                        (Style::default().green(), '*')
                    }
                    _ => (Style::default(), ' '),
                };

                Row::from_iter(vec![
                    Cell::from(format!(" {bullet} {:3}", idx + 1)).style(row_style),
                    Cell::from(is_mounted).style(row_style),
                    Cell::from(item.name.as_str()).style(row_style),
                    Cell::from(item.config.device.as_str()).style(row_style),
                    Cell::from(item.config.mount_point.to_string_lossy()).style(row_style),
                    Cell::from(item.config.filesystem.as_deref().unwrap_or_default())
                        .style(row_style),
                ])
            })
            .collect();

        let header_format = Style::default().bold();
        let header = Row::from_iter([
            Cell::from("").style(header_format),
            Cell::from("Mounted:").style(header_format),
            Cell::from("Name:").style(header_format),
            Cell::from("Device:").style(header_format),
            Cell::from("Mount Point:").style(header_format),
            Cell::from("Filesystem:").style(header_format),
        ]);

        let col_constraints = [
            Constraint::Length(7),
            Constraint::Length(9),
            Constraint::Fill(1),
            Constraint::Fill(1),
            Constraint::Fill(1),
            Constraint::Fill(1),
        ];

        let table = Table::new(table_rows, col_constraints)
            .header(header)
            .row_highlight_style(
                Style::default()
                    .fg(ratatui::style::Color::White)
                    .bg(ratatui::style::Color::Blue),
            )
            .block(
                Block::bordered()
                    .padding(Padding::horizontal(1))
                    .title(Line::from(" Select Mounts: ").bold()),
            );

        frame.render_stateful_widget(table, area, &mut self.state);
    }
}

fn display_boolean(b: bool) -> &'static str {
    if b { "Yes" } else { "No" }
}

#[derive(Debug, Default, Clone, Copy)]
enum AddRemove {
    #[default]
    None,
    Add,
    Remove,
}

#[derive(Debug, Default, Clone, Copy)]
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
    pub add_remove: AddRemove,
    pub needs_mount: MountAction,
}

impl TableRow {
    pub fn new() -> Self {
        Self {
            name: Default::default(),
            config: MountConfiguration {
                device: Default::default(),
                mount_point: Default::default(),
                filesystem: Default::default(),
            },
            is_mounted: Default::default(),
            add_remove: AddRemove::Add,
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
                .any(|m| m.device == config.device && m.mount_point == config.mount_point),
            add_remove: AddRemove::None,
            needs_mount: MountAction::None,
        });
    }
    rows
}

/// The set of actions that were specified on the TUI
#[derive(Debug, Default)]
pub struct TuiActions {
    /// The set of configurations that should be created
    pub to_create: HashMap<String, MountConfiguration>,
    /// The names of the configurations that should be deleted
    pub to_remove: Vec<String>,
    /// The names of configurations that need to be mounted
    pub to_mount: Vec<String>,
    /// The names of configurations that need to be unmounted
    pub to_unmount: Vec<String>,
}

struct EditModal {
    idx: usize,
    name: String,
    device: String,
    mount_point: String,
    filesystem: String,

    is_mounted: bool,
    add_remove: AddRemove,
    needs_mount: MountAction,

    selected: usize,
    pub exit: bool,
}

impl EditModal {
    const NUM_EDIT_FIELDS: usize = 4;

    pub fn new(idx: usize, row: &TableRow) -> Self {
        Self {
            idx,
            name: row.name.clone(),
            device: row.config.device.clone(),
            mount_point: row.config.mount_point.to_string_lossy().to_string(),
            filesystem: row.config.filesystem.clone().unwrap_or_default(),
            is_mounted: row.is_mounted,
            add_remove: row.add_remove,
            needs_mount: row.needs_mount,
            selected: 0,
            exit: false,
        }
    }

    pub fn result(self) -> (usize, TableRow) {
        (
            self.idx,
            TableRow {
                name: self.name,
                config: MountConfiguration {
                    device: self.device,
                    mount_point: self.mount_point.into(),
                    filesystem: if self.filesystem.is_empty() {
                        None
                    } else {
                        Some(self.filesystem)
                    },
                },
                is_mounted: self.is_mounted,
                add_remove: self.add_remove,
                needs_mount: self.needs_mount,
            },
        )
    }

    pub fn handle_input(&mut self) -> Result<()> {
        if let Event::Key(key) = event::read()?
            && key.kind.is_press()
        {
            match key.code {
                KeyCode::Esc => self.exit = true,
                KeyCode::Down => self.selected = (self.selected + 1) % Self::NUM_EDIT_FIELDS,
                KeyCode::Up => {
                    self.selected =
                        (self.selected + Self::NUM_EDIT_FIELDS - 1) % Self::NUM_EDIT_FIELDS
                }
                _ => {}
            }
        }

        Ok(())
    }

    pub fn draw(&self, frame: &mut Frame) {
        let area = frame.area().centered(
            Constraint::Percentage(50),
            Constraint::Length(8), // top and bottom border + content
        );

        let border = Block::bordered()
            .padding(Padding::horizontal(1))
            .title(Line::from(" Edit Mount Record: ").bold());
        let field_areas: [Rect; 6] = border
            .inner(area)
            .layout(&Layout::default().constraints([Constraint::Length(1); 6]));

        frame.render_widget(Clear, area); // Clear space
        frame.render_widget(border, area); // Draw the border of our modal

        let entry_layout = Layout::horizontal([Constraint::Length(13), Constraint::Fill(1)]);

        macro_rules! display_field {
            ($idx:expr, $label:expr, $field:ident) => {{
                let [key_area, value_area] = field_areas[$idx].layout(&entry_layout);
                frame.render_widget(Text::from($label).bold(), key_area);

                let style = if self.selected == $idx {
                    Style::default().on_blue()
                } else {
                    Style::default()
                };

                frame.render_widget(Text::from(self.$field.as_str()).style(style), value_area);
            }};
        }

        display_field!(0, "Name:", name);
        display_field!(1, "Device:", device);
        display_field!(2, "Mount Point:", mount_point);
        display_field!(3, "Filesystem:", filesystem);

        // Empty space
        frame.render_widget(
            Text::from("Accept: ENTER    Discard: ESCAPE").dark_gray(),
            field_areas[5],
        );
    }
}
