//! The survivor-like's launcher: starts the pretend server on loopback, points the client at it
//! and logs straight in, then runs the ordinary benilla client.
//!
//! `$WOW_HOST` is always overwritten, so this binary can never dial a public server.

use benilla_app::BuildId;

fn main() -> benilla_app::AppExit {
    let server = match survivor_server::start() {
        Ok(s) => s,
        Err(e) => {
            eprintln!("survivor: could not start the local server: {e}");
            std::process::exit(1);
        }
    };
    // Set before the client reads them, and before it starts any thread.
    std::env::set_var("WOW_HOST", &server.realmd);
    std::env::set_var("WOW_USER", survivor_server::ACCOUNT);
    std::env::set_var("WOW_PASS", survivor_server::PASSWORD);
    // Straight to the one realm: no realm list.
    std::env::set_var("WOW_REALM", survivor_server::REALM_NAME);
    // Casters cast on the run: no moving refusal, no move cancel.
    std::env::set_var("SURVIVOR_CAST_WHILE_MOVING", "1");
    // The character screen offers the Forsaken alone.
    std::env::set_var("SURVIVOR_RACE", survivor_server::UNDEAD.to_string());
    benilla_app::run_with(BuildId {
        version: env!("CARGO_PKG_VERSION"),
        describe: env!("BENILLA_GIT_DESCRIBE"),
        sha: env!("BENILLA_GIT_SHA"),
        short: env!("BENILLA_GIT_SHORT"),
        date: env!("BENILLA_GIT_DATE"),
        profile: env!("BENILLA_PROFILE"),
        project_dir: env!("BENILLA_PROJECT_DIR"),
        ..Default::default()
    }, |app| {
        // The survivor's own screens: the level-up pick.
        app.add_plugins(survivor_ui::SurvivorUiPlugin);
    })
}
