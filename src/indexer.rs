// indexer.rs
use crate::utils::CreateTokenInfo;
use chrono::{DateTime, Duration, Utc};
use solana_sdk::{instruction::Instruction, pubkey::Pubkey};
use std::collections::{HashMap, HashSet};

#[derive(Clone)]
pub struct IndexerConfig {
    pub persistence_path: String,
    pub persistence_interval_secs: u64,
}

pub struct Indexer {
    tokens: HashMap<Pubkey, CreateTokenInfo>,
    holder_counts: HashMap<Pubkey, HashSet<Pubkey>>,
    config: IndexerConfig,
}

impl Indexer {
    pub fn new(config: IndexerConfig) -> Self {
        Indexer {
            tokens: HashMap::new(),
            holder_counts: HashMap::new(),
            config,
        }
    }

    pub fn add_token(&mut self, token_info: CreateTokenInfo) {
        self.tokens.insert(token_info.mint, token_info.clone());
        self.holder_counts
            .entry(token_info.mint)
            .or_insert_with(HashSet::new);
    }

    pub fn increment_holder_count(&mut self, token_mint: &Pubkey, recipient: &Pubkey) {
        if let Some(holders) = self.holder_counts.get_mut(token_mint) {
            holders.insert(*recipient);
        }
    }

    pub fn get_token_mint_and_recipient(
        &self,
        instruction: &Instruction,
    ) -> Option<(Pubkey, Pubkey)> {
        // Simplified: Assume the token mint is the first account and recipient is the second
        // In a real implementation, parse instruction data to identify the mint and recipient
        if instruction.accounts.len() >= 2 {
            let token_mint = instruction.accounts.get(0)?;
            let recipient = instruction.accounts.get(1)?;
            // Verify that the token_mint exists in our indexed tokens
            let token_mint_pubkey = &token_mint.pubkey;
            let recipient_pubkey = &recipient.pubkey;
            if self.tokens.contains_key(&token_mint.pubkey) {
                return Some((*token_mint_pubkey, *recipient_pubkey));
            }
        }
        None
    }

    pub fn get_token_mint_from_instruction(&self, _instruction: &Instruction) -> Option<Pubkey> {
        // Simplified: in a real implementation, parse instruction data to extract mint
        // For now, assume first token in map for demo purposes
        self.tokens.keys().next().copied()
    }

    pub fn query_tokens(
        &self,
        min_age_hours: Option<u64>,
        name: Option<String>,
        min_holder_count: Option<usize>,
    ) -> Vec<CreateTokenInfo> {
        let now = Utc::now();
        self.tokens
            .values()
            .filter(|token| {
                // Age filter
                if let Some(hours) = min_age_hours {
                    if let Ok(created_at) =
                        DateTime::parse_from_str(&token.created_at, "%Y-%m-%d %H:%M:%S")
                    {
                        let age = now - created_at.with_timezone(&Utc);
                        if age < Duration::hours(hours as i64) {
                            return false;
                        }
                    } else {
                        return false;
                    }
                }
                true
            })
            .filter(|token| {
                // Name filter (case-insensitive)
                if let Some(query) = &name {
                    token.name.to_lowercase().contains(&query.to_lowercase())
                        || token.symbol.to_lowercase().contains(&query.to_lowercase())
                } else {
                    true
                }
            })
            .filter(|token| {
                // Holder count filter
                if let Some(min_count) = min_holder_count {
                    if let Some(holders) = self.holder_counts.get(&token.mint) {
                        holders.len() >= min_count
                    } else {
                        false
                    }
                } else {
                    true
                }
            })
            .cloned()
            .collect()
    }

    pub fn get_all_tokens(&self) -> Vec<CreateTokenInfo> {
        self.tokens.values().cloned().collect()
    }
}
