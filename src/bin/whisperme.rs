use std::process::Command;
use whisperme::socket::send_command;

fn main() {
    let args: Vec<String> = std::env::args().collect();

    if args.len() < 2 {
        eprintln!("Usage: whisperme <start|stop|toggle|status|config>");
        std::process::exit(1);
    }

    let command = &args[1];

    match command.as_str() {
        "start" | "stop" | "toggle" => {
            send_command(command);
        }
        "status" => {
            if let Some(response) = send_command(command) {
                println!("{response}");
            }
        }
        "config" => {
            // Launch the configuration GUI
            let exe_path = std::env::current_exe().ok();
            let config_exe = exe_path
                .as_ref()
                .and_then(|p| p.parent())
                .map(|dir| dir.join("whisperme-config"));
            
            let result = if let Some(path) = config_exe.filter(|p| p.exists()) {
                Command::new(path).spawn()
            } else {
                // Try finding in PATH
                Command::new("whisperme-config").spawn()
            };
            
            match result {
                Ok(_) => {}
                Err(e) => {
                    eprintln!("Failed to launch configuration dialog: {e}");
                    eprintln!("Make sure whisperme-config is installed.");
                    std::process::exit(1);
                }
            }
        }
        _ => {
            eprintln!("Unknown command: {command}");
            eprintln!("Usage: whisperme <start|stop|toggle|status|config>");
            std::process::exit(1);
        }
    }
}
