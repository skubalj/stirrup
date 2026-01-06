use anyhow::Result;
use crossterm::event::{self, Event, KeyCode};
use ratatui::{
    Frame,
    layout::{Constraint, Layout, Rect},
    style::{Style, Stylize},
    text::{Line, Text},
    widgets::{Block, Cell, Clear, Padding, Paragraph, Row, Table, TableState, Wrap},
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

#[derive(Debug, Default)]
enum Modal {
    #[default]
    None,
    EditModal(EditModal),
    DeleteConfirmModal(ConfirmModal),
    Notification(NotifyModal),
}

#[derive(Default)]
pub struct MountTui {
    table_state: TableState,
    table_rows: Vec<TableRow>,
    modal: Modal,
}

impl MountTui {
    pub fn run(config: &ConfigFile) -> Result<Option<TuiActions>> {
        let mounted = mount::probe_mtab()?;

        let mut tui = Self {
            table_state: Default::default(),
            table_rows: make_table_rows(&config, &mounted),
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
            Modal::None => return self.handle_main_input(),
            Modal::EditModal(edit_modal) => match edit_modal.handle_input()? {
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
                        Some(x) => Modal::Notification(NotifyModal::new(
                            "Error".into(),
                            format!(
                                "Configuration '{x}' is duplicated. Configuration names must be unique. If you save the configurations without fixing this, one variant may overwrite another"
                            ),
                        )),
                        None => Modal::None,
                    }
                }
                RunState::Abort => self.modal = Modal::None,
            },
            Modal::DeleteConfirmModal(confirm_modal) => match confirm_modal.handle_input()? {
                RunState::Running => {}
                RunState::Complete(true) => {
                    self.table_rows.remove(self.table_state.selected().unwrap());
                    self.modal = Modal::None;
                }
                RunState::Complete(false) | RunState::Abort => self.modal = Modal::None,
            },
            Modal::Notification(notification) => match notification.handle_input()? {
                RunState::Running => {}
                RunState::Complete(()) | RunState::Abort => self.modal = Modal::None,
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
                _ => {}
            }
        }

        Ok(RunState::Running)
    }

    fn toggle_mounted(&mut self) {
        if let Some(idx) = self.table_state.selected() {
            if let Some(row) = self.table_rows.get_mut(idx) {
                row.toggle_mount();
            }
        }
    }

    fn open_delete_modal(&mut self) {
        if let Some(idx) = self.table_state.selected() {
            if let Some(row) = self.table_rows.get(idx) {
                self.modal = Modal::DeleteConfirmModal(ConfirmModal::new(format!(
                    "Are you sure you want to delete '{}'?",
                    row.name
                )));
            }
        }
    }

    fn add_record(&mut self) {
        self.table_rows.push(TableRow::new());
        self.table_state.select(Some(self.table_rows.len() - 1));
        self.modal = Modal::EditModal(EditModal::new(self.table_rows.last().unwrap()));
    }

    fn edit_record(&mut self) {
        if let Some(idx) = self.table_state.selected() {
            if let Some(row) = self.table_rows.get(idx) {
                self.modal = Modal::EditModal(EditModal::new(row))
            }
        }
    }

    fn draw(&mut self, frame: &mut Frame) {
        let layout = Layout::default()
            .constraints([Constraint::Fill(1), Constraint::Length(1)])
            .split(frame.area());

        self.draw_table(frame, layout[0]);
        frame.render_widget(
            Text::from(
                " Mount/Unmount: SPACEBAR    Delete: DEL    New: N    Edit: E    Apply: ENTER    Discard: ESCAPE",
            ).dark_gray(),
            layout[1],
        );

        match &self.modal {
            Modal::None => {}
            Modal::EditModal(edit_modal) => edit_modal.draw(frame),
            Modal::DeleteConfirmModal(confirm_modal) => confirm_modal.draw(frame),
            Modal::Notification(notification) => notification.draw(frame),
        }
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
                    Style::default().green()
                } else {
                    Style::default()
                };

                Row::from_iter(vec![
                    Cell::from(format!("{:3}", idx + 1)).style(row_style),
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
            Constraint::Length(4),
            Constraint::Length(9),
            Constraint::Fill(1),
            Constraint::Fill(1),
            Constraint::Fill(1),
            Constraint::Fill(1),
        ];

        let table = Table::new(table_rows, col_constraints)
            .header(header)
            .row_highlight_style(Style::default().black().on_dark_gray())
            .block(
                Block::bordered()
                    .padding(Padding::horizontal(1))
                    .title(Line::from(" Select Mounts ").bold()),
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
                .any(|m| m.device == config.device && m.mount_point == config.mount_point),
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

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
enum EditSelection {
    #[default]
    Name,
    Device,
    MountPoint,
    Filesystem,
    AcceptButton,
    DiscardButton,
}

impl EditSelection {
    pub fn is_button(self) -> bool {
        match self {
            Self::AcceptButton | Self::DiscardButton => true,
            _ => false,
        }
    }

    pub fn up(self) -> Self {
        match self {
            Self::Name | Self::Device => Self::Name,
            Self::MountPoint => Self::Device,
            Self::Filesystem => Self::MountPoint,
            Self::AcceptButton | Self::DiscardButton => Self::Filesystem,
        }
    }

    pub fn down(self) -> Self {
        match self {
            Self::Name => Self::Device,
            Self::Device => Self::MountPoint,
            Self::MountPoint => Self::Filesystem,
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

    pub fn next(self) -> Self {
        match self {
            Self::Name => Self::Device,
            Self::Device => Self::MountPoint,
            Self::MountPoint => Self::Filesystem,
            Self::Filesystem => Self::AcceptButton,
            Self::AcceptButton | Self::DiscardButton => Self::DiscardButton,
        }
    }

    pub fn previous(self) -> Self {
        match self {
            Self::Name | Self::Device => Self::Name,
            Self::MountPoint => Self::Device,
            Self::Filesystem => Self::MountPoint,
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
    filesystem: Input,

    is_mounted: bool,
    needs_mount: MountAction,
    selected: EditSelection,
}

impl EditModal {
    pub fn new(row: &TableRow) -> Self {
        Self {
            name: Input::new(row.name.clone()),
            device: Input::new(row.config.device.clone()),
            mount_point: Input::new(row.config.mount_point.to_string_lossy().to_string()),
            filesystem: Input::new(row.config.filesystem.clone().unwrap_or_default()),
            is_mounted: row.is_mounted,
            needs_mount: row.needs_mount,
            selected: Default::default(),
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
                KeyCode::Up => self.selected = self.selected.up(),
                KeyCode::Down => self.selected = self.selected.down(),
                KeyCode::Left if self.selected.is_button() => {
                    self.selected = self.selected.left();
                }
                KeyCode::Right if self.selected.is_button() => {
                    self.selected = self.selected.right();
                }
                KeyCode::Tab => self.selected = self.selected.next(),
                KeyCode::BackTab => self.selected = self.selected.previous(),

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
                    EditSelection::Filesystem => {
                        self.filesystem.handle_event(&event);
                    }
                    _ => {}
                },
            }
        }

        Ok(RunState::Running)
    }

    pub fn draw(&self, frame: &mut Frame) {
        let area = frame.area().centered(
            Constraint::Percentage(50),
            Constraint::Length(8), // top and bottom border + content
        );

        let border = Block::bordered()
            .padding(Padding::horizontal(1))
            .title(Line::from(" Edit Mount Record ").bold());
        let field_areas: [Rect; 6] = border
            .inner(area)
            .layout(&Layout::default().constraints([Constraint::Length(1); 6]));

        frame.render_widget(Clear, area); // Clear space
        frame.render_widget(border, area); // Draw the border of our modal

        let entry_layout = Layout::horizontal([Constraint::Length(13), Constraint::Fill(1)]);

        macro_rules! display_field {
            ($idx:expr, $label:expr, $variant:expr, $field:ident) => {{
                let [key_area, value_area] = field_areas[$idx].layout(&entry_layout);
                frame.render_widget(Text::from($label).bold(), key_area);

                let scroll = self.$field.visual_scroll(value_area.width as usize);

                let style = if self.selected == $variant {
                    let x = self.$field.visual_cursor().max(scroll) - scroll;
                    frame.set_cursor_position((value_area.x + x as u16, value_area.y));
                    Style::default().black().on_dark_gray().bold()
                } else {
                    Style::default()
                };

                frame.render_widget(Text::from(self.$field.value()).style(style), value_area);
            }};
        }

        display_field!(0, "Name:", EditSelection::Name, name);
        display_field!(1, "Device:", EditSelection::Device, device);
        display_field!(2, "Mount Point:", EditSelection::MountPoint, mount_point);
        display_field!(3, "Filesystem:", EditSelection::Filesystem, filesystem);

        // Empty space

        let button_areas: [Rect; 3] = field_areas[5].layout(
            &Layout::horizontal([
                Constraint::Length(8),
                Constraint::Length(9),
                Constraint::Fill(1),
            ])
            .spacing(2),
        );

        frame.render_widget(
            Text::from("[Accept]")
                .style(button_style(self.selected == EditSelection::AcceptButton)),
            button_areas[0],
        );
        frame.render_widget(
            Text::from("[Discard]")
                .style(button_style(self.selected == EditSelection::DiscardButton)),
            button_areas[1],
        );
    }
}

impl From<EditModal> for TableRow {
    fn from(value: EditModal) -> Self {
        Self {
            name: value.name.value().into(),
            config: MountConfiguration {
                device: value.device.value().into(),
                mount_point: value.mount_point.value().into(),
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

    pub fn draw(&self, frame: &mut Frame) {
        let area = frame.area().centered(
            Constraint::Percentage(30),
            Constraint::Length(6), // top and bottom border + content
        );

        let border = Block::bordered()
            .padding(Padding::horizontal(1))
            .title(Line::from(" Confirm ").bold());

        let field_areas: [Rect; 2] = border.inner(area).layout(
            &Layout::default()
                .constraints([Constraint::Fill(1), Constraint::Length(1)])
                .spacing(1),
        );

        frame.render_widget(Clear, area); // Clear space
        frame.render_widget(border, area); // Draw the border of our modal

        frame.render_widget(
            Paragraph::new(self.text.as_str()).wrap(Wrap { trim: true }),
            field_areas[0],
        );

        let button_layout: [Rect; 3] = field_areas[1].layout(
            &Layout::horizontal([
                Constraint::Length(5),
                Constraint::Length(4),
                Constraint::Fill(1),
            ])
            .spacing(2),
        );

        frame.render_widget(
            Text::from("[Yes]").style(button_style(self.yes_selected)),
            button_layout[0],
        );
        frame.render_widget(
            Text::from("[No]").style(button_style(!self.yes_selected)),
            button_layout[1],
        );
    }
}

fn button_style(selected: bool) -> Style {
    if selected {
        Style::default().bold().black().on_dark_gray()
    } else {
        Style::default().bold().dark_gray()
    }
}

#[derive(Debug)]
struct NotifyModal {
    title: String,
    text: String,
}

impl NotifyModal {
    pub fn new(title: String, text: String) -> Self {
        Self { title, text }
    }

    pub fn handle_input(&mut self) -> Result<RunState<()>> {
        if let Event::Key(key) = event::read()?
            && key.kind.is_press()
        {
            match key.code {
                KeyCode::Esc => return Ok(RunState::Abort),
                KeyCode::Enter => return Ok(RunState::Complete(())),
                _ => {}
            }
        }

        Ok(RunState::Running)
    }

    pub fn draw(&self, frame: &mut Frame) {
        let area = frame.area().centered(
            Constraint::Percentage(30),
            Constraint::Length(8), // top and bottom border + content
        );

        let border = Block::bordered()
            .padding(Padding::horizontal(1))
            .title(Line::from(format!(" {} ", self.title)))
            .bold();

        let field_areas: [Rect; 2] = border
            .inner(area)
            .layout(&Layout::default().constraints([Constraint::Fill(1), Constraint::Length(1)]));

        frame.render_widget(Clear, area); // Clear space
        frame.render_widget(border, area); // Draw the border of our modal
        frame.render_widget(
            Paragraph::new(self.text.as_str())
                .wrap(Wrap { trim: true })
                .style(Style::default()),
            field_areas[0],
        );

        let button_area: [Rect; 2] = field_areas[1].layout(&Layout::horizontal([
            Constraint::Length(4),
            Constraint::Fill(1),
        ]));
        frame.render_widget(Text::from("[OK]").on_dark_gray(), button_area[0]);
    }
}
