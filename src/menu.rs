use std::{error::Error, fs::{read_dir}, path::{Path, PathBuf}};

use crate::beatmap::{MapHeader, parse_header};

use ratatui::{
    buffer::Buffer,
    layout::{Constraint, Layout, Rect},
    style::{Modifier, Style, Stylize},
    text::{Line, Span},
    widgets::{Block, Borders, List, ListItem, ListState, Paragraph, StatefulWidget, Widget},
};

// TODO temp; converts are not really working right now
const INCLUDE_CONVERTS: bool = false;

#[derive(Debug)]
pub struct DiffEntry {
    pub path: PathBuf,
    pub header: MapHeader,
}

#[derive(Debug)]
pub struct SongEntry {
    pub dir: PathBuf,
    pub title: String,
    pub artist: String,
    pub diffs: Vec<DiffEntry>,
}

pub fn scan_songs_dir(root: &Path) -> Result<Vec<SongEntry>, Box<dyn Error>> {
    let mut songs = Vec::new();

    for entry in read_dir(root)? {
        let dir = entry?.path();
        if !dir.is_dir() {
            continue;
        }

        match scan_song(&dir) {
            Ok(Some(song)) => songs.push(song),
            Ok(None) => {}
            Err(e) => eprintln!("skipping {}: {e}", dir.display()),
        }
    }

    songs.sort_by(|a, b| {
        a.artist.to_lowercase().cmp(&b.artist.to_lowercase())
            .then_with(|| a.title.to_lowercase().cmp(&b.title.to_lowercase()))
    });

    Ok(songs)
}

fn scan_song(dir: &Path) -> Result<Option<SongEntry>, Box<dyn Error>> {
    let diffs: Vec<DiffEntry> = read_dir(dir)?
        .filter_map(|e| e.ok())
        .map(|e| e.path())
        .filter(|p| p.extension().is_some_and(|x| x.eq("osu")))
        .filter_map(|path| {
            let header = parse_header(&path).ok()?;
            Some(DiffEntry { path, header })
        })
        .filter(|d| if INCLUDE_CONVERTS { true } else { d.header.mode == 1 }) 
        .collect();

    if diffs.is_empty() {
        return Ok(None);
    }

    //TODO sort diffs somehow

    let first = &diffs[0].header;
    Ok(Some(SongEntry {
        title: first.title.clone(),
        artist: first.artist.clone(),
        dir: dir.to_path_buf(),
        diffs,
    }))
}

#[derive(Clone, Copy, PartialEq)]
pub enum Row {
    Song(usize),
    Diff(usize, usize),
}

pub struct Menu {
    pub songs: Vec<SongEntry>,
    pub expanded: Option<usize>,
    pub rows: Vec<Row>,
    pub state: ListState,
}

impl Menu {
    pub fn new(songs: Vec<SongEntry>) -> Self {
        let mut menu = Self {
            songs,
            expanded: None,
            rows: Vec::new(),
            state: ListState::default(),
        };
        menu.rebuild(None);
        menu.state.select(if menu.rows.is_empty() { None } else { Some(0) });
        menu
    }

    fn rebuild(&mut self, keep: Option<Row>) {
        self.rows.clear();
        for (i, song) in self.songs.iter().enumerate() {
            self.rows.push(Row::Song(i));
            if self.expanded == Some(i) {
                for d in 0..song.diffs.len() {
                    self.rows.push(Row::Diff(i, d));
                }
            }
        }

        if let Some(target) = keep {
            let idx = self.rows.iter().position(|r| *r == target);
            self.state.select(idx.or(Some(0)));
        }
    }

    pub fn selected(&self) -> Option<Row> {
        self.state.selected().and_then(|i| self.rows.get(i).copied())
    }

    pub fn move_by(&mut self, delta: isize) {
        if self.rows.is_empty() {
            return;
        }
        let cur = self.state.selected().unwrap_or(0) as isize;
        let next = (cur + delta).rem_euclid(self.rows.len() as isize);
        self.state.select(Some(next as usize));
    }

    pub fn activate(&mut self) -> Option<(PathBuf, MapHeader)> {
        match self.selected()? {
            Row::Song(i) => {
                self.expanded = if self.expanded == Some(i) { 
                    None 
                } else { 
                    Some(i) 
                };

                self.rebuild(Some(Row::Song(i)));
                None
            },
            Row::Diff(i, d) => {
                let diff = &self.songs[i].diffs[d];
                Some( (self.songs[i].diffs[d].path.clone(), diff.header.clone()) )
            },
        }
    }

    pub fn collapse(&mut self) {
        if let Some(Row::Diff(i, _)) = self.selected() {
            self.expanded = None;
            self.rebuild(Some(Row::Song(i)));
        }
    }

    pub fn hovered_audio(&self) -> Option<(&Path, u32)> {
        let Row::Diff(i, d) = self.selected()? else {
            return None;
        };
        
        let h = &self.songs[i].diffs[d].header;
        Some((&h.song_path, h.preview_ms))
    }
}

impl Widget for &mut Menu {
    fn render(self, area: Rect, buf: &mut Buffer) {
        let [list_area, detail_area] =
            Layout::horizontal([Constraint::Percentage(60), Constraint::Min(20)]).areas(area);

        let items: Vec<ListItem> = self
            .rows
            .iter()
            .map(|row| match row {
                Row::Song(i) => {
                    let song = &self.songs[*i];
                    let marker = if self.expanded == Some(*i) { "▼" } else { "▶" };
                    ListItem::new(Line::from(vec![
                        Span::raw(format!("{marker} ")),
                        Span::styled(song.title.clone(), Style::new().bold()),
                        Span::raw(format!(" - {}", song.artist)).dark_gray(),
                    ]))
                },

                Row::Diff(i, d) => {
                    let diff = &self.songs[*i].diffs[*d];
                    ListItem::new(Line::from(format!(
                        "    ◦ {}",
                        diff.header.diff_name
                    )))
                }
            })
            .collect();

        let list = List::new(items)
            .block(Block::default().borders(Borders::ALL).title(" Maps "))
            .highlight_style(Style::new().add_modifier(Modifier::REVERSED))
            .highlight_symbol("");

        StatefulWidget::render(list, list_area, buf, &mut self.state);

        let detail = match self.selected() {
            Some(Row::Diff(i, d)) => {
                let diff = &self.songs[*&i].diffs[d];
                vec![
                    Line::from(diff.header.diff_name.clone()).bold(),
                    Line::from(format!("Mapped by {}", diff.header.creator)).dark_gray(),
                    Line::from(""),
                    Line::from(format!("OD     {:.1}", diff.header.od)),
                ]
            },
            Some(Row::Song(i)) => {
                let song = &self.songs[i];
                vec![
                    Line::from(song.title.clone()).bold(),
                    Line::from(song.artist.clone()).dark_gray(),
                ]
            },
            None => vec![Line::from("No maps found").dark_gray()],
        };

        Paragraph::new(detail)
            .block(Block::default().borders(Borders::ALL).title(" Map Info "))
            .render(detail_area, buf);
    }
}