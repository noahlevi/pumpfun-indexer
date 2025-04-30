use base64::{Engine as _, engine::general_purpose::STANDARD as base64};
use borsh::{BorshDeserialize, BorshSerialize};
use chrono::Utc;
use serde::{Deserialize, Serialize};
use serde_with::{DisplayFromStr, serde_as};
use solana_sdk::{pubkey::Pubkey, signature::Signature};
use solana_transaction_status::EncodedTransactionWithStatusMeta;
use solana_transaction_status::UiTransactionEncoding;
use std::str::FromStr;
use yellowstone_grpc_proto::geyser::SubscribeUpdateTransaction;
use crate::base::PUMPFUN_PROGRAM_ID;

#[derive(Clone)]
pub struct TransactionPretty {
    pub slot: u64,
    pub signature: Signature,
    pub is_vote: bool,
    pub tx: EncodedTransactionWithStatusMeta,
}

#[serde_as]
#[derive(
    Clone, Debug, Default, PartialEq, BorshDeserialize, BorshSerialize, Serialize, Deserialize,
)]
pub struct CreateTokenInfo {
    pub name: String,
    pub symbol: String,
    pub uri: String,
    #[serde_as(as = "DisplayFromStr")]
    pub mint: Pubkey,
    #[serde_as(as = "DisplayFromStr")]
    pub bonding_curve: Pubkey,
    #[serde_as(as = "DisplayFromStr")]
    pub user: Pubkey,
    pub created_at: String,
}

impl From<SubscribeUpdateTransaction> for TransactionPretty {
    fn from(SubscribeUpdateTransaction { transaction, slot }: SubscribeUpdateTransaction) -> Self {
        let tx = transaction.expect("should be defined");
        Self {
            slot,
            signature: Signature::try_from(tx.signature.as_slice()).expect("valid signature"),
            is_vote: tx.is_vote,
            tx: yellowstone_grpc_proto::convert_from::create_tx_with_meta(tx)
                .expect("valid tx with meta")
                .encode(UiTransactionEncoding::Base64, Some(u8::MAX), true)
                .expect("failed to encode"),
        }
    }
}

fn read_u32(data: &[u8]) -> u32 {
    let mut bytes = [0u8; 4];
    bytes.copy_from_slice(&data[..4]);
    u32::from_le_bytes(bytes)
}

pub fn parse_create_token_data(data: &str) -> anyhow::Result<CreateTokenInfo> {
    let decoded = base64
        .decode(data)
        .map_err(|e| anyhow::anyhow!("Failed to decode base64: {}", e))?;
    let mut cursor = if decoded.len() > 8 { 8 } else { 0 };

    if cursor + 4 > decoded.len() {
        return Err(anyhow::anyhow!("Data too short for name length"));
    }
    let name_len = read_u32(&decoded[cursor..]) as usize;
    cursor += 4;

    if cursor + name_len > decoded.len() {
        return Err(anyhow::anyhow!(
            "Data too short for name: need {} bytes",
            name_len
        ));
    }
    let name = String::from_utf8(decoded[cursor..cursor + name_len].to_vec())
        .map_err(|e| anyhow::anyhow!("Invalid UTF-8 in name: {}", e))?;
    cursor += name_len;

    if cursor + 4 > decoded.len() {
        return Err(anyhow::anyhow!("Data too short for symbol length"));
    }
    let symbol_len = read_u32(&decoded[cursor..]) as usize;
    cursor += 4;

    if cursor + symbol_len > decoded.len() {
        return Err(anyhow::anyhow!(
            "Data too short for symbol: need {} bytes",
            symbol_len
        ));
    }
    let symbol = String::from_utf8(decoded[cursor..cursor + symbol_len].to_vec())
        .map_err(|e| anyhow::anyhow!("Invalid UTF-8 in symbol: {}", e))?;
    cursor += symbol_len;

    if cursor + 4 > decoded.len() {
        return Err(anyhow::anyhow!("Data too short for URI length"));
    }
    let uri_len = read_u32(&decoded[cursor..]) as usize;
    cursor += 4;

    if cursor + uri_len > decoded.len() {
        return Err(anyhow::anyhow!(
            "Data too short for URI: need {} bytes",
            uri_len
        ));
    }
    let uri = String::from_utf8(decoded[cursor..cursor + uri_len].to_vec())
        .map_err(|e| anyhow::anyhow!("Invalid UTF-8 in uri: {}", e))?;
    cursor += uri_len;

    if cursor + 32 * 3 > decoded.len() {
        return Err(anyhow::anyhow!("Data too short for public keys"));
    }

    let mint = bs58::encode(&decoded[cursor..cursor + 32]).into_string();
    cursor += 32;
    let bonding_curve = bs58::encode(&decoded[cursor..cursor + 32]).into_string();
    cursor += 32;
    let user = bs58::encode(&decoded[cursor..cursor + 32]).into_string();

    Ok(CreateTokenInfo {
        name,
        symbol,
        uri,
        mint: Pubkey::from_str(&mint).unwrap(),
        bonding_curve: Pubkey::from_str(&bonding_curve).unwrap(),
        user: Pubkey::from_str(&user).unwrap(),
        created_at: Utc::now().format("%Y-%m-%d %H:%M:%S").to_string(),
    })
}

pub fn parse_instruction(logs: &[String]) -> anyhow::Result<Vec<CreateTokenInfo>> {
    let mut current_instruction = None;
    let mut program_data = String::new();
    let mut invoke_depth = 0;
    let mut last_data_len = 0;
    let mut instructions: Vec<CreateTokenInfo> = vec![];

    for log in logs {
        if log.contains(&format!("Program {} invoke", PUMPFUN_PROGRAM_ID)) {
            invoke_depth += 1;
            if invoke_depth == 1 {
                current_instruction = None;
                program_data.clear();
                last_data_len = 0;
            }
            continue;
        }

        if invoke_depth == 0 {
            continue;
        }

        if invoke_depth == 1 && log.contains("Program log: Instruction:") {
            if log.contains("Create") {
                current_instruction = Some("create");
            } else if log.contains("Buy") || log.contains("Sell") {
                current_instruction = Some("trade");
            }
            continue;
        }

        if log.starts_with("Program data: ") {
            let data = log.trim_start_matches("Program data: ");
            if data.len() > last_data_len {
                program_data = data.to_string();
                last_data_len = data.len();
            }
        }

        if log.contains(&format!("Program {} success", PUMPFUN_PROGRAM_ID)) {
            invoke_depth -= 1;
            if invoke_depth == 0 {
                if let Some(instruction_type) = current_instruction {
                    if !program_data.is_empty() {
                        match instruction_type {
                            "create" => {
                                if let Ok(token_info) = parse_create_token_data(&program_data) {
                                    instructions.push(token_info);
                                }
                            }
                            _ => {}
                        }
                    }
                }
            }
        }
    }

    Ok(instructions)
}
