use std::{error::Error, fs::File, io::{BufRead, BufReader}, path::{Path, PathBuf}};

use ratatui::{style::Color};

use crate::{BLUE, RED};

#[derive(Debug, PartialEq)]
pub enum NoteType {
    Don,
    Kat
}

#[derive(Debug)]
pub struct Note {
    // time since song starts in ms
    pub time: u32,
    pub note_type: NoteType,
    pub big: bool, // TODO what is this called
    pub hit: bool,
}

impl Note {
    pub fn color(&self) -> Color {
        match self.note_type {
            NoteType::Don => RED,
            NoteType::Kat => BLUE,
        }
    }
}

#[derive(Debug, Clone)]
pub struct MapHeader {
    pub title: String,
    pub artist: String,
    pub diff_name: String,
    pub mode: u8,
    pub creator: String,
    pub od: f64,
    pub song_path: PathBuf,
    pub preview_ms: u32,
}

impl MapHeader {
    fn new() -> Self {
        MapHeader {
            title: String::new(),
            artist: String::new(),
            diff_name: String::new(),
            mode: 0,
            creator: String::new(),
            od: 5.0,
            song_path: PathBuf::new(),
            preview_ms: 0,
        }
    }
}

#[derive(Debug)]
pub struct Beatmap {
    pub header: MapHeader,
    pub timing_windows: TimingWindows,
    pub notes: Vec<Note>
}

// Timing hit windows in ms
#[derive(Debug)]
pub struct TimingWindows {
    pub great: f64,
    pub ok: f64,
    pub miss: f64
}

impl TimingWindows {
    pub fn new(od: f64) -> Self {
        let great = 50.0 - 3.0 * od;

        let ok = if od <= 5.0 {
            120.0 - 8.0 * od
        } else {
            110.0 - 6.0 * od
        };

        let miss = if od <= 5.0 {
            135.0 - 8.0 * od
        } else {
            120.0 - 5.0 * od
        };

        Self {
            great,
            ok,
            miss 
        }
    }
}

fn parse_hit_object(line: &str) -> Option<Note> {
    let mut values = line.split(',');
    let (_, _, time, object_type, hitsound) = (values.next()?, values.next()?, values.next()?, values.next()?, values.next()?);

    let object_type: u32 = object_type.trim().parse().ok()?;
    // TODO checks if its a circle, ignore for now if it isnt
    if (object_type & 1) != 1 {
        return None;
    }

    let hitsound: u32 = hitsound.trim().parse().ok()?;
    // time in ms
    let time: u32 = time.trim().parse().ok()?;

    // TODO check actual bfs
    let note_type = match hitsound {
        0 => NoteType::Don,
        4 => NoteType::Don,
        8 => NoteType::Kat,
        12 => NoteType::Kat,
        _ => { return None; }
    };

    let big = match hitsound {
        0 => false,
        8 => false,
        12 => true,
        4 => true,
        _ => { return None; }
    };

    Some(Note {
        time,
        note_type,
        big,
        hit: false
    })

}

//TODO this is bad right now

pub fn parse_header(path: &Path) -> Result<MapHeader, Box<dyn Error>> {
    let reader = BufReader::new(File::open(path)?);
    let dir = path.parent().unwrap_or(Path::new("."));
    
    let mut section = String::new();
    
    let mut header = MapHeader::new();
    let mut od = 5f64;

    for line_result in reader.lines() {
        let raw = line_result?;
        let line = raw.trim_start_matches('\u{feff}').trim();

        if line.is_empty() || line.starts_with("//") {
            continue;
        }

        if line.starts_with('[') && line.ends_with(']') {
            section = line.to_string();
            continue;
        }

        match section.as_str() {
            "[General]" => {
                if let Some(audio) = line.strip_prefix("AudioFilename:") {
                    header.song_path = dir.join(audio.trim());
                } else if let Some(p) = line.strip_prefix("PreviewTime:") {
                    header.preview_ms = p.trim().parse().unwrap_or(0);
                } else if let Some(m) = line.strip_prefix("Mode:") {
                    header.mode = m.trim().parse().unwrap_or(0);
                } 
            },
            "[Difficulty]" => {
                if let Some(l) = line.strip_prefix("OverallDifficulty:") {
                    od = l.trim().parse().unwrap();
                }
            },
            "[Metadata]" => {
                if let Some(t) = line.strip_prefix("Title:") {
                    header.title = t.trim().to_string();
                } else if let Some(c) = line.strip_prefix("Creator:") {
                    header.creator = c.trim().to_string();
                } else if let Some(d) = line.strip_prefix("Version:") {
                    header.diff_name = d.trim().to_string();
                } else if let Some(a) = line.strip_prefix("Artist:") {
                    header.artist = a.trim().to_string();
                }
            },
            _ => {}
        }
    }

    header.od = od;

    Ok(header)
}

// Assumes the header matches the map correctly

pub fn parse_map(path: &Path, header: &MapHeader) -> Result<Beatmap, Box<dyn Error>> {
    let reader = BufReader::new(File::open(path)?);
    
    let mut section = String::new();
    let mut notes = Vec::new();

    for line_result in reader.lines() {
        let raw = line_result?;
        let line = raw.trim_start_matches('\u{feff}').trim();

        if line.is_empty() || line.starts_with("//") {
            continue;
        }

        if line.starts_with('[') && line.ends_with(']') {
            section = line.to_string();
            continue;
        }

        match section.as_str() {
            "[HitObjects]" => {
                if let Some(obj) = parse_hit_object(line) {
                    notes.push(obj);
                }
            }
            _ => {}
        }
    }

    Ok(Beatmap {
        header: header.clone(),
        timing_windows: TimingWindows::new(header.od),
        notes
    })
}