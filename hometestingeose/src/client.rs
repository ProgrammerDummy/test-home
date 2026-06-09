use anyhow::{anyhow, Context, Result};
use reqwest::{multipart, Client, ClientBuilder};
use serde::Deserialize;
use tracing::info;

//since all of these are gonna come from the JSON key from the OpenPLC, i will need to use deserialize
//also to print, debug is needed as well

#[derive(Debug, Deserialize)]
pub struct LoginResponse {
    pub access_token: String,
}

#[derive(Debug, Deserialize)]
pub struct UploadResponse {
    #[serde(rename = "UploadFileFail")] //JSON is pascal case but rust is snake case for field 
    pub upload_fail: String,
    #[serde(rename = "CompilationStatus")] 
    pub compilation_status: String,
}

//look at: https://github.com/Autonomy-Logic/openplc-runtime/blob/main/docs/API.md
//https://github.com/Autonomy-Logic/openplc-runtime/blob/main/docs/EDITOR_INTEGRATION.md
//modbus plugin: https://github.com/Autonomy-Logic/openplc-runtime/blob/main/core/src/drivers/README.md

#[derive(Debug, Deserialize)]
pub struct CompilationStatus {
    pub status: String,
    pub logs: Option<Vec<String>>,
    pub exit_code: Option<i32>, 
    //during compilation before the simulation actually runs, logs and exit_code will not exist
    //therefore they should be options 
    //the JSON keys here are also gonna be snake_case according to documentation so no renaming needed
}


#[derive(Debug, Deserialize)]
pub struct PlcStatusResponse {
    pub status: String, //JSON file here should also be snake_case so no renaming 
}

/*
for each HTTP request i am making to OpenPLC server, it will send back a JSON string of text
for a specific request i make, it sends back a specific text as well
*/

#[derive(Clone, PartialEq, Eq)]
pub enum PlcState { //as the name implies, it is an enum represnting the possible states that openPLC can return
    Empty,
    Init,
    Running,
    Stopped,
    Error,
    Unexpected(String), //in case it gives me some bullshit, i should record it
}

impl From<String> for PlcState { //i need to convert from uppercase of the JSON to my enum
    fn from(s: String) -> Self {
        match s.as_str() {
            "EMPTY" => PlcState::Empty,
            "INIT" => PlcState::Init,
            "RUNNING" => PlcState::Running,
            "STOPPED" => PlcState::Stopped,
            "ERROR" => PlcState::Error,
            _ => PlcState::Unexpected(s), //anything else is unexpected
        }
    }
}

pub struct PlcClient {
    reqwest_http_client: reqwest::Client,
    base_api_url: String,
    auth_username: String,
    auth_password: String,
    jwt_token: Option<String>
}

impl PlcClient {
    pub fn new(api_url: &str, username: &str, password: &str) -> Result<Self> {
        let http = ClientBuilder::new().danger_accept_invalid_certs(true).build().context("Failed to create PlcClient")?;
        Ok(PlcClient {
            reqwest_http_client: http,
            base_api_url: api_url.to_string(),
            auth_username: username.to_string(),
            auth_password: password.to_string(),
            jwt_token: None,
        })
    }

    pub fn url(&self, path: &str) -> String {
        format!("{}{}", self.base_api_url, path)
        //to concatenate the base url and the path (specific command) to a full url request
    } 

    pub fn auth_header(&self) -> String {
        format!("Bearer {}", self.jwt_token.as_deref().expect("JWT token does not exist currently, must login first"))
    } 
}