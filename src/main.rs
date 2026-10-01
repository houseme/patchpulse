use clap::Parser;
use patchpulse::{
    app,
    config::{Cli, Config},
    observability,
};

fn main() -> anyhow::Result<()> {
    let cli = Cli::parse();
    if cli.service {
        #[cfg(windows)]
        {
            return patchpulse::service::run();
        }
        #[cfg(not(windows))]
        anyhow::bail!("--service requires Windows; use --foreground on this platform");
    }
    let mut config = Config::load(cli.config.as_deref())?;
    if let Some(bind) = cli.bind {
        config.server.bind = bind;
    }
    if cli.check_config {
        println!("Configuration is valid");
        return Ok(());
    }
    if cli.healthcheck {
        return tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()?
            .block_on(app::healthcheck(&config));
    }
    observability::init_logging(&config.observability)?;
    app::block_on(app::foreground(config))
}
