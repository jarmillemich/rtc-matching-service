use serde::{Deserialize, Serialize};

#[derive(Serialize, Deserialize, Debug)]
pub struct StartHosting {
    pub name: String,
}

#[derive(Serialize, Deserialize, Debug)]
pub struct JoinSessionRequest {
    pub session_name: String,
    pub client_name: String,
    pub rtc_offer: String,
    pub rtc_candidates: Vec<String>,
}

#[derive(Serialize, Deserialize, Debug)]
pub struct JoinSessionResponse {
    pub client_name: String,
    pub rtc_answer: String,
    pub rtc_candidates: Vec<String>,
}
