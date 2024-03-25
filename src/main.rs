use aws_config::BehaviorVersion;
use aws_sdk_ses::error::DisplayErrorContext;
use aws_sdk_ses::types::{Body, Content, Destination, Message};
use lambda_http::http::StatusCode;
use lambda_http::{service_fn, Error, IntoResponse, Request, RequestExt, Response};
use serde::{Deserialize, Serialize};
use reqwest::Client as ReqClient;
use minijinja::{context, Environment};

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

#[tracing::instrument(skip(request), fields(req_id = %request.lambda_context().request_id))]
async fn send_mail(request: Request) -> Result<impl IntoResponse, Error> {
    tracing::info!("handling a request");

    // Initialize the client here to be able to reuse it across
    // different invocations.
    //
    // No extra configuration is needed as long as your Lambda has
    // the necessary permissions attached to its role.
    let config = aws_config::load_defaults(BehaviorVersion::latest()).await;
    let client = aws_sdk_ses::Client::new(&config);

    tracing::info!("Region: {}", config.region().unwrap());

    //let content_form = event.payload;
    let query_params = request.query_string_parameters_ref();
    if query_params.is_none() {
        tracing::error!("No Query Params in the API request!");
        //return Ok(());
    } else {
        tracing::info!("Contact Form Data {:?}", query_params);
    }

    let query_params = request.query_string_parameters_ref().unwrap();

    // SAFETY Unwrap will never fail. Missing Variable is checked by expect.
    let captcha = query_params.all("captcha").expect("Query Param: Captcha missing!").first().unwrap().to_string();

    // Check Google Captcha Response
    let captcha_secret = std::env::var("captchaSiteSecret").expect("captchaSiteSecret Environment Variable must be set!");
    let captcha_response = verify_recaptcha(captcha_secret, captcha).await?;

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
        .set_data(Some("[christopherkolbe.de] Kontakt".to_owned()))
        .charset("UTF-8")
        .build().expect("building Subject");

    // SAFETY Unwrap will never fail. Missing Variable is checked by expect.
    let name = query_params.all("name").expect("Query Param: Name missing!").first().unwrap().to_string();
    let email = query_params.all("email").expect("Query Param: Email missing!").first().unwrap().to_string();
    //let email = query_params.all("email").or_else(|| Some(vec!["Empty"])).unwrap().first().unwrap().to_string();
    let telephone = query_params.all("telephone").expect("Query Param: Telephone missing!").first().unwrap().to_string();
    let detail = query_params.all("detail").expect("Query Param: Detail missing!").first().unwrap().to_string();

    let mail_body_html = template.render(context!(
        name => name,
        email => email,
        telephone => telephone,
        detail => detail)).unwrap();

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

    let _response = Response::builder().body(lambda_http::Body::Empty).unwrap();
    let response = Response::builder()
        .status(StatusCode::SEE_OTHER)
        .header("Location", "/index.html")
        .body(lambda_http::Body::Empty).unwrap();

    Ok(response)

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

    lambda_http::run(service_fn(send_mail)).await?;

    Ok(())
}