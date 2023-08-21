use std::{time::{Duration, Instant}, thread::{self, JoinHandle}, fs::{OpenOptions, File}, sync::mpsc::{Sender, Receiver}};
use std::io::prelude::*;
use chrono::Local;
use config::Config;
use std::sync::mpsc::channel;
use ctrlc;

const CONFIG_FILE: &str = "settings";

fn main() {
    let ctrlc_rx = set_ctrc_handler();
    let config = load_config();
    let targets_data = load_targets(&config);

    ctrlc_rx.recv().expect("Could not receive from channel.");

    unload_targets(targets_data);
}

fn set_ctrc_handler() -> Receiver<()> {
    let (tx, rx) = channel::<()>();
    ctrlc::set_handler(move || tx.send(()).expect("Could not send signal on channel."))
        .expect("Error setting Ctrl-C handler");
    rx
}

fn load_config() -> Config {
    Config::builder()
        .add_source(config::File::with_name(CONFIG_FILE))
        .build()
        .expect(&format!("Error reading settings file \"{CONFIG_FILE}\""))
}

fn load_targets(config: &Config) -> Vec<(Sender<()>, JoinHandle<()>)> {
    let mut targets_data = Vec::new();
    for target in config.get_array("targets").expect("targets not found in settings file") {
        let target_config = target.into_table().expect("target is not a table");
        let logname = target_config.get("logname").expect("logname not found in target").clone().into_string().expect("logname is not a valid string");
        let ip = target_config.get("ip").expect("ip not found in target").clone().into_string().expect("ip is not a valid string");
        targets_data.push(init_target(&logname, &ip))
    };
    targets_data
}

fn init_target(logname: &str, ip: &str) -> (Sender<()>, JoinHandle<()>) {
    let addr = ip.parse().expect("Unparsable ip address");
    let data = [1,2,3,4];  // ping data
    let timeout = Duration::from_secs(1);
    let options = ping_rs::PingOptions { ttl: 128, dont_fragment: true };
    let ip: String = ip.to_string();
    let logname = logname.to_string();
    let (tx, rx) = channel::<()>();

    let join_handle = thread::spawn(move || {
        let mut file = OpenOptions::new()
            .write(true)
            .append(true)
            .create(true)
            .open(logname.clone())
            .expect(&format!("Unable to open log file {logname}"));
        write_log(&mut file, &format!("Starting ping monitor to {ip} as offline status"));

        let mut status = false;
        let mut start = Instant::now();
        loop {
            let result = ping_rs::send_ping(&addr, timeout, &data, Some(&options));
            match result {
                Ok(_) => {
                    if !status {
                        let elapsed = start.elapsed();
                        status = true;
                        start = Instant::now();

                        let sec = elapsed.as_secs();
                        write_log(&mut file, &format!("Exited offline status, elapsed {sec} seconds"));
                        write_log(&mut file, "Entering online status");
                    }
                },
                Err(_) => {
                    if status {
                        let elapsed = start.elapsed();
                        status = false;
                        start = Instant::now();

                        let sec = elapsed.as_secs();
                        write_log(&mut file, &format!("Exited online status, elapsed {sec} seconds"));
                        write_log(&mut file, "Entering offline status");
                    }
                }
            }

            if rx.recv_timeout(Duration::from_secs(1)).is_ok() {
                break;
            }
        }

        write_log(&mut file, "Ping monitor terminated");
    });

    (tx, join_handle)
}

fn write_log(file: &mut File, message: &str) {
    let timestamp = Local::now().format("%Y-%m-%d %H:%M:%S.%3f").to_string();
    if let Err(error) = writeln!(file, "[{timestamp}] {message}") {
        println!("[{timestamp}] write_log error: {error}");
    }
}

fn unload_targets(targets: Vec<(Sender<()>, JoinHandle<()>)>) {
    for (tx, _) in &targets {
        if let Err(error) = tx.send(()) {
            println!("Error shutting down target thread: {error}");
        }
    }
    for (_, join_handle) in targets {
        if join_handle.join().is_err() {
            println!("Error joining target thread");
        }
    }
}