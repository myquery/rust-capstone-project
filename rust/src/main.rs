#![allow(unused)]
use bitcoin::hex::DisplayHex;
use bitcoincore_rpc::bitcoin::{Amount, Network, Txid};
use bitcoincore_rpc::json::AddressType;
use bitcoincore_rpc::{Auth, Client, RpcApi};
use serde::Deserialize;
use serde_json::{json, Value};
use std::fs::File;
use std::io::Write;
use std::str::FromStr;

// Node access params
const RPC_URL: &str = "http://127.0.0.1:18443"; // Default regtest RPC port
const RPC_USER: &str = "alice";
const RPC_PASS: &str = "password";

// You can use calls not provided in RPC lib API using the generic `call` function.
// An example of using the `send` RPC call, which doesn't have exposed API.
// You can also use serde_json `Deserialize` derivation to capture the returned json result.
fn send(rpc: &Client, addr: &str) -> bitcoincore_rpc::Result<String> {
    let args = [
        json!([{addr : 100 }]), // recipient address
        json!(null),            // conf target
        json!(null),            // estimate mode
        json!(null),            // fee rate in sats/vb
        json!(null),            // Empty option object
    ];

    #[derive(Deserialize)]
    struct SendResult {
        complete: bool,
        txid: String,
    }
    let send_result = rpc.call::<SendResult>("send", &args)?;
    assert!(send_result.complete);
    Ok(send_result.txid)
}

fn auth() -> Auth {
    Auth::UserPass(RPC_USER.to_owned(), RPC_PASS.to_owned())
}

fn wallet_client(wallet_name: &str) -> bitcoincore_rpc::Result<Client> {
    Client::new(&format!("{RPC_URL}/wallet/{wallet_name}"), auth())
}
fn ensure_wallet(rpc: &Client, wallet_name: &str) -> bitcoincore_rpc::Result<Client> {
    if !rpc.list_wallets()?.iter().any(|w| w == wallet_name) {
        if rpc.list_wallet_dir()?.iter().any(|w| w == wallet_name) {
            rpc.load_wallet(wallet_name)?;
        } else {
            rpc.create_wallet(wallet_name, None, None, None, None)?;
        }
    }
    wallet_client(wallet_name)
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Connect to Bitcoin Core RPC
    let rpc = Client::new(RPC_URL, auth())?;

    // Get blockchain info
    let blockchain_info = rpc.get_blockchain_info()?;
    println!("Blockchain Info: {blockchain_info:?}");

    // Create/Load the wallets, named 'Miner' and 'Trader'. Have logic to optionally create/load them if they do not exist or not loaded already.
    let miner = ensure_wallet(&rpc, "Miner")?;
    let trader = ensure_wallet(&rpc, "Trader")?;

    // Generate spendable balances in the Miner wallet. How many blocks needs to be mined?

    let mining_address = miner
        .get_new_address(Some("Mining Reward"), Some(AddressType::Bech32))?
        .require_network(Network::Regtest)?;

    let mut mined = 0;
    while miner.get_balance(Some(1), None)?.to_sat() == 0 {
        miner.generate_to_address(1, &mining_address)?;
        mined += 1;
    }

    println!("Mined {mined} blocks before Miner had spendable balance");
    println!(
        "Miner balance: {}",
        miner.get_balance(Some(1), None)?.to_btc()
    );

    // Load Trader wallet and generate a new address

    let trader_address = trader
        .get_new_address(Some("Received"), Some(AddressType::Bech32))?
        .require_network(Network::Regtest)?;

    // Send 20 BTC from Miner to Trader
    let txid = miner.send_to_address(
        &trader_address,
        Amount::from_btc(20.0)?,
        None,
        None,
        None,
        None,
        None,
        None,
    )?;

    // Check transaction in mempool
    let mempool_entry = miner.get_mempool_entry(&txid)?;
    println!("Mempool entry: {mempool_entry:?}");

    // Mine 1 block to confirm the transaction
    let block_num = 1;
    let confirmation_blocks = miner.generate_to_address(block_num, &mining_address)?;
    let confirmation_block_hash = confirmation_blocks[0];

    // Extract all required transaction details
    let transaction_detail: Value = miner.call(
        "gettransaction",
        &[json!(txid.to_string()), json!(null), json!(true)],
    )?;

    let decoded_transaction = transaction_detail
        .get("decoded")
        .ok_or("Transaction response is missing decoded field")?;

    let miner_input = decoded_transaction["vin"]
        .as_array()
        .ok_or("Decoded transaction is missing vin array")?
        .first()
        .ok_or("Decoded transaction has no inputs")?;

    let transaction_outputs = decoded_transaction["vout"]
        .as_array()
        .ok_or("Decoded transaction is missing vout array")?;

    let trader_address_str = trader_address.to_string();

    let trader_output = transaction_outputs
        .iter()
        .find(|output| {
            output["scriptPubKey"]["address"].as_str() == Some(trader_address_str.as_str())
        })
        .ok_or("Trader output not found in transaction")?;

    let change_output = transaction_outputs
        .iter()
        .find(|output| {
            output["scriptPubKey"]["address"].as_str().is_some()
                && output["scriptPubKey"]["address"].as_str() != Some(trader_address_str.as_str())
        })
        .ok_or("Miner change output not found in transaction")?;

    let previous_txid = miner_input["txid"]
        .as_str()
        .ok_or("Missing txid in input")?;
    let previous_vout_index = miner_input["vout"]
        .as_u64()
        .ok_or("Missing vout in input")? as usize;

    let previous_transaction: Value =
        rpc.call("getrawtransaction", &[json!(previous_txid), json!(true)])?;

    let previous_output = previous_transaction["vout"]
        .as_array()
        .ok_or("Previous transaction is missing vout array")?
        .get(previous_vout_index)
        .ok_or("Previous output index not found")?;

    // Write the data to ../out.txt in the specified format given in readme.md

    let miner_input_address = previous_output["scriptPubKey"]["address"]
        .as_str()
        .ok_or("Missing Miner input address")?;
    let miner_input_amount = previous_output["value"]
        .as_f64()
        .ok_or("Missing Miner input amount")?;

    let trader_output_address = trader_address_str.as_str();
    let trader_output_amount = trader_output["value"]
        .as_f64()
        .ok_or("Missing Trader output amount")?;

    let miner_change_address = change_output["scriptPubKey"]["address"]
        .as_str()
        .ok_or("Missing Miner change address")?;
    let miner_change_amount = change_output["value"]
        .as_f64()
        .ok_or("Missing Miner change amount")?;

    let fee = transaction_detail["fee"]
        .as_f64()
        .ok_or("Missing transaction fee")?;
    let block_height = transaction_detail["blockheight"]
        .as_u64()
        .ok_or("Missing block height")?;
    let confirmed_block_hash = transaction_detail["blockhash"]
        .as_str()
        .ok_or("Missing block hash")?;

    let mut output_file = File::create("../out.txt")?;
    writeln!(output_file, "{txid}")?;
    writeln!(output_file, "{miner_input_address}")?;
    writeln!(output_file, "{miner_input_amount}")?;
    writeln!(output_file, "{trader_output_address}")?;
    writeln!(output_file, "{trader_output_amount}")?;
    writeln!(output_file, "{miner_change_address}")?;
    writeln!(output_file, "{miner_change_amount}")?;
    writeln!(output_file, "{fee}")?;
    writeln!(output_file, "{block_height}")?;
    writeln!(output_file, "{confirmed_block_hash}")?;

    Ok(())
}
