#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let sdk_config = aws_config::load_from_env().await;
    let client = aws_sdk_dynamodb::Client::new(&sdk_config);
    let res = client
        .list_tables()
        .limit(10)
        .send()
        .await?;
    println!("Current DynamoDB tables: {:?}", res.table_names());
    Ok(())
}