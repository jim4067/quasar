use clap::Parser;

fn main() {
    let globals = quasar_cli::config::GlobalConfig::load();
    quasar_cli::style::init(globals.ui.color);

    // Intercept top-level help before clap — lets subcommand --help work normally
    let args: Vec<String> = std::env::args().collect();
    if args.len() == 1 || (args.len() == 2 && matches!(args[1].as_str(), "--help" | "-h" | "help"))
    {
        quasar_cli::print_help();
        return;
    }

    let cli = quasar_cli::Cli::parse();
    if let Err(e) = quasar_cli::run(cli) {
        eprintln!("\n  {} {e}", quasar_cli::style::fail(""));
        std::process::exit(1);
    }
}
