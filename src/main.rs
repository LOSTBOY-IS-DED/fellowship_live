use base64::{Engine as _, engine::general_purpose};
use bs58;
use poem::http::StatusCode;
use poem::{Route, Server, handler, listener::TcpListener, post, web::Json};
use serde::{Deserialize, Serialize};
use serde_json::json;
use solana_sdk::{
    instruction::Instruction,
    pubkey::Pubkey,
    signature::{Signature, Signer},
    signer::keypair::Keypair,
    system_instruction,
};
use spl_token::instruction::{initialize_mint, mint_to, transfer};

#[derive(Serialize)]
struct SuccessResponse<T> {
    success: bool,
    data: T,
}

#[derive(Serialize)]
struct ErrorResponse {
    success: bool,
    error: String,
}

#[derive(Deserialize)]
struct CreateTokenRequest {
    mint_authority: String,
    mint: String,
    decimals: u8,
}

#[derive(Deserialize)]
struct MintTokenRequest {
    mint: String,
    destination: String,
    authority: String,
    amount: u64,
}

#[derive(Deserialize)]
struct SignMessageRequest {
    message: String,
    secret: String,
}

#[derive(Deserialize)]
struct VerifyMessageRequest {
    message: String,
    signature: String,
    pubkey: String,
}

#[derive(Deserialize)]
struct SolTransferRequest {
    from: String,
    to: String,
    lamports: u64,
}

#[derive(Deserialize)]
struct TokenTransferRequest {
    destination: String,
    mint: String,
    owner: String,
    amount: u64,
}

#[handler]
async fn generate_keypair() -> Json<SuccessResponse<serde_json::Value>> {
    let kp = Keypair::new();
    Json(SuccessResponse {
        success: true,
        data: json!({
            "pubkey": kp.pubkey().to_string(),
            "secret": bs58::encode(kp.to_bytes()).into_string()
        }),
    })
}

#[handler]
async fn create_token(
    Json(body): Json<CreateTokenRequest>,
) -> poem::Result<Json<SuccessResponse<serde_json::Value>>> {
    let mint = parse_pubkey(&body.mint)?;
    let authority = parse_pubkey(&body.mint_authority)?;
    let ix = initialize_mint(&spl_token::ID, &mint, &authority, None, body.decimals)
        .map_err(|e| poem::Error::from_string(e.to_string(), StatusCode::BAD_REQUEST))?;
    Ok(Json(SuccessResponse {
        success: true,
        data: serialize_instruction(ix),
    }))
}

#[handler]
async fn mint_token(
    Json(body): Json<MintTokenRequest>,
) -> poem::Result<Json<SuccessResponse<serde_json::Value>>> {
    let ix = mint_to(
        &spl_token::ID,
        &parse_pubkey(&body.mint)?,
        &parse_pubkey(&body.destination)?,
        &parse_pubkey(&body.authority)?,
        &[],
        body.amount,
    )
    .map_err(|e| poem::Error::from_string(e.to_string(), StatusCode::BAD_REQUEST))?;
    Ok(Json(SuccessResponse {
        success: true,
        data: serialize_instruction(ix),
    }))
}

#[handler]
async fn sign_message(
    Json(body): Json<SignMessageRequest>,
) -> poem::Result<Json<SuccessResponse<serde_json::Value>>> {
    if body.message.is_empty() || body.secret.is_empty() {
        return Err(poem::Error::from_string(
            "Missing required fields",
            StatusCode::BAD_REQUEST,
        ));
    }

    let secret_bytes = bs58::decode(&body.secret)
        .into_vec()
        .map_err(|_| poem::Error::from_string("Invalid secret", StatusCode::BAD_REQUEST))?;
    let keypair = Keypair::try_from(&secret_bytes[..])
        .map_err(|_| poem::Error::from_string("Invalid keypair", StatusCode::BAD_REQUEST))?;
    let sig = keypair
        .try_sign_message(body.message.as_bytes())
        .map_err(|_| poem::Error::from_string("Signing failed", StatusCode::BAD_REQUEST))?;

    Ok(Json(SuccessResponse {
        success: true,
        data: json!({
            "signature": general_purpose::STANDARD.encode(sig.as_ref()),
            "pubkey": keypair.pubkey().to_string(),
            "message": body.message
        }),
    }))
}

#[handler]
async fn verify_message(
    Json(body): Json<VerifyMessageRequest>,
) -> poem::Result<Json<SuccessResponse<serde_json::Value>>> {
    let pubkey = parse_pubkey(&body.pubkey)?;
    let sig_bytes = general_purpose::STANDARD
        .decode(&body.signature)
        .map_err(|_| {
            poem::Error::from_string("Invalid signature encoding", StatusCode::BAD_REQUEST)
        })?;
    let signature = Signature::try_from(&sig_bytes[..])
        .map_err(|_| poem::Error::from_string("Bad signature", StatusCode::BAD_REQUEST))?;
    let valid = signature.verify(pubkey.as_ref(), body.message.as_bytes());

    Ok(Json(SuccessResponse {
        success: true,
        data: json!({ "valid": valid, "message": body.message, "pubkey": body.pubkey }),
    }))
}

#[handler]
async fn send_sol(
    Json(body): Json<SolTransferRequest>,
) -> poem::Result<Json<SuccessResponse<serde_json::Value>>> {
    let ix = system_instruction::transfer(
        &parse_pubkey(&body.from)?,
        &parse_pubkey(&body.to)?,
        body.lamports,
    );
    Ok(Json(SuccessResponse {
        success: true,
        data: serialize_instruction(ix),
    }))
}

#[handler]
async fn send_token(
    Json(body): Json<TokenTransferRequest>,
) -> poem::Result<Json<SuccessResponse<serde_json::Value>>> {
    let ix = transfer(
        &spl_token::ID,
        &parse_pubkey(&body.owner)?,
        &parse_pubkey(&body.destination)?,
        &parse_pubkey(&body.owner)?,
        &[],
        body.amount,
    )
    .map_err(|e| poem::Error::from_string(e.to_string(), StatusCode::BAD_REQUEST))?;
    Ok(Json(SuccessResponse {
        success: true,
        data: serialize_instruction(ix),
    }))
}

fn parse_pubkey(s: &str) -> poem::Result<Pubkey> {
    s.parse::<Pubkey>()
        .map_err(|_| poem::Error::from_string("Invalid pubkey", StatusCode::BAD_REQUEST))
}

fn serialize_instruction(ix: Instruction) -> serde_json::Value {
    json!({
        "program_id": ix.program_id.to_string(),
        "accounts": ix.accounts.iter().map(|a| json!({
            "pubkey": a.pubkey.to_string(),
            "is_signer": a.is_signer,
            "is_writable": a.is_writable
        })).collect::<Vec<_>>(),
        "instruction_data": general_purpose::STANDARD.encode(&ix.data),
    })
}

#[tokio::main]
async fn main() -> Result<(), std::io::Error> {
    let app = Route::new()
        .at("/keypair", post(generate_keypair))
        .at("/token/create", post(create_token))
        .at("/token/mint", post(mint_token))
        .at("/message/sign", post(sign_message))
        .at("/message/verify", post(verify_message))
        .at("/send/sol", post(send_sol))
        .at("/send/token", post(send_token));

    Server::new(TcpListener::bind("0.0.0.0:3000"))
        .run(app)
        .await
}
