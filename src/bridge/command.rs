use itertools::Itertools;
use std::io::read_to_string;
use std::{
    env,
    path::Path,
    process::{Command as StdCommand, Stdio},
};

use log::{debug, error, warn};
use tokio::process::Command as TokioCommand;

use crate::{cmd_line::CmdLineSettings, settings::*};

pub fn create_nvim_command() -> TokioCommand {
    let mut cmd = build_nvim_cmd();

    debug!("Starting neovim with: {:?}", cmd);

    #[cfg(not(debug_assertions))]
    cmd.stderr(Stdio::piped());

    #[cfg(debug_assertions)]
    cmd.stderr(Stdio::inherit());

    #[cfg(windows)]
    set_windows_creation_flags(&mut cmd);

    cmd
}

#[cfg(target_os = "windows")]
fn set_windows_creation_flags(cmd: &mut TokioCommand) {
    cmd.creation_flags(0x0800_0000); // CREATE_NO_WINDOW
}

fn build_nvim_cmd() -> TokioCommand {
    if let Some(path) = SETTINGS.get::<CmdLineSettings>().neovim_bin {
        if platform_exists(&path) {
            return build_nvim_cmd_with_args(&path);
        } else {
            warn!("NEOVIM_BIN is invalid falling back to first bin in PATH");
        }
    }
    if let Some(path) = platform_which("nvim") {
        build_nvim_cmd_with_args(&path)
    } else {
        error!("nvim not found!");
        std::process::exit(1);
    }
}

// Creates a shell command if needed on this platform (wsl or macos)
fn create_platform_shell_command(command: &str, args: &[&str]) -> Option<StdCommand> {
    if cfg!(target_os = "windows") && SETTINGS.get::<CmdLineSettings>().wsl {
        let mut result = StdCommand::new("wsl");
        result.args(["$SHELL", "-lc"]);
        result.arg(format!("{} {}", command, args.join(" ")));
        #[cfg(windows)]
        std::os::windows::process::CommandExt::creation_flags(
            &mut result,
            winapi::um::winbase::CREATE_NO_WINDOW,
        );

        Some(result)
    } else if cfg!(target_os = "macos") {
        let shell = env::var("SHELL").unwrap_or_else(|_| "/bin/sh".to_string());
        let mut result = StdCommand::new(shell);

        if env::var_os("TERM").is_none() {
            result.arg("-l");
        }
        result.arg("-c");
        result.arg(format!("{} {}", command, args.join(" ")));

        Some(result)
    } else {
        None
    }
}

#[cfg(target_os = "windows")]
fn platform_exists(bin: &str) -> bool {
    // exists command is only on windows
    if let Some(mut exists_command) = create_platform_shell_command("exists", &["-x", bin]) {
        if let Ok(output) = exists_command.output() {
            output.status.success()
        } else {
            error!("Exists failed");
            std::process::exit(1);
        }
    } else {
        Path::new(&bin).exists()
    }
}

#[cfg(not(target_os = "windows"))]
fn platform_exists(bin: &str) -> bool {
    Path::new(&bin).exists()
}

fn platform_which(bin: &str) -> Option<String> {
    if let Some(mut which_command) = create_platform_shell_command("which", &[bin]) {
        debug!("Running which command: {:?}", which_command);
        if let Ok(output) = which_command.output() {
            if output.status.success() {
                let nvim_path = String::from_utf8(output.stdout).unwrap();
                return Some(nvim_path.trim().to_owned());
            } else {
                return None;
            }
        }
    }

    // Platform command failed, fallback to which crate
    if let Ok(path) = which::which(bin) {
        path.into_os_string().into_string().ok()
    } else {
        None
    }
}

#[cfg(target_os = "macos")]
fn nvim_cmd_impl(bin: &str, args: &[String]) -> TokioCommand {
    let shell = env::var("SHELL").unwrap_or_else(|_| "/bin/sh".to_string());
    let mut cmd = TokioCommand::new(shell);
    let args_str = args
        .iter()
        .map(|arg| shlex::quote(arg))
        .collect::<Vec<_>>()
        .join(" ");
    if env::var_os("TERM").is_none() {
        cmd.arg("-l");
    }
    cmd.arg("-c");
    cmd.arg(&format!("{} {}", bin, args_str));
    cmd
}

fn check_wsl_distro(distro: &str) -> bool {
    let child = StdCommand::new("wsl")
        .args(["-l", "-q"])
        .stdout(Stdio::piped())
        .output();

    let Ok(child) = child else {
        return false;
    };

    let utf16 = child
        .stdout
        .chunks(2)
        .map(|bytes| u16::from_ne_bytes([bytes[0], bytes[1]]))
        .collect_vec();

    let Ok(lines) = String::from_utf16(&utf16) else {
        return false;
    };

    lines.lines().any(|line| line == distro)
}

fn nvim_windows_cmd_impl(bin: &str, distro: Option<String>, bin_args: &[String]) -> TokioCommand {
    let mut cmd = TokioCommand::new("wsl");
    let mut args = vec![];

    if let Some(distro) = &distro {
        if check_wsl_distro(distro) {
            args.extend_from_slice(&["-d", distro.as_str()])
        } else {
            warn!("WSL Distro: `{distro}` does not exist, using default instead.")
        }
    }

    args.extend_from_slice(&["$SHELL", "-lc"]);

    let bin_args = format!("{} {}", bin, bin_args.join(" "));

    args.push(bin_args.as_str());

    cmd.args(args);
    cmd
}

#[cfg(not(target_os = "macos"))]
fn nvim_cmd_impl(bin: &str, args: &[String]) -> TokioCommand {
    let settings = SETTINGS.get::<CmdLineSettings>();

    if cfg!(target_os = "windows") && settings.wsl {
        nvim_windows_cmd_impl(bin, settings.wsl_distro, args)
    } else {
        let mut cmd = TokioCommand::new(bin);
        cmd.args(args);
        cmd
    }
}

fn build_nvim_cmd_with_args(bin: &str) -> TokioCommand {
    let mut args = vec!["--embed".to_string()];
    args.extend(SETTINGS.get::<CmdLineSettings>().neovim_args);
    nvim_cmd_impl(bin, &args)
}
