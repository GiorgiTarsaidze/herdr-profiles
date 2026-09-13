use std::io::{self, Stdout, Write};

use anyhow::Result;
use crossterm::cursor::{Hide, MoveTo, Show};
use crossterm::event::{self, Event, KeyCode, KeyEventKind, KeyModifiers};
use crossterm::style::{Attribute, Color, Colors, Print, ResetColor, SetAttribute, SetColors};
use crossterm::terminal::{self, Clear, ClearType, EnterAlternateScreen, LeaveAlternateScreen};
use crossterm::{execute, queue};

use crate::herdr::Herdr;
use crate::keybinding::{self, KeyCombo, Status};
use crate::profiles::{ProfileRow, ProfileStore};
use crate::theme::{PopupColors, Rgb, ThemeConfig};

const HINT: &str = "enter open  n new  x stop  d delete  s hotkey  q close";
const HEADER_ROWS: usize = 2;
const FOOTER_ROWS: usize = 2;

pub fn run(store: &mut ProfileStore, start_with_setup: bool) -> Result<()> {
    let mut screen = Screen::enter()?;
    let mut chooser = Chooser::new(store);
    chooser.refresh(None);
    if start_with_setup {
        chooser.set_hotkey(&mut screen)?;
    }
    loop {
        screen.draw(&chooser.frame(None))?;
        let key = read_key()?;
        if chooser.handle(key, &mut screen)? == Flow::Exit {
            return Ok(());
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Flow {
    Continue,
    Exit,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Key {
    Up,
    Down,
    Enter,
    Esc,
    Backspace,
    Char(char),
}

fn read_key() -> Result<Key> {
    loop {
        if let Event::Key(key) = event::read()? {
            if key.kind != KeyEventKind::Press {
                continue;
            }
            let mapped = match key.code {
                KeyCode::Up => Key::Up,
                KeyCode::Down => Key::Down,
                KeyCode::Enter => Key::Enter,
                KeyCode::Esc => Key::Esc,
                KeyCode::Backspace => Key::Backspace,
                KeyCode::Char('c') if key.modifiers.contains(KeyModifiers::CONTROL) => Key::Esc,
                KeyCode::Char(c) if !key.modifiers.contains(KeyModifiers::CONTROL) => Key::Char(c),
                _ => continue,
            };
            return Ok(mapped);
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Style {
    Normal,
    Bold,
    Dim,
    Selected,
}

struct Span {
    text: String,
    style: Style,
}

fn span(text: impl Into<String>, style: Style) -> Span {
    Span {
        text: text.into(),
        style,
    }
}

struct Frame {
    lines: Vec<Vec<Span>>,
    cursor: Option<(u16, u16)>,
}

/// Raw-mode alternate screen that restores the terminal when dropped.
struct Screen {
    out: Stdout,
    colors: PopupColors,
}

impl Screen {
    fn enter() -> Result<Self> {
        terminal::enable_raw_mode()?;
        let mut out = io::stdout();
        execute!(out, EnterAlternateScreen, Hide)?;
        let theme = ThemeConfig::load(&Herdr::from_env().config_path());
        let appearance = theme
            .follows_host_appearance()
            .then(host_background)
            .flatten()
            .map(Rgb::appearance);
        Ok(Self {
            out,
            colors: theme.resolve(appearance),
        })
    }

    fn paint(&self) -> Colors {
        Colors::new(to_color(self.colors.text), to_color(self.colors.background))
    }

    /// Herdr highlights its active rows with a dedicated background rather
    /// than reverse video; reverse is only the fallback for the terminal theme.
    fn selected_paint(&self) -> Option<Colors> {
        self.colors
            .active_row
            .map(|row| Colors::new(to_color(self.colors.text), to_color(Some(row))))
    }

    fn size() -> (usize, usize) {
        let (cols, rows) = terminal::size().unwrap_or((66, 16));
        (rows.max(6) as usize, cols.max(20) as usize)
    }

    fn draw(&mut self, frame: &Frame) -> Result<()> {
        let (rows, cols) = Self::size();
        let paint = self.paint();
        queue!(self.out, Hide, MoveTo(0, 0))?;
        for row in 0..rows {
            queue!(
                self.out,
                MoveTo(0, row as u16),
                SetColors(paint),
                Clear(ClearType::CurrentLine)
            )?;
            let mut used = 0;
            for Span { text, style } in frame.lines.get(row).map_or(&[][..], Vec::as_slice) {
                let text = fit(text, cols.saturating_sub(used));
                used += text.chars().count();
                let (colors, attribute) = match (style, self.selected_paint()) {
                    (Style::Selected, Some(highlight)) => (highlight, Some(Attribute::Bold)),
                    (Style::Selected, None) => (paint, Some(Attribute::Reverse)),
                    (Style::Bold, _) => (paint, Some(Attribute::Bold)),
                    (Style::Dim, _) => (paint, Some(Attribute::Dim)),
                    (Style::Normal, _) => (paint, None),
                };
                queue!(self.out, SetColors(colors))?;
                if let Some(attribute) = attribute {
                    queue!(self.out, SetAttribute(attribute))?;
                }
                queue!(
                    self.out,
                    Print(&text),
                    SetAttribute(Attribute::Reset),
                    SetColors(paint)
                )?;
                if used >= cols {
                    break;
                }
            }
            queue!(
                self.out,
                Print(" ".repeat(cols.saturating_sub(used))),
                ResetColor
            )?;
        }
        if let Some((col, row)) = frame.cursor {
            queue!(self.out, MoveTo(col, row), Show)?;
        }
        self.out.flush()?;
        Ok(())
    }
}

impl Drop for Screen {
    fn drop(&mut self) {
        let _ = execute!(
            self.out,
            SetAttribute(Attribute::Reset),
            ResetColor,
            Show,
            LeaveAlternateScreen
        );
        let _ = terminal::disable_raw_mode();
    }
}

/// Asks the terminal for its default background (OSC 11). Herdr answers for
/// panes with the host terminal's color, which decides light or dark themes.
fn host_background() -> Option<Rgb> {
    use std::io::Read;
    use std::time::{Duration, Instant};

    let mut out = io::stdout();
    out.write_all(b"\x1b]11;?\x1b\\").ok()?;
    out.flush().ok()?;
    let mut stdin = io::stdin();
    let deadline = Instant::now() + Duration::from_millis(150);
    let mut buffer = Vec::new();
    while Instant::now() < deadline
        && stdin_ready(deadline.saturating_duration_since(Instant::now()))
    {
        let mut chunk = [0u8; 256];
        let read = stdin.read(&mut chunk).ok()?;
        if read == 0 {
            break;
        }
        buffer.extend_from_slice(&chunk[..read]);
        if let Some(color) = parse_background_reply(&buffer) {
            return Some(color);
        }
    }
    None
}

fn stdin_ready(timeout: std::time::Duration) -> bool {
    let mut fd = libc::pollfd {
        fd: 0,
        events: libc::POLLIN,
        revents: 0,
    };
    let millis = timeout.as_millis().min(i32::MAX as u128) as i32;
    unsafe { libc::poll(&mut fd, 1, millis) > 0 }
}

fn parse_background_reply(data: &[u8]) -> Option<Rgb> {
    let text = String::from_utf8_lossy(data);
    let start = text.find("\x1b]11;rgb:")? + "\x1b]11;rgb:".len();
    let body = text[start..].split(['\x07', '\x1b']).next()?;
    let mut channels = body
        .split('/')
        .map(|part| u8::from_str_radix(part.get(..2)?, 16).ok());
    Some(Rgb {
        r: channels.next()??,
        g: channels.next()??,
        b: channels.next()??,
    })
}

fn to_color(rgb: Option<Rgb>) -> Color {
    rgb.map_or(Color::Reset, |c| Color::Rgb {
        r: c.r,
        g: c.g,
        b: c.b,
    })
}

fn fit(text: &str, width: usize) -> String {
    text.chars().take(width).collect()
}

fn pad(text: &str, width: usize) -> String {
    let len = text.chars().count();
    if len >= width {
        fit(text, width)
    } else {
        format!("{text}{}", " ".repeat(width - len))
    }
}

enum Message {
    Info(String),
    Error(String),
}

struct Chooser<'s, 'h> {
    store: &'s mut ProfileStore<'h>,
    rows: Vec<ProfileRow>,
    selected: usize,
    message: Option<Message>,
    binding: Option<Status>,
}

impl<'s, 'h> Chooser<'s, 'h> {
    fn new(store: &'s mut ProfileStore<'h>) -> Self {
        Self {
            store,
            rows: Vec::new(),
            selected: 0,
            message: None,
            binding: None,
        }
    }

    fn refresh(&mut self, select: Option<&str>) {
        self.rows = self.store.rows(true);
        let wanted = select
            .map(str::to_owned)
            .or_else(|| self.store.settings().last.clone());
        if let Some(index) = wanted.and_then(|name| self.rows.iter().position(|r| r.name() == name))
        {
            self.selected = index;
        }
        self.selected = self.selected.min(self.rows.len().saturating_sub(1));
        let herdr = Herdr::from_env();
        self.binding = keybinding::status(&herdr.config_path(), &herdr.default_config()).ok();
    }

    fn info(&mut self, text: impl Into<String>) {
        self.message = Some(Message::Info(text.into()));
    }

    fn error(&mut self, text: impl Into<String>) {
        self.message = Some(Message::Error(text.into()));
    }

    fn selected_row(&self) -> Option<&ProfileRow> {
        self.rows.get(self.selected)
    }

    fn frame(&self, prompt: Option<&str>) -> Frame {
        let (rows, cols) = Screen::size();
        let mut lines = Vec::with_capacity(rows);
        let head = " Profiles";
        let gap = cols.saturating_sub(head.len() + HINT.len()).max(1);
        lines.push(vec![
            span(head, Style::Bold),
            span(" ".repeat(gap), Style::Normal),
            span(HINT, Style::Dim),
        ]);
        let hotkey = match self.binding.as_ref().and_then(|s| s.binding.as_ref()) {
            Some(binding) => format!(" hotkey: {}", binding.key),
            None => " hotkey: none (press s to set one)".to_owned(),
        };
        lines.push(vec![span(hotkey, Style::Dim)]);

        let visible = rows.saturating_sub(HEADER_ROWS + FOOTER_ROWS).max(1);
        let first = self.selected.saturating_sub(visible - 1);
        if self.rows.is_empty() {
            lines.push(vec![span(
                "  No profiles yet. Press n to create one.",
                Style::Dim,
            )]);
        }
        for (index, row) in self.rows.iter().enumerate().skip(first).take(visible) {
            lines.push(vec![row_span(row, index == self.selected, cols)]);
        }
        while lines.len() < rows - 1 {
            lines.push(Vec::new());
        }
        lines.truncate(rows - 1);

        let mut cursor = None;
        let footer = match (prompt, &self.message) {
            (Some(prompt), _) => {
                cursor = Some(((prompt.chars().count() + 1) as u16, (rows - 2) as u16));
                span(format!(" {prompt}"), Style::Normal)
            }
            (None, Some(Message::Info(text))) => span(format!(" {text}"), Style::Dim),
            (None, Some(Message::Error(text))) => span(format!(" {text}"), Style::Bold),
            (None, None) => span("", Style::Normal),
        };
        lines[rows - 2] = vec![footer];
        Frame { lines, cursor }
    }

    fn handle(&mut self, key: Key, screen: &mut Screen) -> Result<Flow> {
        let count = self.rows.len();
        match key {
            Key::Esc | Key::Char('q') => return Ok(Flow::Exit),
            Key::Down | Key::Char('j') if count > 0 => self.selected = (self.selected + 1) % count,
            Key::Up | Key::Char('k') if count > 0 => {
                self.selected = (self.selected + count - 1) % count
            }
            Key::Char(digit @ '1'..='9') if (digit as usize - '1' as usize) < count => {
                self.selected = digit as usize - '1' as usize;
                return self.open_selected();
            }
            Key::Enter => return self.open_selected(),
            Key::Char('r') => {
                self.message = None;
                self.refresh(None);
            }
            Key::Char('s') => self.set_hotkey(screen)?,
            Key::Char('n') => self.create(screen)?,
            Key::Char('x') => self.stop_selected(screen)?,
            Key::Char('d') => self.delete_selected(screen)?,
            _ => {}
        }
        Ok(Flow::Continue)
    }

    fn open_selected(&mut self) -> Result<Flow> {
        let Some(row) = self.selected_row() else {
            return Ok(Flow::Continue);
        };
        match self.store.open(&row.id.clone(), false) {
            Ok(_) => Ok(Flow::Exit),
            Err(err) => {
                self.error(err.to_string());
                Ok(Flow::Continue)
            }
        }
    }

    fn create(&mut self, screen: &mut Screen) -> Result<()> {
        let Some(name) = self.prompt(screen, "New profile name:", "")? else {
            return Ok(());
        };
        match self.store.add(&name, None) {
            Ok(()) => {
                self.info(format!("Created '{name}'. Press enter to open it."));
                self.refresh(Some(&name));
            }
            Err(err) => self.error(err.to_string()),
        }
        Ok(())
    }

    fn stop_selected(&mut self, screen: &mut Screen) -> Result<()> {
        let Some(row) = self.selected_row().cloned() else {
            return Ok(());
        };
        if row.protected {
            self.error("The default profile is Herdr itself; stop it with `herdr server stop`.");
        } else if !row.running {
            self.error(format!("'{}' is not running.", row.name()));
        } else if row.current {
            self.error("That is this window. Use ctrl+b q to detach or herdr session stop.");
        } else if row.attached {
            self.error(format!(
                "'{}' is open in another window; close it there first.",
                row.name()
            ));
        } else if self.confirm(screen, &format!("Stop '{}' and its agents?", row.name()))? {
            match self.store.stop(&row.id) {
                Ok(()) => self.info(format!("Stopped '{}'.", row.name())),
                Err(err) => self.error(err.to_string()),
            }
            self.refresh(None);
        }
        Ok(())
    }

    fn delete_selected(&mut self, screen: &mut Screen) -> Result<()> {
        let Some(row) = self.selected_row().cloned() else {
            return Ok(());
        };
        if row.protected {
            self.error("The default profile cannot be deleted.");
        } else if row.running {
            self.error(format!("Stop '{}' before deleting it.", row.name()));
        } else if self.confirm(
            screen,
            &format!(
                "Delete '{}' and its {} saved spaces?",
                row.name(),
                row.spaces
            ),
        )? {
            match self.store.remove(&row.id) {
                Ok(()) => self.info(format!("Deleted '{}'.", row.name())),
                Err(err) => self.error(err.to_string()),
            }
            self.refresh(None);
        }
        Ok(())
    }

    fn set_hotkey(&mut self, screen: &mut Screen) -> Result<()> {
        let current = self
            .binding
            .as_ref()
            .and_then(|s| s.binding.as_ref())
            .map(|b| b.key.to_string())
            .unwrap_or_default();
        let Some(input) = self.prompt(screen, "Hotkey (e.g. prefix+a):", &current)? else {
            return Ok(());
        };
        let key: KeyCombo = match input.parse() {
            Ok(key) => key,
            Err(err) => {
                self.error(err.to_string());
                return Ok(());
            }
        };
        if let Some(owner) = self.binding.as_ref().and_then(|s| s.owner_of(&key)) {
            if !self.confirm(screen, &format!("{key} is used by {owner}. Bind anyway?"))? {
                return Ok(());
            }
        }
        let herdr = Herdr::from_env();
        match keybinding::apply(&herdr.config_path(), &key) {
            Ok(()) => {
                herdr.reload_config();
                self.info(format!("Hotkey set to {key}. Config reloaded."));
                self.refresh(None);
            }
            Err(err) => self.error(err.to_string()),
        }
        Ok(())
    }

    fn prompt(
        &mut self,
        screen: &mut Screen,
        label: &str,
        initial: &str,
    ) -> Result<Option<String>> {
        let mut text = initial.to_owned();
        loop {
            screen.draw(&self.frame(Some(&format!("{label} {text}"))))?;
            match read_key()? {
                Key::Esc => return Ok(None),
                Key::Enter => return Ok(Some(text.trim().to_owned())),
                Key::Backspace => {
                    text.pop();
                }
                Key::Char(c) if text.chars().count() < 64 => text.push(c),
                _ => {}
            }
        }
    }

    fn confirm(&mut self, screen: &mut Screen, question: &str) -> Result<bool> {
        screen.draw(&self.frame(Some(&format!("{question} [y/N]"))))?;
        Ok(matches!(read_key()?, Key::Char('y' | 'Y')))
    }
}

fn row_span(row: &ProfileRow, selected: bool, width: usize) -> Span {
    let marker = if selected { "▸" } else { " " };
    let label = if row.label == row.name() {
        row.label.clone()
    } else {
        format!("{} ({})", row.label, row.name())
    };
    let spaces = format!(
        "{} space{}",
        row.spaces,
        if row.spaces == 1 { "" } else { "s" }
    );
    let blocked = if row.blocked > 0 {
        format!("  ● {} blocked", row.blocked)
    } else {
        String::new()
    };
    let here = if row.current { "  (this window)" } else { "" };
    let text = format!(
        " {marker} {} {:<8} {:<10}{blocked}{here}",
        pad(&label, 22),
        row.state(),
        spaces
    );
    let style = match (selected, row.running) {
        (true, _) => Style::Selected,
        (false, true) => Style::Bold,
        (false, false) => Style::Normal,
    };
    Span {
        text: pad(&text, width),
        style,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_background_reply() {
        assert_eq!(
            parse_background_reply(b"\x1b]11;rgb:2828/2828/2828\x1b\\"),
            Some(Rgb {
                r: 40,
                g: 40,
                b: 40
            })
        );
        assert_eq!(
            parse_background_reply(b"\x1b]11;rgb:ff/ee/dd\x07"),
            Some(Rgb {
                r: 255,
                g: 238,
                b: 221
            })
        );
        assert_eq!(parse_background_reply(b"j"), None);
    }

    #[test]
    fn pad_and_fit_respect_character_widths() {
        assert_eq!(pad("ab", 4), "ab  ");
        assert_eq!(pad("abcdef", 4), "abcd");
        assert_eq!(fit("▸ été", 3), "▸ é");
    }
}
