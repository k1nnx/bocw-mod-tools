mod auth;
mod lobby;
mod stun;
mod umbrella;
mod crypto;
mod services;
mod config;

use anyhow::Result;
use colored::Colorize;
use t9_common::dw_protocol::{LocalService, Transport};
use tracing::{info, error};

#[tokio::main]
async fn main() -> Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "t9_server=info".into()),
        )
        .init();

    let config = config::ServerConfig::load_or_default();
    anyhow::ensure!(
        config.bind_addr == "127.0.0.1",
        "This experimental server is limited to localhost (127.0.0.1); refusing bind_addr={}",
        config.bind_addr
    );

    print_banner(&config);
    print_limitations();

    info!("Starting all services...");

    let auth_handle = tokio::spawn({
        let cfg = config.clone();
        async move {
            if let Err(e) = auth::run_login_service(cfg).await {
                error!("Login service error: {}", e);
            }
        }
    });

    let lobby_handle = tokio::spawn({
        let cfg = config.clone();
        async move {
            if let Err(e) = lobby::run_lobby_server(cfg).await {
                error!("Lobby server error: {}", e);
            }
        }
    });

    let stun_handle = tokio::spawn(async move {
        if let Err(e) = stun::run_stun_server().await {
            error!("STUN server error: {}", e);
        }
    });

    let umbrella_handle = tokio::spawn(async move {
        if let Err(e) = umbrella::run_umbrella_server().await {
            error!("Umbrella server error: {}", e);
        }
    });

    // Brief pause to let services bind, then print status
    tokio::time::sleep(std::time::Duration::from_millis(200)).await;
    print_service_status();

    println!(
        "\n  {}",
        "Server is running. Press Ctrl+C to stop.".dimmed()
    );
    println!(
        "  {}",
        "Start the game with the proxy DLL installed to connect.\n"
            .dimmed()
    );

    tokio::select! {
        r = auth_handle => { error!("Login service exited: {:?}", r); }
        r = lobby_handle => { error!("Lobby server exited: {:?}", r); }
        r = stun_handle => { error!("STUN server exited: {:?}", r); }
        r = umbrella_handle => { error!("Umbrella server exited: {:?}", r); }
    }

    Ok(())
}

fn print_banner(config: &config::ServerConfig) {
    println!();
    println!(
        "{}",
        "╔══════════════════════════════════════════════════════════╗"
            .cyan()
            .bold()
    );
    println!(
        "{}",
        "║                                                          ║"
            .cyan()
            .bold()
    );
    println!(
        "{}  {}  {}",
        "║".cyan().bold(),
        "T9 DemonWare Emulator Server".bright_cyan().bold(),
        "                     ║".cyan().bold()
    );
    println!(
        "{}  {}  {}",
        "║".cyan().bold(),
        "Black Ops Cold War — Offline Mode".green(),
        "                ║".cyan().bold()
    );
    println!(
        "{}",
        "║                                                          ║"
            .cyan()
            .bold()
    );
    println!(
        "{}",
        "╠══════════════════════════════════════════════════════════╣"
            .cyan()
            .bold()
    );
    println!(
        "{}",
        "║                                                          ║"
            .cyan()
            .bold()
    );
    println!(
        "{}  {}  {}{}",
        "║".cyan().bold(),
        "Player:".yellow(),
        config.username.bright_white().bold(),
        " ".repeat(42 - config.username.len().min(41))
            .cyan()
            .bold()
    );
    println!(
        "{}  {}  {}{}",
        "║".cyan().bold(),
        "UserID:".yellow(),
        format!("{}", config.user_id).bright_white(),
        "                            ║".cyan().bold()
    );
    println!(
        "{}  {}  {}{}",
        "║".cyan().bold(),
        "Bind:  ".yellow(),
        config.bind_addr.bright_white(),
        " ".repeat(42 - config.bind_addr.len().min(41))
            .cyan()
            .bold()
    );
    println!(
        "{}",
        "║                                                          ║"
            .cyan()
            .bold()
    );
    println!(
        "{}",
        "╚══════════════════════════════════════════════════════════╝"
            .cyan()
            .bold()
    );
    println!();
}

fn print_limitations() {
    println!(
        "  {}",
        "── Prototype limitations ──".yellow().bold()
    );

    println!(
        "  {} Services are configured to listen only on 127.0.0.1",
        "[local]".green().bold()
    );
    println!(
        "  {} Proxy interception and game authentication are not verified by this server",
        "[unverified]".yellow().bold()
    );
    println!(
        "  {} Does not establish that no game traffic reaches official servers",
        "[unverified]".yellow().bold()
    );
    println!(
        "\n  {} {}",
        "WARNING:".red().bold(),
        "Remove all modifications and verify files with Steam before official play."
            .yellow()
            .bold()
    );
    println!();
}

fn print_service_status() {
    println!(
        "  {}",
        "── Services ──".cyan().bold()
    );

    for service in LocalService::ALL {
        let (name, port) = (service.name(), service.port());
        let (proto, reachable) = match service.transport() {
            Transport::Udp => ("UDP", None), // A TCP probe cannot verify a UDP listener.
            Transport::Tcp => (
                "TCP",
                Some(
                    std::net::TcpStream::connect_timeout(
                        &format!("127.0.0.1:{}", port).parse().unwrap(),
                        std::time::Duration::from_millis(100),
                    )
                    .is_ok(),
                ),
            ),
        };

        if reachable == Some(true) {
            println!(
                "  {} {} {} {}",
                "●".green(),
                name.bright_white(),
                format!("({})", proto).dimmed(),
                format!(":{}", port).yellow()
            );
        } else if reachable == Some(false) {
            println!(
                "  {} {} {} {}",
                "○".red(),
                name,
                format!("({})", proto).dimmed(),
                format!(":{}", port).yellow()
            );
        } else {
            println!("  {} {} (UDP :{}, not probed)", "?".yellow(), name, port);
        }
    }

    // List registered DW services
    println!(
        "\n  {}",
        "── Prototype DW service handlers (not game-verified) ──".cyan().bold()
    );

    let dw_services = [
        (4u8, "bdStats"),
        (8, "bdProfiles"),
        (12, "bdTitleUtilities"),
        (15, "bdKeyArchive"),
        (18, "bdBandwidthTest"),
        (23, "bdCounter"),
        (27, "bdDML"),
        (28, "bdGroup"),
        (38, "bdAnticheat"),
        (52, "bdTags"),
        (58, "bdPooledStorage"),
        (67, "bdEventLog"),
        (68, "bdRichPresence"),
        (80, "bdMarketplace"),
        (95, "bdPublisherVariables"),
        (104, "bdMarketingComms"),
        (125, "bdUNK125"),
        (193, "bdObjectStore"),
        (195, "bdLootGeneration"),
    ];

    for (id, name) in &dw_services {
        let id_str = format!("{:>3}", id);
        let special = match *id {
            38 => " (unverified stub)".yellow().to_string(),
            _ => String::new(),
        };
        println!(
            "  {} {} {}{}",
            "·".dimmed(),
            id_str.dimmed(),
            name.bright_white(),
            special
        );
    }
}
