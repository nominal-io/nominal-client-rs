use std::future::Future;
use std::io;
use std::path::{Path, PathBuf};
use std::sync::mpsc;
use std::time::{Duration, Instant};

use anyhow::{Context, Result};
use chrono::{DateTime, Utc};
use crossterm::event::{
    self, DisableMouseCapture, EnableMouseCapture, Event, KeyCode, KeyEvent, KeyModifiers,
    MouseButton, MouseEvent,
};
use crossterm::execute;
use crossterm::terminal::{self, EnterAlternateScreen, LeaveAlternateScreen};
use nominal::core::{Drive, FileEntry, FileOperationDestination, FileRevision, NominalClient};
use ratatui::Terminal;
use ratatui::backend::CrosstermBackend;
use ratatui::layout::{Constraint, Layout, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Cell, Clear, Paragraph, Row, Table, TableState, Wrap};

const MAX_VISIBLE_COLUMNS: usize = 4;
const MIN_COLUMN_WIDTH: u16 = 26;
const INSPECTOR_WIDTH: u16 = 48;
const BROWSE_HINT: &str = "Use ←/→ to browse folders; selected-item details are on the right.";

pub async fn run(client: NominalClient, requested_drive: Option<String>) -> Result<()> {
    let drives = client
        .drives()
        .list(false)
        .await
        .context("Failed to list drives")?;
    if drives.is_empty() {
        anyhow::bail!("No active drives found");
    }
    let selected_drive = requested_drive
        .as_deref()
        .map(|id| {
            drives
                .iter()
                .position(|drive| drive.id() == id)
                .ok_or_else(|| anyhow::anyhow!("Drive '{id}' not found"))
        })
        .transpose()?;
    let mut app = App::new(drives, selected_drive);
    if let Some(index) = selected_drive {
        app.select_drive(index, &client).await;
    }

    let previous_hook = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        restore_terminal();
        previous_hook(info);
    }));
    terminal::enable_raw_mode().context("Failed to enable terminal raw mode")?;
    // Restores the terminal on every exit path from here on, including errors.
    let _guard = TerminalGuard;
    let mut stdout = io::stdout();
    execute!(stdout, EnterAlternateScreen, EnableMouseCapture)?;
    let mut terminal = Terminal::new(CrosstermBackend::new(stdout))?;
    event_loop(&mut terminal, &client, &mut app).await
}

struct TerminalGuard;

impl Drop for TerminalGuard {
    fn drop(&mut self) {
        restore_terminal();
    }
}

fn restore_terminal() {
    terminal::disable_raw_mode().ok();
    execute!(
        io::stdout(),
        DisableMouseCapture,
        LeaveAlternateScreen,
        crossterm::cursor::Show
    )
    .ok();
}

async fn event_loop(
    terminal: &mut Terminal<CrosstermBackend<io::Stdout>>,
    client: &NominalClient,
    app: &mut App,
) -> Result<()> {
    loop {
        app.apply_task_results(client).await;
        terminal.draw(|frame| app.draw(frame))?;
        if !event::poll(Duration::from_millis(50))? {
            continue;
        }
        match event::read()? {
            Event::Key(KeyEvent {
                code: KeyCode::Char('c'),
                modifiers: KeyModifiers::CONTROL,
                ..
            }) => return Ok(()),
            Event::Key(key) if app.modal.is_some() => app.handle_modal_key(key, client),
            Event::Key(key) if handle_key(key, app, client).await => return Ok(()),
            Event::Mouse(mouse) if app.modal.is_none() => app.handle_mouse(mouse, client).await,
            _ => {}
        }
    }
}

async fn handle_key(key: KeyEvent, app: &mut App, client: &NominalClient) -> bool {
    if !matches!(key.code, KeyCode::Char('q')) {
        app.quit_armed = false;
    }
    match key {
        KeyEvent {
            code: KeyCode::Char('q'),
            modifiers: KeyModifiers::NONE,
            ..
        } => return app.confirm_quit(),
        KeyEvent {
            code: KeyCode::Down | KeyCode::Char('j'),
            ..
        } => {
            app.next();
            app.refresh_selected_history(client);
        }
        KeyEvent {
            code: KeyCode::Up | KeyCode::Char('k'),
            ..
        } => {
            app.previous();
            app.refresh_selected_history(client);
        }
        KeyEvent {
            code: KeyCode::Right | KeyCode::Char('l'),
            ..
        } if app.drive.is_some() => app.open_selected_directory(client).await,
        KeyEvent {
            code: KeyCode::Left | KeyCode::Char('h'),
            ..
        } => {
            app.go_left();
            app.refresh_selected_history(client);
        }
        KeyEvent {
            code: KeyCode::Esc, ..
        } => {
            if app.drive.is_none() {
                return app.confirm_quit();
            }
            app.back();
            app.refresh_selected_history(client);
        }
        KeyEvent {
            code: KeyCode::Backspace,
            ..
        } => {
            app.go_left();
            app.refresh_selected_history(client);
        }
        KeyEvent {
            code: KeyCode::Enter,
            ..
        } => app.open_selected_directory(client).await,
        KeyEvent {
            code: KeyCode::Char('r'),
            ..
        } => app.refresh(client).await,
        KeyEvent {
            code: KeyCode::Char('u'),
            ..
        } => app.start_upload(),
        KeyEvent {
            code: KeyCode::Char('d'),
            ..
        } => app.start_download(),
        KeyEvent {
            code: KeyCode::Char('m'),
            ..
        } => app.move_or_start_move(),
        KeyEvent {
            code: KeyCode::Char('x'),
            ..
        } => app.start_remove(),
        _ => {}
    }
    false
}

struct App {
    drives: Vec<Drive>,
    drive_index: usize,
    drive_table: TableState,
    drive: Option<Drive>,
    columns: Vec<DirectoryColumn>,
    status: String,
    modal: Option<Modal>,
    pending_move: Option<MoveSource>,
    drive_area: Rect,
    column_areas: Vec<(usize, Rect)>,
    last_click: Option<(ClickTarget, Instant)>,
    history_path: Option<String>,
    history: Vec<FileRevision>,
    history_message: Option<String>,
    /// Label of the drive operation running in the background, if any.
    busy: Option<String>,
    quit_armed: bool,
    task_sender: mpsc::Sender<TaskResult>,
    task_results: mpsc::Receiver<TaskResult>,
}

struct DirectoryColumn {
    path: String,
    entries: Vec<FileEntry>,
    selected: usize,
    /// Kept across frames so the scroll offset is known when mapping mouse clicks to rows.
    table: TableState,
}

impl DirectoryColumn {
    fn new(path: String, entries: Vec<FileEntry>) -> Self {
        Self {
            path,
            entries,
            selected: 0,
            table: TableState::default(),
        }
    }
}

struct MoveSource {
    revision_rid: String,
    path: String,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum ClickTarget {
    Drive(usize),
    Entry { column: usize, row: usize },
}

enum Modal {
    Form(Form),
    Confirm {
        title: &'static str,
        message: String,
        action: ConfirmAction,
    },
}

enum ConfirmAction {
    Remove {
        revision_rid: String,
        path: String,
    },
    Overwrite {
        local_path: PathBuf,
        destination: String,
        revision_rid: String,
    },
    ReplaceLocal {
        source: String,
        destination: PathBuf,
    },
}

struct Form {
    kind: FormKind,
    fields: Vec<FormField>,
    active: usize,
}
enum FormKind {
    Upload {
        destination_folder: String,
    },
    Download {
        source: String,
    },
    Move {
        revision_rid: String,
        source: String,
    },
}
struct FormField {
    label: &'static str,
    value: String,
    hint: String,
}

/// Results sent back to the event loop by background tasks.
enum TaskResult {
    History {
        path: String,
        result: std::result::Result<Vec<FileRevision>, String>,
    },
    /// A drive operation finished; failures carry a ready-to-show message.
    Finished(std::result::Result<Outcome, String>),
}

impl App {
    fn new(drives: Vec<Drive>, selected_drive: Option<usize>) -> Self {
        let (task_sender, task_results) = mpsc::channel();
        Self {
            drives,
            drive_index: selected_drive.unwrap_or(0),
            drive_table: TableState::default(),
            drive: None,
            columns: vec![],
            status: "Select a drive with Enter".into(),
            modal: None,
            pending_move: None,
            drive_area: Rect::default(),
            column_areas: vec![],
            last_click: None,
            history_path: None,
            history: vec![],
            history_message: None,
            busy: None,
            quit_armed: false,
            task_sender,
            task_results,
        }
    }

    async fn select_drive(&mut self, index: usize, client: &NominalClient) {
        let Some(drive) = self.drives.get(index).cloned() else {
            self.status = "No drives to open. Press r to refresh.".into();
            return;
        };
        self.drive_index = index;
        self.drive = Some(drive);
        self.columns.clear();
        self.pending_move = None;
        self.clear_history();
        if self.load_directory(String::new(), client).await {
            self.status = BROWSE_HINT.into();
        }
    }

    /// Push a column for `path`. Returns whether the folder loaded.
    async fn load_directory(&mut self, path: String, client: &NominalClient) -> bool {
        let Some(drive) = &self.drive else {
            return false;
        };
        match client.files(drive.rid()).list(&path, false).await {
            Ok(entries) => {
                self.columns.push(DirectoryColumn::new(path, entries));
                self.refresh_selected_history(client);
                true
            }
            Err(error) => {
                self.status = format!("Could not load folder: {error}");
                false
            }
        }
    }

    async fn refresh(&mut self, client: &NominalClient) {
        if self.drive.is_none() {
            match client.drives().list(false).await {
                Ok(drives) => {
                    self.drives = drives;
                    self.drive_index = self.drive_index.min(self.drives.len().saturating_sub(1));
                    self.status = if self.drives.is_empty() {
                        "No active drives found".into()
                    } else {
                        "Drive list refreshed".into()
                    };
                }
                Err(error) => self.status = format!("Refresh failed: {error}"),
            }
            return;
        }
        if self.reload_columns(client).await {
            self.status = "Refreshed".into();
        }
    }

    /// Re-list every open column, keeping each selection on the same entry
    /// where it still exists. Columns whose folder no longer loads are closed.
    async fn reload_columns(&mut self, client: &NominalClient) -> bool {
        let Some(drive) = &self.drive else {
            return false;
        };
        let files = client.files(drive.rid());
        let mut loaded = true;
        for index in 0..self.columns.len() {
            let column = &mut self.columns[index];
            match files.list(&column.path, false).await {
                Ok(entries) => {
                    let previous = column
                        .entries
                        .get(column.selected)
                        .map(|entry| entry_path(entry).to_string());
                    column.selected = previous
                        .and_then(|path| entries.iter().position(|e| entry_path(e) == path))
                        .unwrap_or_else(|| column.selected.min(entries.len().saturating_sub(1)));
                    column.entries = entries;
                }
                Err(error) => {
                    self.status = format!("Could not reload folder: {error}");
                    self.columns.truncate(index.max(1));
                    loaded = false;
                    break;
                }
            }
        }
        // Revisions may have changed even if the selected path did not.
        self.history_path = None;
        self.refresh_selected_history(client);
        loaded
    }

    fn next(&mut self) {
        if self.drive.is_none() {
            if !self.drives.is_empty() {
                self.drive_index = (self.drive_index + 1) % self.drives.len();
            }
        } else if let Some(column) = self.columns.last_mut()
            && !column.entries.is_empty()
        {
            column.selected = (column.selected + 1) % column.entries.len();
        }
    }

    fn previous(&mut self) {
        if self.drive.is_none() {
            if !self.drives.is_empty() {
                self.drive_index = self
                    .drive_index
                    .checked_sub(1)
                    .unwrap_or(self.drives.len() - 1);
            }
        } else if let Some(column) = self.columns.last_mut()
            && !column.entries.is_empty()
        {
            column.selected = column
                .selected
                .checked_sub(1)
                .unwrap_or(column.entries.len() - 1);
        }
    }

    async fn open_selected_directory(&mut self, client: &NominalClient) {
        if self.drive.is_none() {
            self.select_drive(self.drive_index, client).await;
            return;
        }
        let Some(FileEntry::Directory(directory)) = self.selected_entry() else {
            self.status = "The selected file's details are shown in the right-hand panel.".into();
            return;
        };
        if self
            .load_directory(directory.path().to_string(), client)
            .await
        {
            self.status = BROWSE_HINT.into();
        }
    }

    fn go_left(&mut self) {
        if self.drive.is_some() && self.columns.len() > 1 {
            self.columns.pop();
        }
    }

    fn back(&mut self) {
        if self.pending_move.take().is_some() {
            self.status = "Move cancelled".into();
        } else if self.drive.is_some() {
            self.show_drives();
        }
    }

    fn show_drives(&mut self) {
        if let Some(active_drive) = &self.drive
            && let Some(index) = self
                .drives
                .iter()
                .position(|drive| drive.rid() == active_drive.rid())
        {
            self.drive_index = index;
        }
        self.drive = None;
        self.columns.clear();
        self.pending_move = None;
        self.clear_history();
        self.status = "Select a drive with Enter".into();
    }

    /// Quitting mid-operation abandons it, so the first q only warns.
    fn confirm_quit(&mut self) -> bool {
        match &self.busy {
            Some(label) if !self.quit_armed => {
                self.quit_armed = true;
                self.status = format!("{label} is still running. Press q again to quit anyway.");
                false
            }
            _ => true,
        }
    }

    /// Whether a new drive operation can start; explains why not in the status line.
    fn ensure_idle(&mut self) -> bool {
        if let Some(label) = &self.busy {
            self.status = format!("Wait for {label} to finish");
            return false;
        }
        true
    }

    fn start_upload(&mut self) {
        let Some(_) = self.drive else {
            self.status = "Select a drive and destination folder first".into();
            return;
        };
        if !self.ensure_idle() {
            return;
        }
        self.modal = Some(Modal::Form(Form {
            kind: FormKind::Upload {
                destination_folder: self.current_path().to_string(),
            },
            fields: vec![
                FormField {
                    label: "Local file",
                    value: String::new(),
                    hint: "Paste a path to a local file".into(),
                },
                FormField {
                    label: "Name in this folder",
                    value: String::new(),
                    hint: "Leave empty to keep the local filename".into(),
                },
            ],
            active: 0,
        }));
    }

    fn start_download(&mut self) {
        let Some(FileEntry::File(file)) = self.selected_entry() else {
            self.status = "Select a file to download".into();
            return;
        };
        let source = file.path().to_string();
        if !self.ensure_idle() {
            return;
        }
        let filename = Path::new(&source)
            .file_name()
            .map(|name| name.to_string_lossy().into_owned())
            .unwrap_or_else(|| "download".into());
        self.modal = Some(Modal::Form(Form {
            kind: FormKind::Download { source },
            fields: vec![FormField {
                label: "Save to",
                value: filename,
                hint: "Local path; parent directories will be created".into(),
            }],
            active: 0,
        }));
    }

    fn move_or_start_move(&mut self) {
        if self.pending_move.is_some() {
            if !self.ensure_idle() {
                return;
            }
            let source = self.pending_move.take().expect("pending move exists");
            self.modal = Some(Modal::Form(Form {
                kind: FormKind::Move {
                    revision_rid: source.revision_rid,
                    source: source.path.clone(),
                },
                fields: vec![FormField {
                    label: "Destination path",
                    value: join_path(self.current_path(), file_name(&source.path)),
                    hint: "Drive-relative destination path".into(),
                }],
                active: 0,
            }));
            return;
        }
        let Some(FileEntry::File(file)) = self.selected_entry() else {
            self.status = "Select a file to move".into();
            return;
        };
        let Some(revision_rid) = file.current_revision_rid() else {
            self.status = "This file cannot be moved".into();
            return;
        };
        self.pending_move = Some(MoveSource {
            revision_rid: revision_rid.into(),
            path: file.path().into(),
        });
        self.status =
            "Move mode: browse to the destination folder with ←/→, then press m to edit and confirm. Esc cancels."
                .into();
    }

    fn start_remove(&mut self) {
        let Some(FileEntry::File(file)) = self.selected_entry() else {
            self.status = "Select a file to remove".into();
            return;
        };
        let Some(revision_rid) = file.current_revision_rid() else {
            self.status = "This file cannot be removed".into();
            return;
        };
        let (revision_rid, path) = (revision_rid.to_string(), file.path().to_string());
        if !self.ensure_idle() {
            return;
        }
        self.modal = Some(Modal::Confirm {
            title: " REMOVE FILE ",
            message: format!(
                "Soft-delete '{path}'?\n\nPress y or Enter to remove it; Esc or n to cancel."
            ),
            action: ConfirmAction::Remove { revision_rid, path },
        });
    }

    fn handle_modal_key(&mut self, key: KeyEvent, client: &NominalClient) {
        let Some(mut modal) = self.modal.take() else {
            return;
        };
        match &mut modal {
            Modal::Confirm { .. } => match key.code {
                KeyCode::Esc | KeyCode::Char('n') => {}
                KeyCode::Enter | KeyCode::Char('y') => {
                    let Modal::Confirm { action, .. } = modal else {
                        unreachable!()
                    };
                    self.run_confirmed(action, client);
                }
                _ => self.modal = Some(modal),
            },
            Modal::Form(form) => match key.code {
                KeyCode::Esc => {}
                KeyCode::Tab | KeyCode::Down => {
                    form.active = (form.active + 1) % form.fields.len();
                    self.modal = Some(modal);
                }
                KeyCode::BackTab | KeyCode::Up => {
                    form.active = form.active.checked_sub(1).unwrap_or(form.fields.len() - 1);
                    self.modal = Some(modal);
                }
                KeyCode::Backspace => {
                    form.fields[form.active].value.pop();
                    self.modal = Some(modal);
                }
                KeyCode::Char(character)
                    if !key
                        .modifiers
                        .intersects(KeyModifiers::CONTROL | KeyModifiers::ALT) =>
                {
                    form.fields[form.active].value.push(character);
                    self.modal = Some(modal);
                }
                KeyCode::Enter => {
                    let Modal::Form(form) = modal else {
                        unreachable!()
                    };
                    self.submit_form(form, client);
                }
                _ => self.modal = Some(modal),
            },
        }
    }

    fn submit_form(&mut self, form: Form, client: &NominalClient) {
        let Some(drive) = &self.drive else { return };
        let drive_rid = drive.rid().to_string();
        let mut values = form.fields.into_iter().map(|field| field.value);
        match form.kind {
            FormKind::Upload { destination_folder } => {
                let local_value = values.next().unwrap_or_default();
                let name_value = values.next().unwrap_or_default();
                if local_value.is_empty() {
                    self.status = "Upload cancelled: local file is required".into();
                    return;
                }
                let local_path = PathBuf::from(local_value);
                let destination_name = if name_value.is_empty() {
                    local_path
                        .file_name()
                        .map(|name| name.to_string_lossy().into_owned())
                        .unwrap_or_default()
                } else {
                    name_value
                };
                if destination_name.is_empty() {
                    self.status = "Upload cancelled: a file name is required".into();
                    return;
                }
                let destination = join_path(&destination_folder, &destination_name);
                let client = client.clone();
                self.spawn_operation(format!("Upload to /{destination}"), async move {
                    let files = client.files(drive_rid);
                    // Overwriting needs an explicit confirmation, so look first.
                    if let Ok(existing) = files.get(&destination).await {
                        let revision_rid = existing
                            .current_revision_rid()
                            .context("the existing file cannot be overwritten")?;
                        return Ok(Outcome::ConfirmOverwrite {
                            local_path,
                            destination,
                            revision_rid: revision_rid.to_string(),
                        });
                    }
                    files
                        .put(
                            local_path,
                            destination.as_str(),
                            nominal::core::UploadOptions::new(),
                        )
                        .await?;
                    Ok(Outcome::Done(format!("Uploaded to /{destination}")))
                });
            }
            FormKind::Download { source } => {
                let value = values.next().unwrap_or_default();
                if value.is_empty() {
                    self.status = "Download cancelled: local destination is required".into();
                    return;
                }
                let destination = PathBuf::from(value);
                if destination.exists() {
                    self.modal = Some(Modal::Confirm {
                        title: " REPLACE LOCAL FILE ",
                        message: format!(
                            "'{}' already exists.\n\nPress y or Enter to replace it; Esc or n to cancel.",
                            destination.display()
                        ),
                        action: ConfirmAction::ReplaceLocal {
                            source,
                            destination,
                        },
                    });
                    return;
                }
                self.spawn_download(source, destination, client);
            }
            FormKind::Move {
                revision_rid,
                source,
            } => {
                let destination = values.next().unwrap_or_default().trim().to_string();
                if destination.is_empty() {
                    self.status = "Move cancelled: destination path is required".into();
                    return;
                }
                let client = client.clone();
                self.spawn_operation(format!("Move of '{source}'"), async move {
                    client
                        .files(drive_rid)
                        .move_file(&revision_rid, destination.as_str())
                        .await?;
                    Ok(Outcome::Done(format!("Moved '{source}' to /{destination}")))
                });
            }
        }
    }

    fn run_confirmed(&mut self, action: ConfirmAction, client: &NominalClient) {
        let Some(drive) = &self.drive else { return };
        let drive_rid = drive.rid().to_string();
        let client_for_task = client.clone();
        match action {
            ConfirmAction::Remove { revision_rid, path } => {
                self.spawn_operation(format!("Remove of '{path}'"), async move {
                    client_for_task
                        .files(drive_rid)
                        .remove(&revision_rid)
                        .await?;
                    Ok(Outcome::Done(format!("Removed '{path}'")))
                });
            }
            ConfirmAction::Overwrite {
                local_path,
                destination,
                revision_rid,
            } => {
                self.spawn_operation(format!("Upload to /{destination}"), async move {
                    client_for_task
                        .files(drive_rid)
                        .put(
                            local_path,
                            FileOperationDestination::file_revision_rid(revision_rid),
                            nominal::core::UploadOptions::new(),
                        )
                        .await?;
                    Ok(Outcome::Done(format!(
                        "Uploaded a new revision of /{destination}"
                    )))
                });
            }
            ConfirmAction::ReplaceLocal {
                source,
                destination,
            } => self.spawn_download(source, destination, client),
        }
    }

    fn spawn_download(&mut self, source: String, destination: PathBuf, client: &NominalClient) {
        let Some(drive) = &self.drive else { return };
        let drive_rid = drive.rid().to_string();
        let client = client.clone();
        self.spawn_operation(format!("Download of /{source}"), async move {
            if let Some(parent) = destination
                .parent()
                .filter(|path| !path.as_os_str().is_empty())
            {
                tokio::fs::create_dir_all(parent)
                    .await
                    .context("could not create local directory")?;
            }
            download_to(&client, &drive_rid, &source, &destination).await?;
            Ok(Outcome::Done(format!(
                "Downloaded to {}",
                destination.display()
            )))
        });
    }

    /// Run a drive operation in the background so the UI stays responsive.
    /// Its outcome is applied by [`App::apply_task_results`].
    fn spawn_operation(
        &mut self,
        label: String,
        operation: impl Future<Output = Result<Outcome>> + Send + 'static,
    ) {
        self.status = format!("{label}…");
        self.busy = Some(label.clone());
        let sender = self.task_sender.clone();
        tokio::spawn(async move {
            let result = operation
                .await
                .map_err(|error| format!("{label} failed: {error:#}"));
            sender.send(TaskResult::Finished(result)).ok();
        });
    }

    async fn apply_task_results(&mut self, client: &NominalClient) {
        while let Ok(result) = self.task_results.try_recv() {
            match result {
                TaskResult::History { path, result } => {
                    if self.history_path.as_deref() != Some(path.as_str()) {
                        continue;
                    }
                    match result {
                        Ok(mut revisions) => {
                            revisions
                                .sort_by_key(|revision| std::cmp::Reverse(revision.created_at()));
                            self.history = revisions;
                            self.history_message = None;
                        }
                        Err(error) => {
                            self.history_message = Some(format!("Could not load history: {error}"))
                        }
                    }
                }
                TaskResult::Finished(result) => {
                    self.busy = None;
                    self.quit_armed = false;
                    match result {
                        Ok(Outcome::Done(message)) => {
                            self.status = message;
                            self.reload_columns(client).await;
                        }
                        Ok(Outcome::ConfirmOverwrite {
                            local_path,
                            destination,
                            revision_rid,
                        }) => {
                            self.status = "Upload paused: confirm overwrite".into();
                            self.modal = Some(Modal::Confirm {
                                title: " OVERWRITE FILE ",
                                message: format!(
                                    "'/{destination}' already exists.\n\nPress y or Enter to upload a new revision of it; Esc or n to cancel."
                                ),
                                action: ConfirmAction::Overwrite {
                                    local_path,
                                    destination,
                                    revision_rid,
                                },
                            });
                        }
                        Err(message) => self.status = message,
                    }
                }
            }
        }
    }

    async fn handle_mouse(&mut self, mouse: MouseEvent, client: &NominalClient) {
        if mouse.kind != event::MouseEventKind::Down(MouseButton::Left) {
            return;
        }
        if self.drive.is_none() && contains(self.drive_area, mouse.column, mouse.row) {
            // Skip the border and header rows, then account for scrolling.
            let row = usize::from(mouse.row.saturating_sub(self.drive_area.y + 2))
                + self.drive_table.offset();
            if row < self.drives.len() {
                self.drive_index = row;
                if self.register_click(ClickTarget::Drive(row)) {
                    self.select_drive(row, client).await;
                }
            }
            return;
        }
        for (column_index, area) in self.column_areas.clone() {
            if !contains(area, mouse.column, mouse.row) {
                continue;
            }
            let Some(column) = self.columns.get_mut(column_index) else {
                break;
            };
            let row = usize::from(mouse.row.saturating_sub(area.y + 1)) + column.table.offset();
            if row >= column.entries.len() {
                break;
            }
            column.selected = row;
            let is_folder = matches!(column.entries[row], FileEntry::Directory(_));
            self.columns.truncate(column_index + 1);
            self.status = "Selected. Details are shown in the right-hand panel.".into();
            self.refresh_selected_history(client);
            if self.register_click(ClickTarget::Entry {
                column: column_index,
                row,
            }) && is_folder
            {
                self.open_selected_directory(client).await;
            }
            break;
        }
    }

    fn register_click(&mut self, target: ClickTarget) -> bool {
        let now = Instant::now();
        let repeated = self.last_click.is_some_and(|(previous, at)| {
            previous == target && now.duration_since(at) < Duration::from_millis(500)
        });
        self.last_click = Some((target, now));
        repeated
    }

    fn selected_entry(&self) -> Option<&FileEntry> {
        let column = self.columns.last()?;
        column.entries.get(column.selected)
    }
    fn current_path(&self) -> &str {
        self.columns.last().map_or("", |column| &column.path)
    }

    fn clear_history(&mut self) {
        self.history_path = None;
        self.history.clear();
        self.history_message = None;
    }

    /// Load the selected file's revisions in the background. Results for a
    /// file that is no longer selected are discarded when they arrive.
    fn refresh_selected_history(&mut self, client: &NominalClient) {
        let (path, file_rid) = match self.selected_entry() {
            Some(FileEntry::File(file)) => (
                file.path().to_string(),
                file.managed_file_rid().map(str::to_string),
            ),
            _ => {
                self.clear_history();
                return;
            }
        };
        if self.history_path.as_deref() == Some(path.as_str()) {
            return;
        }
        self.history_path = Some(path.clone());
        self.history.clear();
        let Some(file_rid) = file_rid else {
            self.history_message = Some("History is unavailable for virtual files.".into());
            return;
        };
        let Some(drive) = &self.drive else { return };
        self.history_message = Some("Loading history…".into());
        let files = client.files(drive.rid());
        let sender = self.task_sender.clone();
        tokio::spawn(async move {
            let result = files
                .list_revisions(&file_rid)
                .await
                .map_err(|error| error.to_string());
            sender.send(TaskResult::History { path, result }).ok();
        });
    }

    fn draw(&mut self, frame: &mut ratatui::Frame) {
        let layout =
            Layout::vertical([Constraint::Min(5), Constraint::Length(3)]).split(frame.area());
        if self.drive.is_some() {
            self.draw_browser(frame, layout[0]);
        } else {
            self.draw_drives(frame, layout[0]);
        }
        self.draw_footer(frame, layout[1]);
        if let Some(modal) = &self.modal {
            self.draw_modal(frame, modal);
        }
    }

    fn draw_drives(&mut self, frame: &mut ratatui::Frame, area: Rect) {
        let panels = Layout::horizontal([
            Constraint::Min(MIN_COLUMN_WIDTH),
            Constraint::Length(INSPECTOR_WIDTH),
        ])
        .split(area);
        self.drive_area = panels[0];
        self.column_areas.clear();
        let rows = self.drives.iter().map(|drive| {
            Row::new(vec![
                Cell::from(drive.id().to_string()),
                Cell::from(drive.kind().to_string()),
                Cell::from(drive.source().to_string()),
                Cell::from(drive.state().to_string()),
            ])
        });
        let header = Row::new(["DRIVE", "TYPE", "SOURCE", "STATE"]).style(eyebrow_style());
        self.drive_table
            .select((!self.drives.is_empty()).then_some(self.drive_index));
        frame.render_stateful_widget(
            Table::new(
                rows,
                [
                    Constraint::Percentage(42),
                    Constraint::Percentage(18),
                    Constraint::Percentage(24),
                    Constraint::Percentage(16),
                ],
            )
            .header(header)
            .block(Block::default().title(" DRIVES ").borders(Borders::ALL))
            .row_highlight_style(selected_row_style()),
            panels[0],
            &mut self.drive_table,
        );
        self.draw_drive_inspector(frame, panels[1]);
    }

    fn draw_browser(&mut self, frame: &mut ratatui::Frame, area: Rect) {
        self.drive_area = Rect::default();
        let panels = Layout::horizontal([
            Constraint::Min(MIN_COLUMN_WIDTH),
            Constraint::Length(INSPECTOR_WIDTH),
        ])
        .split(area);
        let drive = self.drive.as_ref().expect("drive exists");
        let browser = Block::default()
            .title(format!(" FILES  {}:/{} ", drive.id(), self.current_path()))
            .borders(Borders::ALL);
        let browser_area = browser.inner(panels[0]);
        frame.render_widget(browser, panels[0]);
        let column_count = self.columns.len();
        let start = column_count.saturating_sub(column_capacity(browser_area.width));
        let visible = column_count - start;
        let constraints = vec![Constraint::Ratio(1, visible.max(1) as u32); visible];
        let areas = Layout::horizontal(constraints).split(browser_area);
        self.column_areas = areas
            .iter()
            .enumerate()
            .map(|(offset, area)| (start + offset, *area))
            .collect();
        for (offset, (column, column_area)) in self.columns[start..]
            .iter_mut()
            .zip(areas.iter())
            .enumerate()
        {
            let rows = column.entries.iter().map(file_row);
            column.table.select(Some(column.selected));
            let title = if column.path.is_empty() {
                " / ".into()
            } else {
                format!(" {}/ ", file_name(&column.path))
            };
            frame.render_stateful_widget(
                Table::new(rows, [Constraint::Min(8), Constraint::Length(10)])
                    .block(Block::default().title(title).borders(Borders::ALL))
                    .row_highlight_style(if offset + start + 1 == column_count {
                        selected_row_style()
                    } else {
                        inactive_selection_style()
                    }),
                *column_area,
                &mut column.table,
            );
        }
        self.draw_entry_inspector(frame, panels[1]);
    }

    fn draw_drive_inspector(&self, frame: &mut ratatui::Frame, area: Rect) {
        let lines = match self.drives.get(self.drive_index) {
            Some(drive) => {
                let mut lines = vec![
                    detail_line("Drive", drive.id()),
                    detail_line("Type", &drive.kind().to_string()),
                    detail_line("Source", &drive.source().to_string()),
                    detail_line("State", &drive.state().to_string()),
                ];
                if let Some(created_at) = drive.created_at() {
                    lines.push(detail_line("Created", &format_utc(created_at)));
                }
                lines.push(detail_line("RID", &short_rid(drive.rid())));
                lines
            }
            None => vec![Line::from(Span::styled("No drives.", muted_style()))],
        };
        frame.render_widget(
            Paragraph::new(lines)
                .block(Block::default().title(" DETAILS ").borders(Borders::ALL))
                .wrap(Wrap { trim: true }),
            area,
        );
    }

    fn draw_entry_inspector(&self, frame: &mut ratatui::Frame, area: Rect) {
        let lines = match self.selected_entry() {
            Some(FileEntry::Directory(directory)) => vec![
                detail_line("Name", file_name(directory.path())),
                detail_line("Path", &format!("/{}", directory.path())),
            ],
            Some(FileEntry::File(file)) => {
                let mut lines = vec![
                    detail_line("Name", file_name(file.path())),
                    detail_line("Path", &format!("/{}", file.path())),
                    detail_line("Size", &format_size(file.size_bytes())),
                    detail_line("State", &file.state().to_string()),
                ];
                if let Some(created_at) = file.created_at() {
                    lines.push(detail_line("Created", &format_utc(created_at)));
                }
                lines.push(Line::from(""));
                lines.push(Line::from(Span::styled("History", eyebrow_style())));
                if let Some(message) = &self.history_message {
                    lines.push(Line::from(Span::styled(
                        message.clone(),
                        status_style(message),
                    )));
                } else if self.history.is_empty() {
                    lines.push(Line::from("No revisions."));
                } else {
                    for revision in self.history.iter().take(6) {
                        let created = revision
                            .created_at()
                            .map(format_utc)
                            .unwrap_or_else(|| "Unknown time".into());
                        lines.push(Line::from(format!(
                            "{created}  {}  {}",
                            format_size(revision.size_bytes()),
                            short_rid(revision.file_revision_rid())
                        )));
                    }
                }
                lines
            }
            None => vec![detail_line("Path", &format!("/{}", self.current_path()))],
        };
        frame.render_widget(
            Paragraph::new(lines)
                .block(Block::default().title(" DETAILS ").borders(Borders::ALL))
                .wrap(Wrap { trim: true }),
            area,
        );
    }

    fn draw_footer(&self, frame: &mut ratatui::Frame, area: Rect) {
        let help = if self.drive.is_some() {
            "←/→ or Enter: open folder  Esc: drives  u upload  d download  m move  x remove  r refresh  q quit"
        } else {
            "↑/↓ or click: select  Enter: files  r refresh  Esc/q: quit"
        };
        let move_status = self.pending_move.as_ref().map(|source| {
            format!(
                "Moving '{}' — browse to a destination, then press m to edit and confirm.",
                source.path
            )
        });
        let status = move_status.as_deref().unwrap_or(&self.status);
        let status_line = match &self.busy {
            Some(label) if move_status.is_none() && !self.quit_armed => Line::from(vec![
                Span::styled("Working: ", eyebrow_style()),
                Span::styled(format!("{label}…"), Style::default().fg(Color::Cyan)),
            ]),
            _ => Line::from(Span::styled(status, status_style(status))),
        };
        frame.render_widget(
            Paragraph::new(vec![Line::from(Span::raw(help)), status_line])
                .wrap(Wrap { trim: true }),
            area,
        );
    }

    fn draw_modal(&self, frame: &mut ratatui::Frame, modal: &Modal) {
        let area = centered_rect(70, 50, frame.area());
        frame.render_widget(Clear, area);
        match modal {
            Modal::Confirm { title, message, .. } => frame.render_widget(
                Paragraph::new(message.as_str())
                    .block(
                        Block::default()
                            .title(Line::styled(
                                *title,
                                Style::default().fg(Color::Red).add_modifier(Modifier::BOLD),
                            ))
                            .borders(Borders::ALL),
                    )
                    .wrap(Wrap { trim: true }),
                area,
            ),
            Modal::Form(form) => {
                let title = match &form.kind {
                    FormKind::Upload { destination_folder } => format!(
                        " UPLOAD TO {}:/{} ",
                        self.drive.as_ref().expect("drive exists").id(),
                        destination_folder
                    ),
                    FormKind::Download { source } => format!(" DOWNLOAD /{source} "),
                    FormKind::Move { source, .. } => format!(" MOVE /{source} "),
                };
                let mut lines = Vec::new();
                for (index, field) in form.fields.iter().enumerate() {
                    lines.push(Line::from(Span::styled(field.label, eyebrow_style())));
                    let value = if field.value.is_empty() {
                        field.hint.as_str()
                    } else {
                        field.value.as_str()
                    };
                    let value_style = if index == form.active {
                        Style::default().add_modifier(Modifier::REVERSED)
                    } else if field.value.is_empty() {
                        muted_style()
                    } else {
                        Style::default()
                    };
                    lines.push(Line::from(vec![
                        Span::raw("  [ "),
                        Span::styled(value, value_style),
                        Span::raw(" ]"),
                    ]));
                    if index + 1 < form.fields.len() {
                        lines.push(Line::from(""));
                    }
                }
                lines.push(Line::from(""));
                let instructions = if form.fields.len() > 1 {
                    "Tab: next field   Enter: confirm   Esc: cancel"
                } else {
                    "Enter: confirm   Esc: cancel"
                };
                lines.push(Line::from(Span::styled(instructions, muted_style())));
                frame.render_widget(
                    Paragraph::new(lines)
                        .block(Block::default().title(title).borders(Borders::ALL))
                        .wrap(Wrap { trim: true }),
                    area,
                );
            }
        }
    }
}

/// What a background drive operation produced.
enum Outcome {
    Done(String),
    /// The upload destination exists; the user must confirm before overwriting.
    ConfirmOverwrite {
        local_path: PathBuf,
        destination: String,
        revision_rid: String,
    },
}

fn detail_line(label: &str, value: &str) -> Line<'static> {
    Line::from(vec![
        Span::styled(format!("{}  ", label.to_uppercase()), eyebrow_style()),
        Span::raw(value.to_string()),
    ])
}

/// The TUI inherits the user's terminal colors, so these styles intentionally
/// use terminal palette roles rather than Nominal's fixed web color values.
/// Reversed selection adapts to both light and dark terminal themes.
fn muted_style() -> Style {
    Style::default().fg(Color::DarkGray)
}

fn eyebrow_style() -> Style {
    muted_style().add_modifier(Modifier::BOLD)
}

fn selected_row_style() -> Style {
    Style::default().add_modifier(Modifier::BOLD | Modifier::REVERSED)
}

fn inactive_selection_style() -> Style {
    muted_style().add_modifier(Modifier::REVERSED)
}

fn status_style(message: &str) -> Style {
    let message = message.to_ascii_lowercase();
    if message.contains("failed") || message.contains("could not") || message.contains("cannot") {
        Style::default().fg(Color::Red)
    } else if message.contains("cancelled") || message.contains("still running") {
        Style::default().fg(Color::Yellow)
    } else if message.starts_with("removed")
        || message.starts_with("uploaded")
        || message.starts_with("downloaded")
        || message.starts_with("moved")
    {
        Style::default().fg(Color::Green)
    } else {
        muted_style()
    }
}

fn format_utc(timestamp: DateTime<Utc>) -> String {
    timestamp.format("%Y-%m-%d %H:%M UTC").to_string()
}

fn column_capacity(available_width: u16) -> usize {
    usize::from(available_width / MIN_COLUMN_WIDTH).clamp(1, MAX_VISIBLE_COLUMNS)
}

fn short_rid(rid: &str) -> String {
    const MAX_LENGTH: usize = 12;
    if rid.len() <= MAX_LENGTH {
        rid.into()
    } else {
        format!("…{}", &rid[rid.len() - MAX_LENGTH..])
    }
}

fn entry_path(entry: &FileEntry) -> &str {
    match entry {
        FileEntry::Directory(directory) => directory.path(),
        FileEntry::File(file) => file.path(),
    }
}

fn file_row(entry: &FileEntry) -> Row<'static> {
    match entry {
        FileEntry::Directory(directory) => Row::new(vec![
            Cell::from(file_name(directory.path()).to_string()),
            Cell::from(format!("{:>10}", "›")),
        ]),
        FileEntry::File(file) => Row::new(vec![
            Cell::from(file_name(file.path()).to_string()),
            Cell::from(format!("{:>10}", format_size(file.size_bytes()))),
        ]),
    }
}
fn file_name(path: &str) -> &str {
    path.rsplit('/').next().unwrap_or(path)
}
fn format_size(bytes: u64) -> String {
    const UNITS: [&str; 5] = ["B", "KB", "MB", "GB", "TB"];
    let mut value = bytes as f64;
    let mut unit = 0;
    while value >= 1024.0 && unit < UNITS.len() - 1 {
        value /= 1024.0;
        unit += 1;
    }
    if unit == 0 {
        format!("{bytes} B")
    } else {
        format!("{value:.1} {}", UNITS[unit])
    }
}
async fn download_to(
    client: &NominalClient,
    drive_rid: &str,
    source: &str,
    destination: &Path,
) -> Result<()> {
    let mut reader = client.files(drive_rid).download(source).await?;
    let mut file = tokio::fs::File::create(destination).await?;
    tokio::io::copy(&mut reader, &mut file).await?;
    Ok(())
}
fn join_path(parent: &str, child: &str) -> String {
    if parent.is_empty() {
        child.into()
    } else {
        format!("{parent}/{child}")
    }
}
fn contains(area: Rect, x: u16, y: u16) -> bool {
    x >= area.x && x < area.right() && y >= area.y && y < area.bottom()
}
fn centered_rect(percent_x: u16, percent_y: u16, area: Rect) -> Rect {
    let vertical = Layout::vertical([
        Constraint::Percentage((100 - percent_y) / 2),
        Constraint::Percentage(percent_y),
        Constraint::Percentage((100 - percent_y) / 2),
    ])
    .split(area);
    Layout::horizontal([
        Constraint::Percentage((100 - percent_x) / 2),
        Constraint::Percentage(percent_x),
        Constraint::Percentage((100 - percent_x) / 2),
    ])
    .split(vertical[1])[1]
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn formats_file_sizes_for_a_table_column() {
        assert_eq!(format_size(999), "999 B");
        assert_eq!(format_size(1024), "1.0 KB");
        assert_eq!(format_size(1_572_864), "1.5 MB");
    }
    #[test]
    fn joins_a_file_name_to_the_current_folder() {
        assert_eq!(join_path("", "flight.csv"), "flight.csv");
        assert_eq!(
            join_path("telemetry/raw", "flight.csv"),
            "telemetry/raw/flight.csv"
        );
    }

    #[test]
    fn formats_timestamps_in_utc_without_fractional_seconds() {
        let timestamp = DateTime::parse_from_rfc3339("2026-09-01T12:34:56.789Z")
            .unwrap()
            .with_timezone(&Utc);
        assert_eq!(format_utc(timestamp), "2026-09-01 12:34 UTC");
    }

    #[test]
    fn abbreviated_rids_show_that_the_prefix_is_hidden() {
        assert_eq!(short_rid("short-rid"), "short-rid");
        assert_eq!(
            short_rid("ri.file-revision.abcdefghijklmnop"),
            "…efghijklmnop"
        );
    }

    #[test]
    fn adds_columns_as_terminal_width_allows() {
        assert_eq!(column_capacity(25), 1);
        assert_eq!(column_capacity(52), 2);
        assert_eq!(column_capacity(78), 3);
        assert_eq!(column_capacity(200), 4);
    }
}
