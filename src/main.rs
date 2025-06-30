use anyhow::Result;
use base64;
use poem::{
    IntoResponse, Route, Server, get, handler,
    listener::TcpListener,
    post,
    web::{Json, Path},
};
use serde::{Deserialize, Serialize};
use solana_client::rpc_client::RpcClient;
use solana_sdk::{
    bs58,
    commitment_config::CommitmentConfig,
    instruction::AccountMeta,
    pubkey::Pubkey,
    signature::{Keypair, Signature, Signer, read_keypair_file},
    system_instruction,
    transaction::Transaction,
};
use spl_token::instruction::{initialize_mint, mint_to};
use std::str::FromStr;

const RPC_URL: &str = "https://api.devnet.solana.com"; // Use devnet for safety

// All structs

// keypair endpoint struct
#[derive(Serialize)]
struct KeypairData {
    pubkey: String,
    secret: String,
}

#[derive(Serialize)]
struct KeypairResponse {
    success: bool,
    data: KeypairData,
}

// structs for creating spl token

#[derive(Deserialize)]
pub struct TokenCreateRequest {
    pub mintAuthority: String,
    pub mint: String,
    pub decimals: u8,
}

#[derive(Serialize)]
struct TokenInstructionResponse {
    success: bool,
    data: TokenInstructionData,
}

#[derive(Serialize)]
struct TokenInstructionData {
    program_id: String,
    accounts: Vec<AccountMetaData>,
    instruction_data: String,
}

#[derive(Serialize)]
struct AccountMetaData {
    pubkey: String,
    is_signer: bool,
    is_writable: bool,
}

// struct for mint token
#[derive(Deserialize)]
struct TokenMintRequest {
    pub mint: String,
    pub destination: String,
    pub authority: String,
    pub amount: u64,
}

// sign message structs

#[derive(Deserialize)]
struct SignMessageRequest {
    message: String,
    secret: String,
}

#[derive(Serialize)]
struct SignMessageResponse {
    success: bool,
    data: Option<SignMessageData>,
    error: Option<String>,
}

#[derive(Serialize)]
struct SignMessageData {
    signature: String,
    public_key: String,
    message: String,
}

#[derive(Serialize)]
struct BalanceResponse {
    address: String,
    balance_sol: f64,
}

#[derive(Serialize)]
// struct TokenAccount {
//     pubkey: String,
// }
#[derive(Deserialize)]
struct SendRequest {
    to: String,
    amount: f64,
}

// ========== HANDLERS ==========

//keypair endpoint
#[handler]
async fn generate_keypair() -> impl IntoResponse {
    let keypair = Keypair::new();

    let pubkey = keypair.pubkey().to_string();
    let secret = bs58::encode(keypair.to_bytes()).into_string();

    Json(KeypairResponse {
        success: true,
        data: KeypairData { pubkey, secret },
    })
}

// create token endpoint
#[handler]
async fn create_token(Json(req): Json<TokenCreateRequest>) -> Json<TokenInstructionResponse> {
    let mint_pubkey = match Pubkey::from_str(&req.mint) {
        Ok(pk) => pk,
        Err(_) => return Json(error_response("Invalid mint pubkey")),
    };

    let authority_pubkey = match Pubkey::from_str(&req.mintAuthority) {
        Ok(pk) => pk,
        Err(_) => return Json(error_response("Invalid mint authority pubkey")),
    };

    let ix = match initialize_mint(
        &spl_token::id(),
        &mint_pubkey,
        &authority_pubkey,
        None,
        req.decimals,
    ) {
        Ok(instruction) => instruction,
        Err(_) => return Json(error_response("Failed to create instruction")),
    };

    let accounts = ix
        .accounts
        .into_iter()
        .map(|meta| AccountMetaData {
            pubkey: meta.pubkey.to_string(),
            is_signer: meta.is_signer,
            is_writable: meta.is_writable,
        })
        .collect();

    let instruction_data = base64::encode(&ix.data);

    Json(TokenInstructionResponse {
        success: true,
        data: TokenInstructionData {
            program_id: ix.program_id.to_string(),
            accounts,
            instruction_data,
        },
    })
}

fn error_response(msg: &str) -> TokenInstructionResponse {
    TokenInstructionResponse {
        success: false,
        data: TokenInstructionData {
            program_id: "".to_string(),
            accounts: vec![],
            instruction_data: msg.to_string(),
        },
    }
}

#[handler]
async fn mint_token(Json(req): Json<TokenMintRequest>) -> Json<TokenInstructionResponse> {
    // Parse pubkeys from strings
    let mint_pubkey = match Pubkey::from_str(&req.mint) {
        Ok(pk) => pk,
        Err(_) => return Json(error_response("Invalid mint pubkey")),
    };
    let destination_pubkey = match Pubkey::from_str(&req.destination) {
        Ok(pk) => pk,
        Err(_) => return Json(error_response("Invalid destination pubkey")),
    };
    let authority_pubkey = match Pubkey::from_str(&req.authority) {
        Ok(pk) => pk,
        Err(_) => return Json(error_response("Invalid authority pubkey")),
    };

    // Create mint_to instruction
    let ix = match mint_to(
        &spl_token::id(),
        &mint_pubkey,
        &destination_pubkey,
        &authority_pubkey,
        &[],
        req.amount,
    ) {
        Ok(instruction) => instruction,
        Err(_) => return Json(error_response("Failed to create mint_to instruction")),
    };

    // Map instruction accounts to your API format
    let accounts = ix
        .accounts
        .into_iter()
        .map(|meta| AccountMetaData {
            pubkey: meta.pubkey.to_string(),
            is_signer: meta.is_signer,
            is_writable: meta.is_writable,
        })
        .collect();

    let instruction_data = base64::encode(&ix.data);

    Json(TokenInstructionResponse {
        success: true,
        data: TokenInstructionData {
            program_id: ix.program_id.to_string(),
            accounts,
            instruction_data,
        },
    })
}

#[handler]
async fn sign_message(Json(req): Json<SignMessageRequest>) -> Json<SignMessageResponse> {
    // Check for missing fields (simple validation)
    if req.message.is_empty() || req.secret.is_empty() {
        return Json(SignMessageResponse {
            success: false,
            data: None,
            error: Some("Missing required fields".to_string()),
        });
    }

    // Decode the secret key from base58
    let secret_bytes = match bs58::decode(&req.secret).into_vec() {
        Ok(bytes) => bytes,
        Err(_) => {
            return Json(SignMessageResponse {
                success: false,
                data: None,
                error: Some("Invalid secret key format".to_string()),
            });
        }
    };

    // Create Keypair from secret bytes
    // Keypair expects 64 bytes: 32 bytes secret + 32 bytes public
    let keypair = match Keypair::from_bytes(&secret_bytes) {
        Ok(kp) => kp,
        Err(_) => {
            return Json(SignMessageResponse {
                success: false,
                data: None,
                error: Some("Invalid secret key length".to_string()),
            });
        }
    };

    // Sign the message bytes
    let message_bytes = req.message.as_bytes();
    let signature = keypair.sign_message(message_bytes);

    // Return base64-encoded signature & pubkey
    Json(SignMessageResponse {
        success: true,
        data: Some(SignMessageData {
            signature: base64::encode(signature.as_ref()),
            public_key: keypair.pubkey().to_string(),
            message: req.message,
        }),
        error: None,
    })
}

#[handler]
async fn get_balance(Path(address): Path<String>) -> Json<BalanceResponse> {
    let client = RpcClient::new(RPC_URL.to_string());

    let pubkey = match Pubkey::from_str(&address) {
        Ok(pk) => pk,
        Err(_) => {
            return Json(BalanceResponse {
                address,
                balance_sol: 0.0,
            });
        }
    };

    let balance = client.get_balance(&pubkey).unwrap_or(0);
    let sol = balance as f64 / 1_000_000_000.0;

    Json(BalanceResponse {
        address,
        balance_sol: sol,
    })
}

// #[handler]
// async fn get_nfts(Path(address): Path<String>) -> Json<Vec<TokenAccount>> {
//     let client = RpcClient::new(RPC_URL.to_string());

//     let owner = match Pubkey::from_str(&address) {
//         Ok(pk) => pk,
//         Err(_) => return Json(vec![]),
//     };

//     let result = client.get_token_accounts_by_owner(
//         &owner,
//         solana_client::rpc_config::RpcTokenAccountsFilter::ProgramId(
//             Pubkey::from_str("TokenkegQfeZyiNwAJbNbGKPFXCWuBvf9Ss623VQ5DA").unwrap(),
//         ),
//     );

//     match result {
//         Ok(accs) => {
//             let tokens = accs
//                 .into_iter()
//                 .map(|acc| TokenAccount { pubkey: acc.pubkey })
//                 .collect();
//             Json(tokens)
//         }
//         Err(_) => Json(vec![]),
//     }
// }

#[handler]
async fn send_sol(Json(body): Json<SendRequest>) -> Json<String> {
    let to_pubkey = match Pubkey::from_str(&body.to) {
        Ok(pk) => pk,
        Err(_) => return Json("Invalid recipient pubkey.".to_string()),
    };

    let from_keypair = match read_keypair_file("id.json") {
        Ok(kp) => kp,
        Err(_) => return Json("Could not load sender keypair.".to_string()),
    };

    let client = RpcClient::new(RPC_URL.to_string());

    let lamports = (body.amount * 1_000_000_000.0) as u64;
    let recent_blockhash = match client.get_latest_blockhash() {
        Ok(bh) => bh,
        Err(_) => return Json("Failed to get blockhash.".to_string()),
    };

    let tx = Transaction::new_signed_with_payer(
        &[system_instruction::transfer(
            &from_keypair.pubkey(),
            &to_pubkey,
            lamports,
        )],
        Some(&from_keypair.pubkey()),
        &[&from_keypair],
        recent_blockhash,
    );

    match client.send_and_confirm_transaction(&tx) {
        Ok(sig) => Json(format!("Success! Tx Signature: {}", sig)),
        Err(e) => Json(format!("Transaction failed: {}", e)),
    }
}

#[handler]
async fn airdrop_sol(Path(address): Path<String>) -> Json<String> {
    let rpc = RpcClient::new_with_commitment(RPC_URL.to_string(), CommitmentConfig::confirmed());

    let pubkey = match Pubkey::from_str(&address) {
        Ok(pk) => pk,
        Err(_) => return Json("Invalid public key.".to_string()),
    };

    match rpc.request_airdrop(&pubkey, 1_000_000_000) {
        Ok(sig) => Json(format!("Airdrop requested. Signature: {}", sig)),
        Err(e) => Json(format!("Airdrop failed: {}", e)),
    }
}

// ========== MAIN ==========

#[tokio::main]
async fn main() -> Result<(), std::io::Error> {
    let app = Route::new()
        .at("/balance/:address", get(get_balance))
        // .at("/nfts/:address", get(get_nfts))
        .at("/send", post(send_sol))
        .at("/airdrop/:address", get(airdrop_sol))
        .at("/keypair", get(generate_keypair))
        .at("/token/create", post(create_token))
        .at("/token/mint", post(mint_token))
        .at("/message/sign", post(sign_message));

    println!("🚀 Server running on http://localhost:3000");
    Server::new(TcpListener::bind("127.0.0.1:3000"))
        .run(app)
        .await
}
