//! Optional host-level bash wrapping: Firejail / Linux `unshare` for network isolation (layer 7).
//!
//! Does **not** replace Docker mode (Docker is default for `bash` when enabled); this applies to **local** `bash -c`.
//!
//! - `HSM_BASH_ISOLATE=firejail` — prefix `firejail --net=none --quiet --` (install Firejail).
//! - `HSM_BASH_ISOLATE=unshare` — Linux only: `unshare -n -- bash -c ...` (may require privileges).

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BashHostIsolate {
    None,
    FirejailNoNet,
    UnshareNet,
}

fn norm(s: &str) -> String {
    s.trim().to_ascii_lowercase()
}

pub fn bash_host_isolate_from_env() -> BashHostIsolate {
    match std::env::var("HSM_BASH_ISOLATE")
        .map(|v| norm(&v))
        .unwrap_or_default()
        .as_str()
    {
        "firejail" | "firejail_nonet" | "firejail-no-net" => BashHostIsolate::FirejailNoNet,
        "unshare" | "unshare_net" | "unshare-net" => BashHostIsolate::UnshareNet,
        _ => BashHostIsolate::None,
    }
}

/// Build a **blocking** `std::process::Command` for `bash -c command` with optional isolation.
pub fn host_bash_command(command: &str, cwd: Option<&std::path::Path>) -> std::process::Command {
    let iso = bash_host_isolate_from_env();
    let mut c = match iso {
        BashHostIsolate::None => {
            let mut c = std::process::Command::new("bash");
            c.arg("-c").arg(command);
            c
        }
        BashHostIsolate::FirejailNoNet => {
            let mut c = std::process::Command::new("firejail");
            c.args(["--net=none", "--quiet", "--"])
                .arg("bash")
                .arg("-c")
                .arg(command);
            c
        }
        BashHostIsolate::UnshareNet => {
            #[cfg(target_os = "linux")]
            {
                let mut c = std::process::Command::new("unshare");
                c.args(["-n", "--"]).arg("bash").arg("-c").arg(command);
                c
            }
            #[cfg(not(target_os = "linux"))]
            {
                let mut c = std::process::Command::new("bash");
                c.arg("-c").arg(command);
                c
            }
        }
    };
    if let Some(d) = cwd {
        c.current_dir(d);
    }
    crate::tools::subprocess_env::apply_minimal_env_std(&mut c);
    c
}
