use aws_config::BehaviorVersion;
use lambda_runtime::{service_fn, Error, LambdaEvent};
use serde::Deserialize;
use std::time::SystemTime;

#[derive(Deserialize)]
struct Request {
    body: String,
}

#[tracing::instrument(skip(event), fields(req_id = %event.context.request_id))]
async fn put_object(
    event: LambdaEvent<Request>,
) -> Result<(), Error> {
    tracing::info!("handling a request");
    // Generate a filename based on when the request was received.
    let timestamp = SystemTime::now()
        .duration_since(SystemTime::UNIX_EPOCH)
        .map(|n| n.as_secs())
        .expect("SystemTime before UNIX EPOCH, clock might have gone backwards");
    let request: String = event.payload.body;

    let filename = format!("{timestamp}-{request}.txt");

    tracing::info!(
        filename = %filename,
        "data successfully stored in S3",
    );
    Ok(())
}

#[tokio::main]
async fn main() -> Result<(), Error> {
    tracing_subscriber::fmt()
        .with_max_level(tracing::Level::INFO)
        // disable printing the name of the module in every log line.
        .with_target(false)
        // disabling time is handy because CloudWatch will add the ingestion time.
        .without_time()
        .init();

    // Initialize the client here to be able to reuse it across
    // different invocations.
    //
    // No extra configuration is needed as long as your Lambda has
    // the necessary permissions attached to its role.
    let _config = aws_config::load_defaults(BehaviorVersion::latest()).await;

    lambda_runtime::run(service_fn(|event: LambdaEvent<Request>| async {
        put_object(event).await
    }))
    .await
}