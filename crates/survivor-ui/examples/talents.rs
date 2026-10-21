//! `cargo run -p survivor-ui --example talents -- <out dir>`: the Forsaken classes' talents and
//! their icons, for the talent-tagging page.

fn main() -> anyhow::Result<()> {
    let out = std::env::args().nth(1).unwrap_or_else(|| "talents-out".to_owned());
    survivor_ui::export_talents(std::path::Path::new(&out))
}
