use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::time::Duration;

use anyhow::{Context, Result};
use crossterm::event::{self, Event, KeyCode, KeyEvent, KeyModifiers};
use crossterm::execute;
use crossterm::terminal::{self, EnterAlternateScreen, LeaveAlternateScreen};
use nominal::core::{Drive, FileEntry, FileState, NominalClient};
use ratatui::Terminal;
use ratatui::backend::CrosstermBackend;
use ratatui::layout::{Constraint, Layout};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span, Text};
use ratatui::widgets::{Block, Borders, List, ListItem, ListState, Paragraph};

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

    terminal::enable_raw_mode().context("Failed to enable terminal raw mode")?;
    let mut stdout = io::stdout();
    execute!(stdout, EnterAlternateScreen)?;
    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend)?;
    let result = event_loop(&mut terminal, &client, &mut app).await;
    terminal::disable_raw_mode().ok();
    execute!(terminal.backend_mut(), LeaveAlternateScreen).ok();
    terminal.show_cursor().ok();
    result
}

async fn event_loop(
    terminal: &mut Terminal<CrosstermBackend<io::Stdout>>,
    client: &NominalClient,
    app: &mut App,
) -> Result<()> {
    loop {
        terminal.draw(|frame| app.draw(frame))?;
        if !event::poll(Duration::from_millis(250))? {
            continue;
        }
        let Event::Key(key) = event::read()? else {
            continue;
        };
        if key.code == KeyCode::Char('q') || key.code == KeyCode::Esc {
            return Ok(());
        }
        match key {
            KeyEvent {
                code: KeyCode::Down | KeyCode::Char('j'),
                ..
            } => app.next(),
            KeyEvent {
                code: KeyCode::Up | KeyCode::Char('k'),
                ..
            } => app.previous(),
            KeyEvent {
                code: KeyCode::Enter,
                ..
            } => app.activate(client).await,
            KeyEvent {
                code: KeyCode::Backspace,
                ..
            } => app.parent(client).await,
            KeyEvent {
                code: KeyCode::Char('r'),
                ..
            } => app.refresh(client).await,
            KeyEvent {
                code: KeyCode::Char('u'),
                ..
            } => app.upload(client, terminal).await,
            KeyEvent {
                code: KeyCode::Char('d'),
                ..
            } => app.download(client, terminal).await,
            KeyEvent {
                code: KeyCode::Char('m'),
                ..
            } => app.move_file(client, terminal).await,
            KeyEvent {
                code: KeyCode::Char('x'),
                ..
            } => app.remove(client, terminal).await,
            KeyEvent {
                code: KeyCode::Char('c'),
                modifiers: KeyModifiers::CONTROL,
                ..
            } => return Ok(()),
            _ => {}
        }
    }
}

struct App {
    drives: Vec<Drive>,
    drive_index: usize,
    drive: Option<Drive>,
    path: String,
    entries: Vec<FileEntry>,
    selected: usize,
    status: String,
}

impl App {
    fn new(drives: Vec<Drive>, selected_drive: Option<usize>) -> Self {
        Self {
            drives,
            drive_index: selected_drive.unwrap_or(0),
            drive: None,
            path: String::new(),
            entries: vec![],
            selected: 0,
            status: "Select a drive and press Enter".into(),
        }
    }

    async fn select_drive(&mut self, index: usize, client: &NominalClient) {
        self.drive_index = index;
        self.drive = self.drives.get(index).cloned();
        self.path.clear();
        self.refresh(client).await;
    }

    async fn refresh(&mut self, client: &NominalClient) {
        let Some(drive) = &self.drive else { return };
        match client.files(drive.rid()).list(&self.path, false).await {
            Ok(entries) => {
                self.entries = entries;
                self.selected = 0;
                self.status = format!("{} entries", self.entries.len());
            }
            Err(error) => self.status = format!("Refresh failed: {error}"),
        }
    }

    async fn activate(&mut self, client: &NominalClient) {
        if self.drive.is_none() {
            self.select_drive(self.drive_index, client).await;
            return;
        }
        let Some(FileEntry::Directory(directory)) = self.entries.get(self.selected) else {
            return;
        };
        self.path = directory.path().to_string();
        self.refresh(client).await;
    }

    async fn parent(&mut self, client: &NominalClient) {
        if self.drive.is_none() {
            return;
        }
        if let Some((parent, _)) = self.path.rsplit_once('/') {
            self.path = parent.to_string();
        } else {
            self.path.clear();
        }
        self.refresh(client).await;
    }

    fn next(&mut self) {
        if self.drive.is_none() {
            self.drive_index = (self.drive_index + 1) % self.drives.len();
        } else if !self.entries.is_empty() {
            self.selected = (self.selected + 1) % self.entries.len();
        }
    }
    fn previous(&mut self) {
        if self.drive.is_none() {
            self.drive_index = self
                .drive_index
                .checked_sub(1)
                .unwrap_or(self.drives.len() - 1);
        } else if !self.entries.is_empty() {
            self.selected = self
                .selected
                .checked_sub(1)
                .unwrap_or(self.entries.len() - 1);
        }
    }

    async fn upload(
        &mut self,
        client: &NominalClient,
        terminal: &mut Terminal<CrosstermBackend<io::Stdout>>,
    ) {
        let Some(drive) = &self.drive else {
            self.status = "Select a drive first".into();
            return;
        };
        let Some(local) = prompt(terminal, "Upload local file: ") else {
            return;
        };
        let local = PathBuf::from(local);
        let default = local
            .file_name()
            .map(|name| name.to_string_lossy())
            .unwrap_or_default();
        let destination = prompt(
            terminal,
            &format!("Destination path [/{}]: ", join_path(&self.path, &default)),
        )
        .filter(|destination| !destination.is_empty())
        .unwrap_or_else(|| join_path(&self.path, &default));
        match client
            .files(drive.rid())
            .put(local, &destination, nominal::core::UploadOptions::new())
            .await
        {
            Ok(_) => {
                self.status = format!("Uploaded {destination}");
                self.refresh(client).await;
            }
            Err(error) => self.status = format!("Upload failed: {error}"),
        }
    }

    async fn download(
        &mut self,
        client: &NominalClient,
        terminal: &mut Terminal<CrosstermBackend<io::Stdout>>,
    ) {
        let Some(drive) = &self.drive else {
            self.status = "Select a drive first".into();
            return;
        };
        let Some(FileEntry::File(file)) = self.entries.get(self.selected) else {
            self.status = "Select a file".into();
            return;
        };
        let source = file.path().to_string();
        let default = Path::new(&source)
            .file_name()
            .map(|name| name.to_string_lossy().into_owned())
            .unwrap_or_else(|| "download".into());
        let Some(destination) = prompt(terminal, &format!("Download to [{}]: ", default)) else {
            return;
        };
        let destination = if destination.is_empty() {
            PathBuf::from(default)
        } else {
            PathBuf::from(destination)
        };
        if let Some(parent) = destination
            .parent()
            .filter(|path| !path.as_os_str().is_empty())
        {
            tokio::fs::create_dir_all(parent).await.ok();
        }
        match download_to(client, drive.rid(), &source, &destination).await {
            Ok(()) => self.status = format!("Downloaded to {}", destination.display()),
            Err(error) => self.status = format!("Download failed: {error}"),
        }
    }

    async fn move_file(
        &mut self,
        client: &NominalClient,
        terminal: &mut Terminal<CrosstermBackend<io::Stdout>>,
    ) {
        let Some(drive) = &self.drive else { return };
        let Some(FileEntry::File(file)) = self.entries.get(self.selected) else {
            self.status = "Select a file".into();
            return;
        };
        let Some(revision) = file.current_revision_rid() else {
            self.status = "This file cannot be moved".into();
            return;
        };
        let Some(destination) = prompt(terminal, "Move to drive-relative path: ") else {
            return;
        };
        if destination.is_empty() {
            return;
        }
        match client
            .files(drive.rid())
            .move_file(revision, destination)
            .await
        {
            Ok(_) => {
                self.status = "File moved".into();
                self.refresh(client).await;
            }
            Err(error) => self.status = format!("Move failed: {error}"),
        }
    }

    async fn remove(
        &mut self,
        client: &NominalClient,
        terminal: &mut Terminal<CrosstermBackend<io::Stdout>>,
    ) {
        let Some(drive) = &self.drive else { return };
        let Some(FileEntry::File(file)) = self.entries.get(self.selected) else {
            self.status = "Select a file".into();
            return;
        };
        let Some(revision) = file.current_revision_rid() else {
            self.status = "This file cannot be removed".into();
            return;
        };
        let Some(answer) = prompt(terminal, &format!("Remove '{}'? [y/N] ", file.path())) else {
            return;
        };
        if answer.to_lowercase() != "y" {
            return;
        }
        match client.files(drive.rid()).remove(revision).await {
            Ok(_) => {
                self.status = "File removed".into();
                self.refresh(client).await;
            }
            Err(error) => self.status = format!("Remove failed: {error}"),
        }
    }

    fn draw(&self, frame: &mut ratatui::Frame) {
        let chunks =
            Layout::vertical([Constraint::Min(1), Constraint::Length(2)]).split(frame.area());
        let title = match &self.drive {
            Some(drive) => format!(" Nominal Drive: {}  /{} ", drive.id(), self.path),
            None => " Nominal Drives ".into(),
        };
        let items: Vec<ListItem> = if self.drive.is_some() {
            self.entries.iter().map(entry_item).collect()
        } else {
            self.drives
                .iter()
                .map(|drive| {
                    ListItem::new(format!("{}  ({})", drive.id(), drive.content_mutability()))
                })
                .collect()
        };
        let mut state = ListState::default();
        state.select(Some(if self.drive.is_some() {
            self.selected
        } else {
            self.drive_index
        }));
        frame.render_stateful_widget(
            List::new(items)
                .block(Block::default().title(title).borders(Borders::ALL))
                .highlight_style(
                    Style::default()
                        .fg(Color::Cyan)
                        .add_modifier(Modifier::BOLD),
                ),
            chunks[0],
            &mut state,
        );
        let help = "↑/↓ navigate  Enter open  Backspace parent  r refresh  u upload  d download  m move  x remove  q quit";
        frame.render_widget(
            Paragraph::new(Text::from(vec![
                Line::from(Span::raw(help)),
                Line::from(Span::styled(
                    &self.status,
                    Style::default().fg(Color::Yellow),
                )),
            ])),
            chunks[1],
        );
    }
}

fn entry_item(entry: &FileEntry) -> ListItem<'static> {
    match entry {
        FileEntry::Directory(dir) => ListItem::new(format!("📁 {}/", dir.path())),
        FileEntry::File(file) => ListItem::new(format!(
            "{}  {} bytes{}",
            file.path(),
            file.size_bytes(),
            if file.state() == FileState::Active {
                ""
            } else {
                "  [removed]"
            }
        )),
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

fn prompt(terminal: &mut Terminal<CrosstermBackend<io::Stdout>>, message: &str) -> Option<String> {
    terminal::disable_raw_mode().ok()?;
    execute!(terminal.backend_mut(), LeaveAlternateScreen).ok()?;
    print!("{message}");
    io::stdout().flush().ok()?;
    let mut input = String::new();
    io::stdin().read_line(&mut input).ok()?;
    execute!(terminal.backend_mut(), EnterAlternateScreen).ok()?;
    terminal::enable_raw_mode().ok()?;
    terminal.clear().ok();
    Some(input.trim().to_string())
}
