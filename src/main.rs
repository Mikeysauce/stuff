use anyhow::{anyhow, Result};
use aws_sdk_lambda::{Client, Error};
use futures::{stream, StreamExt};
use octocrab::{models::repos::Content, params::repos::Reference, Octocrab};
use rayon::prelude::*;
use serde_json::Value;
use std::{collections::HashMap, env, str::FromStr, time::Instant};

#[tokio::main]
async fn main() -> octocrab::Result<(), anyhow::Error> {
    let token = env::var("MY_TOKEN").unwrap_or_else(|_| {
        eprintln!("MY_TOKEN environment variable not set");
        std::process::exit(1);
    });

    let config = aws_config::load_from_env().await;
    let aws_client = Client::new(&config);
    let start = Instant::now();

    // let details = match fetch_packagejson_details(token).await {
    //     Ok(details) => details,
    //     Err(e) => {
    //         println!("Failed to get package.json details: {}", e);
    //         return Ok(());
    //     }
    // };

    let deployed_lambdas = get_deployed_lambdas_list(&aws_client).await?;
    let got_lambdas = start.elapsed();
    println!("Got lambdas in {:?}", got_lambdas);

    println!("Deployed lambdas: {:#?}", deployed_lambdas);

    // for (name, version) in details {
    //     if let Some(fnc) = deployed_lambdas.iter().find(|fnc| fnc.name.contains(&name)) {
    //         println!("-------------------------------------");
    //         println!("Function: {}", fnc.name);
    //         println!("ARN: {}", fnc.arn);
    //         println!("Environment variables: {:#?}", fnc.env_vars);
    //         println!("Package.json version: {}", version);
    //         println!("-------------------------------------");
    //     } else {
    //         println!("Function with name {} not found", name);
    //     }
    // }

    Ok(())
}

#[derive(Debug)]
struct Lambda {
    name: String,
    env_vars: HashMap<String, String>,
    arn: String,
}

impl IntoIterator for Lambda {
    type Item = Lambda;
    type IntoIter = std::vec::IntoIter<Lambda>;

    fn into_iter(self) -> Self::IntoIter {
        vec![self].into_iter()
    }
}

async fn get_deployed_lambdas_list(client: &Client) -> Result<Vec<Lambda>, Error> {
    let mut function_deets: Vec<Lambda> = Vec::new();

    let list_functions_page = client.list_functions().into_paginator().items().send();

    let mut tasks = list_functions_page
        .map(|mut page| {
            let client = client.clone();
            async move {
                let page = page?;
                if let Some(env_vars) = page.environment() {
                    let lambda = Lambda {
                        name: page.function_name().unwrap_or_default().to_string(),
                        env_vars: env_vars.variables.as_ref().unwrap().clone(),
                        arn: page.function_arn().unwrap_or_default().to_string(),
                    };
                    Ok(lambda)
                } else {
                    Err(anyhow!("Failed to get environment variables for function {}", page.function_name().unwrap_or_default()))
                }
            }
        })
        .buffer_unordered(10);

        while let Some(task) = tasks.next().await {
            function_deets.extend(task);
        }

    Ok(function_deets)
}

async fn fetch_packagejson_details(
    token: String,
) -> Result<HashMap<std::string::String, Value>, anyhow::Error> {
    let octocrab = Octocrab::builder().personal_token(token).build()?;
    let repositories = vec![
        "Scotski",
        "scraper",
        "standen-node",
        "now-github-starter",
        "movies-front",
    ];

    let mut package_json_details: HashMap<String, Value> = HashMap::new();

    for repo in repositories {
        let package_json = match get_packagejson(octocrab.clone(), repo).await {
            Ok(package_json) => package_json,
            Err(e) => {
                println!("Failed to get package.json for repo {}: {}", repo, e);
                continue;
            }
        };

        if let Some(version) = package_json.get("version") {
            package_json_details.insert(repo.to_string(), version.clone());
        }
    }

    Ok(package_json_details)
}

async fn get_packagejson(
    octocrab: Octocrab,
    repo: &str,
) -> Result<HashMap<String, Value>, anyhow::Error> {
    let mut content = octocrab
        .repos("Mikeysauce", repo)
        .get_content()
        .path("package.json")
        .send()
        .await
        .map_err(|e| anyhow!("Failed to get package.json content: {}", e))?;

    let package_json_content = content
        .take_items()
        .first()
        .ok_or_else(|| anyhow!("Package JSON content not found"))?
        .decoded_content()
        .ok_or_else(|| anyhow!("Failed to decode package JSON content"))?;

    let package_json_deserialized: HashMap<String, Value> =
        serde_json::from_str(&package_json_content)
            .map_err(|e| anyhow!("Failed to parse package.json: {}", e))?;

    Ok(package_json_deserialized)
}
