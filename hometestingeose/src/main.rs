mod config;
mod client;

fn main() {
    let config = config::Config::from_env();

    println!("PLC URL: {}", config.plc_url);
    println!("Username: {}", config.plc_username);
    println!("Program zip path: {}", config.program_zip_path);
    println!("Node ID: {}", config.node_id);
    println!("Startup delay: {}s", config.plc_startup_delay_secs);
    println!("Password is set: {}", !config.plc_password.is_empty());
}

//REMEMBER TO MODIFY plugins.conf THROUGH THE DOCKERFILE