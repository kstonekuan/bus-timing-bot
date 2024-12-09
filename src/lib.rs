use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use strum_macros::Display;
use worker::*;

const LTA_API_URL: &str = "http://datamall2.mytransport.sg/ltaodataservice/BusArrivalv2";
const TELEGRAM_API_URL: &str = "https://api.telegram.org/bot";

#[derive(Deserialize, Debug)]
struct Chat {
    id: i64,
}

#[derive(Deserialize, Debug)]
struct Message {
    chat: Chat,
    text: Option<String>,
}

#[derive(Deserialize, Debug)]
struct CallbackQuery {
    data: String,
    message: Message,
}

#[derive(Deserialize, Debug)]
struct TelegramUpdate {
    message: Option<Message>,
    callback_query: Option<CallbackQuery>,
}

#[derive(Serialize, Deserialize, Debug)]
struct TelegramButton {
    text: String,
    callback_data: String,
}

#[derive(Serialize, Deserialize, Debug)]
struct ReplyMarkup {
    inline_keyboard: Vec<Vec<TelegramButton>>,
}

#[derive(Serialize, Deserialize, Debug)]
enum TelegramMessageParseMode {
    MarkdownV2,
    // Add other parse modes as needed
}

#[derive(Display, Debug)]
#[strum(serialize_all = "camelCase")]
enum TelegramMessageMethod {
    SendMessage,
    // Add other methods as needed
}

#[derive(Serialize, Deserialize, Debug)]
struct TelegramMessage {
    chat_id: i64,
    text: String,
    parse_mode: TelegramMessageParseMode,
    reply_markup: Option<ReplyMarkup>,
}

#[derive(Deserialize, Debug)]
#[serde(rename_all = "PascalCase")]
struct BusArrivalResponse {
    services: Vec<BusService>,
}

#[derive(Deserialize, Debug)]
#[serde(rename_all = "PascalCase")]
struct BusService {
    service_no: String,
    next_bus: BusArrival,
    next_bus2: BusArrival,
    next_bus3: BusArrival,
}

#[derive(Deserialize, Debug)]
#[serde(rename_all = "PascalCase")]
struct BusArrival {
    estimated_arrival: Option<String>,
}
struct BusTiming {
    service_no: String,
    next_arrival: String,
    next_arrival_2: String,
    next_arrival_3: String,
}

fn format_bus_timings_message(bus_timings: Vec<BusTiming>) -> String {
    let timings: Vec<String> = bus_timings
        .into_iter()
        .map(|bus| {
            format!(
                "*Service No:* {}\n\
             *Next Arrival:* {}\n\
             *Next Arrival 2:* {}\n\
             *Next Arrival 3:* {}\n\
             \\-\\-\\-\\-\\-\\-\\-\\-\\-\\-\\-\\-\\-\\-\\-\\-\\-\\-\\-\\-\\-\\-\\-\\-\\-\\-\n",
                bus.service_no, bus.next_arrival, bus.next_arrival_2, bus.next_arrival_3
            )
        })
        .collect();

    format!("*Bus Timings:*\n\n{}", timings.join("\n"))
}

fn get_telegram_message_with_request_button(chat_id: i64, text: &str) -> TelegramMessage {
    // Claude: Create request button for bus timings
    let request_button = TelegramButton {
        text: "Request Bus Timings".to_string(),
        callback_data: "request_timings".to_string(),
    };

    // Claude: Prepare Telegram message with optional reply markup (buttons)
    TelegramMessage {
        chat_id,
        text: text.to_string(),
        parse_mode: TelegramMessageParseMode::MarkdownV2,
        reply_markup: Some(ReplyMarkup {
            inline_keyboard: vec![vec![request_button]],
        }),
    }
}

async fn fetch_bus_timings(lta_api_key: &str, bus_stop_code: &str) -> Result<Vec<BusTiming>> {
    // Claude: Prepare headers for LTA API request
    let client = reqwest::Client::new();
    let resp = client
        .get(format!("{}?BusStopCode={}", LTA_API_URL, bus_stop_code))
        .header("AccountKey", lta_api_key)
        .header("accept", "application/json")
        .send()
        .await
        .map_err(|e| e.to_string())?;

    let bus_arrival_resp: BusArrivalResponse = resp.json().await.map_err(|e| e.to_string())?;

    console_debug!("LTA API response: {:#?}", bus_arrival_resp);

    let now = Utc::now().timestamp();

    let bus_timings = bus_arrival_resp
        .services
        .into_iter()
        .map(|service| {
            let format_arrival = |arrival: &BusArrival| {
                if let Some(time) = &arrival.estimated_arrival {
                    let arrival_time = DateTime::parse_from_rfc3339(time)
                        .map(|dt| dt.timestamp())
                        .unwrap_or(0);
                    let diff_minutes = (arrival_time - now) / 60;
                    if diff_minutes <= 0 {
                        "ARR".to_string()
                    } else {
                        format!("{} min", diff_minutes)
                    }
                } else {
                    "NIL".to_string()
                }
            };

            BusTiming {
                service_no: service.service_no,
                next_arrival: format_arrival(&service.next_bus),
                next_arrival_2: format_arrival(&service.next_bus2),
                next_arrival_3: format_arrival(&service.next_bus3),
            }
        })
        .collect();

    Ok(bus_timings)
}

async fn send_message(
    telegram_api_key: &str,
    telegram_message: &TelegramMessage,
) -> Result<reqwest::Response> {
    // Claude: Prepare Telegram API request
    let client = reqwest::Client::new();
    let method = TelegramMessageMethod::SendMessage.to_string();
    let telegram_url = format!("{}{}/{}", TELEGRAM_API_URL, telegram_api_key, method);
    console_debug!("Telegram API URL: {}", telegram_url);
    client
        .post(telegram_url)
        .header("Content-Type", "application/json")
        .json(telegram_message)
        .send()
        .await
        .map_err(|e| Error::from(e.to_string()))
}

async fn handle_request(
    mut req: Request,
    lta_api_key: &str,
    telegram_api_key: &str,
    bus_stop_code: &str,
    allowed_chat_id: &str,
) -> Result<Response> {
    let update: TelegramUpdate = req.json().await?;

    console_log!("Incoming Request: {:#?}", update);

    let chat_id = if let Some(callback_query) = &update.callback_query {
        Ok(callback_query.message.chat.id)
    } else if let Some(message) = &update.message {
        Ok(message.chat.id)
    } else {
        Err("No chat id found in request")
    }?;

    // Check if chat ID is allowed
    let () = if chat_id.to_string() == allowed_chat_id {
        Ok(())
    } else {
        Err(format!("Chat ID {} is not allowed", chat_id))
    }?;

    let telegram_message = match update.callback_query {
        Some(callback_query) => match callback_query.data.as_str() {
            "request_timings" => {
                // Claude: Fetch and send bus timings when button is pressed
                let bus_timings = fetch_bus_timings(lta_api_key, bus_stop_code).await?;
                let message = format_bus_timings_message(bus_timings);

                console_log!("Sending message: {}", message);

                Ok(get_telegram_message_with_request_button(chat_id, &message))
            }
            data => Err(format!(
                "Invalid callback query, expected \"request_timings\" but got \"{}\"",
                data
            )),
        },
        None => match update.message {
            None => Err("No message found in request".to_string()),
            Some(message) => match message.text.as_deref() {
                None => Err("No message found in request".to_string()),
                Some("/start") => {
                    let welcome_message = "Welcome to the Bus Arrival Bot! Click the button below to request bus timings:";
                    Ok(get_telegram_message_with_request_button(
                        chat_id,
                        welcome_message,
                    ))
                }
                Some(text) => Err(format!(
                    "Invalid message, expected \"/start\" but got \"{}\"",
                    text
                )),
            },
        },
    }?;

    console_log!("Outgoing Response: {:#?}", telegram_message);
    let resp = send_message(telegram_api_key, &telegram_message).await?;
    let resp_json = resp.text().await.map_err(|e| e.to_string())?;
    console_debug!("Telegram API response: {:#?}", resp_json);

    Response::ok("OK")
}

#[event(fetch)]
pub async fn main(req: Request, env: Env, _ctx: worker::Context) -> Result<Response> {
    console_error_panic_hook::set_once();

    let lta_api_key = env.secret("LTA_API_KEY")?.to_string();
    let telegram_api_key = env.secret("TELEGRAM_API_KEY")?.to_string();
    let kv = env.kv("bus_stops")?;
    let bus_stop_code = kv
        .get("code")
        .text()
        .await?
        .ok_or("No bus stop codes found")?;
    // TODO: Using another KV namespace for this
    let allowed_chat_id = kv
        .get("chat_id")
        .text()
        .await?
        .ok_or("No allowed chat id found")?;

    handle_request(
        req,
        &lta_api_key,
        &telegram_api_key,
        &bus_stop_code,
        &allowed_chat_id,
    )
    .await
    .map_err(|e| {
        console_error!("Error handling request: {}", e);
        e
    })
}
