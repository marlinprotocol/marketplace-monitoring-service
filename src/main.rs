mod db;
mod models;
mod reachability;
mod schema;
mod types;

use ethers::contract::abigen;
use ethers::prelude::*;
use ethers::providers::{Http, Provider};
use log::{error, info};
use std::sync::Arc;
use std::time::Duration as StdDuration;

use db::establish_connection_pool;
use models::{NewOperatorEndpointError, NewReachabilityError};
use reachability::check_reachability;
use types::Metadata;

use crate::reachability::wait_for_ip_address;

abigen!(MarketV1, "src/abis/oyster_market_abi.json");

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    env_logger::init();

    dotenvy::dotenv().ok();

    // Establish database connection pool
    let pool = establish_connection_pool();
    info!("Database connection pool established");

    let rpc_url = std::env::var("RPC_URL").expect("RPC_URL must be set in .env file");

    let provider = Provider::<Http>::try_from(rpc_url)?;
    let provider = Arc::new(provider);

    let contract_address_str =
        std::env::var("CONTRACT_ADDRESS").expect("CONTRACT_ADDRESS must be set in .env file");
    let contract_addr: Address = contract_address_str.parse()?;
    let contract = MarketV1::new(contract_addr, provider.clone());

    // Get the current block number to start from
    let mut last_checked_block = provider.get_block_number().await?;
    info!("Starting from block number: {}", last_checked_block);

    // Poll for new blocks every 10 seconds
    let mut interval = tokio::time::interval(StdDuration::from_secs(10));

    loop {
        interval.tick().await;

        let current_block = match provider.get_block_number().await {
            Ok(block) => block,
            Err(e) => {
                error!("Failed to get current block number: {}", e);
                continue;
            }
        };

        if current_block <= last_checked_block {
            info!("No new blocks. Current block: {}", current_block);
            continue;
        }

        info!(
            "New blocks detected. Checking from {} to {}",
            last_checked_block + 1,
            current_block
        );

        // Query for JobOpened events in the new blocks
        let events = match contract
            .event::<JobOpenedFilter>()
            .from_block(last_checked_block + 1)
            .to_block(current_block)
            .query()
            .await
        {
            Ok(events) => events,
            Err(e) => {
                error!("Failed to query events: {}", e);
                continue;
            }
        };

        info!(
            "Found {} JobOpened events in blocks {} to {}",
            events.len(),
            last_checked_block + 1,
            current_block
        );

        for event in events {
            info!("JobOpened event found");
            let metadata_str = event.metadata;
            let owner = event.owner;
            let job = "0x".to_string() + &hex::encode(event.job);
            let operator = event.provider;
            let cp_url = match contract.providers(operator).call().await {
                Ok(url) => url,
                Err(e) => {
                    error!("Failed to get provider URL: {}", e);
                    continue;
                }
            };

            let metadata: Metadata = match serde_json::from_str(&metadata_str) {
                Ok(m) => m,
                Err(e) => {
                    error!(
                        "Failed to parse metadata JSON: {} | raw: {}",
                        e, metadata_str
                    );
                    continue;
                }
            };

            // Check if the URL matches the allowed blue images
            if let Some(url) = &metadata.url {
                let allowed_urls = [
                    "https://artifacts.marlin.org/oyster/eifs/base-blue_v3.0.0_linux_amd64.eif",
                    "https://artifacts.marlin.org/oyster/eifs/base-blue_v3.0.0_linux_arm64.eif",
                ];

                if !allowed_urls.contains(&url.as_str()) {
                    info!(
                        "Not using blue images for deployment. URL in metadata: {}",
                        url
                    );
                    continue;
                }
            } else {
                info!("No URL found in metadata, skipping deployment checks");
                continue;
            }

            let pool_clone = pool.clone();
            tokio::spawn(async move {
                info!("Handling JobOpened event:");
                info!("job: {:?}", job);
                info!("owner: {:?}", owner);
                info!("operator: {:?}", operator);
                info!("cp_url: {:?}", cp_url);
                if let Some(instance) = &metadata.instance {
                    info!("instance: {}", instance);
                }

                info!("Waiting for 3 minutes for enclave to start...");
                tokio::time::sleep(StdDuration::from_secs(180)).await;

                let instance_ip = match wait_for_ip_address(
                    &cp_url,
                    job.clone(),
                    metadata.region.as_deref().unwrap_or(""),
                )
                .await
                {
                    Ok(ip) => ip,
                    Err(e) => {
                        let error_msg = format!("Failed to get IP address: {}", e);
                        error!("{}", error_msg);

                        // Log error to database
                        let operator_str = format!("{:?}", operator);
                        let new_error = NewReachabilityError::new(
                            job.clone(),
                            operator_str,
                            "N/A".to_string(),
                            error_msg,
                        );

                        if let Ok(mut conn) = pool_clone.get() {
                            if let Err(db_err) = new_error.insert(&mut conn) {
                                error!("Failed to insert error into database: {}", db_err);
                            }
                        }
                        return;
                    }
                };

                info!("instance IP: {}", instance_ip);

                if check_reachability(&instance_ip).await {
                    info!("Instance is reachable");
                } else {
                    let error_msg = "Instance reachability test failed";
                    error!("{}", error_msg);

                    // Log error to database
                    let operator_str = format!("{:?}", operator);
                    let new_error = NewReachabilityError::new(
                        job.clone(),
                        operator_str,
                        instance_ip.clone(),
                        error_msg.to_string(),
                    );

                    if let Ok(mut conn) = pool_clone.get() {
                        if let Err(db_err) = new_error.insert(&mut conn) {
                            error!("Failed to insert error into database: {}", db_err);
                        }
                    }
                }

                // Call the refresh API to verify IP is available
                let refresh_url = format!(
                    "https://sk.arb1.marlin.org/operators/jobs/refresh/ArbOne/{}",
                    job
                );
                info!("Calling refresh API: {}", refresh_url);

                let client = reqwest::Client::new();
                match client.get(&refresh_url).send().await {
                    Ok(response) => {
                        match response.json::<serde_json::Value>().await {
                            Ok(json) => {
                                if json.get("ip").is_some() {
                                    info!("IP key found in refresh API response");
                                } else {
                                    let error_msg = "IP key NOT found in refresh API response";
                                    error!("{}", error_msg);

                                    // Log error to database
                                    let operator_str = format!("{:?}", operator);
                                    let new_error = NewOperatorEndpointError::new(
                                        job.clone(),
                                        operator_str,
                                        instance_ip.clone(),
                                        error_msg.to_string(),
                                    );

                                    if let Ok(mut conn) = pool_clone.get() {
                                        if let Err(db_err) = new_error.insert(&mut conn) {
                                            error!(
                                                "Failed to insert error into database: {}",
                                                db_err
                                            );
                                        }
                                    }
                                }
                            }
                            Err(e) => {
                                let error_msg =
                                    format!("Failed to parse refresh API response: {}", e);
                                error!("{}", error_msg);

                                // Log error to database
                                let operator_str = format!("{:?}", operator);
                                let new_error = NewOperatorEndpointError::new(
                                    job.clone(),
                                    operator_str,
                                    instance_ip.clone(),
                                    error_msg,
                                );

                                if let Ok(mut conn) = pool_clone.get() {
                                    if let Err(db_err) = new_error.insert(&mut conn) {
                                        error!("Failed to insert error into database: {}", db_err);
                                    }
                                }
                            }
                        }
                    }
                    Err(e) => {
                        let error_msg = format!("Failed to call refresh API: {}", e);
                        error!("{}", error_msg);

                        // Log error to database
                        let operator_str = format!("{:?}", operator);
                        let new_error = NewOperatorEndpointError::new(
                            job.clone(),
                            operator_str,
                            instance_ip.clone(),
                            error_msg,
                        );

                        if let Ok(mut conn) = pool_clone.get() {
                            if let Err(db_err) = new_error.insert(&mut conn) {
                                error!("Failed to insert error into database: {}", db_err);
                            }
                        }
                    }
                }
            });
        }

        // Update last checked block
        last_checked_block = current_block;
    }
}
