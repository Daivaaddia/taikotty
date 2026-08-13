mod beatmap;
mod menu;
mod game;
mod loading;

use std::{error::Error, fs::File, io::{BufRead, BufReader}, path::{Path, PathBuf}, sync::mpsc, thread, time::{Duration, Instant}};

use crossterm::event::{KeyCode, KeyEvent, KeyEventKind};
use kira::{AudioManager, AudioManagerSettings, DefaultBackend, Tween, sound::{FromFileError, static_sound::StaticSoundData, streaming::{StreamingSoundData, StreamingSoundHandle}}};
use ratatui::{DefaultTerminal, Frame, restore, style::{Color}};

use crate::{beatmap::MapHeader, game::Game, loading::Loading, menu::{Menu, scan_songs_dir}};

pub const GREEN:  Color = Color::Rgb(0x39, 0xFF, 0x88); 
pub const YELLOW: Color = Color::Rgb(0xFF, 0xD6, 0x2E); 
pub const BLUE:   Color = Color::Rgb(0x3D, 0x9B, 0xFF); 
pub const RED:    Color = Color::Rgb(0xFF, 0x2D, 0x3A);

pub const VOLUME: f32 = -20.0;

const OPTIONS_PATH: &str = "options.txt";

pub struct HitSounds {
    don: StaticSoundData,
    kat: StaticSoundData,
    miss: StaticSoundData,
}
//TODO
impl HitSounds {
    pub fn load() -> Result<Self, Box<dyn Error>> {
        Ok(Self {
            don: StaticSoundData::from_file("taiko-normal-hitnormal.wav")?.volume(VOLUME),
            kat: StaticSoundData::from_file("taiko-normal-hitclap.wav")?.volume(VOLUME),
            miss: StaticSoundData::from_file("combobreak.wav")?.volume(VOLUME),
        })
    }
}

// TODO support non char
pub struct Options {
    songs_dir: PathBuf,
    centre_left: char,
    centre_right: char,
    rim_left: char,
    rim_right: char,
}

impl Options {
    fn new() -> Self {
        Options {
            songs_dir: PathBuf::new(),
            centre_left: 'f',
            centre_right: 'j',
            rim_left: 'd',
            rim_right: 'k',
        }
    }
}

enum Scene {
    Menu,
    Game(Game),
    Loading
}

struct App {
    exit: bool,
    audio_manager: AudioManager,
    hitsounds: HitSounds,
    scene: Scene,
    menu: Menu,
    loading_map: Option<(PathBuf, MapHeader)>,
    options: Options,
    preview: Preview
}
enum Event {
    Input(crossterm::event::KeyEvent),
    Tick,
}

fn input_thread(tx: mpsc::Sender<Event>) {
    loop {
        match crossterm::event::read().unwrap() {
            crossterm::event::Event::Key(key_event) => tx.send(Event::Input(key_event)).unwrap(),
            _ => {}
        }
    }
}

fn tick_thread(tx: mpsc::Sender<Event>) {
    let mut next = Instant::now();
    loop {
        next += Duration::from_millis(16);
        if tx.send(Event::Tick).is_err() {
            break;
        }
        thread::sleep(next.saturating_duration_since(Instant::now()));
    }
}

impl App {
    fn run(&mut self, terminal: &mut DefaultTerminal, rx: mpsc::Receiver<Event>) -> Result<(), Box<dyn Error>> {
        while !self.exit {
            match rx.recv().unwrap() {
                Event::Input(key_event) => self.handle_key_event(key_event)?,
                Event::Tick => {
                    self.update();
                    terminal.draw(|frame| self.draw(frame))?;
                    self.load_loading_map()?;
                }
            }
        }

        Ok(())
    }

    fn update(&mut self) {
        match &mut self.scene {
            Scene::Menu => {
                let wanted = self.menu.hovered_audio();
                self.preview.poll(&mut self.audio_manager, wanted);
            },
            Scene::Game(game) => {
                if !game.update(&mut self.audio_manager, &self.hitsounds) {
                    self.quit_to_menu();
                }
            },
            _ => {}
        }
    }

    fn draw(&mut self, frame: &mut Frame) {
        let area = frame.area();
        match &self.scene {
            Scene::Menu => frame.render_widget(&mut self.menu, area),
            Scene::Game(game) => frame.render_widget(game, area),
            Scene::Loading => frame.render_widget(Loading, area),
        }
    }

    fn start_map(&mut self, path: PathBuf, header: MapHeader) {
        self.scene = Scene::Loading;
        self.loading_map = Some((path, header));        
    }

    fn load_loading_map(&mut self) -> Result<(), Box<dyn Error>> {
        // this thing is blocking

        if let Some((path, header)) = &self.loading_map {
            self.scene = Scene::Game(Game::new(&mut self.audio_manager, path, header.clone())?);
            self.loading_map = None;
        } 

        Ok(())
    }

    fn handle_key_event(&mut self, key: KeyEvent) -> Result<(), Box<dyn Error>> {
        if key.kind != KeyEventKind::Press {
            return Ok(());
        }

        match self.scene {
            Scene::Menu => self.handle_menu_key(key),
            Scene::Game(_) => self.handle_game_key(key),
            Scene::Loading => Ok(()),
        }
    }

    fn handle_menu_key(&mut self, key: KeyEvent) -> Result<(), Box<dyn Error>> {
        match key.code {
            KeyCode::Char('q') | KeyCode::Esc => self.exit = true,
            KeyCode::Down | KeyCode::Right => self.menu.move_by(1),
            KeyCode::Up | KeyCode::Left => self.menu.move_by(-1),
            KeyCode::Enter => {
                if let Some((path, header)) = self.menu.activate() {
                    self.start_map(path, header);
                    self.preview.stop();
                }
            },
            _ => {}
        }

        Ok(())
    }

    fn handle_game_key(&mut self, key: KeyEvent) -> Result<(), Box<dyn Error>> {
        let Scene::Game(game) = &mut self.scene else {
            return Ok(());
        };

        if key.code == KeyCode::Esc {
            game.stop();
            self.quit_to_menu();
            return Ok(());
        }

        game.handle_key_event(&self.options, &mut self.audio_manager, &self.hitsounds, key)
    }

    pub fn quit_to_menu(&mut self) {
        self.scene = Scene::Menu;
    }
}

fn parse_keybind(value: &str, fallback: char) -> char {
    let mut chars = value.chars();

    match (chars.next(), chars.next()) {
        (Some(c), None) => c.to_ascii_lowercase(),
        _ => fallback,
    }
}

// this assumes valid options.txt
fn parse_options() -> Result<Options, Box<dyn Error>>{
    let reader = BufReader::new(File::open(OPTIONS_PATH)?);

    let mut opts = Options::new();
    
    for line_result in reader.lines() {
        let raw = line_result?;
        let line = raw.trim();
        
        let Some((key, value)) = line.split_once(':') else {
            continue;
        };
        let value = value.trim();

        match key.trim() {
            "Songs Path" => opts.songs_dir = PathBuf::from(value),
            "Drum Center (Left)" => opts.centre_left = parse_keybind(value, 'f'),
            "Drum Center (Right)" => opts.centre_right = parse_keybind(value, 'j'),
            "Drum Rim (Left)" => opts.rim_left = parse_keybind(value, 'd'),
            "Drum Rim (Right)" => opts.rim_right = parse_keybind(value, 'k'),
            _ => {}
        }
    }

    Ok(opts)
}

struct Preview {
    handle: Option<StreamingSoundHandle<FromFileError>>,
    playing: Option<PathBuf>,
}

impl Preview {
    fn new() -> Self {
        Self {
            handle: None,
            playing: None,
        }
    }

    fn poll(&mut self, manager: &mut AudioManager, wanted: Option<(&Path, u32)>) {
        if let Some(h) = &mut self.handle {
            if h.pop_error().is_some() {
                self.stop();
            }
        }

        let Some((path, preview_ms)) = wanted else {
            return;
        };

        if self.playing == Some(path.to_path_buf()) {
            return;
        }

        self.stop();
        self.playing = Some(path.to_path_buf());

        let Ok(data) = StreamingSoundData::from_file(path) else {
            return;
        };

        let start = preview_ms as f64 / 1000.0;
        let data = data.start_position(start).loop_region(start..).volume(VOLUME);

        self.handle = manager.play(data).ok();
    }

    fn stop(&mut self) {
        if let Some(h) = &mut self.handle {
            h.stop(Tween::default());
        }
        self.handle = None;
        self.playing = None;
    }
}

fn main() -> Result<(), Box<dyn Error>> {
    let options = parse_options()?;

    let songs = scan_songs_dir(Path::new(&options.songs_dir))?;
    if songs.is_empty() {
        return Err(format!("No maps found").into());
    }

    let audio_manager = AudioManager::<DefaultBackend>::new(AudioManagerSettings::default())?;
    let hitsounds = HitSounds::load()?;

    let (event_tx, event_rx) = mpsc::channel::<Event>();

    let mut app = App {
        exit: false,
        audio_manager,
        hitsounds,
        menu: Menu::new(songs),
        scene: Scene::Menu,
        loading_map: None,
        options,
        preview: Preview::new(),
    };

    let input_tx = event_tx.clone();
    thread::spawn(move || {
        input_thread(input_tx);
    });

    let tick_tx = event_tx.clone();
    thread::spawn(move || {
        tick_thread(tick_tx);
    });

    let mut terminal = ratatui::init();
    let app_result = app.run(&mut terminal, event_rx);

    restore();
    app_result
}
