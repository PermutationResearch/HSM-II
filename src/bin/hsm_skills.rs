//! CLI: list or show on-disk SKILL.md entries (same roots as the personal agent).
//!
//! Uses `HSMII_HOME` (or `.`) and `HSM_SKILL_EXTERNAL_DIRS`.
//!
//! ```text
//! cargo run --bin hsm_skills -- list
//! cargo run --bin hsm_skills -- show plan
//! ```

use std::env;
use std::path::PathBuf;

use hyper_stigmergy::skill_markdown::{collect_skill_roots, SkillMdCatalog};

fn main() {
    let home = env::var("HSMII_HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|_| env::current_dir().unwrap_or_else(|_| PathBuf::from(".")));
    let roots = collect_skill_roots(&home);
    let cat = SkillMdCatalog::from_roots(&roots);
    let args: Vec<String> = env::args().collect();
    match args.get(1).map(|s| s.as_str()) {
        None | Some("list") => print!("{}", cat.format_list_markdown(None)),
        Some("show") => {
            let Some(slug) = args.get(2).map(|s| s.as_str()).filter(|s| !s.is_empty()) else {
                eprintln!("usage: hsm_skills show <slug>");
                std::process::exit(2);
            };
            match cat.read_body(slug, 256 * 1024) {
                Ok(b) => print!("{b}"),
                Err(e) => {
                    eprintln!("{e}");
                    std::process::exit(1);
                }
            }
        }
        Some(flag) if flag == "-h" || flag == "--help" => {
            println!(
                "hsm_skills — list or show SKILL.md under HSMII_HOME/skills (+ HSM_SKILL_EXTERNAL_DIRS)\n\
                 \n\
                 {} [list]\n\
                 {} show <slug>\n",
                args.first().map(|s| s.as_str()).unwrap_or("hsm_skills"),
                args.first().map(|s| s.as_str()).unwrap_or("hsm_skills"),
            );
        }
        Some(_) => {
            eprintln!("usage: hsm_skills [list | show <slug> | --help]");
            std::process::exit(2);
        }
    }
}
