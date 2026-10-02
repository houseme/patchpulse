mod cache;
mod http;
mod hub_smoke;
mod metadata;
mod policy;
mod smoke;

use clap::{Parser, Subcommand};
use std::path::{Path, PathBuf};

#[derive(Parser)]
#[command(
    name = "patchpulse-xtask",
    about = "Repository dependency and Docker verification tasks"
)]
struct Cli {
    #[arg(long, global = true)]
    root: Option<PathBuf>,
    #[command(subcommand)]
    command: Task,
}

#[derive(Subcommand)]
enum Task {
    /// Check dependency bans and license notices, or regenerate their documents.
    DependencyPolicy {
        #[arg(long)]
        write: bool,
        #[arg(long)]
        metadata: Option<PathBuf>,
    },
    /// Prepare public locked registry archives for an offline Docker build.
    PrepareDockerCache,
    /// Verify a Linux image using a temporary hardened container on loopback.
    SmokeDocker {
        #[arg(long, default_value = "patchpulse:0.1.0")]
        image: String,
        #[arg(long, default_value = "target/docker-smoke.json")]
        report: PathBuf,
    },
    /// Verify configured agent/hub communication in a temporary Docker network.
    SmokeHubDocker {
        #[arg(long, default_value = "patchpulse:0.1.0")]
        image: String,
        #[arg(long, default_value = "target/docker-hub-smoke.json")]
        report: PathBuf,
    },
}

pub(super) fn run() -> anyhow::Result<()> {
    let cli = Cli::parse();
    let root = cli
        .root
        .unwrap_or_else(|| PathBuf::from(env!("CARGO_MANIFEST_DIR")))
        .canonicalize()?;
    match cli.command {
        Task::DependencyPolicy {
            write,
            metadata: path,
        } => {
            let metadata = metadata::load(&root, path.as_deref())?;
            policy::run(&root, &metadata, write)?;
        }
        Task::PrepareDockerCache => cache::prepare(&root, &metadata::load(&root, None)?)?,
        Task::SmokeDocker { image, report } => {
            smoke::run(&root, &image, &relative_to(&root, &report))?
        }
        Task::SmokeHubDocker { image, report } => {
            hub_smoke::run(&root, &image, &relative_to(&root, &report))?
        }
    }
    Ok(())
}

fn relative_to(root: &Path, path: &Path) -> PathBuf {
    if path.is_absolute() {
        path.to_owned()
    } else {
        root.join(path)
    }
}

#[cfg(test)]
mod test_support {
    use std::{
        path::PathBuf,
        sync::atomic::{AtomicUsize, Ordering},
    };
    static NEXT: AtomicUsize = AtomicUsize::new(0);
    pub struct Directory(pub PathBuf);
    impl Directory {
        pub fn new() -> Self {
            let path = std::env::temp_dir().join(format!(
                "patchpulse-xtask-{}-{}",
                std::process::id(),
                NEXT.fetch_add(1, Ordering::Relaxed)
            ));
            std::fs::create_dir_all(&path).unwrap();
            Self(path)
        }
    }
    impl Drop for Directory {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }
}
