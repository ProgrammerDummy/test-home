// =============================================================
// CLIENT.RS — COMPLETE REFERENCE IMPLEMENTATION
// =============================================================
// USE THIS ONLY IF YOU ARE COMPLETELY STUCK.
// Every line has a comment explaining why it is written that way.
// Reading this without attempting the code yourself first will
// rob you of the learning. You have been warned.
// =============================================================

use anyhow::{anyhow, Context, Result};
use reqwest::{multipart, Client, ClientBuilder};
use serde::Deserialize;
use tracing::info;

// --- Response Structs ---
// Each struct mirrors exactly one JSON response shape from OpenPLC.
// #[derive(Debug, Deserialize)] on every one:
//   Debug      → lets you print it with {:?} in logs
//   Deserialize → lets serde parse JSON into it automatically

#[derive(Debug, Deserialize)]
pub struct LoginResponse {
    pub access_token: String,
}

#[derive(Debug, Deserialize)]
pub struct UploadResponse {
    // OpenPLC sends "UploadFileFail" (PascalCase) but we store it
    // as upload_fail (snake_case). serde(rename) maps between them.
    #[serde(rename = "UploadFileFail")]
    pub upload_fail: String,
    #[serde(rename = "CompilationStatus")]
    pub compilation_status: String,
}

#[derive(Debug, Deserialize)]
pub struct CompilationStatus {
    pub status: String,
    // Option<T> because these fields are null/absent while compiling.
    // serde automatically sets them to None when absent or null.
    pub logs: Option<Vec<String>>,
    pub exit_code: Option<i32>,
}

#[derive(Debug, Deserialize)]
pub struct PlcStatusResponse {
    pub status: String,
}

// --- PlcState Enum ---
// Typed representation of OpenPLC's state machine.
// We convert raw strings into this so the rest of the code can
// pattern match cleanly instead of comparing strings everywhere.
//
// Clone  → needed to send through channels later
// PartialEq + Eq → needed for == comparisons and some stdlib traits
// Debug  → needed to print with {:?}
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PlcState {
    Empty,
    Init,
    Running,
    Stopped,
    Error,
    Unexpected(String), // catches any state string we do not recognize
}

// Implementing From<String> lets you write PlcState::from(some_string)
// to convert OpenPLC's raw status strings into typed enum variants.
impl From<String> for PlcState {
    fn from(s: String) -> Self {
        // match on &str not String — pattern literals are &str
        // s.as_str() borrows the String as &str for the match
        match s.as_str() {
            "EMPTY"   => PlcState::Empty,
            "INIT"    => PlcState::Init,
            "RUNNING" => PlcState::Running,
            "STOPPED" => PlcState::Stopped,
            "ERROR"   => PlcState::Error,
            // _ catches everything else
            // s is moved into Unexpected — still valid because
            // as_str() only borrowed it, the original s is still owned
            _ => PlcState::Unexpected(s),
        }
    }
}

// --- PlcClient Struct ---
// Holds the HTTP client and auth state.
// Fields are private — outside code uses methods, not direct field access.
// jwt_token is Option<String> because it does not exist until after login().
pub struct PlcClient {
    reqwest_http_client: Client,
    base_api_url: String,
    auth_username: String,
    auth_password: String,
    jwt_token: Option<String>,
}

impl PlcClient {

    // Constructor. Takes &str not String — the function does not need
    // to own the strings, it reads them and stores copies.
    // Returns Result<Self> because ClientBuilder::build() can fail.
    pub fn new(api_url: &str, username: &str, password: &str) -> Result<Self> {
        // danger_accept_invalid_certs(true) disables TLS cert verification.
        // Necessary because OpenPLC uses a self-signed cert that is not
        // in any trusted CA store. Named "danger" to make you think twice.
        let http = ClientBuilder::new()
            .danger_accept_invalid_certs(true)
            .build()
            .context("failed to build HTTP client")?;
            // ? propagates the error up if build() fails

        Ok(Self {
            reqwest_http_client: http,
            base_api_url: api_url.to_string(),   // &str → owned String
            auth_username: username.to_string(),
            auth_password: password.to_string(),
            jwt_token: None, // no token until login() is called
        })
    }

    // Private helper — builds a full URL from an endpoint path.
    // Used by every method so we do not repeat the format!() everywhere.
    fn url(&self, path: &str) -> String {
        format!("{}{}", self.base_api_url, path)
        // e.g. "https://localhost:8443/api" + "/login"
        //    = "https://localhost:8443/api/login"
    }

    // Private helper — builds the Authorization header value.
    // Panics if called before login() — that is a programming error,
    // not a runtime condition, so panic is appropriate here.
    fn auth_header(&self) -> String {
        format!(
            "Bearer {}",
            // as_deref() converts Option<String> to Option<&str>
            // expect() unwraps it or panics with the message
            self.jwt_token
                .as_deref()
                .expect("JWT token not set — login() must be called first")
        )
    }

    // --- create_user() ---
    // Registers credentials with OpenPLC's database.
    // Safe to call every startup — 409 (user exists) is treated as success.
    // &self not &mut self — does not modify the struct.
    pub async fn create_user(&self) -> Result<()> {
        // serde_json::json!() builds a JSON value inline.
        // Passed as a reference to .json() — the & is required.
        let body = serde_json::json!({
            "username": self.auth_username,
            "password": self.auth_password,
            "role": "admin"
        });

        // Build and send the request.
        // .context() on .send().await describes a network-level failure.
        let response = self.reqwest_http_client
            .post(self.url("/create-user"))
            .json(&body)
            .send()
            .await
            .context("create-user request failed — is OpenPLC running?")?;

        // Match manually instead of .error_for_status() because 409 is ok.
        // .as_u16() converts the status code to a plain number for matching.
        match response.status().as_u16() {
            201 => {
                info!("User created successfully");
                Ok(())
            }
            409 => {
                // User already exists from a previous run — totally fine.
                info!("User already exists, continuing");
                Ok(())
            }
            code => {
                // Anything else is unexpected — return an error.
                // anyhow!() constructs an error from a format string.
                Err(anyhow!("create-user returned unexpected status {}", code))
            }
        }
    }

    // --- login() ---
    // Authenticates and stores the JWT token.
    // &mut self because it writes to self.jwt_token.
    pub async fn login(&mut self) -> Result<()> {
        let body = serde_json::json!({
            "username": self.auth_username,
            "password": self.auth_password,
        });

        // Chain of operations — each ? propagates on failure with context.
        // Three separate failure points, three separate context messages.
        let response = self.reqwest_http_client
            .post(self.url("/login"))
            .json(&body)
            .send()
            .await
            .context("login request failed")?
            // error_for_status() returns Err for any 4xx/5xx response.
            // Safe to use here — all non-2xx responses are real errors.
            .error_for_status()
            .context("login rejected — check credentials")?
            // .json::<T>() deserializes the response body as JSON into T.
            // Needs its own .await because reading the body is also async.
            .json::<LoginResponse>()
            .await
            .context("failed to parse login response")?;

        // Store the token for all future requests.
        self.jwt_token = Some(response.access_token);
        info!("Authenticated with OpenPLC runtime");
        Ok(())
    }

    // --- upload_program() ---
    // Sends the program zip to OpenPLC as multipart form data.
    // Takes Vec<u8> (raw bytes) — reading from disk is deploy.rs's job.
    pub async fn upload_program(&self, zip_bytes: Vec<u8>) -> Result<()> {
        // Build a multipart Part from the raw bytes.
        // file_name sets the filename metadata in the multipart envelope.
        // mime_str sets the Content-Type for this part.
        // mime_str() returns Result so it needs ? to propagate errors.
        let part = multipart::Part::bytes(zip_bytes)
            .file_name("program.zip")
            .mime_str("application/zip")
            .context("failed to build multipart part")?;

        // Wrap the part in a Form. The field name "file" must match
        // exactly what OpenPLC's server-side code expects.
        let form = multipart::Form::new().part("file", part);

        // .multipart() instead of .json() — this is a file upload.
        // .header() attaches the JWT auth token.
        let response = self.reqwest_http_client
            .post(self.url("/upload-file"))
            .header("Authorization", self.auth_header())
            .multipart(form)
            .send()
            .await
            .context("upload request failed")?
            .error_for_status()
            .context("server rejected the upload request")?
            .json::<UploadResponse>()
            .await
            .context("failed to parse upload response")?;

        // Even on HTTP 200, the upload might have failed server-side.
        // OpenPLC signals this with a non-empty UploadFileFail string.
        if !response.upload_fail.is_empty() {
            return Err(anyhow!(
                "OpenPLC rejected the program: {}",
                response.upload_fail
            ));
        }

        info!("Program uploaded, compilation starting");
        Ok(())
    }

    // --- wait_for_compilation() ---
    // Polls /compilation-status until SUCCESS or FAILED.
    // Takes Duration parameters — the caller decides the values.
    pub async fn wait_for_compilation(
        &self,
        poll_interval: std::time::Duration,
        timeout: std::time::Duration,
    ) -> Result<()> {
        // Calculate the deadline once upfront.
        // tokio::time::Instant not std::time::Instant — async context.
        let deadline = tokio::time::Instant::now() + timeout;

        loop {
            // Check timeout at the top of every iteration.
            if tokio::time::Instant::now() > deadline {
                return Err(anyhow!(
                    "compilation timed out after {} seconds",
                    timeout.as_secs()
                ));
            }

            let status = self.reqwest_http_client
                .get(self.url("/compilation-status"))
                .header("Authorization", self.auth_header())
                .send()
                .await
                .context("compilation-status request failed")?
                .error_for_status()?
                .json::<CompilationStatus>()
                .await
                .context("failed to parse compilation status")?;

            info!("Compilation status: {}", status.status);

            match status.status.as_str() {
                "SUCCESS" => {
                    info!("Compilation succeeded");
                    return Ok(());
                }
                "FAILED" => {
                    // Include the build logs in the error message.
                    // unwrap_or_default() gives empty Vec if logs is None.
                    // join("\n") combines log lines into one string.
                    let logs = status
                        .logs
                        .unwrap_or_default()
                        .join("\n");
                    return Err(anyhow!("compilation failed:\n{}", logs));
                }
                // IDLE, UNZIPPING, COMPILING — still in progress.
                // Sleep before next poll. tokio::time::sleep yields
                // to the scheduler instead of blocking the thread.
                _ => {
                    tokio::time::sleep(poll_interval).await;
                }
            }
        }
    }

    // --- start_plc() ---
    // Tells the runtime to begin executing the compiled program.
    // Note: GET not POST — that is just how OpenPLC designed this endpoint.
    pub async fn start_plc(&self) -> Result<()> {
        let response = self.reqwest_http_client
            .get(self.url("/start-plc"))
            .header("Authorization", self.auth_header())
            .send()
            .await
            .context("start-plc request failed")?
            .error_for_status()?
            .json::<PlcStatusResponse>()
            .await
            .context("failed to parse start-plc response")?;

        info!("Start PLC response: {}", response.status);

        // Check for known error states in the response string.
        // .contains() handles slight variations in the exact wording.
        if response.status.contains("No PLC program") {
            return Err(anyhow!(
                "runtime has no compiled program — did compilation succeed?"
            ));
        }
        if response.status.contains("No response") {
            return Err(anyhow!(
                "C++ runtime process is not responding"
            ));
        }

        Ok(())
    }

    // --- get_status() ---
    // Returns the current PLC state as a typed enum.
    // Simplest method — GET with auth, deserialize, convert, return.
    pub async fn get_status(&self) -> Result<PlcState> {
        let response = self.reqwest_http_client
            .get(self.url("/status"))
            .header("Authorization", self.auth_header())
            .send()
            .await
            .context("status request failed")?
            .error_for_status()?
            .json::<PlcStatusResponse>()
            .await
            .context("failed to parse status response")?;

        // Convert the raw String from OpenPLC into our typed PlcState.
        // PlcState::from() uses the From<String> impl we wrote earlier.
        let state = PlcState::from(response.status);
        info!("PLC state: {:?}", state);
        Ok(state)
    }
}