//! Blueprint runner for openclaw-instance-blueprint.
//!
//! Wires the job router, Tangle producer/consumer, and optional cron producers
//! into a `BlueprintRunner` and starts the event loop.

use blueprint_sdk::contexts::tangle::TangleClientContext;
use blueprint_sdk::runner::BlueprintRunner;
use blueprint_sdk::runner::config::BlueprintEnvironment;
use blueprint_sdk::runner::tangle::config::TangleConfig;
use blueprint_sdk::tangle::{TangleConsumer, TangleProducer};
use blueprint_sdk::{error, info, warn};
use openclaw_instance_blueprint_lib::operator_api::{operator_api_addr_from_env, run_operator_api};
use openclaw_instance_blueprint_lib::{init_runtime_adapter_from_env, router};

#[tokio::main]
#[allow(clippy::result_large_err)]
async fn main() -> Result<(), blueprint_sdk::Error> {
    setup_log();
    init_runtime_adapter_from_env().map_err(|e| blueprint_sdk::Error::Other(e.to_string()))?;

    let operator_shutdown = tokio::sync::watch::channel(());
    let operator_shutdown_tx = operator_shutdown.0;
    let operator_handle: Option<tokio::task::JoinHandle<()>> = match operator_api_addr_from_env()
        .map_err(blueprint_sdk::Error::Other)?
    {
        Some(addr) => {
            let listener = tokio::net::TcpListener::bind(addr).await.map_err(|e| {
                blueprint_sdk::Error::Other(format!("failed to bind operator api on {addr}: {e}"))
            })?;
            info!("Starting operator API on {addr}");
            let shutdown_rx = operator_shutdown_tx.subscribe();
            Some(tokio::spawn(async move {
                run_operator_api(listener, shutdown_rx).await;
            }))
        }
        None => {
            warn!("Operator API disabled (OPENCLAW_OPERATOR_HTTP_ENABLED=false)");
            None
        }
    };

    let env = BlueprintEnvironment::load()?;

    let tangle_client = env
        .tangle_client()
        .await
        .map_err(|e| blueprint_sdk::Error::Other(e.to_string()))?;

    let service_id = env
        .protocol_settings
        .tangle()
        .map_err(|e| blueprint_sdk::Error::Other(e.to_string()))?
        .service_id
        .ok_or_else(|| blueprint_sdk::Error::Other("SERVICE_ID missing".into()))?;

    info!("Starting openclaw-instance-blueprint for service {service_id}");

    let tangle_producer = TangleProducer::new(tangle_client.clone(), service_id);
    let tangle_consumer = TangleConsumer::new(tangle_client);

    let tangle_config = TangleConfig::default();

    let result = BlueprintRunner::builder(tangle_config, env)
        .router(router())
        .producer(tangle_producer)
        .consumer(tangle_consumer)
        .with_shutdown_handler(async {
            info!("Shutting down openclaw-instance-blueprint");
        })
        .run()
        .await;

    let runner_outcome: Result<(), blueprint_sdk::Error> = match result {
        Ok(()) => Ok(()),
        Err(e) => {
            error!("Runner failed: {e:?}");
            Err(blueprint_sdk::Error::Other(e.to_string()))
        }
    };
    let _ = operator_shutdown_tx.send(());
    if let Some(handle) = operator_handle {
        let _ = handle.await;
    }

    runner_outcome
}

fn setup_log() {
    use tracing_subscriber::prelude::*;
    use tracing_subscriber::{EnvFilter, fmt};
    if tracing_subscriber::registry()
        .with(fmt::layer())
        .with(EnvFilter::from_default_env())
        .try_init()
        .is_err()
    {}
}
