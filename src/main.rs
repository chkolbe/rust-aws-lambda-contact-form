use aws_config::BehaviorVersion;
use aws_sdk_ses::error::DisplayErrorContext;
use aws_sdk_ses::types::{Body, Content, Destination, Message};
use aws_sdk_ses::Client as SesClient;
use lambda_runtime::{service_fn, Error, LambdaEvent};
use serde::{Deserialize, Serialize};
use reqwest::Client as ReqClient;

#[derive(Debug, Deserialize)]
struct ContactFormDetails {
    name: String,
    email: String,
    telephone: String,
    detail: String,
    #[serde(rename(deserialize = "g-recaptcha-response"))]
    captcha: String,
}

#[derive(Serialize)]
struct RecaptchaRequest {
    secret: String,
    response: String,
}

#[derive(Deserialize)]
struct RecaptchaResponse {
    success: bool,
}

#[tracing::instrument(skip(secret, response), fields(response_id = %response))]
async fn verify_recaptcha(secret: String, response: String) -> Result<bool, reqwest::Error> {
    let client = ReqClient::new();
    let req = RecaptchaRequest { secret, response };
    let res: RecaptchaResponse = client.post("https://www.google.com/recaptcha/api/siteverify")
        .json(&req)
        .send()
        .await?
        .json()
        .await?;
    Ok(res.success)
}

#[tracing::instrument(skip(event, client), fields(req_id = %event.context.request_id))]
async fn send_mail(
    event: LambdaEvent<ContactFormDetails>,
    client: &SesClient,
) -> Result<(), Error> {
    tracing::info!("handling a request");

    let content_form = event.payload;
    tracing::info!("Contact Form Data {:?}", content_form);
    let _ctx = event.context;

    // Check Google Captcha Response
    let captcha_secret = std::env::var("captchaSiteSecret").expect("captchaSiteSecret Environment Variable must be set!");
    let captcha_response = verify_recaptcha(captcha_secret, content_form.captcha).await?;

    if captcha_response {
        tracing::info!("Google recaptcha Response Ok.");
    } else {
        tracing::error!("Google recaptcha Response Nok!");
        return Ok(());
    }

    // Create Mail Object and Send by SESv1
    let email_destination = Destination::builder()
        .set_to_addresses(Some(vec!["kontakt@christopherkolbe.de".to_owned()]))
        .build();

    let subject = Content::builder()
        .set_data(Some("[christopherkolbe.de] Kontakt".to_owned()))
        .charset("UTF-8")
        .build().expect("building Subject");

    let detail = Content::builder()
        .set_data(Some(content_form.detail))
        .charset("UTF-8")
        .build().expect("building Detail");

    let body = Body::builder()
        .set_text(Some(detail))
        .build();

    let email_content = Message::builder()
        .set_subject(Some(subject))
        .set_body(Some(body))
        .build();

    let result = &client
        .send_email()
        .set_source(Some("info@christopherkolbe.de".to_owned()))
        .set_destination(Some(email_destination))
        .set_message(Some(email_content))
        .send().await;

    match result {
        Ok(output) => tracing::info!("Mail send with Message_ID: {}", output.message_id),
        Err(error) => {
            tracing::error!("Error send Mail by SESv1 failed!");
            tracing::error!("{}", DisplayErrorContext(error));
        },
    }

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
    let config = aws_config::load_defaults(BehaviorVersion::latest()).await;
    let client = aws_sdk_ses::Client::new(&config);

    tracing::info!("Region: {}", config.region().unwrap());

    lambda_runtime::run(service_fn( |event: LambdaEvent<ContactFormDetails>| async {
        send_mail(event, &client).await
    }))
    .await
}