//! Rust development tasks; excluded from the production binary and image.
#[path = "xtask/mod.rs"]
mod tasks;

fn main() -> anyhow::Result<()> {
    tasks::run()
}
