use serde::{Deserialize, Serialize};
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;
use thiserror::Error;

const MAX_STEPS: usize = 1024;
const MAX_DELAY_MS: u64 = 60_000;

#[derive(Clone, Debug, Deserialize, PartialEq, Eq, Serialize)]
#[serde(tag = "type", rename_all = "snake_case", deny_unknown_fields)]
pub enum Trigger {
    MouseButton { button: u8 },
    Hotkey { key: String, modifiers: Vec<String> },
}

#[derive(Clone, Debug, Deserialize, PartialEq, Eq, Serialize)]
#[serde(tag = "type", rename_all = "snake_case", deny_unknown_fields)]
pub enum Step {
    Key { key: String, pressed: bool },
    Mouse { button: String, pressed: bool },
    Delay { milliseconds: u64 },
}

#[derive(Clone, Debug, Deserialize, PartialEq, Eq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct MacroFile {
    pub version: u8,
    pub trigger: Trigger,
    pub steps: Vec<Step>,
}

#[derive(Debug, Error, PartialEq, Eq)]
pub enum MacroError {
    #[error("unsupported macro format version {0}; expected 1")]
    UnsupportedVersion(u8),
    #[error("macro must contain between 1 and {MAX_STEPS} steps")]
    InvalidStepCount,
    #[error("invalid trigger: {0}")]
    InvalidTrigger(&'static str),
    #[error("invalid step at index {0}")]
    InvalidStep(usize),
    #[error("macro action failed: {0}")]
    Runtime(String),
}

impl MacroFile {
    pub fn validate(&self) -> Result<(), MacroError> {
        if self.version != 1 {
            return Err(MacroError::UnsupportedVersion(self.version));
        }
        if self.steps.is_empty() || self.steps.len() > MAX_STEPS {
            return Err(MacroError::InvalidStepCount);
        }
        match &self.trigger {
            Trigger::MouseButton { button } if !matches!(button, 1..=3) => {
                return Err(MacroError::InvalidTrigger(
                    "host triggers currently support only standard left, right, and middle buttons",
                ));
            }
            Trigger::Hotkey { key, modifiers }
                if !valid_key(key) || modifiers.iter().any(|modifier| !valid_key(modifier)) =>
            {
                return Err(MacroError::InvalidTrigger(
                    "hotkey contains an unsupported key identifier",
                ));
            }
            _ => {}
        }
        for (index, step) in self.steps.iter().enumerate() {
            let valid = match step {
                Step::Key { key, .. } => valid_key(key),
                Step::Mouse { button, .. } => {
                    matches!(button.as_str(), "left" | "middle" | "right")
                }
                Step::Delay { milliseconds } => *milliseconds <= MAX_DELAY_MS,
            };
            if !valid {
                return Err(MacroError::InvalidStep(index));
            }
        }
        Ok(())
    }
}

fn valid_key(name: &str) -> bool {
    let single_suffix = |prefix: &str| {
        name.strip_prefix(prefix)
            .is_some_and(|suffix| suffix.len() == 1 && suffix.as_bytes()[0].is_ascii_uppercase())
    };
    let digit_suffix = |prefix: &str| {
        name.strip_prefix(prefix)
            .is_some_and(|suffix| suffix.len() == 1 && suffix.as_bytes()[0].is_ascii_digit())
    };
    if single_suffix("Key") || digit_suffix("Num") {
        return true;
    }
    if let Some(number) = name
        .strip_prefix('F')
        .and_then(|suffix| suffix.parse::<u8>().ok())
    {
        return (1..=12).contains(&number) && name == format!("F{number}");
    }
    matches!(
        name,
        "Alt"
            | "Backspace"
            | "CapsLock"
            | "ControlLeft"
            | "ControlRight"
            | "Delete"
            | "DownArrow"
            | "End"
            | "Escape"
            | "F1"
            | "F2"
            | "F3"
            | "F4"
            | "F5"
            | "F6"
            | "F7"
            | "F8"
            | "F9"
            | "F10"
            | "F11"
            | "F12"
            | "Home"
            | "LeftArrow"
            | "MetaLeft"
            | "MetaRight"
            | "PageDown"
            | "PageUp"
            | "Return"
            | "RightArrow"
            | "ShiftLeft"
            | "ShiftRight"
            | "Space"
            | "Tab"
            | "UpArrow"
    )
}

pub trait MacroRuntime {
    fn key(&mut self, key: &str, pressed: bool) -> Result<(), String>;
    fn mouse(&mut self, button: &str, pressed: bool) -> Result<(), String>;
    fn delay(&mut self, duration: Duration, cancelled: &AtomicBool);
}

pub fn execute(
    macro_file: &MacroFile,
    runtime: &mut impl MacroRuntime,
    cancelled: &AtomicBool,
) -> Result<(), MacroError> {
    macro_file.validate()?;
    let mut held_keys: Vec<String> = Vec::new();
    let mut held_buttons: Vec<String> = Vec::new();
    for step in &macro_file.steps {
        if cancelled.load(Ordering::Relaxed) {
            release_all(runtime, &mut held_keys, &mut held_buttons);
            return Ok(());
        }
        let result = match step {
            Step::Key { key, pressed } => {
                let result = runtime.key(key, *pressed);
                if result.is_ok() {
                    update_held(&mut held_keys, key, *pressed);
                }
                result
            }
            Step::Mouse { button, pressed } => {
                let result = runtime.mouse(button, *pressed);
                if result.is_ok() {
                    update_held(&mut held_buttons, button, *pressed);
                }
                result
            }
            Step::Delay { milliseconds } => {
                runtime.delay(Duration::from_millis(*milliseconds), cancelled);
                Ok(())
            }
        };
        if let Err(error) = result {
            release_all(runtime, &mut held_keys, &mut held_buttons);
            return Err(MacroError::Runtime(error));
        }
    }
    release_all(runtime, &mut held_keys, &mut held_buttons);
    Ok(())
}

fn update_held(held: &mut Vec<String>, name: &str, pressed: bool) {
    if pressed && !held.iter().any(|entry| entry == name) {
        held.push(name.to_owned());
    } else if !pressed {
        held.retain(|entry| entry != name);
    }
}

fn release_all(runtime: &mut impl MacroRuntime, keys: &mut Vec<String>, buttons: &mut Vec<String>) {
    for key in keys.drain(..).rev() {
        let _ = runtime.key(&key, false);
    }
    for button in buttons.drain(..).rev() {
        let _ = runtime.mouse(&button, false);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::AtomicBool;

    #[derive(Default)]
    struct FakeRuntime {
        events: Vec<String>,
        elapsed: Duration,
        cancel_during_delay: bool,
    }

    impl MacroRuntime for FakeRuntime {
        fn key(&mut self, key: &str, pressed: bool) -> Result<(), String> {
            self.events.push(format!("key:{key}:{pressed}"));
            Ok(())
        }

        fn mouse(&mut self, button: &str, pressed: bool) -> Result<(), String> {
            self.events.push(format!("mouse:{button}:{pressed}"));
            Ok(())
        }

        fn delay(&mut self, duration: Duration, cancelled: &AtomicBool) {
            self.elapsed += duration;
            if self.cancel_during_delay {
                cancelled.store(true, Ordering::Relaxed);
            }
        }
    }

    fn sample() -> MacroFile {
        MacroFile {
            version: 1,
            trigger: Trigger::MouseButton { button: 3 },
            steps: vec![
                Step::Key {
                    key: "KeyA".into(),
                    pressed: true,
                },
                Step::Delay { milliseconds: 75 },
                Step::Key {
                    key: "KeyA".into(),
                    pressed: false,
                },
                Step::Mouse {
                    button: "left".into(),
                    pressed: true,
                },
            ],
        }
    }

    #[test]
    fn executes_steps_in_order_with_requested_timing() {
        let mut runtime = FakeRuntime::default();
        execute(&sample(), &mut runtime, &AtomicBool::new(false)).unwrap();
        assert_eq!(
            runtime.events,
            [
                "key:KeyA:true",
                "key:KeyA:false",
                "mouse:left:true",
                "mouse:left:false"
            ]
        );
        assert_eq!(runtime.elapsed, Duration::from_millis(75));
    }

    #[test]
    fn cancellation_stops_before_the_next_action() {
        let mut runtime = FakeRuntime {
            cancel_during_delay: true,
            ..FakeRuntime::default()
        };
        execute(&sample(), &mut runtime, &AtomicBool::new(false)).unwrap();
        assert_eq!(runtime.events, ["key:KeyA:true", "key:KeyA:false"]);
        assert_eq!(runtime.elapsed, Duration::from_millis(75));
    }

    #[test]
    fn rejects_unknown_versions_and_malformed_actions() {
        let mut macro_file = sample();
        macro_file.version = 2;
        assert_eq!(
            macro_file.validate(),
            Err(MacroError::UnsupportedVersion(2))
        );

        let mut macro_file = sample();
        macro_file.steps[1] = Step::Delay {
            milliseconds: MAX_DELAY_MS + 1,
        };
        assert_eq!(macro_file.validate(), Err(MacroError::InvalidStep(1)));

        let mut macro_file = sample();
        macro_file.steps[0] = Step::Key {
            key: "NotAKey".into(),
            pressed: true,
        };
        assert_eq!(macro_file.validate(), Err(MacroError::InvalidStep(0)));

        assert!(serde_json::from_str::<MacroFile>(
            r#"{"version":1,"trigger":{"type":"mouse_button","button":1},"steps":[{"type":"delay","milliseconds":1,"extra":true}]}"#
        )
        .is_err());
    }

    #[test]
    fn json_round_trip_preserves_configuration() {
        let macro_file = sample();
        let json = serde_json::to_string(&macro_file).unwrap();
        let decoded: MacroFile = serde_json::from_str(&json).unwrap();
        assert_eq!(decoded, macro_file);
        decoded.validate().unwrap();
    }
}
