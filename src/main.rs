use actix_web::{App, HttpServer, web};
use env_logger;
use futures_util::stream::StreamExt;
use log::error;
use log::info;
use solana_sdk::instruction::Instruction;
use solana_transaction_status::{
    EncodedTransaction, UiInstruction, UiMessage, UiParsedInstruction,
};
use std::sync::Arc;
use tokio::sync::{RwLock, mpsc};
use tonic::transport::ClientTlsConfig;
use yellowstone_grpc_client::GeyserGrpcClient;
use yellowstone_grpc_proto::prelude::{
    SubscribeRequest, SubscribeRequestFilterTransactions, SubscribeUpdate,
    subscribe_update::UpdateOneof,
};

mod base;
mod indexer;
mod persistence;
mod query;
mod utils;

use base::*;
use indexer::{Indexer, IndexerConfig};
use persistence::Persistence;
use query::QueryHandler;
use solana_sdk::{pubkey::Pubkey, signature::Signature};
use std::str::FromStr;
use utils::{CreateTokenInfo, TransactionPretty, parse_create_token_data, parse_instruction};

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    env_logger::init();
    start_indexer().await?;
    Ok(())
}

async fn start_indexer() -> anyhow::Result<()> {
    // Initialize indexer and persistence
    let indexer_config = IndexerConfig {
        persistence_path: "tokens.json".to_string(),
        persistence_interval_secs: 60,
    };
    let indexer = Arc::new(RwLock::new(Indexer::new(indexer_config.clone())));
    let persistence = Arc::new(Persistence::new(
        indexer.clone(),
        indexer_config.persistence_path.clone(),
    ));

    // persistence tasks
    let persistence_clone = persistence.clone();
    tokio::spawn(async move {
        persistence_clone.run().await;
    });

    // query handler
    let query_handler = Arc::new(QueryHandler::new(indexer.clone()));

    let query_handler_clone = query_handler.clone();
    let server = HttpServer::new(move || {
        App::new()
            .app_data(web::Data::new(query_handler_clone.clone()))
            .route("/tokens", web::get().to(query::handle_query))
    })
    .bind((SERVER_HOST, SERVER_PORT))?
    .run();

    info!("SERVER STARTS AT {}:{}", SERVER_HOST, SERVER_PORT);

    tokio::spawn(server);

    // geyser streaming
    let (tx, mut rx) = mpsc::channel::<SubscribeUpdate>(100);
    let mut client = GeyserGrpcClient::build_from_static(DEFAULT_GEYSER_ENDPOINT)
        .tls_config(ClientTlsConfig::new().with_native_roots())?
        .connect()
        .await
        .map_err(|e| anyhow::anyhow!("Failed to connect to Geyser: {:?}", e))?;

    info!("Connected to Geyser at {}", DEFAULT_GEYSER_ENDPOINT);

    let mut subscribe_request = SubscribeRequest::default();
    subscribe_request.transactions.insert(
        "pumpfun".to_string(),
        SubscribeRequestFilterTransactions {
            vote: Some(false),
            failed: Some(false),
            signature: None,
            account_include: vec![PUMPFUN_PROGRAM_ID.to_string()],
            account_exclude: vec![],
            account_required: vec![],
        },
    );

    tokio::spawn(async move {
        let (mut _subscribe_tx, subscribe_stream) =
            match client.subscribe_with_request(Some(subscribe_request)).await {
                Ok((tx, stream)) => (tx, stream),
                Err(e) => {
                    error!("Failed to subscribe: {:?}", e);
                    return;
                }
            };

        tokio::pin!(subscribe_stream);
        while let Some(message) = subscribe_stream.next().await {
            match message {
                Ok(update) => {
                    if let Err(e) = tx.send(update).await {
                        error!("Failed to send update: {:?}", e);
                        break;
                    }
                }
                Err(e) => {
                    error!("Stream error: {:?}", e);
                    continue;
                }
            }
        }
    });

    // Process updates
    while let Some(msg) = rx.recv().await {
        if let Some(UpdateOneof::Transaction(subscribe_update_tx)) = msg.update_oneof {
            if let Err(e) =
                process_tx_update(&indexer, TransactionPretty::from(subscribe_update_tx)).await
            {
                error!("Error processing transaction update: {:?}", e);
                continue;
            }
        }
    }

    Ok(())
}

async fn process_tx_update(
    indexer: &Arc<RwLock<Indexer>>,
    transaction_pretty: TransactionPretty,
) -> anyhow::Result<()> {
    let trade_raw = transaction_pretty.tx;
    let meta = trade_raw
        .meta
        .as_ref()
        .ok_or_else(|| anyhow::anyhow!("Missing transaction metadata"))?;

    if meta.err.is_some() {
        return Ok(());
    }

    let logs = if let solana_transaction_status::option_serializer::OptionSerializer::Some(logs) =
        &meta.log_messages
    {
        logs
    } else {
        &vec![]
    };

    let instructions = parse_instruction(logs)?;
    let mut indexer_write = indexer.write().await;

    for token_info in instructions {
        indexer_write.add_token(token_info.clone());
        info!(
            "Indexed new token: {} (CA: {}, Symbol: {})",
            token_info.name, token_info.mint, token_info.symbol
        );
    }

    Ok(())
}
