//! `clevo-cc` entry point.

use std::process::ExitCode;

use clap::Parser;

use clevo_cc_cli::commands;
use clevo_cc_cli::{build_transport, Cli, Commands, TransportChoice};

fn main() -> ExitCode {
    let cli = Cli::parse();

    let result = if cli.transport == TransportChoice::Dbus {
        run_dbus(&cli)
    } else {
        match build_transport(&cli) {
            Ok(transport) => {
                let stdout = std::io::stdout();
                let mut out = stdout.lock();
                run(&cli, transport.as_ref(), &mut out)
            }
            Err(err) => Err(err),
        }
    };

    match result {
        Ok(()) => ExitCode::SUCCESS,
        Err(err) => {
            eprintln!("error: {err}");
            ExitCode::FAILURE
        }
    }
}

fn run(
    cli: &Cli,
    transport: &dyn clevo_transport::Transport,
    out: &mut dyn std::io::Write,
) -> Result<(), clevo_cc_cli::CliError> {
    match &cli.command {
        Commands::Doctor => commands::run_doctor(transport, out),
        Commands::Capabilities => {
            let caps = commands::read_capabilities(transport)?;
            commands::print_capabilities(&caps, out)?;
            Ok(())
        }
        Commands::Fan(fan) => commands::run_fan(transport, fan, out),
        Commands::Profile(profile) => commands::run_profile(transport, profile, out),
    }
}

fn run_dbus(cli: &Cli) -> Result<(), clevo_cc_cli::CliError> {
    let client = clevo_cc_cli::dbus::DbusClient::connect(cli.dbus_session)?;
    let watch = match &cli.command {
        Commands::Fan(clevo_cc_cli::FanCommand::Watch {
            interval_ms,
            count,
            json,
        }) => Some((*interval_ms, *count, *json)),
        _ => None,
    };
    let stdout = std::io::stdout();
    let mut out = stdout.lock();
    commands::run_dbus(&client, &cli.command, watch, &mut out)
}
