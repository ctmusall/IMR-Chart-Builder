//! `imr-chart-cli`: make I-MR charts from the command line.

fn main() -> std::process::ExitCode {
    imr_cli::main_with(std::env::args_os(), None)
}
