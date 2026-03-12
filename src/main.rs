use std::io::{self, Stdout, stdout};
use std::time::Duration;

use chrono::{DateTime, FixedOffset, Local, NaiveDate, Utc};
use chrono_tz::{TZ_VARIANTS, Tz};
use crossterm::cursor::{Hide, Show};
use crossterm::event::{self, Event, KeyCode, KeyEvent, KeyEventKind, KeyModifiers};
use crossterm::execute;
use crossterm::terminal::{EnterAlternateScreen, LeaveAlternateScreen, disable_raw_mode, enable_raw_mode};
use ratatui::backend::CrosstermBackend;
use ratatui::layout::{Constraint, Direction, Layout};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::Line;
use ratatui::widgets::{Cell, Paragraph, Row, Table};
use ratatui::{Frame, Terminal};

const HEADER_HEIGHT: u16 = 2;
const REFRESH_INTERVAL: Duration = Duration::from_millis(250);
const COLUMN_SPACING: u16 = 2;

type AppTerminal = Terminal<CrosstermBackend<Stdout>>;

struct DisplayRow {
    zone: String,
    local_time: String,
    offset: String,
    status: String,
}

struct App {
    zones: Vec<Tz>,
    offsets: Vec<i32>,
    mode: ViewMode,
    pending_g: bool,
    scroll: usize,
}

#[derive(Clone, Copy, Eq, PartialEq)]
enum ViewMode {
    Offsets,
    AllZones,
}

impl App {
    fn new() -> Self {
        Self {
            zones: TZ_VARIANTS.to_vec(),
            offsets: (-12..=14).map(|hours| hours * 3600).collect(),
            mode: ViewMode::Offsets,
            pending_g: false,
            scroll: 0,
        }
    }

    fn run(&mut self) -> io::Result<()> {
        let mut terminal = TerminalSession::enter()?;
        let mut last_rendered_second = i64::MIN;

        loop {
            let area = terminal.terminal.size()?;
            let viewport_rows = visible_rows(area.height);
            self.clamp_scroll(viewport_rows);

            let now = Utc::now();
            let current_second = now.timestamp();
            if current_second != last_rendered_second {
                self.render(&mut terminal.terminal, now)?;
                last_rendered_second = current_second;
            }

            if event::poll(REFRESH_INTERVAL)? {
                match event::read()? {
                    Event::Key(key) if key.kind == KeyEventKind::Press => {
                        if self.handle_key(key, viewport_rows) {
                            break;
                        }

                        self.render(&mut terminal.terminal, Utc::now())?;
                        last_rendered_second = Utc::now().timestamp();
                    }
                    Event::Resize(_, _) => {
                        let area = terminal.terminal.size()?;
                        let viewport_rows = visible_rows(area.height);
                        self.clamp_scroll(viewport_rows);
                        self.render(&mut terminal.terminal, Utc::now())?;
                        last_rendered_second = Utc::now().timestamp();
                    }
                    _ => {}
                }
            }
        }

        Ok(())
    }

    fn handle_key(&mut self, key: KeyEvent, viewport_rows: usize) -> bool {
        let pending_g = self.pending_g;
        self.pending_g = false;

        match (key.code, key.modifiers) {
            (KeyCode::Char('q'), KeyModifiers::NONE) | (KeyCode::Esc, _) => true,
            (KeyCode::Char('a'), KeyModifiers::NONE) => {
                self.mode = match self.mode {
                    ViewMode::Offsets => ViewMode::AllZones,
                    ViewMode::AllZones => ViewMode::Offsets,
                };
                self.pending_g = false;
                self.scroll = 0;
                self.clamp_scroll(viewport_rows);
                false
            }
            (KeyCode::Up, _) | (KeyCode::Char('k'), KeyModifiers::NONE) => {
                self.scroll = self.scroll.saturating_sub(1);
                false
            }
            (KeyCode::Down, _) | (KeyCode::Char('j'), KeyModifiers::NONE) => {
                self.scroll = self.scroll.saturating_add(1);
                self.clamp_scroll(viewport_rows);
                false
            }
            (KeyCode::PageUp, _) => {
                self.scroll = self.scroll.saturating_sub(viewport_rows.max(1));
                false
            }
            (KeyCode::PageDown, _) => {
                self.scroll = self.scroll.saturating_add(viewport_rows.max(1));
                self.clamp_scroll(viewport_rows);
                false
            }
            (KeyCode::Char('u'), modifiers) if modifiers.contains(KeyModifiers::CONTROL) => {
                self.scroll = self.scroll.saturating_sub((viewport_rows / 2).max(1));
                false
            }
            (KeyCode::Char('d'), modifiers) if modifiers.contains(KeyModifiers::CONTROL) => {
                self.scroll = self.scroll.saturating_add((viewport_rows / 2).max(1));
                self.clamp_scroll(viewport_rows);
                false
            }
            (KeyCode::Home, _) => {
                self.scroll = 0;
                false
            }
            (KeyCode::End, _) | (KeyCode::Char('G'), _) => {
                self.scroll = self.max_scroll(viewport_rows);
                false
            }
            (KeyCode::Char('g'), KeyModifiers::NONE) if pending_g => {
                self.scroll = 0;
                false
            }
            (KeyCode::Char('g'), KeyModifiers::NONE) => {
                self.pending_g = true;
                false
            }
            _ => false,
        }
    }

    fn render(&self, terminal: &mut AppTerminal, now: DateTime<Utc>) -> io::Result<()> {
        terminal.draw(|frame| self.draw(frame, now))?;
        Ok(())
    }

    fn draw(&self, frame: &mut Frame, now: DateTime<Utc>) {
        let area = frame.area();

        if area.width < 32 || area.height < 5 {
            let warning = Paragraph::new(vec![
                Line::from("Window too small for the world clock."),
                Line::from("Resize the terminal or press q to quit."),
            ]);
            frame.render_widget(warning, area);
            return;
        }

        let chunks = Layout::default()
            .direction(Direction::Vertical)
            .constraints([Constraint::Length(HEADER_HEIGHT), Constraint::Min(1)])
            .split(area);

        let viewport_rows = visible_rows(area.height);
        let total = self.active_len();
        let start = self.scroll.min(total.saturating_sub(1));
        let end = (start + viewport_rows).min(total);

        let header = Paragraph::new(vec![
            Line::from(format!(
                "World Clock | {} | UTC {} | Showing {}-{} of {}",
                self.mode_label(),
                now.format("%Y-%m-%d %H:%M:%S"),
                start + 1,
                end.max(1),
                total
            )),
            Line::from("Arrows or j/k scroll, Ctrl-u/Ctrl-d jump, gg/G move, a toggles all zones, q exits."),
        ]);
        frame.render_widget(header, chunks[0]);

        let column_headers = self.column_headers();
        let rows = self.rows(now, start, end);

        let header = Row::new(column_headers).style(
            Style::default()
                .fg(Color::Black)
                .bg(Color::Gray)
                .add_modifier(Modifier::BOLD),
        );

        let table = Table::new(
            rows.iter().enumerate().map(|(index, row)| {
                let absolute_index = start + index;
                Row::new(vec![
                    Cell::from(row.zone.clone()),
                    Cell::from(row.local_time.clone()),
                    Cell::from(row.offset.clone()),
                    Cell::from(row.status.clone()),
                ])
                .style(row_style(absolute_index, total, row.status == "Local"))
            }),
            self.column_constraints(chunks[1].width, &column_headers, &rows),
        )
        .header(header)
        .column_spacing(COLUMN_SPACING);

        frame.render_widget(table, chunks[1]);
    }

    fn clamp_scroll(&mut self, viewport_rows: usize) {
        self.scroll = self.scroll.min(self.max_scroll(viewport_rows));
    }

    fn max_scroll(&self, viewport_rows: usize) -> usize {
        self.active_len().saturating_sub(viewport_rows)
    }

    fn active_len(&self) -> usize {
        match self.mode {
            ViewMode::Offsets => self.offsets.len(),
            ViewMode::AllZones => self.zones.len(),
        }
    }

    fn mode_label(&self) -> &'static str {
        match self.mode {
            ViewMode::Offsets => "UTC offsets",
            ViewMode::AllZones => "All IANA zones",
        }
    }

    fn column_headers(&self) -> [&'static str; 4] {
        match self.mode {
            ViewMode::Offsets => ["UTC", "Time", "Offset", "Status"],
            ViewMode::AllZones => ["Zone", "Time", "Offset", "Status"],
        }
    }

    fn column_constraints(
        &self,
        table_width: u16,
        headers: &[&str; 4],
        rows: &[DisplayRow],
    ) -> [Constraint; 4] {
        let spacing = usize::from(COLUMN_SPACING) * 3;
        let mut zone_width = headers[0].len();
        let mut time_width = headers[1].len();
        let mut offset_width = headers[2].len();
        let mut status_width = headers[3].len();

        for row in rows {
            zone_width = zone_width.max(row.zone.len());
            time_width = time_width.max(row.local_time.len());
            offset_width = offset_width.max(row.offset.len());
            status_width = status_width.max(row.status.len());
        }

        let (min_zone_width, max_zone_width) = match self.mode {
            ViewMode::Offsets => (3usize, 9usize),
            ViewMode::AllZones => (8usize, 24usize),
        };

        zone_width = zone_width.clamp(min_zone_width, max_zone_width);
        time_width = time_width.clamp(8, 19);
        offset_width = offset_width.clamp(6, 6);
        status_width = status_width.clamp(6, 9);

        let available = usize::from(table_width).saturating_sub(spacing);
        while zone_width + time_width + offset_width + status_width > available {
            if zone_width > min_zone_width {
                zone_width -= 1;
            } else if time_width > 8 {
                time_width -= 1;
            } else if status_width > 6 {
                status_width -= 1;
            } else {
                break;
            }
        }

        [
            Constraint::Length(zone_width as u16),
            Constraint::Length(time_width as u16),
            Constraint::Length(offset_width as u16),
            Constraint::Length(status_width as u16),
        ]
    }

    fn rows(&self, now: DateTime<Utc>, start: usize, end: usize) -> Vec<DisplayRow> {
        let local_now = now.with_timezone(&Local);
        let local_date = local_now.date_naive();
        let local_offset = local_now.format("%:z").to_string();
        match self.mode {
            ViewMode::Offsets => self.offsets[start..end]
                .iter()
                .map(|seconds| offset_row(now, *seconds, local_date, &local_offset))
                .collect(),
            ViewMode::AllZones => self.zones[start..end]
                .iter()
                .map(|zone| zone_row(now, *zone, local_date, &local_offset))
                .collect(),
        }
    }
}

struct TerminalSession {
    terminal: AppTerminal,
}

impl TerminalSession {
    fn enter() -> io::Result<Self> {
        enable_raw_mode()?;

        let mut stdout = stdout();
        execute!(stdout, EnterAlternateScreen, Hide)?;

        let backend = CrosstermBackend::new(stdout);
        let mut terminal = Terminal::new(backend)?;
        terminal.clear()?;

        Ok(Self { terminal })
    }
}

impl Drop for TerminalSession {
    fn drop(&mut self) {
        let _ = self.terminal.show_cursor();
        let _ = execute!(self.terminal.backend_mut(), Show, LeaveAlternateScreen);
        let _ = disable_raw_mode();
    }
}

fn visible_rows(height: u16) -> usize {
    usize::from(height)
        .saturating_sub(HEADER_HEIGHT as usize + 1)
        .max(1)
}

fn offset_row(
    now: DateTime<Utc>,
    seconds: i32,
    local_date: NaiveDate,
    local_offset: &str,
) -> DisplayRow {
    let offset = FixedOffset::east_opt(seconds).expect("valid UTC offset");
    let local = now.with_timezone(&offset);
    let offset_text = local.format("%:z").to_string();

    DisplayRow {
        zone: offset_label(seconds),
        local_time: local.format("%Y-%m-%d %H:%M:%S").to_string(),
        status: status_label(local.date_naive(), local_date, &offset_text, local_offset),
        offset: offset_text,
    }
}

fn zone_row(
    now: DateTime<Utc>,
    zone: Tz,
    local_date: NaiveDate,
    local_offset: &str,
) -> DisplayRow {
    let local = now.with_timezone(&zone);
    let offset_text = local.format("%:z").to_string();

    DisplayRow {
        zone: zone.name().to_string(),
        local_time: local.format("%Y-%m-%d %H:%M:%S").to_string(),
        status: status_label(local.date_naive(), local_date, &offset_text, local_offset),
        offset: offset_text,
    }
}

fn row_style(index: usize, total: usize, is_local: bool) -> Style {
    let total = total.max(1) as f32;
    let hue = 360.0 * (index as f32 / total);
    let (r, g, b) = hsv_to_rgb(hue, 0.75, 0.95);
    let style = Style::default().fg(Color::Rgb(r, g, b));

    if is_local {
        let (bg_r, bg_g, bg_b) = hsv_to_rgb(hue, 0.55, 0.22);
        style
            .bg(Color::Rgb(bg_r, bg_g, bg_b))
            .add_modifier(Modifier::BOLD)
    } else {
        style
    }
}

fn hsv_to_rgb(hue: f32, saturation: f32, value: f32) -> (u8, u8, u8) {
    let chroma = value * saturation;
    let hue_prime = (hue / 60.0) % 6.0;
    let x = chroma * (1.0 - ((hue_prime % 2.0) - 1.0).abs());
    let (r1, g1, b1) = match hue_prime {
        h if h < 1.0 => (chroma, x, 0.0),
        h if h < 2.0 => (x, chroma, 0.0),
        h if h < 3.0 => (0.0, chroma, x),
        h if h < 4.0 => (0.0, x, chroma),
        h if h < 5.0 => (x, 0.0, chroma),
        _ => (chroma, 0.0, x),
    };
    let m = value - chroma;

    (
        ((r1 + m) * 255.0).round() as u8,
        ((g1 + m) * 255.0).round() as u8,
        ((b1 + m) * 255.0).round() as u8,
    )
}

fn offset_label(seconds: i32) -> String {
    if seconds == 0 {
        return "UTC".to_string();
    }

    let total_minutes = seconds / 60;
    let sign = if total_minutes >= 0 { '+' } else { '-' };
    let absolute_minutes = total_minutes.abs();
    let hours = absolute_minutes / 60;
    let minutes = absolute_minutes % 60;

    if minutes == 0 {
        format!("UTC{sign}{hours}")
    } else {
        format!("UTC{sign}{hours:02}:{minutes:02}")
    }
}

fn status_label(
    row_date: NaiveDate,
    local_date: NaiveDate,
    row_offset: &str,
    local_offset: &str,
) -> String {
    if row_offset == local_offset {
        "Local".to_string()
    } else if row_date < local_date {
        "Yesterday".to_string()
    } else if row_date > local_date {
        "Tomorrow".to_string()
    } else {
        String::new()
    }
}

fn main() -> io::Result<()> {
    App::new().run()
}
