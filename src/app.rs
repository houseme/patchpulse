use crate::{
    api::{self, ApiState},
    cache::SnapshotStore,
    collector,
    config::Config,
    observability::Metrics,
    scheduler,
};
use anyhow::Context;
use std::{
    future::IntoFuture,
    net::{IpAddr, Ipv4Addr, Ipv6Addr},
    sync::Arc,
    time::Duration,
};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    sync::watch,
};

pub async fn run(
    config: Config,
    mut shutdown: watch::Receiver<bool>,
    on_listening: impl FnOnce() -> anyhow::Result<()>,
) -> anyhow::Result<()> {
    let listener = tokio::net::TcpListener::bind(config.server.bind)
        .await
        .context("bind HTTP listener")?;
    let orchestrator = Arc::new(collector::build(&config.collector));
    let store = SnapshotStore::new(config.cache.stale_after_secs, &orchestrator.enabled_names());
    let metrics = Metrics::default();
    let router = api::build(
        ApiState {
            store: store.clone(),
            metrics: metrics.clone(),
        },
        config.observability.metrics_enabled,
        config.server.request_timeout_secs,
    );
    on_listening()?;
    tracing::info!(address = %listener.local_addr()?, "PatchPulse listening");
    let scheduler = tokio::spawn(scheduler::run(
        store,
        Arc::clone(&orchestrator),
        metrics,
        config.collector.interval_secs,
        shutdown.clone(),
    ));
    let mut stop_observer = shutdown.clone();
    let server = axum::serve(listener, router).with_graceful_shutdown(async move {
        while !*shutdown.borrow() {
            if shutdown.changed().await.is_err() {
                break;
            }
        }
    });
    let server = server.into_future();
    tokio::pin!(server);
    let completed = tokio::select! {
        result = &mut server => Some(result),
        _ = async {
            while !*stop_observer.borrow() {
                if stop_observer.changed().await.is_err() { break; }
            }
        } => None,
    };
    orchestrator.cancel();
    let result = match completed {
        Some(result) => result.context("serve HTTP"),
        None => match tokio::time::timeout(Duration::from_secs(10), &mut server).await {
            Ok(result) => result.context("drain HTTP"),
            Err(_) => {
                tracing::warn!("HTTP shutdown exceeded ten seconds; closing remaining connections");
                Ok(())
            }
        },
    };
    scheduler.abort();
    let _ = scheduler.await;
    result
}

/// Foreground and SCM modes share one bounded runtime policy.
pub fn block_on(
    future: impl std::future::Future<Output = anyhow::Result<()>>,
) -> anyhow::Result<()> {
    let runtime = tokio::runtime::Builder::new_multi_thread()
        .worker_threads(2)
        .max_blocking_threads(8)
        .enable_all()
        .build()?;
    let result = runtime.block_on(future);
    runtime.shutdown_timeout(Duration::from_secs(5));
    result
}

pub async fn foreground(config: Config) -> anyhow::Result<()> {
    let (sender, receiver) = watch::channel(false);
    let signals = signal_shutdown(sender.clone());
    let server = run(config, receiver, || Ok(()));
    tokio::pin!(signals, server);
    tokio::select! {
        result = &mut server => result,
        signal = &mut signals => {
            let _ = sender.send(true);
            let served = server.await;
            signal.context("shutdown signal listener")?;
            served
        }
    }
}

/// Probe liveness without a curl executable in the runtime image.
pub async fn healthcheck(config: &Config) -> anyhow::Result<()> {
    let mut address = config.server.bind;
    if address.ip().is_unspecified() {
        address.set_ip(if address.is_ipv4() {
            IpAddr::V4(Ipv4Addr::LOCALHOST)
        } else {
            IpAddr::V6(Ipv6Addr::LOCALHOST)
        });
    }
    tokio::time::timeout(Duration::from_secs(5), async {
        let mut stream = tokio::net::TcpStream::connect(address).await?;
        stream
            .write_all(b"GET /health HTTP/1.1\r\nHost: localhost\r\nConnection: close\r\n\r\n")
            .await?;
        let mut bytes = [0; 128];
        let mut filled = 0;
        loop {
            let read = stream.read(&mut bytes[filled..]).await?;
            filled += read;
            if bytes[..filled].contains(&b'\n') || read == 0 || filled == bytes.len() {
                break;
            }
        }
        anyhow::ensure!(
            bytes[..filled].starts_with(b"HTTP/1.1 200 "),
            "health endpoint did not return 200"
        );
        Ok::<_, anyhow::Error>(())
    })
    .await
    .context("healthcheck timeout")??;
    Ok(())
}

pub async fn signal_shutdown(sender: watch::Sender<bool>) -> anyhow::Result<()> {
    #[cfg(unix)]
    {
        let mut terminate =
            tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())?;
        tokio::select! { result = tokio::signal::ctrl_c() => { result?; }, _ = terminate.recv() => {} }
    }
    #[cfg(not(unix))]
    tokio::signal::ctrl_c().await?;
    let _ = sender.send(true);
    Ok(())
}
