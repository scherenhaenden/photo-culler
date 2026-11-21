use photo_culler_server::demo_args::{HELP, ParseOutcome, parse};

#[tokio::main]
async fn main() {
    match parse(std::env::args().skip(1)) {
        Ok(ParseOutcome::Help) => println!("{HELP}"),
        Ok(ParseOutcome::Run(args)) => {
            if let Err(error) = photo_culler_server::demo::runtime::run(args).await {
                eprintln!("photo-culler-server: {error}");
                std::process::exit(2);
            }
        }
        Err(error) => {
            eprintln!("{error}\n\n{HELP}");
            std::process::exit(2);
        }
    }
}
