use aws_config::BehaviorVersion;
use aws_sdk_ses::error::DisplayErrorContext;
use aws_sdk_ses::types::{Body, Content, Destination, Message};
use aws_sdk_ses::Client as SesClient;
use lambda_runtime::{service_fn, Error, LambdaEvent};
use serde::{Deserialize, Serialize};
use reqwest::{header, Client as ReqClient};
use reqwest::header::{HeaderMap, CONTENT_TYPE};
use minijinja::{context, Environment};

#[derive(Debug, Deserialize)]
struct ContactFormDetails {
    name: String,
    email: String,
    telephone: String,
    detail: String,
    captcha: String,
}

#[derive(Debug, Serialize)]
struct RecaptchaRequest {
    secret: String,
    response: String,
}

#[derive(Debug, Deserialize)]
struct RecaptchaResponse {
    success: bool,
    #[serde(rename(deserialize = "error-codes"))]
    error_codes: Vec<String>,
    //challenge_ts: DateTime,  // timestamp of the challenge load (ISO format yyyy-MM-dd'T'HH:mm:ssZZ)
    //hostname: String,         // the hostname of the site where the reCAPTCHA was solved
}

#[tracing::instrument(skip_all)]
async fn verify_recaptcha(secret: String, response: String) -> Result<bool, reqwest::Error> {
    let client = ReqClient::new();

    let mut headers = HeaderMap::new();
    headers.append(CONTENT_TYPE, "application/json".parse().unwrap());

    let req = RecaptchaRequest { secret, response };

    let res: RecaptchaResponse = client.post("https://www.google.com/recaptcha/api/siteverify")
        .headers(headers)
        .json(&req)
        .send()
        .await?
        .json()
        .await?;
    tracing::info!("Google ReCAPTCHA Response {:?}", res);
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
        //return Ok(());
    }

    // Create Mail Body in HTML
    let mut env = Environment::new();
    env.add_template("mail_body.txt", 
        "<h1>{{name}}</h1>
        <p>E Mail: {{email}}</p>
        <p>Telefon: {{telephone}}</p>
        <p>Detail:</p>
        <p>{{detail}}</p>").unwrap();
    let template = env.get_template("mail_body.txt").unwrap();

    // Create Mail Object and Send by SESv1
    let email_address = std::env::var("forwardAddress").expect("forwardAddress Environment Variable must be set!");
    let email_destination = Destination::builder()
        .set_to_addresses(Some(vec![email_address]))
        .build();

    let subject = Content::builder()
        .set_data(Some("[csdbamberg.de] Kontakt".to_owned()))
        .charset("UTF-8")
        .build().expect("building Subject");

    let mail_body_html = template.render(context!(
        name => content_form.name,
        email => content_form.email,
        telephone => content_form.telephone,
        detail => content_form.detail)).unwrap();

    let detail = Content::builder()
        .set_data(Some(mail_body_html))
        .charset("UTF-8")
        .build().expect("building Detail");

    let body = Body::builder()
        .set_html(Some(detail))
        .build();

    let email_content = Message::builder()
        .set_subject(Some(subject))
        .set_body(Some(body))
        .build();

    let email_source = std::env::var("sourceAddress").expect("sourceAddress Environment Variable must be set!");
    let result = &client
        .send_email()
        .set_source(Some(email_source))
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
        // disabling time is handy because CloudWatch will add the ingestion time.
        .without_time()
        // remove the name of the function from every log entry
        .with_target(false)
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