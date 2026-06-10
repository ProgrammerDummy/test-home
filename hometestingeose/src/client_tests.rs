// =============================================================
// CLIENT.RS — TEST SUITE
// =============================================================
// HOW TO USE THIS FILE:
//
// These tests use a mock HTTP server (wiremock) to simulate
// OpenPLC's REST API responses. You do not need a real OpenPLC
// instance running to run these tests.
//
// First, add wiremock to your Cargo.toml under [dev-dependencies]:
//
//   [dev-dependencies]
//   wiremock = "0.6"
//   tokio-test = "0.4"
//
// Then copy the relevant test section into the bottom of your
// client.rs file inside a #[cfg(test)] mod block.
//
// Run all tests:
//   cargo test
//
// Run a specific test:
//   cargo test test_create_user_first_run
//
// Run with output visible:
//   cargo test -- --nocapture
//
// =============================================================
// PASTE THIS ENTIRE BLOCK AT THE BOTTOM OF YOUR client.rs
// =============================================================

#[cfg(test)]
mod tests {
    use super::*;
    use wiremock::matchers::{method, path};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    // ----------------------------------------------------------
    // HELPER: builds a PlcClient pointed at the mock server.
    // Used by every test so you do not repeat this setup.
    // ----------------------------------------------------------
    async fn make_client(server: &MockServer) -> PlcClient {
        PlcClient::new(&server.uri(), "admin", "secret")
            .expect("failed to build client")
    }

    // ----------------------------------------------------------
    // HELPER: builds a PlcClient that is already "logged in"
    // by setting a fake JWT token. Used for tests that need
    // auth_header() to work without actually calling login().
    // ----------------------------------------------------------
    async fn make_authed_client(server: &MockServer) -> PlcClient {
        let mut client = make_client(server).await;
        client.jwt_token = Some("fake.jwt.token".to_string());
        client
    }

    // ==========================================================
    // SECTION 1: create_user() tests
    // Copy this section to test create_user() in isolation.
    // ==========================================================

    #[tokio::test]
    async fn test_create_user_first_run() {
        // Simulates first container start — OpenPLC returns 201 Created
        let server = MockServer::start().await;

        Mock::given(method("POST"))
            .and(path("/create-user"))
            .respond_with(ResponseTemplate::new(201))
            .mount(&server)
            .await;

        let client = make_client(&server).await;
        let result = client.create_user().await;

        assert!(result.is_ok(), "201 should be treated as success");
    }

    #[tokio::test]
    async fn test_create_user_already_exists() {
        // Simulates every restart after first — user is in the DB already
        let server = MockServer::start().await;

        Mock::given(method("POST"))
            .and(path("/create-user"))
            .respond_with(ResponseTemplate::new(409))
            .mount(&server)
            .await;

        let client = make_client(&server).await;
        let result = client.create_user().await;

        assert!(result.is_ok(), "409 should be treated as success — user already exists");
    }

    #[tokio::test]
    async fn test_create_user_server_error() {
        // Simulates OpenPLC being broken — 500 should be an error
        let server = MockServer::start().await;

        Mock::given(method("POST"))
            .and(path("/create-user"))
            .respond_with(ResponseTemplate::new(500))
            .mount(&server)
            .await;

        let client = make_client(&server).await;
        let result = client.create_user().await;

        assert!(result.is_err(), "500 should be an error");
    }

    // ==========================================================
    // SECTION 2: login() tests
    // Copy this section to test login() in isolation.
    // ==========================================================

    #[tokio::test]
    async fn test_login_success() {
        let server = MockServer::start().await;

        Mock::given(method("POST"))
            .and(path("/login"))
            .respond_with(
                ResponseTemplate::new(200)
                    .set_body_json(serde_json::json!({
                        "access_token": "eyJhbGciOiJIUzI1NiIsInR5cCI6IkpXVCJ9.test"
                    }))
            )
            .mount(&server)
            .await;

        let mut client = make_client(&server).await;

        // Before login, token should be None
        assert!(client.jwt_token.is_none(), "token should be None before login");

        let result = client.login().await;
        assert!(result.is_ok(), "login should succeed");

        // After login, token should be Some
        assert!(client.jwt_token.is_some(), "token should be Some after login");
        assert_eq!(
            client.jwt_token.as_deref().unwrap(),
            "eyJhbGciOiJIUzI1NiIsInR5cCI6IkpXVCJ9.test"
        );
    }

    #[tokio::test]
    async fn test_login_wrong_credentials() {
        let server = MockServer::start().await;

        Mock::given(method("POST"))
            .and(path("/login"))
            .respond_with(ResponseTemplate::new(401))
            .mount(&server)
            .await;

        let mut client = make_client(&server).await;
        let result = client.login().await;

        assert!(result.is_err(), "401 should be an error");
        // Token should still be None after failed login
        assert!(client.jwt_token.is_none(), "token should still be None after failed login");
    }

    #[tokio::test]
    async fn test_login_stores_token() {
        // Verifies the token is correctly stored and usable
        let server = MockServer::start().await;
        let expected_token = "my.test.token";

        Mock::given(method("POST"))
            .and(path("/login"))
            .respond_with(
                ResponseTemplate::new(200)
                    .set_body_json(serde_json::json!({
                        "access_token": expected_token
                    }))
            )
            .mount(&server)
            .await;

        let mut client = make_client(&server).await;
        client.login().await.unwrap();

        // auth_header() should now work without panicking
        let header = client.auth_header();
        assert_eq!(header, format!("Bearer {}", expected_token));
    }

    // ==========================================================
    // SECTION 3: upload_program() tests
    // Copy this section to test upload_program() in isolation.
    // ==========================================================

    #[tokio::test]
    async fn test_upload_program_success() {
        let server = MockServer::start().await;

        Mock::given(method("POST"))
            .and(path("/upload-file"))
            .respond_with(
                ResponseTemplate::new(200)
                    .set_body_json(serde_json::json!({
                        "UploadFileFail": "",
                        "CompilationStatus": "COMPILING"
                    }))
            )
            .mount(&server)
            .await;

        let client = make_authed_client(&server).await;
        // Fake zip bytes — just needs to be non-empty bytes
        let fake_zip = vec![0u8; 100];
        let result = client.upload_program(fake_zip).await;

        assert!(result.is_ok(), "upload should succeed when UploadFileFail is empty");
    }

    #[tokio::test]
    async fn test_upload_program_rejected() {
        // OpenPLC rejected the file — UploadFileFail is non-empty
        let server = MockServer::start().await;

        Mock::given(method("POST"))
            .and(path("/upload-file"))
            .respond_with(
                ResponseTemplate::new(200)
                    .set_body_json(serde_json::json!({
                        "UploadFileFail": "Invalid zip structure",
                        "CompilationStatus": "IDLE"
                    }))
            )
            .mount(&server)
            .await;

        let client = make_authed_client(&server).await;
        let fake_zip = vec![0u8; 100];
        let result = client.upload_program(fake_zip).await;

        assert!(result.is_err(), "non-empty UploadFileFail should be an error");
        let err_msg = result.unwrap_err().to_string();
        assert!(
            err_msg.contains("Invalid zip structure"),
            "error message should contain the rejection reason"
        );
    }

    #[tokio::test]
    async fn test_upload_program_server_error() {
        let server = MockServer::start().await;

        Mock::given(method("POST"))
            .and(path("/upload-file"))
            .respond_with(ResponseTemplate::new(500))
            .mount(&server)
            .await;

        let client = make_authed_client(&server).await;
        let fake_zip = vec![0u8; 100];
        let result = client.upload_program(fake_zip).await;

        assert!(result.is_err(), "500 should be an error");
    }

    // ==========================================================
    // SECTION 4: wait_for_compilation() tests
    // Copy this section to test wait_for_compilation() in isolation.
    // ==========================================================

    #[tokio::test]
    async fn test_compilation_succeeds_immediately() {
        let server = MockServer::start().await;

        Mock::given(method("GET"))
            .and(path("/compilation-status"))
            .respond_with(
                ResponseTemplate::new(200)
                    .set_body_json(serde_json::json!({
                        "status": "SUCCESS",
                        "logs": ["[INFO] Build succeeded"],
                        "exit_code": 0
                    }))
            )
            .mount(&server)
            .await;

        let client = make_authed_client(&server).await;
        let result = client.wait_for_compilation(
            std::time::Duration::from_millis(100),
            std::time::Duration::from_secs(5),
        ).await;

        assert!(result.is_ok(), "SUCCESS status should return Ok");
    }

    #[tokio::test]
    async fn test_compilation_fails() {
        let server = MockServer::start().await;

        Mock::given(method("GET"))
            .and(path("/compilation-status"))
            .respond_with(
                ResponseTemplate::new(200)
                    .set_body_json(serde_json::json!({
                        "status": "FAILED",
                        "logs": ["[ERROR] undefined reference to main"],
                        "exit_code": 1
                    }))
            )
            .mount(&server)
            .await;

        let client = make_authed_client(&server).await;
        let result = client.wait_for_compilation(
            std::time::Duration::from_millis(100),
            std::time::Duration::from_secs(5),
        ).await;

        assert!(result.is_err(), "FAILED status should return Err");
        let err_msg = result.unwrap_err().to_string();
        assert!(
            err_msg.contains("undefined reference to main"),
            "error should contain the build log"
        );
    }

    #[tokio::test]
    async fn test_compilation_timeout() {
        // Server always returns COMPILING — should eventually time out
        let server = MockServer::start().await;

        Mock::given(method("GET"))
            .and(path("/compilation-status"))
            .respond_with(
                ResponseTemplate::new(200)
                    .set_body_json(serde_json::json!({
                        "status": "COMPILING",
                        "logs": null,
                        "exit_code": null
                    }))
            )
            .mount(&server)
            .await;

        let client = make_authed_client(&server).await;
        let result = client.wait_for_compilation(
            std::time::Duration::from_millis(50),  // poll fast
            std::time::Duration::from_millis(200), // timeout fast
        ).await;

        assert!(result.is_err(), "should time out when always COMPILING");
        let err_msg = result.unwrap_err().to_string();
        assert!(
            err_msg.contains("timed out"),
            "error should mention timeout"
        );
    }

    #[tokio::test]
    async fn test_compilation_polls_until_success() {
        // First two calls return COMPILING, third returns SUCCESS
        // Tests that the polling loop actually loops
        let server = MockServer::start().await;

        Mock::given(method("GET"))
            .and(path("/compilation-status"))
            .respond_with(
                ResponseTemplate::new(200)
                    .set_body_json(serde_json::json!({
                        "status": "COMPILING",
                        "logs": null,
                        "exit_code": null
                    }))
            )
            .up_to_n_times(2) // first two calls return COMPILING
            .mount(&server)
            .await;

        Mock::given(method("GET"))
            .and(path("/compilation-status"))
            .respond_with(
                ResponseTemplate::new(200)
                    .set_body_json(serde_json::json!({
                        "status": "SUCCESS",
                        "logs": ["done"],
                        "exit_code": 0
                    }))
            )
            .mount(&server)
            .await;

        let client = make_authed_client(&server).await;
        let result = client.wait_for_compilation(
            std::time::Duration::from_millis(50),
            std::time::Duration::from_secs(5),
        ).await;

        assert!(result.is_ok(), "should succeed after polling through COMPILING states");
    }

    // ==========================================================
    // SECTION 5: start_plc() tests
    // Copy this section to test start_plc() in isolation.
    // ==========================================================

    #[tokio::test]
    async fn test_start_plc_success() {
        let server = MockServer::start().await;

        Mock::given(method("GET"))
            .and(path("/start-plc"))
            .respond_with(
                ResponseTemplate::new(200)
                    .set_body_json(serde_json::json!({ "status": "Running" }))
            )
            .mount(&server)
            .await;

        let client = make_authed_client(&server).await;
        let result = client.start_plc().await;

        assert!(result.is_ok(), "Running response should be Ok");
    }

    #[tokio::test]
    async fn test_start_plc_no_program() {
        let server = MockServer::start().await;

        Mock::given(method("GET"))
            .and(path("/start-plc"))
            .respond_with(
                ResponseTemplate::new(200)
                    .set_body_json(serde_json::json!({ "status": "No PLC program found" }))
            )
            .mount(&server)
            .await;

        let client = make_authed_client(&server).await;
        let result = client.start_plc().await;

        assert!(result.is_err(), "No PLC program should be an error");
    }

    #[tokio::test]
    async fn test_start_plc_no_runtime_response() {
        let server = MockServer::start().await;

        Mock::given(method("GET"))
            .and(path("/start-plc"))
            .respond_with(
                ResponseTemplate::new(200)
                    .set_body_json(serde_json::json!({ "status": "No response from runtime" }))
            )
            .mount(&server)
            .await;

        let client = make_authed_client(&server).await;
        let result = client.start_plc().await;

        assert!(result.is_err(), "No response from runtime should be an error");
    }

    // ==========================================================
    // SECTION 6: get_status() tests
    // Copy this section to test get_status() in isolation.
    // ==========================================================

    #[tokio::test]
    async fn test_get_status_running() {
        let server = MockServer::start().await;

        Mock::given(method("GET"))
            .and(path("/status"))
            .respond_with(
                ResponseTemplate::new(200)
                    .set_body_json(serde_json::json!({ "status": "RUNNING" }))
            )
            .mount(&server)
            .await;

        let client = make_authed_client(&server).await;
        let result = client.get_status().await;

        assert!(result.is_ok());
        assert_eq!(result.unwrap(), PlcState::Running);
    }

    #[tokio::test]
    async fn test_get_status_stopped() {
        let server = MockServer::start().await;

        Mock::given(method("GET"))
            .and(path("/status"))
            .respond_with(
                ResponseTemplate::new(200)
                    .set_body_json(serde_json::json!({ "status": "STOPPED" }))
            )
            .mount(&server)
            .await;

        let client = make_authed_client(&server).await;
        let result = client.get_status().await;

        assert_eq!(result.unwrap(), PlcState::Stopped);
    }

    #[tokio::test]
    async fn test_get_status_unexpected() {
        // OpenPLC returns something we have never seen before
        let server = MockServer::start().await;

        Mock::given(method("GET"))
            .and(path("/status"))
            .respond_with(
                ResponseTemplate::new(200)
                    .set_body_json(serde_json::json!({ "status": "SOME_NEW_STATE" }))
            )
            .mount(&server)
            .await;

        let client = make_authed_client(&server).await;
        let result = client.get_status().await;

        assert!(result.is_ok(), "unexpected state should not error — just wrap it");
        assert_eq!(
            result.unwrap(),
            PlcState::Unexpected("SOME_NEW_STATE".to_string())
        );
    }

    // ==========================================================
    // SECTION 7: PlcState conversion tests
    // These do not need a mock server — pure unit tests.
    // Copy this section at any time to verify From<String>.
    // ==========================================================

    #[test]
    fn test_plc_state_from_string_all_variants() {
        assert_eq!(PlcState::from("EMPTY".to_string()),   PlcState::Empty);
        assert_eq!(PlcState::from("INIT".to_string()),    PlcState::Init);
        assert_eq!(PlcState::from("RUNNING".to_string()), PlcState::Running);
        assert_eq!(PlcState::from("STOPPED".to_string()), PlcState::Stopped);
        assert_eq!(PlcState::from("ERROR".to_string()),   PlcState::Error);
    }

    #[test]
    fn test_plc_state_unexpected_captures_string() {
        let state = PlcState::from("SOME_WEIRD_STATE".to_string());
        assert_eq!(state, PlcState::Unexpected("SOME_WEIRD_STATE".to_string()));
    }

    // ==========================================================
    // SECTION 8: helper method tests
    // Pure unit tests — no mock server needed.
    // ==========================================================

    #[test]
    fn test_url_concatenation() {
        let client = PlcClient::new(
            "https://localhost:8443/api",
            "admin",
            "secret"
        ).unwrap();

        assert_eq!(
            client.url("/login"),
            "https://localhost:8443/api/login"
        );
        assert_eq!(
            client.url("/start-plc"),
            "https://localhost:8443/api/start-plc"
        );
    }

    #[test]
    fn test_auth_header_format() {
        let mut client = PlcClient::new(
            "https://localhost:8443/api",
            "admin",
            "secret"
        ).unwrap();

        client.jwt_token = Some("mytoken123".to_string());
        assert_eq!(client.auth_header(), "Bearer mytoken123");
    }

    #[test]
    #[should_panic(expected = "JWT token not set")]
    fn test_auth_header_panics_before_login() {
        let client = PlcClient::new(
            "https://localhost:8443/api",
            "admin",
            "secret"
        ).unwrap();

        // This should panic because jwt_token is None
        let _ = client.auth_header();
    }
}