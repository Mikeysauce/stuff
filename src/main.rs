use anyhow::{anyhow, Result};
use aws_sdk_lambda::{Client, Error};
use octocrab::{models::repos::Content, params::repos::Reference, Octocrab};
use serde_json::Value;
use std::{collections::HashMap, env, str::FromStr};

#[tokio::main]
async fn main() -> octocrab::Result<(), anyhow::Error> {
    let token = env::var("MY_TOKEN").unwrap_or_else(|_| {
        eprintln!("MY_TOKEN environment variable not set");
        std::process::exit(1);
    });

    let config = aws_config::load_from_env().await;

    let aws_client = Client::new(&config);

    let details = match fetch_packagejson_details(token).await {
        Ok(details) => details,
        Err(e) => {
            println!("Failed to get package.json details: {}", e);
            return Ok(());
        }
    };

    let deployed_lambdas = get_deployed_lambdas_list(&aws_client).await?;

    for (name, version) in details {
        if let Some(fnc) = deployed_lambdas.iter().find(|fnc| fnc.name.contains(&name)) {
            println!("-------------------------------------");
            println!("Function: {}", fnc.name);
            println!("ARN: {}", fnc.arn);
            println!("Environment variables: {:#?}", fnc.env_vars);
            println!("Package.json version: {}", version);
            println!("-------------------------------------");
        } else {
            println!("Function with name {} not found", name);
        }
    }

    Ok(())
}

struct Lambda {
    name: String,
    env_vars: HashMap<String, String>,
    arn: String,
}

async fn get_deployed_lambdas_list(client: &Client) -> Result<Vec<Lambda>, Error> {
    let mut next_marker: Option<String> = None;
    let mut total_functions = 0;
    let mut function_deets: Vec<Lambda> = Vec::new();

    loop {
        let mut request = client.list_functions();
        if let Some(marker) = &next_marker {
            request = request.marker(marker);
        }
        let resp = request.send().await?;
        let resp_functions = resp.functions.unwrap_or_default();
        total_functions += resp_functions.len();

        let functions = resp_functions
            .iter()
            .filter(|fnc| fnc.environment().is_some())
            .map(|func| {
                let env_vars = func.environment().unwrap().variables().unwrap().clone();
                let name = func.function_name().unwrap().to_string();
                let arn = func.function_arn().unwrap().to_string();
                Lambda {
                    name,
                    env_vars,
                    arn,
                }
            });

        function_deets.extend(functions);

        if let Some(marker) = resp.next_marker {
            next_marker = Some(marker);
        } else {
            break;
        }
    }

    if total_functions > 0 {
        println!(
            "Filtered {} function(s) down to {}",
            total_functions,
            function_deets.len()
        );
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
