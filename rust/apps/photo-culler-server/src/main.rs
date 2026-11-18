use photo_culler_server::{
    RouterNotIntegrated,
    demo_args::{HELP, ParseOutcome, parse},
};

fn main() {
    match parse(std::env::args().skip(1)) {
        Ok(ParseOutcome::Help) => println!("{HELP}"),
        Ok(ParseOutcome::Run(_args)) => {
            eprintln!("{RouterNotIntegrated}");
            std::process::exit(2);
        }
        Err(error) => {
            eprintln!("{error}\n\n{HELP}");
            std::process::exit(2);
        }
    }
}
