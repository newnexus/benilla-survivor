//! The stock settings the survivor-like plays with, written into the client's own files before
//! they are read, so no addon and no client change is needed. A setting already in a file is
//! left as the player set it.
//! - `autoSelfCast`: a friendly spell cast with a creature selected lands on the player.

use std::path::Path;

/// Write them, once the client's state folder is known (from a plugin's `build`: the client reads
/// `config.toml` later, at startup).
pub(crate) fn seed() {
    let Some(dir) = benilla_app::config_dir() else {
        return;
    };
    if let Err(e) = std::fs::create_dir_all(&dir)
        .and_then(|()| seed_cvar(&dir.join("config.toml"), "autoSelfCast", "1"))
    {
        eprintln!("survivor-ui: cannot seed the settings in {}: {e}", dir.display());
    }
}

/// `name = "value"` under `config.toml`'s `[cvars]`, unless the file names it already.
fn seed_cvar(path: &Path, name: &str, value: &str) -> std::io::Result<()> {
    let text = std::fs::read_to_string(path).unwrap_or_default();
    if text.lines().any(|l| l.trim_start().starts_with(name)) {
        return Ok(());
    }
    let line = format!("{name} = \"{value}\"");
    let out = match text.find("[cvars]") {
        Some(at) => {
            let end = at + "[cvars]".len();
            format!("{}\n{line}{}", &text[..end], &text[end..])
        }
        None => format!("{text}\n[cvars]\n{line}\n"),
    };
    std::fs::write(path, out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_cvar_goes_under_its_table_and_is_not_written_twice() {
        let dir = std::env::temp_dir().join(format!("survivor-seed-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let p = dir.join("config.toml");
        std::fs::write(&p, "# head\n[cvars]\ngameTip = \"20\"\n").unwrap();
        seed_cvar(&p, "autoSelfCast", "1").unwrap();
        seed_cvar(&p, "autoSelfCast", "1").unwrap();
        assert_eq!(
            std::fs::read_to_string(&p).unwrap(),
            "# head\n[cvars]\nautoSelfCast = \"1\"\ngameTip = \"20\"\n"
        );
        let s = dir.join("saved-variables.lua");
        seed_saved(&s, "SHOW_MULTI_ACTIONBAR_3", "1").unwrap();
        seed_saved(&s, "SHOW_MULTI_ACTIONBAR_3", "1").unwrap();
        assert_eq!(std::fs::read_to_string(&s).unwrap(), "SHOW_MULTI_ACTIONBAR_3 = 1\n");
        let _ = std::fs::remove_dir_all(&dir);
    }
}
