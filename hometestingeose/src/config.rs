//the point of this file is to read env variables to put into the Config struct
//lets say something is not present, then the program should exit immediately 
//i put it here in its own file to keep it modifiable when i eventually implement the heartbeat

pub struct Config {
    pub plc_url: String,
    pub plc_username: String,
    pub plc_password: String,
    pub program_zip_path: String,
    pub node_id: String,
    pub plc_startup_delay_secs: u64, //the PLC runtime needs time to initialize so i think around 8~10 seconds sohuld be fine?
}

impl Config {
    pub fn from_env() -> Self { //ideally this should run without a problem if i provided the correct environment variables
        Self {
            plc_url: std::env::var("PLC_URL")
                .unwrap_or_else(|_| "https://localhost:8443/api".to_string()),

            plc_username: std::env::var("PLC_USERNAME")
                .unwrap_or_else(|_| "admin".to_string()),

            plc_password: std::env::var("PLC_PASSWORD")
                .expect("PLC_PASSWORD must be set"), //since password for the runtime server in the database doesn't have a default, i have to make this panic if not provided within the environment variables 

            program_zip_path: std::env::var("PROGRAM_ZIP")
                .unwrap_or_else(|_| "/app/program.zip".to_string()),

            node_id: std::env::var("NODE_ID")
                .unwrap_or_else(|_| "plc-node-1".to_string()), 

            plc_startup_delay_secs: std::env::var("PLC_STARTUP_DELAY_SECS")
                .unwrap_or_else(|_| "10".to_string())
                .parse::<u64>()
                .expect("PLC_STARTUP_DELAY_SECS must be a valid number"), //the type is a u64 so i have to convert from a string to a u64 using parse()
        }
    }
}