//! The survivor-like on Android: the same launcher as `survivor`, as the library the APK's
//! GameActivity loads, with touch controls ([`survivor_touch`]). A phone has no keyboard for the character screen, so a mage is made and
//! entered at once. The game data rides inside the APK (its `assets/Data`) and is unpacked to the
//! app's private files folder on the first launch, since the game reads it as plain files.

use std::ffi::CString;
use std::io::Read;
use std::path::Path;

use benilla_app::BuildId;
use bevy::android::android_activity::AndroidApp;

/// The one character of an Android run.
const NAME: &str = "Survivor";

/// The APK's data folders, each unpacked file for file (the asset manager lists files, not
/// folders).
const DATA_DIRS: [&str; 3] = ["Data", "Data/survivor", "Data/survivor/voice"];

#[bevy::prelude::bevy_main]
fn main() {
    // A panic's call chain in the log (adb logcat), the only place a phone shows it.
    std::env::set_var("RUST_BACKTRACE", "1");
    // Before the server starts: it reads its tables from `Data/survivor` at once.
    let app = bevy::android::ANDROID_APP.get();
    if let Some((app, files)) = app.and_then(|a| Some((a, a.internal_data_path()?))) {
        unpack_data(app, &files);
        std::env::set_var("WOW_DATA", files.join("Data"));
        std::env::set_var("SURVIVOR_DATA", files.join("Data").join("survivor"));
        // Settings beside the data: the default, beside the executable, is a system folder.
        std::env::set_var("BENILLA_HOME", files.join("benilla-config"));
        std::env::set_var("SURVIVOR_SAVE_DIR", files.join("benilla-config").join("survivor-saves"));
    }
    // Read by the server as it starts: its one character.
    std::env::set_var("SURVIVOR_SEED", NAME);
    let server = match survivor_server::start() {
        Ok(s) => s,
        Err(e) => {
            eprintln!("survivor: could not start the local server: {e}");
            return;
        }
    };
    std::env::set_var("WOW_HOST", &server.realmd);
    std::env::set_var("WOW_USER", survivor_server::ACCOUNT);
    std::env::set_var("WOW_PASS", survivor_server::PASSWORD);
    std::env::set_var("WOW_REALM", survivor_server::REALM_NAME);
    std::env::set_var("SURVIVOR_CAST_WHILE_MOVING", "1");
    std::env::set_var("SURVIVOR_RACE", survivor_server::UNDEAD.to_string());
    std::env::set_var("WOW_CHAR", NAME);
    // A phone screen is small: the interface at the top of its range (the PC's is 0.9).
    std::env::set_var("WOW_UI_SCALE", "1.4");
    benilla_app::run_with(
        BuildId {
            version: env!("CARGO_PKG_VERSION"),
            describe: env!("BENILLA_GIT_DESCRIBE"),
            sha: env!("BENILLA_GIT_SHA"),
            short: env!("BENILLA_GIT_SHORT"),
            date: env!("BENILLA_GIT_DATE"),
            profile: env!("BENILLA_PROFILE"),
            project_dir: env!("BENILLA_PROJECT_DIR"),
            ..Default::default()
        },
        |app| {
            app.add_plugins((
                survivor_ui::SurvivorUiPlugin,
                survivor_touch::SurvivorTouchPlugin,
                survivor_touch::SurvivorCameraPlugin,
            ));
        },
    );
}

/// Copy the APK's `Data` into `files`, skipping a file already there at the same size: the
/// first launch pays the copy (a few seconds for the 300 MB archive), later ones none, and an
/// update with new data copies again.
fn unpack_data(app: &AndroidApp, files: &Path) {
    let assets = app.asset_manager();
    for dir in DATA_DIRS {
        let Some(list) = CString::new(dir).ok().and_then(|d| assets.open_dir(&d)) else {
            eprintln!("survivor: the APK has no {dir} folder");
            continue;
        };
        let out_dir = files.join(dir);
        if let Err(e) = std::fs::create_dir_all(&out_dir) {
            eprintln!("survivor: cannot make {}: {e}", out_dir.display());
            return;
        }
        for name in list {
            let name = name.to_string_lossy().into_owned();
            let Some(mut asset) = CString::new(format!("{dir}/{name}"))
                .ok()
                .and_then(|p| assets.open(&p))
            else {
                continue;
            };
            let out = out_dir.join(&name);
            let have = std::fs::metadata(&out).ok().map(|m| m.len());
            if have == Some(asset.length() as u64) {
                continue;
            }
            eprintln!("survivor: unpacking {dir}/{name} ({} bytes)", asset.length());
            // Through a temporary name, so a launch cut short never leaves a short file that
            // the size check would take for whole.
            let part = out_dir.join(format!("{name}.part"));
            let copied = std::fs::File::create(&part)
                .and_then(|mut f| std::io::copy(&mut asset.by_ref(), &mut f))
                .and_then(|_| std::fs::rename(&part, &out));
            if let Err(e) = copied {
                eprintln!("survivor: unpacking {dir}/{name} failed: {e}");
            }
        }
    }
}
