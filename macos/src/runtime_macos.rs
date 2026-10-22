use crate::macros::{execute, MacroFile, MacroRuntime, Trigger};
use rdev::{listen, simulate, Button, Event, EventType, Key};
use std::collections::HashSet;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::Duration;

struct SystemRuntime;

impl MacroRuntime for SystemRuntime {
    fn key(&mut self, name: &str, pressed: bool) -> Result<(), String> {
        let key = parse_key(name).ok_or_else(|| format!("unsupported key identifier: {name}"))?;
        let event = if pressed {
            EventType::KeyPress(key)
        } else {
            EventType::KeyRelease(key)
        };
        simulate(&event).map_err(|error| error.to_string())?;
        thread::sleep(Duration::from_millis(10));
        Ok(())
    }

    fn mouse(&mut self, name: &str, pressed: bool) -> Result<(), String> {
        let button = match name {
            "left" => Button::Left,
            "right" => Button::Right,
            "middle" => Button::Middle,
            _ => return Err(format!("unsupported mouse button: {name}")),
        };
        let event = if pressed {
            EventType::ButtonPress(button)
        } else {
            EventType::ButtonRelease(button)
        };
        simulate(&event).map_err(|error| error.to_string())?;
        thread::sleep(Duration::from_millis(10));
        Ok(())
    }

    fn delay(&mut self, duration: Duration, cancelled: &AtomicBool) {
        let deadline = std::time::Instant::now() + duration;
        while !cancelled.load(Ordering::Relaxed) {
            let remaining = deadline.saturating_duration_since(std::time::Instant::now());
            if remaining.is_zero() {
                break;
            }
            thread::sleep(remaining.min(Duration::from_millis(5)));
        }
    }
}

pub fn run(macro_file: MacroFile, stop_key: String) -> Result<(), String> {
    macro_file.validate().map_err(|error| error.to_string())?;
    if let Trigger::Hotkey { key, modifiers } = &macro_file.trigger {
        if parse_key(key).is_none()
            || modifiers
                .iter()
                .any(|modifier| parse_key(modifier).is_none())
        {
            return Err("hotkey contains a key identifier unsupported by the macOS runtime".into());
        }
    }
    for step in &macro_file.steps {
        if let crate::macros::Step::Key { key, .. } = step {
            if parse_key(key).is_none() {
                return Err(format!("unsupported key identifier: {key}"));
            }
        }
    }
    if parse_key(&stop_key).is_none() {
        return Err(format!("unsupported stop key identifier: {stop_key}"));
    }

    let stopping = Arc::new(AtomicBool::new(false));
    let cancelled = Arc::new(AtomicBool::new(false));
    let running = Arc::new(AtomicBool::new(false));
    let pressed_keys = Arc::new(Mutex::new(HashSet::<String>::new()));
    let listener_error = Arc::new(Mutex::new(None::<String>));
    let worker_error = Arc::new(Mutex::new(None::<String>));

    {
        let stopping = Arc::clone(&stopping);
        let cancelled = Arc::clone(&cancelled);
        ctrlc::set_handler(move || {
            cancelled.store(true, Ordering::Relaxed);
            stopping.store(true, Ordering::Relaxed);
        })
        .map_err(|error| format!("cannot install Ctrl+C handler: {error}"))?;
    }

    let listener_file = macro_file.clone();
    let listener_stop_key = stop_key.clone();
    let listener_stopping = Arc::clone(&stopping);
    let listener_cancelled = Arc::clone(&cancelled);
    let listener_running = Arc::clone(&running);
    let listener_pressed_keys = Arc::clone(&pressed_keys);
    let listener_error_slot = Arc::clone(&listener_error);
    let worker_error_slot = Arc::clone(&worker_error);

    thread::Builder::new()
        .name("a704f-input-listener".to_owned())
        .spawn(move || {
            let listener_stopping_on_error = Arc::clone(&listener_stopping);
            let listener_cancelled_on_error = Arc::clone(&listener_cancelled);
            let result = listen(move |event| {
                if listener_stopping.load(Ordering::Relaxed) {
                    return;
                }
                let trigger = event_trigger(
                    &event,
                    &listener_file,
                    &listener_stop_key,
                    &listener_pressed_keys,
                    &listener_stopping,
                    &listener_cancelled,
                );
                if !trigger || listener_stopping.load(Ordering::Relaxed) {
                    return;
                }
                if listener_running
                    .compare_exchange(false, true, Ordering::AcqRel, Ordering::Relaxed)
                    .is_err()
                {
                    return;
                }
                listener_cancelled.store(false, Ordering::Relaxed);
                let file = listener_file.clone();
                let cancelled = Arc::clone(&listener_cancelled);
                let running = Arc::clone(&listener_running);
                let error_slot = Arc::clone(&worker_error_slot);
                let spawned =
                    thread::Builder::new()
                        .name("a704f-macro".to_owned())
                        .spawn(move || {
                            let mut runtime = SystemRuntime;
                            if let Err(error) = execute(&file, &mut runtime, &cancelled) {
                                *error_slot.lock().unwrap() = Some(error.to_string());
                            }
                            running.store(false, Ordering::Release);
                        });
                if let Err(error) = spawned {
                    *worker_error_slot.lock().unwrap() = Some(error.to_string());
                    listener_running.store(false, Ordering::Release);
                }
            });
            if let Err(error) = result {
                *listener_error_slot.lock().unwrap() =
                    Some(format!("input listener failed: {error:?}"));
                listener_cancelled_on_error.store(true, Ordering::Relaxed);
                listener_stopping_on_error.store(true, Ordering::Relaxed);
            }
        })
        .map_err(|error| format!("cannot start input listener: {error}"))?;

    println!("Macro listener running. Press {stop_key} or Ctrl+C to stop.");
    println!("macOS Accessibility permission is required; Input Monitoring may also be requested by macOS.");
    while !stopping.load(Ordering::Relaxed) {
        if let Some(error) = listener_error.lock().unwrap().take() {
            stopping.store(true, Ordering::Relaxed);
            cancelled.store(true, Ordering::Relaxed);
            while running.load(Ordering::Acquire) {
                thread::sleep(Duration::from_millis(5));
            }
            return Err(error);
        }
        if let Some(error) = worker_error.lock().unwrap().take() {
            stopping.store(true, Ordering::Relaxed);
            cancelled.store(true, Ordering::Relaxed);
            while running.load(Ordering::Acquire) {
                thread::sleep(Duration::from_millis(5));
            }
            return Err(error);
        }
        thread::sleep(Duration::from_millis(10));
    }
    cancelled.store(true, Ordering::Relaxed);
    while running.load(Ordering::Acquire) {
        thread::sleep(Duration::from_millis(5));
    }
    println!("Macro listener stopped; active inputs were released.");
    Ok(())
}

fn event_trigger(
    event: &Event,
    macro_file: &MacroFile,
    stop_key: &str,
    pressed_keys: &Mutex<HashSet<String>>,
    stopping: &AtomicBool,
    cancelled: &AtomicBool,
) -> bool {
    match event.event_type {
        EventType::KeyPress(key) => {
            let name = format!("{key:?}");
            if name == stop_key {
                cancelled.store(true, Ordering::Relaxed);
                stopping.store(true, Ordering::Relaxed);
                return false;
            }
            let mut pressed = pressed_keys.lock().unwrap();
            pressed.insert(name.clone());
            match &macro_file.trigger {
                Trigger::Hotkey { key, modifiers } => {
                    name == *key && modifiers.iter().all(|modifier| pressed.contains(modifier))
                }
                _ => false,
            }
        }
        EventType::KeyRelease(key) => {
            pressed_keys.lock().unwrap().remove(&format!("{key:?}"));
            false
        }
        EventType::ButtonPress(button) => {
            let number = match button {
                Button::Left => 1,
                Button::Right => 2,
                Button::Middle => 3,
                Button::Unknown(_) => return false,
            };
            matches!(&macro_file.trigger, Trigger::MouseButton { button } if *button == number)
        }
        _ => false,
    }
}

fn parse_key(name: &str) -> Option<Key> {
    Some(match name {
        "Alt" => Key::Alt,
        "AltGr" => Key::AltGr,
        "Backspace" => Key::Backspace,
        "CapsLock" => Key::CapsLock,
        "ControlLeft" => Key::ControlLeft,
        "ControlRight" => Key::ControlRight,
        "Delete" => Key::Delete,
        "DownArrow" => Key::DownArrow,
        "End" => Key::End,
        "Escape" => Key::Escape,
        "F1" => Key::F1,
        "F2" => Key::F2,
        "F3" => Key::F3,
        "F4" => Key::F4,
        "F5" => Key::F5,
        "F6" => Key::F6,
        "F7" => Key::F7,
        "F8" => Key::F8,
        "F9" => Key::F9,
        "F10" => Key::F10,
        "F11" => Key::F11,
        "F12" => Key::F12,
        "Home" => Key::Home,
        "LeftArrow" => Key::LeftArrow,
        "MetaLeft" => Key::MetaLeft,
        "MetaRight" => Key::MetaRight,
        "PageDown" => Key::PageDown,
        "PageUp" => Key::PageUp,
        "Return" => Key::Return,
        "RightArrow" => Key::RightArrow,
        "ShiftLeft" => Key::ShiftLeft,
        "ShiftRight" => Key::ShiftRight,
        "Space" => Key::Space,
        "Tab" => Key::Tab,
        "UpArrow" => Key::UpArrow,
        "Num0" => Key::Num0,
        "Num1" => Key::Num1,
        "Num2" => Key::Num2,
        "Num3" => Key::Num3,
        "Num4" => Key::Num4,
        "Num5" => Key::Num5,
        "Num6" => Key::Num6,
        "Num7" => Key::Num7,
        "Num8" => Key::Num8,
        "Num9" => Key::Num9,
        "KeyA" => Key::KeyA,
        "KeyB" => Key::KeyB,
        "KeyC" => Key::KeyC,
        "KeyD" => Key::KeyD,
        "KeyE" => Key::KeyE,
        "KeyF" => Key::KeyF,
        "KeyG" => Key::KeyG,
        "KeyH" => Key::KeyH,
        "KeyI" => Key::KeyI,
        "KeyJ" => Key::KeyJ,
        "KeyK" => Key::KeyK,
        "KeyL" => Key::KeyL,
        "KeyM" => Key::KeyM,
        "KeyN" => Key::KeyN,
        "KeyO" => Key::KeyO,
        "KeyP" => Key::KeyP,
        "KeyQ" => Key::KeyQ,
        "KeyR" => Key::KeyR,
        "KeyS" => Key::KeyS,
        "KeyT" => Key::KeyT,
        "KeyU" => Key::KeyU,
        "KeyV" => Key::KeyV,
        "KeyW" => Key::KeyW,
        "KeyX" => Key::KeyX,
        "KeyY" => Key::KeyY,
        "KeyZ" => Key::KeyZ,
        _ => return None,
    })
}
