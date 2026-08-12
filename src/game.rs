use std::{error::Error, path::Path};

use crossterm::event::{KeyCode, KeyEventKind};
use kira::{AudioManager, Tween, clock::{ClockHandle, ClockSpeed}, sound::static_sound::{StaticSoundData, StaticSoundHandle}};
use ratatui::{buffer::Buffer, layout::{Constraint, Layout, Rect}, style::{Color, Stylize}, symbols::Marker, text::Line, widgets::{Widget, canvas::{Canvas, Circle, Line as CanvasLine, Painter, Shape}}};

use crate::{HitSounds, Options, beatmap::{Beatmap, MapHeader, NoteType, parse_map}};

const CLOCK_HZ: f64 = 1000.0;
// TODO time taken for circle to cross screen. This should be based on sv?
const APPROACH_MS: f64 = 1200.0;

// give a bit of time in the beginning before starting
const START_OFFSET_TICKS: f64 = 1000.0;
// delay between ending a map and quitting to menu
const OUTRO_MS: f64 = 5000.0;

pub const GREEN:  Color = Color::Rgb(0x39, 0xFF, 0x88); 
pub const YELLOW: Color = Color::Rgb(0xFF, 0xD6, 0x2E); 
pub const RED:    Color = Color::Rgb(0xFF, 0x2D, 0x3A);

pub struct Game {
    song_handle: StaticSoundHandle,
    clock: ClockHandle,
    beatmap: Beatmap,
    // index of next note to be judged
    next_note: usize,
    combo: u32,
    misses: u32,
    latest_judgement: Option<Judgement>,
    // song time in ms when map finished
    song_finished_at: Option<f64>,
}

enum Judgement {
    Great,
    Ok,
    Miss
}

impl Judgement {
    fn label(&self) -> String {
        match self {
            Judgement::Great => String::from("GREAT"),
            Judgement::Ok => String::from("OK"),
            Judgement::Miss => String::from("Miss"),
        }
    }

    fn color(&self) -> Color {
        match self {
            Judgement::Great => YELLOW,
            Judgement::Ok => GREEN,
            Judgement::Miss => RED,
        }
    }
}

impl Game {
    pub fn new(manager: &mut AudioManager, path: &Path, header: MapHeader) -> Result<Self, Box<dyn Error>> {
        let beatmap = parse_map(path, &header)?;
        let mut clock = manager.add_clock(ClockSpeed::TicksPerSecond(CLOCK_HZ))?;

        let song = StaticSoundData::from_file(&header.song_path)?.start_time(clock.time() + START_OFFSET_TICKS);
        let song_handle = manager.play(song)?;
        clock.start();

        Ok(Self {
            song_handle,
            clock,
            beatmap,
            next_note: 0,
            combo: 0,
            misses: 0,
            latest_judgement: None,
            song_finished_at: None,
        })
    }

    // time in ms since song started
    fn time(&self) -> f64{
        let clock_time = self.clock.time();
        ((clock_time.ticks as f64 + clock_time.fraction - START_OFFSET_TICKS) / CLOCK_HZ) * 1000.0
    }

    pub fn stop(&mut self) {
        self.song_handle.stop(Tween::default());
    }

    // TODO temporary returns false if stopped, true if running
    pub fn update(&mut self, manager: &mut AudioManager, hitsounds: &HitSounds) -> bool {
        let now = self.time();

        if self.next_note >= self.beatmap.notes.len() {
            match self.song_finished_at {
                None => self.song_finished_at = Some(now),
                Some(t) if now - t >= OUTRO_MS => { 
                    self.stop();
                    return false;
                },
                Some(_) => {}
            }
        }

        let mut missed = 0;

        while self.next_note < self.beatmap.notes.len() {
            let note = &mut self.beatmap.notes[self.next_note];

            if note.hit {
                self.next_note += 1;
            } else if now - (note.time as f64) > self.beatmap.timing_windows.miss {
                note.hit = true;
                missed += 1;
                self.next_note += 1;
            } else {
                break;
            }
        }

        if missed > 0 {
            if self.combo >= 20 {
                // TODO handle error 
                let _ = manager.play(hitsounds.miss.clone());
            }
            self.combo = 0;
            self.misses += missed;
            self.latest_judgement = Some(Judgement::Miss);
        }

        true
    }

    fn judge(&mut self, note_type: NoteType, manager: &mut AudioManager, hitsounds: &HitSounds) {
        let now = self.time();
        let windows = &self.beatmap.timing_windows;

        if self.next_note >= self.beatmap.notes.len() {
            return;
        }
    
        let note = &mut self.beatmap.notes[self.next_note];

        if note.hit || note.time as f64 - now > windows.miss {
            return;
        }

        let delta = (note.time as f64 - now).abs();
        let wrong_key = note.note_type != note_type;
        note.hit = true;
 
        let judgement = if wrong_key {
            Judgement::Miss
        } else if delta <= windows.great {
            Judgement::Great
        } else if delta <= windows.ok {
            Judgement::Ok
        } else {
            Judgement::Miss
        };
 
        match judgement {
            Judgement::Miss => {
                if self.combo >= 20 {
                    // TODO handle error 
                    let _ = manager.play(hitsounds.miss.clone());
                }
                self.combo = 0;
                self.misses += 1;
            }
            _ => {
                self.combo += 1;

                //TODO scoring etc
            }
        }

        self.latest_judgement = Some(judgement);
    }

    pub fn handle_key_event(&mut self, options: &Options, manager: &mut AudioManager, hitsounds: &HitSounds, key_event: crossterm::event::KeyEvent) -> Result<(), Box<dyn Error>> {
        if key_event.kind != KeyEventKind::Press {
            return Ok(());
        }
        
        if key_event.code == KeyCode::Char(options.centre_left) || key_event.code == KeyCode::Char(options.centre_right) {
            manager.play(hitsounds.don.clone())?;
            self.judge(NoteType::Don, manager, hitsounds);
        } else if key_event.code == KeyCode::Char(options.rim_left) || key_event.code == KeyCode::Char(options.rim_right) {
            manager.play(hitsounds.kat.clone())?;
            self.judge(NoteType::Kat, manager, hitsounds);
        }

        Ok(())
    }

    fn render_playarea(&self, area: Rect, buf: &mut Buffer) {
        if area.height < 3 || area.width < 12 {
            return;
        }
 
        let w = area.width as f64 * 2.0;
        let h = area.height as f64 * 4.0;
 
        let hit_x = w / 8.0;
        let mid_y = h / 2.0;
        let r_small = (h / 2.0 - 4.0).clamp(2.0, 11.0);
        let r_big = r_small * 1.5;

        let circles_per_sec = (w - hit_x) / APPROACH_MS;
 
        let now = self.time();
        let notes = &self.beatmap.notes;
        let next_note = self.next_note.min(notes.len());
 
        Canvas::default()
            .marker(Marker::Braille)
            .x_bounds([0.0, w])
            .y_bounds([0.0, h])
            .paint(|ctx| {
                ctx.draw(&CanvasLine {
                    x1: 0.0,
                    y1: h - 1.0,
                    x2: w,
                    y2: h - 1.0,
                    color: Color::DarkGray,
                });
                ctx.draw(&CanvasLine {
                    x1: 0.0,
                    y1: 1.0,
                    x2: w,
                    y2: 1.0,
                    color: Color::DarkGray,
                });
                ctx.draw(&Circle {
                    x: hit_x,
                    y: mid_y,
                    radius: r_big,
                    color: Color::DarkGray,
                });
 
                ctx.layer();
 
                let visible_until = now + APPROACH_MS + 200.0;
                let mut batch: Vec<(f64, f64, Color)> = Vec::new();
                for note in notes[next_note..].iter() {
                    if note.time as f64 > visible_until {
                        break;
                    }
                    if note.hit {
                        continue;
                    }
                    let x = hit_x + (note.time as f64 - now) * circles_per_sec;
                    let r = if note.big { r_big } else { r_small };
                    if x < -r || x > w + r {
                        continue;
                    }
                    batch.push((x, r, note.color()));
                }

                for (x, r, color) in batch.into_iter().rev() {
                    ctx.draw(&HitCircle {
                        x,
                        y: mid_y,
                        radius: r,
                        color,
                    });
                }
            })
            .render(area, buf);
 

        if let Some(j) = &self.latest_judgement {
            let text = j.label();
            let cell_x = area.left() + area.width / 16;
            let rect = Rect {
                x: cell_x,
                y: area.top() + 1,
                width: (text.len() as u16).min(area.right().saturating_sub(cell_x)),
                height: 1,
            };

            Line::from(text).bold().fg(j.color()).render(rect, buf);
        }

    }
}

struct HitCircle {
    x: f64,
    y: f64,
    radius: f64,
    color: Color,
}
 
impl Shape for HitCircle {
    fn draw(&self, painter: &mut Painter) {
        let r2 = self.radius * self.radius;
        let r = self.radius.ceil() as i32;

        for dy in -r..=r {
            for dx in -r..=r {
                let (fx, fy) = (dx as f64, dy as f64);

                if fx * fx + fy * fy > r2 {
                    continue;
                }

                if let Some((px, py)) = painter.get_point(self.x + fx, self.y + fy) {
                    painter.paint(px, py, self.color);
                }
            }
        }
    }
}

impl Widget for &Game {
    fn render(self, area: Rect, buf: &mut Buffer) {
        let [header, playarea, footer] = Layout::vertical([
            Constraint::Length(1),
            Constraint::Length(9),
            Constraint::Min(1),
        ])
        .areas(area);

        let h = &self.beatmap.header;
        Line::from(format!("{} - {} [{}]", h.artist, h.title, h.diff_name))
            .bold()
            .render(header, buf);

        self.render_playarea(playarea, buf);

        Line::from(format!(
            "{}x    {} misses    esc: exit",
            self.combo, self.misses
        ))
        .render(footer, buf);
    }
}
