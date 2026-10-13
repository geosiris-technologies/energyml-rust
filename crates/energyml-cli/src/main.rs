//! The `energyml` binary: everything is in the library, shared with `enyml`.

fn main() -> std::process::ExitCode {
    energyml_cli::run()
}
