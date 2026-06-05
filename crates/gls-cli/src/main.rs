use gls_cli::run_dev_smoke;
use gls_report::{render_html, render_markdown};

fn main() {
    let command = std::env::args()
        .nth(1)
        .unwrap_or_else(|| "dev-smoke".to_string());
    let result = match command.as_str() {
        "dev-smoke" => run_dev_smoke().map(|report| render_markdown(&report)),
        "dev-smoke-html" => run_dev_smoke().map(|report| render_html(&report)),
        _ => Err(format!("unknown_command:{command}")),
    };
    match result {
        Ok(output) => println!("{output}"),
        Err(reason) => {
            eprintln!("{reason}");
            std::process::exit(1);
        }
    }
}
