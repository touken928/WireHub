mod api;
mod hub_key;
mod kernel;
mod model;
mod network;
mod startup;
mod static_assets;
mod storage;

#[derive(Debug, PartialEq, Eq)]
enum Command {
    Serve,
    Openapi,
    Version,
    Help,
}
fn command(arguments: &[String]) -> Result<Command, &'static str> {
    match arguments {
        [] => Ok(Command::Serve),
        [value] => match value.as_str() {
            "serve" => Ok(Command::Serve),
            "export-openapi" => Ok(Command::Openapi),
            "--version" | "-V" | "version" => Ok(Command::Version),
            "--help" | "-h" | "help" => Ok(Command::Help),
            _ => Err("Unknown command. Use wirehub --help."),
        },
        _ => Err("Expected one command. Use wirehub --help."),
    }
}

#[tokio::main]
async fn main() -> startup::Result<()> {
    match command(&std::env::args().skip(1).collect::<Vec<_>>())? {
        Command::Serve => startup::run().await,
        Command::Openapi => {
            print!("{}", api::openapi());
            Ok(())
        }
        Command::Version => {
            println!(
                "wirehub {} ({})",
                env!("CARGO_PKG_VERSION"),
                env!("WIREHUB_BUILD_ID")
            );
            Ok(())
        }
        Command::Help => {
            println!("WireHub {}\nUsage: wirehub [serve | export-openapi | --version | --help]\n\nServe uses WIREHUB_ADMIN_TOKEN (required), WIREHUB_PORT (51820),\nWIREHUB_HTTP_BIND (127.0.0.1), WIREHUB_DB (wirehub.sqlite3),\nWIREHUB_HUB_KEY (wirehub.key), WIREHUB_TRUSTED_PROXY_MODE (0).\nNon-loopback HTTP requires a trusted TLS proxy and proxy mode 1.\nVersion/help/OpenAPI export never open the database or hub key.\nSIGINT/SIGTERM drain HTTP for up to 8 seconds before stopping UDP.", env!("CARGO_PKG_VERSION"));
            Ok(())
        }
    }
}

#[cfg(test)]
mod command_tests {
    use super::*;
    #[test]
    fn commands_are_explicit_and_unknown_or_multiple_arguments_are_rejected() {
        assert_eq!(command(&[]), Ok(Command::Serve));
        for (value, expected) in [
            ("serve", Command::Serve),
            ("--help", Command::Help),
            ("--version", Command::Version),
            ("export-openapi", Command::Openapi),
        ] {
            assert_eq!(command(&[value.into()]), Ok(expected));
        }
        assert!(command(&["export-openapi".into(), "serve".into()]).is_err());
        assert!(command(&["--unknown".into()]).is_err());
    }
}
