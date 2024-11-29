use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use worker::*;

// Claude: Define constants for API endpoints and bus stop information
const LTA_API_URL: &str = "http://datamall2.mytransport.sg/ltaodataservice/BusArrivalv2";

// Claude: Structs for parsing Telegram webhook updates
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

// Claude: Structs for parsing LTA bus arrival API response
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

// Claude: Structs for creating Telegram bot messages and buttons
#[derive(Serialize, Debug)]
struct TelegramButton {
    text: String,
    callback_data: String,
}

#[derive(Serialize, Debug)]
struct ReplyMarkup {
    inline_keyboard: Vec<Vec<TelegramButton>>,
}

#[derive(Serialize, Debug)]
enum TelegramMessageParseMode {
    MarkdownV2,
    // Add other parse modes as needed
}

#[derive(Serialize, Debug)]
#[serde(rename_all = "camelCase")]
enum TelegramMessageMethod {
    SendMessage,
    // Add other methods as needed
}

#[derive(Serialize, Debug)]
struct TelegramMessage {
    method: TelegramMessageMethod,
    chat_id: i64,
    text: String,
    parse_mode: TelegramMessageParseMode,
    reply_markup: Option<ReplyMarkup>,
}

// Claude: Struct to hold parsed bus timing information
struct BusTiming {
    service_no: String,
    next_arrival: String,
    next_arrival_2: String,
    next_arrival_3: String,
}

// Claude: Helper function to format bus arrival times
fn format_message(bus_timings: &[BusTiming]) -> String {
    let mut message = String::from("*Bus Timings:*\n\n");

    for bus in bus_timings {
        message.push_str(&format!(
            "*Service No:* {}\n\
             *Next Arrival:* {}\n\
             *Next Arrival 2:* {}\n\
             *Next Arrival 3:* {}\n\
             -----------------------------\n",
            bus.service_no, bus.next_arrival, bus.next_arrival_2, bus.next_arrival_3
        ));
    }

    message
}

// Claude: Function to send messages with inline keyboard buttons to Telegram
fn get_telegram_message_with_request_button(chat_id: i64, text: &str) -> TelegramMessage {
    // Claude: Create request button for bus timings
    let request_button = TelegramButton {
        text: "Request Bus Timings".to_string(),
        callback_data: "request_timings".to_string(),
    };

    // Claude: Prepare Telegram message with optional reply markup (buttons)
    TelegramMessage {
        method: TelegramMessageMethod::SendMessage,
        chat_id,
        text: text.to_string(),
        parse_mode: TelegramMessageParseMode::MarkdownV2,
        reply_markup: Some(ReplyMarkup {
            inline_keyboard: vec![vec![request_button]],
        }),
    }
}

// Claude: Function to fetch bus timings from LTA API
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

    // Claude: Fetch bus arrival data
    let data: BusArrivalResponse = resp.json().await.map_err(|e| e.to_string())?;

    console_debug!("LTA API response: {:#?}", data);

    // Claude: Get current timestamp for time calculations
    let now = Utc::now().timestamp();

    // Claude: Transform API response into our BusTiming struct
    let bus_timings = data
        .services
        .into_iter()
        .map(|service| {
            // Claude: Helper closure to format arrival times
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

            // Claude: Create BusTiming struct for each bus service
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

// Claude: Main request handler for processing Telegram webhook updates
async fn handle_request(
    mut req: Request,
    lta_api_key: &str,
    bus_stop_code: &str,
) -> Result<Response> {
    // Claude: Parse incoming webhook request body
    let update: TelegramUpdate = req.json().await?;

    console_log!("Incoming Request: {:#?}", update);

    // Claude: Extract chat ID from either callback query or message
    let chat_id = if let Some(callback_query) = &update.callback_query {
        Ok(callback_query.message.chat.id)
    } else if let Some(message) = &update.message {
        Ok(message.chat.id)
    } else {
        Err("No chat id found in request")
    }?;

    // Claude: Handle different types of incoming updates
    let telegram_message = match update.callback_query {
        Some(callback_query) => match callback_query.data.as_str() {
            "request_timings" => {
                // Claude: Fetch and send bus timings when button is pressed
                let bus_timings = fetch_bus_timings(lta_api_key, bus_stop_code).await?;
                let message = format_message(&bus_timings);

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

    let resp_body = serde_json::to_string(&telegram_message)?;
    Response::ok(resp_body)
}

// Claude: Main event handler for Cloudflare Workers
#[event(fetch)]
pub async fn main(req: Request, env: Env, _ctx: worker::Context) -> Result<Response> {
    console_error_panic_hook::set_once();

    // Claude: Retrieve API keys from environment secrets
    let lta_api_key = env.secret("LTA_API_KEY")?.to_string();
    let bus_stop_code = env
        .kv("bus_stops")?
        .get("code")
        .text()
        .await?
        .ok_or("No bus stop codes found")?;

    // Claude: Process the incoming request and handle any errors
    handle_request(req, &lta_api_key, &bus_stop_code)
        .await
        .map_err(|e| {
            console_error!("Error handling request: {}", e);
            e
        })
}
