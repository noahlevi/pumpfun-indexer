use crate::indexer::Indexer;
use actix_web::{HttpResponse, Responder, web};
use serde::Deserialize;
use std::sync::Arc;
use tokio::sync::RwLock;

pub struct QueryHandler {
    indexer: Arc<RwLock<Indexer>>,
}

#[derive(Deserialize)]
pub struct QueryParams {
    min_age_hours: Option<u64>,
    name: Option<String>,
    min_holder_count: Option<usize>,
}

impl QueryHandler {
    pub fn new(indexer: Arc<RwLock<Indexer>>) -> Self {
        QueryHandler { indexer }
    }
}

pub async fn handle_query(
    query_handler: web::Data<QueryHandler>,
    query: web::Query<QueryParams>,
) -> impl Responder {
    let indexer = query_handler.indexer.read().await;
    let tokens = indexer.query_tokens(
        query.min_age_hours,
        query.name.clone(),
        query.min_holder_count,
    );

    HttpResponse::Ok().json(tokens)
}
