use a704f_macos::macros::MacroFile;
use a704f_macos::protocol::Settings;
use clap::{Parser, Subcommand};
use std::fs;
use std::path::PathBuf;

#[cfg(target_os = "macos")]
use a704f_macos::protocol::DeviceIdentity;
#[cfg(target_os = "macos")]
use a704f_macos::transport::{apply_settings, ApplyResult};

#[derive(Debug, Parser)]
#[command(
    name = "a704f",
    version,
    about = "Instant A704F macOS configuration CLI"
)]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Debug, Subcommand)]
enum Command {
    Device {
        #[command(subcommand)]
        command: DeviceCommand,
    },
    Settings {
        #[command(subcommand)]
        command: SettingsCommand,
    },
    Macro {
        #[command(subcommand)]
        command: MacroCommand,
    },
}

#[derive(Debug, Subcommand)]
enum DeviceCommand {
    List,
    Inspect,
}

#[derive(Debug, Subcommand)]
enum SettingsCommand {
    Plan {
        #[arg(short, long)]
        file: PathBuf,
    },
    Apply {
        #[arg(short, long)]
        file: PathBuf,
        #[arg(long, help = "Explicitly write settings to the connected A704F")]
        write: bool,
    },
    Export {
        #[arg(short, long)]
        file: PathBuf,
    },
}

#[derive(Debug, Subcommand)]
enum MacroCommand {
    Validate {
        #[arg(short, long)]
        file: PathBuf,
    },
    Export {
        #[arg(short, long)]
        file: PathBuf,
    },
    Run {
        #[arg(short, long)]
        file: PathBuf,
        #[arg(
            long,
            default_value = "F12",
            help = "Standalone stop key; Ctrl+C also stops"
        )]
        stop_key: String,
    },
}

fn main() {
    if let Err(error) = run(Cli::parse()) {
        eprintln!("error: {error}");
        std::process::exit(1);
    }
}

fn run(cli: Cli) -> Result<(), String> {
    match cli.command {
        Command::Device { command } => device_command(command),
        Command::Settings { command } => settings_command(command),
        Command::Macro { command } => macro_command(command),
    }
}

fn device_command(command: DeviceCommand) -> Result<(), String> {
    match command {
        DeviceCommand::List | DeviceCommand::Inspect => inspect_devices(),
    }
}

#[cfg(target_os = "macos")]
fn inspect_devices() -> Result<(), String> {
    let devices = a704f_macos::transport::macos::discover()?;
    if devices.is_empty() {
        println!("No known A704F-family HID interfaces found.");
        return Ok(());
    }
    for device in devices {
        let writes_supported =
            device.identity.vendor_id == 0x18f8 && device.identity.product_id == 0x1286;
        println!(
            "{:04x}:{:04x} interface={} manufacturer={:?} product={:?} usage={:04x}:{:04x} writes_supported={}",
            device.identity.vendor_id,
            device.identity.product_id,
            device.interface,
            device.manufacturer,
            device.product,
            device.usage_page,
            device.usage,
            writes_supported
        );
    }
    println!("Inspection is read-only; this protocol documents no settings readback command.");
    Ok(())
}

#[cfg(not(target_os = "macos"))]
fn inspect_devices() -> Result<(), String> {
    Err(
        "HID discovery is available only in a macOS build; protocol and macro tests run on Linux"
            .into(),
    )
}

fn settings_command(command: SettingsCommand) -> Result<(), String> {
    match command {
        SettingsCommand::Plan { file } => {
            let settings = load_settings(&file)?;
            print_plan(&settings)
        }
        SettingsCommand::Apply { file, write } => {
            let settings = load_settings(&file)?;
            if !write {
                println!("Dry run only. No device was opened and no reports were sent.");
                return print_plan(&settings);
            }
            apply_to_device(&settings)
        }
        SettingsCommand::Export { file } => {
            let json = serde_json::to_string_pretty(&Settings::default())
                .map_err(|error| error.to_string())?;
            fs::write(&file, format!("{json}\n"))
                .map_err(|error| format!("cannot write {}: {error}", file.display()))?;
            println!("Wrote settings template to {}", file.display());
            Ok(())
        }
    }
}

fn load_settings(path: &PathBuf) -> Result<Settings, String> {
    let source = fs::read_to_string(path)
        .map_err(|error| format!("cannot read {}: {error}", path.display()))?;
    let settings: Settings =
        serde_json::from_str(&source).map_err(|error| format!("invalid settings JSON: {error}"))?;
    settings.reports().map_err(|error| error.to_string())?;
    Ok(settings)
}

fn print_plan(settings: &Settings) -> Result<(), String> {
    let reports = settings.reports().map_err(|error| error.to_string())?;
    println!(
        "Dry-run plan: {} documented feature reports; nothing has been sent.",
        reports.len()
    );
    for (index, report) in reports.iter().enumerate() {
        let bytes = report.encode();
        let hex = bytes
            .iter()
            .map(|byte| format!("{byte:02x}"))
            .collect::<Vec<_>>()
            .join(" ");
        println!("{:02}: {hex}", index + 1);
    }
    Ok(())
}

#[cfg(target_os = "macos")]
fn apply_to_device(settings: &Settings) -> Result<(), String> {
    let mut device = a704f_macos::transport::macos::ConfigDevice::open()?;
    let identity = DeviceIdentity {
        vendor_id: 0x18f8,
        product_id: 0x1286,
    };
    match apply_settings(&mut device, identity, settings, true)
        .map_err(|error| error.to_string())?
    {
        ApplyResult::Written(count) => println!("Sent {count} feature reports to 18f8:1286."),
        ApplyResult::DryRun(_) => unreachable!(),
    }
    println!("No readback is available to confirm device acceptance.");
    Ok(())
}

#[cfg(not(target_os = "macos"))]
fn apply_to_device(_settings: &Settings) -> Result<(), String> {
    Err("hardware writes require a macOS build connected to 18f8:1286".into())
}

fn macro_command(command: MacroCommand) -> Result<(), String> {
    match command {
        MacroCommand::Validate { file } => {
            let macro_file = load_macro(&file)?;
            macro_file.validate().map_err(|error| error.to_string())?;
            println!("Valid version 1 macro: {} steps", macro_file.steps.len());
            Ok(())
        }
        MacroCommand::Export { file } => {
            let macro_file = MacroFile {
                version: 1,
                trigger: a704f_macos::macros::Trigger::Hotkey {
                    key: "KeyF".into(),
                    modifiers: vec!["ControlLeft".into(), "ShiftLeft".into()],
                },
                steps: vec![
                    a704f_macos::macros::Step::Key {
                        key: "KeyA".into(),
                        pressed: true,
                    },
                    a704f_macos::macros::Step::Delay { milliseconds: 50 },
                    a704f_macos::macros::Step::Key {
                        key: "KeyA".into(),
                        pressed: false,
                    },
                ],
            };
            macro_file.validate().map_err(|error| error.to_string())?;
            let json =
                serde_json::to_string_pretty(&macro_file).map_err(|error| error.to_string())?;
            fs::write(&file, format!("{json}\n"))
                .map_err(|error| format!("cannot write {}: {error}", file.display()))?;
            println!("Wrote macro template to {}", file.display());
            Ok(())
        }
        MacroCommand::Run { file, stop_key } => run_macro(file, stop_key),
    }
}

fn load_macro(path: &PathBuf) -> Result<MacroFile, String> {
    let source = fs::read_to_string(path)
        .map_err(|error| format!("cannot read {}: {error}", path.display()))?;
    let macro_file: MacroFile =
        serde_json::from_str(&source).map_err(|error| format!("invalid macro JSON: {error}"))?;
    macro_file.validate().map_err(|error| error.to_string())?;
    Ok(macro_file)
}

#[cfg(target_os = "macos")]
fn run_macro(path: PathBuf, stop_key: String) -> Result<(), String> {
    let macro_file = load_macro(&path)?;
    a704f_macos::runtime_macos::run(macro_file, stop_key)
}

#[cfg(not(target_os = "macos"))]
fn run_macro(_path: PathBuf, _stop_key: String) -> Result<(), String> {
    Err("host macro input monitoring and injection require a macOS build; validation works on Linux".into())
}
