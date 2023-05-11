use anyhow::{anyhow, bail};
use bitcoin::consensus::{serialize, Decodable};
use bitcoin::network::Magic;
use bitcoin::Transaction;
use bitcoincore_rpc::RpcApi;
use clap::Parser;
use hex_string::HexString;
use nostr::prelude::*;
use nostr::Keys;
use nostr_sdk::relay::pool::RelayPoolNotification::*;
use nostr_sdk::Client;
use std::str::FromStr;
extern crate pretty_env_logger;


#[derive(Parser)]
#[command()]
struct Args {
    #[clap(default_value_t = Network::Bitcoin, short, long)]
    network: Network,

    #[arg(short, long)]
    relays: Vec<String>,

    #[arg(long)]
    bitcoin_host: Option<String>,

    #[arg(long)]
    bitcoin_user: Option<String>,

    #[arg(long)]
    bitcoin_password: Option<String>,
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    pretty_env_logger::init();
    
    let args = Args::parse();

    let my_keys = Keys::generate();

    let client = Client::new(&my_keys);

    if args.relays.len() == 0 {
        anyhow::bail!("No relay(s) provided");
    }

    for relay in args.relays {
        client.add_relay(relay, None).await?;
    }
    client.connect().await;

    let bitcoin_tx_kind = Kind::Custom(28333);
    let subscription = Filter::new()
        .kinds(vec![bitcoin_tx_kind])
        .since(Timestamp::now());

    client.subscribe(vec![subscription]).await;

    println!("Connecting bitcoin core...");
    let rpc = bitcoincore_rpc::Client::new(
        &args.bitcoin_host.unwrap(),
        bitcoincore_rpc::Auth::UserPass(args.bitcoin_user.unwrap(), args.bitcoin_password.unwrap()),
    )
    .unwrap();

    let version = rpc.get_network_info().unwrap().subversion;

    println!("Connected to bitcoin core version {}", version);

    println!("Listening for bitcoin txs...");
    client
        .handle_notifications(|notification| async {
            if let Event(_, event) = notification {
                if event.kind == bitcoin_tx_kind {
                    // calculate network from magic
                    let magic = event
                        .tags
                        .clone()
                        .into_iter()
                        .find(|t| t.kind() == TagKind::Custom("magic".to_string()))
                        .and_then(|t| {
                            if let Tag::Generic(_, magic) = t {
                                magic.first().and_then(|m| Magic::from_str(m).ok())
                            } else {
                                None
                            }
                        });

                    match magic {
                        Some(magic) => {
                            if magic != args.network.magic() {
                                return Ok(());
                            }
                        }

                        None => return Ok(()),
                    }

                    // get transactions
                    let txs: Vec<Transaction> = event
                        .tags
                        .clone()
                        .into_iter()
                        .find(|t| t.kind() == TagKind::Custom("transactions".to_string()))
                        .map(|t| {
                            if let Tag::Generic(_, txs) = t {
                                txs.iter().filter_map(|tx| {
                                    HexString::from_string(tx).ok().and_then(|hex| {
                                        Transaction::consensus_decode(&mut hex.as_bytes().as_slice()).ok()
                                    })
                                }).collect()
                            } else {
                                vec![]
                            }
                        }).unwrap_or_default();

                    if let Err(e) = broadcast_txs(&rpc, txs).await {
                        println!("Error broadcasting txs: {e}");
                    }
                }
            }
            Ok(())
        })
        .await?;
    Ok(())
}

async fn broadcast_txs(rpc: &bitcoincore_rpc::Client, txs: Vec<Transaction>) -> anyhow::Result<()> {
    match txs.len() {
        0 => return Ok(()),
        1 => {
            // Use send_raw_transaction for single txs, because submitpackage
            // doesn't support them.
            for tx in &txs {
                let result = rpc.send_raw_transaction(tx);
        
                if let Err(e) = result {
                    println!("Error broadcasting tx: {}", e);
        
                    continue;
                }
        
                println!("Broadcasted tx: {}", tx.txid());
            }
        },
        _ => {
            let tx_refs: Vec<&Transaction> = txs.iter().collect();

            let result = rpc.submit_package(&tx_refs);
            if let Err(e) = result {
                bail!("Error submitting package: {}", e);
            }
        
            println!("{:?}", result);
        }
    }

    let txids: Vec<String> = txs.iter().
        map(|tx| tx.txid().to_string()).
        collect();

    println!("Submitted transactions: {}", txids.join(","));

    Ok(())
}
