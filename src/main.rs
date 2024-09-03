#![windows_subsystem = "windows"]

use windows_registry::{Key, CURRENT_USER};
use std::fs::OpenOptions;
use std::thread::sleep;
use std::time::{Duration, SystemTime};
use serde::{Deserialize, Serialize};
use std::path::Path;
use itertools::Itertools;
use chrono::Timelike;

fn main(){
    run();
}

pub(crate) fn run() -> ! {
    let mut modification_date = last_modification_date(CONFIG_PATH);
    let mut error: Option<String> = None;
    loop {
        if let Err(current_error) = check(|| !modification_date.eq(&last_modification_date(CONFIG_PATH))) {
            if error.is_none() || !error.as_ref().unwrap().eq(&current_error) {
                println!("{current_error}");
                error = Some(current_error);
            }
        }
        while modification_date.eq(&last_modification_date(CONFIG_PATH)) {
            sleep(Duration::from_secs(5));
        }
        modification_date = last_modification_date(CONFIG_PATH);
        println!("Reloading");
    }
}

fn last_modification_date<P: AsRef<Path>>(path: P) -> Option<SystemTime> {
    std::fs::metadata(path).ok().map(|metadata| metadata.modified().ok()).flatten()
}

#[derive(Serialize, Deserialize, Debug)]
enum Mode {
    Dark,
    Night,
    Light,
    Day,
}

#[derive(Serialize, Deserialize, Debug)]
struct Turn {
    mode: Mode,
    hour: u8,
    minute: u8,
}

const fn stamp(hour: u8, minute: u8) -> u16 {
    (hour as u16 * 60) + (minute as u16)
}

const MAX_STAMP: u16 = (24 * 60) + 59;

impl Turn {
    const fn time_past_after_reaching(&self, hour: u8, minute: u8) -> u16 {
        let current_stamp = stamp(self.hour, self.minute);
        let target_stamp = stamp(hour, minute);
        let stamp_distance = (target_stamp as i16) - (current_stamp as i16);
        if stamp_distance >= 0 { stamp_distance as u16 } else { (stamp_distance + (MAX_STAMP as i16 + 1)) as u16 }
    }

    const fn stamp(&self) -> u16 {
        stamp(self.hour, self.minute)
    }
}

const CONFIG_PATH: &'static str = r"./turns.yml";

fn check<StopFN: FnMut() -> bool>(mut stop_loop_when: StopFN) -> Result<(), String> {
    let key = CURRENT_USER.create(r"Software\Microsoft\Windows\CurrentVersion\Themes\Personalize")
        .map_err(|_| r"Could not get Windows key: Software\Microsoft\Windows\CurrentVersion\Themes\Personalize")?;


    let turns_file = OpenOptions::new().read(true).open(CONFIG_PATH)
        .map_err(|error| format!("No turns.yml file found.\n{error}"))?;
    let turns = serde_yaml::from_reader::<_, Vec<Turn>>(turns_file)
        .map_err(|error| format!("The turns.yml file is wrong.\n{error}"))?;

    let wrong_timed_turns = turns.iter()
        .filter(|turn| turn.hour > 24 || turn.minute >= 60)
        .map(|turn| format!("{}:{}", turn.hour, turn.minute))
        .collect::<Vec<_>>();
    if !wrong_timed_turns.is_empty() {
        return Err(format!("The turns.yml contains wrong hour minute formats, being those: {wrong_timed_turns:?}"));
    }

    let turns = turns.into_iter()
        .map(|mut turn| {
            if turn.hour == 24 {
                turn.hour = 0;
            }
            turn
        })
        .unique_by(Turn::stamp)
        .sorted_by_key(Turn::stamp)
        .collect::<Vec<Turn>>();

    if turns.is_empty() {
        return Err("File turns.yml is empty".to_string());
    }


    let (mut current_hour, mut current_minute) = (chrono::offset::Local::now().hour() as u8, chrono::offset::Local::now().minute() as u8);

    let mut current_turn_index = turns.iter().enumerate().min_by_key(|(_, turn)| turn.time_past_after_reaching(current_hour, current_minute))
        .unwrap()
        .0;
    let mut next_turn_index = if current_turn_index < turns.len() - 1 { current_turn_index + 1 } else { 0 };

    println!("With time {current_hour}:{current_minute}, current turn is the one defined at {}:{} with mode {:?}",
             turns[current_turn_index].hour, turns[current_turn_index].minute, turns[current_turn_index].mode);
    set_mode(&key, &turns[current_turn_index].mode);

    loop {
        sleep(Duration::from_secs(1));
        if stop_loop_when() {
            return Ok(());
        }
        if turns.is_empty() {
            continue;
        }
        (current_hour, current_minute) = (chrono::offset::Local::now().hour() as u8, chrono::offset::Local::now().minute() as u8);
        let current_turn = &turns[current_turn_index];
        let next_turn = &turns[next_turn_index];

        if next_turn.time_past_after_reaching(current_hour, current_minute) < current_turn.time_past_after_reaching(current_hour, current_minute) {
            println!("Advancing to turn {}:{} with mode {:?}", next_turn.hour, next_turn.minute, next_turn.mode);
            set_mode(&key, &next_turn.mode);
            current_turn_index = next_turn_index;
            next_turn_index = if current_turn_index < turns.len() - 1 { current_turn_index + 1 } else { 0 };
        }
    }
}

fn set_mode(key: &Key, mode: &Mode) {
    let light_theme_value = match mode {
        Mode::Light | Mode::Day => 1,
        Mode::Night | Mode::Dark => 0
    };
    let _ = key.set_u32("AppsUseLightTheme", light_theme_value).inspect_err(|error| println!("Cannot set {:?} mode due to {error}", mode));
}